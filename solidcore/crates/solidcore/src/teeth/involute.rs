//! Involute splines: the teeth of the shaft and the tooth spaces of its hub.
//!
//! An external tooth ([`External`]) is generated: it is what a straight-sided rack with a rounded
//! tip leaves when it rolls on the pitch circle. Its flank is the involute, its root fillet is the
//! trochoid the tip round cuts, and where the rack reaches too deep for the count of teeth the
//! fillet cuts into the flank (undercut). Nothing of that is approximated: each point of the
//! outline is the point of the rack that touches the tooth at one instant of the rolling, found
//! from the rule that the common normal passes through the point of rolling contact.
//!
//! An internal tooth space ([`Internal`]) is formed: the same involute flanks, with circular rounds
//! at the root corners, as a broach leaves them.
//!
//! Lengths are millimetres. Heights of the tooth are given in modules, as usual for involute
//! splines.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::teeth::mesh::Material;
use crate::teeth::outline::{checked_tolerance, on_circle, sampled, Outline, Piece, Point, Role};
use crate::teeth::pair::{Motion, Pair, Part};
use crate::teeth::{in_range, zero_up_to, Error, MAX_LENGTH, MAX_TEETH};

/// The involute function `tan a - a` of an angle in radians.
pub fn inv(angle: f64) -> f64 {
    angle.tan() - angle
}

/// What module, count of teeth and pressure angle fix.
#[derive(Clone, Copy, Debug)]
struct Base {
    m: f64,
    z: f64,
    sin: f64,
    cos: f64,
    /// Radius of the pitch circle.
    r: f64,
    /// Radius of the base circle.
    rb: f64,
    /// The pitch along the pitch circle.
    pitch: f64,
    /// The involute function of the pressure angle.
    inv: f64,
}

fn base(module: f64, teeth: u32, pressure_angle: f64) -> Result<Base, Error> {
    let m = in_range("module", module, 0.0, 1000.0)?;
    if !(3..=MAX_TEETH).contains(&teeth) {
        return Err(Error::OutOfRange {
            what: "teeth",
            value: f64::from(teeth),
            above: 2.0,
            up_to: f64::from(MAX_TEETH),
        });
    }
    let alpha = in_range("pressure angle", pressure_angle, 1.0, 50.0)?.to_radians();
    let (sin, cos) = alpha.sin_cos();
    let z = f64::from(teeth);
    let r = m * z / 2.0;
    Ok(Base {
        m,
        z,
        sin,
        cos,
        r,
        rb: r * cos,
        pitch: PI * m,
        inv: inv(alpha),
    })
}

impl Base {
    /// How far an involute has turned from its start on the base circle when it reaches `radius`.
    fn turned(&self, radius: f64) -> Option<f64> {
        if radius.is_finite() && radius >= self.rb {
            let roll = (radius * radius - self.rb * self.rb).sqrt() / self.rb;
            Some(roll - roll.atan())
        } else {
            None
        }
    }
}

/// The teeth of a splined shaft, as a rack-shaped cutting tool cuts them.
///
/// The rack is the usual straight-sided one: its flanks lean by the pressure angle, its teeth are
/// as thick as its spaces on its reference line, and the corners of its teeth are rounded with
/// `tool_tip_radius`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct External {
    /// The module in millimetres: the pitch diameter divided by the count of teeth.
    pub module: f64,
    /// How many teeth, 3 to [`MAX_TEETH`].
    pub teeth: u32,
    /// The pressure angle in degrees, above 1 and at most 50.
    pub pressure_angle: f64,
    /// How far the tooth reaches above the pitch circle when there is no shift, in modules.
    pub addendum: f64,
    /// How far the root lies below the pitch circle when there is no shift, in modules.
    pub dedendum: f64,
    /// The profile shift in modules: the rack is drawn back from the axis by this much, which
    /// thickens the tooth and moves tip and root outward.
    pub shift: f64,
    /// The radius of the round at the corners of the rack's teeth, in modules. It makes the root
    /// fillet.
    pub tool_tip_radius: f64,
    /// Millimetres taken from the tooth's thickness on the pitch circle, for play in the hub
    /// (negative thickens).
    pub thinning: f64,
}

