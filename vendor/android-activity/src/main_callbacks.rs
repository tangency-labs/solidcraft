use jni::vm::JavaVM;
use std::{
    ffi::c_void,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{atomic::AtomicBool, Arc, Mutex, Weak},
};

use crate::util::abort_on_panic;

struct CallbackBuffers {
    pub front: Vec<Box<dyn FnOnce() + Send>>,
    pub back: Vec<Box<dyn FnOnce() + Send>>,
}

impl std::fmt::Debug for CallbackBuffers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackBuffers")
            .field("front", &self.front.len())
            .field("back", &self.back.len())
            .finish()
    }
}

impl CallbackBuffers {
    pub fn take_front(&mut self) -> Vec<Box<dyn FnOnce() + Send>> {
        std::mem::swap(&mut self.front, &mut self.back);
        std::mem::take(&mut self.back)
    }

    // After calling `take_front` and draining callbacks then the empty
    // vec should be put back so the capacity can be reused
    //
    // The given `back` vector must be empty
    pub fn replace_back(&mut self, back: Vec<Box<dyn FnOnce() + Send>>) {
        assert!(back.is_empty());
        self.back = back;
    }
}

#[derive(Debug)]
pub(crate) struct MainCallbacksState {
    _pending_detach: AtomicBool,
    event_fd: libc::c_int,
    callbacks: Mutex<CallbackBuffers>,
}

impl Drop for MainCallbacksState {
    fn drop(&mut self) {
        eprintln!("Dropping MainCallbacksState");
        log::warn!("Dropping MainCallbacksState");
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MainCallbacks {
    inner: Arc<MainCallbacksState>,
}

impl std::ops::Deref for MainCallbacks {
    type Target = MainCallbacksState;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl MainCallbacks {
    pub fn new(java_main_looper: &ndk::looper::ForeignLooper) -> Self {
        let java_main_callbacks_event_fd =
            unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
        assert_ne!(
            java_main_callbacks_event_fd, -1,
            "Failed to create Java main looper event fd"
        );

        let inner = Arc::new(MainCallbacksState {
            _pending_detach: AtomicBool::new(false),
            event_fd: java_main_callbacks_event_fd,
            callbacks: Mutex::new(CallbackBuffers {
                front: Vec::new(),
                back: Vec::new(),
            }),
        });

        let weak = Arc::downgrade(&inner);
        let weak = weak.into_raw();
        unsafe {
            ndk_sys::ALooper_addFd(
                java_main_looper.ptr().as_ptr(),
                java_main_callbacks_event_fd,
                ndk_sys::ALOOPER_POLL_CALLBACK,
                ndk_sys::ALOOPER_EVENT_INPUT as libc::c_int,
                Some(run_java_main_callbacks),
                weak as _,
            );
        }

        Self { inner }
    }

    pub fn wake_java_main_for_callbacks(&self) {
        let count: u64 = 1;

        loop {
            match unsafe {
                libc::write(self.event_fd, &count as *const _ as *const libc::c_void, 8)
            } {
                8 => break,
                -1 => {
                    let err = std::io::Error::last_os_error();
                    if err.kind() != std::io::ErrorKind::Interrupted {
                        log::error!("Failure waking up java main loop: {}", err);
                        return;
                    }
                }
                count => {
                    log::error!("Spurious write of {count} bytes while waking up java main loop");
                    return;
                }
            }
        }
    }

    pub fn run_on_java_main_thread<F>(&self, f: Box<F>)
    where
        F: FnOnce() + Send + 'static,
    {
        {
            let mut guard = self.callbacks.lock().unwrap();
            guard.front.push(f);
        }

        self.wake_java_main_for_callbacks();
    }

    // Asynchronously detach the callbacks event fd from the Java main looper
    //
    // Note: we can't do this synchronously because ALooper_removeFd can't
    // guarantee that there isn't already a callback pending (which will still
    // require a valid data pointer)
    //
    // Since the java main Looper runs for the lifetime of the application
    // process we never actually expect to detach the callbacks event fd, and in
    // the unlikely case where there is no future callback after calling
    // `wake_java_main_for_callbacks` then the event fd and `MainCallbacks` will
    // be leaked - but the implication is that the process is about to terminate
    // (otherwise the Looper would still be running)
    pub fn _detach_callbacks_event_fd_from_java_main_looper(&mut self) {
        self._pending_detach
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.wake_java_main_for_callbacks();
    }
}

unsafe extern "C" fn run_java_main_callbacks(fd: i32, events: i32, data: *mut c_void) -> i32 {
    abort_on_panic(|| {
        // Reset the eventfd counter
        if events & ndk_sys::ALOOPER_EVENT_INPUT as i32 != 0 {
            let counter: u64 = 0;
            loop {
                match unsafe { libc::read(fd, &counter as *const _ as *mut libc::c_void, 8) } {
                    8 => break,
                    -1 => {
                        let error = std::io::Error::last_os_error();
                        if error.kind() != std::io::ErrorKind::Interrupted {
                            log::error!("Error reading from fd: {:?}", error);
                            break;
                        }
                    }
                    count => {
                        log::error!("Unexpected read count from event fd: {}", count);
                    }
                }
            }
        }

        let weak_ptr: *const MainCallbacksState = data.cast();
        let weak_ref = Weak::from_raw(weak_ptr);
        let maybe_upgraded = weak_ref.upgrade();

        // Make sure we don't Drop the Weak reference (so the data pointer
        // remains valid for future callbacks)
        let _ = weak_ref.into_raw();

        if let Some(main_callbacks) = maybe_upgraded {
            if main_callbacks
                ._pending_detach
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                let _ = unsafe { libc::close(main_callbacks.event_fd) };
                let _drop_weak = Weak::from_raw(weak_ptr);
                // Returning zero indicates that the fd / callback should be
                // removed from the Looper
                return 0;
            }

            let mut callbacks = main_callbacks.callbacks.lock().unwrap().take_front();

            let jvm = JavaVM::singleton().unwrap();

            for callback in callbacks.drain(0..) {
                let res = jvm.attach_current_thread(|_env| -> jni::errors::Result<()> {
                    let res = catch_unwind(AssertUnwindSafe(|| {
                        callback();
                    }));
                    if let Err(err) = res {
                        log::error!("Panic in Java main/UI thread callback: {:?}", err);
                    }
                    Ok(())
                });
                if let Err(err) = res {
                    log::error!(
                        "JNI Error while running Java main/UI thread callback: {:?}",
                        err
                    );
                }
            }

            // put callbacks vec back so we can keep reusing its capacity
            let mut guard = main_callbacks.callbacks.lock().unwrap();
            guard.replace_back(callbacks);
        }

        1
    })
}
