use std::iter::FusedIterator;

use bitflags::bitflags;

pub use crate::activity_impl::input::*;
use crate::InputStatus;

mod sdk;
pub use sdk::*;

/// An enum representing the source of an [`MotionEvent`] or [`KeyEvent`]
///
/// See [the InputDevice docs](https://developer.android.com/reference/android/view/InputDevice#SOURCE_ANY)
///
/// # Android Extensible Enum
///
/// This is a runtime [extensible enum](`crate#android-extensible-enums`) and
/// should be handled similar to a `#[non_exhaustive]` enum to maintain
/// forwards compatibility.
///
/// This implements `Into<u32>` and `From<u32>` for converting to/from Android
/// SDK integer values.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, num_enum::FromPrimitive, num_enum::IntoPrimitive)]
#[non_exhaustive]
#[repr(u32)]
pub enum Source {
    BluetoothStylus = 0x0000c002,
    Dpad = 0x00000201,
    /// Either a gamepad or a joystick
    Gamepad = 0x00000401,
    Hdmi = 0x02000001,
    /// Either a gamepad or a joystick
    Joystick = 0x01000010,
    /// Pretty much any device with buttons. Query the keyboard type to determine
    /// if it has alphabetic keys and can be used for text entry.
    Keyboard = 0x00000101,
    /// A pointing device, such as a mouse or trackpad
    Mouse = 0x00002002,
    /// A pointing device, such as a mouse or trackpad whose relative motions should be treated as navigation events
    MouseRelative = 0x00020004,
    /// An input device akin to a scroll wheel
    RotaryEncoder = 0x00400000,
    Sensor = 0x04000000,
    Stylus = 0x00004002,
    Touchpad = 0x00100008,
    Touchscreen = 0x00001002,
    TouchNavigation = 0x00200000,
    Trackball = 0x00010004,

    // We need to consider that the enum variants may be extended across
    // different versions of Android (i.e. effectively at runtime) but at the
    // same time we don't want it to be an API break to extend this enum in
    // future releases of `android-activity` with new variants from the latest
    // NDK/SDK.
    //
    // We can't just use `#[non_exhaustive]` because that only really helps
    // when adding new variants in sync with android-activity releases.
    //
    // On the other hand we also can't rely on a catch-all `Unknown(u32)` that
    // only really helps with unknown variants seen at runtime.
    //
    // What we aim for instead is to have a hidden catch-all variant that
    // is considered (practically) unmatchable so code is forced to have
    // a `unknown => {}` catch-all pattern match that will cover unknown variants
    // either in the form of Rust variants added in future versions or
    // in the form of an `__Unknown(u32)` integer that represents an unknown
    // variant seen at runtime.
    //
    // Any `unknown => {}` pattern match can rely on `IntoPrimitive` to convert
    // the `unknown` variant to the integer that comes from the Android SDK
    // in case that values needs to be passed on, even without knowing its
    // semantic meaning at compile time.
    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(u32),
}

// ndk_sys doesn't currently have the `TRACKBALL` flag so we define our
// own internal class constants for now
bitflags! {
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    struct SourceFlags: u32 {
        const CLASS_MASK = 0x000000ff;

        const BUTTON = 0x00000001;
        const POINTER = 0x00000002;
        const TRACKBALL = 0x00000004;
        const POSITION = 0x00000008;
        const JOYSTICK = 0x00000010;
        const NONE = 0;
    }
}

impl Source {
    #[inline]
    pub fn is_button_class(self) -> bool {
        let class = SourceFlags::from_bits_truncate(self.into());
        class.contains(SourceFlags::BUTTON)
    }
    #[inline]
    pub fn is_pointer_class(self) -> bool {
        let class = SourceFlags::from_bits_truncate(self.into());
        class.contains(SourceFlags::POINTER)
    }
    #[inline]
    pub fn is_trackball_class(self) -> bool {
        let class = SourceFlags::from_bits_truncate(self.into());
        class.contains(SourceFlags::TRACKBALL)
    }
    #[inline]
    pub fn is_position_class(self) -> bool {
        let class = SourceFlags::from_bits_truncate(self.into());
        class.contains(SourceFlags::POSITION)
    }
    #[inline]
    pub fn is_joystick_class(self) -> bool {
        let class = SourceFlags::from_bits_truncate(self.into());
        class.contains(SourceFlags::JOYSTICK)
    }
}

/// A bitfield representing the state of modifier keys during an event.
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-25)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MetaState(pub u32);

impl MetaState {
    #[inline]
    pub fn alt_on(self) -> bool {
        self.0 & ndk_sys::AMETA_ALT_ON != 0
    }
    #[inline]
    pub fn alt_left_on(self) -> bool {
        self.0 & ndk_sys::AMETA_ALT_LEFT_ON != 0
    }
    #[inline]
    pub fn alt_right_on(self) -> bool {
        self.0 & ndk_sys::AMETA_ALT_RIGHT_ON != 0
    }
    #[inline]
    pub fn shift_on(self) -> bool {
        self.0 & ndk_sys::AMETA_SHIFT_ON != 0
    }
    #[inline]
    pub fn shift_left_on(self) -> bool {
        self.0 & ndk_sys::AMETA_SHIFT_LEFT_ON != 0
    }
    #[inline]
    pub fn shift_right_on(self) -> bool {
        self.0 & ndk_sys::AMETA_SHIFT_RIGHT_ON != 0
    }
    #[inline]
    pub fn sym_on(self) -> bool {
        self.0 & ndk_sys::AMETA_SYM_ON != 0
    }
    #[inline]
    pub fn function_on(self) -> bool {
        self.0 & ndk_sys::AMETA_FUNCTION_ON != 0
    }
    #[inline]
    pub fn ctrl_on(self) -> bool {
        self.0 & ndk_sys::AMETA_CTRL_ON != 0
    }
    #[inline]
    pub fn ctrl_left_on(self) -> bool {
        self.0 & ndk_sys::AMETA_CTRL_LEFT_ON != 0
    }
    #[inline]
    pub fn ctrl_right_on(self) -> bool {
        self.0 & ndk_sys::AMETA_CTRL_RIGHT_ON != 0
    }
    #[inline]
    pub fn meta_on(self) -> bool {
        self.0 & ndk_sys::AMETA_META_ON != 0
    }
    #[inline]
    pub fn meta_left_on(self) -> bool {
        self.0 & ndk_sys::AMETA_META_LEFT_ON != 0
    }
    #[inline]
    pub fn meta_right_on(self) -> bool {
        self.0 & ndk_sys::AMETA_META_RIGHT_ON != 0
    }
    #[inline]
    pub fn caps_lock_on(self) -> bool {
        self.0 & ndk_sys::AMETA_CAPS_LOCK_ON != 0
    }
    #[inline]
    pub fn num_lock_on(self) -> bool {
        self.0 & ndk_sys::AMETA_NUM_LOCK_ON != 0
    }
    #[inline]
    pub fn scroll_lock_on(self) -> bool {
        self.0 & ndk_sys::AMETA_SCROLL_LOCK_ON != 0
    }
}

impl From<ndk::event::MetaState> for MetaState {
    fn from(value: ndk::event::MetaState) -> Self {
        Self(value.0)
    }
}

/// A motion action.
///
/// See [the NDK
/// docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-29)
///
/// # Android Extensible Enum
///
/// This is a runtime [extensible enum](`crate#android-extensible-enums`) and
/// should be handled similar to a `#[non_exhaustive]` enum to maintain
/// forwards compatibility.
///
/// This implements `Into<u32>` and `From<u32>` for converting to/from Android
/// SDK integer values.
///
#[derive(Copy, Clone, Debug, PartialEq, Eq, num_enum::FromPrimitive, num_enum::IntoPrimitive)]
#[non_exhaustive]
#[repr(u32)]
pub enum MotionAction {
    Down = ndk_sys::AMOTION_EVENT_ACTION_DOWN,
    Up = ndk_sys::AMOTION_EVENT_ACTION_UP,
    Move = ndk_sys::AMOTION_EVENT_ACTION_MOVE,
    Cancel = ndk_sys::AMOTION_EVENT_ACTION_CANCEL,
    Outside = ndk_sys::AMOTION_EVENT_ACTION_OUTSIDE,
    PointerDown = ndk_sys::AMOTION_EVENT_ACTION_POINTER_DOWN,
    PointerUp = ndk_sys::AMOTION_EVENT_ACTION_POINTER_UP,
    HoverMove = ndk_sys::AMOTION_EVENT_ACTION_HOVER_MOVE,
    Scroll = ndk_sys::AMOTION_EVENT_ACTION_SCROLL,
    HoverEnter = ndk_sys::AMOTION_EVENT_ACTION_HOVER_ENTER,
    HoverExit = ndk_sys::AMOTION_EVENT_ACTION_HOVER_EXIT,
    ButtonPress = ndk_sys::AMOTION_EVENT_ACTION_BUTTON_PRESS,
    ButtonRelease = ndk_sys::AMOTION_EVENT_ACTION_BUTTON_RELEASE,

    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(u32),
}

