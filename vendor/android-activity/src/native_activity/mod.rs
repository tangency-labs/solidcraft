use std::collections::HashMap;
use std::marker::PhantomData;
use std::panic::AssertUnwindSafe;
use std::ptr;
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Duration;

use jni::objects::JObject;
use jni::JavaVM;
use libc::c_void;
use log::{error, trace};
use ndk::input_queue::InputQueue;
use ndk::{asset::AssetManager, native_window::NativeWindow};

use crate::error::InternalResult;
use crate::main_callbacks::MainCallbacks;
use crate::sdk::{Activity, Context, InputMethodManager};
use crate::{
    util, AndroidApp, AndroidAppWaker, ConfigurationRef, InputStatus, MainEvent, PollEvent, Rect,
    WindowManagerFlags,
};

pub mod input;
use crate::input::{
    device_key_character_map, Axis, ImeOptions, InputType, KeyCharacterMap, TextInputAction,
    TextInputState, TextSpan,
};

mod glue;
use self::glue::NativeActivityGlue;
pub use self::glue::set_nonblocking_callbacks;

pub const LOOPER_ID_MAIN: libc::c_int = 1;
pub const LOOPER_ID_INPUT: libc::c_int = 2;
//pub const LOOPER_ID_USER: ::std::os::raw::c_uint = 3;

/// An interface for saving application state during [MainEvent::SaveState] events
///
/// This interface is only available temporarily while handling a [MainEvent::SaveState] event.
#[derive(Debug)]
pub struct StateSaver<'a> {
    app: &'a AndroidAppInner,
}

impl<'a> StateSaver<'a> {
    /// Stores the given `state` such that it will be available to load the next
    /// time that the application resumes.
    pub fn store(&self, state: &'a [u8]) {
        self.app.native_activity.set_saved_state(state);
    }
}

/// An interface for loading application state during [MainEvent::Resume] events
///
/// This interface is only available temporarily while handling a [MainEvent::Resume] event.
#[derive(Debug)]
pub struct StateLoader<'a> {
    app: &'a AndroidAppInner,
}
impl StateLoader<'_> {
    /// Returns whatever state was saved during the last [MainEvent::SaveState] event or `None`
    pub fn load(&self) -> Option<Vec<u8>> {
        self.app.native_activity.saved_state()
    }
}

impl AndroidApp {
    pub(crate) fn new(
        jvm: JavaVM,
        main_looper: ndk::looper::ForeignLooper,
        main_callbacks: MainCallbacks,
        app_asset_manager: AssetManager,
        native_activity: NativeActivityGlue,
        jni_activity: &JObject,
    ) -> Self {
        jvm.with_local_frame(10, |env| -> jni::errors::Result<_> {
            if let Err(err) = crate::sdk::jni_init(env) {
                panic!("Failed to init JNI bindings: {err:?}");
            };

            let looper = unsafe {
                let ptr = ndk_sys::ALooper_prepare(
                    ndk_sys::ALOOPER_PREPARE_ALLOW_NON_CALLBACKS as libc::c_int,
                );
                ndk::looper::ForeignLooper::from_ptr(ptr::NonNull::new(ptr).unwrap())
            };

            // The global reference in `ANativeActivity` is only guaranteed to be valid until
            // `onDestroy` returns, so we create our own global reference that we can guarantee will
            // remain valid until `AndroidApp` is dropped.
            let activity = env
                .new_global_ref(jni_activity)
                .expect("Failed to create global ref for Activity instance");

            let app = Self {
                inner: Arc::new(RwLock::new(AndroidAppInner {
                    jvm: jvm.clone(),
                    main_looper,
                    main_callbacks,
                    app_asset_manager,
                    native_activity,
                    activity,
                    looper,
                    key_maps: Mutex::new(HashMap::new()),
                    input_receiver: Mutex::new(None),
                })),
            };

            {
                let guard = app.inner.write().unwrap();

                let main_fd = guard.native_activity.cmd_read_fd();
                unsafe {
                    ndk_sys::ALooper_addFd(
                        guard.looper.ptr().as_ptr(),
                        main_fd,
                        LOOPER_ID_MAIN,
                        ndk_sys::ALOOPER_EVENT_INPUT as libc::c_int,
                        None,
                        //&mut guard.cmd_poll_source as *mut _ as *mut _);
                        ptr::null_mut(),
                    );
                }
            }

            Ok(app)
        })
        .expect("Failed to create AndroidApp instance")
    }
}

