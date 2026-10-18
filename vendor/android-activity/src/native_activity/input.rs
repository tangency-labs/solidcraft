use std::{iter::FusedIterator, marker::PhantomData, ptr::NonNull};

use crate::input::{
    Axis, Button, ButtonState, EdgeFlags, KeyAction, Keycode, MetaState, MotionAction,
    MotionEventFlags, Pointer, PointersIter, Source, ToolType,
};

/// A motion event
///
/// For general discussion of motion events in Android, see [the relevant
/// javadoc](https://developer.android.com/reference/android/view/MotionEvent).
#[derive(Debug)]
#[repr(transparent)]
pub struct MotionEvent<'a> {
    ndk_event: ndk::event::MotionEvent,
    _lifetime: PhantomData<&'a ndk::event::MotionEvent>,
}
impl MotionEvent<'_> {
    pub(crate) fn new(ndk_event: ndk::event::MotionEvent) -> Self {
        Self {
            ndk_event,
            _lifetime: PhantomData,
        }
    }
    pub(crate) fn into_ndk_event(self) -> ndk::event::MotionEvent {
        self.ndk_event
    }

    /// Get the source of the event.
    ///
    #[inline]
    pub fn source(&self) -> Source {
        // XXX: we use `AInputEvent_getSource` directly (instead of calling
        // ndk_event.source()) since we have our own `Source` enum that we
        // share between backends, which may also capture unknown variants
        // added in new versions of Android.
        let source =
            unsafe { ndk_sys::AInputEvent_getSource(self.ndk_event.ptr().as_ptr()) as u32 };
        source.into()
    }

    /// Get the device id associated with the event.
    ///
    #[inline]
    pub fn device_id(&self) -> i32 {
        self.ndk_event.device_id()
    }

    /// Returns the motion action associated with the event.
    ///
    /// See [the MotionEvent docs](https://developer.android.com/reference/android/view/MotionEvent#getActionMasked())
    #[inline]
    pub fn action(&self) -> MotionAction {
        // XXX: we use `AMotionEvent_getAction` directly since we have our own
        // `MotionAction` enum that we share between backends, which may also
        // capture unknown variants added in new versions of Android.
        let action =
            unsafe { ndk_sys::AMotionEvent_getAction(self.ndk_event.ptr().as_ptr()) as u32 }
                & ndk_sys::AMOTION_EVENT_ACTION_MASK;
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
        let action_button =
            unsafe { ndk_sys::AMotionEvent_getActionButton(self.ndk_event.ptr().as_ptr()) as u32 };
        action_button.into()
    }

    /// Returns the pointer index of an `Up` or `Down` event.
    ///
    /// Pointer indices can change per motion event.  For an identifier that stays the same, see
    /// [`Pointer::pointer_id()`].
    ///
    /// This only has a meaning when the [action](Self::action) is one of [`Up`](MotionAction::Up),
    /// [`Down`](MotionAction::Down), [`PointerUp`](MotionAction::PointerUp),
    /// or [`PointerDown`](MotionAction::PointerDown).
    #[inline]
    pub fn pointer_index(&self) -> usize {
        self.ndk_event.pointer_index()
    }

    /*
    /// Returns the pointer id associated with the given pointer index.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getpointerid)
    // TODO: look at output with out-of-range pointer index
    // Probably -1 though
    pub fn pointer_id_for(&self, pointer_index: usize) -> i32 {
        unsafe { ndk_sys::AMotionEvent_getPointerId(self.ndk_event.ptr.as_ptr(), pointer_index) }
    }
    */

    /// Returns the number of pointers in this event
    ///
    /// See [the MotionEvent docs](https://developer.android.com/reference/android/view/MotionEvent#getPointerCount())
    #[inline]
    pub fn pointer_count(&self) -> usize {
        self.ndk_event.pointer_count()
    }

    /// An iterator over the pointers in this motion event
    #[inline]
    pub fn pointers(&self) -> PointersIter<'_> {
        PointersIter {
            inner: PointersIterImpl {
                event: self.ndk_event.ptr(),
                pointer_index: 0,
                pointer_count: self.ndk_event.pointer_count(),
                _marker: std::marker::PhantomData,
            },
        }
    }

    /// The pointer at a given pointer index. Panics if the pointer index is out of bounds.
    ///
    /// If you need to loop over all the pointers, prefer the [`pointers()`](Self::pointers) method.
    #[inline]
    pub fn pointer_at_index(&self, index: usize) -> Pointer<'_> {
        Pointer {
            inner: PointerImpl {
                event: self.ndk_event.ptr(),
                pointer_index: index,
                _marker: std::marker::PhantomData,
            },
        }
    }

    /// Returns the state of any modifier keys that were pressed during the event.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getmetastate)
    #[inline]
    pub fn meta_state(&self) -> MetaState {
        self.ndk_event.meta_state().into()
    }

    /// Returns the button state during this event, as a bitfield.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getbuttonstate)
    #[inline]
    pub fn button_state(&self) -> ButtonState {
        self.ndk_event.button_state().into()
    }

    /// Returns the time of the start of this gesture, in the `java.lang.System.nanoTime()` time
    /// base
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getdowntime)
    #[inline]
    pub fn down_time(&self) -> i64 {
        self.ndk_event.down_time()
    }

    /// Returns a bitfield indicating which edges were touched by this event.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getedgeflags)
    #[inline]
    pub fn edge_flags(&self) -> EdgeFlags {
        self.ndk_event.edge_flags().into()
    }

    /// Returns the time of this event, in the `java.lang.System.nanoTime()` time base
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_geteventtime)
    #[inline]
    pub fn event_time(&self) -> i64 {
        self.ndk_event.event_time()
    }

    /// The flags associated with a motion event.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getflags)
    #[inline]
    pub fn flags(&self) -> MotionEventFlags {
        self.ndk_event.flags().into()
    }

    /* Missing from GameActivity currently...
    /// Returns the offset in the x direction between the coordinates and the raw coordinates
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getxoffset)
    #[inline]
    pub fn x_offset(&self) -> f32 {
        self.ndk_event.x_offset()
    }

    /// Returns the offset in the y direction between the coordinates and the raw coordinates
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getyoffset)
    #[inline]
    pub fn y_offset(&self) -> f32 {
        self.ndk_event.y_offset()
    }
    */

    /// Returns the precision of the x value of the coordinates
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getxprecision)
    #[inline]
    pub fn x_precision(&self) -> f32 {
        self.ndk_event.x_precision()
    }

    /// Returns the precision of the y value of the coordinates
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getyprecision)
    #[inline]
    pub fn y_precision(&self) -> f32 {
        self.ndk_event.y_precision()
    }
}

