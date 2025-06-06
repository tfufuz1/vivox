// novade-system/src/compositor/backend/winit_backend.rs

use anyhow::{Result, Context as AnyhowContext};
use calloop::{LoopHandle, EventLoop, timer::{Timer, TimeoutAction}};
use smithay::{
    backend::{
        // renderer::{glow::GlowRenderer, gles::GlesError}, // Removed GlowRenderer
        winit::{self, WinitEvent, WinitEventLoop, WinitInputEvent}, // Removed WinitGraphicsBackend
    },
    reexports::wayland_server::DisplayHandle,
    utils::{Size as SmithaySize, SERIAL_COUNTER}, // Renamed Size to SmithaySize, Added SERIAL_COUNTER
};
use std::sync::Arc; // Added for window
use std::time::Duration;
use smithay::backend::input::Axis; // For Axis enum
use pollster; // For blocking on async WGPU init
use raw_window_handle::HasRawWindowHandle; // For WGPU surface creation

use crate::compositor::{
    core::state::{DesktopState, ActiveRendererType}, // Added ActiveRendererType
    renderer_interface::abstraction::FrameRenderer, // Added FrameRenderer trait
    // Import items that might be needed from main.rs, e.g., rendering logic
    // For now, we'll keep it minimal and refer to main.rs for complex parts.
};
use crate::renderer::wgpu_renderer::NovaWgpuRenderer; // Added NovaWgpuRenderer
use super::CompositorBackend; // Super refers to novade-system/src/compositor/backend/mod.rs

pub struct WinitBackend {
    event_loop_handle: LoopHandle<'static, DesktopState>,
    winit_event_loop: WinitEventLoop,
    window: Arc<winit::window::Window>, // Store the Winit window
    // backend: WinitGraphicsBackend<GlowRenderer>, // Removed
    // renderer: GlowRenderer, // Removed
    display_handle: DisplayHandle,
}

impl CompositorBackend for WinitBackend {
    fn init(
        event_loop_handle: LoopHandle<'static, DesktopState>,
        display_handle: DisplayHandle,
        desktop_state: &mut DesktopState, // desktop_state is used for setup
    ) -> Result<Self>
    where
        Self: Sized,
    {
        tracing::info!("Initializing Winit backend with WGPU...");

        let mut event_loop_builder = winit::WinitEventLoopBuilder::new()
            .with_title("NovaDE Compositor (Winit + WGPU)")
            .with_explicit_event_loop_handle(event_loop_handle.clone());

        let winit_event_loop = event_loop_builder.build().map_err(|e| anyhow::anyhow!("Failed to build winit event loop: {}",e))?;

        let window_builder = winit_event_loop.create_window_builder().with_title("NovaDE (WGPU)");
        let window = Arc::new(window_builder.build().map_err(|e| anyhow::anyhow!("Failed to build winit window: {}",e))?);

        tracing::info!("Winit event loop and window created for WGPU backend.");

        let initial_size_physical = window.inner_size();
        let wgpu_renderer = pollster::block_on(NovaWgpuRenderer::new(
            window.as_ref(),
            SmithaySize::from((initial_size_physical.width, initial_size_physical.height))
        )).context("Failed to initialize NovaWgpuRenderer for Winit backend")?;

        let renderer_arc = Arc::new(Mutex::new(wgpu_renderer));
        desktop_state.active_renderer = Some(renderer_arc.clone() as Arc<Mutex<dyn FrameRenderer>>);
        desktop_state.wgpu_renderer_concrete = Some(renderer_arc); // Store concrete type for commit path
        desktop_state.active_renderer_type = ActiveRendererType::Wgpu;
        tracing::info!("NovaWgpuRenderer initialized and stored in DesktopState.");

        Ok(WinitBackend {
            event_loop_handle,
            winit_event_loop,
            window,
            display_handle,
        })
    }

