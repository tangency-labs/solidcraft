use std::collections::HashMap;
use std::marker::PhantomData;
use std::panic::catch_unwind;
use std::ptr;
use std::ptr::NonNull;
use std::sync::Weak;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use jni::objects::JObject;
use jni::refs::Global;
use libc::c_void;
use log::{error, trace};

use jni::sys::*;

use ndk_sys::ALooper_pollOnce;

use ndk::asset::AssetManager;
use ndk::configuration::Configuration;
use ndk::native_window::NativeWindow;

use crate::error::InternalResult;
use crate::init::{init_android_main_thread, init_java_main_thread_on_create};
use crate::main_callbacks::MainCallbacks;
use crate::util::{abort_on_panic, log_panic, try_get_path_from_ptr};
use crate::{
    AndroidApp, AndroidAppWaker, ConfigurationRef, InputStatus, MainEvent, PollEvent, Rect,
    WindowManagerFlags,
};

mod ffi;

pub mod input;
use crate::input::{
    device_key_character_map, Axis, ImeOptions, InputType, KeyCharacterMap, TextInputAction,
    TextInputState, TextSpan,
};
use input::{InputEvent, KeyEvent, MotionEvent};

// The only time it's safe to update the android_app->savedState pointer is
// while handling a SaveState event, so this API is only exposed for those
// events...
#[derive(Debug)]
pub struct StateSaver<'a> {
    app: &'a AndroidAppInner,
}

impl<'a> StateSaver<'a> {
    pub fn store(&self, state: &'a [u8]) {
        self.app.game_activity.with_locked_app(|app_ptr| {
            if app_ptr.is_null() {
                // Could probably be a panic since it shouldn't be possible to retain a `StateSaver`
                // long enough for the `GameActivity` to be destroyed.
                log::error!("Spurious attempt to save state after GameActivity was destroyed");
                return;
            }
            // android_native_app_glue specifically expects savedState to have been allocated
            // via libc::malloc since it will automatically handle freeing the data once it
            // has been handed over to the Java Activity / main thread.
            unsafe {
                // In case the application calls store() multiple times for some reason we
                // make sure to free any pre-existing state...
                if !(*app_ptr).savedState.is_null() {
                    libc::free((*app_ptr).savedState);
                    (*app_ptr).savedState = ptr::null_mut();
                    (*app_ptr).savedStateSize = 0;
                }

                let buf = libc::malloc(state.len());
                if buf.is_null() {
                    panic!("Failed to allocate save_state buffer");
                }

                // Since it's a byte array there's no special alignment requirement here.
                //
                // Since we re-define `buf` we ensure it's not possible to access the buffer
                // via its original pointer for the lifetime of the slice.
                {
                    let buf: &mut [u8] = std::slice::from_raw_parts_mut(buf.cast(), state.len());
                    buf.copy_from_slice(state);
                }

                (*app_ptr).savedState = buf;
                (*app_ptr).savedStateSize = state.len() as _;
            }
        });
    }
}

#[derive(Debug)]
pub struct StateLoader<'a> {
    app: &'a AndroidAppInner,
}
impl StateLoader<'_> {
    pub fn load(&self) -> Option<Vec<u8>> {
        self.app.game_activity.with_locked_app(|app_ptr| {
            if app_ptr.is_null() {
                // Could probably be a panic since it shouldn't be possible to retain a `StateLoader`
                // long enough for the `GameActivity` to be destroyed.
                log::error!("Spurious attempt to load state after GameActivity was destroyed");
                return None;
            }
            unsafe {
                if !(*app_ptr).savedState.is_null() && (*app_ptr).savedStateSize > 0 {
                    let buf: &mut [u8] = std::slice::from_raw_parts_mut(
                        (*app_ptr).savedState.cast(),
                        (*app_ptr).savedStateSize,
                    );
                    let state = buf.to_vec();
                    Some(state)
                } else {
                    None
                }
            }
        })
    }
}

impl AndroidApp {
    pub(crate) fn new(
        jvm: jni::JavaVM,
        main_looper: ndk::looper::ForeignLooper,
        main_callbacks: MainCallbacks,
        app_asset_manager: AssetManager,
        game_activity_ptr: *mut ffi::android_app,
        jni_activity: &JObject,
    ) -> Self {
        // We attach to the thread before creating the AndroidApp
        jvm.with_local_frame(10, |env| -> jni::errors::Result<_> {
            if let Err(err) = crate::sdk::jni_init(env) {
                panic!("Failed to init JNI bindings: {err:?}");
            };

            // Note: we don't use from_ptr since we don't own the android_app.config
            // and need to keep in mind that the Drop handler is going to call
            // AConfiguration_delete()
            let config = unsafe {
                Configuration::clone_from_ptr(NonNull::new_unchecked((*game_activity_ptr).config))
            };

            // The global reference in `android_app` is only guaranteed to be valid until
            // `onDestroy` returns, so we create our own global reference that we can guarantee will
            // remain valid until `AndroidApp` is dropped.
            let activity = env
                .new_global_ref(jni_activity)
                .expect("Failed to create global ref for Activity instance");

            // In order to support `AndroidApp::create_waker()` we need to acquire our own reference
            // to the android_main thread looper because the GameActivity glue code will release
            // it's own reference when handling the APP_CMD_DESTROY event, which could happen while
            // we still have a live AndroidApp instance.
            let looper = unsafe {
                let ptr = (*game_activity_ptr).looper;
                ndk::looper::ForeignLooper::from_ptr(ptr::NonNull::new(ptr).unwrap())
            };
            Ok(Self {
                inner: Arc::new(RwLock::new(AndroidAppInner {
                    jvm: jvm.clone(),
                    main_looper,
                    main_callbacks,
                    app_asset_manager,
                    game_activity: GameActivityGlue::new(game_activity_ptr),
                    activity,
                    looper,
                    config: ConfigurationRef::new(config),
                    native_window: Default::default(),
                    key_maps: Mutex::new(HashMap::new()),
                    input_receiver: Mutex::new(None),
                })),
            })
        })
        .expect("Failed to create AndroidApp instance")
    }
}

