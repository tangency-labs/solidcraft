//! Storage Access Framework pickers for Android: the system's "Open document" and "Create
//! document" screens, and the single place where they meet the rest of SolidCraft.
//!
//! Android hands a chosen document back only as a `content://` uri, and only to an Activity's
//! `onActivityResult` — a callback the `NativeActivity` of this package never receives. The
//! Java shim (`java/io/solidcraft/SolidcraftActivity.java`) catches it and calls
//! [`native_result`](imp::native_result), a native method the shell registers before any
//! picker opens; the pending [`Picker`](imp::Picker) future reads that answer from
//! [`ANSWER`](imp::ANSWER).
//!
//! A picked document then becomes one of two things. **Open** copies it into the app's own
//! store — designs beside the autosaves, everything else into `imports/` — and the copy is
//! what the design works on. **Save As** (a design) hands the engine a mirror in `designs/`
//! and remembers the document beside it; **exports** hand the engine a temporary. [`deliver`]
//! runs every frame afterwards and closes the loop: each temporary is copied to its document
//! once, each mirror is copied over whenever the session has been clean since the last copy.
//!
//! Everything that needs no JNI — names, free paths, mime types, the mirror's bookkeeping —
//! is plain Rust at the top of this module and covered by host tests.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// A file name from a dialog field or a document provider stays this long (it is user input).
const MAX_NAME: usize = 180;
/// A free path search gives up after this many taken names; the picker then cancels.
const NAME_TRIES: usize = 100;
/// The only principal type that satisfies the system picker's `*/*` filter whatever the
/// command wants (Android forbids a MIME type of exactly `*`).
pub(crate) const ANY_TYPE: &str = "*/*";

/// Where a picked document belongs once the engine has written its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    /// An export: the engine writes `temp`, and [`deliver`](imp::deliver) copies the file to
    /// `uri` exactly once, then drops both.
    OneShot {
        uri: String,
        temp: PathBuf,
        /// The document's file name, for the status line.
        name: String,
    },
    /// A saved design: the engine writes `mirror`; [`deliver`](imp::deliver) copies every
    /// clean save to `uri`. `synced` is the session revision whose bytes last arrived there.
    Linked { uri: String, mirror: PathBuf, synced: Option<u64> },
}

/// The picked documents of this process that are still waiting for bytes.
#[derive(Default)]
pub(crate) struct Targets(RefCell<Vec<Target>>);

impl Targets {
    /// Remember `target`, replacing any earlier target for the same document: re-saving the
    /// same document must not leave a previous mirror behind to write to it as well.
    pub(crate) fn upsert(&self, target: Target) {
        let uri = match &target {
            Target::OneShot { uri, .. } | Target::Linked { uri, .. } => uri.clone(),
        };
        let mut list = self.0.borrow_mut();
        list.retain(|held| match held {
            Target::OneShot { uri: held, .. } | Target::Linked { uri: held, .. } => *held != uri,
        });
        list.push(target);
    }
}

/// The dialog's `name`, kept to one file name of a sane length (it is user input: no
/// directories, no `..`, no endless names); an unusable name becomes `fallback`.
pub(crate) fn sanitize_name(name: &str, fallback: &str, max: usize) -> String {
    let base = Path::new(name.trim()).file_name().and_then(|n| n.to_str()).unwrap_or("");
    if base.is_empty() {
        return fallback.chars().take(max).collect();
    }
    base.chars().take(max).collect()
}

/// A free path in `dir` for `name`: the name itself, else `dir/<stem> (2)<ext>` and so on.
/// Existing files are never replaced — neither picker offers a confirm dialog.
pub(crate) fn free_path(dir: &Path, name: &str, fallback: &str) -> Option<PathBuf> {
    let _ = std::fs::create_dir_all(dir);
    let base = sanitize_name(name, fallback, MAX_NAME);
    if base.is_empty() {
        return None;
    }
    let (stem, ext) = match base.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !e.is_empty() => (s.to_string(), Some(e.to_string())),
        _ => (base.clone(), None),
    };
    for n in 1..=NAME_TRIES {
        let file = if n == 1 {
            dir.join(&base)
        } else {
            match &ext {
                Some(e) => dir.join(format!("{stem} ({n}).{e}")),
                None => dir.join(format!("{stem} ({n})")),
            }
        };
        // An unreadable entry counts as taken: never replace what cannot be listed.
        if matches!(std::fs::exists(&file), Ok(false)) {
            return Some(file);
        }
    }
    None
}

/// The mime type a document provider should expect for an export of `ext` (the picker
/// suggests the extension from it). Anything else — designs above all — stays generic: the
/// filter must never hide the provider's own way of creating the file.
pub(crate) fn export_mime(ext: &str) -> &'static str {
    match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "json" => "application/json",
        "dxf" => "image/vnd.dxf",
        _ => "application/octet-stream",
    }
}