/// Identifies buttons that are associated with motion events.
///
/// See [the NDK
/// docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-47)
///
/// # Android Extensible Enum
///
/// This is a runtime [extensible enum](`crate#android-extensible-enums`) and
/// should be handled similar to a `#[non_exhaustive]` enum to maintain
/// forwards compatibility.
///
/// This implements `Into<u32>` and `From<u32>` for converting to/from Android
/// SDK integer values.
///
#[derive(Copy, Clone, Debug, PartialEq, Eq, num_enum::FromPrimitive, num_enum::IntoPrimitive)]
#[non_exhaustive]
#[repr(u32)]
pub enum Button {
    Back = ndk_sys::AMOTION_EVENT_BUTTON_BACK,
    Forward = ndk_sys::AMOTION_EVENT_BUTTON_FORWARD,
    Primary = ndk_sys::AMOTION_EVENT_BUTTON_PRIMARY,
    Secondary = ndk_sys::AMOTION_EVENT_BUTTON_SECONDARY,
    StylusPrimary = ndk_sys::AMOTION_EVENT_BUTTON_STYLUS_PRIMARY,
    StylusSecondary = ndk_sys::AMOTION_EVENT_BUTTON_STYLUS_SECONDARY,
    Tertiary = ndk_sys::AMOTION_EVENT_BUTTON_TERTIARY,

    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(u32),
}

/// An axis of a motion event.
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-32)
///
/// # Android Extensible Enum
///
/// This is a runtime [extensible enum](`crate#android-extensible-enums`) and
/// should be handled similar to a `#[non_exhaustive]` enum to maintain
/// forwards compatibility.
///
/// This implements `Into<u32>` and `From<u32>` for converting to/from Android
/// SDK integer values.
///
#[derive(Copy, Clone, Debug, PartialEq, Eq, num_enum::FromPrimitive, num_enum::IntoPrimitive)]
#[non_exhaustive]
#[repr(u32)]
pub enum Axis {
    X = ndk_sys::AMOTION_EVENT_AXIS_X,
    Y = ndk_sys::AMOTION_EVENT_AXIS_Y,
    Pressure = ndk_sys::AMOTION_EVENT_AXIS_PRESSURE,
    Size = ndk_sys::AMOTION_EVENT_AXIS_SIZE,
    TouchMajor = ndk_sys::AMOTION_EVENT_AXIS_TOUCH_MAJOR,
    TouchMinor = ndk_sys::AMOTION_EVENT_AXIS_TOUCH_MINOR,
    ToolMajor = ndk_sys::AMOTION_EVENT_AXIS_TOOL_MAJOR,
    ToolMinor = ndk_sys::AMOTION_EVENT_AXIS_TOOL_MINOR,
    Orientation = ndk_sys::AMOTION_EVENT_AXIS_ORIENTATION,
    Vscroll = ndk_sys::AMOTION_EVENT_AXIS_VSCROLL,
    Hscroll = ndk_sys::AMOTION_EVENT_AXIS_HSCROLL,
    Z = ndk_sys::AMOTION_EVENT_AXIS_Z,
    Rx = ndk_sys::AMOTION_EVENT_AXIS_RX,
    Ry = ndk_sys::AMOTION_EVENT_AXIS_RY,
    Rz = ndk_sys::AMOTION_EVENT_AXIS_RZ,
    HatX = ndk_sys::AMOTION_EVENT_AXIS_HAT_X,
    HatY = ndk_sys::AMOTION_EVENT_AXIS_HAT_Y,
    Ltrigger = ndk_sys::AMOTION_EVENT_AXIS_LTRIGGER,
    Rtrigger = ndk_sys::AMOTION_EVENT_AXIS_RTRIGGER,
    Throttle = ndk_sys::AMOTION_EVENT_AXIS_THROTTLE,
    Rudder = ndk_sys::AMOTION_EVENT_AXIS_RUDDER,
    Wheel = ndk_sys::AMOTION_EVENT_AXIS_WHEEL,
    Gas = ndk_sys::AMOTION_EVENT_AXIS_GAS,
    Brake = ndk_sys::AMOTION_EVENT_AXIS_BRAKE,
    Distance = ndk_sys::AMOTION_EVENT_AXIS_DISTANCE,
    Tilt = ndk_sys::AMOTION_EVENT_AXIS_TILT,
    Scroll = ndk_sys::AMOTION_EVENT_AXIS_SCROLL,
    RelativeX = ndk_sys::AMOTION_EVENT_AXIS_RELATIVE_X,
    RelativeY = ndk_sys::AMOTION_EVENT_AXIS_RELATIVE_Y,
    Generic1 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_1,
    Generic2 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_2,
    Generic3 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_3,
    Generic4 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_4,
    Generic5 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_5,
    Generic6 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_6,
    Generic7 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_7,
    Generic8 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_8,
    Generic9 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_9,
    Generic10 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_10,
    Generic11 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_11,
    Generic12 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_12,
    Generic13 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_13,
    Generic14 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_14,
    Generic15 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_15,
    Generic16 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_16,

    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(u32),
}

/// The tool type of a pointer.
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-48)
///
/// # Android Extensible Enum
///
/// This is a runtime [extensible enum](`crate#android-extensible-enums`) and
/// should be handled similar to a `#[non_exhaustive]` enum to maintain
/// forwards compatibility.
///
/// Implements `Into<u32>` and `From<u32>` for converting to/from Android SDK
/// integer values.
///
#[derive(Copy, Clone, Debug, PartialEq, Eq, num_enum::FromPrimitive, num_enum::IntoPrimitive)]
#[non_exhaustive]
#[repr(u32)]
pub enum ToolType {
    /// Unknown tool type.
    ///
    /// This constant is used when the tool type is not known or is not relevant, such as for a trackball or other non-pointing device.
    Unknown = ndk_sys::AMOTION_EVENT_TOOL_TYPE_UNKNOWN,

    /// The tool is a finger.
    Finger = ndk_sys::AMOTION_EVENT_TOOL_TYPE_FINGER,

    /// The tool is a stylus.
    Stylus = ndk_sys::AMOTION_EVENT_TOOL_TYPE_STYLUS,

    ///  The tool is a mouse.
    Mouse = ndk_sys::AMOTION_EVENT_TOOL_TYPE_MOUSE,

    /// The tool is an eraser or a stylus being used in an inverted posture.
    Eraser = ndk_sys::AMOTION_EVENT_TOOL_TYPE_ERASER,

    /// The tool is a palm and should be rejected
    Palm = ndk_sys::AMOTION_EVENT_TOOL_TYPE_PALM,

    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(u32),
}

/// A bitfield representing the state of buttons during a motion event.
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-33)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ButtonState(pub u32);

impl ButtonState {
    #[inline]
    pub fn primary(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_BUTTON_PRIMARY != 0
    }
    #[inline]
    pub fn secondary(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_BUTTON_SECONDARY != 0
    }
    #[inline]
    pub fn teriary(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_BUTTON_TERTIARY != 0
    }
    #[inline]
    pub fn back(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_BUTTON_BACK != 0
    }
    #[inline]
    pub fn forward(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_BUTTON_FORWARD != 0
    }
    #[inline]
    pub fn stylus_primary(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_BUTTON_STYLUS_PRIMARY != 0
    }
    #[inline]
    pub fn stylus_secondary(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_BUTTON_STYLUS_SECONDARY != 0
    }
}

impl From<ndk::event::ButtonState> for ButtonState {
    fn from(value: ndk::event::ButtonState) -> Self {
        Self(value.0)
    }
}

/// A bitfield representing which edges were touched by a motion event.
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-31)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EdgeFlags(pub u32);

impl EdgeFlags {
    #[inline]
    pub fn top(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_EDGE_FLAG_TOP != 0
    }
    #[inline]
    pub fn bottom(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_EDGE_FLAG_BOTTOM != 0
    }
    #[inline]
    pub fn left(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_EDGE_FLAG_LEFT != 0
    }
    #[inline]
    pub fn right(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_EDGE_FLAG_RIGHT != 0
    }
}

impl From<ndk::event::EdgeFlags> for EdgeFlags {
    fn from(value: ndk::event::EdgeFlags) -> Self {
        Self(value.0)
    }
}

/// Flags associated with this [`MotionEvent`].
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-30)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MotionEventFlags(pub u32);

impl MotionEventFlags {
    #[inline]
    pub fn window_is_obscured(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_FLAG_WINDOW_IS_OBSCURED != 0
    }
}

impl From<ndk::event::MotionEventFlags> for MotionEventFlags {
    fn from(value: ndk::event::MotionEventFlags) -> Self {
        Self(value.0)
    }
}

/// Key actions.
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-27)
///
/// # Android Extensible Enum
///
/// This is a runtime [extensible enum](`crate#android-extensible-enums`) and
/// should be handled similar to a `#[non_exhaustive]` enum to maintain
/// forwards compatibility.
///
/// Implements `Into<u32>` and `From<u32>` for converting to/from Android SDK
/// integer values.
///
#[derive(Copy, Clone, Debug, PartialEq, Eq, num_enum::FromPrimitive, num_enum::IntoPrimitive)]
#[non_exhaustive]
#[repr(u32)]
pub enum KeyAction {
    Down = ndk_sys::AKEY_EVENT_ACTION_DOWN,
    Up = ndk_sys::AKEY_EVENT_ACTION_UP,
    Multiple = ndk_sys::AKEY_EVENT_ACTION_MULTIPLE,

    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(u32),
}