/// An external tooth worked out: the rack that cuts it, and where its pieces begin and end.
#[derive(Clone, Copy, Debug)]
struct Cut {
    base: Base,
    /// Thickness of the tooth on the pitch circle.
    s: f64,
    /// Radius of the rack's tip round, and its centre in the rack: along and across the line of
    /// rolling.
    rho: f64,
    uc: f64,
    vc: f64,
    ra: f64,
    rf: f64,
    /// The angle of rolling at which the fillet reaches the root circle.
    phi_root: f64,
    /// The angle of rolling at which the fillet meets the flank, and the height in the rack of that
    /// point.
    phi_join: f64,
    v_join: f64,
    /// The height in the rack of the flank's point on the tip circle.
    v_tip: f64,
    undercut: bool,
    /// Half the angle the top of the tooth spans.
    tip_half_angle: f64,
}

const NO_JOIN: Error = Error::Invalid {
    why: "the root fillet and the flank would not meet",
};

impl Cut {
    /// The point of the upper flank cut by the point of the rack's flank at height `v` above the
    /// line of rolling.
    ///
    /// That point of the rack touches the tooth when its normal passes through the point of rolling
    /// contact, which is at the angle `phi` below.
    fn flank(&self, v: f64) -> Point {
        let b = &self.base;
        let phi = (self.s / 2.0 - v / (b.sin * b.cos)) / b.r;
        let (sn, cs) = phi.sin_cos();
        let along = v * b.cos / b.sin;
        [(b.r + v) * cs - along * sn, (b.r + v) * sn + along * cs]
    }

    /// The point of the upper root fillet cut by the rack's tip round at the angle of rolling
    /// `phi`: the point of the round on the line from the point of rolling contact through the
    /// round's centre, on the far side.
    fn fillet(&self, phi: f64) -> Point {
        let b = &self.base;
        let (du, dv) = (b.r * phi - self.uc, -self.vc);
        let len = du.hypot(dv);
        let (qu, qv) = (self.uc - self.rho * du / len, self.vc - self.rho * dv / len);
        let (sn, cs) = phi.sin_cos();
        let along = qu - b.r * phi;
        [(b.r + qv) * cs - along * sn, (b.r + qv) * sn + along * cs]
    }

    /// Half the angle the tooth spans at `radius`, were its flanks involutes all the way down to
    /// the base circle.
    fn half_angle(&self, radius: f64) -> Option<f64> {
        Some(self.s / (2.0 * self.base.r) + self.base.inv - self.base.turned(radius)?)
    }

    /// The height in the rack of the flank's point at `radius` (at or above the base circle).
    fn flank_height(&self, radius: f64) -> f64 {
        let b = &self.base;
        -b.r * b.sin * b.sin + b.sin * (radius * radius - b.rb * b.rb).max(0.0).sqrt()
    }

    /// Where an undercutting fillet crosses the flank: the angle of rolling of the fillet's point
    /// and the height of the flank's.
    ///
    /// From the root upward the fillet is the tooth's boundary for as long as it runs inside the
    /// involute. It starts below the base circle, where there is no involute at all.
    fn crossing(&self, phi_tangent: f64) -> Result<(f64, f64), Error> {
        const STEPS: u32 = 4096;
        let outside = |phi: f64| {
            let [x, y] = self.fillet(phi);
            self.half_angle(x.hypot(y))
                .is_some_and(|half| y.atan2(x) >= half)
        };
        let at = |i: u32| {
            self.phi_root + (phi_tangent - self.phi_root) * f64::from(i) / f64::from(STEPS)
        };
        if outside(at(0)) {
            return Err(NO_JOIN);
        }
        let first = (1..=STEPS).find(|&i| outside(at(i))).ok_or(NO_JOIN)?;
        let (mut inside_at, mut outside_at) = (at(first - 1), at(first));
        for _ in 0..200 {
            let mid = 0.5 * (inside_at + outside_at);
            if outside(mid) {
                outside_at = mid;
            } else {
                inside_at = mid;
            }
        }
        let [x, y] = self.fillet(outside_at);
        let v = self.flank_height(x.hypot(y));
        let [fx, fy] = self.flank(v);
        if (fx - x).hypot(fy - y) > 1e-9 * (1.0 + self.base.r) {
            return Err(NO_JOIN);
        }
        Ok((outside_at, v))
    }

