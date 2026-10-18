//! A glue layer for building standalone, Rust applications on Android
//!
//! This crate provides a "glue" layer for building native Rust applications on
//! Android, supporting multiple [`Activity`] base classes. It's comparable to
//! [`android_native_app_glue.c`][ndk_concepts] for C/C++ applications.
//!
//! Currently the crate supports two `Activity` base classes:
//! 1. [`NativeActivity`] - Built in to Android, this doesn't require compiling
//!    any Java or Kotlin code.
//! 2. [`GameActivity`] - From the Android Game Development Kit, it has more
//!    sophisticated input handling support than `NativeActivity`.
//!    `GameActivity` is also based on the `AndroidAppCompat` class which can
//!    help with supporting a wider range of devices.
//!
//! Standalone applications based on this crate need to be built as `cdylib`
//! libraries, like:
//! ```toml
//! [lib]
//! crate-type=["cdylib"]
//! ```
//!
//! ## Lifecycle of an Activity
//!
//! Keep in mind that Android's application programming model is based around
//! the
//! [lifecycle](https://developer.android.com/guide/components/activities/activity-lifecycle)
//! of [`Activity`] and [`Service`] components, and not the lifecycle of the
//! application process.
//!
//! An Android application may have multiple [`Activity`] and [`Service`]
//! instances created and destroyed over its lifetime, and each of these
//! [`Activity`] and [`Service`] instances will have their own lifecycles that
//! are independent from the lifecycle of the application process.
//!
//! See the Android SDK [activity lifecycle
//! documentation](https://developer.android.com/guide/components/activities/activity-lifecycle)
//! for more details on the [`Activity`] lifecycle.
//!
//! Although native applications will typically only have a single instance of
//! [`NativeActivity`] or [`GameActivity`], it's possible for these activities
//! to be created and destroyed multiple times within the lifetime of your
//! application process.
//!
//! Although [`NativeActivity`] and [`GameActivity`] were historically designed
//! for full-screen games and based on the assumption that there would only be a
//! single instance of these activities, it is good to keep in mind that Android
//! itself makes no such assumption. It's very common for non-native Android
//! applications to be tracking multiple `Activity` instances at the same time.
//!
//! The `android-activity` crate is designed to be robust to multiple `Activity`
//! instances being created and destroyed over the lifetime of the application
//! process.
//!
//! ## Entrypoints
//!
//! There are currently two supported entrypoints for an `android-activity`
//! application:
//!
//! 1. `android_on_create` **(optional)** - This runs early, on the Java main /
//!    UI thread, during `Activity.onCreate()`. It can be a good place to
//!    initialize logging and JNI bindings.
//! 2. `android_main` **(required)** - This run a dedicated main loop thread for
//!    handling lifecycle and input events for your `Activity`.
//!
//! **Important**: Your `android-activity` entrypoints are tied to the lifecycle
//! of your native **`Activity`** (i.e. [`NativeActivity`] or [`GameActivity`])
//! and not the lifecycle of your application process! This means that if your
//! `Activity` is destroyed and re-created (e.g. depending on how your
//! application handles configuration changes) then these entrypoints may be
//! called multiple times, for each `Activity` instance.
//!
//! #### Your AndroidManifest `configureChanges` state affects Activity re-creation
//!
//! Beware that, by default, certain configuration changes (e.g. device
//! rotation) will cause the Android system to destroy and re-create your
//! `Activity`, which will lead to a [`MainEvent::Destroy`] event being sent to
//! your `android_main()` thread and then `android_main()` will be called again
//! as a new native `Activity` instance is created.
//!
//! Since this can be awkward to handle, it is common practice to set the
//! `android:configChanges` property to indicate that your application can
//! handle these changes at runtime via events instead.
//!
//! **Example**:
//!
//! Here's how you can set `android:configChanges` for your `Activity` in your
//! AndroidManifest.xml:
//!
//! ```xml
//! <activity
//!     android:name="android.app.NativeActivity"
//!     android:configChanges="orientation|screenSize|screenLayout|keyboardHidden"
//!     android:label="NativeActivity Example"
//!     android:theme="@android:style/Theme.NoTitleBar.Fullscreen"
//!     android:exported="true">
//!
//!     <!-- ... -->
//! </activity>
//! ```
//!
//! ### onCreate entrypoint: `android_on_create` (optional)
//!
//! The `android_on_create` entry point will be called from the Java main
//! thread, within the `Activity`'s `onCreate` method, before the `android_main`
//! entry point is called.
//!
//! This must be an exported, unmangled, `"Rust"` ABI function with the
//! signature `fn android_on_create(state: &OnCreateState)`.
//!
//! The easiest way to achieve this is with `#[unsafe(no_mangle)]` like this:
//! ```no_run
//! #[unsafe(no_mangle)]
//! fn android_on_create(state: &android_activity::OnCreateState) {
//!     // Initialization code here
//! }
//! ```
//! (Note `extern "Rust"` is the default ABI)
//!
//! **I/O redirection**: Before `android_on_create()` is called an I/O thread is
//! spawned that will handle redirecting standard input and output to the
//! Android log, visible via `logcat`.
//!
//! [`OnCreateState`] provides access to the Java VM and a JNI reference to the
//! `Activity` instance, as well as any saved state from a previous instance of
//! the Activity.
//!
//! Due to the way JNI class loading works, this can be a convenient place to
//! initialize JNI bindings because it's called while the `Activity`'s
//! `onCreate` callback is on the stack, so the default class loader will be
//! able to find the application's Java classes. See the Android
//! [JNI tips](https://developer.android.com/ndk/guides/jni-tips#faq:-why-didnt-findclass-find-my-class)
//! guide for more details on this.
//!
//! This can also be a good place to initialize logging, since it's called
//! first.
//!
//! **Important**: This entrypoint must not block for a long time or do heavy
//! work, since it's running on the Java main thread and will block the
//! `Activity` from being created until it returns.
//!
//! Blocking the Java main thread for too long may cause an "Application Not
//! Responding" (ANR) dialog to be shown to the user, and cause users to force
//! close your application.
//!
//! **Panic behavior**: If `android_on_create` panics, the application will
//! abort. This is because the callback runs within a native JNI callback where
//! unwinding is not permitted. Ensure your initialization code either cannot
//! panic or uses `catch_unwind` internally if you want to allow partial
//! initialization failures.
//!
//! #### Example:
//!
//! ```no_run
//! # use std::sync::OnceLock;
//! # use android_activity::OnCreateState;
//! # use jni::{JavaVM, objects::JObject};
//! #[unsafe(no_mangle)]
//! fn android_on_create(state: &OnCreateState) {
//!     static APP_ONCE: OnceLock<()> = OnceLock::new();
//!     APP_ONCE.get_or_init(|| {
//!         // Initialize logging...
//!         //
//!         // Remember, `android_on_create` may be called multiple times but, depending on
//!         // the crate, logger initialization may panic if attempted multiple times.
//!     });
//!     let vm = unsafe { JavaVM::from_raw(state.vm_as_ptr().cast()) };
//!     let activity = state.activity_as_ptr() as jni::sys::jobject;
//!     // Although the thread is implicitly already attached (we are inside an onCreate native method)
//!     // using `vm.attach_current_thread` here will use the existing attachment, give us an `&Env`
//!     // reference and also catch Java exceptions.
//!     if let Err(err) = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
//!         // SAFETY:
//!         // - The `Activity` reference / pointer is at least valid until we return
//!         // - By creating a `Cast` we ensure we can't accidentally delete the reference
//!         let activity = unsafe { env.as_cast_raw::<JObject>(&activity)? };
//!
//!         // Do something with the activity on the Java main thread...
//!         Ok(())
//!     }) {
//!        eprintln!("Failed to interact with Android SDK on Java main thread: {err:?}");
//!     }
//! }
//! ```
//!
//! ### Main loop thread entrypoint: `android_main` (required)
//!
//! Your application must always define an `android_main` function as an entry
//! point for running a main loop thread for your Activity.
//!
//! This must be an exported, unmangled, `"Rust"` ABI function with the
//! signature `fn android_main(app: AndroidApp)`.
//!
//! The easiest way to achieve this is with `#[unsafe(no_mangle)]` like this:
//! ```no_run
//! #[unsafe(no_mangle)]
//! fn android_main(app: android_activity::AndroidApp) {
//!     // Main loop code here
//! }
//! ```
//! (Note `extern "Rust"` is the default ABI)
//!
//! Once your application's `Activity` class has loaded and it calls `onCreate`
//! then `android-activity` will spawn a dedicated thread to run your
//! `android_main` function, separate from the Java thread that created the
//! corresponding `Activity`.
//!
//! Before `android_main()` is called:
//! - A `JavaVM` and
//!   [`android.content.Context`](https://developer.android.com/reference/android/content/Context)
//!   instance will be associated with the [`ndk_context`] crate so that other,
//!   independent, Rust crates are able to find a JavaVM for making JNI calls.
//! - The `JavaVM` will be attached to the native thread (for JNI)
//! - A [Looper] is attached to the Rust native thread.
//!
//! **Important:** This thread *must* call [`AndroidApp::poll_events()`]
//! regularly in order to receive lifecycle and input events for the `Activity`.
//! Some `Activity` lifecycle callbacks on the Java main thread will block until
//! the next time `poll_events()` is called, so if you don't call
//! `poll_events()` regularly you may trigger an ANR dialog and cause users to
//! force close your application.
//!
//! **Important**: You should return from `android_main()` as soon as possible
//! if you receive a [`MainEvent::Destroy`] event from `poll_events()`.  Most
//! [`AndroidApp`] methods will become a no-op after [`MainEvent::Destroy`] is
//! received, since it no longer has an associated `Activity`.
//!
//! **Important**: Do *not* call `std::process::exit()` from your
//! `android_main()` function since that will subvert the normal lifecycle of
//! the `Activity` and other components. Keep in mind that code running in
//! `android_main()` does not logically own the entire process since there may
//! be other Android components (e.g. Services) running within the process.
//!
//! ## AndroidApp: State and Event Loop
//!
//! [`AndroidApp`] provides an interface to query state for the application as
//! well as monitor events, such as lifecycle and input events for the
//! associated native `Activity` instance.
//!
//! ### Cheaply Cloneable [`AndroidApp`]
//!
//! [`AndroidApp`] is intended to be something that can be cheaply passed around
//! within an application. It is reference-counted and can be cheaply cloned.
//!
//! ### `Send` and `Sync` [`AndroidApp`] (**but...**)
//!
//! Although an [`AndroidApp`] implements `Send` and `Sync` you do need to take
//! into consideration that some APIs, such as [`AndroidApp::poll_events()`] are
//! explicitly documented to only be usable from your `android_main()` thread.
//!
//! ### No associated Activity after [`MainEvent::Destroy`]
//!
//! After you receive a [`MainEvent::Destroy`] event from `poll_events()` then
//! the [`AndroidApp`] will no longer have an associated `Activity` and most of
//! its methods will become no-ops. You should return from `android_main()` as
//! soon as possible after receiving a `Destroy` event since your native
//! `Activity` no longer exists.
//!
//! If a new [`Activity`] instance is created after that then a new
//! [`AndroidApp`] will be created for that new [`Activity`] instance and sent
//! to a new call to `android_main()`.
//!
//! **Important**: It's not recommended to store an [`AndroidApp`] as global
//! static state and it should instead be passed around by reference within your
//! application so it can be reliably dropped when the `Activity` is destroyed
//! and you return from `android_main()`.
//!
//! # Android Extensible Enums
//!
//! There are numerous enums in the `android-activity` API which are effectively
//! bindings to enums declared in the Android SDK which need to be considered
//! _runtime_ extensible.
//!
//! Any enum variants that come from the Android SDK may be extended in future
//! versions of Android and your code could be exposed to new variants if you
//! build an application that might be installed on new versions of Android.
//!
//! This crate follows a convention of adding a hidden `__Unknown(u32)` variant
//! to these enums to ensure we can always do lossless conversions between the
//! integers from the SDK and our corresponding Rust enums. This can be
//! important in case you need to pass certain variants back to the SDK
//! regardless of whether you knew about that variants specific semantics at
//! compile time.
//!
//! You should never include this `__Unknown(u32)` variant within any exhaustive
//! pattern match and should instead treat the enums like `#[non_exhaustive]`
//! enums that require you to add a catch-all for any `unknown => {}` values.
//!
//! Any code that would exhaustively include the `__Unknown(u32)` variant when
//! pattern matching can not be guaranteed to be forwards compatible with new
//! releases of `android-activity` which may add new Rust variants to these
//! enums without requiring a breaking semver bump.
//!
//! You can (infallibly) convert these enums to and from primitive `u32` values
//! using `.into()`:
//!
//! For example, here is how you could ensure forwards compatibility with both
//! compile-time and runtime extensions of a `SomeEnum` enum:
//!
//! ```ignore
//! match some_enum {
//!     SomeEnum::Foo => {},
//!     SomeEnum::Bar => {},
//!     unhandled => {
//!         let sdk_val: u32 = unhandled.into();
//!         println!("Unhandled enum variant {some_enum:?} has SDK value: {sdk_val}");
//!     }
//! }
//! ```
//!
//! [`Activity`]: https://developer.android.com/reference/android/app/Activity
//! [`NativeActivity`]:
//!     https://developer.android.com/reference/android/app/NativeActivity
//! [ndk_concepts]: https://developer.android.com/ndk/guides/concepts#naa
//! [`GameActivity`]:
//!     https://developer.android.com/games/agdk/integrate-game-activity
//! [`Service`]: https://developer.android.com/reference/android/app/Service
//! [Looper]: https://developer.android.com/reference/android/os/Looper
//! [`Context`]: https://developer.android.com/reference/android/content/Context