/// Key codes.
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-39)
///
/// # Android Extensible Enum
///
/// This is a runtime [extensible enum](`crate#android-extensible-enums`) and
/// should be handled similar to a `#[non_exhaustive]` enum to maintain
/// forwards compatibility.
///
/// Implements `Into<u32>` and `From<u32>` for converting to/from Android SDK
/// integer values.
///
#[derive(Copy, Clone, Debug, PartialEq, Eq, num_enum::FromPrimitive, num_enum::IntoPrimitive)]
#[non_exhaustive]
#[repr(u32)]
pub enum Keycode {
    Unknown = ndk_sys::AKEYCODE_UNKNOWN,
    SoftLeft = ndk_sys::AKEYCODE_SOFT_LEFT,
    SoftRight = ndk_sys::AKEYCODE_SOFT_RIGHT,
    Home = ndk_sys::AKEYCODE_HOME,
    Back = ndk_sys::AKEYCODE_BACK,
    Call = ndk_sys::AKEYCODE_CALL,
    Endcall = ndk_sys::AKEYCODE_ENDCALL,
    Keycode0 = ndk_sys::AKEYCODE_0,
    Keycode1 = ndk_sys::AKEYCODE_1,
    Keycode2 = ndk_sys::AKEYCODE_2,
    Keycode3 = ndk_sys::AKEYCODE_3,
    Keycode4 = ndk_sys::AKEYCODE_4,
    Keycode5 = ndk_sys::AKEYCODE_5,
    Keycode6 = ndk_sys::AKEYCODE_6,
    Keycode7 = ndk_sys::AKEYCODE_7,
    Keycode8 = ndk_sys::AKEYCODE_8,
    Keycode9 = ndk_sys::AKEYCODE_9,
    Star = ndk_sys::AKEYCODE_STAR,
    Pound = ndk_sys::AKEYCODE_POUND,
    DpadUp = ndk_sys::AKEYCODE_DPAD_UP,
    DpadDown = ndk_sys::AKEYCODE_DPAD_DOWN,
    DpadLeft = ndk_sys::AKEYCODE_DPAD_LEFT,
    DpadRight = ndk_sys::AKEYCODE_DPAD_RIGHT,
    DpadCenter = ndk_sys::AKEYCODE_DPAD_CENTER,
    VolumeUp = ndk_sys::AKEYCODE_VOLUME_UP,
    VolumeDown = ndk_sys::AKEYCODE_VOLUME_DOWN,
    Power = ndk_sys::AKEYCODE_POWER,
    Camera = ndk_sys::AKEYCODE_CAMERA,
    Clear = ndk_sys::AKEYCODE_CLEAR,
    A = ndk_sys::AKEYCODE_A,
    B = ndk_sys::AKEYCODE_B,
    C = ndk_sys::AKEYCODE_C,
    D = ndk_sys::AKEYCODE_D,
    E = ndk_sys::AKEYCODE_E,
    F = ndk_sys::AKEYCODE_F,
    G = ndk_sys::AKEYCODE_G,
    H = ndk_sys::AKEYCODE_H,
    I = ndk_sys::AKEYCODE_I,
    J = ndk_sys::AKEYCODE_J,
    K = ndk_sys::AKEYCODE_K,
    L = ndk_sys::AKEYCODE_L,
    M = ndk_sys::AKEYCODE_M,
    N = ndk_sys::AKEYCODE_N,
    O = ndk_sys::AKEYCODE_O,
    P = ndk_sys::AKEYCODE_P,
    Q = ndk_sys::AKEYCODE_Q,
    R = ndk_sys::AKEYCODE_R,
    S = ndk_sys::AKEYCODE_S,
    T = ndk_sys::AKEYCODE_T,
    U = ndk_sys::AKEYCODE_U,
    V = ndk_sys::AKEYCODE_V,
    W = ndk_sys::AKEYCODE_W,
    X = ndk_sys::AKEYCODE_X,
    Y = ndk_sys::AKEYCODE_Y,
    Z = ndk_sys::AKEYCODE_Z,
    Comma = ndk_sys::AKEYCODE_COMMA,
    Period = ndk_sys::AKEYCODE_PERIOD,
    AltLeft = ndk_sys::AKEYCODE_ALT_LEFT,
    AltRight = ndk_sys::AKEYCODE_ALT_RIGHT,
    ShiftLeft = ndk_sys::AKEYCODE_SHIFT_LEFT,
    ShiftRight = ndk_sys::AKEYCODE_SHIFT_RIGHT,
    Tab = ndk_sys::AKEYCODE_TAB,
    Space = ndk_sys::AKEYCODE_SPACE,
    Sym = ndk_sys::AKEYCODE_SYM,
    Explorer = ndk_sys::AKEYCODE_EXPLORER,
    Envelope = ndk_sys::AKEYCODE_ENVELOPE,
    Enter = ndk_sys::AKEYCODE_ENTER,
    Del = ndk_sys::AKEYCODE_DEL,
    Grave = ndk_sys::AKEYCODE_GRAVE,
    Minus = ndk_sys::AKEYCODE_MINUS,
    Equals = ndk_sys::AKEYCODE_EQUALS,
    LeftBracket = ndk_sys::AKEYCODE_LEFT_BRACKET,
    RightBracket = ndk_sys::AKEYCODE_RIGHT_BRACKET,
    Backslash = ndk_sys::AKEYCODE_BACKSLASH,
    Semicolon = ndk_sys::AKEYCODE_SEMICOLON,
    Apostrophe = ndk_sys::AKEYCODE_APOSTROPHE,
    Slash = ndk_sys::AKEYCODE_SLASH,
    At = ndk_sys::AKEYCODE_AT,
    Num = ndk_sys::AKEYCODE_NUM,
    Headsethook = ndk_sys::AKEYCODE_HEADSETHOOK,
    Focus = ndk_sys::AKEYCODE_FOCUS,
    Plus = ndk_sys::AKEYCODE_PLUS,
    Menu = ndk_sys::AKEYCODE_MENU,
    Notification = ndk_sys::AKEYCODE_NOTIFICATION,
    Search = ndk_sys::AKEYCODE_SEARCH,
    MediaPlayPause = ndk_sys::AKEYCODE_MEDIA_PLAY_PAUSE,
    MediaStop = ndk_sys::AKEYCODE_MEDIA_STOP,
    MediaNext = ndk_sys::AKEYCODE_MEDIA_NEXT,
    MediaPrevious = ndk_sys::AKEYCODE_MEDIA_PREVIOUS,
    MediaRewind = ndk_sys::AKEYCODE_MEDIA_REWIND,
    MediaFastForward = ndk_sys::AKEYCODE_MEDIA_FAST_FORWARD,
    Mute = ndk_sys::AKEYCODE_MUTE,
    PageUp = ndk_sys::AKEYCODE_PAGE_UP,
    PageDown = ndk_sys::AKEYCODE_PAGE_DOWN,
    Pictsymbols = ndk_sys::AKEYCODE_PICTSYMBOLS,
    SwitchCharset = ndk_sys::AKEYCODE_SWITCH_CHARSET,
    ButtonA = ndk_sys::AKEYCODE_BUTTON_A,
    ButtonB = ndk_sys::AKEYCODE_BUTTON_B,
    ButtonC = ndk_sys::AKEYCODE_BUTTON_C,
    ButtonX = ndk_sys::AKEYCODE_BUTTON_X,
    ButtonY = ndk_sys::AKEYCODE_BUTTON_Y,
    ButtonZ = ndk_sys::AKEYCODE_BUTTON_Z,
    ButtonL1 = ndk_sys::AKEYCODE_BUTTON_L1,
    ButtonR1 = ndk_sys::AKEYCODE_BUTTON_R1,
    ButtonL2 = ndk_sys::AKEYCODE_BUTTON_L2,
    ButtonR2 = ndk_sys::AKEYCODE_BUTTON_R2,
    ButtonThumbl = ndk_sys::AKEYCODE_BUTTON_THUMBL,
    ButtonThumbr = ndk_sys::AKEYCODE_BUTTON_THUMBR,
    ButtonStart = ndk_sys::AKEYCODE_BUTTON_START,
    ButtonSelect = ndk_sys::AKEYCODE_BUTTON_SELECT,
    ButtonMode = ndk_sys::AKEYCODE_BUTTON_MODE,
    Escape = ndk_sys::AKEYCODE_ESCAPE,
    ForwardDel = ndk_sys::AKEYCODE_FORWARD_DEL,
    CtrlLeft = ndk_sys::AKEYCODE_CTRL_LEFT,
    CtrlRight = ndk_sys::AKEYCODE_CTRL_RIGHT,
    CapsLock = ndk_sys::AKEYCODE_CAPS_LOCK,
    ScrollLock = ndk_sys::AKEYCODE_SCROLL_LOCK,
    MetaLeft = ndk_sys::AKEYCODE_META_LEFT,
    MetaRight = ndk_sys::AKEYCODE_META_RIGHT,
    Function = ndk_sys::AKEYCODE_FUNCTION,
    Sysrq = ndk_sys::AKEYCODE_SYSRQ,
    Break = ndk_sys::AKEYCODE_BREAK,
    MoveHome = ndk_sys::AKEYCODE_MOVE_HOME,
    MoveEnd = ndk_sys::AKEYCODE_MOVE_END,
    Insert = ndk_sys::AKEYCODE_INSERT,
    Forward = ndk_sys::AKEYCODE_FORWARD,
    MediaPlay = ndk_sys::AKEYCODE_MEDIA_PLAY,
    MediaPause = ndk_sys::AKEYCODE_MEDIA_PAUSE,
    MediaClose = ndk_sys::AKEYCODE_MEDIA_CLOSE,
    MediaEject = ndk_sys::AKEYCODE_MEDIA_EJECT,
    MediaRecord = ndk_sys::AKEYCODE_MEDIA_RECORD,
    F1 = ndk_sys::AKEYCODE_F1,
    F2 = ndk_sys::AKEYCODE_F2,
    F3 = ndk_sys::AKEYCODE_F3,
    F4 = ndk_sys::AKEYCODE_F4,
    F5 = ndk_sys::AKEYCODE_F5,
    F6 = ndk_sys::AKEYCODE_F6,
    F7 = ndk_sys::AKEYCODE_F7,
    F8 = ndk_sys::AKEYCODE_F8,
    F9 = ndk_sys::AKEYCODE_F9,
    F10 = ndk_sys::AKEYCODE_F10,
    F11 = ndk_sys::AKEYCODE_F11,
    F12 = ndk_sys::AKEYCODE_F12,
    NumLock = ndk_sys::AKEYCODE_NUM_LOCK,
    Numpad0 = ndk_sys::AKEYCODE_NUMPAD_0,
    Numpad1 = ndk_sys::AKEYCODE_NUMPAD_1,
    Numpad2 = ndk_sys::AKEYCODE_NUMPAD_2,
    Numpad3 = ndk_sys::AKEYCODE_NUMPAD_3,
    Numpad4 = ndk_sys::AKEYCODE_NUMPAD_4,
    Numpad5 = ndk_sys::AKEYCODE_NUMPAD_5,
    Numpad6 = ndk_sys::AKEYCODE_NUMPAD_6,
    Numpad7 = ndk_sys::AKEYCODE_NUMPAD_7,
    Numpad8 = ndk_sys::AKEYCODE_NUMPAD_8,
    Numpad9 = ndk_sys::AKEYCODE_NUMPAD_9,
    NumpadDivide = ndk_sys::AKEYCODE_NUMPAD_DIVIDE,
    NumpadMultiply = ndk_sys::AKEYCODE_NUMPAD_MULTIPLY,
    NumpadSubtract = ndk_sys::AKEYCODE_NUMPAD_SUBTRACT,
    NumpadAdd = ndk_sys::AKEYCODE_NUMPAD_ADD,
    NumpadDot = ndk_sys::AKEYCODE_NUMPAD_DOT,
    NumpadComma = ndk_sys::AKEYCODE_NUMPAD_COMMA,
    NumpadEnter = ndk_sys::AKEYCODE_NUMPAD_ENTER,
    NumpadEquals = ndk_sys::AKEYCODE_NUMPAD_EQUALS,
    NumpadLeftParen = ndk_sys::AKEYCODE_NUMPAD_LEFT_PAREN,
    NumpadRightParen = ndk_sys::AKEYCODE_NUMPAD_RIGHT_PAREN,
    VolumeMute = ndk_sys::AKEYCODE_VOLUME_MUTE,
    Info = ndk_sys::AKEYCODE_INFO,
    ChannelUp = ndk_sys::AKEYCODE_CHANNEL_UP,
    ChannelDown = ndk_sys::AKEYCODE_CHANNEL_DOWN,
    ZoomIn = ndk_sys::AKEYCODE_ZOOM_IN,
    ZoomOut = ndk_sys::AKEYCODE_ZOOM_OUT,
    Tv = ndk_sys::AKEYCODE_TV,
    Window = ndk_sys::AKEYCODE_WINDOW,
    Guide = ndk_sys::AKEYCODE_GUIDE,
    Dvr = ndk_sys::AKEYCODE_DVR,
    Bookmark = ndk_sys::AKEYCODE_BOOKMARK,
    Captions = ndk_sys::AKEYCODE_CAPTIONS,
    Settings = ndk_sys::AKEYCODE_SETTINGS,
    TvPower = ndk_sys::AKEYCODE_TV_POWER,
    TvInput = ndk_sys::AKEYCODE_TV_INPUT,
    StbPower = ndk_sys::AKEYCODE_STB_POWER,
    StbInput = ndk_sys::AKEYCODE_STB_INPUT,
    AvrPower = ndk_sys::AKEYCODE_AVR_POWER,
    AvrInput = ndk_sys::AKEYCODE_AVR_INPUT,
    ProgRed = ndk_sys::AKEYCODE_PROG_RED,
    ProgGreen = ndk_sys::AKEYCODE_PROG_GREEN,
    ProgYellow = ndk_sys::AKEYCODE_PROG_YELLOW,
    ProgBlue = ndk_sys::AKEYCODE_PROG_BLUE,
    AppSwitch = ndk_sys::AKEYCODE_APP_SWITCH,
    Button1 = ndk_sys::AKEYCODE_BUTTON_1,
    Button2 = ndk_sys::AKEYCODE_BUTTON_2,
    Button3 = ndk_sys::AKEYCODE_BUTTON_3,
    Button4 = ndk_sys::AKEYCODE_BUTTON_4,
    Button5 = ndk_sys::AKEYCODE_BUTTON_5,
    Button6 = ndk_sys::AKEYCODE_BUTTON_6,
    Button7 = ndk_sys::AKEYCODE_BUTTON_7,
    Button8 = ndk_sys::AKEYCODE_BUTTON_8,
    Button9 = ndk_sys::AKEYCODE_BUTTON_9,
    Button10 = ndk_sys::AKEYCODE_BUTTON_10,
    Button11 = ndk_sys::AKEYCODE_BUTTON_11,
    Button12 = ndk_sys::AKEYCODE_BUTTON_12,
    Button13 = ndk_sys::AKEYCODE_BUTTON_13,
    Button14 = ndk_sys::AKEYCODE_BUTTON_14,
    Button15 = ndk_sys::AKEYCODE_BUTTON_15,
    Button16 = ndk_sys::AKEYCODE_BUTTON_16,
    LanguageSwitch = ndk_sys::AKEYCODE_LANGUAGE_SWITCH,
    MannerMode = ndk_sys::AKEYCODE_MANNER_MODE,
    Keycode3dMode = ndk_sys::AKEYCODE_3D_MODE,
    Contacts = ndk_sys::AKEYCODE_CONTACTS,
    Calendar = ndk_sys::AKEYCODE_CALENDAR,
    Music = ndk_sys::AKEYCODE_MUSIC,
    Calculator = ndk_sys::AKEYCODE_CALCULATOR,
    ZenkakuHankaku = ndk_sys::AKEYCODE_ZENKAKU_HANKAKU,
    Eisu = ndk_sys::AKEYCODE_EISU,
    Muhenkan = ndk_sys::AKEYCODE_MUHENKAN,
    Henkan = ndk_sys::AKEYCODE_HENKAN,
    KatakanaHiragana = ndk_sys::AKEYCODE_KATAKANA_HIRAGANA,
    Yen = ndk_sys::AKEYCODE_YEN,
    Ro = ndk_sys::AKEYCODE_RO,
    Kana = ndk_sys::AKEYCODE_KANA,
    Assist = ndk_sys::AKEYCODE_ASSIST,
    BrightnessDown = ndk_sys::AKEYCODE_BRIGHTNESS_DOWN,
    BrightnessUp = ndk_sys::AKEYCODE_BRIGHTNESS_UP,
    MediaAudioTrack = ndk_sys::AKEYCODE_MEDIA_AUDIO_TRACK,
    Sleep = ndk_sys::AKEYCODE_SLEEP,
    Wakeup = ndk_sys::AKEYCODE_WAKEUP,
    Pairing = ndk_sys::AKEYCODE_PAIRING,
    MediaTopMenu = ndk_sys::AKEYCODE_MEDIA_TOP_MENU,
    Keycode11 = ndk_sys::AKEYCODE_11,
    Keycode12 = ndk_sys::AKEYCODE_12,
    LastChannel = ndk_sys::AKEYCODE_LAST_CHANNEL,
    TvDataService = ndk_sys::AKEYCODE_TV_DATA_SERVICE,
    VoiceAssist = ndk_sys::AKEYCODE_VOICE_ASSIST,
    TvRadioService = ndk_sys::AKEYCODE_TV_RADIO_SERVICE,
    TvTeletext = ndk_sys::AKEYCODE_TV_TELETEXT,
    TvNumberEntry = ndk_sys::AKEYCODE_TV_NUMBER_ENTRY,
    TvTerrestrialAnalog = ndk_sys::AKEYCODE_TV_TERRESTRIAL_ANALOG,
    TvTerrestrialDigital = ndk_sys::AKEYCODE_TV_TERRESTRIAL_DIGITAL,
    TvSatellite = ndk_sys::AKEYCODE_TV_SATELLITE,
    TvSatelliteBs = ndk_sys::AKEYCODE_TV_SATELLITE_BS,
    TvSatelliteCs = ndk_sys::AKEYCODE_TV_SATELLITE_CS,
    TvSatelliteService = ndk_sys::AKEYCODE_TV_SATELLITE_SERVICE,
    TvNetwork = ndk_sys::AKEYCODE_TV_NETWORK,
    TvAntennaCable = ndk_sys::AKEYCODE_TV_ANTENNA_CABLE,
    TvInputHdmi1 = ndk_sys::AKEYCODE_TV_INPUT_HDMI_1,
    TvInputHdmi2 = ndk_sys::AKEYCODE_TV_INPUT_HDMI_2,
    TvInputHdmi3 = ndk_sys::AKEYCODE_TV_INPUT_HDMI_3,
    TvInputHdmi4 = ndk_sys::AKEYCODE_TV_INPUT_HDMI_4,
    TvInputComposite1 = ndk_sys::AKEYCODE_TV_INPUT_COMPOSITE_1,
    TvInputComposite2 = ndk_sys::AKEYCODE_TV_INPUT_COMPOSITE_2,
    TvInputComponent1 = ndk_sys::AKEYCODE_TV_INPUT_COMPONENT_1,
    TvInputComponent2 = ndk_sys::AKEYCODE_TV_INPUT_COMPONENT_2,
    TvInputVga1 = ndk_sys::AKEYCODE_TV_INPUT_VGA_1,
    TvAudioDescription = ndk_sys::AKEYCODE_TV_AUDIO_DESCRIPTION,
    TvAudioDescriptionMixUp = ndk_sys::AKEYCODE_TV_AUDIO_DESCRIPTION_MIX_UP,
    TvAudioDescriptionMixDown = ndk_sys::AKEYCODE_TV_AUDIO_DESCRIPTION_MIX_DOWN,
    TvZoomMode = ndk_sys::AKEYCODE_TV_ZOOM_MODE,
    TvContentsMenu = ndk_sys::AKEYCODE_TV_CONTENTS_MENU,
    TvMediaContextMenu = ndk_sys::AKEYCODE_TV_MEDIA_CONTEXT_MENU,
    TvTimerProgramming = ndk_sys::AKEYCODE_TV_TIMER_PROGRAMMING,
    Help = ndk_sys::AKEYCODE_HELP,
    NavigatePrevious = ndk_sys::AKEYCODE_NAVIGATE_PREVIOUS,
    NavigateNext = ndk_sys::AKEYCODE_NAVIGATE_NEXT,
    NavigateIn = ndk_sys::AKEYCODE_NAVIGATE_IN,
    NavigateOut = ndk_sys::AKEYCODE_NAVIGATE_OUT,
    StemPrimary = ndk_sys::AKEYCODE_STEM_PRIMARY,
    Stem1 = ndk_sys::AKEYCODE_STEM_1,
    Stem2 = ndk_sys::AKEYCODE_STEM_2,
    Stem3 = ndk_sys::AKEYCODE_STEM_3,
    DpadUpLeft = ndk_sys::AKEYCODE_DPAD_UP_LEFT,
    DpadDownLeft = ndk_sys::AKEYCODE_DPAD_DOWN_LEFT,
    DpadUpRight = ndk_sys::AKEYCODE_DPAD_UP_RIGHT,
    DpadDownRight = ndk_sys::AKEYCODE_DPAD_DOWN_RIGHT,
    MediaSkipForward = ndk_sys::AKEYCODE_MEDIA_SKIP_FORWARD,
    MediaSkipBackward = ndk_sys::AKEYCODE_MEDIA_SKIP_BACKWARD,
    MediaStepForward = ndk_sys::AKEYCODE_MEDIA_STEP_FORWARD,
    MediaStepBackward = ndk_sys::AKEYCODE_MEDIA_STEP_BACKWARD,
    SoftSleep = ndk_sys::AKEYCODE_SOFT_SLEEP,
    Cut = ndk_sys::AKEYCODE_CUT,
    Copy = ndk_sys::AKEYCODE_COPY,
    Paste = ndk_sys::AKEYCODE_PASTE,
    SystemNavigationUp = ndk_sys::AKEYCODE_SYSTEM_NAVIGATION_UP,
    SystemNavigationDown = ndk_sys::AKEYCODE_SYSTEM_NAVIGATION_DOWN,
    SystemNavigationLeft = ndk_sys::AKEYCODE_SYSTEM_NAVIGATION_LEFT,
    SystemNavigationRight = ndk_sys::AKEYCODE_SYSTEM_NAVIGATION_RIGHT,
    AllApps = ndk_sys::AKEYCODE_ALL_APPS,
    Refresh = ndk_sys::AKEYCODE_REFRESH,
    ThumbsUp = ndk_sys::AKEYCODE_THUMBS_UP,
    ThumbsDown = ndk_sys::AKEYCODE_THUMBS_DOWN,
    ProfileSwitch = ndk_sys::AKEYCODE_PROFILE_SWITCH,

    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(u32),
}

