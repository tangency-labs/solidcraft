//! Formed beads (stiffening ribs) on a developed sheet: a bead pressed into one planar panel along
//! a straight centreline in the panel's chart. Across, its section is a trapezoid with filleted
//! corners; along, each end ramps up from the face over the end length. The sheet keeps its
//! thickness: the skin that stands out (the raised skin) and the skin under it are concentric,
//! and both meet the panel's faces on one rectangle, the footprint.
//!
//! [`bead_form`] gives what a bead does to the folded solid: the footprint, which its panel's faces
//! leave open, and the bead's 26 faces that close it, each with what it is ([`BeadFace`]). In the
//! flat the blank stays flat: [`bead_outline`] gives the footprint and the centreline there.
//! [`bead_against`] places a bead's footprint against what lies on its panel, exactly (no
//! polyline stands in for a curve), [`line_in_chart`] and [`segment_in_region`] find where a
//! sketch line on a panel's face lies in its chart, [`bead_flank_angle`] gives the flanks' angle
//! that leaves a crown width, and [`bead_note`] where a note along a bead stands in the flat.
//!
//! In a bead's own frame ξ runs across (0 on the centreline), η along it (0 and the centreline's
//! length at the ends' virtual sharps on the face) and h out of the face it stands out of (the
//! other face is at h = −t). Each skin is the lower of two profiles, P across (in ξ) and Q along
//! (in η); where they meet, at the footprint's four corners, the creases are traced as cubic
//! Bézier chains within [`CREASE_TOL`].
//!
//! Not upstream (SolidCraft's sheet-metal bead, moved into the kernel, docs/layers.md §1).
//!
//! Deviations: none (no upstream counterpart).

use crate::math::det;
use std::f64::consts::{FRAC_PI_2, PI};

use super::fold::Fold;
use super::layout::crossings;
use super::plane::{from_angle, Planar, X};
use super::rigid::Rigid3;
use crate::eval::vec::VecExt;
use crate::math::{Vec2, Vec3};
use crate::prism::placement::LoopPlacement;
use crate::prism::profile::{Loop2, Region2, Seg2};

/// The narrowest a bead's crown, and the shortest its crown line, may be (mm).
const MIN_CROWN: f64 = 1e-3;
/// The smallest inner radius of a bead's bends (mm): below it a bend's faces collapse onto its
/// edges.
pub const MIN_BEAD_RADIUS: f64 = 1e-3;
/// How far a traced crease may stand off the two faces it joins (mm), as a bend's edges may.
pub const CREASE_TOL: f64 = 1e-7;
/// Most cubic pieces in one crease.
const MAX_CREASE_PIECES: usize = 512;
/// How far an extruded skin's surface runs past its face, so the boundary lies well inside it.
const PAD: f64 = 1.0;
/// The least a bend of a bead may rise across its arc (mm): heights nearer than 1e-9 are one
/// height where the faces are built, so a gentler bend would lose its faces.
pub const MIN_BEND_RISE: f64 = 1e-6;
/// How near two outlines may come and still be apart where no distance is asked (mm).
const CONTACT_TOL: f64 = 1e-9;
/// How far a crease traced at the most pieces may still be off before the bead is refused (mm).
const CREASE_REFUSED: f64 = 1e-4;

/// A bead's section and end ramps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeadSection {
    /// From the face it stands out of to its crown on that side.
    pub depth: f64,
    /// Across, between the flanks' virtual sharps on that face.
    pub width: f64,
    /// The flanks' angle to the face (radians).
    pub angle: f64,
    /// The inner radius of the bends at the base.
    pub base_radius: f64,
    /// The inner radius of the bends at the crown.
    pub crown_radius: f64,
    /// Along the centreline, from an end's virtual sharp on the face to the virtual sharp where
    /// the ramp reaches the crown.
    pub end_length: f64,
}

/// A bead on a planar panel, in the panel's chart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bead {
    /// The panel it is pressed into.
    pub panel: usize,
    /// The centreline's start, where the first end ramp's virtual sharp meets the face.
    pub a: Vec2,
    /// The centreline's end.
    pub b: Vec2,
    /// It stands out of the panel's far face (the one a thickness into the material), else out of
    /// its near face (the sheet's surface).
    pub far: bool,
    /// Its section.
    pub section: BeadSection,
}

/// Why a bead cannot be made, with what would fit. An application words it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BeadRefusal {
    /// A size is not finite, or the sheet's thickness is not positive.
    NotFinite,
    /// Its centreline has no length.
    NoLength,
    /// Its depth, width or end length is not above 0.
    NotPositive,
    /// Its flanks' angle (radians) is not between 0° and 90°.
    Angle {
        /// The angle given.
        angle: f64,
    },
    /// A radius is less than [`MIN_BEAD_RADIUS`].
    Radius,
    /// Its gentlest bend (the smallest radius over the flatter of the flanks and the end ramps)
    /// rises `rise`, less than [`MIN_BEND_RISE`].
    GentleBend {
        /// How far the gentlest bend rises.
        rise: f64,
    },
    /// Too shallow for its radii at its flanks' angle (radians): at least `min_depth` deep.
    ShallowFlanks {
        /// The flanks' angle.
        angle: f64,
        /// The least depth that fits.
        min_depth: f64,
    },
    /// Too shallow for its radii on its end ramps, at their angle (radians).
    ShallowRamps {
        /// The ramps' angle.
        angle: f64,
    },
    /// Too narrow to leave a crown at its flanks' angle (radians): at least `min_width` wide.
    NoCrown {
        /// The flanks' angle.
        angle: f64,
        /// The least width that fits.
        min_width: f64,
    },
    /// Its centreline, `length` long, is too short for its two end ramps: at least `min_length`.
    TooShort {
        /// The centreline's length.
        length: f64,
        /// The least length that fits.
        min_length: f64,
    },
    /// A crease could not be traced within its tolerance: off by `off` mm.
    Crease {
        /// How far the trace was off.
        off: f64,
    },
    /// Its panel has no place on the folded sheet or in the flat.
    Unplaced,
    /// Its crown width is not at least 0 and less than its width.
    CrownWidth {
        /// The crown width given.
        crown_width: f64,
        /// The bead's width.
        width: f64,
    },
    /// An outline beside it could not be placed against its footprint.
    Undecided,
}

