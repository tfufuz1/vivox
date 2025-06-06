// src/input/pointer.rs
use crate::input::config::PointerConfig;
use input::event::pointer::{PointerMotionAbsoluteEvent, PointerMotionEvent};
use tracing::debug;

#[derive(Debug, Clone)]
pub struct Pointer {
    config: PointerConfig,
    // Current absolute coordinates (if applicable, might be managed elsewhere like in a seat or surface handler)
    // current_x: f64,
    // current_y: f64,

    // Pointer Constraints Stubs
    is_confined: bool,
    // active_constraint: Option<SomeConstraintRegionType>, // Replace with actual type later
}

// Conceptual types for pointer constraints stubs
// pub struct SomeConstraintRegionType { pub x: f64, pub y: f64, pub width: f64, pub height: f64 }
// pub enum SomeLifetimeType { Persistent, Oneshot }


impl Pointer {
    pub fn new(config: &PointerConfig) -> Self {
        debug!("Pointer: Initializing with config: {:?}", config);
        Self {
            config: config.clone(),
            // current_x: 0.0,
            // current_y: 0.0,
            is_confined: false,
            // active_constraint: None,
        }
    }

    // --- Pointer Constraints Stubs ---
    pub fn confine(&mut self /*, region: Option<SomeConstraintRegionType>, lifetime: Option<SomeLifetimeType> */) {
        // TODO: Implement full pointer constraint logic here, interfacing with Wayland protocol (e.g., pointer-constraints-unstable-v1).
        // This would involve receiving region details from a Wayland client request,
        // storing the constraint, and then applying it in handle_motion_event.
        // The 'lifetime' parameter would determine if it's a persistent or oneshot constraint.
        info!("Pointer: Confinement requested (stub). Region: conceptual, Lifetime: conceptual");
        self.is_confined = true; // Simplified state for now
        // self.active_constraint = region; // Store the actual region and lifetime
    }

    pub fn unconfine(&mut self) {
        // TODO: Implement unconfine logic, removing any active constraints.
        info!("Pointer: Unconfinement requested (stub).");
        self.is_confined = false;
        // self.active_constraint = None;
    }
    // --- End Pointer Constraints Stubs ---

    /// Handles relative pointer motion events.
    /// Applies simple acceleration based on the configuration.
    /// Returns the accelerated (dx, dy).
    pub fn handle_motion_event(&mut self, event: &PointerMotionEvent) -> (f64, f64) {
        let mut dx = event.dx();
        let mut dy = event.dy();
        let time = event.time(); // Milliseconds

        // TODO: If confined, adjust dx/dy based on constraint region before applying acceleration.
        // This would involve checking self.active_constraint and clamping/modifying dx, dy
        // so the pointer does not leave the confined region relative to its last position.
        if self.is_confined {
            // Simplified: Log that it's confined. Real logic would clamp dx/dy.
            debug!("Pointer: Motion event while confined (dx={}, dy={}). Clamping logic TBD.", dx, dy);
            // Example conceptual clamping (very basic, needs current position and region knowledge):
            // (dx, dy) = self.apply_confinement(self.current_x, self.current_y, dx, dy);
        }

        // TODO: Implement advanced acceleration curves here. This might involve a different config structure
        // for profiles (e.g., linear, adaptive) and more sophisticated velocity tracking.
        // The current `self.config.acceleration_factor` is a simple linear multiplier.
        let effective_accel_factor = (1.0 + self.config.acceleration_factor).max(0.01);

        let accelerated_dx = dx * effective_accel_factor;
        let accelerated_dy = dy * effective_accel_factor;

        debug!(
            "Pointer: Motion Event: time={}, raw_dx={:.2}, raw_dy={:.2}, accel_factor_cfg={:.2}, effective_multiplier={:.2}, accel_dx={:.2}, accel_dy={:.2}",
            time, event.dx(), event.dy(), self.config.acceleration_factor, effective_accel_factor, accelerated_dx, accelerated_dy
        );

        // TODO: These accelerated deltas need to be sent to the client/surface.
        // self.current_x += accelerated_dx; // Update internal conceptual position if maintained here
        // self.current_y += accelerated_dy;
        (accelerated_dx, accelerated_dy)
    }

    /// Handles absolute pointer motion events.
    /// Transforms coordinates to a virtual screen space if output dimensions are known.
    /// Returns the transformed (x, y) assuming a single virtual output space.
    pub fn handle_motion_absolute_event(
        &mut self,
        event: &PointerMotionAbsoluteEvent,
        output_width: u32,  // Width of the output screen/area for transformation
        output_height: u32, // Height of the output screen/area for transformation
    ) -> (f64, f64) {
        let time = event.time();

        // These methods transform the event's normalized absolute coordinates (0.0-1.0)
        // to the given output dimensions.
        let x = event.absolute_x_transformed(output_width);
        let y = event.absolute_y_transformed(output_height);

        // self.current_x = x; // Update internal state if needed
        // self.current_y = y;

        debug!(
            "Pointer: Absolute Motion Event: time={}, transformed_x={:.2}, transformed_y={:.2} (for output {}x{})",
            time, x, y, output_width, output_height
        );

        // TODO: This absolute position needs to be mapped to global compositor coordinates
        // and then to a specific surface, or used for cursor position directly.
        (x,y)
    }