/// The principal mime type the picker's intent must carry: the system picker's own filter
/// (`DocumentsUI` declares `*/*` for `OPEN_DOCUMENT`) matches no intent without one, and
/// `startActivityForResult` then throws. `*/*` satisfies every command's filter; an export
/// with a known type asks for it directly so its screen starts on the right file type.
pub(crate) fn principal_mime(ext: &str) -> &'static str {
    if ext.is_empty() { ANY_TYPE } else { export_mime(ext) }
}

/// The `EXTRA_MIME_TYPES` set a picker offers for `exts` (an extension list), `None` when
/// there is nothing to refine `principal` with. Android requires the extra to be a strict
/// refinement, so a type equal to the principal — and the `*` wildcard, which `principal`
/// already covers — is dropped.
pub(crate) fn extra_mime_types(principal: &str, exts: &[String]) -> Option<Vec<&'static str>> {
    let refined = exts.iter().filter(|e| e.as_str() != "*").map(|e| export_mime(e)).filter(|t| *t != principal);
    let types: Vec<&'static str> = refined.collect();
    (!types.is_empty()).then_some(types)
}

/// The extension a picked file's materialized copy should carry: the provider name's own
/// extension when it has one, else the uri's, else the first one the command offered (a
/// provider may report a name without an extension — the engine reads the format from the
/// path, so an `imports/1075` would be refused by `file.insert_step`, `doc.open`, …).
pub(crate) fn materialized_ext(shown: &str, uri: &str, offered: &[String]) -> Option<String> {
    let real = |name: &str| Path::new(name).extension().and_then(|e| e.to_str()).filter(|e| !e.is_empty()).map(str::to_ascii_lowercase);
    real(shown)
        .or_else(|| display_from_uri(uri).as_deref().and_then(real))
        .or_else(|| offered.iter().find(|e| e.as_str() != "*").map(|e| e.to_ascii_lowercase()))
}

/// `name` with `ext` appended when it has none: the copy keeps the provider's name, but the
/// engine reads the format from the path, so a bare `1075` would not be recognized. A name
/// that already carries an extension is left alone (the provider's choice wins).
pub(crate) fn with_ext(name: &str, ext: Option<&str>) -> String {
    let name = sanitize_name(name, name, MAX_NAME);
    match (Path::new(&name).extension(), ext) {
        (None, Some(want)) => format!("{name}.{want}"),
        _ => name,
    }
}