/// Where a bead's footprint lies against an outline on its panel ([`bead_against`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeadAgainst {
    /// Farther than the distance asked from the outline everywhere, outside it, and the outline
    /// outside the footprint.
    Clear,
    /// Within the distance of the outline somewhere: across it or along it.
    Meets,
    /// Farther than the distance from the outline everywhere, and held inside it.
    Inside,
    /// The outline lies wholly inside the footprint, farther than the distance from its edges.
    Around,
}

/// What a face of a bead is: its crown, or the `piece`th piece of its profile from the base (0:
/// the base bend, 1: the flank or the end ramp, 2: the crown bend), across it on its left or right
/// (seen from the sheet's surface, looking from the centreline's start to its end) or along it at
/// its start or end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeadPart {
    /// The crown.
    Crown,
    /// A piece of the section across it.
    Across {
        /// On the centreline's left.
        left: bool,
        /// Which piece, from the base.
        piece: usize,
    },
    /// A piece of an end ramp.
    Along {
        /// At the centreline's start.
        start: bool,
        /// Which piece, from the base.
        piece: usize,
    },
}

/// A face of a bead: its part, on the skin that continues the sheet's surface (`top`) or the
/// other one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeadFace {
    /// Which part of the skin.
    pub part: BeadPart,
    /// On the skin that continues the sheet's surface.
    pub top: bool,
}

/// An edge of a bead's face in space.
#[derive(Clone, Debug, PartialEq)]
pub enum FormEdge {
    /// A straight line.
    Line {
        /// Its start.
        a: Vec3,
        /// Its end.
        b: Vec3,
    },
    /// A circular arc through `mid`.
    Arc {
        /// Its start.
        a: Vec3,
        /// Its end.
        b: Vec3,
        /// A point between them.
        mid: Vec3,
    },
    /// A chain of cubic Bézier pieces: their control points, each piece's last shared with the
    /// next one's first.
    Chain {
        /// The control points.
        ctrl: Vec<Vec3>,
    },
}

impl FormEdge {
    /// The same edge run the other way.
    #[must_use]
    pub fn reversed(&self) -> FormEdge {
        match self {
            FormEdge::Line { a, b } => FormEdge::Line { a: *b, b: *a },
            FormEdge::Arc { a, b, mid } => FormEdge::Arc {
                a: *b,
                b: *a,
                mid: *mid,
            },
            FormEdge::Chain { ctrl } => FormEdge::Chain {
                ctrl: ctrl.iter().rev().copied().collect(),
            },
        }
    }
}

/// The surface a bead's face lies on.
#[derive(Clone, Debug, PartialEq)]
pub enum FormSurface {
    /// The plane of its loop.
    Plane,
    /// The circular arc from `a` through `mid` to `b`, swept along `dir`.
    Extruded {
        /// The arc's start.
        a: Vec3,
        /// The arc's end.
        b: Vec3,
        /// A point of the arc between them.
        mid: Vec3,
        /// The sweep.
        dir: Vec3,
    },
}

/// A face of a bead in space: its loop (the right way round about the way out), its surface, a
/// point on it and the way out of the material there, and what it is.
#[derive(Clone, Debug, PartialEq)]
pub struct FormFace {
    /// Its loop.
    pub edges: Vec<FormEdge>,
    /// Its surface.
    pub surface: FormSurface,
    /// A point on it.
    pub point: Vec3,
    /// The way out of the material at `point`.
    pub out: Vec3,
    /// What it is.
    pub face: BeadFace,
}

/// What a bead does to the folded solid: the footprint its panel's faces leave open (its corners
/// in the flat, clockwise as a cell's holes run, and its middle) and the bead's faces that close
/// it, in space.
#[derive(Clone, Debug, PartialEq)]
pub struct BeadForm {
    /// The footprint's corners in the flat, clockwise.
    pub open: [Vec2; 4],
    /// The footprint's middle in the flat.
    pub centre: Vec2,
    /// The bead's 26 faces.
    pub faces: Vec<FormFace>,
}

/// A bead in the flat: its footprint, its centreline, its crown, and how a note along it is
/// written.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeadOutline {
    /// The footprint's corners in the flat, counter-clockwise.
    pub footprint: [Vec2; 4],
    /// The centreline's start and end in the flat.
    pub start: Vec2,
    /// The centreline's end in the flat.
    pub end: Vec2,
    /// The crown's width between its virtual sharps.
    pub crown_width: f64,
    /// The crown line's length between the end ramps.
    pub crown_length: f64,
    /// The angle of a note written along the centreline (radians, in (−π/2, π/2], so it reads
    /// left to right or upward).
    pub note_angle: f64,
    /// The unit vector across the note, toward where it stands beside the centreline.
    pub note_side: Vec2,
}

/// One piece of a profile in (u, h): u from the footprint's edge inward, h out of the face the
/// bead stands out of.
#[derive(Clone, Copy, Debug)]
enum Seg1 {
    Line {
        u0: f64,
        h0: f64,
        u1: f64,
        h1: f64,
    },
    /// Centre (cu, ch), radius r, from angle a0 to a1.
    Arc {
        cu: f64,
        ch: f64,
        r: f64,
        a0: f64,
        a1: f64,
    },
}