/// Flags associated with [`KeyEvent`].
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-28)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct KeyEventFlags(pub u32);

impl KeyEventFlags {
    #[inline]
    pub fn cancelled(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_CANCELED != 0
    }
    #[inline]
    pub fn cancelled_long_press(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_CANCELED_LONG_PRESS != 0
    }
    #[inline]
    pub fn editor_action(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_EDITOR_ACTION != 0
    }
    #[inline]
    pub fn fallback(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_FALLBACK != 0
    }
    #[inline]
    pub fn from_system(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_FROM_SYSTEM != 0
    }
    #[inline]
    pub fn keep_touch_mode(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_KEEP_TOUCH_MODE != 0
    }
    #[inline]
    pub fn long_press(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_LONG_PRESS != 0
    }
    #[inline]
    pub fn soft_keyboard(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_SOFT_KEYBOARD != 0
    }
    #[inline]
    pub fn tracking(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_TRACKING != 0
    }
    #[inline]
    pub fn virtual_hard_key(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_VIRTUAL_HARD_KEY != 0
    }
    #[inline]
    pub fn woke_here(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_WOKE_HERE != 0
    }
}

impl From<ndk::event::KeyEventFlags> for KeyEventFlags {
    fn from(value: ndk::event::KeyEventFlags) -> Self {
        Self(value.0)
    }
}

