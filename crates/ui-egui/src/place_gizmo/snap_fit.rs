//! Snap Fit: the marker moves the arm and snaps to sketch points (#3423 asks to put it where a
//! sketch says); the ring's grip points the catch, in the four directions the Catch Direction
//! combo offers for the face, so the ring and the combo always agree.

use solidcraft_engine::geom::Vec3;

use super::{Spec, Turn, angle_of, len, rect};
use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::dialogs_plastic::{Pl, hook_dirs};

pub(super) static SPEC: Spec = Spec {
    name: "snap_fit",
    serves: |app, d| super::on(app) && matches!(d.kind, Kind::Plastic(Pl::SnapFit { .. })),
    snap_commands: &["plastic.snap_fit"],
    move_tip: "Drag to move the snap fit across the model. It snaps to sketch points on the face.",
    many: false,
    places: |app, d| super::one(crate::dialogs_plastic::handle_frame(app, d)),
    move_to: |app, d, hits, _| crate::dialogs_plastic::handle_move(app, d, hits),
    click: None,
    footprint,
    freedom: super::anywhere,
    turn: Some(Turn { tip: "Drag to point the catch in another direction.", reach, angle, set }),
};

/// The catch's direction across the face, as the combo shows it.
fn catch(d: &Dialog, n: Vec3) -> Option<Vec3> {
    let Kind::Plastic(Pl::SnapFit { hook, .. }) = &d.kind else { return None };
    hook_dirs(n).get(*hook).map(|h| h.1)
}

/// The arm's foot: its thickness behind the place, towards the catch's opposite side, and its
/// width centred across (the evaluator's side profile).
fn footprint(app: &SolveApp, d: &Dialog, base: Vec3, n: Vec3) -> Vec<Vec3> {
    let (Kind::Plastic(Pl::SnapFit { thickness, width, .. }), Some(c)) = (&d.kind, catch(d, n)) else { return Vec::new() };
    let t = len(app, thickness);
    rect(base - c * (t * 0.5), n, c, t * 0.5, len(app, width) * 0.5)
}

/// The catch reaches its depth past the place.
fn reach(app: &SolveApp, d: &Dialog) -> Option<f64> {
    let Kind::Plastic(Pl::SnapFit { catch_depth, .. }) = &d.kind else { return None };
    Some(len(app, catch_depth))
}

fn angle(_: &SolveApp, d: &Dialog, n: Vec3) -> f64 {
    catch(d, n).map(|c| angle_of(c, n)).unwrap_or(0.0)
}

/// The catch direction nearest the pointer; free turning is not offered, the command keeps only
/// the combo's four directions.
fn set(_: &SolveApp, d: &mut Dialog, n: Vec3, raw: f64, _free: bool) -> f64 {
    let dirs = hook_dirs(n);
    let off = |a: f64| (a - raw + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
    let best = dirs.iter().map(|h| angle_of(h.1, n)).enumerate().min_by(|a, b| off(a.1).abs().total_cmp(&off(b.1).abs()));
    let Some((i, a)) = best else { return raw };
    if let Kind::Plastic(Pl::SnapFit { hook, .. }) = &mut d.kind {
        *hook = i;
    }
    a
}

#[cfg(test)]
#[path = "snap_fit_tests.rs"]
mod tests;