    // Placeholder for button event handling
    pub fn handle_button_event(&mut self, event: &input::event::pointer::PointerButtonEvent) {
        let button_code = event.button(); // This is a u32 code, e.g., 0x110 for BTN_LEFT
        let state = event.button_state(); // input::event::button::ButtonState::Pressed or ::Released
        let time = event.time();

        // TODO: Button Mapping (Stub)
        // let mapped_button_code = self.config.button_mapping
        // .as_ref()
        // .and_then(|m| m.get(&button_code))
        // .copied()
        // .unwrap_or(button_code);
        // For now, using raw button_code.
        let mapped_button_code = button_code;


        debug!(
            "Pointer: Button Event: time={}, raw_button={}, mapped_button={}, state={:?}",
            time, button_code, mapped_button_code, state
        );

        // TODO: These processed button events need to be sent to the client/surface.
        // This would typically involve:
        // 1. Determining the currently focused surface.
        // 2. Translating button code to Wayland's format if necessary.
        // 3. Sending wl_pointer.button event.
        // 4. Managing click-drag state, double-click logic, etc.
    }

    pub fn handle_axis_event(&mut self, event: &input::event::pointer::PointerAxisEvent) {
        let time = event.time();
        let source = event.axis_source().unwrap_or(input::event::pointer::AxisSource::Wheel); // Default if source is unknown

        let mut vertical_value: Option<f64> = None;
        let mut horizontal_value: Option<f64> = None;
        let mut vertical_discrete: Option<f64> = None;
        let mut horizontal_discrete: Option<f64> = None;

        if event.has_axis(input::event::pointer::Axis::Vertical) {
            let raw_v = event.axis_value(input::event::pointer::Axis::Vertical).unwrap_or(0.0);
            vertical_value = Some(raw_v * self.config.scroll_factor);
            if let Some(discrete_val) = event.axis_discrete(input::event::pointer::Axis::Vertical) {
                 // Discrete usually means number of "detents" or "clicks" of a wheel.
                 // Factor might apply differently or not at all to discrete, depending on desired behavior.
                 // For now, let's apply it, but this might need refinement.
                vertical_discrete = Some(discrete_val as f64 * self.config.scroll_factor);
            }
        }

        if event.has_axis(input::event::pointer::Axis::Horizontal) {
            let raw_h = event.axis_value(input::event::pointer::Axis::Horizontal).unwrap_or(0.0);
            horizontal_value = Some(raw_h * self.config.scroll_factor);
             if let Some(discrete_val) = event.axis_discrete(input::event::pointer::Axis::Horizontal) {
                horizontal_discrete = Some(discrete_val as f64 * self.config.scroll_factor);
            }
        }

        // Natural Scrolling for continuous sources (like touchpad)
        // This logic might be better placed where the final Wayland event is constructed,
        // or be a more integral part of how scroll_factor is applied.
        // For now, just applying it to the processed values if the source is Finger.
        let (processed_v, processed_h) = if self.config.natural_scrolling && source == input::event::pointer::AxisSource::Finger {
            (vertical_value.map(|v| -v), horizontal_value.map(|h| -h))
        } else {
            (vertical_value, horizontal_value)
        };

        // Discrete values usually aren't affected by natural scrolling in the same way,
        // but if they are, the inversion should be applied there too.

        debug!(
            "Pointer: Axis Event: time={}, source={:?}, vert_raw={:.2}, horz_raw={:.2}, scroll_factor_cfg={:.2}, processed_vert={:.2?}, processed_horz={:.2?}, vert_discrete={:.2?}, horz_discrete={:.2?}",
            time,
            source,
            event.axis_value(input::event::pointer::Axis::Vertical).unwrap_or(0.0),
            event.axis_value(input::event::pointer::Axis::Horizontal).unwrap_or(0.0),
            self.config.scroll_factor,
            processed_v,
            processed_h,
            vertical_discrete,
            horizontal_discrete
        );

        // TODO: These processed scroll values need to be sent to the client/surface.
        // This would involve:
        // 1. Determining the focused surface.
        // 2. Sending wl_pointer.axis, wl_pointer.axis_source, wl_pointer.axis_discrete,
        //    and potentially wl_pointer.axis_value120 (for high-resolution wheel scroll).
        // 3. Managing scroll accumulation if needed for some devices/modes.
    }
}