    fn run(mut self, _desktop_state_arg_for_timer: &mut DesktopState) -> Result<()> { // Renamed to avoid conflict with `state` in closure
        tracing::info!("Running Winit backend event loop (WGPU)...");

        let winit_timer = Timer::immediate();
        // self.event_loop_handle needs to be 'static for insert_source. LoopHandle itself is,
        // but the data it carries (DesktopState) also needs to be managed considering this.
        // The closure for Timer::new captures `self.winit_event_loop` and `self.window`.
        // `self.display_handle` is also captured.
        self.event_loop_handle.insert_source(winit_timer, move |_, _, state: &mut DesktopState| {
            let mut calloop_timeout_action = TimeoutAction::ToDuration(Duration::from_millis(16));

            if let Err(e) = self.winit_event_loop.dispatch_new_events(|event| {
                match event {
                    WinitEvent::Resized { size, .. } => {
                        tracing::info!("Winit window resized to: {:?}", size);
                        if let Some(renderer_mutex) = state.active_renderer.as_ref() {
                            let mut renderer = renderer_mutex.lock().unwrap();
                            // Assuming renderer can be downcast or resize is on FrameRenderer trait
                            // For now, let's assume FrameRenderer needs a resize method, or we downcast.
                            // If FrameRenderer has resize:
                            // renderer.resize(SmithaySize::from((size.width, size.height)));
                            // If we need to downcast to NovaWgpuRenderer:
                            if let Some(wgpu_renderer) = (&mut *renderer as &mut dyn std::any::Any).downcast_mut::<NovaWgpuRenderer>() {
                                 wgpu_renderer.resize(SmithaySize::from((size.width, size.height)));
                            } else {
                                tracing::error!("Failed to downcast active_renderer to NovaWgpuRenderer for resize.");
                            }
                        }
                    }
                    WinitEvent::CloseRequested { .. } => {
                        tracing::info!("Winit window close requested, initiating shutdown.");
                        calloop_timeout_action = TimeoutAction::Break;
                    }
                    WinitEvent::OutputCreated { output, .. } => {
                        tracing::info!("Winit backend created an output: {}", output.name());
                        // OutputHandler logic in DesktopState should handle this
                    }
                    WinitEvent::Input(winit_event_data) => {
                        match winit_event_data {
                            WinitInputEvent::Keyboard { event } => {
                                if let Some(keyboard) = state.seat.get_keyboard() {
                                    let serial = SERIAL_COUNTER.next_serial();
                                    keyboard.input(
                                        state, // DesktopState as &mut D
                                        event.key_code(),
                                        event.state(),
                                        serial,
                                        event.time_msec(),
                                        |_, modifiers, handle| {
                                            tracing::debug!(
                                                "Winit Keyboard event: keycode {}, state {:?}, keysym {:?}, modifiers {:?}",
                                                event.key_code(), event.state(), handle.modified_sym(), modifiers
                                            );
                                            smithay::input::keyboard::FilterResult::Forward
                                        }
                                    );
                                }
                            }
                            WinitInputEvent::PointerMotion { delta, time, .. } => {
                                if let Some(pointer) = state.seat.get_pointer() {
                                    state.pointer_location = pointer.current_position() + delta;
                                    pointer.motion(
                                        state, // DesktopState as &mut D
                                        time,
                                    );
                                }
                            }
                            WinitInputEvent::PointerButton { button, state: button_state, time, .. } => {
                                if let Some(pointer) = state.seat.get_pointer() {
                                    let serial = SERIAL_COUNTER.next_serial();
                                    pointer.button(
                                        state, // DesktopState as &mut D
                                        button,
                                        button_state,
                                        serial,
                                        time,
                                    );
                                }
                            }
                            WinitInputEvent::PointerAxis { source, horizontal, vertical, time, .. } => {
                                if let Some(pointer) = state.seat.get_pointer() {
                                    let mut axis_frame = smithay::input::pointer::AxisFrame::new(time)
                                        .source(source);
                                    if let Some((discrete_x, discrete_y)) = vertical.discrete_pixels().or_else(|| horizontal.discrete_pixels()) {
                                        if horizontal.discrete_pixels().is_some() {
                                            axis_frame = axis_frame.discrete(Axis::Horizontal, discrete_x as i32);
                                        }
                                        if vertical.discrete_pixels().is_some() {
                                             axis_frame = axis_frame.discrete(Axis::Vertical, discrete_y as i32);
                                        }
                                    }
                                    if let Some((continuous_x, continuous_y)) = vertical.pixels().or_else(|| horizontal.pixels()) {
                                         if horizontal.pixels().is_some() {
                                            axis_frame = axis_frame.value(Axis::Horizontal, continuous_x);
                                         }
                                         if vertical.pixels().is_some() {
                                            axis_frame = axis_frame.value(Axis::Vertical, continuous_y);
                                         }
                                    }
                                    pointer.axis(state, axis_frame); // DesktopState as &mut D
                                }
                            }
                            _ => {
                                 tracing::trace!("Unhandled WinitInputEvent: {:?}", winit_event_data);
                            }
                        }
                    }
                    _ => {
                        // tracing::trace!("Other Winit event: {:?}", event);
                    }
                }
            }) {
                tracing::error!("Error dispatching winit events in winit_backend: {}", e);
                calloop_timeout_action = TimeoutAction::Break; // Exit loop on error
            }

            if calloop_timeout_action == TimeoutAction::Break {
                return TimeoutAction::Break; // Propagate break request
            }

            // --- WGPU Rendering Logic ---
            if let Some(renderer_mutex) = state.active_renderer.as_ref() {
                let mut renderer_guard = renderer_mutex.lock().unwrap();

                let mut render_elements: Vec<RenderElement<'_>> = Vec::new();
                // overall_output_damage is not used in this version of render_frame, but kept for future.
                // let mut overall_output_damage: Vec<smithay::utils::Rectangle<i32, smithay::utils::Physical>> = Vec::new();

                // Assuming one primary output for Winit, represented by the window itself.
                // We use the first output found in DesktopState.outputs which should be the one Winit created.
                // If no outputs, we can't really render.
                let output_opt = state.outputs.first();

                if let Some(output) = output_opt {
                    let window_size_physical = self.window.inner_size();
                    let output_geometry = state.space.output_geometry(output).unwrap_or_else(|| {
                        tracing::warn!("Winit output not found in space, using window inner_size as fallback geometry.");
                        smithay::utils::Rectangle::from_loc_and_size(
                            (0, 0),
                            (window_size_physical.width as i32, window_size_physical.height as i32)
                        )
                    });
                    let output_scale = output.current_scale().fractional_scale();

                    state.space.elements_for_output(output).for_each(|window_arc| {
                        if !window_arc.is_mapped() { return; }

                        let window_geometry = window_arc.geometry();
                        let window_surface_damage = window_arc.damage();

                        let surface_wl_opt = window_arc.wl_surface();
                        if surface_wl_opt.is_none() {
                            tracing::warn!("Window has no WlSurface, skipping render element creation for window id: {:?}", window_arc.id());
                            return;
                        }
                        // WlSurface does not implement Clone, but it is Arc-like internally through ResourceRc.
                        // We need a reference for RenderElement.
                        let surface_wl_ref = surface_wl_opt.as_ref().unwrap();

                        let surface_data_guard = surface_wl_ref.data_map()
                            .get::<Arc<std::sync::Mutex<crate::compositor::surface_management::SurfaceData>>>();

                        if let Some(surface_data_mutex_arc_cloned) = surface_data_guard.cloned() {
                            render_elements.push(RenderElement::WaylandSurface {
                                surface_wl: surface_wl_ref,
                                surface_data_mutex_arc: surface_data_mutex_arc_cloned,
                                geometry: window_geometry,
                                damage_surface_local: window_surface_damage,
                            });
                        } else {
                            tracing::warn!("SurfaceData not found for WlSurface {:?} during element collection.", surface_wl_ref.id());
                        }
                    });

                    match renderer_guard.render_frame(render_elements, output_geometry, output_scale) {
                        Ok(_) => {
                            if let Err(e) = renderer_guard.present_frame() {
                                tracing::error!("Error presenting frame via WGPU backend: {:?}", e);
                                calloop_timeout_action = TimeoutAction::Break;
                            }
                        }
                        Err(e) => {
                            tracing::error!("Error rendering frame via WGPU backend: {:?}", e);
                            calloop_timeout_action = TimeoutAction::Break; // Exit loop on error
                        }
                    }
                } else {
                    tracing::warn!("No output found in DesktopState.outputs for WinitBackend rendering.");
                    // Clear the surface to indicate an issue or do nothing.
                    // This might happen if OutputCreated event hasn't been processed fully by DesktopState.
                    // For now, just skip rendering if no output.
                }
            } else {
                tracing::warn!("No active_renderer found in DesktopState for WinitBackend rendering.");
            }
            // --- End WGPU Rendering Logic ---


            // --- Post-render Wayland processing ---
            state.space.damage_all_outputs(); // Request redraw for next frame

            let now_ns = state.clock.now();
            let time_for_send_frames = std::time::Duration::from_nanos(now_ns);
            state.space.send_frames(time_for_send_frames);

            if let Err(e) = self.display_handle.flush_clients() {
                tracing::warn!("Failed to flush clients in winit_backend: {}", e);
            }
            // --- End Post-render Wayland processing ---

            calloop_timeout_action
        }).context("Failed to insert Winit event timer into event loop")?;

        Ok(())
    }

