// GameActivity handles input events by double buffering MotionEvent and KeyEvent structs which
// essentially just mirror state from the corresponding Java objects.
//
// See also the javadocs for
// [`android.view.InputEvent`](https://developer.android.com/reference/android/view/InputEvent.html),
// [`android.view.MotionEvent`](https://developer.android.com/reference/android/view/MotionEvent.html),
// and [`android.view.KeyEvent`](https://developer.android.com/reference/android/view/KeyEvent).
//
// This code is mostly based on https://github.com/rust-windowing/android-ndk-rs/blob/master/ndk/src/event.rs
//
// The `Source` enum was defined based on the Java docs since there are a couple of source types that
// aren't exposed via the AInputQueue API
// The `Class` was also bound differently to `android-ndk-rs` considering how the class is defined
// by masking bits from the `Source`.

use std::iter::FusedIterator;

use super::ffi::{self, GameActivityKeyEvent, GameActivityMotionEvent};
use crate::input::{
    Axis, Button, ButtonState, EdgeFlags, KeyAction, KeyEventFlags, Keycode, MetaState,
    MotionAction, MotionEventFlags, Pointer, PointersIter, Source, ToolType,
};

// Note: try to keep this wrapper API compatible with the AInputEvent API if possible

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum InputEvent<'a> {
    MotionEvent(MotionEvent<'a>),
    KeyEvent(KeyEvent<'a>),
    TextEvent(crate::input::TextInputState),
    TextAction(crate::input::TextInputAction),
}

/// A motion event.
///
/// For general discussion of motion events in Android, see [the relevant
/// javadoc](https://developer.android.com/reference/android/view/MotionEvent).
#[derive(Clone, Debug)]
pub struct MotionEvent<'a> {
    ga_event: &'a GameActivityMotionEvent,
}

impl<'a> MotionEvent<'a> {
    pub(crate) fn new(ga_event: &'a GameActivityMotionEvent) -> Self {
        Self { ga_event }
    }

    /// Get the source of the event.
    ///
    #[inline]
    pub fn source(&self) -> Source {
        let source = self.ga_event.source as u32;
        source.into()
    }

    /// Get the device id associated with the event.
    ///
    #[inline]
    pub fn device_id(&self) -> i32 {
        self.ga_event.deviceId
    }

    /// Returns the motion action associated with the event.
    ///
    /// See [the MotionEvent docs](https://developer.android.com/reference/android/view/MotionEvent#getActionMasked())
    #[inline]
    pub fn action(&self) -> MotionAction {
        let action = self.ga_event.action as u32 & ndk_sys::AMOTION_EVENT_ACTION_MASK;
        action.into()
    }

    /// Returns which button has been modified during a press or release action.
    ///
    /// For actions other than [`MotionAction::ButtonPress`] and
    /// [`MotionAction::ButtonRelease`] the returned value is undefined.
    ///
    /// See [the MotionEvent docs](https://developer.android.com/reference/android/view/MotionEvent#getActionButton())
    #[inline]
    pub fn action_button(&self) -> Button {
        let action = self.ga_event.actionButton as u32;
        action.into()
    }

    /// Returns the pointer index of an `Up` or `Down` event.
    ///
    /// Pointer indices can change per motion event.  For an identifier that stays the same, see
    /// [`Pointer::pointer_id()`].
    ///
    /// This only has a meaning when the [action](self::action) is one of [`Up`](MotionAction::Up),
    /// [`Down`](MotionAction::Down), [`PointerUp`](MotionAction::PointerUp),
    /// or [`PointerDown`](MotionAction::PointerDown).
    #[inline]
    pub fn pointer_index(&self) -> usize {
        let action = self.ga_event.action as u32;
        let index = (action & ndk_sys::AMOTION_EVENT_ACTION_POINTER_INDEX_MASK)
            >> ndk_sys::AMOTION_EVENT_ACTION_POINTER_INDEX_SHIFT;
        index as usize
    }