impl Seg1 {
    fn at(&self, s: f64) -> (f64, f64) {
        match *self {
            Seg1::Line { u0, h0, u1, h1 } => (u0 + (u1 - u0) * s, h0 + (h1 - h0) * s),
            Seg1::Arc { cu, ch, r, a0, a1 } => {
                let a = a0 + (a1 - a0) * s;
                (cu + r * det::cos(a), ch + r * det::sin(a))
            }
        }
    }

    /// The angle of an arc at height h, on the side of the circle the arc runs on.
    fn angle_at(ch_r: (f64, f64), a0: f64, a1: f64, h: f64) -> f64 {
        let (ch, r) = ch_r;
        let x = ((h - ch) / r).clamp(-1.0, 1.0);
        if det::cos((a0 + a1) / 2.0) >= 0.0 {
            det::asin(x)
        } else {
            PI - det::asin(x)
        }
    }

    /// u at height h.
    fn u_at(&self, h: f64) -> f64 {
        match *self {
            Seg1::Line { u0, h0, u1, h1 } => u0 + (h - h0) * (u1 - u0) / (h1 - h0),
            Seg1::Arc { cu, ch, r, a0, a1 } => {
                cu + r * det::cos(Self::angle_at((ch, r), a0, a1, h))
            }
        }
    }

    /// The tangent at parameter s (any length), the way s runs.
    fn tangent(&self, s: f64) -> (f64, f64) {
        match *self {
            Seg1::Line { u0, h0, u1, h1 } => (u1 - u0, h1 - h0),
            Seg1::Arc { r, a0, a1, .. } => {
                let a = a0 + (a1 - a0) * s;
                let k = r * (a1 - a0);
                (-det::sin(a) * k, det::cos(a) * k)
            }
        }
    }
}

/// One skin's profile: rising from the footprint's edge (u = 0, h = `base`) to the crown
/// (u = `top_u`, h = `top`): the base bend, the flank and the crown bend.
#[derive(Clone, Debug)]
struct Profile {
    segs: [Seg1; 3],
    base: f64,
    top: f64,
    top_u: f64,
}

impl Profile {
    /// A skin of a bead rising `depth` at `angle`, with inner radii `rb` (base) and `rc`
    /// (crown), in a sheet `t` thick: the raised one (its face at h = 0) or the one under it.
    fn new(angle: f64, depth: f64, rb: f64, rc: f64, t: f64, raised: bool) -> Profile {
        let ro = rc + t;
        let half = det::tan(angle / 2.0);
        let uc = rb * half + depth / det::tan(angle) + ro * half;
        let (r_base, r_crown, base, top) = if raised {
            (rb, ro, 0.0, depth)
        } else {
            (rb + t, rc, -t, depth - t)
        };
        let (s, c) = det::sin_cos(angle);
        let (bu, bh) = (r_base * s, rb - r_base * c);
        let (cu, chh) = (uc - r_crown * s, depth - ro + r_crown * c);
        Profile {
            segs: [
                Seg1::Arc {
                    cu: 0.0,
                    ch: rb,
                    r: r_base,
                    a0: -FRAC_PI_2,
                    a1: -FRAC_PI_2 + angle,
                },
                Seg1::Line {
                    u0: bu,
                    h0: bh,
                    u1: cu,
                    h1: chh,
                },
                Seg1::Arc {
                    cu: uc,
                    ch: depth - ro,
                    r: r_crown,
                    a0: FRAC_PI_2 + angle,
                    a1: FRAC_PI_2,
                },
            ],
            base,
            top,
            top_u: uc,
        }
    }

    /// The heights where the pieces meet, from the base to the top.
    fn breaks(&self) -> [f64; 4] {
        let [_, flank, crown] = &self.segs;
        [self.base, flank.at(0.0).1, crown.at(0.0).1, self.top]
    }

    /// The piece holding the heights from h up (the upper one at a joint).
    fn piece(&self, h: f64) -> Seg1 {
        let [base, flank, crown] = self.segs;
        let [_, b1, b2, _] = self.breaks();
        if h < b1 - 1e-12 {
            base
        } else if h < b2 - 1e-12 {
            flank
        } else {
            crown
        }
    }
}

/// A bead's numbers, worked out for a sheet `t` thick.
#[derive(Clone, Debug)]
struct BeadGeom {
    /// Along: the centreline's length; across and along unit vectors in the panel's chart.
    len: f64,
    ex: Vec2,
    ey: Vec2,
    /// Half the footprint's width, and how far the footprint runs past each end.
    xf: f64,
    sq: f64,
    /// Across and along, raised skin then the one under it.
    p: [Profile; 2],
    q: [Profile; 2],
}