#[derive(Debug)]
pub(crate) struct AndroidAppInner {
    pub(crate) jvm: JavaVM,

    pub(crate) native_activity: NativeActivityGlue,

    activity: jni::refs::Global<jni::objects::JObject<'static>>,

    main_callbacks: MainCallbacks,

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
    pub(crate) fn activity_as_ptr(&self) -> *mut c_void {
        // Note: The global reference in `ANativeActivity::clazz` (misnomer for instance reference)
        // is only guaranteed to be valid until `onDestroy` returns, so we have our own global
        // reference that we can instead guarantee will remain valid until `AndroidApp` is dropped.
        self.activity.as_raw() as *mut c_void
    }

    pub(crate) fn looper_as_ptr(&self) -> *mut ndk_sys::ALooper {
        self.looper.ptr().as_ptr()
    }

    pub fn java_main_looper(&self) -> ndk::looper::ForeignLooper {
        self.main_looper.clone()
    }

    pub fn native_window(&self) -> Option<NativeWindow> {
        self.native_activity.mutex.lock().unwrap().window.clone()
    }

    pub fn poll_events<F>(&self, timeout: Option<Duration>, mut callback: F)
    where
        F: FnMut(PollEvent<'_>),
    {
        trace!("poll_events");

        unsafe {
            let mut fd: i32 = 0;
            let mut events: i32 = 0;
            let mut source: *mut c_void = ptr::null_mut();

            let timeout_milliseconds = if let Some(timeout) = timeout {
                timeout.as_millis() as i32
            } else {
                -1
            };

            trace!("Calling ALooper_pollOnce, timeout = {timeout_milliseconds}");
            assert_eq!(
                ndk_sys::ALooper_forThread(),
                self.looper_as_ptr(),
                "Application tried to poll events from non-main thread"
            );
            let id = ndk_sys::ALooper_pollOnce(
                timeout_milliseconds,
                &mut fd,
                &mut events,
                &mut source as *mut *mut c_void,
            );
            trace!("pollOnce id = {id}");
            match id {
                ndk_sys::ALOOPER_POLL_WAKE => {
                    trace!("ALooper_pollOnce returned POLL_WAKE");
                    callback(PollEvent::Wake);
                }
                ndk_sys::ALOOPER_POLL_CALLBACK => {
                    // ALooper_pollOnce is documented to handle all callback sources internally so it should
                    // never return a _CALLBACK source id...
                    error!("Spurious ALOOPER_POLL_CALLBACK from ALooper_pollOnce() (ignored)");
                }
                ndk_sys::ALOOPER_POLL_TIMEOUT => {
                    trace!("ALooper_pollOnce returned POLL_TIMEOUT");
                    callback(PollEvent::Timeout);
                }
                ndk_sys::ALOOPER_POLL_ERROR => {
                    // If we have an IO error with our pipe to the main Java thread that's surely
                    // not something we can recover from
                    panic!("ALooper_pollOnce returned POLL_ERROR");
                }
                id if id >= 0 => {
                    match id {
                        LOOPER_ID_MAIN => {
                            trace!("ALooper_pollOnce returned ID_MAIN");
                            if let Some(ipc_cmd) = self.native_activity.read_cmd() {
                                let main_cmd = match ipc_cmd {
                                    // We don't forward info about the AInputQueue to apps since it's
                                    // an implementation details that's also not compatible with
                                    // GameActivity
                                    glue::AppCmd::InputQueueChanged => None,

                                    glue::AppCmd::InitWindow => Some(MainEvent::InitWindow {}),
                                    glue::AppCmd::TermWindow => Some(MainEvent::TerminateWindow {}),
                                    glue::AppCmd::WindowResized => {
                                        Some(MainEvent::WindowResized {})
                                    }
                                    glue::AppCmd::WindowRedrawNeeded => {
                                        Some(MainEvent::RedrawNeeded {})
                                    }
                                    glue::AppCmd::ContentRectChanged => {
                                        Some(MainEvent::ContentRectChanged {})
                                    }
                                    glue::AppCmd::GainedFocus => Some(MainEvent::GainedFocus),
                                    glue::AppCmd::LostFocus => Some(MainEvent::LostFocus),
                                    glue::AppCmd::ConfigChanged => {
                                        Some(MainEvent::ConfigChanged {})
                                    }
                                    glue::AppCmd::LowMemory => Some(MainEvent::LowMemory),
                                    glue::AppCmd::Start => Some(MainEvent::Start),
                                    glue::AppCmd::Resume => Some(MainEvent::Resume {
                                        loader: StateLoader { app: self },
                                    }),
                                    glue::AppCmd::SaveState => Some(MainEvent::SaveState {
                                        saver: StateSaver { app: self },
                                    }),
                                    glue::AppCmd::Pause => Some(MainEvent::Pause),
                                    glue::AppCmd::Stop => Some(MainEvent::Stop),
                                    glue::AppCmd::Destroy => Some(MainEvent::Destroy),
                                };

                                trace!("Calling pre_exec_cmd({ipc_cmd:#?})");
                                self.native_activity.pre_exec_cmd(
                                    ipc_cmd,
                                    self.looper_as_ptr(),
                                    LOOPER_ID_INPUT,
                                );

                                if let Some(main_cmd) = main_cmd {
                                    trace!("Invoking callback for ID_MAIN command = {main_cmd:?}");
                                    callback(PollEvent::Main(main_cmd));
                                }

                                trace!("Calling post_exec_cmd({ipc_cmd:#?})");
                                self.native_activity.post_exec_cmd(ipc_cmd);
                            }
                        }
                        LOOPER_ID_INPUT => {
                            trace!("ALooper_pollOnce returned ID_INPUT");

                            // To avoid spamming the application with event loop iterations notifying them of
                            // input events then we only send one `InputAvailable` per iteration of input
                            // handling. We re-attach the looper when the application calls
                            // `AndroidApp::input_events()`
                            self.native_activity.detach_input_queue_from_looper();
                            callback(PollEvent::Main(MainEvent::InputAvailable))
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

    pub fn create_waker(&self) -> AndroidAppWaker {
        // Safety: we know that the looper is a valid, non-null pointer
        unsafe { AndroidAppWaker::new(self.looper_as_ptr()) }
    }

    pub fn run_on_java_main_thread<F>(&self, f: Box<F>)
    where
        F: FnOnce() + Send + 'static,
    {
        self.main_callbacks.run_on_java_main_thread(f);
    }

    pub fn config(&self) -> ConfigurationRef {
        self.native_activity.config()
    }

    pub fn content_rect(&self) -> Rect {
        self.native_activity.content_rect()
    }

    pub fn asset_manager(&self) -> AssetManager {
        // Safety: While constructing the AndroidApp we do a OnceLock initialization
        // where we get the Application AssetManager and leak a single global JNI
        // reference that guarantees it will not be garbage collected, so we can
        // safely return the corresponding AAssetManager here.
        unsafe { AssetManager::from_ptr(self.app_asset_manager.ptr()) }
    }

    pub fn set_window_flags(
        &self,
        add_flags: WindowManagerFlags,
        remove_flags: WindowManagerFlags,
    ) {
        let guard = self.native_activity.mutex.lock().unwrap();
        let na = guard.activity;
        if na.is_null() {
            log::error!("Can't set window flags after NativeActivity has been destroyed");
            return;
        }

        let na_mut = na;
        unsafe {
            ndk_sys::ANativeActivity_setWindowFlags(
                na_mut.cast(),
                add_flags.bits(),
                remove_flags.bits(),
            );
        }
    }

    // TODO: move into a trait
    pub fn show_soft_input(&self, show_implicit: bool) {
        let guard = self.native_activity.mutex.lock().unwrap();
        let na = guard.activity;
        if na.is_null() {
            log::error!("Can't show soft input after NativeActivity has been destroyed");
            return;
        }

        // Note: `.attach_current_thread()` will also handle catching any Java exceptions that
        // might be thrown by the JNI calls we make.
        let res = self
            .jvm
            .attach_current_thread(|env| -> jni::errors::Result<()> {
                // SolidCraft: prefer the Activity's own IME host view when it provides one. A bare NativeActivity
                // has no editable view, so showSoftInput() on the decor view is unreliable and never
                // delivers text; the generated Activity focuses a real editable EditText instead.
                // Activities that do not opt in fall through to the stock decor-view call.
                if env
                    .call_method(self.activity.as_ref(), jni::jni_str!("imeShow"), jni::jni_sig!("()V"), &[])
                    .is_ok()
                {
                    return Ok(());
                }
                let _ = env.exception_clear();

                let activity = env.as_cast::<Activity>(self.activity.as_ref())?;

                let ims = Context::INPUT_METHOD_SERVICE(env)?;
                let im_manager = activity.as_context().get_system_service(env, ims)?;
                let im_manager = InputMethodManager::cast_local(env, im_manager)?;
                let jni_window = activity.get_window(env)?;
                let view = jni_window.get_decor_view(env)?;
                let flags = if show_implicit {
                    ndk_sys::ANATIVEACTIVITY_SHOW_SOFT_INPUT_IMPLICIT as i32
                } else {
                    0
                };
                im_manager.show_soft_input(env, view, flags)?;
                Ok(())
            });
        if let Err(err) = res {
            log::warn!("Failed to show soft input: {err:?}");
        }
    }

    // TODO: move into a trait
    pub fn hide_soft_input(&self, hide_implicit_only: bool) {
        let guard = self.native_activity.mutex.lock().unwrap();
        let na = guard.activity;
        if na.is_null() {
            log::error!("Can't hide soft input after NativeActivity has been destroyed");
            return;
        }

        // Note: `.attach_current_thread()` will also handle catching any Java exceptions that
        // might be thrown by the JNI calls we make.
        let res = self
            .jvm
            .attach_current_thread(|env| -> jni::errors::Result<()> {
                // Mirror `show_soft_input`: when the Activity owns an IME host view, dismiss the
                // keyboard through it (and release its focus). Falls through to the decor view
                // otherwise.
                if env
                    .call_method(self.activity.as_ref(), jni::jni_str!("imeHide"), jni::jni_sig!("()V"), &[])
                    .is_ok()
                {
                    return Ok(());
                }
                let _ = env.exception_clear();

                let activity = env.as_cast::<Activity>(self.activity.as_ref())?;

                let ims = Context::INPUT_METHOD_SERVICE(env)?;
                let imm_obj = activity.as_context().get_system_service(env, ims)?;
                let imm = InputMethodManager::cast_local(env, imm_obj)?;

                let window = activity.get_window(env)?;
                let decor = window.get_decor_view(env)?;
                let token = decor.get_window_token(env)?;

                // HIDE_IMPLICIT_ONLY == 1, HIDE_NOT_ALWAYS == 2
                let flags = if hide_implicit_only { 1 } else { 0 };

                let _hidden = imm.hide_soft_input_from_window(env, token, flags)?;
                Ok(())
            });

        if let Err(err) = res {
            error!("Failed to hide soft input: {err:?}");
        }
    }

    // TODO: move into a trait
    pub fn text_input_state(&self) -> TextInputState {
        TextInputState {
            text: String::new(),
            selection: TextSpan { start: 0, end: 0 },
            compose_region: None,
        }
    }

    // TODO: move into a trait
    pub fn set_text_input_state(&self, _state: TextInputState) {
        // NOP: Unsupported
    }

    // TODO: move into a trait
    pub fn set_ime_editor_info(
        &self,
        _input_type: InputType,
        _action: TextInputAction,
        _options: ImeOptions,
    ) {
        // NOP: Unsupported
    }

    pub fn device_key_character_map(&self, device_id: i32) -> InternalResult<KeyCharacterMap> {
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

    pub fn enable_motion_axis(&self, _axis: Axis) {
        // NOP - The InputQueue API doesn't let us optimize which axis values are read
    }

    pub fn disable_motion_axis(&self, _axis: Axis) {
        // NOP - The InputQueue API doesn't let us optimize which axis values are read
    }

    pub fn input_events_receiver(&self) -> InternalResult<Arc<InputReceiver>> {
        let mut guard = self.input_receiver.lock().unwrap();

        if let Some(receiver) = &*guard {
            if receiver.strong_count() > 0 {
                return Err(crate::error::InternalAppError::InputUnavailable);
            }
        }
        *guard = None;

        // Get the InputQueue for the NativeActivity (if there is one) and also ensure
        // the queue is re-attached to our event Looper (so new input events will again
        // trigger a wake up)
        let queue = self
            .native_activity
            .looper_attached_input_queue(self.looper_as_ptr(), LOOPER_ID_INPUT);

        // Note: we don't treat it as an error if there is no queue, so if applications
        // iterate input before a queue has been created (e.g. before onStart) then
        // it will simply behave like there are no events available currently.
        let receiver = Arc::new(InputReceiver { queue });

        *guard = Some(Arc::downgrade(&receiver));
        Ok(receiver)
    }

    pub fn internal_data_path(&self) -> Option<std::path::PathBuf> {
        let guard = self.native_activity.mutex.lock().unwrap();
        let na = guard.activity;
        if na.is_null() {
            log::error!("Can't get internal data path after NativeActivity has been destroyed");
            return None;
        }
        unsafe { util::try_get_path_from_ptr((*na).internalDataPath) }
    }

    pub fn external_data_path(&self) -> Option<std::path::PathBuf> {
        let guard = self.native_activity.mutex.lock().unwrap();
        let na = guard.activity;
        if na.is_null() {
            log::error!("Can't get external data path after NativeActivity has been destroyed");
            return None;
        }
        unsafe { util::try_get_path_from_ptr((*na).externalDataPath) }
    }

    pub fn obb_path(&self) -> Option<std::path::PathBuf> {
        let guard = self.native_activity.mutex.lock().unwrap();
        let na = guard.activity;
        if na.is_null() {
            log::error!("Can't get OBB path after NativeActivity has been destroyed");
            return None;
        }
        unsafe { util::try_get_path_from_ptr((*na).obbPath) }
    }
}

#[derive(Debug)]
pub(crate) struct InputReceiver {
    queue: Option<InputQueue>,
}

impl From<Arc<InputReceiver>> for InputIteratorInner<'_> {
    fn from(receiver: Arc<InputReceiver>) -> Self {
        Self {
            receiver,
            _lifetime: PhantomData,
        }
    }
}

pub(crate) struct InputIteratorInner<'a> {
    // Held to maintain exclusive access to buffered input events
    receiver: Arc<InputReceiver>,
    _lifetime: PhantomData<&'a ()>,
}

impl InputIteratorInner<'_> {
    pub(crate) fn next<F>(&self, callback: F) -> bool
    where
        F: FnOnce(&input::InputEvent) -> InputStatus,
    {
        let Some(queue) = &self.receiver.queue else {
            log::trace!("no queue available for events");
            return false;
        };

        // Note: we basically ignore errors from event() currently. Looking at the source code for
        // Android's InputQueue, the only error that can be returned here is 'WOULD_BLOCK', which we
        // want to just treat as meaning the queue is empty.
        //
        // ref: https://github.com/aosp-mirror/platform_frameworks_base/blob/master/core/jni/android_view_InputQueue.cpp
        //
        if let Ok(Some(ndk_event)) = queue.event() {
            log::trace!("queue: got event: {ndk_event:?}");

            if let Some(ndk_event) = queue.pre_dispatch(ndk_event) {
                let event = match ndk_event {
                    ndk::event::InputEvent::MotionEvent(e) => {
                        input::InputEvent::MotionEvent(input::MotionEvent::new(e))
                    }
                    ndk::event::InputEvent::KeyEvent(e) => {
                        input::InputEvent::KeyEvent(input::KeyEvent::new(e))
                    }
                    _ => todo!("NDK added a new type"),
                };

                // `finish_event` needs to be called for each event otherwise
                // the app would likely get an ANR
                let result = std::panic::catch_unwind(AssertUnwindSafe(|| callback(&event)));

                let ndk_event = match event {
                    input::InputEvent::MotionEvent(e) => {
                        ndk::event::InputEvent::MotionEvent(e.into_ndk_event())
                    }
                    input::InputEvent::KeyEvent(e) => {
                        ndk::event::InputEvent::KeyEvent(e.into_ndk_event())
                    }
                    _ => unreachable!(),
                };

                let handled = match result {
                    Ok(handled) => handled,
                    Err(payload) => {
                        log::error!("Calling `finish_event` after panic in input event handler, to try and avoid being killed via an ANR");
                        queue.finish_event(ndk_event, false);
                        std::panic::resume_unwind(payload);
                    }
                };

                log::trace!("queue: finishing event");
                queue.finish_event(ndk_event, handled == InputStatus::Handled);
            }

            true
        } else {
            log::trace!("queue: no more events");
            false
        }
    }
}
