// Adapted from Smithay's smallvil example (v0.7.0, MIT licence,
// https://github.com/Smithay/smithay), for sol-comp: one handler per
// event, and no panics (a compositor that panics takes every app with it).

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
    },
    input::{
        keyboard::FilterResult,
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::SERIAL_COUNTER,
};

use crate::state::Comp;

/// Scroll distance of one wheel notch, in logical pixels.
const NOTCH: f64 = 15.0;

impl Comp {
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event, .. } => self.on_key::<I>(&event),
            InputEvent::PointerMotionAbsolute { event, .. } => {
                self.on_pointer_absolute::<I>(&event);
            }
            InputEvent::PointerButton { event, .. } => self.on_button::<I>(&event),
            InputEvent::PointerAxis { event, .. } => self.on_axis::<I>(&event),
            _ => {}
        }
    }

    fn on_key<I: InputBackend>(&mut self, event: &I::KeyboardKeyEvent) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        keyboard.input::<(), _>(
            self,
            event.key_code(),
            event.state(),
            SERIAL_COUNTER.next_serial(),
            Event::time_msec(event),
            |_, _, _| FilterResult::Forward,
        );
    }

    fn on_pointer_absolute<I: InputBackend>(&mut self, event: &I::PointerMotionAbsoluteEvent) {
        let Some(output_geo) = self
            .space
            .outputs()
            .next()
            .and_then(|o| self.space.output_geometry(o))
        else {
            return;
        };
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();
        let under = self.surface_under(pos);
        pointer.motion(
            self,
            under,
            &MotionEvent {
                location: pos,
                serial: SERIAL_COUNTER.next_serial(),
                time: event.time_msec(),
            },
        );
        pointer.frame(self);
    }

    fn on_button<I: InputBackend>(&mut self, event: &I::PointerButtonEvent) {
        let (Some(pointer), Some(keyboard)) = (self.seat.get_pointer(), self.seat.get_keyboard())
        else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        let state = event.state();
        // A click raises and focuses the window under the pointer, or
        // clears focus on the desktop.
        if state == ButtonState::Pressed && !pointer.is_grabbed() {
            let clicked = self
                .space
                .element_under(pointer.current_location())
                .map(|(w, _)| w.clone());
            if let Some(window) = clicked {
                self.space.raise_element(&window, true);
                let surface = window.toplevel().map(|t| t.wl_surface().clone());
                keyboard.set_focus(self, surface, serial);
            } else {
                for window in self.space.elements() {
                    window.set_activated(false);
                }
                keyboard.set_focus(self, Option::<WlSurface>::None, serial);
            }
            for window in self.space.elements() {
                if let Some(t) = window.toplevel() {
                    t.send_pending_configure();
                }
            }
        }
        pointer.button(
            self,
            &ButtonEvent {
                button: event.button_code(),
                state,
                serial,
                time: event.time_msec(),
            },
        );
        pointer.frame(self);
    }

    fn on_axis<I: InputBackend>(&mut self, event: &I::PointerAxisEvent) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let source = event.source();
        let mut frame = AxisFrame::new(event.time_msec()).source(source);
        for axis in [Axis::Horizontal, Axis::Vertical] {
            let v120 = event.amount_v120(axis);
            let amount = event
                .amount(axis)
                .unwrap_or_else(|| v120.unwrap_or(0.0) * NOTCH / 120.0);
            if amount != 0.0 {
                frame = frame.value(axis, amount);
                if let Some(discrete) = v120 {
                    // v120 steps are whole numbers by definition.
                    #[allow(clippy::cast_possible_truncation)]
                    let discrete = discrete as i32;
                    frame = frame.v120(axis, discrete);
                }
            }
            if source == AxisSource::Finger && event.amount(axis) == Some(0.0) {
                frame = frame.stop(axis);
            }
        }
        pointer.axis(self, frame);
        pointer.frame(self);
    }
}
