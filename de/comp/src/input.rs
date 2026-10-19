// Adapted from Smithay's smallvil example (v0.7.0, MIT licence,
// https://github.com/Smithay/smithay), for sol-comp: one handler per
// event, and no panics (a compositor that panics takes every app with it).

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
    },
    input::{
        keyboard::{FilterResult, Keysym, ModifiersState},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    utils::{Logical, Point, SERIAL_COUNTER},
};

use crate::state::{Comp, SessionRequest};

/// Scroll distance of one wheel notch, in logical pixels.
const NOTCH: f64 = 15.0;

/// Keys the compositor keeps for itself: Ctrl+Alt+F1..F12 switch virtual
/// terminals (xkb maps them to `XF86Switch_VT_n`), Ctrl+Alt+Backspace quits.
pub fn session_key(modifiers: ModifiersState, sym: Keysym) -> Option<SessionRequest> {
    let raw = sym.raw();
    let first = Keysym::XF86_Switch_VT_1.raw();
    if (first..=Keysym::XF86_Switch_VT_12.raw()).contains(&raw) {
        return i32::try_from(raw - first + 1)
            .ok()
            .map(SessionRequest::SwitchVt);
    }
    (modifiers.ctrl && modifiers.alt && sym == Keysym::BackSpace).then_some(SessionRequest::Quit)
}

impl Comp {
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event, .. } => self.on_key::<I>(&event),
            InputEvent::PointerMotion { event, .. } => self.on_pointer_relative::<I>(&event),
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
        let request = keyboard.input::<SessionRequest, _>(
            self,
            event.key_code(),
            event.state(),
            SERIAL_COUNTER.next_serial(),
            Event::time_msec(event),
            |_, modifiers, keysym| {
                if event.state() == smithay::backend::input::KeyState::Pressed
                    && let Some(r) = session_key(*modifiers, keysym.modified_sym())
                {
                    FilterResult::Intercept(r)
                } else {
                    FilterResult::Forward
                }
            },
        );
        if let Some(r) = request {
            match r {
                SessionRequest::Quit => self.loop_signal.stop(),
                SessionRequest::SwitchVt(_) => self.session_request = Some(r),
            }
        }
    }

    /// The pointer moved by a delta (mice, touchpads): stays on the outputs.
    fn on_pointer_relative<I: InputBackend>(&mut self, event: &I::PointerMotionEvent) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let pos = self.clamp_to_outputs(pointer.current_location() + event.delta());
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

    /// The nearest point on any output.
    fn clamp_to_outputs(&self, p: Point<f64, Logical>) -> Point<f64, Logical> {
        let mut best: Option<(f64, Point<f64, Logical>)> = None;
        for geo in self
            .space
            .outputs()
            .filter_map(|o| self.space.output_geometry(o))
        {
            let g = geo.to_f64();
            let q = Point::from((
                p.x.clamp(g.loc.x, g.loc.x + g.size.w - 1.0),
                p.y.clamp(g.loc.y, g.loc.y + g.size.h - 1.0),
            ));
            let d = (q.x - p.x).hypot(q.y - p.y);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, q));
            }
        }
        best.map_or(p, |(_, q)| q)
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
        let Some(pointer) = self.seat.get_pointer() else {
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
            self.focus(clicked.as_ref());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_keys() {
        let none = ModifiersState::default();
        let ctrl_alt = ModifiersState {
            ctrl: true,
            alt: true,
            ..ModifiersState::default()
        };
        assert_eq!(
            session_key(ctrl_alt, Keysym::XF86_Switch_VT_1),
            Some(SessionRequest::SwitchVt(1))
        );
        assert_eq!(
            session_key(ctrl_alt, Keysym::XF86_Switch_VT_12),
            Some(SessionRequest::SwitchVt(12))
        );
        assert_eq!(
            session_key(ctrl_alt, Keysym::BackSpace),
            Some(SessionRequest::Quit)
        );
        assert_eq!(session_key(none, Keysym::BackSpace), None);
        assert_eq!(session_key(ctrl_alt, Keysym::F1), None);
    }
}
