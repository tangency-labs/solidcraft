use jni::{
    jni_sig, jni_str,
    objects::{JObject, JString, JThread},
    vm::JavaVM,
};
use log::Level;
use ndk::asset::AssetManager;
use std::{
    ffi::{c_void, CStr, CString},
    fs::File,
    io::{BufRead as _, BufReader},
    os::fd::{FromRawFd as _, RawFd},
    sync::OnceLock,
};

use crate::{
    main_callbacks::MainCallbacks, util::android_log, OnCreateState, ANDROID_ACTIVITY_TAG,
};

fn forward_stdio_to_logcat() -> std::thread::JoinHandle<std::io::Result<()>> {
    // XXX: make this stdout/stderr redirection an optional / opt-in feature?...

    let file = unsafe {
        let mut logpipe: [RawFd; 2] = Default::default();
        libc::pipe2(logpipe.as_mut_ptr(), libc::O_CLOEXEC);
        libc::dup2(logpipe[1], libc::STDOUT_FILENO);
        libc::dup2(logpipe[1], libc::STDERR_FILENO);
        libc::close(logpipe[1]);

        File::from_raw_fd(logpipe[0])
    };

    std::thread::Builder::new()
        .name("stdio-to-logcat".to_string())
        .spawn(move || -> std::io::Result<()> {
            let tag = c"RustStdoutStderr";
            let mut reader = BufReader::new(file);
            let mut buffer = String::new();
            loop {
                buffer.clear();
                let len = match reader.read_line(&mut buffer) {
                    Ok(len) => len,
                    Err(e) => {
                        android_log(
                            Level::Error,
                            ANDROID_ACTIVITY_TAG,
                            &CString::new(format!(
                                "Logcat forwarder failed to read stdin/stderr: {e:?}"
                            ))
                            .unwrap(),
                        );
                        break Err(e);
                    }
                };
                if len == 0 {
                    break Ok(());
                } else if let Ok(msg) = CString::new(buffer.clone()) {
                    android_log(Level::Info, tag, &msg);
                }
            }
        })
        .expect("Failed to start stdout/stderr to logcat forwarder thread")
}

unsafe extern "C" fn _android_activity_anchor() {}

/// Get a handle to the shared library that we are linked into, so that we can
/// look up symbols within it.
fn dlopen_self() -> Result<*mut c_void, String> {
    unsafe {
        let mut info: libc::Dl_info = std::mem::zeroed();

        // NB: `dladdr` does not update the `dlerror` state
        if libc::dladdr(
            _android_activity_anchor as *const () as *const c_void,
            &mut info,
        ) == 0
        {
            return Err("dladdr failed".into());
        }
        if info.dli_fname.is_null() {
            return Err("dladdr returned null dli_fname".into());
        }

        // Clear any existing error
        libc::dlerror();

        let handle = libc::dlopen(info.dli_fname, libc::RTLD_NOW | libc::RTLD_NOLOAD);
        if handle.is_null() {
            let err = CStr::from_ptr(libc::dlerror())
                .to_string_lossy()
                .into_owned();
            let path = CStr::from_ptr(info.dli_fname)
                .to_string_lossy()
                .into_owned();
            return Err(format!("dlopen({path}) failed: {err}"));
        }

        Ok(handle)
    }
}

/// Look up a symbol within our own shared library
///
/// This can be used to look up optional application entry points, such as
/// `android_on_create`
///
/// Returns `None` if the symbol is not found (which is not considered an error)
fn lookup_self_symbol(symbol: &CStr) -> Option<*mut c_void> {
    unsafe {
        let handle = match dlopen_self() {
            Ok(h) => h,
            Err(err) => {
                let msg = format!(
                    "Warning: failed to dlopen self, looking for symbol {}: {err}",
                    symbol.to_string_lossy()
                );
                android_log(
                    Level::Warn,
                    ANDROID_ACTIVITY_TAG,
                    &CString::new(msg).unwrap(),
                );
                return None;
            }
        };

        // Clear any existing error
        libc::dlerror();

        let sym = libc::dlsym(handle, symbol.as_ptr());

        // Close the handle to avoid leaking a reference count
        if libc::dlclose(handle) != 0 {
            let err = CStr::from_ptr(libc::dlerror())
                .to_string_lossy()
                .into_owned();
            let msg = format!("dlclose failed for self handle: {err}");
            android_log(
                Level::Warn,
                ANDROID_ACTIVITY_TAG,
                &CString::new(msg).unwrap(),
            );
        }

        if sym.is_null() {
            None
        } else {
            Some(sym)
        }
    }
}