    /*
    /// Returns the pointer id associated with the given pointer index.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getpointerid)
    // TODO: look at output with out-of-range pointer index
    // Probably -1 though
    pub fn pointer_id_for(&self.ga_event, pointer_index: usize) -> i32 {
        unsafe { ndk_sys::AMotionEvent_getPointerId(self.ga_event.ptr.as_ptr(), pointer_index) }
    }
    */

    /// Returns the number of pointers in this event
    ///
    /// See [the MotionEvent docs](https://developer.android.com/reference/android/view/MotionEvent#getPointerCount())
    #[inline]
    pub fn pointer_count(&self) -> usize {
        self.ga_event.pointerCount as usize
    }

    /// An iterator over the pointers in this motion event
    #[inline]
    pub fn pointers(&self) -> PointersIter<'_> {
        PointersIter {
            inner: PointersIterImpl {
                event: self,
                next_index: 0,
                count: self.pointer_count(),
            },
        }
    }

    /// The pointer at a given pointer index. Panics if the pointer index is out of bounds.
    ///
    /// If you need to loop over all the pointers, prefer the [`pointers()`](self::pointers) method.
    #[inline]
    pub fn pointer_at_index(&self, index: usize) -> Pointer<'_> {
        if index >= self.pointer_count() {
            panic!("Pointer index {} is out of bounds", index);
        }
        Pointer {
            inner: PointerImpl { event: self, index },
        }
    }

    /// Returns the state of any modifier keys that were pressed during the event.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getmetastate)
    #[inline]
    pub fn meta_state(&self) -> MetaState {
        MetaState(self.ga_event.metaState as u32)
    }

    /// Returns the button state during this event, as a bitfield.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getbuttonstate)
    #[inline]
    pub fn button_state(&self) -> ButtonState {
        ButtonState(self.ga_event.buttonState as u32)
    }

    /// Returns the time of the start of this gesture, in the `java.lang.System.nanoTime()` time
    /// base
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getdowntime)
    #[inline]
    pub fn down_time(&self) -> i64 {
        self.ga_event.downTime
    }

    /// Returns a bitfield indicating which edges were touched by this event.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getedgeflags)
    #[inline]
    pub fn edge_flags(&self) -> EdgeFlags {
        EdgeFlags(self.ga_event.edgeFlags as u32)
    }

    /// Returns the time of this event, in the `java.lang.System.nanoTime()` time base
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_geteventtime)
    #[inline]
    pub fn event_time(&self) -> i64 {
        self.ga_event.eventTime * 1_000_000 // Convert from milliseconds to nanoseconds
    }

    /// The flags associated with a motion event.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getflags)
    #[inline]
    pub fn flags(&self) -> MotionEventFlags {
        MotionEventFlags(self.ga_event.flags as u32)
    }

    /* Missing from GameActivity currently...
    /// Returns the offset in the x direction between the coordinates and the raw coordinates
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getxoffset)
    #[inline]
    pub fn x_offset(&self.ga_event) -> f32 {
        self.ga_event.x_offset
    }

    /// Returns the offset in the y direction between the coordinates and the raw coordinates
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getyoffset)
    #[inline]
    pub fn y_offset(&self.ga_event) -> f32 {
        self.ga_event.y_offset
    }
    */

    /// Returns the precision of the x value of the coordinates
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getxprecision)
    #[inline]
    pub fn x_precision(&self) -> f32 {
        self.ga_event.precisionX
    }

    /// Returns the precision of the y value of the coordinates
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getyprecision)
    #[inline]
    pub fn y_precision(&self) -> f32 {
        self.ga_event.precisionY
    }
}

/// A view into the data of a specific pointer in a motion event.
#[derive(Debug)]
pub(crate) struct PointerImpl<'a> {
    event: &'a MotionEvent<'a>,
    index: usize,
}

