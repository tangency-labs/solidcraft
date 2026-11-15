//! Cached accessibility signal, observed only while a theme offers blur. No UI-thread IO.
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

#[derive(Default)]
struct Reading {
    needed: AtomicBool,
    started: AtomicBool,
    value: AtomicU8,
}

pub fn cached(ctx: &egui::Context) -> Box<dyn Fn(bool) -> Option<bool>> {
    let state = Arc::new(Reading::default());
    let ctx = ctx.clone();
    Box::new(move |needed| {
        state.needed.store(needed, Ordering::Relaxed);
        if needed && !state.started.swap(true, Ordering::Relaxed) {
            let weak = Arc::downgrade(&state);
            let ctx = ctx.clone();
            if let Err(error) = std::thread::Builder::new().name("desktop-transparency".into()).spawn(move || {
                while let Some(state) = weak.upgrade() {
                    if state.needed.load(Ordering::Relaxed) {
                        let code = match read() {
                            Some(true) => 1,
                            Some(false) => 2,
                            None => 0,
                        };
                        if state.value.swap(code, Ordering::Relaxed) != code {
                            ctx.request_repaint();
                        }
                    }
                    drop(state);
                    std::thread::sleep(std::time::Duration::from_secs(5));
                }
            }) {
                log::warn!("desktop transparency: could not observe accessibility setting: {error}");
            }
        }
        match state.value.load(Ordering::Relaxed) {
            1 => Some(true),
            2 => Some(false),
            _ => None,
        }
    })
}

#[cfg(target_os = "windows")]
fn read() -> Option<bool> {
    let key =
        winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize").ok()?;
    match key.get_value::<u32, _>("EnableTransparency").ok()? {
        0 => Some(true),
        1 => Some(false),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn read() -> Option<bool> {
    objc2::rc::autoreleasepool(|_| Some(objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency()))
}
