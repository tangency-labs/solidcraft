//! Boss: the marker moves the boss across the model and snaps to sketch points, so a screw boss
//! lands on the centre of a circle drawn for it. A boss is round: no ring.

use solidcraft_engine::geom::Vec3;

use super::{PlaceSource, Spec, circle, len};
use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::dialogs_plastic::Pl;

pub(super) static SPEC: Spec = Spec {
    name: "boss",
    serves: |app, d| super::on(app) && matches!(d.kind, Kind::Plastic(Pl::Boss { .. })),
    snap_commands: &["plastic.boss"],
    move_tip: "Drag to move the boss across the model. It snaps to sketch points on the face.",
    many: false,
    place: PlaceSource::Input(0),
    click: None,
    footprint,
    freedom: super::anywhere,
    turn: None,
};

/// The boss's outer circle.
fn footprint(app: &SolveApp, d: &Dialog, base: Vec3, n: Vec3) -> Vec<Vec3> {
    let Kind::Plastic(Pl::Boss { diameter, .. }) = &d.kind else { return Vec::new() };
    circle(base, n, len(app, diameter) * 0.5)
}

#[cfg(test)]
#[path = "boss_tests.rs"]
mod tests;