// Wrapper around the raw android_app pointer that can be safely sent across threads.
// SAFETY: The android_app pointer is managed by the GameActivity glue code and protected
// by a Mutex. Access is synchronized and the pointer is cleared on APP_CMD_DESTROY.
// The Mutex wrapper provides Sync, so we only need to implement Send.
#[derive(Debug)]
struct SendAndroidApp(*mut ffi::android_app);

unsafe impl Send for SendAndroidApp {}

#[derive(Debug, Clone)]
struct GameActivityGlue {
    game_activity_app: Arc<Mutex<SendAndroidApp>>,
}

impl GameActivityGlue {
    fn new(game_activity_app: *mut ffi::android_app) -> Self {
        Self {
            game_activity_app: Arc::new(Mutex::new(SendAndroidApp(game_activity_app))),
        }
    }

    fn locked_app(&self) -> std::sync::MutexGuard<'_, SendAndroidApp> {
        self.game_activity_app.lock().unwrap()
    }

    /// Access the GameActivity `android_app` glue with the guarantee that the
    /// pointer will remain consistent for the duration of the closure because
    /// the same lock must be held in order to handle the `APP_CMD_DESTROY`
    /// event that invalidates the pointer.
    ///
    /// *Important*: The app pointer may _already_ be `null` (indicating that
    /// the GameActivity has been destroyed) and must be checked by the caller
    /// before dereferencing.
    fn with_locked_app<F, R>(&self, f: F) -> R
    where
        F: FnOnce(*mut ffi::android_app) -> R,
    {
        let app = self.locked_app();
        f(app.0)
    }

    /// Called when handling the `APP_CMD_DESTROY` event to clear our retained
    /// pointer to the GameActivity `android_app` glue so that we don't
    /// accidentally access it after it's been freed.
    fn clear_app(&self) {
        let mut app = self.locked_app();
        app.0 = ptr::null_mut();
    }
}

unsafe impl Send for GameActivityGlue {}
unsafe impl Sync for GameActivityGlue {}

impl GameActivityGlue {
    // TODO: move into a trait
    /// Returns the current text input state
    ///
    /// If `take` is true then will check for some newly-flagged text input state and if set it will
    /// clear the flag and return `Some` new state, otherwise it will return None.
    ///
    /// If `take` is false this this is guaranteed to return `Some` with the current text input
    /// state.
    pub fn text_input_state(&self, take: bool) -> Option<TextInputState> {
        self.with_locked_app(|app_ptr| {
            if app_ptr.is_null() {
                log::error!("Attempted to get text input state after GameActivity was destroyed");
                return if take {
                    None
                } else {
                    Some(TextInputState::default())
                };
            }
            unsafe {
                if take {
                    // XXX: The GameActivity implementation should be using
                    // atomic ops to set this flag, and require us to use
                    // atomics to check and clear it too.
                    //
                    // We currently just hope that with the lack of atomic ops that
                    // the compiler isn't reordering code so this gets flagged
                    // before the java main thread really updates the state.
                    if (*app_ptr).textInputState == 0 {
                        return None;
                    }
                    (*app_ptr).textInputState = 0;
                }
                let activity = (*app_ptr).activity;
                let mut out_state = TextInputState {
                    text: String::new(),
                    selection: TextSpan { start: 0, end: 0 },
                    compose_region: None,
                };
                let out_ptr = &mut out_state as *mut TextInputState;

                // NEON WARNING:
                //
                // It's not clearly documented but the GameActivity API over the
                // GameTextInput library directly exposes _modified_ UTF8 text
                // from Java so we need to be careful to convert text to and
                // from UTF8
                //
                // GameTextInput also uses a pre-allocated, fixed-sized buffer for
                // the current text state and has shared `currentState_` that
                // appears to have no lock to guard access from multiple threads.
                //
                // There's also no locking at the GameActivity level, so I'm fairly
                // certain that `GameActivity_getTextInputState` isn't thread
                // safe: https://issuetracker.google.com/issues/294112477
                //
                // Overall this is all quite gnarly - and probably a good reminder
                // of why we want to use Rust instead of C/C++.
                ffi::GameActivity_getTextInputState(
                    activity,
                    Some(AndroidAppInner::map_input_state_to_text_event_callback),
                    out_ptr.cast(),
                );

                Some(out_state)
            }
        })
    }

    pub fn take_text_input_state(&self) -> Option<TextInputState> {
        self.text_input_state(true)
    }

    pub fn set_ime_editor_info(
        &self,
        input_type: InputType,
        action: TextInputAction,
        options: ImeOptions,
    ) {
        self.with_locked_app(|app_ptr| {
            if app_ptr.is_null() {
                log::error!("Attempted to set IME editor info after GameActivity was destroyed");
                return;
            }
            unsafe {
                let activity = (*app_ptr).activity;
                let action_id: i32 = action.into();

                ffi::GameActivity_setImeEditorInfo(
                    activity,
                    input_type.bits(),
                    action_id as _,
                    options.bits(),
                );
            }
        });
    }