#![deny(clippy::manual_let_else)]

use std::ffi::CStr;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;

use bitflags::bitflags;
use jni::vm::JavaVM;
use libc::c_void;

use ndk::asset::AssetManager;
use ndk::native_window::NativeWindow;

// Since we expose `ndk` types in our public API it's convenient if crates can
// defer to these re-exported APIs and avoid having to bump explicit
// dependencies when they pull in new releases of android-activity.
pub use ndk;
pub use ndk_sys;

#[cfg(not(target_os = "android"))]
compile_error!("android-activity only supports compiling for Android");

#[cfg(all(feature = "game-activity", feature = "native-activity"))]
compile_error!(
    r#"The "game-activity" and "native-activity" features cannot be enabled at the same time"#
);
#[cfg(all(
    not(any(feature = "game-activity", feature = "native-activity")),
    not(any(doc, used_on_docsrs)),
))]
compile_error!(
    r#"Either "game-activity" or "native-activity" must be enabled as features

If you have set one of these features then this error indicates that Cargo is trying to
link together multiple implementations of android-activity (with incompatible versions)
which is not supported.

Since android-activity is responsible for the `android_main` entrypoint of your application
then there can only be a single implementation of android-activity linked with your application.

You can use `cargo tree` (e.g. via `cargo ndk -t arm64-v8a tree`) to identify why multiple
versions have been resolved.

You may need to add a `[patch]` into your Cargo.toml to ensure a specific version of
android-activity is used across all of your application's crates."#
);