    /// The smallest and the largest angle from the middle of the tooth of the points of its upper
    /// root fillet.
    ///
    /// The fillet runs from the root circle up to the flank. Where the rack undercuts the tooth
    /// deeply, the fillet swings back past the middle of the tooth, and then the two fillets of one
    /// tooth cut through each other.
    fn fillet_angles(&self) -> (f64, f64) {
        const STEPS: u32 = 256;
        let angle = |phi: f64| {
            let [x, y] = self.fillet(phi);
            y.atan2(x)
        };
        let at = |i: u32| {
            self.phi_root + (self.phi_join - self.phi_root) * f64::from(i) / f64::from(STEPS)
        };
        // The extreme among the steps, then closer between its neighbours (the curve is smooth
        // there).
        let extreme = |sign: f64| {
            let (mut best, mut best_i) = (f64::INFINITY, 0);
            for i in 0..=STEPS {
                let value = sign * angle(at(i));
                if value < best {
                    (best, best_i) = (value, i);
                }
            }
            let (mut lo, mut hi) = (at(best_i.saturating_sub(1)), at((best_i + 1).min(STEPS)));
            for _ in 0..100 {
                let (a, b) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
                if sign * angle(a) < sign * angle(b) {
                    hi = b;
                } else {
                    lo = a;
                }
            }
            sign * best.min(sign * angle(0.5 * (lo + hi)))
        };
        (extreme(1.0), extreme(-1.0))
    }
}

impl External {
    fn cut(&self) -> Result<Cut, Error> {
        let b = base(self.module, self.teeth, self.pressure_angle)?;
        let addendum = in_range("addendum", self.addendum, 0.0, 5.0)?;
        let dedendum = in_range("dedendum", self.dedendum, 0.0, 5.0)?;
        let shift = in_range("profile shift", self.shift, -5.0, 5.0)?;
        let rho = zero_up_to("tool tip radius", self.tool_tip_radius, 5.0)? * b.m;
        let thinning = in_range("thinning", self.thinning, -MAX_LENGTH, MAX_LENGTH)?;
        let tan = b.sin / b.cos;

        let s = b.pitch / 2.0 + 2.0 * shift * b.m * tan - thinning;
        if !(s > 0.0 && s < b.pitch) {
            return Err(Error::Invalid {
                why: "the tooth would be thinner than nothing or as wide as the pitch",
            });
        }
        let depth = (dedendum - shift) * b.m; // of the rack's tip below the line of rolling
        let vc = rho - depth;
        if !(vc < 0.0) {
            return Err(Error::Invalid {
                why: "the root must lie below the pitch circle by more than the tool's tip radius",
            });
        }
        let rf = b.r - depth;
        if !(rf > 1e-9 * b.m) {
            return Err(Error::Invalid {
                why: "the root circle would be inside the axis",
            });
        }
        let uc = s / 2.0 + (rho - vc * b.sin) / b.cos;
        if uc > b.pitch / 2.0 * (1.0 + 1e-12) {
            return Err(Error::Invalid {
                why: "the tool's tip radius is too large: its two rounds would overlap",
            });
        }
        let ra = b.r + (addendum + shift) * b.m;

        let mut cut = Cut {
            base: b,
            s,
            rho,
            uc,
            vc,
            ra,
            rf,
            phi_root: uc / b.r,
            phi_join: 0.0,
            v_join: 0.0,
            v_tip: 0.0,
            undercut: false,
            tip_half_angle: 0.0,
        };
        // Where the rack's flank ends and its tip round begins: the lowest point of the flank that
        // cuts.
        let v_tangent = vc - rho * b.sin;
        let phi_tangent = (uc - vc * b.cos / b.sin) / b.r;
        // The flank's point at this height in the rack cuts the tooth on the base circle; lower
        // ones cut it away.
        let v_base = -b.r * b.sin * b.sin;
        cut.undercut = v_tangent < v_base - 1e-12 * b.r;
        (cut.phi_join, cut.v_join) = if cut.undercut {
            cut.crossing(phi_tangent)?
        } else {
            (phi_tangent, v_tangent)
        };
        let (lowest, highest) = cut.fillet_angles();
        if !(lowest > 0.0) {
            return Err(Error::Invalid { why: "the undercut would cut the tooth through at its root: its two root fillets would meet" });
        }
        if !(highest < PI / b.z) {
            return Err(Error::Invalid {
                why: "the root fillets of neighbouring teeth would cross",
            });
        }

        let [jx, jy] = cut.flank(cut.v_join);
        if !(ra > jx.hypot(jy) * (1.0 + 1e-12) && ra > b.rb) {
            return Err(Error::Invalid {
                why: "the tip circle is at or below the start of the flank: no flank is left",
            });
        }
        cut.v_tip = cut.flank_height(ra);
        let [tx, ty] = cut.flank(cut.v_tip);
        cut.tip_half_angle = ty.atan2(tx);
        if !(cut.tip_half_angle > 0.0) {
            return Err(Error::Invalid {
                why: "the teeth would come to a point below the tip circle",
            });
        }
        Ok(cut)
    }