impl BeadGeom {
    /// The bead's shape, or why it cannot be made.
    fn new(b: &Bead, t: f64) -> Result<BeadGeom, BeadRefusal> {
        let s = &b.section;
        let d = b.b - b.a;
        let len = d.len();
        let finite = [
            s.depth,
            s.width,
            s.angle,
            s.base_radius,
            s.crown_radius,
            s.end_length,
            len,
            t,
        ]
        .iter()
        .all(|v| v.is_finite());
        if !finite || t <= 0.0 {
            return Err(BeadRefusal::NotFinite);
        }
        if len <= 1e-6 {
            return Err(BeadRefusal::NoLength);
        }
        if !(s.depth > 0.0 && s.width > 0.0 && s.end_length > 0.0) {
            return Err(BeadRefusal::NotPositive);
        }
        if !(s.angle > 1e-3 && s.angle < FRAC_PI_2 - 1e-3) {
            return Err(BeadRefusal::Angle { angle: s.angle });
        }
        if !(s.base_radius >= MIN_BEAD_RADIUS && s.crown_radius >= MIN_BEAD_RADIUS) {
            return Err(BeadRefusal::Radius);
        }
        let (alpha, beta) = (s.angle, det::atan(s.depth / s.end_length));
        let (rb, rc) = (s.base_radius, s.crown_radius);
        let gentlest = rb.min(rc) * (1.0 - det::cos(alpha.min(beta)));
        if gentlest < MIN_BEND_RISE {
            return Err(BeadRefusal::GentleBend { rise: gentlest });
        }
        let ro = rc + t;
        // The flanks and the end ramps have length only if the bends' rises leave room.
        let rise = |a: f64| (rb + ro) * (1.0 - det::cos(a));
        if s.depth <= rise(alpha) + 1e-6 {
            return Err(BeadRefusal::ShallowFlanks {
                angle: alpha,
                min_depth: rise(alpha) + 1e-6,
            });
        }
        if s.depth <= rise(beta) + 1e-6 {
            return Err(BeadRefusal::ShallowRamps { angle: beta });
        }
        let half = |a: f64| det::tan(a / 2.0);
        let crown = s.width / 2.0 - s.depth / det::tan(alpha) - ro * half(alpha);
        if crown < MIN_CROWN / 2.0 {
            return Err(BeadRefusal::NoCrown {
                angle: alpha,
                min_width: 2.0 * (s.depth / det::tan(alpha) + ro * half(alpha)) + MIN_CROWN,
            });
        }
        let ramp = s.end_length + ro * half(beta);
        if len - 2.0 * ramp < MIN_CROWN {
            return Err(BeadRefusal::TooShort {
                length: len,
                min_length: 2.0 * ramp + MIN_CROWN,
            });
        }
        let ey = d * (1.0 / len);
        let ex = Vec2::new(ey.y, -ey.x);
        let p = [
            Profile::new(alpha, s.depth, rb, rc, t, true),
            Profile::new(alpha, s.depth, rb, rc, t, false),
        ];
        let q = [
            Profile::new(beta, s.depth, rb, rc, t, true),
            Profile::new(beta, s.depth, rb, rc, t, false),
        ];
        Ok(BeadGeom {
            len,
            ex,
            ey,
            xf: s.width / 2.0 + rb * half(alpha),
            sq: rb * half(beta),
            p,
            q,
        })
    }

    /// One skin's two profiles (0: the raised skin, 1: the one under it).
    fn skin(&self, skin: usize) -> (&Profile, &Profile) {
        let [p0, p1] = &self.p;
        let [q0, q1] = &self.q;
        if skin == 0 {
            (p0, q0)
        } else {
            (p1, q1)
        }
    }

    /// The footprint's corners in the panel's chart, counter-clockwise.
    fn footprint(&self, b: &Bead) -> [Vec2; 4] {
        let at = |x: f64, y: f64| b.a + self.ex * x + self.ey * y;
        let (x, y0, y1) = (self.xf, -self.sq, self.len + self.sq);
        [at(-x, y0), at(x, y0), at(x, y1), at(-x, y1)]
    }

    /// The footprint's middle in the panel's chart: the centreline's.
    fn centre(&self, b: &Bead) -> Vec2 {
        b.a + self.ey * (self.len / 2.0)
    }

    /// The footprint grown by `grow` on every side, as a loop in the panel's chart,
    /// counter-clockwise.
    fn footprint_loop(&self, b: &Bead, grow: f64) -> Loop2 {
        let at = |x: f64, y: f64| b.a + self.ex * x + self.ey * y;
        let (x, y0, y1) = (self.xf + grow, -self.sq - grow, self.len + self.sq + grow);
        let c = [at(-x, y0), at(x, y0), at(x, y1), at(-x, y1)];
        Loop2 {
            segs: c
                .iter()
                .zip(c.iter().cycle().skip(1))
                .map(|(a, b)| Seg2::Line { a: *a, b: *b })
                .collect(),
        }
    }

    /// The crown's width and the length of its line between the ramps.
    fn crown(&self) -> (f64, f64) {
        let (p, q) = self.skin(0);
        (
            2.0 * (self.xf - p.top_u),
            self.len + 2.0 * self.sq - 2.0 * q.top_u,
        )
    }
}

/// The point of the left-start crease (ξ, η, h) on the piece where P's piece meets Q's piece, by
/// a parameter that runs smoothly along it: the shared angle where two bends of one radius and
/// centre height meet (a mitre), else an arc's angle, else the height.
#[derive(Clone, Copy)]
enum Param {
    Mitre { r: f64, ch: f64, cp: f64, cq: f64 },
    PArc,
    QArc,
    Height,
}

fn crease_param(sp: &Seg1, sq: &Seg1) -> Param {
    match (sp, sq) {
        (
            Seg1::Arc { cu: cp, ch, r, .. },
            Seg1::Arc {
                cu: cq,
                ch: chq,
                r: rq,
                ..
            },
        ) if (r - rq).abs() < 1e-12 && (ch - chq).abs() < 1e-12 => Param::Mitre {
            r: *r,
            ch: *ch,
            cp: *cp,
            cq: *cq,
        },
        (_, Seg1::Arc { .. }) => Param::QArc,
        (Seg1::Arc { .. }, _) => Param::PArc,
        _ => Param::Height,
    }
}

/// A cubic Bézier piece at `u` in [0, 1].
fn bezier([c0, c1, c2, c3]: &[Vec3; 4], u: f64) -> Vec3 {
    let v = 1.0 - u;
    *c0 * (v * v * v) + *c1 * (3.0 * v * v * u) + *c2 * (3.0 * v * u * u) + *c3 * (u * u * u)
}