#[cfg_attr(feature = "native-activity", path = "native_activity/mod.rs")]
#[cfg_attr(feature = "game-activity", path = "game_activity/mod.rs")]
#[cfg_attr(
    all(
        // No activities enabled.
        not(any(feature = "native-activity", feature = "game-activity")),
        // And building docs.
        any(doc, used_on_docsrs),
    ),
    // Fall back to documenting native activity.
    path = "native_activity/mod.rs"
)]
pub(crate) mod activity_impl;

pub mod error;
use error::Result;

mod init;

pub mod input;
use input::KeyCharacterMap;

mod config;
pub use config::ConfigurationRef;

mod util;

mod jni_utils;

mod sdk;

mod waker;
pub use waker::AndroidAppWaker;

mod main_callbacks;

pub(crate) const ANDROID_ACTIVITY_TAG: &CStr = c"android-activity";

/// A rectangle with integer edge coordinates. Used to represent window insets, for example.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    /// An empty `Rect` with all components set to zero.
    pub fn empty() -> Self {
        Self {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        }
    }
}

impl From<Rect> for ndk_sys::ARect {
    fn from(rect: Rect) -> Self {
        Self {
            left: rect.left,
            right: rect.right,
            top: rect.top,
            bottom: rect.bottom,
        }
    }
}

impl From<ndk_sys::ARect> for Rect {
    fn from(arect: ndk_sys::ARect) -> Self {
        Self {
            left: arect.left,
            right: arect.right,
            top: arect.top,
            bottom: arect.bottom,
        }
    }
}

pub use activity_impl::StateLoader;
pub use activity_impl::StateSaver;
#[cfg(feature = "native-activity")]
pub use activity_impl::set_nonblocking_callbacks;

/// An application event delivered during [`AndroidApp::poll_events`]
#[non_exhaustive]
#[derive(Debug)]
pub enum MainEvent<'a> {
    /// New input events are available via [`AndroidApp::input_events_iter()`]
    ///
    /// _Note: Even if more input is received this event will not be resent
    /// until [`AndroidApp::input_events_iter()`] has been called, which enables
    /// applications to batch up input processing without there being lots of
    /// redundant event loop wake ups._
    ///
    /// [`AndroidApp::input_events_iter()`]: AndroidApp::input_events_iter
    InputAvailable,

    /// Command from main thread: a new [`NativeWindow`] is ready for use.  Upon
    /// receiving this command, [`AndroidApp::native_window()`] will return the new window
    #[non_exhaustive]
    InitWindow {},

    /// Command from main thread: the existing [`NativeWindow`] needs to be
    /// terminated.  Upon receiving this command, [`AndroidApp::native_window()`] still
    /// returns the existing window; after returning from the [`AndroidApp::poll_events()`]
    /// callback then [`AndroidApp::native_window()`] will return `None`.
    #[non_exhaustive]
    TerminateWindow {},

    // TODO: include the prev and new size in the event
    /// Command from main thread: the current [`NativeWindow`] has been resized.
    /// Please redraw with its new size.
    #[non_exhaustive]
    WindowResized {},

    /// Command from main thread: the current [`NativeWindow`] needs to be redrawn.
    /// You should redraw the window before the [`AndroidApp::poll_events()`]
    /// callback returns in order to avoid transient drawing glitches.
    #[non_exhaustive]
    RedrawNeeded {},

    /// Command from main thread: the content area of the window has changed,
    /// such as from the soft input window being shown or hidden.  You can
    /// get the new content rect by calling [`AndroidApp::content_rect()`]
    #[non_exhaustive]
    ContentRectChanged {},

    /// Command from main thread: the app's activity window has gained
    /// input focus.
    GainedFocus,

    /// Command from main thread: the app's activity window has lost
    /// input focus.
    LostFocus,

    /// Command from main thread: the current device configuration has changed.  Any
    /// reference gotten via [`AndroidApp::config()`] will automatically contain the latest
    /// [`ndk::configuration::Configuration`].
    #[non_exhaustive]
    ConfigChanged {},

    /// Command from main thread: the system is running low on memory.
    /// Try to reduce your memory use.
    LowMemory,

    /// Command from main thread: the app's activity has been started.
    Start,

    /// Command from main thread: the app's activity has been resumed.
    #[non_exhaustive]
    Resume { loader: StateLoader<'a> },

    /// Command from main thread: the app should generate a new saved state
    /// for itself, to restore from later if needed.  If you have saved state,
    /// allocate it with malloc and place it in android_app.savedState with
    /// the size in android_app.savedStateSize.  The will be freed for you
    /// later.
    #[non_exhaustive]
    SaveState { saver: StateSaver<'a> },

    /// Command from main thread: the app's activity has been paused.
    Pause,

    /// Command from main thread: the app's activity has been stopped.
    Stop,

    /// Command from main thread: the app's activity is being destroyed,
    /// and waiting for the app thread to clean up and exit before proceeding.
    Destroy,

    /// Command from main thread: the app's insets have changed.
    #[non_exhaustive]
    InsetsChanged {},
}

/// An event delivered during [`AndroidApp::poll_events`]
#[derive(Debug)]
#[non_exhaustive]
pub enum PollEvent<'a> {
    Wake,
    Timeout,
    Main(MainEvent<'a>),
}

/// Indicates whether an application has handled or ignored an event
///
/// If an event is not handled by an application then some default handling may happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputStatus {
    Handled,
    Unhandled,
}