/// The file name a document provider shows for a document, as far as the uri itself says:
/// the last path segment of the percent-decoded uri, after its last `/` or `:` (a documents
/// provider keeps names like `primary:Download/part.step` in one segment).
pub(crate) fn display_from_uri(uri: &str) -> Option<String> {
    let decoded = percent_decode(uri);
    let segment = decoded.rsplit(['/', ':']).next()?;
    let name = sanitize_name(segment, "", MAX_NAME);
    (!name.is_empty()).then_some(name)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_byte(bytes[i + 1]), hex_byte(bytes[i + 2]))
        {
            out.push(hi * 16 + lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_byte(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Whether the mirror of a linked design is behind its document: the session is clean, its
/// path is that mirror (the user has not Save As'd elsewhere since), and the bytes have not
/// been copied over since the last change (`revision` also moves when undo returns to the
/// saved state, so a redundant copy may happen — never a missing one).
pub(crate) fn needs_sync(dirty: bool, path_matches: bool, synced: Option<u64>, revision: u64) -> bool {
    !dirty && path_matches && synced != Some(revision)
}

#[cfg(target_os = "android")]
mod imp {
    use super::*;
    use std::future::Future;
    use std::pin::Pin;
    use std::rc::Rc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::{Context, Poll};

    use jni::objects::{JClass, JObject, JString};
    use jni::refs::Reference as _;
    use jni::sys::jint;
    use jni::{Env, JValue, JavaVM, errors::LogErrorAndDefault, jni_sig, jni_str, native_method};
    use solidcraft_engine::recovery;
    use solidcraft_ui_egui::SolveApp;
    use winit::platform::android::activity::AndroidApp;

    /// The request codes the launch picks; an answer is only believed for its own request.
    const REQUEST_OPEN: jint = 1001;
    const REQUEST_SAVE: jint = 1002;
    /// `android.app.Activity.RESULT_OK` — the only result that carries a chosen document.
    const RESULT_OK: jint = -1;
    /// The documents query column that carries the name the provider shows.
    const DISPLAY_NAME: &str = "_display_name";
    /// `Intent.CATEGORY_OPENABLE`: only documents the system can hand us as a stream.
    const CATEGORY_OPENABLE: &str = "android.intent.category.OPENABLE";
    /// `Intent.EXTRA_MIME_TYPES`: the extra types a multi-extension command accepts.
    const EXTRA_MIME_TYPES: &str = "android.intent.extra.MIME_TYPES";
    /// One JNI array per write chunk: small enough to move without holding the world.
    const CHUNK: usize = 64 * 1024;

    /// The shim's answer: `(requestCode, resultCode, uri)`, written by [`native_result`] (a
    /// system picker) or by [`launch`] when the picker could not open, read once by the
    /// pending [`Picker`].
    static ANSWER: Mutex<Option<(jint, jint, Option<String>)>> = Mutex::new(None);

    /// `SolidcraftActivity.nativeResult`: the shim's picker answer. Registered before any
    /// picker opens (see [`register_result_method`]), which both resolves the method without
    /// an exported symbol and forces this constant — wrapper and all — into the binary.
    // (jni's generated wrapper still calls std's renamed `Atomic::fetch_update`.)
    #[allow(deprecated)]
    const RESULT_METHOD: jni::NativeMethod = native_method! {
        java_type = "io.solidcraft.SolidcraftActivity",
        error_policy = LogErrorAndDefault,
        static extern fn native_result(request: jint, result: jint, uri: JString) -> void,
    };

    fn native_result<'local>(
        _env: &mut Env<'local>,
        _class: JClass<'local>,
        request: jint,
        result: jint,
        uri: JString<'local>,
    ) -> Result<(), jni::errors::Error> {
        let uri = if uri.is_null() { None } else { Some(uri.try_to_string(_env)?) };
        *ANSWER.lock().unwrap_or_else(|e| e.into_inner()) = Some((request, result, uri));
        Ok(())
    }

    /// Which screen a picker opens.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Kind {
        /// Pick a file to copy into the app's own store.
        Open,
        /// Create the design's mirror; every later save follows the document.
        Design,
        /// Create a file the export is copied into once.
        Export,
    }

    /// One pending Storage Access Framework picker, as the file picker's future. The first
    /// poll opens the screen (on the Java main thread); later polls wait for the shim's
    /// answer; the completion gets the path the engine must write.
    pub(crate) struct Picker {
        app: AndroidApp,
        targets: Rc<Targets>,
        kind: Kind,
        /// The suggested file name (empty for Open).
        name: String,
        /// The principal mime type for the screen (`*/*`: let the user choose any document).
        mime: &'static str,
        /// The extensions the command accepts (`["*"]` for Open): the export temp takes the
        /// first so the engine's own format check sees the format it was asked for, and all
        /// of them become the picker's offered types. Owned: the picker outlives the caller.
        exts: Vec<String>,
        request: jint,
        launched: bool,
    }

    impl Picker {
        pub(crate) fn open(app: AndroidApp, targets: Rc<Targets>, exts: &[&str]) -> Self {
            Picker {
                app,
                targets,
                kind: Kind::Open,
                name: String::new(),
                mime: ANY_TYPE,
                exts: if exts.is_empty() { vec!["*".into()] } else { exts.iter().map(|e| (*e).to_string()).collect() },
                request: REQUEST_OPEN,
                launched: false,
            }
        }

        pub(crate) fn design(app: AndroidApp, targets: Rc<Targets>, name: &str) -> Self {
            Picker {
                app,
                targets,
                kind: Kind::Design,
                name: name.to_string(),
                mime: ANY_TYPE,
                exts: Vec::new(),
                request: REQUEST_SAVE,
                launched: false,
            }
        }

        pub(crate) fn export(app: AndroidApp, targets: Rc<Targets>, name: &str, exts: &[&str]) -> Self {
            let ext = name.rsplit('.').next().unwrap_or("");
            Picker {
                app,
                targets,
                kind: Kind::Export,
                name: name.to_string(),
                mime: principal_mime(ext),
                exts: exts.iter().map(|e| (*e).to_string()).collect(),
                request: REQUEST_SAVE,
                launched: false,
            }
        }

        /// The path the completion hands back: the materialized copy (Open), the engine's
        /// mirror (Design) or the temporary the export is written to (Export).
        fn finish(&self, uri: &str) -> Option<String> {
            let shown = query_display_name(&self.app, uri).or_else(|| display_from_uri(uri)).unwrap_or_else(|| self.name.clone());
            match self.kind {
                Kind::Open => {
                    // Designs land beside the autosaves (Save then keeps that copy current,
                    // as the web build keeps its downloads); everything else is imported.
                    // The copy keeps a usable extension: a provider that reports only a name
                    // (`1075`) would otherwise leave the engine unable to read the format.
                    let ext = materialized_ext(&shown, uri, &self.exts);
                    let name = with_ext(&shown, ext.as_deref());
                    let sub = if solidcraft_engine::io::is_design_path(&name) { "designs" } else { "imports" };
                    let dir = recovery::data_dir()?.join(sub);
                    let path = free_path(&dir, &name, "imported")?;
                    let bytes = read_uri(&self.app, uri, solidcraft_engine::io::MAX_DESIGN_BYTES).ok()?;
                    if let Err(e) = std::fs::write(&path, &bytes) {
                        log::error!("Android: could not copy the picked document to {}: {e}", path.display());
                        return None;
                    }
                    Some(path.to_string_lossy().into_owned())
                }
                Kind::Design => {
                    // The suggested name is the dialog's, built by the caller (a design
                    // name): never empty, never a path.
                    let dir = recovery::data_dir()?.join("designs");
                    let mirror = free_path(&dir, &shown, &self.name)?;
                    self.targets.upsert(Target::Linked { uri: uri.to_string(), mirror: mirror.clone(), synced: None });
                    Some(mirror.to_string_lossy().into_owned())
                }
                Kind::Export => {
                    // The engine names the format from the path's extension, so the temporary
                    // must carry the extension the command offered (`render.export` and
                    // `file.export` both reject a name without it). The provider's own name is
                    // display only and need not have one.
                    let dir = recovery::data_dir()?.join("exports");
                    let ext = self.exts.first().map(String::as_str).unwrap_or("");
                    let temp = free_path(&dir, &self.name, &format!("{}.{ext}", self.name))?;
                    self.targets.upsert(Target::OneShot { uri: uri.to_string(), temp: temp.clone(), name: self.name.clone() });
                    Some(temp.to_string_lossy().into_owned())
                }
            }
        }
    }

    impl Future for Picker {
        type Output = Option<String>;

        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();
            if !this.launched {
                this.launched = true;
                launch(&this.app, this.kind, &this.name, this.mime, &this.exts, this.request);
                // The answer of this launch can only arrive on a later poll; the picker's
                // tick repaints every 50 ms until then.
                return Poll::Pending;
            }
            let Some((code, result, uri)) = ANSWER.lock().ok().and_then(|mut slot| slot.take()) else {
                return Poll::Pending;
            };
            if code != this.request {
                // An answer no future waited for (the launcher failed after all, or the
                // process outlived an earlier picker): ours has not arrived yet.
                return Poll::Pending;
            }
            if result != RESULT_OK {
                return Poll::Ready(None);
            }
            let Some(uri) = uri else { return Poll::Ready(None) };
            Poll::Ready(this.finish(&uri))
        }
    }

    /// Open the system's document screen. Everything that must happen on the Java main
    /// thread — resolving our own class, building the intent, starting the activity — is
    /// posted there; a failure answers the future with a cancel instead of leaving it pending.
    fn launch(app: &AndroidApp, kind: Kind, name: &str, mime: &str, exts: &[String], request: jint) {
        *ANSWER.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let app = app.clone();
        let name = name.to_string();
        let mime = mime.to_string();
        let exts: Vec<String> = exts.iter().map(|e| (*e).to_string()).collect();
        app.clone().run_on_java_main_thread(Box::new(move || {
            if let Err(e) = launch_on_main(&app, kind, &name, &mime, &exts, request) {
                log::error!("Android: the file picker did not open: {e}");
                *ANSWER.lock().unwrap_or_else(|err| err.into_inner()) = Some((request, 0, None));
            }
        }));
    }

    fn launch_on_main(app: &AndroidApp, kind: Kind, name: &str, mime: &str, exts: &[String], request: jint) -> Result<(), String> {
        with_env(app, |env| -> jni::errors::Result<Result<(), String>> {
            let activity = activity(env, app);
            // Before anything can be picked, and therefore before the shim can call the
            // native method: register it through the Activity's own class (obtained by
            // calling `getClass()`, never by loading the class by name — a posted callback
            // has no Java frame whose class loader could find it).
            if let Err(e) = register_result_method(env, &activity) {
                return Ok(Err(e.to_string()));
            }
            let action = match kind {
                Kind::Open => "android.intent.action.OPEN_DOCUMENT",
                Kind::Design | Kind::Export => "android.intent.action.CREATE_DOCUMENT",
            };
            let action = JString::new(env, action)?;
            let intent = env.new_object(jni_str!("android/content/Intent"), jni_sig!("(Ljava/lang/String;)V"), &[JValue::Object(&action)])?;
            // The system picker's own filter requires a MIME type (`DocumentsUI` declares
            // `*/*` for OPEN_DOCUMENT); an intent with none fails the match and
            // `startActivityForResult` throws. `mime` is the principal (`*/*` for Open and
            // designs, the export's own type otherwise); the command's extensions refine it
            // through `EXTRA_MIME_TYPES` when they name a narrower set.
            let offered = extra_mime_types(mime, exts);
            let principal = JString::new(env, mime)?;
            env.call_method(&intent, jni_str!("setType"), jni_sig!("(Ljava/lang/String;)Landroid/content/Intent;"), &[JValue::Object(&principal)])?;
            let openable = JString::new(env, CATEGORY_OPENABLE)?;
            env.call_method(
                &intent,
                jni_str!("addCategory"),
                jni_sig!("(Ljava/lang/String;)Landroid/content/Intent;"),
                &[JValue::Object(&openable)],
            )?;
            if let Some(offered) = offered {
                let key = JString::new(env, EXTRA_MIME_TYPES)?;
                let types = env.new_object_array(offered.len() as i32, jni_str!("java/lang/String"), JObject::null())?;
                for (i, ty) in offered.iter().enumerate() {
                    let ty = JString::new(env, ty)?;
                    types.set_element(env, i, &ty)?;
                }
                env.call_method(
                    &intent,
                    jni_str!("putExtra"),
                    jni_sig!("(Ljava/lang/String;[Ljava/lang/String;)Landroid/content/Intent;"),
                    &[JValue::Object(&key), JValue::Object(&types)],
                )?;
            }
            if kind != Kind::Open {
                let key = JString::new(env, "android.intent.extra.TITLE")?;
                let title = JString::new(env, name)?;
                env.call_method(
                    &intent,
                    jni_str!("putExtra"),
                    jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;"),
                    &[JValue::Object(&key), JValue::Object(&title)],
                )?;
            }
            env.call_method(
                &activity,
                jni_str!("startActivityForResult"),
                jni_sig!("(Landroid/content/Intent;I)V"),
                &[JValue::Object(&intent), JValue::Int(request)],
            )?;
            Ok(Ok(()))
        })?
    }

    /// Resolve `nativeResult` for the shim once: registered natives take precedence over
    /// symbol search, so the answer never depends on how (or whether) an exported symbol was
    /// linked, and evaluating [`RESULT_METHOD`] here is what puts its wrapper in the binary.
    #[allow(unsafe_code)] // SAFETY: on the `unsafe` inside — RegisterNatives of our own method.
    fn register_result_method(env: &mut Env<'_>, activity: &JObject<'_>) -> jni::errors::Result<()> {
        static DONE: AtomicBool = AtomicBool::new(false);
        if DONE.load(Ordering::Acquire) {
            return Ok(());
        }
        let class = env.call_method(activity, jni_str!("getClass"), jni_sig!("()Ljava/lang/Class;"), &[])?.l()?;
        let class = JClass::cast_local(env, class)?;
        // SAFETY: `RESULT_METHOD` names our own static method with exactly the signature the
        // shim declares; `RegisterNatives` only stores the wrapper's address.
        unsafe { env.register_native_methods(&class, &[RESULT_METHOD]) }?;
        DONE.store(true, Ordering::Release);
        Ok(())
    }

    /// The running Activity as a JNI object. The reference is unowned: it must not outlive
    /// the frame it is created in, and the `AndroidApp` behind it outlives every call here.
    #[allow(unsafe_code)] // SAFETY: on the `unsafe` inside — the Activity pointer's lifetime.
    fn activity<'local>(env: &Env<'local>, app: &AndroidApp) -> JObject<'local> {
        let raw = app.activity_as_ptr() as jni::sys::jobject;
        // SAFETY: `activity_as_ptr` is the process's Activity global for as long as the app
        // runs, and `JObject` has no `Drop`, so nothing is released when it goes out of scope.
        unsafe { JObject::from_raw(env, raw) }
    }

    /// A JNI environment for this thread, attached on first use (cheap when already
    /// attached). The closure's JNI errors come back as text so callers can mix them with
    /// ordinary Rust errors; only owned data may leave the closure.
    #[allow(unsafe_code)] // SAFETY: on the `unsafe` inside — the process's own VM pointer.
    fn with_env<R>(app: &AndroidApp, f: impl FnOnce(&mut Env<'_>) -> jni::errors::Result<R>) -> Result<R, String> {
        // SAFETY: the VM pointer comes from this process's `AndroidApp` and stays valid for
        // its lifetime — the pattern `android-activity` documents for jni 0.22.
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as _) };
        vm.attach_current_thread(f).map_err(|e| e.to_string())
    }

    fn parse_uri<'local>(env: &mut Env<'local>, uri: &str) -> jni::errors::Result<JObject<'local>> {
        let uri = JString::new(env, uri)?;
        env.call_static_method(
            jni_str!("android/net/Uri"),
            jni_str!("parse"),
            jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"),
            &[JValue::Object(&uri)],
        )?
        .l()
    }

    fn content_resolver<'local>(env: &mut Env<'local>, app: &AndroidApp) -> jni::errors::Result<JObject<'local>> {
        let activity = activity(env, app);
        env.call_method(&activity, jni_str!("getContentResolver"), jni_sig!("()Landroid/content/ContentResolver;"), &[])?.l()
    }

    /// The `_display_name` the provider shows for the document. `None` when the provider has
    /// no such column — the caller then falls back to the uri's own path segment.
    fn query_display_name(app: &AndroidApp, uri: &str) -> Option<String> {
        with_env(app, |env| -> jni::errors::Result<Option<String>> {
            let resolver = content_resolver(env, app)?;
            let uri_obj = parse_uri(env, uri)?;
            let projection = env.new_object_array(1, jni_str!("java/lang/String"), JObject::null())?;
            let column = JString::new(env, DISPLAY_NAME)?;
            projection.set_element(env, 0, &column)?;
            let cursor = env
                .call_method(
                    &resolver,
                    jni_str!("query"),
                    jni_sig!("(Landroid/net/Uri;[Ljava/lang/String;)Landroid/database/Cursor;"),
                    &[JValue::Object(&uri_obj), JValue::Object(&projection)],
                )?
                .l()?;
            let mut name = None;
            if !cursor.is_null() {
                if env.call_method(&cursor, jni_str!("moveToFirst"), jni_sig!("()Z"), &[])?.z()? {
                    let index = env
                        .call_method(&cursor, jni_str!("getColumnIndexOrThrow"), jni_sig!("(Ljava/lang/String;)I"), &[JValue::Object(&column)])?
                        .i()?;
                    let value = env.call_method(&cursor, jni_str!("getString"), jni_sig!("(I)Ljava/lang/String;"), &[JValue::Int(index)])?.l()?;
                    if !value.is_null() {
                        name = Some(JString::cast_local(env, value)?.try_to_string(env)?);
                    }
                }
                // On any error above the cursor is left to the collector, which closes it.
                let _ = env.call_method(&cursor, jni_str!("close"), jni_sig!("()V"), &[]);
            }
            Ok(name)
        })
        .ok()
        .flatten()
    }

    /// The document's bytes, at most `cap` of them (a provider is not ours: it may claim a
    /// document of any size).
    fn read_uri(app: &AndroidApp, uri: &str, cap: usize) -> Result<Vec<u8>, String> {
        with_env(app, |env| -> jni::errors::Result<Result<Vec<u8>, String>> {
            let resolver = content_resolver(env, app)?;
            let uri_obj = parse_uri(env, uri)?;
            let stream = env
                .call_method(
                    &resolver,
                    jni_str!("openInputStream"),
                    jni_sig!("(Landroid/net/Uri;)Ljava/io/InputStream;"),
                    &[JValue::Object(&uri_obj)],
                )?
                .l()?;
            if stream.is_null() {
                return Ok(Err("the document could not be opened".into()));
            }
            let mut out: Vec<u8> = Vec::new();
            let buf = env.byte_array_from_slice(&[0u8; CHUNK])?;
            loop {
                let n = env.call_method(&stream, jni_str!("read"), jni_sig!("([B)I"), &[JValue::Object(&buf)])?.i()?;
                if n <= 0 {
                    break;
                }
                let chunk = env.convert_byte_array(&buf)?;
                let take = (n as usize).min(chunk.len());
                if out.len() + take > cap {
                    return Ok(Err(format!("the document is larger than the {cap} byte limit")));
                }
                out.extend_from_slice(&chunk[..take]);
            }
            // Success closes the stream; the error paths above leave theirs to the collector
            // (which closes it when the Java object goes).
            let _ = env.call_method(&stream, jni_str!("close"), jni_sig!("()V"), &[]);
            Ok(Ok(out))
        })?
    }

    /// The document's new bytes, chunked (a whole design need not fit in one JNI array).
    fn write_uri(app: &AndroidApp, uri: &str, bytes: &[u8]) -> Result<(), String> {
        with_env(app, |env| -> jni::errors::Result<Result<(), String>> {
            let resolver = content_resolver(env, app)?;
            let uri_obj = parse_uri(env, uri)?;
            let mode = JString::new(env, "w")?;
            let stream = env
                .call_method(
                    &resolver,
                    jni_str!("openOutputStream"),
                    jni_sig!("(Landroid/net/Uri;Ljava/lang/String;)Ljava/io/OutputStream;"),
                    &[JValue::Object(&uri_obj), JValue::Object(&mode)],
                )?
                .l()?;
            if stream.is_null() {
                return Ok(Err("the document could not be opened for writing".into()));
            }
            let mut written = 0usize;
            while written < bytes.len() {
                let end = (written + CHUNK).min(bytes.len());
                let buf = env.byte_array_from_slice(&bytes[written..end])?;
                env.call_method(
                    &stream,
                    jni_str!("write"),
                    jni_sig!("([BII)V"),
                    &[JValue::Object(&buf), JValue::Int(0), JValue::Int((end - written) as i32)],
                )?;
                written = end;
            }
            if let Err(e) = env.call_method(&stream, jni_str!("close"), jni_sig!("()V"), &[]) {
                return Ok(Err(e.to_string()));
            }
            Ok(Ok(()))
        })?
    }

    /// Move the picked documents' bytes after every engine write, once per frame: each
    /// export's temporary is copied over exactly once, each design mirror whenever the
    /// session has been clean since the last copy.
    pub(crate) fn deliver(app: &AndroidApp, targets: &Rc<Targets>, solve: &mut SolveApp) {
        let mut list = targets.0.borrow_mut();
        if list.is_empty() {
            return;
        }
        let path = solve.session.path.clone();
        let path = path.as_deref().map(Path::new);
        let dirty = solve.session.is_dirty();
        let revision = solve.session.revision;
        // Taken out so the attempts below can update `synced` on owned values; only the
        // linked documents go back in (an export is copied at most once, then forgotten).
        let pending = std::mem::take(&mut *list);
        let mut keep = Vec::with_capacity(pending.len());
        for target in pending {
            match target {
                Target::OneShot { uri, temp, name } => match std::fs::read(&temp) {
                    Ok(bytes) => match write_uri(app, &uri, &bytes) {
                        Ok(()) => {
                            let _ = std::fs::remove_file(&temp);
                            solve.set_status(format!("Exported {name}"), false);
                        }
                        Err(e) => {
                            log::error!("Android: export {name}: {e}");
                            let _ = std::fs::remove_file(&temp);
                            solve.set_status(format!("Could not write {name}: {e}"), true);
                        }
                    },
                    Err(e) if temp.exists() => {
                        log::error!("Android: export {name} could not be read: {e}");
                        let _ = std::fs::remove_file(&temp);
                        solve.set_status(format!("Could not write {name}: {e}"), true);
                    }
                    // Never written: the engine's own error is already on screen.
                    Err(_) => {}
                },
                Target::Linked { uri, mirror, mut synced } => {
                    let path_matches = path.is_some_and(|p| p == mirror.as_path());
                    if needs_sync(dirty, path_matches, synced, revision) {
                        // One attempt per revision: a failing document must not be retried
                        // every frame (its error is shown once).
                        synced = Some(revision);
                        let written = std::fs::read(&mirror).map_err(|e| e.to_string()).and_then(|bytes| write_uri(app, &uri, &bytes));
                        if let Err(e) = written {
                            log::error!("Android: mirror {}: {e}", mirror.display());
                            solve.set_status(format!("Could not update the picked file: {e}"), true);
                        }
                    }
                    keep.push(Target::Linked { uri, mirror, synced });
                }
            }
        }
        *list = keep;
    }
}