/// This struct holds a span within a region of text from `start` to `end`.
///
/// The `start` index may be greater than the `end` index (swapping `start` and `end` will represent the same span)
///
/// The lower index is inclusive and the higher index is exclusive.
///
/// An empty span or cursor position is specified with `start == end`.
///
#[derive(Debug, Clone, Copy)]
pub struct TextSpan {
    /// The start of the span (inclusive)
    pub start: usize,

    /// The end of the span (exclusive)
    pub end: usize,
}

#[derive(Debug, Clone)]
pub struct TextInputState {
    pub text: String,

    /// A selection defined on the text.
    ///
    /// To set the cursor position, start and end should have the same value.
    ///
    /// Changing the selection has no effect on the compose_region.
    pub selection: TextSpan,

    /// A composing region defined on the text.
    ///
    /// When being set, then if there was a composing region, the region is replaced.
    ///
    /// The given indices will be clamped to the `text` bounds
    ///
    /// If the resulting region is zero-sized, no region is marked (equivalent to passing `None`)
    pub compose_region: Option<TextSpan>,
}

impl Default for TextInputState {
    fn default() -> Self {
        Self {
            text: String::new(),
            selection: TextSpan { start: 0, end: 0 },
            compose_region: None,
        }
    }
}

// Represents the action button on a soft keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, num_enum::FromPrimitive, num_enum::IntoPrimitive)]
#[non_exhaustive]
#[repr(i32)]
pub enum TextInputAction {
    /// Let receiver decide what logical action to perform
    Unspecified = 0,
    /// No action - receiver could instead interpret as an "enter" key that inserts a newline character
    None = 1,
    /// Navigate to the input location (such as a URL)
    Go = 2,
    /// Search based on the input text
    Search = 3,
    /// Send the input to the target
    Send = 4,
    /// Move to the next input field
    Next = 5,
    /// Indicate that input is done
    Done = 6,
    /// Move to the previous input field
    Previous = 7,

    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(i32),
}