/// Attempt to call an optional "android_on_create" entry point within the
/// application's shared library
///
/// Note: this function does not propagate any errors, while it's assumed that
/// this is called within an `onCreate` native method.
///
/// # Safety
///
/// - This must be called from the Java main thread, while onCreate is running
/// - The `jni_activity` pointer must be a valid JNI reference to the Java
///   Activity instance being created
///
/// The safety here also depends on the application declaring an
/// `android_on_create` function with the correct signature. (It's safe to not
/// declare an `android_on_create` function at all, and the code will simply
/// skip calling it)
pub(crate) unsafe fn init_java_main_thread_on_create(
    jvm: JavaVM,
    jni_activity: *mut c_void,
    saved_state: &[u8],
) {
    let _join_log_forwarder = forward_stdio_to_logcat();

    let msg = CString::new(format!(
        "Creating: Activity = {:p}, saved state size = {}",
        jni_activity,
        saved_state.len()
    ))
    .unwrap();
    android_log(Level::Info, ANDROID_ACTIVITY_TAG, &msg);

    // SAFETY: It's the application's responsibility to declare any `android_on_create`
    // function with the correct signature and ABI.
    let android_on_create: extern "Rust" fn(state: &OnCreateState) = unsafe {
        let Some(symbol) = lookup_self_symbol(c"android_on_create") else {
            // android_on_create is optional, so simply return if not found
            return;
        };
        std::mem::transmute(symbol)
    };

    let state = OnCreateState::new(jvm.clone(), jni_activity, saved_state);
    // Catch any exceptions from the callback and log them instead of allowing any
    // exception to propagate back to the Activity.
    let res = jvm.attach_current_thread(|_env| -> jni::errors::Result<()> {
        android_on_create(&state);
        Ok(())
    });
    if let Err(err) = res {
        let msg = CString::new(format!(
            "JNI error while running android_on_create: {:?}",
            err
        ))
        .unwrap();
        android_log(Level::Error, ANDROID_ACTIVITY_TAG, &msg);
    }
}

struct AppState {
    main_callbacks: MainCallbacks,
    app_asset_manager: AssetManager,
}

static APP_ONCE: OnceLock<AppState> = OnceLock::new();

// Get the Application instance from the Activity
fn get_application<'local, 'any>(
    env: &mut jni::Env<'local>,
    activity: &JObject<'any>,
) -> jni::errors::Result<JObject<'local>> {
    let app = env
        .call_method(
            activity,
            jni_str!("getApplication"),
            jni_sig!(() -> android.app.Application),
            &[],
        )?
        .l()?;
    Ok(app)
}

fn get_assets<'local, 'any>(
    env: &mut jni::Env<'local>,
    application: &JObject<'any>,
) -> jni::errors::Result<JObject<'local>> {
    let assets_manager = env
        .call_method(
            application,
            jni_str!("getAssets"),
            jni_sig!(() -> android.content.res.AssetManager),
            &[],
        )?
        .l()?;
    Ok(assets_manager)
}

fn try_init_current_thread(env: &mut jni::Env, activity: &JObject) -> jni::errors::Result<()> {
    let activity_class = env.get_object_class(activity)?;
    let class_loader = activity_class.get_class_loader(env)?;

    let thread = JThread::current_thread(env)?;
    thread.set_context_class_loader(env, &class_loader)?;
    let thread_name = JString::from_jni_str(env, jni_str!("android_main"))?;
    thread.set_name(env, &thread_name)?;

    // Also name native thread - this needs to happen here after attaching to a JVM thread,
    // since that changes the thread name to something like "Thread-2".
    unsafe {
        let thread_name = c"android_main";
        let _ = libc::pthread_setname_np(libc::pthread_self(), thread_name.as_ptr());
    }
    Ok(())
}

/// Name the Java Thread + native thread "android_main" and set the Java Thread context class loader
/// so that jni code can more-easily find non-system Java classes.
pub(crate) fn init_android_main_thread(
    vm: &JavaVM,
    jni_activity: &JObject,
    java_main_looper: &ndk::looper::ForeignLooper,
) -> jni::errors::Result<(AssetManager, MainCallbacks)> {
    vm.with_local_frame(10, |env| -> jni::errors::Result<_> {
        let app_state = APP_ONCE.get_or_init(|| unsafe {
            let application =
                get_application(env, jni_activity).expect("Failed to get Application instance");
            let app_asset_manager =
                get_assets(env, &application).expect("Failed to get AssetManager");
            let app_global = env
                .new_global_ref(application)
                .expect("Failed to create global ref for Application");
            // Make sure we don't delete the global reference via Drop
            let app_global = app_global.into_raw();
            ndk_context::initialize_android_context(vm.get_raw().cast(), app_global.cast());

            let asset_manager_global = env
                .new_global_ref(app_asset_manager)
                .expect("Failed to create global ref for AssetManager");
            // Make sure we don't delete the global reference via Drop because
            // the AAssetManager pointer will only be valid while we can
            // guarantee that the Java AssetManager is not garbage collected
            let asset_manager_global = asset_manager_global.into_raw();
            let asset_manager_ptr =
                ndk_sys::AAssetManager_fromJava(env.get_raw() as _, asset_manager_global as _);
            assert_ne!(
                asset_manager_ptr,
                std::ptr::null_mut(),
                "Failed to get Application AAssetManager"
            );
            let app_asset_manager =
                AssetManager::from_ptr(std::ptr::NonNull::new(asset_manager_ptr).unwrap());

            let main_callbacks = MainCallbacks::new(java_main_looper);

            AppState {
                main_callbacks,
                app_asset_manager,
            }
        });

        if let Err(err) = try_init_current_thread(env, jni_activity) {
            let msg =
                CString::new(format!("Failed to initialize Java thread state: {:?}", err)).unwrap();
            android_log(Level::Error, ANDROID_ACTIVITY_TAG, &msg);
        }

        let asset_manager = unsafe { AssetManager::from_ptr(app_state.app_asset_manager.ptr()) };
        let main_callbacks = app_state.main_callbacks.clone();

        Ok((asset_manager, main_callbacks))
    })
}
