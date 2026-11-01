//! Rest: the marker moves the pad, the ring turns a rectangular one in 5° steps, pulled onto the
//! face's compass points when near one (Alt or Ctrl held: free), kept as the command's `along`.

use solidcraft_engine::geom::Vec3;

use super::{PlaceSource, Spec, Turn, angle_of, circle, dir_at, len, pt, pulled, rect, vec3_of};
use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::dialogs_plastic::Pl;

pub(super) static SPEC: Spec = Spec {
    name: "rest",
    serves: |app, d| super::on(app) && matches!(d.kind, Kind::Plastic(Pl::Rest { .. })),
    snap_commands: &["plastic.rest"],
    move_tip: "Drag to move the rest across the model. It snaps to sketch points on the face.",
    many: false,
    place: PlaceSource::Input(0),
    click: None,
    footprint,
    freedom: super::anywhere,
    turn: Some(Turn { tip: "Drag to turn the rest in 5° steps. Hold Alt or Ctrl to turn it freely.", reach, angle, set }),
};

/// Rotation step without a modifier (degrees).
const STEP_DEG: f64 = 5.0;

/// The pad's footprint: a circle of the diameter, or the rectangle of the length along `along`
/// and the width across it.
fn footprint(app: &SolveApp, d: &Dialog, base: Vec3, n: Vec3) -> Vec<Vec3> {
    let Kind::Plastic(Pl::Rest { round, width, length, .. }) = &d.kind else { return Vec::new() };
    if *round {
        circle(base, n, len(app, width) * 0.5)
    } else {
        rect(base, n, dir_at(n, angle(app, d, n)), len(app, length) * 0.5, len(app, width) * 0.5)
    }
}

/// Half the length of a rectangular pad; a round one has no ring.
fn reach(app: &SolveApp, d: &Dialog) -> Option<f64> {
    let Kind::Plastic(Pl::Rest { round: false, length, .. }) = &d.kind else { return None };
    Some(len(app, length) * 0.5)
}

/// The stored `along` as an angle about `n` from the engine's default direction.
fn angle(_: &SolveApp, d: &Dialog, n: Vec3) -> f64 {
    d.extra.get("along").and_then(vec3_of).map(|a| angle_of(a, n)).unwrap_or(0.0)
}

fn set(_: &SolveApp, d: &mut Dialog, n: Vec3, raw: f64, free: bool) -> f64 {
    let a = pulled(raw.to_degrees(), n, STEP_DEG, free).to_radians();
    d.extra.insert("along".into(), pt(dir_at(n, a)));
    a
}

#[cfg(test)]
#[path = "rest_tests.rs"]
mod tests;