bitflags! {
    /// Flags for [`AndroidApp::set_ime_editor_info`]
    /// as per the [android.view.inputmethod.EditorInfo Java API](https://developer.android.com/reference/android/view/inputmethod/EditorInfo)
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct ImeOptions: u32 {

        /// The mask of bits that configure alternative actions for the "enter" key. This helps the
        /// IME provide clear feedback for what the key will do and provide alternative mechanisms
        /// for taking the same action.
        const IME_MASK_ACTION = 0x000000ff;

        /// Indicates that ascii input is a priority (such as for entering an account ID)
        const IME_FLAG_FORCE_ASCII = 0x80000000;

        /// Indicates that it's possible to navigate focus forwards to something.
        ///
        /// This is similar to using `IME_ACTION_NEXT` except it allows for multi-line input with
        /// an enter key in addition to forward navigation for focus.
        ///
        /// This may not be supported by all IMEs (especially on small screens)
        const IME_FLAG_NAVIGATE_NEXT = 0x08000000;

        /// Similar to `IME_FLAG_NAVIGATE_NEXT`, except it indicates that it's possible to navigate
        /// focus backwards to something.
        const IME_FLAG_NAVIGATE_PREVIOUS = 0x04000000;

        /// This requests that the IME should not show any accessory actions next to the extracted
        /// text UI, when it is in fullscreen mode.
        ///
        /// The implication is that you think it's more important to prioritize having room for
        /// previewing more text, instead of showing accessory actions.
        ///
        /// Note: In some cases this can make the action unavailable.
        const IME_FLAG_NO_ACCESSORY_ACTION = 0x20000000;

        /// If this flag is not set, IMEs will normally replace the "enter" key with the action
        /// supplied. This flag indicates that the action should not be available in-line as a
        /// replacement for the "enter" key. Typically this is because the action has such a
        /// significant impact or is not recoverable enough that accidentally hitting it should be
        /// avoided, such as sending a message.
        const IME_FLAG_NO_ENTER_ACTION = 0x40000000;

        /// Don't show any "extracted-text UI" as part of the on-screen IME.
        ///
        /// Some keyboards may show an additional text box above the keyboard for previewing what
        /// you type (referred to as the extracted text UI) and it can sometimes be quite large.
        ///
        /// The exact semantics of this flag can be unclear sometimes and the UI that becomes
        /// visible may not respond to input as you would expect.
        ///
        /// This flag may be deprecated in the future and it's recommend to use
        /// `IME_FLAG_NO_FULLSCREEN` instead, to avoid having the extracted text UI appear to cover
        /// the full screen.
        const IMG_FLAG_NO_EXTRACT_UI = 0x10000000;

        /// Request that the IME should avoid ever entering a fullscreen mode and should always
        /// leave some room for the application UI.
        ///
        /// Note: It's not guaranteed that an IME will honor this state
        const IME_FLAG_NO_FULLSCREEN = 0x02000000;

        /// Request that the IME should not update personalized data, such as typing history.
        ///
        /// Note: It's not guaranteed that an IME will honor this state
        const IME_FLAG_NO_PERSONALIZED_LEARNING = 0x01000000;

        /// Generic unspecified type for ImeOptions
        const IME_NULL = 0;
    }
}

impl ImeOptions {
    /// Specify what action the IME's "enter" key should perform.
    ///
    /// This helps the IME provide clear feedback for what the key will do and provide alternative
    /// mechanisms for taking the same action.
    pub fn set_action(&mut self, action: TextInputAction) {
        let action: i32 = action.into();
        let action = action as u32;
        *self = Self::from_bits_truncate(
            (self.bits() & !Self::IME_MASK_ACTION.bits()) | (action & Self::IME_MASK_ACTION.bits()),
        );
    }

    /// Get the current action of the IME's "enter" key.
    pub fn action(&self) -> TextInputAction {
        let action_bits = self.bits() & Self::IME_MASK_ACTION.bits();
        TextInputAction::from(action_bits as i32)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, num_enum::FromPrimitive, num_enum::IntoPrimitive)]
#[non_exhaustive]
#[repr(u32)]
pub enum InputTypeClass {
    /// Special content type for when no explicit type has been specified.
    ///
    /// This should be interpreted to mean that the target input connection is
    /// not rich, it can not process and show things like candidate text nor
    /// retrieve the current text, so the input method will need to run in a
    /// limited "generate key events" mode, if it supports it.
    ///
    /// Note that some input methods may not support it, for example a
    /// voice-based input method will likely not be able to generate key events
    /// even if this flag is set.
    Null = 0,

    ///  Class for normal text.
    ///
    /// This class supports the following flags (only one of which should be set):
    /// - TYPE_TEXT_FLAG_CAP_CHARACTERS
    /// - TYPE_TEXT_FLAG_CAP_WORDS
    /// - TYPE_TEXT_FLAG_CAP_SENTENCES.
    ///
    /// It also supports the following variations:
    /// - TYPE_TEXT_VARIATION_NORMAL
    /// - TYPE_TEXT_VARIATION_URI
    ///
    /// *If you do not recognize the variation, normal should be assumed.*
    Text = 1,

    /// Class for numeric text.
    ///
    /// This class supports the following flags:
    /// - `TYPE_NUMBER_FLAG_SIGNED`
    /// - `TYPE_NUMBER_FLAG_DECIMAL`
    ///
    /// It also supports the following variations:
    /// - `TYPE_NUMBER_VARIATION_NORMAL`
    /// - `TYPE_NUMBER_VARIATION_PASSWORD`
    ///
    /// *IME authors: If you do not recognize the variation, normal should be assumed.*
    Number = 2,

    ///  Class for a phone number.
    ///
    /// This class currently supports no variations or flags.
    Phone = 3,

    ///  Class for dates and times.
    ///
    /// It supports the following variations:
    /// - TYPE_DATETIME_VARIATION_NORMAL
    /// - TYPE_DATETIME_VARIATION_DATE
    /// - TYPE_DATETIME_VARIATION_TIME
    DateTime = 4,

    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(u32),
}

