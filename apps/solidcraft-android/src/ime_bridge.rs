// Builder-generated Android soft-keyboard (IME) bridge (shared module).
//
// A bare NativeActivity has no editable view, so Android's InputMethodManager cannot reliably show
// the soft keyboard and never delivers typed text. The generated Activity (see
// templates/android/SafActivity.java) installs a tiny, invisible but focusable `EditText` whose real
// InputConnection fixes both: the shell calls `imeShow`/`imeHide` through the vendored
// `android-activity`, and this module drains the queued commit/composition/delete events once per
// frame into the egui events the focused `TextEdit` understands. The upstream ArtCraft crates are
// never modified.
//
// ARTCRAFT_ANDROID_IME_BRIDGE_V1
pub mod ime {
    use android_activity::AndroidApp;
    use jni::{
        JavaVM, jni_sig, jni_str,
        objects::{JObject, JString},
    };
    use std::sync::Arc;

    /// Emitted at startup so a device log (and the packaged `.so` strings) can confirm this shell —
    /// including the soft-keyboard bridge — is the one running.
    pub const IME_MARKER: &str = "ARTCRAFT_ANDROID_IME_BRIDGE_V1";

    pub struct ImeBridge {
        app: AndroidApp,
        vm: Arc<JavaVM>,
        /// Set once the Activity reports it has no `imePoll` (an Activity that never adopted the IME
        /// host view), so the per-frame poll stops paying for a call that can only fail.
        unsupported: bool,
    }

    impl ImeBridge {
        pub fn new(app: &AndroidApp) -> Self {
            // SAFETY: AndroidApp owns the live VM pointer for the lifetime of the activity.
            let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut _) };
            Self { app: app.clone(), vm: Arc::new(vm), unsupported: false }
        }

        /// Translate every keyboard event the Activity has queued since the last frame into the egui
        /// IME events the focused `TextEdit` understands. Call once per frame from `raw_input_hook`.
        pub fn poll(&mut self, raw: &mut egui::RawInput) {
            if self.unsupported {
                return;
            }
            loop {
                match self.next_event() {
                    Ok(Some(event)) => {
                        if let Some(event) = translate(&event) {
                            raw.events.push(event);
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        log::warn!("Android soft-keyboard bridge unavailable: {error}");
                        self.unsupported = true;
                        break;
                    }
                }
            }
        }

        fn next_event(&self) -> Result<Option<String>, String> {
            self.vm
                .attach_current_thread(|env| -> jni::errors::Result<Option<String>> {
                    let activity_raw = self.app.activity_as_ptr() as jni::sys::jobject;
                    // SAFETY: AndroidApp retains this global Activity reference; as_cast_raw borrows
                    // it without taking ownership or attempting to delete the reference.
                    let activity = unsafe { env.as_cast_raw::<JObject>(&activity_raw)? };
                    // An Activity that never adopted the IME host has no `imePoll`; invoking it raises
                    // a pending `NoSuchMethodError`. jni deliberately leaves that exception for the
                    // caller to handle, and this thread is shared with the SAF bridge, so clear it
                    // before reporting the missing method (otherwise the next unrelated JNI call sees
                    // a stale pending exception).
                    let value = match env.call_method(&*activity, jni_str!("imePoll"), jni_sig!("()Ljava/lang/String;"), &[]) {
                        Ok(value) => value.l()?,
                        Err(error) => {
                            env.exception_clear();
                            return Err(error);
                        }
                    };
                    if value.is_null() {
                        return Ok(None);
                    }
                    let value = JString::cast_local(env, value)?;
                    let value: String = value.try_to_string(env)?;
                    Ok(Some(value))
                })
                .map_err(|error| error.to_string())
        }
    }

    /// A synthetic Enter press. Both the IME's action key (`a`) and its Enter/newline key (`c` with a
    /// bare newline) become this, so egui decides what Enter means for the focused field.
    fn enter_key() -> egui::Event {
        egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::default() }
    }

    /// Decode one queued event. The first character tags it and the rest is the payload:
    /// `c` commit, `p` pre-edit, `f` finish composing, `d` delete `before,after`, `a` editor action.
    fn translate(event: &str) -> Option<egui::Event> {
        let mut chars = event.chars();
        let tag = chars.next()?;
        let payload = chars.as_str();
        match tag {
            'c' if !payload.is_empty() => {
                // The Enter/newline key arrives as a bare newline. egui deliberately ignores
                // `Commit("\n")` (newlines are handled by `Key::Enter`), so translate it: a multiline
                // field gets a newline, a single-line field confirms and drops focus.
                if payload == "\n" || payload == "\r" || payload == "\r\n" {
                    return Some(enter_key());
                }
                Some(egui::Event::Ime(egui::ImeEvent::Commit(payload.to_owned())))
            }
            'p' => Some(egui::Event::Ime(egui::ImeEvent::Preedit { text: payload.to_owned(), active_range_chars: None })),
            'f' => Some(egui::Event::Ime(egui::ImeEvent::Preedit { text: String::new(), active_range_chars: None })),
            'd' => {
                let (before, after) = payload.split_once(',')?;
                let before_chars = before.trim().parse().ok()?;
                let after_chars = after.trim().parse().ok()?;
                Some(egui::Event::Ime(egui::ImeEvent::DeleteSurrounding { before_chars, after_chars }))
            }
            'a' => {
                let action: i32 = payload.trim().parse().ok()?;
                // IME_ACTION_NONE (0) carries no meaning; any real action (Done/Go/Search/Send)
                // confirms the field.
                if action == 0 {
                    return None;
                }
                Some(enter_key())
            }
            _ => None,
        }
    }
}