    fn loop_handle(&self) -> LoopHandle<'static, DesktopState> {
        self.event_loop_handle.clone()
    }
}

// Helper trait/extension for GlowRenderer if needed for render_frame_legacy_wrapper
// This is a temporary workaround for the GlowRenderer not having this method directly.
// Ideally, the rendering logic would be more robustly integrated.
// Remove GlowRendererExt as GlowRenderer is no longer used.
/*
pub trait GlowRendererExt {
    unsafe fn render_frame_legacy_wrapper(
                                        state, // DesktopState as &mut D
                                        event.key_code(),
                                        event.state(),
                                        serial,
                                        event.time_msec(),
                                        |_, modifiers, handle| {
                                            tracing::debug!(
                                                "Winit Keyboard event: keycode {}, state {:?}, keysym {:?}, modifiers {:?}",
                                                event.key_code(), event.state(), handle.modified_sym(), modifiers
                                            );
                                            smithay::input::keyboard::FilterResult::Forward
                                        }
                                    );
                                }
                            }
                            WinitInputEvent::PointerMotion { delta, time, .. } => {
                                if let Some(pointer) = state.seat.get_pointer() {
                                    state.pointer_location = pointer.current_position() + delta;
                                    pointer.motion(
                                        state, // DesktopState as &mut D
                                        time,
                                    );
                                }
                            }
                            WinitInputEvent::PointerButton { button, state: button_state, time, .. } => {
                                if let Some(pointer) = state.seat.get_pointer() {
                                    let serial = SERIAL_COUNTER.next_serial();
                                    pointer.button(
                                        state, // DesktopState as &mut D
                                        button,
                                        button_state,
                                        serial,
                                        time,
                                    );
                                }
                            }
                            WinitInputEvent::PointerAxis { source, horizontal, vertical, time, .. } => {
                                if let Some(pointer) = state.seat.get_pointer() {
                                    let mut axis_frame = smithay::input::pointer::AxisFrame::new(time)
                                        .source(source);
                                    if let Some((discrete_x, discrete_y)) = vertical.discrete_pixels().or_else(|| horizontal.discrete_pixels()) {
                                        if horizontal.discrete_pixels().is_some() {
                                            axis_frame = axis_frame.discrete(Axis::Horizontal, discrete_x as i32);
                                        }
                                        if vertical.discrete_pixels().is_some() {
                                             axis_frame = axis_frame.discrete(Axis::Vertical, discrete_y as i32);
                                        }
                                    }
                                    if let Some((continuous_x, continuous_y)) = vertical.pixels().or_else(|| horizontal.pixels()) {
                                         if horizontal.pixels().is_some() {
                                            axis_frame = axis_frame.value(Axis::Horizontal, continuous_x);
                                         }
                                         if vertical.pixels().is_some() {
                                            axis_frame = axis_frame.value(Axis::Vertical, continuous_y);
                                         }
                                    }
                                    pointer.axis(state, axis_frame); // DesktopState as &mut D
                                }
                            }
                            _ => {
                                 tracing::trace!("Unhandled WinitInputEvent: {:?}", winit_event_data);
                            }
                        }
                    }
                    _ => {
                        // tracing::trace!("Other Winit event: {:?}", event);
                    }
                }
            }) {
                tracing::error!("Error dispatching winit events in winit_backend: {}", e);
                calloop_timeout_action = TimeoutAction::Break; // Exit loop on error
            }

            if calloop_timeout_action == TimeoutAction::Break {
                return TimeoutAction::Break; // Propagate break request
            }

            // --- WGPU Rendering Logic ---
            if let Some(renderer_mutex) = state.active_renderer.as_ref() {
                let mut renderer_guard = renderer_mutex.lock().unwrap();

                let mut render_elements: Vec<RenderElement<'_>> = Vec::new();
                // overall_output_damage is not used in this version of render_frame, but kept for future.
                // let mut overall_output_damage: Vec<smithay::utils::Rectangle<i32, smithay::utils::Physical>> = Vec::new();

                // Assuming one primary output for Winit, represented by the window itself.
                // We use the first output found in DesktopState.outputs which should be the one Winit created.
                // If no outputs, we can't really render.
                let output_opt = state.outputs.first();

                if let Some(output) = output_opt {
                    let window_size_physical = self.window.inner_size();
                    let output_geometry = state.space.output_geometry(output).unwrap_or_else(|| {
                        tracing::warn!("Winit output not found in space, using window inner_size as fallback geometry.");
                        smithay::utils::Rectangle::from_loc_and_size(
                            (0, 0),
                            (window_size_physical.width as i32, window_size_physical.height as i32)
                        )
                    });
                    let output_scale = output.current_scale().fractional_scale();

                    state.space.elements_for_output(output).for_each(|window_arc| {
                        if !window_arc.is_mapped() { return; }

                        let window_geometry = window_arc.geometry();
                        let window_surface_damage = window_arc.damage();

                        let surface_wl_opt = window_arc.wl_surface();
                        if surface_wl_opt.is_none() {
                            tracing::warn!("Window has no WlSurface, skipping render element creation for window id: {:?}", window_arc.id());
                            return;
                        }
                        // WlSurface does not implement Clone, but it is Arc-like internally through ResourceRc.
                        // We need a reference for RenderElement.
                        let surface_wl_ref = surface_wl_opt.as_ref().unwrap();

                        let surface_data_guard = surface_wl_ref.data_map()
                            .get::<Arc<std::sync::Mutex<crate::compositor::surface_management::SurfaceData>>>();

                        if let Some(surface_data_mutex_arc_cloned) = surface_data_guard.cloned() {
                            render_elements.push(RenderElement::WaylandSurface {
                                surface_wl: surface_wl_ref,
                                surface_data_mutex_arc: surface_data_mutex_arc_cloned,
                                geometry: window_geometry,
                                damage_surface_local: window_surface_damage,
                            });
                        } else {
                            tracing::warn!("SurfaceData not found for WlSurface {:?} during element collection.", surface_wl_ref.id());
                        }
                    });

                    match renderer_guard.render_frame(render_elements, output_geometry, output_scale) {
                        Ok(_) => {
                            if let Err(e) = renderer_guard.present_frame() {
                                tracing::error!("Error presenting frame via WGPU backend: {:?}", e);
                                calloop_timeout_action = TimeoutAction::Break;
                            }
                        }
                        Err(e) => {
                            tracing::error!("Error rendering frame via WGPU backend: {:?}", e);
                            calloop_timeout_action = TimeoutAction::Break; // Exit loop on error
                        }
                    }
                } else {
                    tracing::warn!("No output found in DesktopState.outputs for WinitBackend rendering.");
                    // Clear the surface to indicate an issue or do nothing.
                    // This might happen if OutputCreated event hasn't been processed fully by DesktopState.
                    // For now, just skip rendering if no output.
                }
            } else {
                tracing::warn!("No active_renderer found in DesktopState for WinitBackend rendering.");
            }
            // --- End WGPU Rendering Logic ---


            // --- Post-render Wayland processing ---
            state.space.damage_all_outputs(); // Request redraw for next frame

            let now_ns = state.clock.now();
            let time_for_send_frames = std::time::Duration::from_nanos(now_ns);
            state.space.send_frames(time_for_send_frames);

            if let Err(e) = self.display_handle.flush_clients() {
                tracing::warn!("Failed to flush clients in winit_backend: {}", e);
            }
            // --- End Post-render Wayland processing ---

            calloop_timeout_action
        }).context("Failed to insert Winit event timer into event loop")?;

        Ok(())
    }

    fn loop_handle(&self) -> LoopHandle<'static, DesktopState> {
        self.event_loop_handle.clone()
    }
}

// Helper trait/extension for GlowRenderer if needed for render_frame_legacy_wrapper
// This is a temporary workaround for the GlowRenderer not having this method directly.
// Ideally, the rendering logic would be more robustly integrated.
// Remove GlowRendererExt as GlowRenderer is no longer used.
/*
pub trait GlowRendererExt {
    unsafe fn render_frame_legacy_wrapper(
                                        state, // DesktopState as &mut D
                                        event.key_code(),
                                        event.state(),
                                        serial,
                                        event.time_msec(),
                                        |_, modifiers, handle| {
                                            tracing::debug!(
                                                "Winit Keyboard event: keycode {}, state {:?}, keysym {:?}, modifiers {:?}",
                                                event.key_code(), event.state(), handle.modified_sym(), modifiers
                                            );
                                            smithay::input::keyboard::FilterResult::Forward
                                        }
                                    );
                                }
                            }
                            WinitInputEvent::PointerMotion { delta, time, .. } => {
                                if let Some(pointer) = state.seat.get_pointer() {
                                    state.pointer_location = pointer.current_position() + delta;
                                    pointer.motion(
                                        state, // DesktopState as &mut D
                                        time,
                                    );
                                }
                            }
                            WinitInputEvent::PointerButton { button, state: button_state, time, .. } => {
                                if let Some(pointer) = state.seat.get_pointer() {
                                    let serial = SERIAL_COUNTER.next_serial();
                                    pointer.button(
                                        state, // DesktopState as &mut D
                                        button,
                                        button_state,
                                        serial,
                                        time,
                                    );
                                }
                            }
                            WinitInputEvent::PointerAxis { source, horizontal, vertical, time, .. } => {
                                if let Some(pointer) = state.seat.get_pointer() {
                                    let mut axis_frame = smithay::input::pointer::AxisFrame::new(time)
                                        .source(source);
                                    if let Some((discrete_x, discrete_y)) = vertical.discrete_pixels().or_else(|| horizontal.discrete_pixels()) {
                                        if horizontal.discrete_pixels().is_some() {
                                            axis_frame = axis_frame.discrete(Axis::Horizontal, discrete_x as i32);
                                        }
                                        if vertical.discrete_pixels().is_some() {
                                             axis_frame = axis_frame.discrete(Axis::Vertical, discrete_y as i32);
                                        }
                                    }
                                    if let Some((continuous_x, continuous_y)) = vertical.pixels().or_else(|| horizontal.pixels()) {
                                         if horizontal.pixels().is_some() {
                                            axis_frame = axis_frame.value(Axis::Horizontal, continuous_x);
                                         }
                                         if vertical.pixels().is_some() {
                                            axis_frame = axis_frame.value(Axis::Vertical, continuous_y);
                                         }
                                    }
                                    pointer.axis(state, axis_frame); // DesktopState as &mut D
                                }
                            }
                            _ => {
                                 tracing::trace!("Unhandled WinitInputEvent: {:?}", winit_event_data);
                            }
                        }
                    }
                    _ => {
                        // tracing::trace!("Other Winit event: {:?}", event);
                    }
                }
            }) {
                tracing::error!("Error dispatching winit events in winit_backend: {}", e);
                calloop_timeout_action = TimeoutAction::Break; // Exit loop on error
            }

            if calloop_timeout_action == TimeoutAction::Break {
                return TimeoutAction::Break; // Propagate break request
            }

            // --- WGPU Rendering Logic ---
            if let Some(renderer_mutex) = state.active_renderer.as_ref() {
                let mut renderer_guard = renderer_mutex.lock().unwrap();

                let mut render_elements: Vec<RenderElement<'_>> = Vec::new();
                // overall_output_damage is not used in this version of render_frame, but kept for future.
                // let mut overall_output_damage: Vec<smithay::utils::Rectangle<i32, smithay::utils::Physical>> = Vec::new();

                // Assuming one primary output for Winit, represented by the window itself.
                // We use the first output found in DesktopState.outputs which should be the one Winit created.
                // If no outputs, we can't really render.
                let output_opt = state.outputs.first();

                if let Some(output) = output_opt {
                    let window_size_physical = self.window.inner_size();
                    let output_geometry = state.space.output_geometry(output).unwrap_or_else(|| {
                        tracing::warn!("Winit output not found in space, using window inner_size as fallback geometry.");
                        smithay::utils::Rectangle::from_loc_and_size(
                            (0, 0),
                            (window_size_physical.width as i32, window_size_physical.height as i32)
                        )
                    });
                    let output_scale = output.current_scale().fractional_scale();

                    state.space.elements_for_output(output).for_each(|window_arc| {
                        if !window_arc.is_mapped() { return; }

                        let window_geometry = window_arc.geometry();
                        let window_surface_damage = window_arc.damage();

                        let surface_wl_opt = window_arc.wl_surface();
                        if surface_wl_opt.is_none() {
                            tracing::warn!("Window has no WlSurface, skipping render element creation for window id: {:?}", window_arc.id());
                            return;
                        }
                        // WlSurface does not implement Clone, but it is Arc-like internally through ResourceRc.
                        // We need a reference for RenderElement.
                        let surface_wl_ref = surface_wl_opt.as_ref().unwrap();

                        let surface_data_guard = surface_wl_ref.data_map()
                            .get::<Arc<std::sync::Mutex<crate::compositor::surface_management::SurfaceData>>>();

                        if let Some(surface_data_mutex_arc_cloned) = surface_data_guard.cloned() {
                            render_elements.push(RenderElement::WaylandSurface {
                                surface_wl: surface_wl_ref,
                                surface_data_mutex_arc: surface_data_mutex_arc_cloned,
                                geometry: window_geometry,
                                damage_surface_local: window_surface_damage,
                            });
                        } else {
                            tracing::warn!("SurfaceData not found for WlSurface {:?} during element collection.", surface_wl_ref.id());
                        }
                    });

                    match renderer_guard.render_frame(render_elements, output_geometry, output_scale) {
                        Ok(_) => {
                            if let Err(e) = renderer_guard.present_frame() {
                                tracing::error!("Error presenting frame via WGPU backend: {:?}", e);
                                calloop_timeout_action = TimeoutAction::Break;
                            }
                        }
                        Err(e) => {
                            tracing::error!("Error rendering frame via WGPU backend: {:?}", e);
                            calloop_timeout_action = TimeoutAction::Break; // Exit loop on error
                        }
                    }
                } else {
                    tracing::warn!("No output found in DesktopState.outputs for WinitBackend rendering.");
                    // Clear the surface to indicate an issue or do nothing.
                    // This might happen if OutputCreated event hasn't been processed fully by DesktopState.
                    // For now, just skip rendering if no output.
                }
            } else {
                tracing::warn!("No active_renderer found in DesktopState for WinitBackend rendering.");
            }
            // --- End WGPU Rendering Logic ---


            // --- Post-render Wayland processing ---
            state.space.damage_all_outputs(); // Request redraw for next frame

            let now_ns = state.clock.now();
            let time_for_send_frames = std::time::Duration::from_nanos(now_ns);
            state.space.send_frames(time_for_send_frames);

            if let Err(e) = self.display_handle.flush_clients() {
                tracing::warn!("Failed to flush clients in winit_backend: {}", e);
            }
            // --- End Post-render Wayland processing ---

            calloop_timeout_action
        }).context("Failed to insert Winit event timer into event loop")?;

        Ok(())
    }

    fn loop_handle(&self) -> LoopHandle<'static, DesktopState> {
        self.event_loop_handle.clone()
    }
}

// Helper trait/extension for GlowRenderer if needed for render_frame_legacy_wrapper
// This is a temporary workaround for the GlowRenderer not having this method directly.
// Ideally, the rendering logic would be more robustly integrated.
// Remove GlowRendererExt as GlowRenderer is no longer used.
/*
pub trait GlowRendererExt {
    unsafe fn render_frame_legacy_wrapper(
        elements: &[crate::compositor::renderer_interface::abstraction::RenderElement<'_,'_>], // Adjusted lifetime
        output_geometry: smithay::utils::Rectangle<i32, smithay::utils::Physical>,
        output_scale: f64,
    ) -> Result<(), smithay::backend::renderer::gles::GlesError>;
}

impl GlowRendererExt for GlowRenderer {
    unsafe fn render_frame_legacy_wrapper(
        &mut self,
        _elements: &[crate::compositor::renderer_interface::abstraction::RenderElement<'_,'_>],
        _output_geometry: smithay::utils::Rectangle<i32, smithay::utils::Physical>,
        _output_scale: f64,
    ) -> Result<(), smithay::backend::renderer::gles::GlesError> {
        // This is a stub. The actual rendering logic from novade-system/src/main.rs
        // (the part that calls render_elements, deals with textures, shaders etc.)
        // needs to be moved or adapted here.
        // For now, let's just clear the screen to indicate it's working.
        let screen_size = _output_geometry.size; // Assuming this is the size to clear
        self.clear([0.1, 0.2, 0.8, 1.0], &[smithay::utils::Rectangle::from_loc_and_size((0,0), screen_size)]);
        // tracing::info!("render_frame_legacy_wrapper called, cleared screen (STUBBED)");
        Ok(())
    }
}
*/
                        if let Err(e) = renderer_guard.present_frame() {
                            tracing::error!("Error presenting frame via WGPU backend: {:?}", e);
                            calloop_timeout_action = TimeoutAction::Break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error rendering frame via WGPU backend: {:?}", e);
                        calloop_timeout_action = TimeoutAction::Break; // Exit loop on error
                    }
                }
            } else {
                tracing::warn!("No active_renderer found in DesktopState for WinitBackend rendering.");
            }
            // --- End WGPU Rendering Logic ---


            // --- Post-render Wayland processing ---
            state.space.damage_all_outputs(); // Request redraw for next frame

            let now_ns = state.clock.now();
            let time_for_send_frames = std::time::Duration::from_nanos(now_ns);
            state.space.send_frames(time_for_send_frames);

            if let Err(e) = self.display_handle.flush_clients() {
                tracing::warn!("Failed to flush clients in winit_backend: {}", e);
            }
            // --- End Post-render Wayland processing ---

            calloop_timeout_action
        }).context("Failed to insert Winit event timer into event loop")?;

        Ok(())
    }

    fn loop_handle(&self) -> LoopHandle<'static, DesktopState> {
        self.event_loop_handle.clone()
    }
}

