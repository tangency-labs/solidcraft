use jni::sys::jint;
use jni::{objects::Global, JavaVM};

use crate::error::{AppError, InternalAppError, InternalResult};
use crate::input::{Keycode, MetaState};
use crate::jni_utils;

/// An enum representing the types of keyboards that may generate key events
///
/// See [getKeyboardType() docs](https://developer.android.com/reference/android/view/KeyCharacterMap#getKeyboardType())
///
/// # Android Extensible Enum
///
/// This is a runtime [extensible enum](`crate#android-extensible-enums`) and
/// should be handled similar to a `#[non_exhaustive]` enum to maintain
/// forwards compatibility.
///
/// This implements `Into<u32>` and `From<u32>` for converting to/from Android
/// SDK integer values.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, num_enum::FromPrimitive, num_enum::IntoPrimitive,
)]
#[non_exhaustive]
#[repr(u32)]
pub enum KeyboardType {
    /// A numeric (12-key) keyboard.
    ///
    /// A numeric keyboard supports text entry using a multi-tap approach. It may be necessary to tap a key multiple times to generate the desired letter or symbol.
    ///
    /// This type of keyboard is generally designed for thumb typing.
    Numeric,

    /// A keyboard with all the letters, but with more than one letter per key.
    ///
    /// This type of keyboard is generally designed for thumb typing.
    Predictive,

    /// A keyboard with all the letters, and maybe some numbers.
    ///
    /// An alphabetic keyboard supports text entry directly but may have a condensed layout with a small form factor. In contrast to a full keyboard, some symbols may only be accessible using special on-screen character pickers. In addition, to improve typing speed and accuracy, the framework provides special affordances for alphabetic keyboards such as auto-capitalization and toggled / locked shift and alt keys.
    ///
    /// This type of keyboard is generally designed for thumb typing.
    Alpha,

    /// A full PC-style keyboard.
    ///
    /// A full keyboard behaves like a PC keyboard. All symbols are accessed directly by pressing keys on the keyboard without on-screen support or affordances such as auto-capitalization.
    ///
    /// This type of keyboard is generally designed for full two hand typing.
    Full,

    /// A keyboard that is only used to control special functions rather than for typing.
    ///
    /// A special function keyboard consists only of non-printing keys such as HOME and POWER that are not actually used for typing.
    SpecialFunction,

    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(u32),
}

/// Either represents, a unicode character or combining accent from a
/// [`KeyCharacterMap`], or `None` for non-printable keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyMapChar {
    None,
    Unicode(char),
    CombiningAccent(char),
}

jni::bind_java_type! {
    pub(crate) AKeyCharacterMap => "android.view.KeyCharacterMap",
    methods {
        priv fn _get(key_code: jint, meta_state: jint) -> jint,
        priv static fn _get_dead_char(accent_char: jint, base_char: jint) -> jint,
        priv fn _get_keyboard_type() -> jint,
    }
}

impl AKeyCharacterMap<'_> {
    pub(crate) fn get(
        &self,
        env: &mut jni::Env,
        key_code: jint,
        meta_state: jint,
    ) -> Result<jint, InternalAppError> {
        self._get(env, key_code, meta_state)
            .map_err(|err| jni_utils::clear_and_map_exception_to_err(env, err))
    }

    pub(crate) fn get_dead_char(
        env: &mut jni::Env,
        accent_char: jint,
        base_char: jint,
    ) -> Result<jint, InternalAppError> {
        Self::_get_dead_char(env, accent_char, base_char)
            .map_err(|err| jni_utils::clear_and_map_exception_to_err(env, err))
    }

    pub(crate) fn get_keyboard_type(&self, env: &mut jni::Env) -> Result<jint, InternalAppError> {
        self._get_keyboard_type(env)
            .map_err(|err| jni_utils::clear_and_map_exception_to_err(env, err))
    }
}

jni::bind_java_type! {
    pub(crate) AInputDevice => "android.view.InputDevice",
    type_map {
        AKeyCharacterMap => "android.view.KeyCharacterMap",
    },
    methods {
        static fn get_device(id: jint) -> AInputDevice,
        fn get_key_character_map() -> AKeyCharacterMap,
    }
}

/// Describes the keys provided by a keyboard device and their associated labels.
#[derive(Debug)]
pub struct KeyCharacterMap {
    jvm: JavaVM,
    key_map: Global<AKeyCharacterMap<'static>>,
}
impl Clone for KeyCharacterMap {
    fn clone(&self) -> Self {
        let jvm = self.jvm.clone();
        jvm.attach_current_thread(|env| -> jni::errors::Result<_> {
            Ok(Self {
                jvm: jvm.clone(),
                key_map: env.new_global_ref(&self.key_map)?,
            })
        })
        .expect("Failed to attach thread to JVM and clone key map")
    }
}

impl KeyCharacterMap {
    pub(crate) fn new(jvm: JavaVM, key_map: Global<AKeyCharacterMap<'static>>) -> Self {
        Self { jvm, key_map }
    }

