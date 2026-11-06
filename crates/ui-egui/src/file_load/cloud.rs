//! Cloud opening shares the local file worker, progress card, cancellation and adoption.
use super::*;
use solidcraft_cloud::jobs::Phase;

pub(super) fn target(app: &SolveApp, request: &Request) -> Result<Option<Session>, String> {
    let Kind::CloudOpen { operation, .. } = &request.kind else { return Ok(None) };
    app.session.cloud_open_target(operation).map(Some).map_err(|e| e.to_string())
}

pub(super) fn ready(session: &mut Session, params: &Value, cancelled: bool) -> Result<bool, String> {
    let operation = params.get("operation").and_then(Value::as_str).ok_or("missing cloud operation")?;
    if cancelled {
        let _ = session.execute("cloud.cancel", params);
        return Err("file operation cancelled".into());
    }
    let status = session
        .cloud_operations()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|s| s.operation == operation)
        .ok_or("opening operation no longer available")?;
    match status.phase {
        Phase::Ready => Ok(true),
        Phase::Failed | Phase::Cancelled => Err(status.message.unwrap_or_else(|| "opening cancelled".into())),
        _ => Ok(false),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn wait(session: &mut Session, command: &str, params: &Value, progress: &Progress) -> Result<(), String> {
    if command != "cloud.open.finish" {
        return Ok(());
    }
    while !ready(session, params, progress.is_cancelled())? {
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    Ok(())
}

pub(super) fn status(app: &SolveApp, request: &Request) -> Option<String> {
    let Kind::CloudOpen { operation, .. } = &request.kind else { return None };
    let status = app.session.cloud_operations().ok()?.into_iter().find(|status| &status.operation == operation)?;
    if status.phase == Phase::Ready {
        return None;
    }
    Some(match status.download_progress {
        Some((done, total)) => crate::i18n::format("cloud.downloading", &[("done", &done.to_string()), ("total", &total.to_string())]),
        None => crate::i18n::tr("cloud.checking_updates").into(),
    })
}

pub(super) fn preview(app: &SolveApp, request: &Request) -> Option<egui::TextureHandle> {
    let Kind::CloudOpen { destination, .. } = &request.kind else { return None };
    crate::cloud_browser::opening_preview(app, destination)
}
