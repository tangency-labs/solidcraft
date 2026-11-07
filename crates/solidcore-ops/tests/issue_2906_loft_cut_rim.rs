//! #2906: rounds and chamfers on the rim of a shallow loft cut, refused (1001, or an unported
//! trim cache) since the End's trace seeds (#2500). SolidCraft's operand
//! (`data/issue_2906_slab_less_loft.smb`, its failing blend's dump): a 900 x 480 x 160 slab less
//! a loft from a 556.68 x 99 rectangle on its top face down 4 mm to an 80 x 80 square. The loft's
//! sides are ruled patches whose rows fan out, so a first-order step across a side lands far
//! along the edge: the seed of a trace with no through point, in the plane through that guess,
//! was a section 100 mm or more from the edge's end, clamped to the face's domain, and the rail
//! ran the wrong way. The section at an edge's end is in the plane through that end.
//! Its own test binary: the fix set is the process kernel's.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::print_stdout,
    clippy::disallowed_methods
)] // test: the closed forms use trigonometry

mod common;

use common::{edge_at, volume};
use solidcore_ops::{blend, session, Blend, Body, CheckLevel, Corners};

/// The slab less the loft's frustum, by the prismoidal formula.
fn slab_less_loft() -> f64 {
    let (top, bottom) = (556.68 * 99.0, 80.0 * 80.0);
    let middle = (556.68 + 80.0) / 2.0 * (99.0 + 80.0) / 2.0;
    900.0 * 480.0 * 160.0 - 4.0 / 6.0 * (top + 4.0 * middle + bottom)
}

/// SolidCraft's set: every fix but two.
fn fixes() {
    let off = [
        "fillet_refuse_rounds_breaking_through",
        "fillet_round_ends_on_outline",
    ];
    let on = solidcore::kernel::UpstreamFixes::NAMES
        .iter()
        .copied()
        .filter(|n| !off.contains(n));
    session::set_fixes(on).unwrap();
}

fn operand() -> Body {
    Body::from_smb(
        include_bytes!("data/issue_2906_slab_less_loft.smb").to_vec(),
        false,
    )
    .unwrap()
}

/// `kind` size 8 on the edge through `p`, built and valid; its volume.
fn blended(kind: Blend, p: [f64; 3]) -> f64 {
    let b = operand();
    let e = edge_at(&b, p);
    blend(&b, kind, &[e], &[8.0], &[], Corners::Filled).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid(), "{p:?}");
    volume(&b)
}

/// The operand's volume to 1e-8, so the closed forms are held to 2e-10 of the body (0.014 mm3).
const TOL: f64 = 2e-10;

/// The long sides meet the floor at atan(4 / 9.5) along 80 mm. The edge runs on within the
/// tangency tolerance into the rim between two sides, so each blend ends square to its edge at
/// the smooth vertex, and adds a prism: the section between the blend and the corner.
#[test]
fn the_floor_edges_of_the_long_sides_take_their_closed_forms() {
    fixes();
    let v = slab_less_loft();
    let phi = (4.0_f64 / 9.5).atan();
    let round = 80.0 * 64.0 * ((phi / 2.0).tan() - phi / 2.0);
    let chamfer = 80.0 * 32.0 * phi.sin();
    for y in [418.84, 338.84] {
        let p = [450.0, y, 156.0];
        let r = blended(Blend::Round, p);
        assert!((r - (v + round)).abs() <= TOL * v, "{r}");
        let c = blended(Blend::Chamfer, p);
        assert!((c - (v + chamfer)).abs() <= TOL * v, "{c}");
    }
}

/// The rest of the rim's picks build, each more material than the operand: the floor's short
/// edges, and the rounds of the rim between two sides, which the same seeds stopped.
#[test]
fn the_rest_of_the_rim_builds() {
    fixes();
    let v = slab_less_loft();
    for (p, kinds) in [
        ([410.0, 378.84, 156.0], &[Blend::Round, Blend::Chamfer][..]),
        ([490.0, 378.84, 156.0], &[Blend::Round, Blend::Chamfer][..]),
        ([290.83, 423.59, 158.0], &[Blend::Round][..]),
        ([290.83, 334.09, 158.0], &[Blend::Round][..]),
        ([609.17, 423.59, 158.0], &[Blend::Round][..]),
        ([609.17, 334.09, 158.0], &[Blend::Round][..]),
    ] {
        for &kind in kinds {
            let made = blended(kind, p);
            println!("RIM {p:?} {kind:?}: {made:.6}");
            assert!(made > v, "{p:?}");
        }
    }
}