impl BeadGeom {
    /// The left-start crease of one skin between heights `h0` and `h1`, traced to
    /// [`CREASE_TOL`]: its cubic chain's control points.
    fn crease(&self, skin: usize, h0: f64, h1: f64) -> Result<Vec<Vec3>, BeadRefusal> {
        let (p, q) = self.skin(skin);
        let mid = (h0 + h1) / 2.0;
        let (sp, sq) = (p.piece(mid), q.piece(mid));
        let param = crease_param(&sp, &sq);
        let (xf, sqo) = (self.xf, self.sq);
        let point = move |s: f64| -> Vec3 {
            let (up, vq, h) = match (param, sp, sq) {
                (Param::Mitre { r, ch, cp, cq }, ..) => (
                    cp + r * det::cos(s),
                    cq + r * det::cos(s),
                    ch + r * det::sin(s),
                ),
                (Param::PArc, Seg1::Arc { cu, ch, r, .. }, _) => {
                    let h = ch + r * det::sin(s);
                    (cu + r * det::cos(s), sq.u_at(h), h)
                }
                (Param::QArc, _, Seg1::Arc { cu, ch, r, .. }) => {
                    let h = ch + r * det::sin(s);
                    (sp.u_at(h), cu + r * det::cos(s), h)
                }
                _ => (sp.u_at(s), sq.u_at(s), s),
            };
            Vec3::new(-xf + up, -sqo + vq, h)
        };
        let s_at = |h: f64| match (param, sp, sq) {
            (Param::Mitre { .. } | Param::PArc, Seg1::Arc { ch, r, a0, a1, .. }, _)
            | (Param::QArc, _, Seg1::Arc { ch, r, a0, a1, .. }) => {
                Seg1::angle_at((ch, r), a0, a1, h)
            }
            _ => h,
        };
        let (s0, s1) = (s_at(h0), s_at(h1));
        let delta = (s1 - s0).abs().max(1e-12) * 1e-5;
        let deriv = |s: f64| (point(s + delta) - point(s - delta)) * (0.5 / delta);
        let cubic = |a: f64, b: f64| -> [Vec3; 4] {
            let (pa, pb) = (point(a), point(b));
            let k = (b - a) / 3.0;
            [pa, pa + deriv(a) * k, pb - deriv(b) * k, pb]
        };
        let mut n = 2usize;
        loop {
            let at = |k: f64| s0 + (s1 - s0) * k / n as f64;
            let pieces: Vec<[Vec3; 4]> = (0..n)
                .map(|k| cubic(at(k as f64), at(k as f64 + 1.0)))
                .collect();
            let mut worst = 0.0f64;
            for (k, c) in pieces.iter().enumerate() {
                for j in 1..4 {
                    let u = f64::from(j) / 4.0;
                    worst = worst.max(bezier(c, u).dist(point(at(k as f64 + u))));
                }
            }
            if worst <= CREASE_TOL || n >= MAX_CREASE_PIECES {
                if !worst.is_finite() || worst > CREASE_REFUSED {
                    return Err(BeadRefusal::Crease { off: worst });
                }
                let mut chain = Vec::new();
                for (k, c) in pieces.iter().enumerate() {
                    chain.extend(c.iter().skip(usize::from(k > 0)).copied());
                }
                return Ok(chain);
            }
            n = (n * 2).min(MAX_CREASE_PIECES);
        }
    }
}

/// Mirror a point of the left-start corner into corner `c` (0 left start, 1 right start, 2 left
/// end, 3 right end).
fn corner(p: Vec3, c: usize, len: f64) -> Vec3 {
    let x = if c % 2 == 1 { -p.x } else { p.x };
    let y = if c >= 2 { len - p.y } else { p.y };
    Vec3::new(x, y, p.z)
}

/// The control points (or ends) of an edge, for a rough normal of its loop.
fn edge_points(e: &FormEdge) -> Vec<Vec3> {
    match e {
        FormEdge::Line { a, .. } => vec![*a],
        FormEdge::Arc { a, mid, .. } => vec![*a, *mid],
        FormEdge::Chain { ctrl } => ctrl
            .iter()
            .take(ctrl.len().saturating_sub(1))
            .copied()
            .collect(),
    }
}

/// A loop's area vector (Newell): its normal, as long as the area it bounds.
fn newell(l: &[FormEdge]) -> Vec3 {
    let pts: Vec<Vec3> = l.iter().flat_map(edge_points).collect();
    let mut n = Vec3::ZERO;
    for (p, q) in pts.iter().zip(pts.iter().cycle().skip(1)) {
        n += Vec3::new(
            (p.y - q.y) * (p.z + q.z),
            (p.z - q.z) * (p.x + q.x),
            (p.x - q.x) * (p.y + q.y),
        );
    }
    n * 0.5
}

/// A face of a bead, in its frame: its loop (the right way round about its probe's normal), its
/// surface, a point on it and the way out there, and what part of the skin it is.
struct LocalFace {
    edges: Vec<FormEdge>,
    surface: FormSurface,
    probe: (Vec3, Vec3),
    part: BeadPart,
}

