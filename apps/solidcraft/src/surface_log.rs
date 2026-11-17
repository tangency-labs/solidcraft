//! Opt-in display-wake diagnostics. A file works for Windows GUI builds without a console.

use std::io::Write;
use std::sync::Mutex;

struct SurfaceLog(Mutex<Option<std::fs::File>>);

static LOGGER: SurfaceLog = SurfaceLog(Mutex::new(None));

impl log::Log for SurfaceLog {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Debug && (metadata.target().starts_with("egui_wgpu") || metadata.target().starts_with("eframe::native"))
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata())
            && let Ok(mut file) = self.0.lock()
            && let Some(file) = file.as_mut()
        {
            let millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
            let _ = writeln!(file, "{millis} {} {}: {}", record.level(), record.target(), record.args());
            let _ = file.flush();
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.0.lock()
            && let Some(file) = file.as_mut()
        {
            let _ = file.flush();
        }
    }
}

pub fn install() {
    let Some(path) = std::env::var_os("SOLIDCRAFT_SURFACE_LOG") else { return };
    let file = match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("could not open surface log: {error}");
            return;
        }
    };
    if let Ok(mut destination) = LOGGER.0.lock() {
        *destination = Some(file);
    }
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
        log::debug!(target: "egui_wgpu", "Surface diagnostics started: SolidCraft {}", env!("CARGO_PKG_VERSION"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Log;

    #[test]
    fn captures_surface_and_lifecycle_debug_without_per_frame_trace() {
        for (target, level, enabled) in [
            ("egui_wgpu::winit", log::Level::Debug, true),
            ("eframe::native::wgpu_integration", log::Level::Debug, true),
            ("egui_wgpu::winit", log::Level::Trace, false),
            ("wgpu_core", log::Level::Debug, false),
        ] {
            assert_eq!(LOGGER.enabled(&log::Metadata::builder().target(target).level(level).build()), enabled);
        }
    }
}