use activity_impl::AndroidAppInner;

bitflags! {
    /// Flags for [`AndroidApp::set_window_flags`]
    /// as per the [android.view.WindowManager.LayoutParams Java API](https://developer.android.com/reference/android/view/WindowManager.LayoutParams)
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct WindowManagerFlags: u32 {
        /// As long as this window is visible to the user, allow the lock
        /// screen to activate while the screen is on.  This can be used
        /// independently, or in combination with
        /// [`Self::KEEP_SCREEN_ON`] and/or [`Self::SHOW_WHEN_LOCKED`]
        const ALLOW_LOCK_WHILE_SCREEN_ON = 0x00000001;

        /// Everything behind this window will be dimmed. */
        const DIM_BEHIND = 0x00000002;

        /// Blur everything behind this window.
        #[deprecated = "Blurring is no longer supported"]
        const BLUR_BEHIND = 0x00000004;

        /// This window won't ever get key input focus, so the
        /// user can not send key or other button events to it.  Those will
        /// instead go to whatever focusable window is behind it.  This flag
        /// will also enable [`Self::NOT_TOUCH_MODAL`] whether or not
        /// that is explicitly set.
        ///
        /// Setting this flag also implies that the window will not need to
        /// interact with
        /// a soft input method, so it will be Z-ordered and positioned
        /// independently of any active input method (typically this means it
        /// gets Z-ordered on top of the input method, so it can use the full
        /// screen for its content and cover the input method if needed.  You
        /// can use [`Self::ALT_FOCUSABLE_IM`] to modify this
        /// behavior.
        const NOT_FOCUSABLE = 0x00000008;

        /// This window can never receive touch events.
        const NOT_TOUCHABLE = 0x00000010;

        /// Even when this window is focusable (if
        /// [`Self::NOT_FOCUSABLE`] is not set), allow any pointer
        /// events outside of the window to be sent to the windows behind it.
        /// Otherwise it will consume all pointer events itself, regardless of
        /// whether they are inside of the window.
        const NOT_TOUCH_MODAL = 0x00000020;

        /// When set, if the device is asleep when the touch
        /// screen is pressed, you will receive this first touch event.  Usually
        /// the first touch event is consumed by the system since the user can
        /// not see what they are pressing on.
        #[deprecated]
        const TOUCHABLE_WHEN_WAKING = 0x00000040;

        /// As long as this window is visible to the user, keep
        /// the device's screen turned on and bright.
        const KEEP_SCREEN_ON = 0x00000080;

        /// Place the window within the entire screen, ignoring
        /// decorations around the border (such as the status bar).  The
        /// window must correctly position its contents to take the screen
        /// decoration into account.
        const LAYOUT_IN_SCREEN = 0x00000100;

        /// Allows the window to extend outside of the screen.
        const LAYOUT_NO_LIMITS = 0x00000200;

        /// Hide all screen decorations (such as the status
        /// bar) while this window is displayed.  This allows the window to
        /// use the entire display space for itself -- the status bar will
        /// be hidden when an app window with this flag set is on the top
        /// layer. A fullscreen window will ignore a value of
        /// [`Self::SOFT_INPUT_ADJUST_RESIZE`] the window will stay
        /// fullscreen and will not resize.
        const FULLSCREEN = 0x00000400;

        /// Override [`Self::FULLSCREEN`] and force the
        /// screen decorations (such as the status bar) to be shown.
        const FORCE_NOT_FULLSCREEN = 0x00000800;
        /// Turn on dithering when compositing this window to
        /// the screen.
        #[deprecated="This flag is no longer used"]
        const DITHER = 0x00001000;

        /// Treat the content of the window as secure, preventing
        /// it from appearing in screenshots or from being viewed on non-secure
        /// displays.
        const SECURE = 0x00002000;

        /// A special mode where the layout parameters are used
        /// to perform scaling of the surface when it is composited to the
        /// screen.
        const SCALED = 0x00004000;

        /// Intended for windows that will often be used when the user is
        /// holding the screen against their face, it will aggressively
        /// filter the event stream to prevent unintended presses in this
        /// situation that may not be desired for a particular window, when
        /// such an event stream is detected, the application will receive
        /// a `AMOTION_EVENT_ACTION_CANCEL` to indicate this so
        /// applications can handle this accordingly by taking no action on
        /// the event until the finger is released.
        const IGNORE_CHEEK_PRESSES = 0x00008000;

        /// A special option only for use in combination with
        /// [`Self::LAYOUT_IN_SCREEN`].  When requesting layout in
        /// the screen your window may appear on top of or behind screen decorations
        /// such as the status bar.  By also including this flag, the window
        /// manager will report the inset rectangle needed to ensure your
        /// content is not covered by screen decorations.
        const LAYOUT_INSET_DECOR = 0x00010000;

        /// Invert the state of [`Self::NOT_FOCUSABLE`] with
        /// respect to how this window interacts with the current method.
        /// That is, if [`Self::NOT_FOCUSABLE`] is set and this flag is set,
        /// then the window will behave as if it needs to interact with the
        /// input method and thus be placed behind/away from it; if
        /// [`Self::NOT_FOCUSABLE`] is not set and this flag is set,
        /// then the window will behave as if it doesn't need to interact
        /// with the input method and can be placed to use more space and
        /// cover the input method.
        const ALT_FOCUSABLE_IM = 0x00020000;

        /// If you have set [`Self::NOT_TOUCH_MODAL`], you
        /// can set this flag to receive a single special MotionEvent with
        /// the action
        /// `AMOTION_EVENT_ACTION_OUTSIDE` for
        /// touches that occur outside of your window.  Note that you will not
        /// receive the full down/move/up gesture, only the location of the
        /// first down as an `AMOTION_EVENT_ACTION_OUTSIDE`.
        const WATCH_OUTSIDE_TOUCH = 0x00040000;

        /// Special flag to let windows be shown when the screen
        /// is locked. This will let application windows take precedence over
        /// key guard or any other lock screens. Can be used with
        /// [`Self::KEEP_SCREEN_ON`] to turn screen on and display
        /// windows directly before showing the key guard window.  Can be used with
        /// [`Self::DISMISS_KEYGUARD`] to automatically fully
        /// dismiss non-secure key guards.  This flag only applies to the top-most
        /// full-screen window.
        const SHOW_WHEN_LOCKED = 0x00080000;

        /// Ask that the system wallpaper be shown behind
        /// your window.  The window surface must be translucent to be able
        /// to actually see the wallpaper behind it; this flag just ensures
        /// that the wallpaper surface will be there if this window actually
        /// has translucent regions.
        const SHOW_WALLPAPER = 0x00100000;

        /// When set as a window is being added or made
        /// visible, once the window has been shown then the system will
        /// poke the power manager's user activity (as if the user had woken
        /// up the device) to turn the screen on.
        const TURN_SCREEN_ON = 0x00200000;

        /// When set the window will cause the key guard to
        /// be dismissed, only if it is not a secure lock key guard.  Because such
        /// a key guard is not needed for security, it will never re-appear if
        /// the user navigates to another window (in contrast to
        /// [`Self::SHOW_WHEN_LOCKED`], which will only temporarily
        /// hide both secure and non-secure key guards but ensure they reappear
        /// when the user moves to another UI that doesn't hide them).
        /// If the key guard is currently active and is secure (requires an
        /// unlock pattern) then the user will still need to confirm it before
        /// seeing this window, unless [`Self::SHOW_WHEN_LOCKED`] has
        /// also been set.
        const DISMISS_KEYGUARD = 0x00400000;
    }
}