    /// The radius of the pitch circle: half of module times teeth.
    pub fn pitch_radius(&self) -> Result<f64, Error> {
        Ok(self.cut()?.base.r)
    }

    /// The radius of the base circle, from which the flanks unwind.
    pub fn base_radius(&self) -> Result<f64, Error> {
        Ok(self.cut()?.base.rb)
    }

    /// The radius of the tip circle.
    pub fn tip_radius(&self) -> Result<f64, Error> {
        Ok(self.cut()?.ra)
    }

    /// The radius of the root circle.
    pub fn root_radius(&self) -> Result<f64, Error> {
        Ok(self.cut()?.rf)
    }

    /// The thickness of the tooth as an arc on the pitch circle.
    pub fn tooth_thickness(&self) -> Result<f64, Error> {
        Ok(self.cut()?.s)
    }

    /// The radius at which the involute flank starts: below it the tooth is root fillet. The hub
    /// must not reach below this radius.
    pub fn form_radius(&self) -> Result<f64, Error> {
        let cut = self.cut()?;
        let [x, y] = cut.flank(cut.v_join);
        Ok(x.hypot(y))
    }

    /// Whether the rack cuts into the flank near the base circle, as it does when the teeth are
    /// few.
    pub fn is_undercut(&self) -> Result<bool, Error> {
        Ok(self.cut()?.undercut)
    }

    /// Half the angle (radians) the tooth spans at `radius`, for a radius on the flank.
    pub fn half_angle_at(&self, radius: f64) -> Result<f64, Error> {
        let cut = self.cut()?;
        let [x, y] = cut.flank(cut.v_join);
        let r = in_range(
            "radius",
            radius,
            x.hypot(y) * (1.0 - 1e-12),
            cut.ra * (1.0 + 1e-12),
        )?;
        cut.half_angle(r.max(cut.base.rb)).ok_or(NO_JOIN)
    }