/// A view into the data of a specific pointer in a motion event.
#[derive(Debug)]
pub(crate) struct PointerImpl<'a> {
    event: NonNull<ndk_sys::AInputEvent>,
    pointer_index: usize,
    _marker: std::marker::PhantomData<&'a MotionEvent<'a>>,
}

impl PointerImpl<'_> {
    #[inline]
    pub fn pointer_index(&self) -> usize {
        self.pointer_index
    }

    #[inline]
    pub fn pointer_id(&self) -> i32 {
        unsafe { ndk_sys::AMotionEvent_getPointerId(self.event.as_ptr(), self.pointer_index) }
    }

    #[inline]
    pub fn axis_value(&self, axis: Axis) -> f32 {
        let value: u32 = axis.into();
        let value = value as i32;
        unsafe {
            ndk_sys::AMotionEvent_getAxisValue(self.event.as_ptr(), value, self.pointer_index)
        }
    }

    #[inline]
    pub fn raw_x(&self) -> f32 {
        unsafe { ndk_sys::AMotionEvent_getRawX(self.event.as_ptr(), self.pointer_index) }
    }

    #[inline]
    pub fn raw_y(&self) -> f32 {
        unsafe { ndk_sys::AMotionEvent_getRawY(self.event.as_ptr(), self.pointer_index) }
    }

    #[inline]
    pub fn tool_type(&self) -> ToolType {
        let value =
            unsafe { ndk_sys::AMotionEvent_getToolType(self.event.as_ptr(), self.pointer_index) };
        let value = value as u32;
        value.into()
    }

    pub fn history(&self) -> crate::input::PointerHistoryIter<'_> {
        let history_size =
            unsafe { ndk_sys::AMotionEvent_getHistorySize(self.event.as_ptr()) } as usize;
        crate::input::PointerHistoryIter {
            inner: PointerHistoryIterImpl {
                event: self.event,
                pointer_index: self.pointer_index,
                front: 0,
                back: history_size,
                _marker: std::marker::PhantomData,
            },
        }
    }
}

#[derive(Debug)]
pub(crate) struct PointersIterImpl<'a> {
    event: NonNull<ndk_sys::AInputEvent>,
    pointer_count: usize,
    pointer_index: usize,
    _marker: std::marker::PhantomData<&'a MotionEvent<'a>>,
}

impl<'a> Iterator for PointersIterImpl<'a> {
    type Item = Pointer<'a>;
    fn next(&mut self) -> Option<Pointer<'a>> {
        if self.pointer_index == self.pointer_count {
            return None;
        }
        let pointer = Pointer {
            inner: PointerImpl {
                event: self.event,
                pointer_index: self.pointer_index,
                _marker: std::marker::PhantomData,
            },
        };
        self.pointer_index += 1;
        Some(pointer)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.pointer_count - self.pointer_index;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for PointersIterImpl<'_> {}

/// A view into a pointer at a historical moment
#[derive(Debug)]
pub struct HistoricalPointerImpl<'a> {
    event: NonNull<ndk_sys::AInputEvent>,
    pointer_index: usize,
    history_index: usize,
    _marker: std::marker::PhantomData<&'a MotionEvent<'a>>,
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
        unsafe {
            ndk_sys::AMotionEvent_getHistoricalEventTime(self.event.as_ptr(), self.history_index)
        }
    }

    #[inline]
    pub fn pointer_id(&self) -> i32 {
        unsafe { ndk_sys::AMotionEvent_getPointerId(self.event.as_ptr(), self.pointer_index) }
    }

    #[inline]
    pub fn history_index(&self) -> usize {
        self.history_index
    }

    #[inline]
    pub fn axis_value(&self, axis: Axis) -> f32 {
        unsafe {
            ndk_sys::AMotionEvent_getHistoricalAxisValue(
                self.event.as_ptr(),
                Into::<u32>::into(axis) as i32,
                self.pointer_index,
                self.history_index,
            )
        }
    }
}