/// The top-level state and interface for a native Rust application
///
/// `AndroidApp` provides an interface to query state for the application as
/// well as monitor events, such as lifecycle and input events, that are
/// marshalled between the Java thread that owns the `Activity` and the native
/// thread that runs the `android_main()` code.
///
/// # Cheaply Clonable [`AndroidApp`]
///
/// [`AndroidApp`] is intended to be something that can be cheaply passed around
/// by referenced within an application. It is reference counted and can be
/// cheaply cloned.
///
/// # `Send` and `Sync` [`AndroidApp`]
///
/// Although an [`AndroidApp`] implements `Send` and `Sync` you do need to take
/// into consideration that some APIs, such as [`AndroidApp::poll_events()`] are
/// explicitly documented to only be usable from your `android_main()` thread.
///
#[derive(Debug, Clone)]
pub struct AndroidApp {
    pub(crate) inner: Arc<RwLock<AndroidAppInner>>,
}

impl PartialEq for AndroidApp {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}
impl Eq for AndroidApp {}

impl Hash for AndroidApp {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.inner).hash(state);
    }
}

impl AndroidApp {
    /// Queries the current [`NativeWindow`] for the application.
    ///
    /// This will only return `Some(window)` between
    /// [`MainEvent::InitWindow`] and [`MainEvent::TerminateWindow`]
    /// events.
    pub fn native_window(&self) -> Option<NativeWindow> {
        self.inner.read().unwrap().native_window()
    }

    /// Returns a [`ndk::looper::ForeignLooper`] associated with the Java
    /// main / UI thread.
    ///
    /// This can be used to register file descriptors that may wake up the
    /// Java main / UI thread and optionally run callbacks on that thread.
    ///
    /// ```ignore
    /// # use ndk;
    /// # let app: AndroidApp = todo!();
    /// let looper = app.java_main_looper();
    /// looper.add_fd_with_callback(todo!(), ndk::looper::FdEvent::INPUT, todo!()).unwrap();
    /// ```
    pub fn java_main_looper(&self) -> ndk::looper::ForeignLooper {
        self.inner.read().unwrap().java_main_looper().clone()
    }

    /// Returns a pointer to the Java Virtual Machine, for making JNI calls
    ///
    /// This returns a pointer to the Java Virtual Machine which can be used
    /// with the [`jni`] crate (or similar crates) to make JNI calls that bridge
    /// between native Rust code and Java/Kotlin code running within the JVM.
    ///
    /// If you use the [`jni`] crate you can could this as a [`JavaVM`] via:
    /// ```no_run
    /// # use jni::JavaVM;
    /// # let app: android_activity::AndroidApp = todo!();
    /// let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    /// ```
    ///
    /// [`jni`]: https://crates.io/crates/jni
    /// [`JavaVM`]: https://docs.rs/jni/latest/jni/struct.JavaVM.html
    pub fn vm_as_ptr(&self) -> *mut c_void {
        JavaVM::singleton().unwrap().get_raw() as _
    }

    /// Returns an (*unowned*) JNI global object reference for this
    /// application's JVM `Activity` as a pointer
    ///
    /// If you use the [`jni`] crate you can cast this as a `JObject` reference
    /// via:
    /// ```no_run
    /// # use jni::objects::JObject;
    /// # use jni::refs::Global;
    /// # fn use_jni(env: &jni::Env, app: &android_activity::AndroidApp) -> jni::errors::Result<()> {
    /// let raw_activity_global = app.activity_as_ptr() as jni::sys::jobject;
    /// // SAFETY: The reference / pointer is valid as long as `app` is valid
    /// let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity_global)? };
    /// # Ok(()) }
    /// ```
    ///
    /// # JNI Safety
    ///
    /// Note that the returned reference will be a JNI global reference *that
    /// you do not own*.
    /// - Don't wrap the reference as a [`Global`] which would try to delete the
    ///   reference when dropped.
    /// - Don't wrap the reference in an [`Auto`] which would treat the
    ///   reference like a local reference and try to delete it when dropped.
    ///
    /// The reference is only guaranteed to be valid until you drop the
    /// [`AndroidApp`].
    ///
    /// **Warning:** Don't assume the returned reference has a `'static` lifetime
    /// since it's possible for `android_main()` to run multiple times over the
    /// lifetime of an application with a new `AndroidApp` instance each time.
    ///
    /// [`jni`]: https://crates.io/crates/jni
    /// [`Auto`]: https://docs.rs/jni/latest/jni/refs/struct.Auto.html
    /// [`Global`]: https://docs.rs/jni/latest/jni/refs/struct.Global.html
    pub fn activity_as_ptr(&self) -> *mut c_void {
        self.inner.read().unwrap().activity_as_ptr()
    }