    /// The outline of the part, a tooth centred on the x axis. Flanks and fillets are given as
    /// points whose chords stay within `tolerance` (millimetres) of the true curves.
    ///
    /// One tooth is six pieces, counter-clockwise: fillet, flank, tip arc, flank, fillet, root arc.
    pub fn outline(&self, tolerance: f64) -> Result<Outline, Error> {
        let tolerance = checked_tolerance(tolerance)?;
        let cut = self.cut()?;
        // The upper side, each from the lower end upward.
        let fillet = sampled(
            &|phi| cut.fillet(phi),
            cut.phi_root,
            cut.phi_join,
            tolerance,
        )?;
        let flank = sampled(&|v| cut.flank(v), cut.v_join, cut.v_tip, tolerance)?;
        let mirrored =
            |points: &[Point]| points.iter().map(|&[x, y]| [x, -y]).collect::<Vec<Point>>();
        let reversed = |points: &[Point]| points.iter().rev().copied().collect::<Vec<Point>>();

        let mut tooth = vec![
            Piece::Curve {
                points: mirrored(&fillet),
                role: Role::Fillet,
            },
            Piece::Curve {
                points: mirrored(&flank),
                role: Role::Flank,
            },
            Piece::Arc {
                centre: [0.0, 0.0],
                radius: cut.ra,
                start: -cut.tip_half_angle,
                sweep: 2.0 * cut.tip_half_angle,
                role: Role::Tip,
            },
            Piece::Curve {
                points: reversed(&flank),
                role: Role::Flank,
            },
            Piece::Curve {
                points: reversed(&fillet),
                role: Role::Fillet,
            },
        ];
        let root_sweep = TAU / cut.base.z - 2.0 * cut.phi_root;
        if root_sweep > 1e-14 {
            tooth.push(Piece::Arc {
                centre: [0.0, 0.0],
                radius: cut.rf,
                start: cut.phi_root,
                sweep: root_sweep,
                role: Role::Root,
            });
        }
        Outline::new(self.teeth, tooth, tolerance)
    }
}

/// The opening of the hub that goes on a splined shaft.
///
/// The spaces between its teeth have involute flanks, like the teeth of the shaft, and circular
/// rounds where the flanks meet the root circle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Internal {
    /// The module in millimetres.
    pub module: f64,
    /// How many teeth, 3 to [`MAX_TEETH`].
    pub teeth: u32,
    /// The pressure angle in degrees, above 1 and at most 50.
    pub pressure_angle: f64,
    /// The width of the space between two teeth, as an arc on the pitch circle, in millimetres.
    pub space_width: f64,
    /// The radius of the tips of the teeth (they point inward), in millimetres. Not below the base
    /// circle.
    pub minor_radius: f64,
    /// The radius of the roots, in millimetres.
    pub major_radius: f64,
    /// The radius of the rounds at the root corners, in millimetres (0 for sharp corners).
    pub fillet_radius: f64,
}

/// An internal part worked out.
#[derive(Clone, Copy, Debug)]
struct Formed {
    base: Base,
    /// Half the angle a space would span on the base circle.
    half_at_base: f64,
    minor: f64,
    major: f64,
    rho: f64,
    /// Half the angle a space spans at the tips of the teeth.
    half_at_minor: f64,
    /// How much of the flank is unwound from the base circle at the tips, and where the root round
    /// begins.
    roll_minor: f64,
    roll_round: f64,
    /// The centre of a root round: its distance from the axis and its angle from the middle of the
    /// space.
    centre_radius: f64,
    centre_angle: f64,
}

impl Formed {
    /// The point of the upper flank at which a length `roll` is unwound from the base circle.
    fn flank(&self, roll: f64) -> Point {
        let rb = self.base.rb;
        let angle = self.half_at_base - (roll / rb - (roll / rb).atan());
        on_circle([0.0, 0.0], rb.hypot(roll), angle)
    }
}

impl Internal {
    fn formed(&self) -> Result<Formed, Error> {
        let b = base(self.module, self.teeth, self.pressure_angle)?;
        let space = in_range("space width", self.space_width, 0.0, MAX_LENGTH)?;
        let minor = in_range("minor radius", self.minor_radius, 0.0, MAX_LENGTH)?;
        let major = in_range("major radius", self.major_radius, 0.0, MAX_LENGTH)?;
        let rho = zero_up_to("fillet radius", self.fillet_radius, MAX_LENGTH)?;
        if !(space < b.pitch) {
            return Err(Error::Invalid {
                why: "the spaces would be as wide as the pitch: no tooth is left",
            });
        }
        if minor < b.rb {
            return Err(Error::Invalid {
                why: "the tips of the teeth would be below the base circle",
            });
        }
        let centre_radius = major - rho;
        if !(centre_radius > minor) {
            return Err(Error::Invalid {
                why: "the roots must lie outside the tips by more than the fillet radius",
            });
        }
        let half_at_base = space / (2.0 * b.r) + b.inv;
        let roll_of = |radius: f64| (radius * radius - b.rb * b.rb).max(0.0).sqrt();
        let (roll_minor, roll_centre) = (roll_of(minor), roll_of(centre_radius));
        let turned = |roll: f64| roll / b.rb - (roll / b.rb).atan();
        let half_at_minor = half_at_base - turned(roll_minor);
        if !(half_at_minor < PI / b.z) {
            return Err(Error::Invalid {
                why: "the spaces would meet at the tips: no tooth is left",
            });
        }
        // The round's centre is on the involute that runs beside the flank at the round's radius,
        // inside the space: the same involute, turned back by the radius over the base radius.
        let centre_angle = half_at_base - rho / b.rb - turned(roll_centre);
        if !(centre_angle >= 0.0) {
            return Err(Error::Invalid {
                why: "the root rounds are too large: they would overlap",
            });
        }
        Ok(Formed {
            base: b,
            half_at_base,
            minor,
            major,
            rho,
            half_at_minor,
            roll_minor,
            roll_round: roll_centre + rho,
            centre_radius,
            centre_angle,
        })
    }