/// An iterator over the historical points of a specific pointer in a [`MotionEvent`].
#[derive(Debug)]
pub struct PointerHistoryIterImpl<'a> {
    event: NonNull<ndk_sys::AInputEvent>,
    pointer_index: usize,
    front: usize,
    back: usize,
    _marker: std::marker::PhantomData<&'a MotionEvent<'a>>,
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
                _marker: std::marker::PhantomData,
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
                _marker: std::marker::PhantomData,
            },
        })
    }
}
impl ExactSizeIterator for PointerHistoryIterImpl<'_> {}
impl FusedIterator for PointerHistoryIterImpl<'_> {}

/// A key event
///
/// For general discussion of key events in Android, see [the relevant
/// javadoc](https://developer.android.com/reference/android/view/KeyEvent).
#[derive(Debug)]
#[repr(transparent)]
pub struct KeyEvent<'a> {
    ndk_event: ndk::event::KeyEvent,
    _lifetime: PhantomData<&'a ndk::event::KeyEvent>,
}
impl KeyEvent<'_> {
    pub(crate) fn new(ndk_event: ndk::event::KeyEvent) -> Self {
        Self {
            ndk_event,
            _lifetime: PhantomData,
        }
    }
    pub(crate) fn into_ndk_event(self) -> ndk::event::KeyEvent {
        self.ndk_event
    }

    /// Get the source of the event.
    ///
    #[inline]
    pub fn source(&self) -> Source {
        // XXX: we use `AInputEvent_getSource` directly (instead of calling
        // ndk_event.source()) since we have our own `Source` enum that we
        // share between backends, which may also capture unknown variants
        // added in new versions of Android.
        let source =
            unsafe { ndk_sys::AInputEvent_getSource(self.ndk_event.ptr().as_ptr()) as u32 };
        source.into()
    }

    /// Get the device id associated with the event.
    ///
    #[inline]
    pub fn device_id(&self) -> i32 {
        self.ndk_event.device_id()
    }

    /// Returns the key action associated with the event.
    ///
    /// See [the KeyEvent docs](https://developer.android.com/reference/android/view/KeyEvent#getAction())
    #[inline]
    pub fn action(&self) -> KeyAction {
        // XXX: we use `AInputEvent_getAction` directly since we have our own
        // `KeyAction` enum that we share between backends, which may also
        // capture unknown variants added in new versions of Android.
        let action = unsafe { ndk_sys::AKeyEvent_getAction(self.ndk_event.ptr().as_ptr()) as u32 };
        action.into()
    }

    /// Returns the last time the key was pressed.  This is on the scale of
    /// `java.lang.System.nanoTime()`, which has nanosecond precision, but no defined start time.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_getdowntime)
    #[inline]
    pub fn down_time(&self) -> i64 {
        self.ndk_event.down_time()
    }

    /// Returns the time this event occured.  This is on the scale of
    /// `java.lang.System.nanoTime()`, which has nanosecond precision, but no defined start time.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_geteventtime)
    #[inline]
    pub fn event_time(&self) -> i64 {
        self.ndk_event.event_time()
    }

    /// Returns the keycode associated with this key event
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_getkeycode)
    #[inline]
    pub fn key_code(&self) -> Keycode {
        // XXX: we use `AInputEvent_getKeyCode` directly since we have our own
        // `Keycode` enum that we share between backends, which may also
        // capture unknown variants added in new versions of Android.
        let keycode =
            unsafe { ndk_sys::AKeyEvent_getKeyCode(self.ndk_event.ptr().as_ptr()) as u32 };
        keycode.into()
    }

    /// Returns the number of repeats of a key.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_getrepeatcount)
    #[inline]
    pub fn repeat_count(&self) -> i32 {
        self.ndk_event.repeat_count()
    }

    /// Returns the hardware keycode of a key.  This varies from device to device.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_getscancode)
    #[inline]
    pub fn scan_code(&self) -> i32 {
        self.ndk_event.scan_code()
    }

    /// Returns the state of the modifiers during this key event, represented by a bitmask.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_getmetastate)
    #[inline]
    pub fn meta_state(&self) -> MetaState {
        self.ndk_event.meta_state().into()
    }
}

// We use our own wrapper type for input events to have better consistency
// with GameActivity and ensure the enum can be extended without needing a
// semver bump
/// Enum of possible input events
#[derive(Debug)]
#[non_exhaustive]
pub enum InputEvent<'a> {
    MotionEvent(self::MotionEvent<'a>),
    KeyEvent(self::KeyEvent<'a>),
    TextEvent(crate::input::TextInputState),
    TextAction(crate::input::TextInputAction),
}