    /// Polls for any events associated with this [AndroidApp] and processes
    /// those events (such as lifecycle events) via the given `callback`.
    ///
    /// It's important to use this API for polling, and not call
    /// [`ALooper_pollAll`] or [`ALooper_pollOnce`] directly since some events
    /// require pre- and post-processing either side of the callback. For
    /// correct behavior events should be handled immediately, before returning
    /// from the callback and not simply queued for batch processing later. For
    /// example the existing [`NativeWindow`] is accessible during a
    /// [`MainEvent::TerminateWindow`] callback and will be set to `None` once
    /// the callback returns, and this is also synchronized with the Java main
    /// thread. The [`MainEvent::SaveState`] event is also synchronized with the
    /// Java main thread.
    ///
    /// Internally this is based on [`ALooper_pollOnce`] and will only poll
    /// file descriptors once per invocation.
    ///
    /// # Wake Events
    ///
    /// Note that although there is an explicit [PollEvent::Wake] that _can_
    /// indicate that the main loop was explicitly woken up (E.g. via
    /// [`AndroidAppWaker::wake`]) it's possible that there will be
    /// more-specific events that will be delivered after a wake up.
    ///
    /// In other words you should only expect to explicitly see
    /// [`PollEvent::Wake`] events after an early wake up if there were no
    /// other, more-specific, events that could be delivered after the wake up.
    ///
    /// Again, said another way - it's possible that _any_ event could
    /// effectively be delivered after an early wake up so don't assume there is
    /// a 1:1 relationship between invoking a wake up via
    /// [`AndroidAppWaker::wake`] and the delivery of [PollEvent::Wake].
    ///
    /// # Panics
    ///
    /// This must only be called from your `android_main()` thread and it may
    /// panic if called from another thread.
    ///
    /// [`ALooper_pollAll`]: ndk::looper::ThreadLooper::poll_all
    /// [`ALooper_pollOnce`]: ndk::looper::ThreadLooper::poll_once
    pub fn poll_events<F>(&self, timeout: Option<Duration>, callback: F)
    where
        F: FnMut(PollEvent<'_>),
    {
        self.inner.read().unwrap().poll_events(timeout, callback);
    }

    /// Creates a means to wake up the main loop while it is blocked waiting for
    /// events within [`AndroidApp::poll_events()`].
    pub fn create_waker(&self) -> AndroidAppWaker {
        self.inner.read().unwrap().create_waker()
    }

    /// Runs the given closure on the Java main / UI thread.
    ///
    /// This is useful for performing operations that must be executed on the
    /// main thread, such as interacting with Android SDK APIs that require
    /// execution on the main thread.
    ///
    /// Any panic within the closure will be caught and logged as an error,
    /// (assuming your application is built to allow unwinding).
    ///
    /// The thread will be attached to the JVM (for using JNI) and any
    /// un-cleared Java exceptions left over by the callback will be caught,
    /// cleared and logged as an error.
    ///
    /// There is no built-in mechanism to propagate results back to the caller
    /// but you can use channels or other synchronization primitives that you
    /// capture.
    ///
    /// It's important to avoid blocking the `android_main` thread while waiting
    /// for any results because this could lead to deadlocks for `Activity`
    /// callbacks that require a synchronous response for the `android_activity`
    /// thread.
    ///
    /// # Example
    ///
    /// This example demonstrates using the `jni` 0.22 API to show a toast
    /// message from the Java main thread.
    ///
    /// ```no_run
    /// use android_activity::AndroidApp;
    /// use jni::{objects::JString, refs::Global};
    ///
    /// jni::bind_java_type! { Context => "android.content.Context" }
    /// jni::bind_java_type! {
    ///     Activity => "android.app.Activity",
    ///     type_map {
    ///         Context => "android.content.Context",
    ///     },
    ///     is_instance_of {
    ///         context: Context
    ///     },
    /// }
    ///
    /// jni::bind_java_type! {
    ///     Toast => "android.widget.Toast",
    ///     type_map {
    ///         Context => "android.content.Context",
    ///     },
    ///     methods {
    ///         static fn make_text(context: Context, text: JCharSequence, duration: i32) -> Toast,
    ///         fn show(),
    ///     }
    /// }
    ///
    /// enum ToastDuration {
    ///     Short = 0,
    ///     Long = 1,
    /// }
    ///
    /// fn send_toast(outer_app: &AndroidApp, msg: impl AsRef<str>, duration: ToastDuration) {
    ///     let app = outer_app.clone();
    ///     let msg = msg.as_ref().to_string();
    ///     outer_app.run_on_java_main_thread(Box::new(move || {
    ///         let jvm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as _) };
    ///         // As an micro optimization you could use jvm.with_top_local_frame, since we know
    ///         // we're already attached
    ///         if let Err(err) = jvm.attach_current_thread(|env| -> jni::errors::Result<()> {
    ///             let activity: jni::sys::jobject = app.activity_as_ptr() as _;
    ///             let activity = unsafe { env.as_cast_raw::<Global<Activity>>(&activity)? };
    ///             let message = JString::new(env, &msg)?;
    ///             let toast = Toast::make_text(env, activity.as_ref(), &message, duration as i32)?;
    ///             toast.show(env)?;
    ///             Ok(())
    ///         }) {
    ///             log::error!("Failed to show toast on main thread: {err:?}");
    ///         }
    ///     }));
    /// }
    /// ```
    pub fn run_on_java_main_thread<F>(&self, f: Box<F>)
    where
        F: FnOnce() + Send + 'static,
    {
        self.inner.read().unwrap().run_on_java_main_thread(f);
    }

    /// Returns a **reference** to this application's [`ndk::configuration::Configuration`].
    ///
    /// # Warning
    ///
    /// The value held by this reference **will change** with every [`MainEvent::ConfigChanged`]
    /// event that is raised.  You should **not** [`Clone`] this type to compare it against a
    /// "new" [`AndroidApp::config()`] when that event is raised, since both point to the same
    /// internal [`ndk::configuration::Configuration`] and will be identical.
    pub fn config(&self) -> ConfigurationRef {
        self.inner.read().unwrap().config()
    }

    /// Queries the current content rectangle of the window; this is the area where the
    /// window's content should be placed to be seen by the user.
    pub fn content_rect(&self) -> Rect {
        self.inner.read().unwrap().content_rect()
    }

    /// Returns the `AssetManager` for the application's `Application` context.
    ///
    /// Use this to access raw files bundled in the application's .apk file.
    ///
    /// This is an `Application`-scoped asset manager, not an `Activity`-scoped
    /// one. In normal usage those behave the same for packaged assets, so this
    /// is usually the correct API to use.
    ///
    /// In uncommon cases, an `Activity` may have a context-specific
    /// asset/resource view that differs from the `Application` context. If you
    /// specifically need the current `Activity`'s `AssetManager`, obtain the
    /// `Activity` via [`AndroidApp::activity_as_ptr`] and call `getAssets()`
    /// through JNI.
    ///
    /// The returned `AssetManager` has a `'static` lifetime and remains valid
    /// across `Activity` recreation, including when `android_main()` is
    /// re-entered.
    ///
    /// **Beware**: If you consider accessing the `Activity` context's
    /// `AssetManager` through JNI you must keep the `AssetManager` alive via a
    /// global reference before accessing the ndk `AAssetManager` and
    /// `ndk::asset::AssetManager` does not currently handle this for you.
    pub fn asset_manager(&self) -> AssetManager {
        self.inner.read().unwrap().asset_manager()
    }

    /// Change the window flags of the given activity.
    ///
    /// Note that some flags must be set before the window decoration is created,
    /// see
    /// `<https://developer.android.com/reference/android/view/Window#setFlags(int,%20int)>`.
    pub fn set_window_flags(
        &self,
        add_flags: WindowManagerFlags,
        remove_flags: WindowManagerFlags,
    ) {
        self.inner
            .write()
            .unwrap()
            .set_window_flags(add_flags, remove_flags);
    }

    /// Enable additional input axis
    ///
    /// To reduce overhead, by default only [`input::Axis::X`] and [`input::Axis::Y`] are enabled
    /// and other axis should be enabled explicitly.
    pub fn enable_motion_axis(&self, axis: input::Axis) {
        self.inner.write().unwrap().enable_motion_axis(axis);
    }

    /// Disable input axis
    ///
    /// To reduce overhead, by default only [`input::Axis::X`] and [`input::Axis::Y`] are enabled
    /// and other axis should be enabled explicitly.
    pub fn disable_motion_axis(&self, axis: input::Axis) {
        self.inner.write().unwrap().disable_motion_axis(axis);
    }

    /// Explicitly request that the current input method's soft input area be
    /// shown to the user, if needed.
    ///
    /// Call this if the user interacts with your view in such a way that they
    /// have expressed they would like to start performing input into it.
    pub fn show_soft_input(&self, show_implicit: bool) {
        self.inner.read().unwrap().show_soft_input(show_implicit);
    }

    /// Request to hide the soft input window from the context of the window
    /// that is currently accepting input.
    ///
    /// This should be called as a result of the user doing some action that
    /// fairly explicitly requests to have the input window hidden.
    pub fn hide_soft_input(&self, hide_implicit_only: bool) {
        self.inner
            .read()
            .unwrap()
            .hide_soft_input(hide_implicit_only);
    }

    /// Fetch the current input text state, as updated by any active IME.
    pub fn text_input_state(&self) -> input::TextInputState {
        self.inner.read().unwrap().text_input_state()
    }

    /// Forward the given input text `state` to any active IME.
    pub fn set_text_input_state(&self, state: input::TextInputState) {
        self.inner.read().unwrap().set_text_input_state(state);
    }

    /// Specify the type of text being input, how the IME enter/action key
    /// should behave and any additional IME options.
    ///
    /// Also see the Android SDK documentation for
    /// [android.view.inputmethod.EditorInfo](https://developer.android.com/reference/android/view/inputmethod/EditorInfo)
    pub fn set_ime_editor_info(
        &self,
        input_type: input::InputType,
        action: input::TextInputAction,
        options: input::ImeOptions,
    ) {
        self.inner
            .read()
            .unwrap()
            .set_ime_editor_info(input_type, action, options);
    }

    /// Get an exclusive, lending iterator over buffered input events
    ///
    /// Applications are expected to call this in-sync with their rendering or
    /// in response to a [`MainEvent::InputAvailable`] event being delivered.
    ///
    /// _**Note:** your application is will only be delivered a single
    /// [`MainEvent::InputAvailable`] event between calls to this API._
    ///
    /// To reduce overhead, by default, only [`input::Axis::X`] and [`input::Axis::Y`] are enabled
    /// and other axis should be enabled explicitly via [`Self::enable_motion_axis`].
    ///
    /// This isn't the most ergonomic iteration API since we can't return a standard `Iterator`:
    /// - This API returns a lending iterator may borrow from the internal buffer
    ///   of pending events without copying them.
    /// - For each event we want to ensure the application reports whether the
    ///   event was handled.
    ///
    /// # Example
    /// Code to iterate all pending input events would look something like this:
    ///
    /// ```no_run
    /// # use android_activity::{AndroidApp, InputStatus, input::InputEvent};
    /// # let app: AndroidApp = todo!();
    /// match app.input_events_iter() {
    ///     Ok(mut iter) => {
    ///         loop {
    ///             let read_input = iter.next(|event| {
    ///                 let handled = match event {
    ///                     InputEvent::KeyEvent(key_event) => {
    ///                         // Snip
    ///                         InputStatus::Handled
    ///                     }
    ///                     InputEvent::MotionEvent(motion_event) => {
    ///                         InputStatus::Unhandled
    ///                     }
    ///                     event => {
    ///                         InputStatus::Unhandled
    ///                     }
    ///                 };
    ///
    ///                 handled
    ///             });
    ///
    ///             if !read_input {
    ///                 break;
    ///             }
    ///         }
    ///     }
    ///     Err(err) => {
    ///         log::error!("Failed to get input events iterator: {err:?}");
    ///     }
    /// }
    /// ```
    ///
    /// # Panics
    ///
    /// This must only be called from your `android_main()` thread and it may panic if called
    /// from another thread.
    pub fn input_events_iter(&self) -> Result<input::InputIterator<'_>> {
        let receiver = {
            let guard = self.inner.read().unwrap();
            guard.input_events_receiver()?
        };

        Ok(input::InputIterator {
            inner: receiver.into(),
        })
    }