#[cfg(target_os = "android")]
pub(crate) use imp::{Picker, deliver};

#[cfg(test)]
mod tests {
    use super::*;

    fn workdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-saf-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sanitize_name_keeps_one_sane_segment() {
        assert_eq!(sanitize_name("  part.step ", "x", MAX_NAME), "part.step");
        assert_eq!(sanitize_name("../../evil.solidcraft", "x", MAX_NAME), "evil.solidcraft");
        assert_eq!(sanitize_name("", "fallback.solidcraft", MAX_NAME), "fallback.solidcraft");
        assert_eq!(sanitize_name("..", "fallback.solidcraft", MAX_NAME), "fallback.solidcraft");
        let long = "a".repeat(MAX_NAME + 40);
        assert_eq!(sanitize_name(&long, "x", MAX_NAME).chars().count(), MAX_NAME);
    }

    #[test]
    fn free_path_numbers_around_existing_files() {
        let dir = workdir("free");
        std::fs::write(dir.join("part.step"), b"old").unwrap();
        assert_eq!(free_path(&dir, "part.step", "part.step"), Some(dir.join("part (2).step")));
        std::fs::write(dir.join("part (2).step"), b"old").unwrap();
        assert_eq!(free_path(&dir, "part.step", "part.step"), Some(dir.join("part (3).step")));
        assert_eq!(free_path(&dir, "new.step", "new.step"), Some(dir.join("new.step")));
        // A name the dialog would never produce still lands somewhere usable (and never
        // escapes the directory: `../part.step` is the same file name as `part.step`).
        assert_eq!(free_path(&dir, "../part.step", "part.step"), Some(dir.join("part (3).step")));
        // Neither an empty name nor an empty fallback may resolve to the directory itself.
        assert_eq!(free_path(&dir, "", ""), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_mime_matches_common_types_and_stays_generic_otherwise() {
        assert_eq!(export_mime("png"), "image/png");
        assert_eq!(export_mime("PNG"), "image/png");
        assert_eq!(export_mime(".jpg"), "image/jpeg");
        assert_eq!(export_mime("json"), "application/json");
        assert_eq!(export_mime("dxf"), "image/vnd.dxf");
        assert_eq!(export_mime("step"), "application/octet-stream");
        assert_eq!(export_mime("solidcraft"), "application/octet-stream");
        assert_eq!(export_mime(""), "application/octet-stream");
    }

    #[test]
    fn principal_mime_always_satisfies_the_pickers_own_filter() {
        // A design save carries no extension: the principal type must still be one the
        // system picker's `*/*` filter accepts (an intent without a type throws).
        assert_eq!(principal_mime(""), ANY_TYPE);
        assert_eq!(principal_mime("solidcraft"), "application/octet-stream");
        assert_eq!(principal_mime("png"), "image/png");
        assert_eq!(principal_mime("json"), "application/json");
    }

    #[test]
    fn extra_mime_types_only_refines_a_principal_type() {
        // Open offers `*`: the principal already covers it, so no extra is sent.
        assert_eq!(extra_mime_types(ANY_TYPE, &["*".into()]), None);
        // A single-type export repeats its principal: no extra.
        assert_eq!(extra_mime_types("image/png", &["png".into()]), None);
        // Save As offers both design and JSON: JSON refines the generic principal.
        let exts = vec!["solidcraft".to_string(), "json".to_string()];
        assert_eq!(extra_mime_types("application/octet-stream", &exts), Some(vec!["application/json"]));
    }

    #[test]
    fn materialized_ext_recovers_the_engine_readable_extension() {
        // The provider reports a bare name (the Android case that broke Insert STEP): the
        // extension comes from the offered set so `insert_step`/`doc.open` can read it.
        let offered = vec!["step".to_string(), "stp".to_string()];
        assert_eq!(materialized_ext("1075", "content://x/1075", &offered).as_deref(), Some("step"));
        // A provider name with its own extension wins over the offered set.
        assert_eq!(materialized_ext("part.stp", "content://x/1", &offered).as_deref(), Some("stp"));
        // No name, but the uri ends in one: use it.
        assert_eq!(materialized_ext("", "content://x/part.step", &offered).as_deref(), Some("step"));
        // Nothing to go on (`*` is not an extension).
        assert_eq!(materialized_ext("1075", "content://x/1075", &["*".into()]), None);
    }

    #[test]
    fn with_ext_only_fills_a_missing_extension() {
        assert_eq!(with_ext("1075", Some("step")), "1075.step");
        assert_eq!(with_ext("part.stp", Some("step")), "part.stp");
        assert_eq!(with_ext("part.stp", None), "part.stp");
        assert_eq!(with_ext("../../evil", Some("step")), "evil.step");
    }

    #[test]
    fn display_from_uri_takes_the_last_segment() {
        // A documents provider keeps `primary:Download/widget.step` in one path segment.
        assert_eq!(
            display_from_uri("content://com.android.providers.documents/document/primary%3ADownload%2Fwidget.step"),
            Some("widget.step".into())
        );
        assert_eq!(display_from_uri("content://media/external/file/42"), Some("42".into()));
        assert_eq!(display_from_uri("content://x/..%2F..%2Fetc%2Fpasswd"), Some("passwd".into()));
        assert_eq!(display_from_uri("content://x/"), None);
        assert_eq!(display_from_uri(""), None);
    }

    #[test]
    fn needs_sync_only_for_clean_unmirrored_saves() {
        assert!(needs_sync(false, true, None, 7));
        assert!(needs_sync(false, true, Some(6), 7));
        assert!(!needs_sync(false, true, Some(7), 7));
        assert!(!needs_sync(true, true, None, 7));
        assert!(!needs_sync(false, false, None, 7));
    }

    #[test]
    fn upsert_replaces_the_target_of_the_same_document() {
        let targets = Targets::default();
        let uri = "content://documents/1".to_string();
        targets.upsert(Target::Linked { uri: uri.clone(), mirror: PathBuf::from("/a.solidcraft"), synced: Some(3) });
        targets.upsert(Target::OneShot { uri: "content://documents/2".into(), temp: PathBuf::from("/x.step"), name: "x.step".into() });
        targets.upsert(Target::Linked { uri: uri.clone(), mirror: PathBuf::from("/b.solidcraft"), synced: None });
        let list = targets.0.borrow();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|t| matches!(t, Target::Linked { mirror, synced: None, .. } if mirror == &PathBuf::from("/b.solidcraft"))));
        assert!(!list.iter().any(|t| matches!(t, Target::Linked { mirror, .. } if mirror == &PathBuf::from("/a.solidcraft"))));
        assert!(list.iter().any(|t| matches!(t, Target::OneShot { name, .. } if name == "x.step")));
    }
}