    /// The radius of the pitch circle.
    pub fn pitch_radius(&self) -> Result<f64, Error> {
        Ok(self.formed()?.base.r)
    }

    /// The radius of the base circle.
    pub fn base_radius(&self) -> Result<f64, Error> {
        Ok(self.formed()?.base.rb)
    }

    /// The radius at which the flank ends and the root round begins.
    pub fn form_radius(&self) -> Result<f64, Error> {
        let f = self.formed()?;
        Ok(f.base.rb.hypot(f.roll_round))
    }

    /// The outline of the opening, a space centred on the x axis. The flanks are given as points
    /// whose chords stay within `tolerance` (millimetres) of the true curves.
    ///
    /// One pitch is six pieces, counter-clockwise: flank, root round, root arc, root round, flank,
    /// tip arc (four when the corners are sharp).
    pub fn outline(&self, tolerance: f64) -> Result<Outline, Error> {
        let tolerance = checked_tolerance(tolerance)?;
        let f = self.formed()?;
        // From the tip outward
        let upper = sampled(&|roll| f.flank(roll), f.roll_minor, f.roll_round, tolerance)?;
        let lower: Vec<Point> = upper.iter().map(|&[x, y]| [x, -y]).collect();
        let centre = on_circle([0.0, 0.0], f.centre_radius, f.centre_angle);
        // From the root circle's direction to the flank's normal: a quarter turn less what the
        // centre's own involute has unwound.
        let round_sweep = FRAC_PI_2 - (((f.roll_round - f.rho) / f.base.rb).atan());

        let mut tooth = vec![Piece::Curve {
            points: lower,
            role: Role::Flank,
        }];
        if f.rho > 0.0 {
            tooth.push(Piece::Arc {
                centre: [centre[0], -centre[1]],
                radius: f.rho,
                start: -f.centre_angle - round_sweep,
                sweep: round_sweep,
                role: Role::Fillet,
            });
        }
        if f.centre_angle > 1e-14 {
            tooth.push(Piece::Arc {
                centre: [0.0, 0.0],
                radius: f.major,
                start: -f.centre_angle,
                sweep: 2.0 * f.centre_angle,
                role: Role::Root,
            });
        }
        if f.rho > 0.0 {
            tooth.push(Piece::Arc {
                centre,
                radius: f.rho,
                start: f.centre_angle,
                sweep: round_sweep,
                role: Role::Fillet,
            });
        }
        tooth.push(Piece::Curve {
            points: upper.iter().rev().copied().collect(),
            role: Role::Flank,
        });
        tooth.push(Piece::Arc {
            centre: [0.0, 0.0],
            radius: f.minor,
            start: f.half_at_minor,
            sweep: TAU / f.base.z - 2.0 * f.half_at_minor,
            role: Role::Tip,
        });
        Outline::new(self.teeth, tooth, tolerance)
    }
}