/// One skin's faces in the bead's frame (0: raised, 1: under it); `left` is whether the side at
/// −ξ is the centreline's left.
fn skin_faces(g: &BeadGeom, skin: usize, left: bool) -> Result<Vec<LocalFace>, BeadRefusal> {
    let (p, q) = g.skin(skin);
    let outward = if skin == 0 { 1.0 } else { -1.0 };
    // The heights where either profile changes piece, from the base to the top.
    let mut hs: Vec<f64> = p
        .breaks()
        .iter()
        .chain(q.breaks().iter())
        .copied()
        .collect();
    hs.sort_by(f64::total_cmp);
    hs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let (Some(&lowest), Some(&highest)) = (hs.first(), hs.last()) else {
        return Err(BeadRefusal::NotPositive);
    };
    if hs.len() < 2 || highest <= lowest {
        return Err(BeadRefusal::NotPositive);
    }
    // The left-start crease's pieces and its points at those heights; the base and the top
    // exactly at the footprint's and the crown's corners.
    let creases: Vec<Vec<Vec3>> = hs
        .windows(2)
        .map(|w| {
            g.crease(
                skin,
                w.first().copied().unwrap_or(lowest),
                w.last().copied().unwrap_or(highest),
            )
        })
        .collect::<Result<_, _>>()?;
    let mut verts: Vec<Vec3> = Vec::new();
    verts.push(Vec3::new(-g.xf, -g.sq, p.base));
    verts.extend(
        creases
            .iter()
            .skip(1)
            .map(|c| c.first().copied().unwrap_or(Vec3::ZERO)),
    );
    verts.push(Vec3::new(-g.xf + p.top_u, -g.sq + q.top_u, p.top));
    // Every index below is a height's place in `hs`, so `verts` and `creases` hold it.
    let vert = |c: usize, j: usize| corner(verts.get(j).copied().unwrap_or(Vec3::ZERO), c, g.len);
    // Corner c's crease piece j, from height j to j + 1, its ends exactly on the vertices.
    let crease = |c: usize, j: usize| -> FormEdge {
        let mut ctrl: Vec<Vec3> = creases
            .get(j)
            .map(|ch| ch.iter().map(|p| corner(*p, c, g.len)).collect())
            .unwrap_or_default();
        if let Some(first) = ctrl.first_mut() {
            *first = vert(c, j);
        }
        if let Some(last) = ctrl.last_mut() {
            *last = vert(c, j + 1);
        }
        FormEdge::Chain { ctrl }
    };
    let index = |h: f64| hs.iter().position(|x| (x - h).abs() < 1e-9).unwrap_or(0);
    let line = |a: Vec3, b: Vec3| FormEdge::Line { a, b };
    // A loop between creases `cs` and `ce`, from height j0 up to j1.
    let band = |cs: usize, ce: usize, j0: usize, j1: usize| {
        let mut edges = vec![line(vert(cs, j0), vert(ce, j0))];
        edges.extend((j0..j1).map(|j| crease(ce, j)));
        edges.push(line(vert(ce, j1), vert(cs, j1)));
        edges.extend((j0..j1).rev().map(|j| crease(cs, j).reversed()));
        edges
    };
    let mut faces = Vec::new();
    let mut push =
        |mut edges: Vec<FormEdge>, surface: FormSurface, probe: (Vec3, Vec3), part: BeadPart| {
            if newell(&edges).dot(probe.1) < 0.0 {
                edges = edges.iter().rev().map(FormEdge::reversed).collect();
            }
            faces.push(LocalFace {
                edges,
                surface,
                probe,
                part,
            });
        };
    let (pb, qb) = (p.breaks(), q.breaks());
    let pieces = p
        .segs
        .iter()
        .zip(&q.segs)
        .zip(pb.windows(2).zip(qb.windows(2)));
    for (k, ((&sp, &sq), (pw, qw))) in pieces.enumerate() {
        let (Some(&pb0), Some(&pb1), Some(&qb0), Some(&qb1)) =
            (pw.first(), pw.last(), qw.first(), qw.last())
        else {
            continue;
        };
        // Across: the piece of P on each side, between creases at the start and at the end.
        let (j0, j1) = (index(pb0), index(pb1));
        for (side, (cs, ce)) in [(0usize, 2usize), (1, 3)].into_iter().enumerate() {
            let sign = if side == 0 { 1.0 } else { -1.0 };
            let x = |u: f64| sign * (-g.xf + u);
            let surface = match sp {
                Seg1::Line { .. } => FormSurface::Plane,
                Seg1::Arc { .. } => {
                    let pt = |s: f64| {
                        let (u, h) = sp.at(s);
                        Vec3::new(x(u), -g.sq - PAD, h)
                    };
                    FormSurface::Extruded {
                        a: pt(0.0),
                        b: pt(1.0),
                        mid: pt(0.5),
                        dir: Vec3::new(0.0, g.len + 2.0 * g.sq + 2.0 * PAD, 0.0),
                    }
                }
            };
            let (u, h) = sp.at(0.5);
            let (du, dh) = sp.tangent(0.5);
            let normal = Vec3::new(-dh * sign, 0.0, du) * outward;
            // Side 0 is at −ξ.
            push(
                band(cs, ce, j0, j1),
                surface,
                (Vec3::new(x(u), g.len / 2.0, h), normal),
                BeadPart::Across {
                    left: (side == 0) == left,
                    piece: k,
                },
            );
        }
        // Along: the piece of Q at each end, between the left and the right crease.
        let (j0, j1) = (index(qb0), index(qb1));
        for (end, (cl, cr)) in [(0usize, 1usize), (2, 3)].into_iter().enumerate() {
            let sign = if end == 0 { 1.0 } else { -1.0 };
            let y = |v: f64| {
                if end == 0 {
                    -g.sq + v
                } else {
                    g.len + g.sq - v
                }
            };
            let surface = match sq {
                Seg1::Line { .. } => FormSurface::Plane,
                Seg1::Arc { .. } => {
                    let pt = |s: f64| {
                        let (v, h) = sq.at(s);
                        Vec3::new(-g.xf - PAD, y(v), h)
                    };
                    FormSurface::Extruded {
                        a: pt(0.0),
                        b: pt(1.0),
                        mid: pt(0.5),
                        dir: Vec3::new(2.0 * g.xf + 2.0 * PAD, 0.0, 0.0),
                    }
                }
            };
            let (v, h) = sq.at(0.5);
            let (dv, dh) = sq.tangent(0.5);
            let normal = Vec3::new(0.0, -dh * sign, dv) * outward;
            push(
                band(cl, cr, j0, j1),
                surface,
                (Vec3::new(0.0, y(v), h), normal),
                BeadPart::Along {
                    start: end == 0,
                    piece: k,
                },
            );
        }
    }
    // The crown: a rectangle at the top.
    let top = hs.len() - 1;
    let edges = vec![
        line(vert(0, top), vert(1, top)),
        line(vert(1, top), vert(3, top)),
        line(vert(3, top), vert(2, top)),
        line(vert(2, top), vert(0, top)),
    ];
    push(
        edges,
        FormSurface::Plane,
        (
            Vec3::new(0.0, g.len / 2.0, p.top),
            Vec3::new(0.0, 0.0, outward),
        ),
        BeadPart::Crown,
    );
    Ok(faces)
}