impl PointerImpl<'_> {
    #[inline]
    pub fn pointer_index(&self) -> usize {
        self.index
    }

    #[inline]
    pub fn pointer_id(&self) -> i32 {
        let pointer = &self.event.ga_event.pointers[self.index];
        pointer.id
    }

    #[inline]
    pub fn axis_value(&self, axis: Axis) -> f32 {
        let pointer = &self.event.ga_event.pointers[self.index];
        let axis: u32 = axis.into();
        pointer.axisValues[axis as usize]
    }

    #[inline]
    pub fn raw_x(&self) -> f32 {
        let pointer = &self.event.ga_event.pointers[self.index];
        pointer.rawX
    }

    #[inline]
    pub fn raw_y(&self) -> f32 {
        let pointer = &self.event.ga_event.pointers[self.index];
        pointer.rawY
    }

    #[inline]
    pub fn tool_type(&self) -> ToolType {
        let pointer = &self.event.ga_event.pointers[self.index];
        let tool_type = pointer.toolType as u32;
        tool_type.into()
    }

    pub fn history(&self) -> crate::input::PointerHistoryIter<'_> {
        let history_size = self.event.ga_event.historySize as usize;
        crate::input::PointerHistoryIter {
            inner: PointerHistoryIterImpl {
                event: self.event.ga_event,
                pointer_index: self.index,
                front: 0,
                back: history_size,
            },
        }
    }
}

/// An iterator over the pointers in a [`MotionEvent`].
#[derive(Debug)]
pub(crate) struct PointersIterImpl<'a> {
    event: &'a MotionEvent<'a>,
    next_index: usize,
    count: usize,
}

impl<'a> Iterator for PointersIterImpl<'a> {
    type Item = Pointer<'a>;
    fn next(&mut self) -> Option<Pointer<'a>> {
        if self.next_index < self.count {
            let ptr = Pointer {
                inner: PointerImpl {
                    event: self.event,
                    index: self.next_index,
                },
            };
            self.next_index += 1;
            Some(ptr)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let size = self.count - self.next_index;
        (size, Some(size))
    }
}

impl ExactSizeIterator for PointersIterImpl<'_> {}

/// A view into a pointer at a historical moment
#[derive(Debug)]
pub struct HistoricalPointerImpl<'a> {
    event: &'a GameActivityMotionEvent,
    pointer_index: usize,
    history_index: usize,
}

impl<'a> HistoricalPointerImpl<'a> {
    #[inline]
    pub fn pointer_index(&self) -> usize {
        self.pointer_index
    }

    /// Returns the time of the historical event, in the `java.lang.System.nanoTime()` time base
    ///
    /// See [`MotionEvent.getHistoricalEventTimeNanos`](https://developer.android.com/reference/android/view/MotionEvent#getHistoricalEventTimeNanos(int)) SDK docs
    #[inline]
    pub fn event_time(&self) -> i64 {
        unsafe { *self.event.historicalEventTimesNanos.add(self.history_index) }
    }

    #[inline]
    pub fn pointer_id(&self) -> i32 {
        let pointer = &self.event.pointers[self.pointer_index];
        pointer.id
    }

    #[inline]
    pub fn history_index(&self) -> usize {
        self.history_index
    }

    #[inline]
    pub fn axis_value(&self, axis: Axis) -> f32 {
        unsafe {
            ffi::GameActivityMotionEvent_getHistoricalAxisValue(
                self.event,
                Into::<u32>::into(axis) as i32,
                self.pointer_index as i32,
                self.history_index as i32,
            )
        }
    }
}

/// An iterator over the historical points of a specific pointer in a [`MotionEvent`].
#[derive(Debug)]
pub struct PointerHistoryIterImpl<'a> {
    event: &'a GameActivityMotionEvent,
    pointer_index: usize,
    front: usize,
    back: usize,
}

impl<'a> Iterator for PointerHistoryIterImpl<'a> {
    type Item = crate::input::HistoricalPointer<'a>;