    // TODO: move into a trait
    pub fn set_text_input_state(&self, state: TextInputState) {
        self.with_locked_app(|app_ptr| {
            if app_ptr.is_null() {
                log::error!("Attempted to set text input state after GameActivity was destroyed");
                return;
            }
            unsafe {
                let activity = (*app_ptr).activity;
                let modified_utf8 = simd_cesu8::mutf8::encode(&state.text);
                let text_length = modified_utf8.len() as i32;
                let modified_utf8_bytes = modified_utf8.as_ptr();
                let ffi_state = ffi::GameTextInputState {
                    text_UTF8: modified_utf8_bytes.cast(), // NB: may be signed or unsigned depending on target
                    text_length,
                    selection: ffi::GameTextInputSpan {
                        start: state.selection.start as i32,
                        end: state.selection.end as i32,
                    },
                    composingRegion: match state.compose_region {
                        Some(span) => {
                            // The GameText subclass of InputConnection only has a special case for removing the
                            // compose region if `start == -1` but the docs for `setComposingRegion` imply that
                            // the region should effectively be removed if any empty region is given (unlike for the
                            // selection region, it's not meaningful to maintain an empty compose region)
                            //
                            // We aim for more consistent behaviour by normalizing any empty region into `(-1, -1)`
                            // to remove the compose region.
                            //
                            // NB `setComposingRegion` itself is documented to clamp start/end to the text bounds
                            // so apart from this special-case handling in GameText's implementation of
                            // `setComposingRegion` then there's nothing special about `(-1, -1)` - it's just an empty
                            // region that should get clamped to `(0, 0)` and then get removed.
                            if span.start == span.end {
                                ffi::GameTextInputSpan { start: -1, end: -1 }
                            } else {
                                ffi::GameTextInputSpan {
                                    start: span.start as i32,
                                    end: span.end as i32,
                                }
                            }
                        }
                        None => ffi::GameTextInputSpan { start: -1, end: -1 },
                    },
                };
                ffi::GameActivity_setTextInputState(activity, &ffi_state as *const _);
            }
        })
    }

    pub fn take_pending_editor_action(&self) -> Option<i32> {
        self.with_locked_app(|app_ptr| {
            if app_ptr.is_null() {
                log::error!(
                    "Attempted to take pending editor action after GameActivity was destroyed"
                );
                return None;
            }
            unsafe {
                if (*app_ptr).pendingEditorAction {
                    (*app_ptr).pendingEditorAction = false;
                    Some((*app_ptr).editorAction)
                } else {
                    None
                }
            }
        })
    }
}

#[derive(Debug)]
pub struct AndroidAppInner {
    pub(crate) jvm: jni::JavaVM,
    game_activity: GameActivityGlue,
    config: ConfigurationRef,
    native_window: RwLock<Option<NativeWindow>>,

    activity: jni::refs::Global<jni::objects::JObject<'static>>,

    pub(crate) main_callbacks: MainCallbacks,

    /// Looper associated with the Rust `android_main` thread
    looper: ndk::looper::ForeignLooper,

    /// Looper associated with the activity's Java main thread, sometimes called
    /// the UI thread.
    main_looper: ndk::looper::ForeignLooper,

    /// A table of `KeyCharacterMap`s per `InputDevice` ID
    /// these are used to be able to map key presses to unicode
    /// characters
    key_maps: Mutex<HashMap<i32, KeyCharacterMap>>,

    /// While an app is reading input events it holds an
    /// InputReceiver reference which we track to ensure
    /// we don't hand out more than one receiver at a time
    input_receiver: Mutex<Option<Weak<InputReceiver>>>,

    /// An `AAssetManager` wrapper for the `Application` `AssetManager`
    /// Note: `AAssetManager_fromJava` specifies that the pointer is only valid
    /// while we hold a global reference to the `AssetManager` Java object
    /// to ensure it is not garbage collected. This AssetManager comes from
    /// a OnceLock initialization that leaks a single global JNI reference
    /// to guarantee that it remains valid for the lifetime of the process.
    app_asset_manager: AssetManager,
}

impl AndroidAppInner {
    pub fn activity_as_ptr(&self) -> *mut c_void {
        // Note: The global reference in `android_app` is only guaranteed to be
        // valid until `onDestroy` returns, so we have our own global reference
        // that we can instead guarantee will remain valid until `AndroidApp` is
        // dropped.
        self.activity.as_raw() as *mut c_void
    }

    pub(crate) fn looper_as_ptr(&self) -> *mut ndk_sys::ALooper {
        self.looper.ptr().as_ptr()
    }

    pub fn native_window(&self) -> Option<NativeWindow> {
        self.native_window.read().unwrap().clone()
    }

    pub fn java_main_looper(&self) -> ndk::looper::ForeignLooper {
        self.main_looper.clone()
    }