// Helper trait/extension for GlowRenderer if needed for render_frame_legacy_wrapper
// This is a temporary workaround for the GlowRenderer not having this method directly.
// Ideally, the rendering logic would be more robustly integrated.
// Remove GlowRendererExt as GlowRenderer is no longer used.
/*
pub trait GlowRendererExt {
    unsafe fn render_frame_legacy_wrapper(
                                        state,
                                        event.key_code(),
                                        event.state(),
                                        serial,
                                        event.time_msec(),
                                        |_, modifiers, handle| {
                                            tracing::debug!(
                                                "Winit Keyboard event: keycode {}, state {:?}, keysym {:?}, modifiers {:?}",
                                                event.key_code(), event.state(), handle.modified_sym(), modifiers
                                            );
                                            smithay::input::keyboard::FilterResult::Forward
                                        }
                                    );
                                }
                            }
                            WinitInputEvent::PointerMotion { delta, time, .. } => {
                                if let Some(pointer) = state.seat.get_pointer() {
                                    state.pointer_location = pointer.current_position() + delta;
                                    pointer.motion(
                                        state,
                                        time,
                                    );
                                }
                            }
                            WinitInputEvent::PointerButton { button, state: button_state, time, .. } => {
                                if let Some(pointer) = state.seat.get_pointer() {
                                    let serial = SERIAL_COUNTER.next_serial();
                                    pointer.button(
                                        state,
                                        button,
                                        button_state,
                                        serial,
                                        time,
                                    );
                                }
                            }
                            WinitInputEvent::PointerAxis { source, horizontal, vertical, time, .. } => {
                                if let Some(pointer) = state.seat.get_pointer() {
                                    let mut axis_frame = smithay::input::pointer::AxisFrame::new(time)
                                        .source(source);
                                    if let Some((discrete_x, discrete_y)) = vertical.discrete_pixels().or_else(|| horizontal.discrete_pixels()) {
                                        if horizontal.discrete_pixels().is_some() {
                                            axis_frame = axis_frame.discrete(Axis::Horizontal, discrete_x as i32);
                                        }
                                        if vertical.discrete_pixels().is_some() {
                                             axis_frame = axis_frame.discrete(Axis::Vertical, discrete_y as i32);
                                        }
                                    }
                                    if let Some((continuous_x, continuous_y)) = vertical.pixels().or_else(|| horizontal.pixels()) {
                                         if horizontal.pixels().is_some() {
                                            axis_frame = axis_frame.value(Axis::Horizontal, continuous_x);
                                         }
                                         if vertical.pixels().is_some() {
                                            axis_frame = axis_frame.value(Axis::Vertical, continuous_y);
                                         }
                                    }
                                    pointer.axis(state, axis_frame);
                                }
                            }
                            _ => {
                                 tracing::trace!("Unhandled WinitInputEvent: {:?}", winit_event_data);
                            }
                        }
                    }
                    _ => {
                        // tracing::trace!("Other Winit event: {:?}", event);
                    }
                }
            }) {
                tracing::error!("Error dispatching winit events in winit_backend: {}", e);
                calloop_timeout_action = TimeoutAction::Break; // Exit loop on error
            }

            if calloop_timeout_action == TimeoutAction::Break {
                return TimeoutAction::Break; // Propagate break request
            }

            // --- Rendering Logic (Simplified Placeholder) ---
            // The detailed rendering logic from main.rs needs to be integrated here.
            // This involves:
            // 1. Binding the backend (self.backend.bind())
            // 2. Getting damage (state.space.damage_for_outputs())
            // 3. Iterating outputs, collecting render elements
            // 4. Calling self.renderer.render_frame_legacy_wrapper(...)
            // 5. Submitting the frame (self.backend.submit(None))
            // 6. Sending frame callbacks (state.space.send_frames(...))
            // 7. Flushing clients (state.display_handle.flush_clients())

        elements: &[crate::compositor::renderer_interface::abstraction::RenderElement<'_,'_>], // Adjusted lifetime
        output_geometry: smithay::utils::Rectangle<i32, smithay::utils::Physical>,
        output_scale: f64,
    ) -> Result<(), smithay::backend::renderer::gles::GlesError>;
}

impl GlowRendererExt for GlowRenderer {
    unsafe fn render_frame_legacy_wrapper(
        &mut self,
        _elements: &[crate::compositor::renderer_interface::abstraction::RenderElement<'_,'_>],
        _output_geometry: smithay::utils::Rectangle<i32, smithay::utils::Physical>,
        _output_scale: f64,
    ) -> Result<(), smithay::backend::renderer::gles::GlesError> {
        // This is a stub. The actual rendering logic from novade-system/src/main.rs
        // (the part that calls render_elements, deals with textures, shaders etc.)
        // needs to be moved or adapted here.
        // For now, let's just clear the screen to indicate it's working.
        let screen_size = _output_geometry.size; // Assuming this is the size to clear
        self.clear([0.1, 0.2, 0.8, 1.0], &[smithay::utils::Rectangle::from_loc_and_size((0,0), screen_size)]);
        // tracing::info!("render_frame_legacy_wrapper called, cleared screen (STUBBED)");
        Ok(())
    }
}
*/
            let time_for_send_frames = std::time::Duration::from_nanos(now_ns);
            state.space.send_frames(time_for_send_frames);

            if let Err(e) = self.display_handle.flush_clients() {
                tracing::warn!("Failed to flush clients in winit_backend: {}", e);
            }
            // --- End Rendering Logic Placeholder ---

            calloop_timeout_action
        }).context("Failed to insert Winit event timer into event loop")?;

        // The event_loop.run() call is made by the top-level application (e.g. in main.rs)
        // after the backend is initialized. This `run` method here sets up the winit event source.
        // The actual blocking run will be on the event_loop instance itself.
        // So, this function doesn't block; it just prepares the winit source.
        // The trait name `run` might be slightly misleading if it doesn't block.
        // However, many backend `run` methods in Smithay examples *do* block.
        // Let's clarify: this `run` method completes the setup for Winit events.
        // The main `event_loop.run()` in `main.rs` will drive this.
        Ok(())
    }

    fn loop_handle(&self) -> LoopHandle<'static, DesktopState> {
        self.event_loop_handle.clone()
    }
}