    /// Lookup the [`KeyCharacterMap`] for the given input `device_id`
    ///
    /// Use [`KeyCharacterMap::get`] to map key codes + meta state into unicode characters
    /// or dead keys that compose with the next key.
    ///
    /// # Example
    ///
    /// Code to handle unicode character mapping as well as combining dead keys could look some thing like:
    ///
    /// ```no_run
    /// # use android_activity::{AndroidApp, input::{InputEvent, KeyEvent, KeyMapChar}};
    /// # let app: AndroidApp = todo!();
    /// # let key_event: KeyEvent = todo!();
    /// let mut combining_accent = None;
    /// // Snip
    ///
    /// let combined_key_char = if let Ok(map) = app.device_key_character_map(key_event.device_id()) {
    ///     match map.get(key_event.key_code(), key_event.meta_state()) {
    ///         Ok(KeyMapChar::Unicode(unicode)) => {
    ///             let combined_unicode = if let Some(accent) = combining_accent {
    ///                 match map.get_dead_char(accent, unicode) {
    ///                     Ok(Some(key)) => {
    ///                         println!("KeyEvent: Combined '{unicode}' with accent '{accent}' to give '{key}'");
    ///                         Some(key)
    ///                     }
    ///                     Ok(None) => None,
    ///                     Err(err) => {
    ///                         eprintln!("KeyEvent: Failed to combine 'dead key' accent '{accent}' with '{unicode}': {err:?}");
    ///                         None
    ///                     }
    ///                 }
    ///             } else {
    ///                 println!("KeyEvent: Pressed '{unicode}'");
    ///                 Some(unicode)
    ///             };
    ///             combining_accent = None;
    ///             combined_unicode.map(|unicode| KeyMapChar::Unicode(unicode))
    ///         }
    ///         Ok(KeyMapChar::CombiningAccent(accent)) => {
    ///             println!("KeyEvent: Pressed 'dead key' combining accent '{accent}'");
    ///             combining_accent = Some(accent);
    ///             Some(KeyMapChar::CombiningAccent(accent))
    ///         }
    ///         Ok(KeyMapChar::None) => {
    ///             println!("KeyEvent: Pressed non-unicode key");
    ///             combining_accent = None;
    ///             None
    ///         }
    ///         Err(err) => {
    ///             eprintln!("KeyEvent: Failed to get key map character: {err:?}");
    ///             combining_accent = None;
    ///             None
    ///         }
    ///     }
    /// } else {
    ///     None
    /// };
    /// ```
    ///
    /// # Errors
    ///
    /// Since this API needs to use JNI internally to call into the Android JVM it may return
    /// a [`error::AppError::JavaError`] in case there is a spurious JNI error or an exception
    /// is caught.
    ///
    /// This API should not be called with a `device_id` of `0`, since that indicates a non-physical
    /// device and will result in a [`error::AppError::JavaError`].
    pub fn device_key_character_map(&self, device_id: i32) -> Result<KeyCharacterMap> {
        Ok(self
            .inner
            .read()
            .unwrap()
            .device_key_character_map(device_id)?)
    }

