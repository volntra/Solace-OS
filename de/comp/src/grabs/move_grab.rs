// Adapted from Smithay's smallvil example (v0.7.0, MIT licence,
// https://github.com/Smithay/smithay), for fern-comp.
// Kept close to the original: pixel deltas are truncated to whole logical
// pixels on purpose, and helper items sit next to their use.
#![allow(clippy::cast_possible_truncation, clippy::items_after_statements)]

use crate::state::Comp;
use smithay::{
    desktop::Window,
    input::pointer::{
        AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent,
        GesturePinchEndEvent, GesturePinchUpdateEvent, GestureSwipeBeginEvent,
        GestureSwipeEndEvent, GestureSwipeUpdateEvent, GrabStartData as PointerGrabStartData,
        MotionEvent, PointerGrab, PointerInnerHandle, RelativeMotionEvent,
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point},
};

pub struct MoveSurfaceGrab {
    pub start_data: PointerGrabStartData<Comp>,
    pub window: Window,
    pub initial_window_location: Point<i32, Logical>,
    /// The window's width when the move began.
    pub initial_width: i32,
}

impl PointerGrab<Comp> for MoveSurfaceGrab {
    fn motion(
        &mut self,
        data: &mut Comp,
        handle: &mut PointerInnerHandle<'_, Comp>,
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        // While the grab is active, no client has pointer focus
        handle.motion(data, None, event);

        // The pointer keeps its place across the window: if the window
        // changes width while held (dragged out of a tile), the point under
        // the pointer stays at the same fraction of the width.
        let start = self.start_data.location;
        let initial = self.initial_window_location.to_f64();
        let width = f64::from(self.window.geometry().size.w);
        let across = if self.initial_width > 0 && width > 0.0 {
            (start.x - initial.x) * width / f64::from(self.initial_width)
        } else {
            start.x - initial.x
        };
        let new_location = Point::<f64, Logical>::from((
            event.location.x - across,
            event.location.y - (start.y - initial.y),
        ));
        data.space
            .map_element(self.window.clone(), new_location.to_i32_round(), false);
        data.wm_move_update(&self.window, event.location, false);
    }

    fn relative_motion(
        &mut self,
        data: &mut Comp,
        handle: &mut PointerInnerHandle<'_, Comp>,
        focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &RelativeMotionEvent,
    ) {
        handle.relative_motion(data, focus, event);
    }

    fn button(
        &mut self,
        data: &mut Comp,
        handle: &mut PointerInnerHandle<'_, Comp>,
        event: &ButtonEvent,
    ) {
        handle.button(data, event);

        // The button is a button code as defined in the
        // Linux kernel's linux/input-event-codes.h header file, e.g. BTN_LEFT.
        const BTN_LEFT: u32 = 0x110;

        if !handle.current_pressed().contains(&BTN_LEFT) {
            // No more buttons are pressed, release the grab.
            let at = handle.current_location();
            handle.unset_grab(self, data, event.serial, event.time, true);
            data.wm_move_update(&self.window, at, true);
        }
    }

    fn axis(
        &mut self,
        data: &mut Comp,
        handle: &mut PointerInnerHandle<'_, Comp>,
        details: AxisFrame,
    ) {
        handle.axis(data, details);
    }

    fn frame(&mut self, data: &mut Comp, handle: &mut PointerInnerHandle<'_, Comp>) {
        handle.frame(data);
    }

    fn gesture_swipe_begin(
        &mut self,
        data: &mut Comp,
        handle: &mut PointerInnerHandle<'_, Comp>,
        event: &GestureSwipeBeginEvent,
    ) {
        handle.gesture_swipe_begin(data, event);
    }

    fn gesture_swipe_update(
        &mut self,
        data: &mut Comp,
        handle: &mut PointerInnerHandle<'_, Comp>,
        event: &GestureSwipeUpdateEvent,
    ) {
        handle.gesture_swipe_update(data, event);
    }

    fn gesture_swipe_end(
        &mut self,
        data: &mut Comp,
        handle: &mut PointerInnerHandle<'_, Comp>,
        event: &GestureSwipeEndEvent,
    ) {
        handle.gesture_swipe_end(data, event);
    }

    fn gesture_pinch_begin(
        &mut self,
        data: &mut Comp,
        handle: &mut PointerInnerHandle<'_, Comp>,
        event: &GesturePinchBeginEvent,
    ) {
        handle.gesture_pinch_begin(data, event);
    }

    fn gesture_pinch_update(
        &mut self,
        data: &mut Comp,
        handle: &mut PointerInnerHandle<'_, Comp>,
        event: &GesturePinchUpdateEvent,
    ) {
        handle.gesture_pinch_update(data, event);
    }

    fn gesture_pinch_end(
        &mut self,
        data: &mut Comp,
        handle: &mut PointerInnerHandle<'_, Comp>,
        event: &GesturePinchEndEvent,
    ) {
        handle.gesture_pinch_end(data, event);
    }

    fn gesture_hold_begin(
        &mut self,
        data: &mut Comp,
        handle: &mut PointerInnerHandle<'_, Comp>,
        event: &GestureHoldBeginEvent,
    ) {
        handle.gesture_hold_begin(data, event);
    }

    fn gesture_hold_end(
        &mut self,
        data: &mut Comp,
        handle: &mut PointerInnerHandle<'_, Comp>,
        event: &GestureHoldEndEvent,
    ) {
        handle.gesture_hold_end(data, event);
    }

    fn start_data(&self) -> &PointerGrabStartData<Comp> {
        &self.start_data
    }

    fn unset(&mut self, _data: &mut Comp) {}
}
