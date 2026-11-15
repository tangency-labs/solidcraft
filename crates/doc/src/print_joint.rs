//! The measures of a foldable 3D print's joints: its film and clearance, a press fit's nub, the
//! filament and its channel, and where the core of a joint on a bend lies. Unfold for Printing
//! ([`crate::print_unfold_walls`]) makes its joints to these.

use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;

/// The film a bend turns on and the clearance between two walls that meet (mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrintFold {
    /// The film left under each bend.
    pub film: f64,
    /// How far two walls stay apart once folded.
    pub clearance: f64,
}

/// The default film under a bend (mm): two or three printed layers.
pub const DEFAULT_FILM: f64 = 0.4;

/// The default clearance of a foldable print (mm): walls that print side by side stay this far
/// apart, so they don't fuse (a PLA test print at 0.1 mm joined them a little).
pub const DEFAULT_PRINT_CLEARANCE: f64 = 0.15;

/// The narrowest a finger round a bend is made (mm): a test print's 3 mm fingers were frail.
pub const MIN_FINGER_WIDTH: f64 = 5.0;

/// A loose piece of a foldable print smaller than this is a crumb its cuts left, and is dropped (mm³).
pub const CRUMB: f64 = 5.0;

/// The default wall round the channel or the nub of a joint on a bend, inside its fingers (mm):
/// two lines of a 0.4 mm nozzle.
pub const DEFAULT_JOINT_WALL: f64 = 0.8;

/// The default filament diameter (mm).
pub const DEFAULT_FILAMENT: f64 = 1.75;

/// The default play of a filament channel (mm): test prints took the filament at no play, and in
/// places at 0.05 mm, only with a lot of force.
pub const DEFAULT_CHANNEL_TOLERANCE: f64 = 0.1;

/// A press fit's nub: a cap of a ball, `radius` round on the finger's end face and `height`
/// high. The dimple it snaps into is the same cap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PressFit {
    pub radius: f64,
    pub height: f64,
}

impl PressFit {
    /// The radius of the ball the nub is a cap of.
    pub fn ball(&self) -> f64 {
        (self.radius * self.radius + self.height * self.height) / (2.0 * self.height)
    }
}

/// Where a joint on a bend runs: its core `d` from the bend's edge (on the outer skin) along
/// the line halfway between the two faces, inside the body; its fingers a sector `r` round the
/// edge. A press fit's nub sits at `nub` on the first face's fingers (from the edge into that
/// face, negative, and up from its outer skin): on the core, or where the core is no place for
/// it, nearer that face's wall ([`fold_core`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoldCore {
    pub d: f64,
    pub r: f64,
    pub nub: Vec2,
}

/// The fold a joint on a bend takes (degrees).
pub const FOLD_JOINT_ANGLES: (f64, f64) = (30.0, 150.0);

/// The core of a joint on a bend folded by `angle` in a wall `t` thick: a channel (square,
/// `half` its side each way, lined up with the wall of the finger it runs in) or a nub (`round`,
/// `half` its radius), `wall` from the face's outer skin and, with the film and the clearance
/// besides, from the other face's. The fingers are a sector round the edge large enough for
/// the core with its wall and for the corner where the two walls overlap once folded.
pub fn fold_core(t: f64, angle: f64, pf: PrintFold, half: f64, round: bool, wall: f64) -> std::result::Result<FoldCore, String> {
    let angle = angle.abs();
    let (lo, hi) = FOLD_JOINT_ANGLES;
    if !(angle.is_finite() && angle >= lo.to_radians() - 1e-6 && angle <= hi.to_radians() + 1e-6) {
        return Err(format!("it needs a fold of {lo}° to {hi}°, not {:.1}°", angle.to_degrees()));
    }
    if !(wall.is_finite() && wall > 0.0 && wall <= 20.0) {
        return Err("the wall round its channel or nub must be more than 0 and at most 20 mm".into());
    }
    if !(half.is_finite() && half > 0.0 && half <= 20.0) {
        return Err("its channel or nub must be more than 0 and at most 40 mm across".into());
    }
    let (sin, cos) = (angle.sin(), angle.cos());
    let (s2, c2) = ((angle / 2.0).sin(), (angle / 2.0).cos());
    let m = pf.film + pf.clearance;
    // How far the core's outline reaches toward the other face's outer skin.
    let reach = if round { half } else { half * (sin + cos.abs()) };
    let d = ((reach + m + wall) / c2).max((half + wall) / c2);
    let far = if round { d + half } else { (d * s2 + half).hypot(d * c2 + half) };
    // The corner both walls fill once folded, and the line beside the other's film at the inner skin.
    let keep_top = (t * cos - m) / sin;
    let corner = (t / c2).max(t / sin).max(keep_top.hypot(t));
    let over = (t * 0.05).max(1e-3);
    let r = (far + wall).max(corner + 0.05).max(t + over + 0.05);
    if !(d.is_finite() && r.is_finite() && r <= 100.0) {
        return Err("its fingers would reach more than 100 mm round the bend".into());
    }
    // A nub stands out of its finger's end face into the next finger's stretch of the bend. In
    // the flat that finger is the other face's, mirrored across the edge: it lies where
    // `x·sin + z·cos` is at least `m`, and the nub must print clear of it. At a small fold it
    // covers the core; the nub then goes as low as its wall lets it once folded (the other
    // face's finger starts `m` above this one's outer skin), and as far toward this face's
    // wall as clears the other's finger in the flat.
    let core = Vec2::new(-d * s2, d * c2);
    let gap = pf.clearance.max(0.05);
    let clear_of_mirror = |p: Vec2| p.x * sin + p.y * cos <= m - half - gap + 1e-9;
    let nub = if !round || clear_of_mirror(core) {
        core
    } else {
        let z = m + half + wall;
        let low = Vec2::new((m - half - gap - z * cos) / sin, z);
        if !(low.x * sin - low.y * cos <= -m - half) {
            return Err(format!("at a fold of {:.1}° its fingers have no room for a nub clear of the next finger in the flat", angle.to_degrees()));
        }
        low
    };
    // The fingers reach round the nub with its wall too.
    let r = r.max(nub.len() + half + wall);
    if !(r <= 100.0) {
        return Err("its fingers would reach more than 100 mm round the bend".into());
    }
    Ok(FoldCore { d, r, nub })
}