bitflags! {
    /// Flags specifying the content type of text being input.
    ///
    /// Corresponds to the Android SDK [InputType](https://developer.android.com/reference/android/text/InputType) API
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct InputType: u32 {
        ///  Mask of bits that determine the overall class of text being given. Currently
        ///  supported classes are: TYPE_CLASS_TEXT, TYPE_CLASS_NUMBER, TYPE_CLASS_PHONE,
        ///  TYPE_CLASS_DATETIME. IME authors: If the class is not one you understand, assume
        ///  TYPE_CLASS_TEXT with NO variation or flags.
        const TYPE_MASK_CLASS = 0x0000000f;

        ///  Mask of bits that determine the variation of the base content class.
        const TYPE_MASK_VARIATION = 0x00000ff0;

        ///  Mask of bits that provide addition bit flags of options.
        const TYPE_MASK_FLAGS = 0x00fff000;

        ///  Special content type for when no explicit type has been specified. This should be
        ///  interpreted to mean that the target input connection is not rich, it can not process
        ///  and show things like candidate text nor retrieve the current text, so the input
        ///  method will need to run in a limited "generate key events" mode, if it supports
        ///  it. Note that some input methods may not support it, for example a voice-based
        ///  input method will likely not be able to generate key events even if this flag is
        ///  set.
        const TYPE_NULL = 0;

        ///  Class for normal text. This class supports the following flags (only one of which
        ///  should be set): TYPE_TEXT_FLAG_CAP_CHARACTERS, TYPE_TEXT_FLAG_CAP_WORDS, and.
        ///  TYPE_TEXT_FLAG_CAP_SENTENCES.  It also supports the following variations:
        ///  TYPE_TEXT_VARIATION_NORMAL, and TYPE_TEXT_VARIATION_URI. If you do not recognize the
        ///  variation, normal should be assumed.
        const TYPE_CLASS_TEXT = 1;

        ///  Flag for TYPE_CLASS_TEXT: capitalize all characters.  Overrides
        ///  #TYPE_TEXT_FLAG_CAP_WORDS} and  #TYPE_TEXT_FLAG_CAP_SENTENCES}.  This value is
        ///  explicitly defined to be the same as  TextUtils#CAP_MODE_CHARACTERS}. Of
        ///  course, this only affects languages where there are upper-case and lower-case
        ///  letters.
        const TYPE_TEXT_FLAG_CAP_CHARACTERS = 0x00001000;

        ///  Flag for TYPE_CLASS_TEXT: capitalize the first character of every word.
        ///  Overrides TYPE_TEXT_FLAG_CAP_SENTENCES.  This value is explicitly defined
        ///  to be the same as TextUtils#CAP_MODE_WORDS. Of course, this only affects
        ///  languages where there are upper-case and lower-case letters.
        const TYPE_TEXT_FLAG_CAP_WORDS = 0x00002000;

        ///  Flag for TYPE_CLASS_TEXT: capitalize the first character of each sentence.  This value
        ///  is explicitly defined to be the same as TextUtils#CAP_MODE_SENTENCES. For example in
        ///  English it means to capitalize after a period and a space (note that other languages
        ///  may have different characters for period, or not use spaces, or use different
        ///  grammatical rules). Of course, this only affects languages where there are upper-case
        ///  and lower-case letters.
        const TYPE_TEXT_FLAG_CAP_SENTENCES = 0x00004000;

        ///  Flag for TYPE_CLASS_TEXT: the user is entering free-form text that should have
        ///  auto-correction applied to it. Without this flag, the IME will not try to correct
        ///  typos. You should always set this flag unless you really expect users to type
        ///  non-words in this field, for example to choose a name for a character in a game.
        ///  Contrast this with  TYPE_TEXT_FLAG_AUTO_COMPLETE and TYPE_TEXT_FLAG_NO_SUGGESTIONS:
        ///  TYPE_TEXT_FLAG_AUTO_CORRECT means that the IME will try to auto-correct typos as the
        ///  user is typing, but does not define whether the IME offers an interface to show
        ///  suggestions.
        const TYPE_TEXT_FLAG_AUTO_CORRECT = 0x00008000;

        ///  Flag for TYPE_CLASS_TEXT: the text editor (which means the application) is performing
        ///  auto-completion of the text being entered based on its own semantics, which it will
        ///  present to the user as they type. This generally means that the input method should
        ///  not be showing candidates itself, but can expect the editor to supply its own
        ///  completions/candidates from
        ///  android.view.inputmethod.InputMethodSession#displayCompletions
        ///  InputMethodSession.displayCompletions()} as a result of the editor calling
        ///  android.view.inputmethod.InputMethodManager#displayCompletions
        ///  InputMethodManager.displayCompletions()}. Note the contrast with
        ///  TYPE_TEXT_FLAG_AUTO_CORRECT and  TYPE_TEXT_FLAG_NO_SUGGESTIONS:
        ///  TYPE_TEXT_FLAG_AUTO_COMPLETE means the editor should show an interface for displaying
        ///  suggestions, but instead of supplying its own it will rely on the Editor to pass
        ///  completions/corrections.
        const TYPE_TEXT_FLAG_AUTO_COMPLETE = 0x00010000;

        ///  Flag for TYPE_CLASS_TEXT: multiple lines of text can be entered into the
        ///  field.  If this flag is not set, the text field will be constrained to a single
        ///  line. The IME may also choose not to display an enter key when this flag is not set,
        ///  as there should be no need to create new lines.
        const TYPE_TEXT_FLAG_MULTI_LINE = 0x00020000;

        ///  Flag for TYPE_CLASS_TEXT: the regular text view associated with this should
        ///  not be multi-line, but when a fullscreen input method is providing text it should
        ///  use multiple lines if it can.
        const TYPE_TEXT_FLAG_IME_MULTI_LINE = 0x00040000;

        ///  Flag for TYPE_CLASS_TEXT: the input method does not need to display any
        ///  dictionary-based candidates. This is useful for text views that do not contain words
        ///  from the language and do not benefit from any dictionary-based completions or
        ///  corrections. It overrides the TYPE_TEXT_FLAG_AUTO_CORRECT value when set.  Please
        ///  avoid using this unless you are certain this is what you want. Many input methods need
        ///  suggestions to work well, for example the ones based on gesture typing.  Consider
        ///  clearing TYPE_TEXT_FLAG_AUTO_CORRECT instead if you just do not want the IME to
        ///  correct typos. Note the contrast with TYPE_TEXT_FLAG_AUTO_CORRECT and
        ///  TYPE_TEXT_FLAG_AUTO_COMPLETE: TYPE_TEXT_FLAG_NO_SUGGESTIONS means the IME does not
        ///  need to show an interface to display suggestions. Most IMEs will also take this to
        ///  mean they do not need to try to auto-correct what the user is typing.
        const TYPE_TEXT_FLAG_NO_SUGGESTIONS = 0x00080000;

        ///  Flag for TYPE_CLASS_TEXT: Let the IME know the text conversion suggestions are
        ///  required by the application. Text conversion suggestion is for the transliteration
        ///  languages which has pronunciation characters and target characters.  When the user is
        ///  typing the pronunciation charactes, the IME could provide the possible target
        ///  characters to the user. When this flag is set, the IME should insert the text
        ///  conversion suggestions through  Builder#setTextConversionSuggestions(List)} and the
        ///  TextAttribute} with initialized with the text conversion suggestions is provided by
        ///  the IME to the application. To receive the additional information, the application
        ///  needs to implement  InputConnection#setComposingText(CharSequence, int,
        ///  TextAttribute)},  InputConnection#setComposingRegion(int, int, TextAttribute)}, and
        ///  InputConnection#commitText(CharSequence, int, TextAttribute)}.
        const TYPE_TEXT_FLAG_ENABLE_TEXT_CONVERSION_SUGGESTIONS = 0x00100000;

        /// Flag for TYPE_CLASS_TEXT: Let the IME know that conversion candidate selection
        /// information is requested by the application. Text conversion suggestion is for the
        /// transliteration languages, which have the notions of pronunciation and target
        /// characters. When the user actively selects a candidate from the conversion suggestions,
        /// notifying when candidate selection is occurring helps assistive technologies generate
        /// more effective feedback. When this flag is set, and there is an active selected
        /// suggestion, the IME should set that a conversion suggestion is selected when
        /// initializing the TextAttribute. To receive this information, the application should
        /// implement InputConnection.setComposingText(CharSequence, int, TextAttribute),
        /// InputConnection.setComposingRegion(int, int, TextAttribute), and
        /// InputConnection.commitText(CharSequence, int, TextAttribute)
        const TYPE_TEXT_FLAG_ENABLE_TEXT_SUGGESTION_SELECTED = 0x00200000;

        ///  Default variation of TYPE_CLASS_TEXT: plain old normal text.
        const TYPE_TEXT_VARIATION_NORMAL = 0;
        ///  Variation of TYPE_CLASS_TEXT: entering a URI.
        const TYPE_TEXT_VARIATION_URI = 0x00000010;
        ///  Variation of TYPE_CLASS_TEXT: entering an e-mail address.
        const TYPE_TEXT_VARIATION_EMAIL_ADDRESS = 0x00000020;
        ///  Variation of TYPE_CLASS_TEXT: entering the subject line of an e-mail.
        const TYPE_TEXT_VARIATION_EMAIL_SUBJECT = 0x00000030;
        ///  Variation of TYPE_CLASS_TEXT: entering a short, possibly informal message such as an instant message or a text message.
        const TYPE_TEXT_VARIATION_SHORT_MESSAGE = 64;
        ///  Variation of TYPE_CLASS_TEXT: entering the content of a long, possibly formal message such as the body of an e-mail.
        const TYPE_TEXT_VARIATION_LONG_MESSAGE = 0x00000050;
        ///  Variation of TYPE_CLASS_TEXT: entering the name of a person.
        const TYPE_TEXT_VARIATION_PERSON_NAME = 0x00000060;
        ///  Variation of TYPE_CLASS_TEXT: entering a postal mailing address.
        const TYPE_TEXT_VARIATION_POSTAL_ADDRESS = 0x00000070;
        ///  Variation of TYPE_CLASS_TEXT: entering a password.
        const TYPE_TEXT_VARIATION_PASSWORD = 0x00000080;
        ///  Variation of TYPE_CLASS_TEXT: entering a password, which should be visible to the user.
        const TYPE_TEXT_VARIATION_VISIBLE_PASSWORD = 0x00000090;
        ///  Variation of TYPE_CLASS_TEXT: entering text inside of a web form.
        const TYPE_TEXT_VARIATION_WEB_EDIT_TEXT = 0x000000a0;
        ///  Variation of TYPE_CLASS_TEXT: entering text to filter contents of a list etc.
        const TYPE_TEXT_VARIATION_FILTER = 0x000000b0;
        ///  Variation of TYPE_CLASS_TEXT: entering text for phonetic pronunciation, such as a
        ///  phonetic name field in contacts. This is mostly useful for languages where one
        ///  spelling may have several phonetic readings, like Japanese.
        const TYPE_TEXT_VARIATION_PHONETIC = 0x000000c0;
        ///  Variation of TYPE_CLASS_TEXT: entering e-mail address inside of a web form.  This
        ///  was added in  android.os.Build.VERSION_CODES#HONEYCOMB}.  An IME must target this API
        ///  version or later to see this input type; if it doesn't, a request for this type will
        ///  be seen as  #TYPE_TEXT_VARIATION_EMAIL_ADDRESS} when passed through
        ///  android.view.inputmethod.EditorInfo#makeCompatible(int)
        ///  EditorInfo.makeCompatible(int)}.
        const TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS = 0x000000d0;
        ///  Variation of TYPE_CLASS_TEXT: entering password inside of a web form.  This was
        ///  added in  android.os.Build.VERSION_CODES#HONEYCOMB}.  An IME must target this API
        ///  version or later to see this input type; if it doesn't, a request for this type will
        ///  be seen as  #TYPE_TEXT_VARIATION_PASSWORD} when passed through
        ///  android.view.inputmethod.EditorInfo#makeCompatible(int)
        ///  EditorInfo.makeCompatible(int)}.
        const TYPE_TEXT_VARIATION_WEB_PASSWORD = 0x000000e0;
        ///  Class for numeric text.  This class supports the following flags:
        ///  #TYPE_NUMBER_FLAG_SIGNED} and  #TYPE_NUMBER_FLAG_DECIMAL}.  It also supports the
        ///  following variations:  #TYPE_NUMBER_VARIATION_NORMAL} and
        ///  #TYPE_NUMBER_VARIATION_PASSWORD}. <p>IME authors: If you do not recognize the
        ///  variation, normal should be assumed.</p>
        const TYPE_CLASS_NUMBER = 2;
        ///  Flag of TYPE_CLASS_NUMBER: the number is signed, allowing a positive or negative
        ///  sign at the start.
        const TYPE_NUMBER_FLAG_SIGNED = 0x00001000;
        ///  Flag of TYPE_CLASS_NUMBER: the number is decimal, allowing a decimal point to
        ///  provide fractional values.
        const TYPE_NUMBER_FLAG_DECIMAL = 0x00002000;
        ///  Default variation of TYPE_CLASS_NUMBER: plain normal numeric text.  This was added
        ///  in  android.os.Build.VERSION_CODES#HONEYCOMB}.  An IME must target this API version or
        ///  later to see this input type; if it doesn't, a request for this type will be dropped
        ///  when passed through  android.view.inputmethod.EditorInfo#makeCompatible(int)
        ///  EditorInfo.makeCompatible(int)}.
        const TYPE_NUMBER_VARIATION_NORMAL = 0;
        ///  Variation of TYPE_CLASS_NUMBER: entering a numeric password. This was added in
        ///  android.os.Build.VERSION_CODES#HONEYCOMB}.  An IME must target this API version or
        ///  later to see this input type; if it doesn't, a request for this type will be dropped
        ///  when passed through  android.view.inputmethod.EditorInfo#makeCompatible(int)
        ///  EditorInfo.makeCompatible(int)}.
        const TYPE_NUMBER_VARIATION_PASSWORD = 0x00000010;
        ///  Class for a phone number.  This class currently supports no variations or flags.
        const TYPE_CLASS_PHONE = 3;
        ///  Class for dates and times.  It supports the following variations:
        ///  #TYPE_DATETIME_VARIATION_NORMAL}  #TYPE_DATETIME_VARIATION_DATE}, and
        ///  #TYPE_DATETIME_VARIATION_TIME}.
        const TYPE_CLASS_DATETIME = 4;
        ///  Default variation of  #TYPE_CLASS_DATETIME}: allows entering both a date and time.
        const TYPE_DATETIME_VARIATION_NORMAL = 0;
        ///  Default variation of  #TYPE_CLASS_DATETIME}: allows entering only a date.
        const TYPE_DATETIME_VARIATION_DATE = 16;
        ///  Default variation of  #TYPE_CLASS_DATETIME}: allows entering only a time.
        const TYPE_DATETIME_VARIATION_TIME = 32;

    }
}