/// How a hub fits its shaft.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// The play between the flanks, both sides together, as an arc on the pitch circle, in
    /// millimetres.
    pub side_clearance: f64,
    /// How far the hub's teeth reach inward from the pitch circle, in modules.
    pub hub_addendum: f64,
    /// The gap between the tips of the shaft's teeth and the hub's roots, in millimetres.
    pub tip_clearance: f64,
    /// The radius of the rounds at the hub's root corners, in millimetres.
    pub hub_fillet_radius: f64,
    /// The least distance kept between the tips of the hub's teeth and the start of the shaft's
    /// root fillet, in millimetres.
    pub form_clearance: f64,
}

/// The hub that fits a shaft: the same flanks moved apart by the side clearance, roots clear of the
/// shaft's tips, and tips that stay on the shaft's flanks, clear of its root fillets.
///
/// Refused where the hub's teeth would reach into the shaft's root fillets (closer than the form
/// clearance), and where the rounds at the hub's root corners would reach the shaft's tip corners
/// with the hub turned on the shaft as far as the side clearance lets it.
pub fn hub_for(shaft: &External, fit: &Fit) -> Result<Internal, Error> {
    let cut = shaft.cut()?;
    let side = zero_up_to("side clearance", fit.side_clearance, MAX_LENGTH)?;
    let reach = in_range("hub addendum", fit.hub_addendum, 0.0, 5.0)? * cut.base.m;
    let tip = in_range("tip clearance", fit.tip_clearance, 0.0, MAX_LENGTH)?;
    let form = zero_up_to("form clearance", fit.form_clearance, MAX_LENGTH)?;
    let minor = cut.base.r - reach;
    if !(minor >= shaft.form_radius()? + form) {
        return Err(Error::Invalid {
            why: "the hub's teeth would reach into the shaft's root fillet",
        });
    }
    let hub = Internal {
        module: shaft.module,
        teeth: shaft.teeth,
        pressure_angle: shaft.pressure_angle,
        space_width: cut.s + side,
        minor_radius: minor,
        major_radius: cut.ra + tip,
        fillet_radius: fit.hub_fillet_radius,
    };
    let f = hub.formed()?;
    // Beyond its form radius the hub's space narrows into its root rounds; the shaft's tip corners,
    // the shaft's points nearest to them there, must stay inside the rounds.
    if cut.ra > f.base.rb.hypot(f.roll_round) && f.rho > 0.0 {
        let (ra, c) = (cut.ra, f.centre_radius);
        let reach_of_round = ((ra * ra + c * c - f.rho * f.rho) / (2.0 * ra * c))
            .clamp(-1.0, 1.0)
            .acos();
        // The hub is free to turn on the shaft by half the side clearance either way; turned as far
        // as it goes, the flanks touching, a tip corner stands that much nearer a round.
        let turned = side / (2.0 * cut.base.r);
        if !(cut.tip_half_angle + turned <= (f.centre_angle + reach_of_round) * (1.0 + 1e-12)) {
            return Err(Error::Invalid {
                why: "the hub's root rounds would reach the shaft's tip corners: the rounds are too large for the \
                      tip clearance",
            });
        }
    }
    Ok(hub)
}

/// A splined shaft and the hub that fits it ([`hub_for`]) as a pair on one axis. They turn as one;
/// adding to [`Pair::second_start`] turns the hub on the shaft, to find the play.
///
/// Refused as [`hub_for`] refuses the hub. Centred, the flanks are half the side clearance apart on
/// each side; centred or turned on the shaft until the flanks touch, the parts do not overlap: the
/// hub's tips clear the shaft's fillets by the form clearance, its roots clear the shaft's tips,
/// and its root rounds clear the shaft's tip corners.
pub fn spline_pair(shaft: &External, fit: &Fit, tolerance: f64) -> Result<Pair, Error> {
    let hub = hub_for(shaft, fit)?;
    Ok(Pair {
        first: Part {
            outline: shaft.outline(tolerance)?,
            material: Material::Inside,
            axis: [0.0, 0.0],
        },
        second: Part {
            outline: hub.outline(tolerance)?,
            material: Material::Outside,
            axis: [0.0, 0.0],
        },
        motion: Motion::Turning { ratio: 1.0 },
        second_start: 0.0,
        cycle: TAU / f64::from(shaft.teeth),
    })
}