/// What bead `b` does to the folded solid of a sheet `t` thick, folded as `fold` says: the
/// footprint its panel's faces leave open (in the flat) and its 26 faces, in space. Its faces are
/// built in the bead's frame and carried to the panel's place by the fold.
pub fn bead_form(fold: &Fold, b: &Bead, t: f64) -> Result<BeadForm, BeadRefusal> {
    let g = BeadGeom::new(b, t)?;
    let flat = |q: Vec2| fold.flat_point(b.panel, q).ok_or(BeadRefusal::Unplaced);
    let [f0, f1, f2, f3] = g.footprint(b);
    let open = [flat(f0)?, flat(f3)?, flat(f2)?, flat(f1)?];
    let centre = flat(g.centre(b))?;
    // A point of the bead's frame (ξ, η, h) in space. Depth runs into the material from the
    // panel's surface: out of the far face is t + h, out of the near face −h, with ξ turned so
    // that the frame keeps its hand.
    let w = |p: Vec3| -> Result<Vec3, BeadRefusal> {
        let x = if b.far { p.x } else { -p.x };
        let q = b.a + g.ex * x + g.ey * p.y;
        let depth = if b.far { t + p.z } else { -p.z };
        fold.surface_point(b.panel, q, depth)
            .filter(|v| VecExt::is_finite(*v))
            .ok_or(BeadRefusal::Unplaced)
    };
    // Which side of the centreline −ξ is on, seen from the sheet's surface: −ξ is the chart's
    // `ex` side unless the bead stands out of the far face (see `w`), and `ex` is the
    // centreline's left where the chart turns the other way about the surface's normal from the
    // way space does.
    let on_face = |q: Vec2, depth: f64| {
        fold.surface_point(b.panel, q, depth)
            .ok_or(BeadRefusal::Unplaced)
    };
    let o = on_face(b.a, 0.0)?;
    let (along_x, along_y, up) = (
        on_face(b.a + g.ex, 0.0)? - o,
        on_face(b.a + g.ey, 0.0)? - o,
        o - on_face(b.a, t)?,
    );
    let ex_left = along_x.cross(along_y).dot(up) < 0.0;
    let left = b.far != ex_left;
    let edge = |e: &FormEdge| -> Result<FormEdge, BeadRefusal> {
        Ok(match e {
            FormEdge::Line { a, b: e1 } => FormEdge::Line {
                a: w(*a)?,
                b: w(*e1)?,
            },
            FormEdge::Arc { a, b: e1, mid } => FormEdge::Arc {
                a: w(*a)?,
                b: w(*e1)?,
                mid: w(*mid)?,
            },
            FormEdge::Chain { ctrl } => FormEdge::Chain {
                ctrl: ctrl.iter().map(|p| w(*p)).collect::<Result<_, _>>()?,
            },
        })
    };
    let mut faces = Vec::new();
    for skin in 0..2 {
        // The raised skin continues the face the bead stands out of.
        let top = (skin == 0) != b.far;
        for f in skin_faces(&g, skin, left)? {
            let edges: Vec<FormEdge> = f.edges.iter().map(&edge).collect::<Result<_, _>>()?;
            let surface = match f.surface {
                FormSurface::Extruded { a, b: e1, mid, dir } => FormSurface::Extruded {
                    a: w(a)?,
                    b: w(e1)?,
                    mid: w(mid)?,
                    dir: w(a + dir)? - w(a)?,
                },
                FormSurface::Plane => FormSurface::Plane,
            };
            let (p, n) = f.probe;
            let point = w(p)?;
            faces.push(FormFace {
                edges,
                surface,
                point,
                out: w(p + n)? - point,
                face: BeadFace { part: f.part, top },
            });
        }
    }
    Ok(BeadForm {
        open,
        centre,
        faces,
    })
}

/// Bead `b` in the flat of a sheet `t` thick, folded as `fold` says: its footprint, its
/// centreline, its crown, and how a note along it is written. With `mirror`, in the flat mirrored
/// across its x axis (y to −y): the flat as a flat pattern is read, from the sheet's surface (the
/// panels' charts face the material, so the flat they lay out is that view mirrored).
pub fn bead_outline(
    fold: &Fold,
    b: &Bead,
    t: f64,
    mirror: bool,
) -> Result<BeadOutline, BeadRefusal> {
    let g = BeadGeom::new(b, t)?;
    let flat = |q: Vec2| {
        let p = fold.flat_point(b.panel, q).ok_or(BeadRefusal::Unplaced)?;
        Ok(if mirror { Vec2::new(p.x, -p.y) } else { p })
    };
    // Counter-clockwise either way: the mirror turns the corners' order round.
    let [f0, f1, f2, f3] = g.footprint(b);
    let [f0, f1, f2, f3] = if mirror {
        [f0, f3, f2, f1]
    } else {
        [f0, f1, f2, f3]
    };
    let (start, end) = (flat(b.a)?, flat(b.b)?);
    let (crown_width, crown_length) = g.crown();
    // Along the centreline, turned to read left to right or upward.
    let mut note_angle = (end - start).normalized().unwrap_or(X).angle();
    if note_angle > FRAC_PI_2 + 1e-9 {
        note_angle -= PI;
    } else if note_angle <= -FRAC_PI_2 + 1e-9 {
        note_angle += PI;
    }
    Ok(BeadOutline {
        footprint: [flat(f0)?, flat(f1)?, flat(f2)?, flat(f3)?],
        start,
        end,
        crown_width,
        crown_length,
        note_angle,
        note_side: from_angle(note_angle).perp(),
    })
}