impl InputType {
    /// Extract just the class of the input type.
    pub fn class(&self) -> InputTypeClass {
        let class = self.bits() & InputType::TYPE_MASK_CLASS.bits();
        InputTypeClass::from(class)
    }
}

/// An exclusive, lending iterator for input events
pub struct InputIterator<'a> {
    pub(crate) inner: crate::activity_impl::InputIteratorInner<'a>,
}

impl InputIterator<'_> {
    /// Reads and handles the next input event by passing it to the given `callback`
    ///
    /// `callback` should return [`InputStatus::Unhandled`] for any input events that aren't directly
    /// handled by the application, or else [`InputStatus::Handled`]. Unhandled events may lead to a
    /// fallback interpretation of the event.
    pub fn next<F>(&mut self, callback: F) -> bool
    where
        F: FnOnce(&crate::activity_impl::input::InputEvent) -> InputStatus,
    {
        self.inner.next(callback)
    }
}

/// A view into the data of a specific pointer in a motion event.
#[derive(Debug)]
pub struct Pointer<'a> {
    pub(crate) inner: PointerImpl<'a>,
}

impl Pointer<'_> {
    #[inline]
    pub fn pointer_index(&self) -> usize {
        self.inner.pointer_index()
    }

    #[inline]
    pub fn pointer_id(&self) -> i32 {
        self.inner.pointer_id()
    }

    #[inline]
    pub fn axis_value(&self, axis: Axis) -> f32 {
        self.inner.axis_value(axis)
    }

    #[inline]
    pub fn orientation(&self) -> f32 {
        self.axis_value(Axis::Orientation)
    }

    #[inline]
    pub fn pressure(&self) -> f32 {
        self.axis_value(Axis::Pressure)
    }

    #[inline]
    pub fn raw_x(&self) -> f32 {
        self.inner.raw_x()
    }

    #[inline]
    pub fn raw_y(&self) -> f32 {
        self.inner.raw_y()
    }

    #[inline]
    pub fn x(&self) -> f32 {
        self.axis_value(Axis::X)
    }

    #[inline]
    pub fn y(&self) -> f32 {
        self.axis_value(Axis::Y)
    }

    #[inline]
    pub fn size(&self) -> f32 {
        self.axis_value(Axis::Size)
    }

    #[inline]
    pub fn tool_major(&self) -> f32 {
        self.axis_value(Axis::ToolMajor)
    }

    #[inline]
    pub fn tool_minor(&self) -> f32 {
        self.axis_value(Axis::ToolMinor)
    }

    #[inline]
    pub fn touch_major(&self) -> f32 {
        self.axis_value(Axis::TouchMajor)
    }

    #[inline]
    pub fn touch_minor(&self) -> f32 {
        self.axis_value(Axis::TouchMinor)
    }

    #[inline]
    pub fn tool_type(&self) -> ToolType {
        self.inner.tool_type()
    }

    /// Gives access to the historical data of a pointer in a [`MotionEvent`].
    ///
    /// This provides access to higher-frequency data points that were recorded
    /// between the current event and the previous event, which can be used for
    /// more accurate gesture detection and smoother animations.
    ///
    /// For a single [`MotionEvent`] each pointer will have the same number of
    /// historical events, and the corresponding historical events will have the
    /// same timestamps.
    #[inline]
    pub fn history(&self) -> PointerHistoryIter<'_> {
        self.inner.history()
    }
}

/// An iterator over the pointers in a [`MotionEvent`].
#[derive(Debug)]
pub struct PointersIter<'a> {
    pub(crate) inner: PointersIterImpl<'a>,
}

impl<'a> Iterator for PointersIter<'a> {
    type Item = Pointer<'a>;
    fn next(&mut self) -> Option<Pointer<'a>> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for PointersIter<'_> {}

/// An iterator over the historical data of a pointer in a [`MotionEvent`].
///
/// This provides access to higher-frequency data points that were recorded
/// between the current event and the previous event, which can be used for more
/// accurate gesture detection and smoother animations.
///
/// For a single [`MotionEvent`] each pointer will have the same number of
/// historical events, and the corresponding historical events will have the
/// same timestamps.
///
#[derive(Debug)]
pub struct PointerHistoryIter<'a> {
    pub(crate) inner: PointerHistoryIterImpl<'a>,
}

impl<'a> Iterator for PointerHistoryIter<'a> {
    type Item = HistoricalPointer<'a>;
    fn next(&mut self) -> Option<HistoricalPointer<'a>> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}
impl<'a> DoubleEndedIterator for PointerHistoryIter<'a> {
    fn next_back(&mut self) -> Option<HistoricalPointer<'a>> {
        self.inner.next_back()
    }
}
impl ExactSizeIterator for PointerHistoryIter<'_> {}
impl FusedIterator for PointerHistoryIter<'_> {}

pub struct HistoricalPointer<'a> {
    pub(crate) inner: HistoricalPointerImpl<'a>,
}

impl HistoricalPointer<'_> {
    #[inline]
    pub fn history_index(&self) -> usize {
        self.inner.history_index()
    }

    #[inline]
    pub fn pointer_index(&self) -> usize {
        self.inner.pointer_index()
    }

    #[inline]
    pub fn event_time(&self) -> i64 {
        self.inner.event_time()
    }

    #[inline]
    pub fn axis_value(&self, axis: Axis) -> f32 {
        self.inner.axis_value(axis)
    }

    #[inline]
    pub fn orientation(&self) -> f32 {
        self.axis_value(Axis::Orientation)
    }

    #[inline]
    pub fn pressure(&self) -> f32 {
        self.axis_value(Axis::Pressure)
    }

    #[inline]
    pub fn x(&self) -> f32 {
        self.axis_value(Axis::X)
    }

    #[inline]
    pub fn y(&self) -> f32 {
        self.axis_value(Axis::Y)
    }

    #[inline]
    pub fn size(&self) -> f32 {
        self.axis_value(Axis::Size)
    }

    #[inline]
    pub fn tool_major(&self) -> f32 {
        self.axis_value(Axis::ToolMajor)
    }

    #[inline]
    pub fn tool_minor(&self) -> f32 {
        self.axis_value(Axis::ToolMinor)
    }

    #[inline]
    pub fn touch_major(&self) -> f32 {
        self.axis_value(Axis::TouchMajor)
    }

    #[inline]
    pub fn touch_minor(&self) -> f32 {
        self.axis_value(Axis::TouchMinor)
    }
}