// Helper trait/extension for GlowRenderer if needed for render_frame_legacy_wrapper
// This is a temporary workaround for the GlowRenderer not having this method directly.
// Ideally, the rendering logic would be more robustly integrated.
pub trait GlowRendererExt {
    unsafe fn render_frame_legacy_wrapper(
        &mut self,
        elements: &[crate::compositor::renderer_interface::abstraction::RenderElement<'_,'_>], // Adjusted lifetime
        output_geometry: smithay::utils::Rectangle<i32, smithay::utils::Physical>,
        output_scale: f64,
    ) -> Result<(), smithay::backend::renderer::gles::GlesError>;
}

impl GlowRendererExt for GlowRenderer {
    unsafe fn render_frame_legacy_wrapper(
        &mut self,
        _elements: &[crate::compositor::renderer_interface::abstraction::RenderElement<'_,'_>],
        _output_geometry: smithay::utils::Rectangle<i32, smithay::utils::Physical>,
        _output_scale: f64,
    ) -> Result<(), smithay::backend::renderer::gles::GlesError> {
        // This is a stub. The actual rendering logic from novade-system/src/main.rs
        // (the part that calls render_elements, deals with textures, shaders etc.)
        // needs to be moved or adapted here.
        // For now, let's just clear the screen to indicate it's working.
        let screen_size = _output_geometry.size; // Assuming this is the size to clear
        self.clear([0.1, 0.2, 0.8, 1.0], &[smithay::utils::Rectangle::from_loc_and_size((0,0), screen_size)]);
        // tracing::info!("render_frame_legacy_wrapper called, cleared screen (STUBBED)");
        Ok(())
    }
}