    /// The user-visible SDK version of the framework
    ///
    /// Also referred to as [`Build.VERSION_CODES`](https://developer.android.com/reference/android/os/Build.VERSION_CODES)
    pub fn sdk_version() -> i32 {
        let mut prop = android_properties::getprop("ro.build.version.sdk");
        if let Some(val) = prop.value() {
            val.parse::<i32>()
                .expect("Failed to parse ro.build.version.sdk property")
        } else {
            panic!("Couldn't read ro.build.version.sdk system property");
        }
    }

    /// Path to this application's internal data directory
    pub fn internal_data_path(&self) -> Option<std::path::PathBuf> {
        self.inner.read().unwrap().internal_data_path()
    }

    /// Path to this application's external data directory
    pub fn external_data_path(&self) -> Option<std::path::PathBuf> {
        self.inner.read().unwrap().external_data_path()
    }

    /// Path to the directory containing the application's OBB files (if any).
    pub fn obb_path(&self) -> Option<std::path::PathBuf> {
        self.inner.read().unwrap().obb_path()
    }
}

#[test]
fn test_app_is_send_sync() {
    fn needs_send_sync<T: Send + Sync>() {}
    needs_send_sync::<AndroidApp>();
}

/// The state passed to the optional `android_on_create` entry point if
/// available.
///
/// This gives access to the Java VM, the Java `Activity` and any saved state
/// from a previous instance of the `Activity` that was saved via the
/// `onSaveInstanceState` callback.
///
/// Each time `android_on_create` is called it will receive a new `Activity`
/// reference.
///
/// See the top-level [`android-activity`](crate) documentation for more details
/// on `android_on_create`.
pub struct OnCreateState<'a> {
    jvm: JavaVM,
    java_activity: *mut c_void,
    saved_state: &'a [u8],
}

impl<'a> OnCreateState<'a> {
    pub(crate) fn new(jvm: JavaVM, java_activity: *mut c_void, saved_state: &'a [u8]) -> Self {
        Self {
            jvm,
            java_activity,
            saved_state,
        }
    }

    /// Returns a pointer to the Java Virtual Machine, for making JNI calls
    ///
    /// If you use the `jni` crate, you can wrap this pointer as a `JavaVM` via:
    /// ```no_run
    /// # use jni::JavaVM;
    /// # let on_create_state: android_activity::OnCreateState = todo!();
    /// let vm = unsafe { JavaVM::from_raw(on_create_state.vm_as_ptr().cast()) };
    /// ```
    pub fn vm_as_ptr(&self) -> *mut c_void {
        self.jvm.get_raw().cast()
    }

    /// Returns an (*unowned*) JNI global object reference for this `Activity`
    /// as a pointer
    ///
    /// If you use the `jni` crate, you can cast this as a `JObject` reference
    /// via:
    ///
    /// ```no_run
    /// # use jni::{JavaVM, objects::JObject};
    /// # let on_create_state: android_activity::OnCreateState = todo!();
    /// let vm = unsafe { JavaVM::from_raw(on_create_state.vm_as_ptr().cast()) };
    /// let _res = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
    ///     let activity = on_create_state.activity_as_ptr() as jni::sys::jobject;
    ///     // SAFETY: The reference / pointer is valid at least until we return from `android_on_create`
    ///     let activity = unsafe { env.as_cast_raw::<JObject>(&activity)? };
    ///     // Do something with `activity` here
    ///     Ok(())
    /// });
    /// ```
    ///
    /// # JNI Safety
    ///
    /// It is not specified whether this will be a global or local reference and
    /// in any case you must treat is as a reference that you do not own and
    /// must not attempt to delete it.
    /// - Don't wrap the reference as a `Global` which would try to delete the
    ///   reference when dropped.
    /// - Don't wrap the reference in an `Auto` which would treat the reference
    ///   like a local reference and try to delete it when dropped.
    pub fn activity_as_ptr(&self) -> *mut c_void {
        self.java_activity
    }

    /// Returns the saved state of the `Activity` as a byte slice, which may be
    /// empty if there is no saved state.
    pub fn saved_state(&self) -> &[u8] {
        self.saved_state
    }
}
