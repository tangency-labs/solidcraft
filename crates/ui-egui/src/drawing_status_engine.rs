//! Read-only bridge between engine drawing commands and the status controller.
//! It does not own drawing state and never executes update or repair commands.
use crate::drawing_status_controller::DrawingStatusController;
use serde_json::json;
use solidcraft_engine::Session;

/// Refresh a complete status snapshot. Invalidates previous responses first, so
/// an error cannot leave old information looking current.
pub fn refresh(session: &mut Session, controller: &mut DrawingStatusController) -> Result<(), String> {
    let generation = controller.invalidate();
    let status = session.execute("drawing.status", &json!({})).map_err(|error| error.to_string())?;
    if !controller.accept_status(generation, status) {
        return Err("Drawing status generation changed during refresh".to_owned());
    }
    let request = controller.pending_inspections();
    for dimension in request.inspect_ids {
        let result = session
            .execute("drawing.dimension.inspect", &json!({"dimension": dimension}))
            .map_err(|error| error.to_string());
        controller.accept_inspection(request.generation, &dimension, result);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_refresh_does_not_keep_a_previous_snapshot() {
        let mut session = Session::default();
        let mut controller = DrawingStatusController::default();
        // An empty session may reject drawing.status; either way a refresh
        // must not invent a dimension requiring attention.
        let _ = refresh(&mut session, &mut controller);
        if let Some(snapshot) = controller.snapshot() {
            assert!(snapshot.dimensions.is_empty());
        }
    }
}