    pub fn poll_events<F>(&self, timeout: Option<Duration>, mut callback: F)
    where
        F: FnMut(PollEvent),
    {
        trace!("poll_events");

        unsafe {
            assert_eq!(
                ndk_sys::ALooper_forThread(),
                self.looper_as_ptr(),
                "Application tried to poll events from non-main thread"
            );

            let mut fd: i32 = 0;
            let mut events: i32 = 0;
            let mut source: *mut core::ffi::c_void = ptr::null_mut();

            let timeout_milliseconds = if let Some(timeout) = timeout {
                timeout.as_millis() as i32
            } else {
                -1
            };
            trace!("Calling ALooper_pollOnce, timeout = {timeout_milliseconds}");
            let id = ALooper_pollOnce(
                timeout_milliseconds,
                &mut fd,
                &mut events,
                &mut source as *mut *mut core::ffi::c_void,
            );

            // Always check to see if pollOnce woke up due to input being available
            // (NB: we can't assume we will specifically get a POLL_WAKE event after a ALooper_wake())
            if self.game_activity.with_locked_app(|app_ptr| {
                if app_ptr.is_null() {
                    false
                } else {
                    ffi::android_app_input_available_wake_up(app_ptr)
                }
            }) {
                log::debug!("Notifying Input Available");
                callback(PollEvent::Main(MainEvent::InputAvailable));
            }

            match id {
                ffi::ALOOPER_POLL_WAKE => {
                    trace!("ALooper_pollOnce returned POLL_WAKE");
                    callback(PollEvent::Wake);
                }
                ffi::ALOOPER_POLL_CALLBACK => {
                    // ALooper_pollOnce is documented to handle all callback sources internally so it should
                    // never return a _CALLBACK source id...
                    error!("Spurious ALOOPER_POLL_CALLBACK from ALooper_pollOnce() (ignored)");
                }
                ffi::ALOOPER_POLL_TIMEOUT => {
                    trace!("ALooper_pollOnce returned POLL_TIMEOUT");
                    callback(PollEvent::Timeout);
                }
                ffi::ALOOPER_POLL_ERROR => {
                    // If we have an IO error with our pipe to the main Java thread that's surely
                    // not something we can recover from
                    panic!("ALooper_pollOnce returned POLL_ERROR");
                }
                id if id >= 0 => {
                    match id as ffi::NativeAppGlueLooperId {
                        ffi::NativeAppGlueLooperId_LOOPER_ID_MAIN => {
                            trace!("ALooper_pollOnce returned ID_MAIN");
                            let source: *mut ffi::android_poll_source = source.cast();
                            if !source.is_null() {
                                let cmd_i = ffi::android_app_read_cmd((*source).app);

                                let cmd = match cmd_i as ffi::NativeAppGlueAppCmd {
                                    //NativeAppGlueAppCmd_UNUSED_APP_CMD_INPUT_CHANGED => AndroidAppMainEvent::InputChanged,
                                    ffi::NativeAppGlueAppCmd_APP_CMD_INIT_WINDOW => {
                                        Some(MainEvent::InitWindow {})
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_TERM_WINDOW => {
                                        Some(MainEvent::TerminateWindow {})
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_WINDOW_RESIZED => {
                                        Some(MainEvent::WindowResized {})
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_WINDOW_REDRAW_NEEDED => {
                                        Some(MainEvent::RedrawNeeded {})
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_CONTENT_RECT_CHANGED => {
                                        Some(MainEvent::ContentRectChanged {})
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_GAINED_FOCUS => {
                                        Some(MainEvent::GainedFocus)
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_LOST_FOCUS => {
                                        Some(MainEvent::LostFocus)
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_CONFIG_CHANGED => {
                                        Some(MainEvent::ConfigChanged {})
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_LOW_MEMORY => {
                                        Some(MainEvent::LowMemory)
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_START => {
                                        Some(MainEvent::Start)
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_RESUME => {
                                        Some(MainEvent::Resume {
                                            loader: StateLoader { app: self },
                                        })
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_SAVE_STATE => {
                                        Some(MainEvent::SaveState {
                                            saver: StateSaver { app: self },
                                        })
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_PAUSE => {
                                        Some(MainEvent::Pause)
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_STOP => Some(MainEvent::Stop),
                                    ffi::NativeAppGlueAppCmd_APP_CMD_DESTROY => {
                                        Some(MainEvent::Destroy)
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_WINDOW_INSETS_CHANGED => {
                                        Some(MainEvent::InsetsChanged {})
                                    }
                                    ffi::NativeAppGlueAppCmd_APP_CMD_SOFTWARE_KB_VIS_CHANGED => {
                                        // NOOP: we ignore these events because they are driven by a
                                        // potentially-unreliable heuristic (based on watching for
                                        // inset changes) and we don't currently have a public event
                                        // for exposing this state.
                                        None
                                    }
                                    _ => unreachable!(),
                                };

                                trace!("Read ID_MAIN command {cmd_i} = {cmd:?}");

                                trace!("Calling android_app_pre_exec_cmd({cmd_i})");
                                ffi::android_app_pre_exec_cmd((*source).app, cmd_i);

                                if let Some(cmd) = cmd {
                                    match cmd {
                                        MainEvent::ConfigChanged { .. } => {
                                            self.config.replace(Configuration::clone_from_ptr(
                                                NonNull::new_unchecked((*(*source).app).config),
                                            ));
                                        }
                                        MainEvent::InitWindow { .. } => {
                                            let win_ptr = (*(*source).app).window;
                                            // It's important that we use ::clone_from_ptr() here
                                            // because NativeWindow has a Drop implementation that
                                            // will unconditionally _release() the native window
                                            *self.native_window.write().unwrap() =
                                                Some(NativeWindow::clone_from_ptr(
                                                    NonNull::new(win_ptr).unwrap(),
                                                ));
                                        }
                                        _ => {}
                                    }

                                    trace!("Invoking callback for ID_MAIN command = {:?}", cmd);
                                    callback(PollEvent::Main(cmd));

                                    match cmd_i as ffi::NativeAppGlueAppCmd {
                                        ffi::NativeAppGlueAppCmd_APP_CMD_TERM_WINDOW => {
                                            *self.native_window.write().unwrap() = None;
                                        }
                                        ffi::NativeAppGlueAppCmd_APP_CMD_DESTROY => {
                                            // We need to clear our `*mut android_app` pointer here because
                                            // `android_native_app_glue.c` is going to free the `android_app` once it
                                            // knows that this `android_main` thread has handled the `APP_CMD_DESTROY`
                                            // event. In this case the Java main thread is in the middle
                                            // of running `android_app_free()` in response to `onDestroy()`.
                                            self.game_activity.clear_app();
                                        }
                                        _ => {}
                                    }
                                }

                                trace!("Calling android_app_post_exec_cmd({cmd_i})");
                                // SAFETY: Keep in mind that if we have just handled an `APP_CMD_DESTROY` event then we
                                // have just cleared our retained `android_app` pointer and the `(*source).app` pointer
                                // will become invalid after this call returns. In this case the Java main thread is in
                                // the middle of running `android_app_free()` in response to `onDestroy()`.
                                ffi::android_app_post_exec_cmd((*source).app, cmd_i);
                            } else {
                                panic!("ALooper_pollOnce returned ID_MAIN event with NULL android_poll_source!");
                            }
                        }
                        _ => {
                            error!("Ignoring spurious ALooper event source: id = {id}, fd = {fd}, events = {events:?}, data = {source:?}");
                        }
                    }
                }
                _ => {
                    error!("Spurious ALooper_pollOnce return value {id} (ignored)");
                }
            }
        }
    }

    pub fn set_window_flags(
        &self,
        add_flags: WindowManagerFlags,
        remove_flags: WindowManagerFlags,
    ) {
        self.game_activity.with_locked_app(|app_ptr| {
            if app_ptr.is_null() {
                log::error!("Attempted to set window flags after GameActivity was destroyed");
                return;
            }
            unsafe {
                let activity = (*app_ptr).activity;
                ffi::GameActivity_setWindowFlags(activity, add_flags.bits(), remove_flags.bits())
            }
        });
    }

    // TODO: move into a trait
    pub fn show_soft_input(&self, show_implicit: bool) {
        self.game_activity.with_locked_app(|app_ptr| {
            if app_ptr.is_null() {
                log::error!("Attempted to show soft input after GameActivity was destroyed");
                return;
            }
            unsafe {
                let activity = (*app_ptr).activity;
                let flags = if show_implicit {
                    ffi::ShowImeFlags_SHOW_IMPLICIT
                } else {
                    0
                };
                ffi::GameActivity_showSoftInput(activity, flags);
            }
        });
    }

    // TODO: move into a trait
    pub fn hide_soft_input(&self, hide_implicit_only: bool) {
        self.game_activity.with_locked_app(|app_ptr| {
            if app_ptr.is_null() {
                log::error!("Attempted to hide soft input after GameActivity was destroyed");
                return;
            }
            unsafe {
                let activity = (*app_ptr).activity;
                let flags = if hide_implicit_only {
                    ffi::HideImeFlags_HIDE_IMPLICIT_ONLY
                } else {
                    0
                };
                ffi::GameActivity_hideSoftInput(activity, flags);
            }
        });
    }

    unsafe extern "C" fn map_input_state_to_text_event_callback(
        context: *mut c_void,
        state: *const ffi::GameTextInputState,
    ) {
        // Java uses a modified UTF-8 format, which is a modified cesu8 format
        let out_ptr: *mut TextInputState = context.cast();
        let text_modified_utf8: *const u8 = (*state).text_UTF8.cast();
        let text_modified_utf8 =
            std::slice::from_raw_parts(text_modified_utf8, (*state).text_length as usize);
        match simd_cesu8::mutf8::decode(text_modified_utf8) {
            Ok(str) => {
                let len = str.len();
                (*out_ptr).text = String::from(str);

                let selection_start = (*state).selection.start.clamp(0, len as i32 + 1);
                let selection_end = (*state).selection.end.clamp(0, len as i32 + 1);
                (*out_ptr).selection = TextSpan {
                    start: selection_start as usize,
                    end: selection_end as usize,
                };
                if (*state).composingRegion.start < 0 || (*state).composingRegion.end < 0 {
                    (*out_ptr).compose_region = None;
                } else {
                    (*out_ptr).compose_region = Some(TextSpan {
                        start: (*state).composingRegion.start as usize,
                        end: (*state).composingRegion.end as usize,
                    });
                }
            }
            Err(err) => {
                log::error!("Invalid UTF8 text in TextEvent: {}", err);
            }
        }
    }

    // TODO: move into a trait
    pub fn text_input_state(&self) -> TextInputState {
        // `.text_input_state` is guaranteed to return `Some` if `take` is `false` so we can unwrap here
        self.game_activity.text_input_state(false).unwrap()
    }

    // TODO: move into a trait
    pub fn set_text_input_state(&self, state: TextInputState) {
        self.game_activity.set_text_input_state(state);
    }

    pub fn set_ime_editor_info(
        &self,
        input_type: InputType,
        action: TextInputAction,
        options: ImeOptions,
    ) {
        self.game_activity
            .set_ime_editor_info(input_type, action, options);
    }

    pub(crate) fn device_key_character_map(
        &self,
        device_id: i32,
    ) -> InternalResult<KeyCharacterMap> {
        let mut guard = self.key_maps.lock().unwrap();

        let key_map = match guard.entry(device_id) {
            std::collections::hash_map::Entry::Occupied(occupied) => occupied.get().clone(),
            std::collections::hash_map::Entry::Vacant(vacant) => {
                let character_map = device_key_character_map(self.jvm.clone(), device_id)?;
                vacant.insert(character_map.clone());
                character_map
            }
        };

        Ok(key_map)
    }

    pub fn enable_motion_axis(&mut self, axis: Axis) {
        let axis: u32 = axis.into();
        unsafe { ffi::GameActivityPointerAxes_enableAxis(axis as i32) }
    }

    pub fn disable_motion_axis(&mut self, axis: Axis) {
        let axis: u32 = axis.into();
        unsafe { ffi::GameActivityPointerAxes_disableAxis(axis as i32) }
    }

    pub fn create_waker(&self) -> AndroidAppWaker {
        // Safety: we know that the looper is a valid, non-null pointer
        unsafe { AndroidAppWaker::new(self.looper.ptr().as_ptr()) }
    }

    pub fn run_on_java_main_thread<F>(&self, f: Box<F>)
    where
        F: FnOnce() + Send + 'static,
    {
        self.main_callbacks.run_on_java_main_thread(f);
    }

    pub fn config(&self) -> ConfigurationRef {
        self.config.clone()
    }

    pub fn content_rect(&self) -> Rect {
        self.game_activity.with_locked_app(|app_ptr| {
            if app_ptr.is_null() {
                log::error!("Attempted to get content rect after GameActivity was destroyed");
                return Rect::default();
            }
            unsafe {
                Rect {
                    left: (*app_ptr).contentRect.left,
                    right: (*app_ptr).contentRect.right,
                    top: (*app_ptr).contentRect.top,
                    bottom: (*app_ptr).contentRect.bottom,
                }
            }
        })
    }

    pub fn asset_manager(&self) -> AssetManager {
        // Safety: While constructing the AndroidApp we do a OnceLock initialization
        // where we get the Application AssetManager and leak a single global JNI
        // reference that guarantees it will not be garbage collected, so we can
        // safely return the corresponding AAssetManager here.
        unsafe { AssetManager::from_ptr(self.app_asset_manager.ptr()) }
    }

    pub(crate) fn input_events_receiver(&self) -> InternalResult<Arc<InputReceiver>> {
        let mut guard = self.input_receiver.lock().unwrap();

        // Make sure we don't hand out more than one receiver at a time because
        // turning the receiver into an iterator will perform a swap_buffers
        // for the buffered input events which shouldn't happen while we're in
        // the middle of iterating events
        if let Some(receiver) = &*guard {
            if receiver.strong_count() > 0 {
                return Err(crate::error::InternalAppError::InputUnavailable);
            }
        }
        *guard = None;

        let receiver = Arc::new(InputReceiver {
            game_activity: self.game_activity.clone(),
        });

        *guard = Some(Arc::downgrade(&receiver));
        Ok(receiver)
    }

    pub fn internal_data_path(&self) -> Option<std::path::PathBuf> {
        self.game_activity.with_locked_app(|app_ptr| {
            if app_ptr.is_null() {
                log::error!("Attempted to get internal data path after GameActivity was destroyed");
                return None;
            }
            unsafe { try_get_path_from_ptr((*(*app_ptr).activity).internalDataPath) }
        })
    }

    pub fn external_data_path(&self) -> Option<std::path::PathBuf> {
        self.game_activity.with_locked_app(|app_ptr| {
            if app_ptr.is_null() {
                log::error!("Attempted to get external data path after GameActivity was destroyed");
                return None;
            }
            unsafe { try_get_path_from_ptr((*(*app_ptr).activity).externalDataPath) }
        })
    }

    pub fn obb_path(&self) -> Option<std::path::PathBuf> {
        self.game_activity.with_locked_app(|app_ptr| {
            if app_ptr.is_null() {
                log::error!("Attempted to get OBB path after GameActivity was destroyed");
                return None;
            }
            unsafe { try_get_path_from_ptr((*(*app_ptr).activity).obbPath) }
        })
    }
}

struct MotionEventsLendingIterator {
    pos: usize,
    count: usize,
}

impl MotionEventsLendingIterator {
    fn new(buffer: &InputBuffer) -> Self {
        Self {
            pos: 0,
            count: buffer.motion_events_count(),
        }
    }
    fn next<'buf>(&mut self, buffer: &'buf InputBuffer) -> Option<MotionEvent<'buf>> {
        if self.pos < self.count {
            // Safety:
            // - This iterator currently has exclusive access to the front buffer of events
            // - We know the buffer is non-null
            // - `pos` is less than the number of events stored in the buffer
            let ga_event = unsafe {
                (*buffer.ptr.as_ptr())
                    .motionEvents
                    .add(self.pos)
                    .as_ref()
                    .unwrap()
            };
            let event = MotionEvent::new(ga_event);
            self.pos += 1;
            Some(event)
        } else {
            None
        }
    }
}

struct KeyEventsLendingIterator {
    pos: usize,
    count: usize,
}

impl KeyEventsLendingIterator {
    fn new(buffer: &InputBuffer) -> Self {
        Self {
            pos: 0,
            count: buffer.key_events_count(),
        }
    }
    fn next<'buf>(&mut self, buffer: &'buf InputBuffer) -> Option<KeyEvent<'buf>> {
        if self.pos < self.count {
            // Safety:
            // - This iterator currently has exclusive access to the front buffer of events
            // - We know the buffer is non-null
            // - `pos` is less than the number of events stored in the buffer
            let ga_event = unsafe {
                (*buffer.ptr.as_ptr())
                    .keyEvents
                    .add(self.pos)
                    .as_ref()
                    .unwrap()
            };
            let event = KeyEvent::new(ga_event);
            self.pos += 1;
            Some(event)
        } else {
            None
        }
    }
}

struct InputBuffer<'a> {
    ptr: NonNull<ffi::android_input_buffer>,
    _lifetime: PhantomData<&'a ffi::android_input_buffer>,
}

impl<'a> InputBuffer<'a> {
    pub(crate) fn from_ptr(ptr: NonNull<ffi::android_input_buffer>) -> InputBuffer<'a> {
        Self {
            ptr,
            _lifetime: PhantomData,
        }
    }

    pub fn motion_events_count(&self) -> usize {
        unsafe { (*self.ptr.as_ptr()).motionEventsCount as usize }
    }

    pub fn key_events_count(&self) -> usize {
        unsafe { (*self.ptr.as_ptr()).keyEventsCount as usize }
    }
}

impl Drop for InputBuffer<'_> {
    fn drop(&mut self) {
        unsafe {
            ffi::android_app_clear_motion_events(self.ptr.as_ptr());
            ffi::android_app_clear_key_events(self.ptr.as_ptr());
        }
    }
}

/// Conceptually we can think of this like the receiver end of an
/// input events channel.
///
/// After being passed back to AndroidApp it gets turned into a
/// lending iterator for pending input events.
///
/// It serves two purposes:
/// 1. It represents an exclusive access to input events (the application
///    can only have one receiver at a time) and it's intended to support
///    the double-buffering design for input events in GameActivity where
///    we issue a swap_buffers before iterating events and wouldn't want
///    another swap to be possible before finishing - especially since
///    we want to borrow directly from the buffer while dispatching.
/// 2. It doesn't borrow from AndroidAppInner so we can pass it back to
///    AndroidApp which can drop its lock around AndroidAppInner and
///    it can then be turned into a lending iterator. (We wouldn't
///    be able to pass the iterator back to the application if it
///    borrowed from within the lock and we need to drop the lock
///    because otherwise the app wouldn't be able to access the AndroidApp
///    API in any way while iterating events)
#[derive(Debug)]
pub(crate) struct InputReceiver {
    // Safety: the `GameActivityGlue` effectively has a static lifetime and it
    // has a mutex around the `*mut android_app` pointer to ensure we can't
    // dereference a pointer that could be freed and `android_app` has its own
    // internal locking when calling `android_app_swap_input_buffers`
    game_activity: GameActivityGlue,
}

impl<'a> From<Arc<InputReceiver>> for InputIteratorInner<'a> {
    fn from(receiver: Arc<InputReceiver>) -> Self {
        let buffered = unsafe {
            let input_buffer = receiver.game_activity.with_locked_app(|app_ptr| {
                if app_ptr.is_null() {
                    log::error!(
                        "Attempting to swap input buffers after GameActivity was destroyed"
                    );
                    // `null` here will result in `InputIteratorInner.buffered` being `None` below.
                    return ptr::null_mut();
                }
                ffi::android_app_swap_input_buffers(app_ptr)
            });
            NonNull::new(input_buffer).map(|input_buffer| {
                let buffer = InputBuffer::from_ptr(input_buffer);
                let keys_iter = KeyEventsLendingIterator::new(&buffer);
                let motion_iter = MotionEventsLendingIterator::new(&buffer);
                BufferedEvents::<'a> {
                    buffer,
                    keys_iter,
                    motion_iter,
                }
            })
        };

        let game_activity = receiver.game_activity.clone();
        Self {
            _receiver: receiver,
            buffered,
            game_activity,
            ime_text_input_state_checked: false,
            ime_editor_action_checked: false,
        }
    }
}

struct BufferedEvents<'a> {
    buffer: InputBuffer<'a>,
    keys_iter: KeyEventsLendingIterator,
    motion_iter: MotionEventsLendingIterator,
}

pub(crate) struct InputIteratorInner<'a> {
    // Held to maintain exclusive access to buffered input events
    _receiver: Arc<InputReceiver>,

    buffered: Option<BufferedEvents<'a>>,
    game_activity: GameActivityGlue,
    ime_text_input_state_checked: bool,
    ime_editor_action_checked: bool,
}

impl InputIteratorInner<'_> {
    pub(crate) fn next<F>(&mut self, callback: F) -> bool
    where
        F: FnOnce(&input::InputEvent) -> InputStatus,
    {
        if let Some(buffered) = &mut self.buffered {
            if let Some(key_event) = buffered.keys_iter.next(&buffered.buffer) {
                let _ = callback(&InputEvent::KeyEvent(key_event));
                return true;
            }
            if let Some(motion_event) = buffered.motion_iter.next(&buffered.buffer) {
                let _ = callback(&InputEvent::MotionEvent(motion_event));
                return true;
            }
            self.buffered = None;
        }

        // We make sure any input state changes are sent before we check
        // for editor actions, so actions will apply to the latest state.
        if !self.ime_text_input_state_checked {
            self.ime_text_input_state_checked = true;
            if let Some(state) = self.game_activity.take_text_input_state() {
                let _ = callback(&InputEvent::TextEvent(state));
                return true;
            }
        }

        if !self.ime_editor_action_checked {
            self.ime_editor_action_checked = true;
            if let Some(action) = self.game_activity.take_pending_editor_action() {
                let _ = callback(&InputEvent::TextAction(TextInputAction::from(action)));
                return true;
            }
        }

        false
    }
}

// Rust doesn't give us a clean way to directly export symbols from C/C++
// so we rename the C/C++ symbols and re-export these JNI entrypoints from
// Rust...
//
// https://github.com/rust-lang/rfcs/issues/2771
extern "C" {
    pub fn Java_com_google_androidgamesdk_GameActivity_initializeNativeCode_C(
        env: *mut JNIEnv,
        javaGameActivity: jobject,
        internalDataDir: jstring,
        obbDir: jstring,
        externalDataDir: jstring,
        jAssetMgr: jobject,
        savedState: jbyteArray,
        javaConfig: jobject,
    ) -> jlong;

    pub fn GameActivity_onCreate_C(
        activity: *mut ffi::GameActivity,
        savedState: *mut ::std::os::raw::c_void,
        savedStateSize: libc::size_t,
    );
}

#[no_mangle]
pub unsafe extern "C" fn Java_com_google_androidgamesdk_GameActivity_initializeNativeCode(
    env: *mut JNIEnv,
    java_game_activity: jobject,
    internal_data_dir: jstring,
    obb_dir: jstring,
    external_data_dir: jstring,
    jasset_mgr: jobject,
    saved_state: jbyteArray,
    java_config: jobject,
) -> jlong {
    Java_com_google_androidgamesdk_GameActivity_initializeNativeCode_C(
        env,
        java_game_activity,
        internal_data_dir,
        obb_dir,
        external_data_dir,
        jasset_mgr,
        saved_state,
        java_config,
    )
}

#[no_mangle]
pub unsafe extern "C" fn GameActivity_onCreate(
    activity: *mut ffi::GameActivity,
    saved_state: *mut ::std::os::raw::c_void,
    saved_state_size: libc::size_t,
) {
    abort_on_panic(|| unsafe {
        let vm = jni::JavaVM::from_raw((*activity).vm as *mut _);
        let java_activity = (*activity).javaGameActivity;
        let saved_state = if !saved_state.is_null() && saved_state_size > 0 {
            std::slice::from_raw_parts(saved_state.cast(), saved_state_size)
        } else {
            &[]
        };
        init_java_main_thread_on_create(vm, java_activity as *mut c_void, saved_state);
    });

    GameActivity_onCreate_C(activity, saved_state, saved_state_size);
}

extern "Rust" {
    pub fn android_main(app: AndroidApp);
}

// This is a spring board between android_native_app_glue and the user's
// `android_main` function. This is run on a dedicated thread spawned
// by android_native_app_glue.
#[no_mangle]
pub unsafe extern "C" fn _rust_glue_entry(game_activity_glue: *mut ffi::android_app) {
    abort_on_panic(|| {
        let (jvm, jni_activity) = unsafe {
            let jvm = (*(*game_activity_glue).activity).vm;
            let activity: jobject = (*(*game_activity_glue).activity).javaGameActivity;
            (jni::JavaVM::from_raw(jvm), activity)
        };
        // Note: At this point we can assume jni::JavaVM::singleton is initialized

        let main_looper = unsafe {
            ndk::looper::ForeignLooper::from_ptr(
                std::ptr::NonNull::new((*game_activity_glue).mainLooper).unwrap(),
            )
        };

        // Note: the GameActivity implementation will have already attached the main thread to the
        // JVM before calling _rust_glue_entry so we don't to set the thread name via
        // attach_current_thread_with_config since that won't actually create a new attachment.
        //
        // Calling .attach_current_thread will ensure that the `jni` crate knows about the
        // attachment, as a convenience.
        jvm.attach_current_thread(|env| -> jni::errors::Result<()> {
            // SAFETY: We know jni_activity is a valid JNI global ref to an Activity instance
            // that will remain valid until `onDestroy` is handled (not possible until we start
            // `android_main()`).
            let jni_activity = unsafe { env.as_cast_raw::<Global<JObject>>(&jni_activity)? };

            let (app_asset_manager, main_callbacks) =
                match init_android_main_thread(&jvm, &jni_activity, &main_looper) {
                    Ok((asset_manager, main_callbacks)) => (asset_manager, main_callbacks),
                    Err(err) => {
                        eprintln!(
                            "Failed to name Java thread and set thread context class loader: {err}"
                        );
                        return Err(err);
                    }
                };

            unsafe {
                let app = AndroidApp::new(
                    jvm.clone(),
                    main_looper,
                    main_callbacks,
                    app_asset_manager,
                    game_activity_glue,
                    &jni_activity,
                );
                // We want to specifically catch any panic from the application's android_main
                // so we can finish + destroy the Activity gracefully via the JVM
                catch_unwind(|| {
                    // XXX: If we were in control of the Java Activity subclass then
                    // we could potentially run the android_main function via a Java native method
                    // springboard (e.g. call an Activity subclass method that calls a jni native
                    // method that then just calls android_main()) that would make sure there was
                    // a Java frame at the base of our call stack which would then be recognised
                    // when calling FindClass to lookup a suitable classLoader, instead of
                    // defaulting to the system loader. Without this then it's difficult for native
                    // code to look up non-standard Java classes.
                    android_main(app);
                })
                .unwrap_or_else(log_panic);

                // Let JVM know that our Activity can be destroyed before detaching from the JVM
                //
                // "Note that this method can be called from any thread; it will send a message
                //  to the main thread of the process where the Java finish call will take place"
                ffi::GameActivity_finish((*game_activity_glue).activity);
            }

            Ok(())
        })
        .expect("Failed to attach thread to JVM");
    })
}
