//! Read-only M1-A projection of drawing.status and drawing.dimension.inspect.
//! No drawing command, update, repair or persistence mutation is performed here.
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewItem {
    pub id: String,
    pub sheet: String,
    pub status: String,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DimensionItem {
    pub id: String,
    pub view: Option<String>,
    pub sheet: Option<String>,
    pub status: String,
    pub why: Option<String>,
    pub out_of_date: bool,
    pub inspect_error: Option<String>,
}

impl DimensionItem {
    pub fn needs_attention(&self) -> bool {
        self.out_of_date || !matches!(self.status.as_str(), "ok" | "overridden") || self.inspect_error.is_some()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrawingAttention {
    pub views: Vec<ViewItem>,
    pub dimensions: Vec<DimensionItem>,
    pub waiting_views: usize,
    pub held: bool,
    pub warnings: Vec<String>,
}

impl DrawingAttention {
    /// The count is by unique dimension ID, not by individual flags.
    pub fn dimensions_needing_attention(&self) -> usize {
        self.dimensions.iter().filter(|d| d.needs_attention()).count()
    }
    /// An ID is returned only when inspect is needed to map it to a view/sheet.
    pub fn dimensions_to_inspect(&self) -> Vec<&str> {
        self.dimensions.iter().filter(|d| d.view.is_none()).map(|d| d.id.as_str()).collect()
    }
    /// Attach a previously fetched inspect result. Unknown/missing views remain unknown.
    pub fn with_inspections(mut self, inspections: &BTreeMap<String, Result<Value, String>>) -> Self {
        let sheets: BTreeMap<_, _> = self.views.iter().map(|v| (v.id.as_str(), v.sheet.as_str())).collect();
        for dim in &mut self.dimensions {
            match inspections.get(&dim.id) {
                Some(Ok(value)) => {
                    let view = value.get("view").and_then(Value::as_str).map(str::to_owned);
                    dim.sheet = view.as_deref().and_then(|id| sheets.get(id).copied()).map(str::to_owned);
                    dim.view = view;
                    dim.inspect_error = None;
                }
                Some(Err(error)) => dim.inspect_error = Some(error.clone()),
                None => {}
            }
        }
        self
    }
}

/// Projects one engine response; never guesses a dimension's sheet from its order.
/// Malformed entries are ignored rather than assigned fabricated identifiers.
pub fn from_status(status: &Value) -> DrawingAttention {
    let views = status
        .get("views")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| {
            Some(ViewItem {
                id: v.get("view")?.as_str()?.to_owned(),
                sheet: v.get("sheet")?.as_str()?.to_owned(),
                status: v.get("status")?.as_str()?.to_owned(),
                reason: v.get("reason").and_then(Value::as_str).map(str::to_owned),
            })
        })
        .collect();
    let mut seen = BTreeSet::new();
    let dimensions = status
        .get("dimensions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|d| {
            let id = d.get("dimension")?.as_str()?.to_owned();
            if !seen.insert(id.clone()) {
                return None;
            }
            Some(DimensionItem {
                id,
                view: None,
                sheet: None,
                status: d.get("status")?.as_str()?.to_owned(),
                why: d.get("why").and_then(Value::as_str).map(str::to_owned),
                out_of_date: d.get("out_of_date").and_then(Value::as_bool).unwrap_or(false),
                inspect_error: None,
            })
        })
        .collect();
    DrawingAttention {
        views,
        dimensions,
        waiting_views: status.get("waiting").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).unwrap_or(0),
        held: status.get("held").and_then(Value::as_bool).unwrap_or(false),
        warnings: status.get("param_warnings").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lost_and_stale_count_once_and_map_by_inspected_view() {
        let status = json!({"views":[{"view":"v1","sheet":"s1","status":"current"},{"view":"v2","sheet":"s2","status":"current"}],"dimensions":[{"dimension":"d1","status":"lost","why":"missing edge","out_of_date":true},{"dimension":"d1","status":"lost","out_of_date":true},{"dimension":"d2","status":"ok"}],"waiting":0});
        let a = from_status(&status);
        assert_eq!(a.dimensions_needing_attention(), 1);
        assert_eq!(a.dimensions.len(), 2);
        let map = BTreeMap::from([("d1".into(), Ok(json!({"view":"v2"}))), ("d2".into(), Ok(json!({"view":"v1"})))]);
        let a = a.with_inspections(&map);
        assert_eq!(a.dimensions[0].sheet.as_deref(), Some("s2"));
        assert_eq!(a.dimensions[1].sheet.as_deref(), Some("s1"));
    }

    #[test]
    fn held_without_waiting_and_drawn_reason_are_preserved() {
        let a = from_status(
            &json!({"views":[{"view":"v","sheet":"s","status":"current","reason":"study no longer exists"}],"dimensions":[{"dimension":"d","status":"ok","out_of_date":true}],"waiting":0,"held":true}),
        );
        assert!(a.held);
        assert_eq!(a.waiting_views, 0);
        assert_eq!(a.views[0].reason.as_deref(), Some("study no longer exists"));
        assert_eq!(a.dimensions_needing_attention(), 1);
    }

    #[test]
    fn inspect_failure_and_unknown_view_never_guess_sheet() {
        let a = from_status(&json!({"views":[{"view":"v","sheet":"s","status":"current"}],"dimensions":[{"dimension":"d","status":"ambiguous"}]}));
        let a = a.with_inspections(&BTreeMap::from([("d".into(), Err("inspect unavailable".into()))]));
        assert_eq!(a.dimensions[0].sheet, None);
        assert!(a.dimensions[0].needs_attention());
        let a = a.with_inspections(&BTreeMap::from([("d".into(), Ok(json!({"view":"gone"})))]));
        assert_eq!(a.dimensions[0].view.as_deref(), Some("gone"));
        assert_eq!(a.dimensions[0].sheet, None);
    }
}

#[cfg(test)]
mod w23a_engine_integration_tests {
    use super::*;
    use serde_json::json;
    use solidcore_record::Vec3;
    use solidcraft_engine::Session;
    use solidcraft_viewport::vec::Vec3Ops;

    fn command(session: &mut Session, name: &str, params: Value) -> Value {
        session.execute(name, &params).unwrap_or_else(|error| panic!("{name}: {error}"))
    }

    /// Exercise the real W23a model, engine status and the actual UI projection.
    #[test]
    fn w23a_deleted_bore_is_reported_by_real_engine_and_ui_adapter() {
        let script: Value = serde_json::from_str(include_str!("../../../examples/acceptance/drawing_m2.json")).unwrap();
        let mut session = Session::default();
        let replies = session.run_script(&script).expect("W23a script must execute");
        let view = replies.last().expect("W23a creates a view")["view"].clone();
        let centre = Vec3::from([20.0, 20.0, 20.0]);
        let mesh = session.model.state().body("Plate").expect("Plate body").mesh();
        let index = mesh.edges.iter().position(|edge| {
            !edge.is_empty() && edge.iter().all(|point| (point.dist(centre) - 5.0).abs() < 1e-6)
        }).expect("Bore1 circular edge");
        let dimension = command(&mut session, "drawing.dimension.add", json!({
            "view": view,
            "kind": {"radial": {"edge": {"body": "Plate", "index": index}}},
            "text": {"at": [12.0, 24.0]}
        }))["dimension"].clone();
        assert_eq!(command(&mut session, "drawing.dimension.inspect", json!({"dimension": dimension}))["status"], "ok");

        let drawing = command(&mut session, "drawing.close", json!({}))["closed"].clone();
        assert!(drawing.is_string(), "drawing.close must return encoded drawing ID");
        command(&mut session, "timeline.delete", json!({"feature": "Bore1"}));
        command(&mut session, "drawing.open", json!({"drawing": drawing}));
        let engine_status = command(&mut session, "drawing.status", json!({}));
        let attention = from_status(&engine_status);
        let id = dimension.as_str().expect("dimension ID");
        let lost = attention.dimensions.iter().find(|item| item.id == id)
            .unwrap_or_else(|| panic!("W23a dimension absent from drawing.status: {engine_status}"));
        assert_eq!(lost.status, "lost", "{engine_status}");
        assert!(lost.needs_attention());
        assert!(attention.dimensions_needing_attention() >= 1);
        let inspected = command(&mut session, "drawing.dimension.inspect", json!({"dimension": dimension}));
        assert_eq!(inspected["status"], "lost");
        let enriched = attention.with_inspections(&BTreeMap::from([(id.to_owned(), Ok(inspected))]));
        assert!(enriched.dimensions.iter().find(|item| item.id == id).unwrap().view.is_some());
    }
}