/// Bead `b`'s footprint in its panel's chart, counter-clockwise, on a sheet `t` thick.
pub fn bead_footprint(b: &Bead, t: f64) -> Result<[Vec2; 4], BeadRefusal> {
    Ok(BeadGeom::new(b, t)?.footprint(b))
}

/// Where bead `b`'s footprint (on a sheet `t` thick, in its panel's chart) lies against the
/// outline `l` (in the chart), coming within `near` of it counting as meeting it. Exact: for
/// outlines of lines and arcs the nearest approach is the kernel's own ([`Loop2::placement`]); an
/// outline with other curves is tested against the footprint grown by `near`, by where their
/// edges cross ([`crossings`]) and which holds the other ([`Loop2::holds`]).
pub fn bead_against(b: &Bead, t: f64, l: &Loop2, near: f64) -> Result<BeadAgainst, BeadRefusal> {
    let g = BeadGeom::new(b, t)?;
    let near = if near.is_finite() {
        near.max(0.0)
    } else {
        return Err(BeadRefusal::NotFinite);
    };
    let footprint = g.footprint_loop(b, 0.0);
    let tol = near.max(CONTACT_TOL);
    if let (Some(fp), Some(lp)) = (footprint.placement(l, tol), l.placement(&footprint, tol)) {
        return Ok(match (fp, lp) {
            (LoopPlacement::Crossing | LoopPlacement::Coincident, _)
            | (_, LoopPlacement::Crossing | LoopPlacement::Coincident) => BeadAgainst::Meets,
            (LoopPlacement::Inside, _) => BeadAgainst::Inside,
            (_, LoopPlacement::Inside) => BeadAgainst::Around,
            _ => BeadAgainst::Clear,
        });
    }
    let grown = g.footprint_loop(b, near);
    if !crossings(&grown.segs, &l.segs).is_empty() {
        return Ok(BeadAgainst::Meets);
    }
    let first = |lp: &Loop2| lp.segs.first().map(Seg2::start);
    let held =
        |by: &Loop2, p: Option<Vec2>| p.and_then(|p| by.holds(p)).ok_or(BeadRefusal::Undecided);
    if held(l, first(&grown))? {
        Ok(BeadAgainst::Inside)
    } else if held(&grown, first(l))? {
        Ok(BeadAgainst::Around)
    } else {
        Ok(BeadAgainst::Clear)
    }
}

/// Whether the segment from `a` to `b` lies wholly in `region`: both ends held by it and no edge
/// of it crossed. None where a hold cannot be decided.
#[must_use]
pub fn segment_in_region(region: &Region2, a: Vec2, b: Vec2) -> Option<bool> {
    let held = |p: Vec2| -> Option<bool> {
        let mut inside = region.outer.holds(p)?;
        for h in &region.holes {
            inside = inside && !h.holds(p)?;
        }
        Some(inside)
    };
    if !(held(a)? && held(b)?) {
        return Some(false);
    }
    let line = [Seg2::Line { a, b }];
    Some(
        std::iter::once(&region.outer)
            .chain(&region.holes)
            .all(|l| crossings(&line, &l.segs).is_empty()),
    )
}

/// The flanks' angle (radians) of a bead `depth` deep and `width` wide that leaves a crown
/// `crown_width` wide between the flanks' virtual sharps.
pub fn bead_flank_angle(depth: f64, width: f64, crown_width: f64) -> Result<f64, BeadRefusal> {
    if ![depth, width, crown_width].iter().all(|v| v.is_finite()) {
        return Err(BeadRefusal::NotFinite);
    }
    if !(crown_width >= 0.0 && crown_width < width) {
        return Err(BeadRefusal::CrownWidth { crown_width, width });
    }
    Ok(det::atan(depth / ((width - crown_width) / 2.0)))
}

/// Where a note `height` high along bead `b` stands in the flat of a sheet `t` thick, folded as
/// `fold` says (with `mirror`, as a flat pattern is read: see [`bead_outline`]): its anchor, beside
/// the centreline's middle, half the width and a text height off it on its left, and its angle
/// (radians, reading left to right or upward).
pub fn bead_note(
    fold: &Fold,
    b: &Bead,
    t: f64,
    mirror: bool,
    height: f64,
) -> Result<(Vec2, f64), BeadRefusal> {
    let o = bead_outline(fold, b, t, mirror)?;
    let at = (o.start + o.end) * 0.5 + o.note_side * (b.section.width / 2.0 + height);
    Ok((at, o.note_angle))
}

/// Where a straight line from `a` to `b` (in space), drawn on the plane through `origin` with unit
/// normal `normal`, lies in the chart of a planar panel placed at `place` in a sheet `t` thick:
/// its ends in the chart, and whether the plane is the panel's far face (else its surface). None
/// where the plane is neither of the panel's faces.
#[must_use]
pub fn line_in_chart(
    place: &Rigid3,
    t: f64,
    origin: Vec3,
    normal: Vec3,
    a: Vec3,
    b: Vec3,
) -> Option<(Vec2, Vec2, bool)> {
    if place.z.cross(normal).len() > 1e-6 {
        return None;
    }
    let inv = place.inverse();
    let depth = inv.point(origin).z;
    let far = (depth - t).abs() < 1e-6;
    if !(depth.abs() < 1e-6 || far) {
        return None;
    }
    let (qa, qb) = (inv.point(a), inv.point(b));
    Some((Vec2::new(qa.x, qa.y), Vec2::new(qb.x, qb.y), far))
}

#[cfg(test)]
#[path = "bead_tests.rs"]
mod tests;