    fn next(&mut self) -> Option<crate::input::HistoricalPointer<'a>> {
        if self.front == self.back {
            return None;
        }

        let history_index = self.front;
        self.front += 1;
        Some(crate::input::HistoricalPointer {
            inner: crate::input::HistoricalPointerImpl {
                event: self.event,
                history_index,
                pointer_index: self.pointer_index,
            },
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let size = self.back - self.front;
        (size, Some(size))
    }
}
impl<'a> DoubleEndedIterator for PointerHistoryIterImpl<'a> {
    fn next_back(&mut self) -> Option<crate::input::HistoricalPointer<'a>> {
        if self.front == self.back {
            return None;
        }

        self.back -= 1;
        let history_index = self.back;
        Some(crate::input::HistoricalPointer {
            inner: crate::input::HistoricalPointerImpl {
                event: self.event,
                history_index,
                pointer_index: self.pointer_index,
            },
        })
    }
}
impl ExactSizeIterator for PointerHistoryIterImpl<'_> {}
impl FusedIterator for PointerHistoryIterImpl<'_> {}

/// A key event.
///
/// For general discussion of key events in Android, see [the relevant
/// javadoc](https://developer.android.com/reference/android/view/KeyEvent).
#[derive(Debug, Clone)]
pub struct KeyEvent<'a> {
    ga_event: &'a GameActivityKeyEvent,
}

impl<'a> KeyEvent<'a> {
    pub(crate) fn new(ga_event: &'a GameActivityKeyEvent) -> Self {
        Self { ga_event }
    }

    /// Get the source of the event.
    ///
    #[inline]
    pub fn source(&self) -> Source {
        let source = self.ga_event.source as u32;
        source.into()
    }

    /// Get the device id associated with the event.
    ///
    #[inline]
    pub fn device_id(&self) -> i32 {
        self.ga_event.deviceId
    }

    /// Returns the key action associated with the event.
    ///
    /// See [the KeyEvent docs](https://developer.android.com/reference/android/view/KeyEvent#getAction())
    #[inline]
    pub fn action(&self) -> KeyAction {
        let action = self.ga_event.action as u32;
        action.into()
    }

    #[inline]
    pub fn action_button(&self) -> KeyAction {
        let action = self.ga_event.action as u32;
        action.into()
    }

    /// Returns the last time the key was pressed.  This is on the scale of
    /// `java.lang.System.nanoTime()`, which has nanosecond precision, but no defined start time.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_getdowntime)
    #[inline]
    pub fn down_time(&self) -> i64 {
        self.ga_event.downTime
    }

    /// Returns the time this event occured.  This is on the scale of
    /// `java.lang.System.nanoTime()`, which has nanosecond precision, but no defined start time.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_geteventtime)
    #[inline]
    pub fn event_time(&self) -> i64 {
        self.ga_event.eventTime
    }

    /// Returns the keycode associated with this key event
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_getkeycode)
    #[inline]
    pub fn key_code(&self) -> Keycode {
        let keycode = self.ga_event.keyCode as u32;
        keycode.into()
    }

    /// Returns the number of repeats of a key.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_getrepeatcount)
    #[inline]
    pub fn repeat_count(&self) -> i32 {
        self.ga_event.repeatCount
    }

    /// Returns the hardware keycode of a key.  This varies from device to device.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_getscancode)
    #[inline]
    pub fn scan_code(&self) -> i32 {
        self.ga_event.scanCode
    }
}

impl KeyEvent<'_> {
    /// Flags associated with this [`KeyEvent`].
    ///
    /// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#akeyevent_getflags)
    #[inline]
    pub fn flags(&self) -> KeyEventFlags {
        KeyEventFlags(self.ga_event.flags as u32)
    }

    /// Returns the state of the modifiers during this key event, represented by a bitmask.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_getmetastate)
    #[inline]
    pub fn meta_state(&self) -> MetaState {
        MetaState(self.ga_event.metaState as u32)
    }
}