    /// Gets the Unicode character generated by the specified [`Keycode`] and [`MetaState`] combination.
    ///
    /// Returns [`KeyMapChar::None`] if the key is not one that is used to type Unicode characters.
    ///
    /// Returns [`KeyMapChar::CombiningAccent`] if the key is a "dead key" that should be combined with
    /// another to actually produce a character -- see [`KeyCharacterMap::get_dead_char`].
    ///
    /// # Errors
    ///
    /// Since this API needs to use JNI internally to call into the Android JVM it may return
    /// a [`AppError::JavaError`] in case there is a spurious JNI error or an exception
    /// is caught.
    pub fn get(&self, key_code: Keycode, meta_state: MetaState) -> Result<KeyMapChar, AppError> {
        let key_code: u32 = key_code.into();
        let key_code = key_code as jni::sys::jint;
        let meta_state: u32 = meta_state.0;
        let meta_state = meta_state as jni::sys::jint;

        let vm = self.jvm.clone();
        vm.attach_current_thread(|env| -> InternalResult<_> {
            let unicode = self.key_map.get(env, key_code, meta_state)?;
            let unicode = unicode as u32;

            const COMBINING_ACCENT: u32 = 0x80000000;
            const COMBINING_ACCENT_MASK: u32 = !COMBINING_ACCENT;

            if unicode == 0 {
                Ok(KeyMapChar::None)
            } else if unicode & COMBINING_ACCENT == COMBINING_ACCENT {
                let accent = unicode & COMBINING_ACCENT_MASK;
                // Safety: assumes Android key maps don't contain invalid unicode characters
                Ok(KeyMapChar::CombiningAccent(unsafe {
                    char::from_u32_unchecked(accent)
                }))
            } else {
                // Safety: assumes Android key maps don't contain invalid unicode characters
                Ok(KeyMapChar::Unicode(unsafe {
                    char::from_u32_unchecked(unicode)
                }))
            }
        })
        .map_err(|err| {
            let err: InternalAppError = err;
            err.into()
        })
    }

    /// Get the character that is produced by combining the dead key producing accent with the key producing character c.
    ///
    /// For example, ``get_dead_char('`', 'e')`` returns `'è'`. `get_dead_char('^', ' ')` returns `'^'` and `get_dead_char('^', '^')` returns `'^'`.
    ///
    /// # Errors
    ///
    /// Since this API needs to use JNI internally to call into the Android JVM it may return a
    /// [`AppError::JavaError`] in case there is a spurious JNI error or an exception is caught.
    pub fn get_dead_char(
        &self,
        accent_char: char,
        base_char: char,
    ) -> Result<Option<char>, AppError> {
        let accent_char = accent_char as jni::sys::jint;
        let base_char = base_char as jni::sys::jint;

        let vm = self.jvm.clone();
        vm.attach_current_thread(|env| -> InternalResult<_> {
            let unicode = AKeyCharacterMap::get_dead_char(env, accent_char, base_char)?;
            let unicode = unicode as u32;

            // Safety: assumes Android key maps don't contain invalid unicode characters
            Ok(if unicode == 0 {
                None
            } else {
                Some(unsafe { char::from_u32_unchecked(unicode) })
            })
        })
        .map_err(|err| {
            let err: InternalAppError = err;
            err.into()
        })
    }

    /// Gets the keyboard type.
    ///
    /// Different keyboard types have different semantics. See [`KeyboardType`] for details.
    ///
    /// # Errors
    ///
    /// Since this API needs to use JNI internally to call into the Android JVM it may return
    /// a [`AppError::JavaError`] in case there is a spurious JNI error or an exception
    /// is caught.
    pub fn get_keyboard_type(&self) -> Result<KeyboardType, AppError> {
        let vm = self.jvm.clone();
        vm.attach_current_thread(|env| -> InternalResult<_> {
            let keyboard_type = self.key_map.get_keyboard_type(env)?;
            let keyboard_type = keyboard_type as u32;
            Ok(keyboard_type.into())
        })
        .map_err(|err| {
            let err: InternalAppError = err;
            err.into()
        })
    }
}

fn device_key_character_map_with_env(
    env: &mut jni::Env<'_>,
    device_id: i32,
) -> jni::errors::Result<KeyCharacterMap> {
    let device = AInputDevice::get_device(env, device_id)?;
    if device.is_null() {
        // This isn't really an error from a JNI perspective but we would only expect
        // this to return null for a device ID of zero or an invalid device ID.
        log::error!("No input device with id {}", device_id);
        return Err(jni::errors::Error::WrongObjectType);
    }
    let character_map = device.get_key_character_map(env)?;
    let character_map = env.new_global_ref(character_map)?;
    let jvm = JavaVM::singleton().expect("Failed to get singleton JavaVM");
    Ok(KeyCharacterMap::new(jvm, character_map))
}

pub(crate) fn device_key_character_map(
    jvm: JavaVM,
    device_id: i32,
) -> InternalResult<KeyCharacterMap> {
    jvm.attach_current_thread(|env| {
        if device_id == 0 {
            return Err(InternalAppError::JniBadArgument(
                "Can't get key character map for non-physical device_id 0".into(),
            ));
        }
        device_key_character_map_with_env(env, device_id)
            .map_err(|err| jni_utils::clear_and_map_exception_to_err(env, err))
    })
}
