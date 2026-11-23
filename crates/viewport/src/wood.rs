//! Procedural 3D wood grain: a colour and a roughness for any point of a body, worked out from
//! where the point lies in the body's grain frame. There are no texture coordinates, so there
//! are no seams and no stretching on tapered, angled or curved faces.
//!
//! The grain frame puts the tree's pith on the frame's X axis: X runs along the grain, and the
//! growth rings are circles around X in the YZ plane. The rings are distorted with low-frequency
//! noise (the figure), and fine streaks and pores run along X.
//!
//! The same function runs on the CPU ([`WoodShade::sample`]) and on the GPU (`wood.wgsl` in the
//! GPU viewport crate). Both use `f32`, the same integer hash and the same constants, so the CPU
//! copy is a reference the shader is tested against (to within GPU rounding).

use crate::vec::Vec3Ops;
use solidcore_record::Vec3;

/// One species' look: colours in sRGB (0..1) and the shape of its grain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WoodLook {
    /// Earlywood: the light, soft part of each ring.
    pub early: [f32; 3],
    /// Latewood: the dense, dark band at the end of each ring.
    pub late: [f32; 3],
    /// The colour of the dark streaks along the grain (walnut's near-black lines, teak's oily
    /// ones).
    pub streak: [f32; 3],
    /// The third tone: the lighter, warmer colour of the broad zones that run along a board,
    /// and of its ray flecks.
    pub light: [f32; 3],
    /// Ring spacing, mm.
    pub ring_mm: f32,
    /// How strongly latewood shows (0: no rings, 1: full latewood colour).
    pub contrast: f32,
    /// How far the rings wander (figure), mm.
    pub figure_mm: f32,
    /// Pore density, 0..1 (oak and ash: high; maple and birch: low).
    pub pores: f32,
    /// How much streak colour shows, 0..1.
    pub streaks: f32,
    /// Roughness of earlywood and of latewood.
    pub roughness: [f32; 2],
    /// Noise seed: two boards of one species differ by their seed.
    pub seed: u32,
    /// Glued-up stock, in grain mm: the boards' width and length (0: one piece; length 0: full
    /// length), then where the pith lies from the body's middle across its width (the boards
    /// are laid out from the body's middle, not from the pith).
    pub stock: [f32; 3],
    /// Plywood, in grain mm: the thickness of one ply (0: solid wood), where the sheet's bottom
    /// face lies through the thickness, and how many plies the sheet has. A species names only
    /// its plies' usual thickness; [`fit_plies`] fits them to a body.
    pub ply: [f32; 3],
    /// How uneven the growth is, 0…2 (0: even, round, parallel rings; 1: as a real log): wide
    /// and narrow years, good and lean decades, a log that is not round and tapers, and more
    /// or less latewood from ring to ring.
    pub variation: f32,
    /// How many knots, 0…1 (0: clear timber): branches that grow out from the pith, the rings
    /// bending around them.
    pub knots: f32,
    /// Burl, 0…1 (0: none): the grain swirls in every direction around small eyes.
    pub burl: f32,
    /// How this species knots, 0…1 (only where `knots` asks for any): toward 1 its branches are
    /// smaller and it carries more pin knots, small dark dots in loose groups (cherry, birch,
    /// maple); toward 0 it has few, larger knots (walnut, teak).
    pub pins: f32,
}

/// A species: a stable id (the starter preset id without `starter.wood.`) and its look.
pub struct Species {
    pub id: &'static str,
    pub look: WoodLook,
}

const fn rgb(r: u8, g: u8, b: u8) -> [f32; 3] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
}

#[allow(clippy::too_many_arguments)]
const fn species(
    id: &'static str,
    early: [f32; 3],
    late: [f32; 3],
    streak: [f32; 3],
    light: [f32; 3],
    ring_mm: f32,
    contrast: f32,
    figure_mm: f32,
    pores: f32,
    streaks: f32,
    roughness: [f32; 2],
) -> Species {
    Species {
        id,
        look: WoodLook {
            early,
            late,
            streak,
            light,
            ring_mm,
            contrast,
            figure_mm,
            pores,
            streaks,
            roughness,
            seed: 0,
            stock: [0.0; 3],
            ply: [0.0; 3],
            variation: 1.0,
            knots: 0.0,
            burl: 0.0,
            pins: 0.3,
        },
    }
}

/// A species with its own way of knotting ([`WoodLook::pins`]).
const fn pins(s: Species, pins: f32) -> Species {
    Species { id: s.id, look: WoodLook { pins, ..s.look } }
}

/// A species as raw, unfinished timber: greyer, paler, flatter and matt (what an oil or a
/// lacquer then deepens into the finished look).
const fn raw(id: &'static str, finished: &Species) -> Species {
    const fn dull(c: [f32; 3]) -> [f32; 3] {
        const fn f(v: f32, grey: f32) -> f32 {
            (v + (grey - v) * 0.25) * 0.78 + 0.2
        }
        let grey = 0.3 * c[0] + 0.55 * c[1] + 0.15 * c[2];
        [f(c[0], grey), f(c[1], grey), f(c[2], grey)]
    }
    let l = finished.look;
    Species {
        id,
        look: WoodLook {
            early: dull(l.early),
            late: dull(l.late),
            streak: dull(l.streak),
            light: dull(l.light),
            contrast: l.contrast * 0.8,
            streaks: l.streaks * 0.8,
            roughness: [0.80, 0.88],
            ..l
        },
    }
}

/// A species as plywood: thin plies of it glued in a stack, each turned a quarter turn from the
/// one below, `ply_mm` thick.
const fn plywood(id: &'static str, veneer: &Species, ply_mm: f32) -> Species {
    Species { id, look: WoodLook { ply: [ply_mm, 0.0, 0.0], ..veneer.look } }
}

const OAK_HONEY: Species =
    pins(species("oak-honey", rgb(196, 148, 88), rgb(144, 96, 50), rgb(124, 84, 46), rgb(222, 180, 118), 3.0, 0.7, 2.5, 0.8, 0.3, [0.34, 0.44]), 0.5);
const OAK_SMOKED: Species =
    pins(species("oak-smoked", rgb(106, 84, 64), rgb(60, 44, 32), rgb(46, 34, 26), rgb(132, 106, 82), 3.0, 0.7, 2.5, 0.8, 0.3, [0.36, 0.46]), 0.5);
// Walnut with a clear finish: warm chocolate heartwood, amber zones, near-black streaks, open
// pores (no sapwood band).
const WALNUT: Species =
    pins(species("walnut-dark", rgb(112, 70, 44), rgb(70, 43, 29), rgb(56, 36, 27), rgb(142, 92, 56), 4.0, 0.55, 4.5, 0.6, 0.45, [0.30, 0.38]), 0.15);
// Raw walnut is its own colours: a cool grey-brown with a faint purple cast.
const WALNUT_RAW: Species = pins(
    species("walnut-dark-raw", rgb(146, 121, 104), rgb(112, 90, 77), rgb(92, 74, 68), rgb(170, 146, 122), 4.0, 0.4, 4.5, 0.6, 0.35, [0.80, 0.88]),
    0.15,
);
const MAPLE: Species = pins(
    species("maple-light", rgb(228, 200, 150), rgb(202, 166, 112), rgb(186, 148, 100), rgb(242, 220, 176), 3.5, 0.3, 2.0, 0.1, 0.2, [0.30, 0.38]),
    0.6,
);
const CHERRY: Species = pins(
    species("cherry-warm", rgb(176, 104, 66), rgb(136, 74, 44), rgb(110, 58, 36), rgb(202, 134, 88), 3.0, 0.4, 2.5, 0.2, 0.35, [0.30, 0.38]),
    0.7,
);
const ASH: Species = pins(
    species("ash-pale", rgb(218, 194, 152), rgb(160, 124, 80), rgb(146, 114, 76), rgb(236, 216, 178), 3.5, 0.75, 2.5, 0.75, 0.2, [0.36, 0.46]),
    0.2,
);
const BIRCH: Species = pins(
    species("birch-cream", rgb(230, 208, 164), rgb(208, 178, 130), rgb(192, 162, 116), rgb(244, 228, 190), 3.0, 0.3, 2.0, 0.1, 0.2, [0.32, 0.40]),
    0.7,
);
// Birch plywood of the all-birch, void-free kind: 1.4 mm plies (13 in an 18 mm sheet).
const BIRCH_PLY: Species = plywood("birch-plywood", &BIRCH, 1.4);
const BIRCH_PLY_RAW: Species = plywood("birch-plywood-raw", &raw("birch-cream-raw", &BIRCH), 1.4);
const TEAK: Species = pins(
    species("teak-golden", rgb(174, 124, 64), rgb(124, 84, 40), rgb(88, 60, 32), rgb(204, 158, 90), 4.5, 0.5, 3.0, 0.45, 0.5, [0.34, 0.42]),
    0.1,
);

/// The starter species, in the starter pack's order: each with a clear finish (`oak-honey`) and
/// as raw timber (`oak-honey-raw`).
pub const SPECIES: &[Species] = &[
    OAK_HONEY,
    raw("oak-honey-raw", &OAK_HONEY),
    OAK_SMOKED,
    raw("oak-smoked-raw", &OAK_SMOKED),
    WALNUT,
    WALNUT_RAW,
    MAPLE,
    raw("maple-light-raw", &MAPLE),
    CHERRY,
    raw("cherry-warm-raw", &CHERRY),
    ASH,
    raw("ash-pale-raw", &ASH),
    BIRCH,
    raw("birch-cream-raw", &BIRCH),
    BIRCH_PLY,
    BIRCH_PLY_RAW,
    TEAK,
    raw("teak-golden-raw", &TEAK),
];

/// A species by id (`walnut-dark`) or starter preset id (`starter.wood.walnut-dark`).
pub fn species_look(id: &str) -> Option<WoodLook> {
    let id = id.strip_prefix("starter.wood.").unwrap_or(id);
    SPECIES.iter().find(|s| s.id.eq_ignore_ascii_case(id)).map(|s| s.look)
}

/// A shaded wood sample: sRGB colour (0..1) and roughness.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WoodSample {
    pub srgb: [f32; 3],
    pub roughness: f32,
}

/// A body's wood: its look, and the affine map from the coordinates a renderer draws it in
/// (world, mm) to grain space (rows: grain = row · (p, 1)).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WoodShade {
    pub look: WoodLook,
    pub to_grain: [[f32; 4]; 3],
}

impl WoodShade {
    /// The point `p` (world, mm) in grain space.
    pub fn grain_point(&self, p: Vec3) -> [f32; 3] {
        let [x, y, z] = [p.x as f32, p.y as f32, p.z as f32];
        self.to_grain.map(|r| r[0] * x + r[1] * y + r[2] * z + r[3])
    }

    /// The wood's colour and roughness at `p` (world, mm).
    pub fn sample(&self, p: Vec3) -> WoodSample {
        sample(&self.look, self.grain_point(p), [0.0; 3], [0.0; 3])
    }

    /// A world step (mm) in grain space.
    pub fn grain_step(&self, v: Vec3) -> [f32; 3] {
        let [x, y, z] = [v.x as f32, v.y as f32, v.z as f32];
        self.to_grain.map(|r| r[0] * x + r[1] * y + r[2] * z)
    }

    /// The same, as a pixel sees it whose steps across the screen are `dx` and `dy` on the
    /// surface (world, mm): detail finer than the pixel fades to its average, so a far or a
    /// steep view is calm and does not shimmer.
    pub fn sample_seen(&self, p: Vec3, dx: Vec3, dy: Vec3) -> WoodSample {
        sample(&self.look, self.grain_point(p), self.grain_step(dx), self.grain_step(dy))
    }

    /// The same shade seen through a rigid or affine move `m` (rows, world ← local): for a
    /// renderer that draws the body in its local coordinates.
    pub fn through(&self, m: [[f64; 4]; 3]) -> WoodShade {
        let g = self.to_grain.map(|r| r.map(f64::from));
        let mut out = [[0f32; 4]; 3];
        for (i, row) in g.iter().enumerate() {
            for j in 0..4 {
                let mut v = (0..3).map(|k| row[k] * m[k][j]).sum::<f64>();
                if j == 3 {
                    v += row[3];
                }
                out[i][j] = v as f32;
            }
        }
        WoodShade { look: self.look, to_grain: out }
    }

    /// The shade as the GPU reads it: 11 `vec4`s (see `wood.wgsl` in the GPU viewport crate).
    pub fn texels(&self) -> [[f32; 4]; TEXELS] {
        let l = &self.look;
        let [r0, r1, r2] = self.to_grain;
        [
            [l.early[0], l.early[1], l.early[2], l.ring_mm],
            [l.late[0], l.late[1], l.late[2], l.contrast],
            [l.streak[0], l.streak[1], l.streak[2], l.figure_mm],
            [l.pores, l.streaks, l.roughness[0], l.roughness[1]],
            // The seed as an exact f32 (below 2^24).
            [(l.seed & 0x00ff_ffff) as f32, l.light[0], l.light[1], l.light[2]],
            r0,
            r1,
            r2,
            [l.stock[0], l.stock[1], l.stock[2], 0.0],
            // Kept for the rings' variation, knots and burl.
            [l.variation, l.knots, l.burl, l.pins],
            [l.ply[0], l.ply[1], l.ply[2], 0.0],
        ]
    }
}

/// `vec4`s per wood slot on the GPU.
pub const TEXELS: usize = 11;

/// Wood's id among the viewport's procedural materials: the first value of its record, by
/// which the shader picks the procedure (`procedural` in viewport.wgsl).
pub const PROCEDURE: f32 = 1.0;

// Integer hash → [0, 1). Kept bit for bit equal to the WGSL `wood_hash`.
fn hash(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ (z as u32).wrapping_mul(0xcb1a_b31f) ^ seed;
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h >> 8) as f32 / 16_777_216.0
}

fn mixf(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// Trilinear value noise in [0, 1), smooth (C1) between lattice points.
fn noise(p: [f32; 3], seed: u32) -> f32 {
    let i = p.map(f32::floor);
    let f = [p[0] - i[0], p[1] - i[1], p[2] - i[2]];
    let u = f.map(|v| v * v * (3.0 - 2.0 * v));
    let [x, y, z] = i.map(|v| v as i32);
    let h = |dx: i32, dy: i32, dz: i32| hash(x.wrapping_add(dx), y.wrapping_add(dy), z.wrapping_add(dz), seed);
    let x00 = mixf(h(0, 0, 0), h(1, 0, 0), u[0]);
    let x10 = mixf(h(0, 1, 0), h(1, 1, 0), u[0]);
    let x01 = mixf(h(0, 0, 1), h(1, 0, 1), u[0]);
    let x11 = mixf(h(0, 1, 1), h(1, 1, 1), u[0]);
    mixf(mixf(x00, x10, u[1]), mixf(x01, x11, u[1]), u[2])
}

fn scaled(g: [f32; 3], along: f32, across: f32) -> [f32; 3] {
    [g[0] * along, g[1] * across, g[2] * across]
}

// What end grain keeps of a colour: it drinks a finish and goes darker and warmer.
const END_GRAIN: [f32; 3] = [0.80, 0.72, 0.62];

/// Plywood's plies fitted to a body drawn as `positions` (world, mm) with the grain map
/// `to_grain`: an odd number of plies about `ply_mm` thick (real mm; the pattern's `scale`
/// stretches the grain, not the plies) that fill the body's thickness exactly, so both of its
/// faces are whole face veneers with the grain running the same way. For [`WoodLook::ply`].
pub fn fit_plies(positions: &[Vec3], to_grain: &[[f32; 4]; 3], ply_mm: f64, scale: f64) -> [f32; 3] {
    let r = to_grain[2].map(f64::from);
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in positions {
        let z = r[0] * p.x + r[1] * p.y + r[2] * p.z + r[3];
        low = low.min(z);
        high = high.max(z);
    }
    let scale = scale.clamp(0.01, 100.0);
    let ply_mm = if ply_mm.is_finite() { ply_mm.clamp(0.05, 1000.0) } else { 1.0 };
    let thick = high - low;
    if !(thick.is_finite() && thick > 1e-9) {
        return [(ply_mm / scale) as f32, 0.0, 1.0];
    }
    // The nearest odd count.
    let want = (thick * scale / ply_mm).clamp(1.0, 99_999.0);
    let count = 2.0 * ((want - 1.0) / 2.0).round() + 1.0;
    [(thick / count) as f32, low as f32, count as f32]
}

/// The wood function, at point `g` in grain space (mm), seen by a pixel whose steps across the
/// screen are `fx` and `fy` in grain space (zero: a point). Detail finer than the pixel fades
/// to its average, each kind by the footprint in its own direction: rings by how many of them
/// a pixel crosses, fibres and pores by the pixel's width across the grain. So a far or a
/// steep view is calm, and a steep view along the grain keeps its lines. Mirrors the WGSL
/// `wood_sample`.
pub fn sample(l: &WoodLook, g: [f32; 3], fx: [f32; 3], fy: [f32; 3]) -> WoodSample {
    let s = l.seed;
    let (mut fx, mut fy) = (fx, fy);
    // The pixel's width across the grain and its length along it, mm.
    let mut across = (fx[1] * fx[1] + fx[2] * fx[2]).sqrt().max((fy[1] * fy[1] + fy[2] * fy[2]).sqrt());
    let mut along = fx[0].abs().max(fy[0].abs());
    // Glued-up stock: the body is made of boards `stock[0]` wide (and `stock[1]` long, 0: full
    // length), each cut from its own place in a log, with a glue line between them.
    let mut g = g;
    let mut seam = 0.0;
    let mut tone = 1.0;
    // How this piece of timber grew (one number per board): fast or slow.
    let mut grew = hash(0, 0, 9, s);
    let mut end = 0.0;
    if l.ply[0] > 0.0 {
        // Plywood: a stack of plies through the thickness, counted from the sheet's bottom
        // face. Each is a veneer peeled around its log (the pith far below it, so its face
        // shows broad, wandering rings), and every second one is turned a quarter turn.
        let tp = l.ply[0];
        let top = (l.ply[2] - 1.0).max(0.0);
        let zb = g[2] - l.ply[1];
        let i = (zb / tp).floor().clamp(0.0, top);
        let ii = i as i32;
        let zl = zb - (i + 0.5) * tp;
        let turned = ii & 1 == 1;
        // A glue line between two plies (none on the sheet's two faces).
        let mut edge = 0.5 * tp - zl.abs();
        if (i <= 0.0 && zl < 0.0) || (i >= top && zl > 0.0) {
            edge = 1.0e9;
        }
        let fz = fx[2].abs().max(fy[2].abs());
        let line_mm = 0.1 + fz;
        seam = 0.5 * (1.0 - smoothstep(0.0, line_mm, edge)) * (0.1 / line_mm);
        // End grain is darker: how squarely the surface (its normal, from the pixel's two
        // steps) cuts across this ply's fibres. Without steps, the turned plies are the dark
        // ones.
        let n = [fx[1] * fy[2] - fx[2] * fy[1], fx[2] * fy[0] - fx[0] * fy[2], fx[0] * fy[1] - fx[1] * fy[0]];
        let nn = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
        let na = if turned { n[1] } else { n[0] };
        end = if turned { 0.6 } else { 0.0 };
        if nn > 1.0e-20 {
            end = na * na / nn;
        }
        // Plies finer than a pixel fade to their average.
        end = mixf(0.5, end, 1.0 - smoothstep(0.5, 1.5, fz / tp));
        let h1 = hash(ii, 1, 0, s ^ 0x7f4a_7c15);
        let h2 = hash(ii, 2, 0, s ^ 0x7f4a_7c15);
        let h3 = hash(ii, 3, 0, s ^ 0x7f4a_7c15);
        let h4 = hash(ii, 4, 0, s ^ 0x7f4a_7c15);
        tone = 0.95 + 0.1 * h4;
        let deep = zl + 2500.0 + 1500.0 * h3;
        if turned {
            g = [g[1] + 40000.0 * h1, g[0] + 400.0 * (h2 - 0.5), deep];
            fx = [fx[1], fx[0], fx[2]];
            fy = [fy[1], fy[0], fy[2]];
            across = (fx[1] * fx[1] + fx[2] * fx[2]).sqrt().max((fy[1] * fy[1] + fy[2] * fy[2]).sqrt());
            along = fx[0].abs().max(fy[0].abs());
        } else {
            g = [g[0] + 40000.0 * h1, g[1] + 400.0 * (h2 - 0.5), deep];
        }
    } else if l.stock[0] > 0.0 {
        let wide = l.stock[0];
        let yb = g[1] + l.stock[2];
        let j = (yb / wide + 0.5).floor();
        let ji = j as i32;
        let mut ii = 0;
        let mut butt = 1.0e9;
        if l.stock[1] > 0.0 {
            let xs = g[0] + hash(ji, 0, 0, s ^ 0x2545_f491) * l.stock[1];
            let i = (xs / l.stock[1]).floor();
            let u = xs - i * l.stock[1];
            butt = u.min(l.stock[1] - u);
            ii = i as i32;
        }
        let h1 = hash(ji, ii, 1, s ^ 0x2545_f491);
        let h2 = hash(ji, ii, 2, s ^ 0x2545_f491);
        let h3 = hash(ji, ii, 3, s ^ 0x2545_f491);
        let h4 = hash(ji, ii, 4, s ^ 0x2545_f491);
        let yl = yb - j * wide;
        let edge = (0.5 * wide - yl.abs()).min(butt);
        let line_mm = 0.15 + across.max(along);
        seam = 0.45 * (1.0 - smoothstep(0.0, line_mm, edge)) * (0.15 / line_mm);
        tone = 0.93 + 0.14 * h4;
        grew = hash(ji, ii, 5, s ^ 0x2545_f491);
        let side = if h3 < 0.5 { -1.0 } else { 1.0 };
        g = [g[0] + 40000.0 * h3, yl - l.stock[2] - (h1 - 0.5) * wide * 1.2, g[2] - side * (15.0 + 120.0 * h2)];
    }
    let vary = l.variation.clamp(0.0, 2.0);
    let ring_mm = l.ring_mm * mixf(1.0, 0.7 + 0.7 * grew, vary.min(1.0));
    // Knots: every 300 mm of the log may carry a branch, growing out from the pith in its own
    // direction and leaning up the trunk. A tree lays each year's wood as one skin over trunk
    // and branch alike, so a branch is not drawn over the trunk's rings: it is a second, thin
    // cone of growth, and further down the two are joined into one (`bt`: when the branch's
    // wood here grew, as mm of trunk radius; `bw`: how much of a branch there is here;
    // `bthick`: how fast it thickens against the trunk; `bdark`: how much darker its wood is). `lift` (mm) raises the rings around
    // pin knots and burl eyes, `knot` is their darker wood, `kd` and `kring` their own rings.
    let mut lift = 0.0;
    let mut knot = 0.0;
    let mut kd = 0.0;
    let mut kring = 0.0;
    let mut halo: f32 = 0.0;
    let mut bdark = 0.0;
    let mut bw = 0.0;
    let mut bt = 0.0;
    let mut bthick = 1.0;
    let mut bd = 0.0;
    if l.knots > 0.0 {
        let cell = (g[0] / 300.0).floor();
        let ci = cell as i32;
        if hash(ci, 1, 11, s ^ 0x7f4a_7c15) < l.knots {
            let h1 = hash(ci, 2, 11, s ^ 0x7f4a_7c15);
            let h2 = hash(ci, 3, 11, s ^ 0x7f4a_7c15);
            let h3 = hash(ci, 4, 11, s ^ 0x7f4a_7c15);
            let h4 = hash(ci, 5, 11, s ^ 0x7f4a_7c15);
            let h5 = hash(ci, 6, 11, s ^ 0x7f4a_7c15);
            // The branch's direction across the grain (a unit vector).
            let ay = 2.0 * h2 - 1.0;
            let az = if h3 < 0.5 { -1.0 } else { 1.0 } * (1.0 - ay * ay).max(0.0).sqrt();
            let out = g[1] * ay + g[2] * az;
            let side = g[2] * ay - g[1] * az;
            // It leans up the trunk (never out of its 300 mm), so a face cuts it at a slant.
            let lean = 0.25 + 0.4 * h5;
            let reach = out.max(0.0);
            let u = g[0] - (cell + 0.2 + 0.25 * h1) * 300.0 - lean * 160.0 * reach / (reach + 160.0);
            // Only where the branch is, and nothing of it reaches the next 300 mm.
            bw = smoothstep(0.0, 8.0, out) * (1.0 - smoothstep(35.0, 60.0, u.abs()));
            // Across the leaning branch; its wood is laid a little longer along the trunk's
            // grain than across it, and never quite round.
            let d0 = (u * u / (1.0 + lean * lean) + side * side).sqrt();
            let wob = noise([1.6 * u / (d0 + 1.0e-3) + 40.0 * h1, 1.6 * side / (d0 + 1.0e-3) + 40.0 * h2, out * 0.03], s ^ 0x4cf5_ad43) - 0.5;
            let d = (u * u * 0.2 / (1.0 + lean * lean) + side * side).sqrt() * (1.0 + 0.3 * wob);
            bthick = (0.08 + 0.14 * h4) * (1.0 - 0.45 * l.pins);
            // The branch's tip passed here early (it grows in length far faster than the trunk
            // in girth), and it has thickened since.
            // (and ever more slowly: an old branch stays slim.)
            let slow = d * d * d * d / 10000.0;
            bt = reach / 6.0 + d * (1.0 + slow) / bthick;
            bd = d;
            // The trunk makes way for the branch from far off: its rings rise toward it in a
            // long, pointed lens (longest down the trunk), and its fibres part around it.
            let rk = (0.8 * bthick * reach).min(14.0) + 2.0;
            let tail = if u < 0.0 { 26.0 } else { 9.0 };
            let lens = (side * side / (rk * rk) + 0.1).sqrt() + u * u / (tail * rk * rk);
            // (Fading gently over the whole of the branch's 120 mm of trunk.)
            let fade = (1.0 - u * u / 3600.0).max(0.0);
            let near = smoothstep(0.0, 8.0, out) * fade * fade;
            lift = near * 1.2 * rk / (1.0 + 0.5 * lens * lens);
            let wide = 1.5 * rk;
            let part = -near * side * wide * wide / (u * u * 0.16 + side * side + wide * wide);
            g = [g[0], g[1] - part * az, g[2] + part * ay];
            // Most knots are only a shade darker than the trunk, a few much darker.
            bdark = 0.15 + 0.6 * h3 * h3;
        }
    }
    // Pin knots: small shoots that start somewhere out in the trunk, in loose groups.
    if l.knots > 0.0 && l.pins > 0.0 {
        let cell = (g[0] / 80.0).floor();
        let ci = cell as i32;
        if hash(ci, 1, 13, s ^ 0x7f4a_7c15) < l.knots * l.pins {
            let h1 = hash(ci, 2, 13, s ^ 0x7f4a_7c15);
            let h2 = hash(ci, 3, 13, s ^ 0x7f4a_7c15);
            let h3 = hash(ci, 4, 13, s ^ 0x7f4a_7c15);
            let h4 = hash(ci, 5, 13, s ^ 0x7f4a_7c15);
            let ay = 2.0 * h2 - 1.0;
            let az = if h3 < 0.5 { -1.0 } else { 1.0 } * (1.0 - ay * ay).max(0.0).sqrt();
            let out = g[1] * ay + g[2] * az;
            let side = g[2] * ay - g[1] * az;
            let u = g[0] - (cell + 0.35 + 0.3 * h1) * 80.0;
            let rk = 1.0 + 2.2 * h4;
            let start = 150.0 * hash(ci, 6, 13, s ^ 0x7f4a_7c15);
            let there = smoothstep(start, start + 10.0, out) * (1.0 - smoothstep(12.0, 24.0, u.abs()));
            let pd = (u * u + side * side).sqrt() / rk;
            let lens = (side * side / (rk * rk) + 0.1).sqrt() + u * u / (12.0 * rk * rk);
            lift += there * 1.5 * rk / (1.0 + 0.8 * lens * lens);
            halo = halo.max(there / (1.0 + 0.3 * pd * pd * pd * pd));
            let pin = there * 0.55 * (1.0 - smoothstep(0.5, 1.1, pd));
            if pin > knot {
                knot = pin;
                kd = pd;
                kring = pd * 2.0;
            }
        }
    }
    // Burl: the grain swirls the same in every direction (two sizes of swirl), around eyes:
    // small dormant buds, each with its own tight rings.
    if l.burl > 0.0 {
        let b1 = [g[0] / 34.0, g[1] / 34.0, g[2] / 34.0];
        let b2 = [g[0] / 12.0, g[1] / 12.0, g[2] / 12.0];
        let sy = 22.0 * (noise(b1, s ^ 0x3243_f6a8) - 0.5) + 7.0 * (noise(b2, s ^ 0x85a3_08d3) - 0.5);
        let sz = 22.0 * (noise([b1[0] + 19.0, b1[1] + 7.0, b1[2] + 43.0], s ^ 0x1319_8a2e) - 0.5)
            + 7.0 * (noise([b2[0] + 13.0, b2[1] + 37.0, b2[2] + 5.0], s ^ 0x0370_7344) - 0.5);
        g = [g[0], g[1] + l.burl * sy, g[2] + l.burl * sz];
        let e = [(g[0] / 30.0).floor(), (g[1] / 30.0).floor(), (g[2] / 30.0).floor()];
        let [ex, ey, ez] = e.map(|v| v as i32);
        if hash(ex, ey, ez, s ^ 0xa409_3822) < 0.35 + 0.6 * l.burl {
            let c = [
                0.35 + 0.3 * hash(ex, ey, ez, s ^ 0x299f_31d0),
                0.35 + 0.3 * hash(ex, ey, ez, s ^ 0x082e_fa98),
                0.35 + 0.3 * hash(ex, ey, ez, s ^ 0xec4e_6c89),
            ];
            let re = 1.2 + 2.2 * hash(ex, ey, ez, s ^ 0x4528_21e6);
            let d = [g[0] - (e[0] + c[0]) * 30.0, g[1] - (e[1] + c[1]) * 30.0, g[2] - (e[2] + c[2]) * 30.0];
            let ed = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() / re;
            let eye = l.burl.min(1.0) * (1.0 - smoothstep(0.0, 3.0, ed));
            lift += 3.0 * re * eye;
            knot = knot.max(l.burl.min(1.0) * (1.0 - smoothstep(0.5, 1.0, ed)));
            if ed < 1.0 {
                kd = ed;
                kring = ed * 4.0;
            }
        }
    }
    // Figure: the rings wander, slowly along the grain and a little faster across it.
    let w = scaled(g, 0.004, 0.02);
    let dy = noise(w, s) - 0.5;
    let dz = noise([w[0] + 17.0, w[1] + 31.0, w[2] + 47.0], s ^ 0x9e37_79b9) - 0.5;
    // And they sweep in long, slow waves: the flowing cathedral figure of a flat-sawn board.
    let w2 = scaled(g, 0.0012, 0.006);
    let ey = noise([w2[0] + 5.0, w2[1] + 11.0, w2[2] + 23.0], s ^ 0x510e_527f) - 0.5;
    let ez = noise([w2[0] + 41.0, w2[1] + 3.0, w2[2] + 29.0], s ^ 0x9b05_688c) - 0.5;
    let y = g[1] + l.figure_mm * (2.0 * dy + 9.0 * ey);
    let z = g[2] + l.figure_mm * (2.0 * dz + 9.0 * ez);
    let r = (y * y + z * z).sqrt();
    // A log is not round: it grows faster on some sides than on others (lobes around the pith
    // that drift along it), and it tapers, so a ring lies closer to the pith farther up.
    let uy = y / (r + 1.0e-3);
    let uz = z / (r + 1.0e-3);
    let lobe = noise([g[0] * 0.0015 + 3.0, 1.3 * uy + 7.0, 1.3 * uz + 13.0], s ^ 0x243f_6a88) - 0.5;
    let lobe2 = noise([g[0] * 0.004 + 11.0, 3.1 * uy + 5.0, 3.1 * uz + 29.0], s ^ 0xb7e1_5162) - 0.5;
    let trunk = r * (1.0 + vary * (0.22 * lobe + 0.07 * lobe2 * smoothstep(5.0, 60.0, r))) + vary * 0.006 * g[0] - lift;
    // Trunk and branch are one growth: whichever laid its wood here first, joined smoothly
    // where they meet (the collar), so a ring of the trunk turns and runs up the branch.
    let mut rr = trunk;
    let mut branch = 0.0;
    if bw > 0.0 {
        let w = 0.8 * trunk.max(0.0) + 2.0;
        // The branch stops thickening in time, and it ends before the next 300 mm: there
        // its wood comes so late that the trunk's is always first. So the two stay one
        // growth everywhere, and no ring of one ever crosses a ring of the other.
        let late = bt + 8.0 * w * (1.0 - bw) * (1.0 - bw) + 2.0 * (trunk.max(0.0) + w) * smoothstep(22.0, 34.0, bd);
        // (Late enough is late enough: huge numbers would only lose the trunk's digits.)
        let late = late.min(trunk + w);
        let h = (0.5 + 0.5 * (late - trunk) / w).clamp(0.0, 1.0);
        rr = mixf(late, trunk, h) - w * h * (1.0 - h);
        branch = 1.0 - h;
    }
    // The branch's wood is its own, darker shade: no outline, only its tighter rings.
    let knot = knot.max(bdark * branch);
    // Rings a pixel crosses: its steps, taken outward from the pith.
    let fr = ((y * fx[1] + z * fx[2]).abs() + (y * fy[1] + z * fy[2]).abs()) / (r.max(1.0e-3) * ring_mm);
    // (A branch's rings are finer.)
    let fr = fr.max(branch * across.max(along) / (bthick * ring_mm));
    // (And fainter.)
    let rings = (1.0 - smoothstep(0.35, 1.0, fr)) * (1.0 - 0.55 * branch);
    let thin = (1.0 - smoothstep(0.08, 0.35, fr)) * (1.0 - 0.6 * branch);
    // The years, counted out from the pith: good and lean decades and single odd years (the
    // rings bunch up and spread out, differently on each side of the log), and small waves.
    let ring0 = rr / ring_mm;
    let decade = noise([ring0 * 0.11 + 3.0, g[0] * 0.0008, 2.0 * uy + 2.0 * uz], s ^ 0x13c6_ef37) - 0.5;
    let year = noise([ring0 * 0.37 + 9.0, g[0] * 0.0015, 3.0 * uy - 3.0 * uz], s ^ 0x5be0_cd19) - 0.5;
    let ring = ring0
        + vary * (3.6 * decade + 0.7 * year)
        + 0.6 * (noise(scaled([g[0], y, z], 0.01, 0.04), s ^ 0x85eb_ca6b) - 0.5)
        + 0.25 * vary * (noise(scaled([g[0], y, z], 0.03, 0.12), s ^ 0x1b87_3593) - 0.5);
    // Each year ends where it happens to: its edge lies up to 0.3 of a ring early or late, so
    // no two neighbours are as wide.
    let jitter = 0.6 * vary.min(1.5);
    let n0 = ring.floor();
    let ni = n0 as i32;
    let edge = |k: i32| (n0 + k as f32) + jitter * (hash(ni.wrapping_add(k), 0, 7, s ^ 0x6c62_272e) - 0.5);
    let e0 = edge(0);
    let e1 = edge(1);
    let (lo, hi, nr) = if ring < e0 {
        (edge(-1), e0, ni.wrapping_sub(1))
    } else if ring >= e1 {
        (e1, edge(2), ni.wrapping_add(1))
    } else {
        (e0, e1, ni)
    };
    let t = ((ring - lo) / (hi - lo)).clamp(0.0, 1.0);
    // And each year has its own share of latewood, and its own depth of colour.
    let share = mixf(0.55, 0.3 + 0.5 * hash(nr, 1, 7, s ^ 0x6c62_272e), vary.min(1.0));
    let depth = mixf(1.0, 0.5 + 0.7 * hash(nr, 2, 7, s ^ 0x6c62_272e), vary.min(1.0));
    // Earlywood grades into latewood across the ring, which ends at the next ring: sharply
    // close up, over a pixel's width from afar (a hard edge finer than a pixel would shimmer).
    let wrap = 1.0 - smoothstep(1.0 - fr.clamp(0.02, 0.5), 1.0, t);
    let late = mixf(0.25, (smoothstep(share, 0.95, t) * wrap * depth).min(1.0), rings) * l.contrast;
    let q = [g[0], y, z];
    // Colour zones: broad bands of the lighter, warmer tone that run the length of a board.
    // The same point by its ring: along the grain, out from the pith, and around it. Colour
    // belongs to the rings, so its zones and streaks bend with them (the cathedral arches).
    let around = 20.0 * y / (r + 1.0);
    let zone = smoothstep(0.35, 0.8, noise([g[0] * 0.0015, r * 0.05, around * 0.05], s ^ 0x1656_67b1));
    // Dark streaks: long along the grain, a few mm across.
    let streak = smoothstep(0.6, 0.92, noise([g[0] * 0.004, r * 0.25, around * 0.03], s ^ 0xc2b2_ae35)) * l.streaks;
    // How much of each size of detail a pixel this wide still resolves.
    let fine = 1.0 - smoothstep(0.08, 0.25, across);
    let coarse = 1.0 - smoothstep(0.25, 0.8, across);
    let mid = 1.0 - smoothstep(0.8, 2.5, across);
    // Pores: the cut vessels, short dark dashes along the grain (dots on end grain), mostly in
    // the earlywood.
    let pore = smoothstep(1.0 - 0.3 * l.pores, 1.0, noise(scaled(q, 0.1, 1.8), s ^ 0x27d4_eb2f))
        * l.pores.min(1.0)
        * (1.0 - 0.5 * late)
        * coarse
        * (1.0 - smoothstep(3.0, 10.0, along));
    // Ray flecks: short pale ticks.
    let fleck =
        smoothstep(0.8, 0.97, noise(scaled(q, 0.3, 1.6), s ^ 0x6a09_e667)) * (0.25 + 0.5 * l.pores) * coarse * (1.0 - smoothstep(1.0, 3.3, along));
    // Fibres along the grain: coarse, fine, and broad soft ones that still show from afar.
    let fibre = mixf(1.0, 0.92 + 0.16 * noise(scaled(q, 0.03, 1.2), s ^ 0x3c6e_f372), coarse)
        * mixf(1.0, 0.94 + 0.12 * noise(scaled(q, 0.05, 4.0), s ^ 0xbb67_ae85), fine)
        * mixf(1.0, 0.95 + 0.1 * noise(scaled(q, 0.012, 0.45), s ^ 0xa54f_f53a), mid);
    // The crisp dark line that ends each year's growth.
    let line = smoothstep(0.88, 0.97, t) * wrap * thin * l.contrast * depth.min(1.0);
    // Ripple: faint bands across the grain, where the fibres dip and rise.
    let ripple = mixf(1.0, 0.95 + 0.1 * noise(scaled(g, 0.06, 0.012), s ^ 0x1f83_d9ab), 1.0 - smoothstep(5.0, 16.0, along));
    let knot_ring = smoothstep(0.5, 1.0, (kring - kring.floor() - 0.5).abs() * 2.0).max(smoothstep(0.7, 1.0, kd));
    let mut c = [0f32; 3];
    for (k, ck) in c.iter_mut().enumerate() {
        let base = mixf(mixf(l.early[k], l.light[k], zone), l.late[k], late);
        let streaked = mixf(base, l.streak[k], streak);
        let flecked = mixf(streaked, l.light[k], 0.35 * fleck);
        // A knot is dense, dark wood with rings of its own, darkest at its rim and its heart,
        // and the wood packed around it is a little darker too.
        let flecked = mixf(flecked, l.late[k], 0.18 * halo);
        // (Darker than the timber even in a pale wood.)
        let heart = l.late[k] * mixf(l.streak[k], 1.0, 0.45) * (0.95 - 0.55 * l.late[1]);
        let flecked = mixf(flecked, heart, knot * (0.7 + 0.3 * knot_ring));
        *ck =
            (flecked * (1.0 - 0.7 * pore) * (1.0 - 0.2 * line) * fibre * ripple * tone * (1.0 - seam) * mixf(1.0, END_GRAIN[k], end)).clamp(0.0, 1.0);
    }
    let roughness = (mixf(l.roughness[0], l.roughness[1], late / l.contrast.max(1e-3)) + 0.08 * pore).clamp(0.02, 1.0);
    WoodSample { srgb: c, roughness }
}

/// How the grain axis is chosen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Axis {
    /// Along the body's longest direction (its principal axis), so boards run lengthwise and
    /// legs run along their length, even when tapered or splayed.
    Long,
    /// A fixed direction (world).
    Fixed([f64; 3]),
}

/// How a body's grain frame is set up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub axis: Axis,
    /// Scale of the whole pattern (1: real size).
    pub scale: f64,
    /// Where the pith (the log's centre) lies from the body's middle, mm: `[across its width,
    /// across its thickness]`. `[0, 0]` (the default) centres the rings on the board, so they
    /// grow out from its middle; moving it out places the board elsewhere in the log, toward
    /// its bark (cathedral arches on a flat-sawn face, straight lines once far away).
    pub pith_mm: [f64; 2],
    /// How far the grain is slid along its length, mm.
    pub shift_mm: f64,
    /// The grain turned from where the axis puts it, degrees: `[turn, tilt, roll]`. Turn swings
    /// the grain direction on the body's broad face (about its thickness), tilt leans it out of
    /// that face (about its width), and roll turns the rings about the grain itself. The pith
    /// offset and the shift are then measured along the turned directions.
    pub rotate_deg: [f64; 3],
}

impl Default for Frame {
    fn default() -> Self {
        Frame { axis: Axis::Long, scale: 1.0, pith_mm: [0.0, 0.0], shift_mm: 0.0, rotate_deg: [0.0; 3] }
    }
}

/// The grain map (world → grain space) of a body drawn as `triangles` over `positions` (world,
/// mm): the grain runs along `frame.axis` (or the body's principal axis), the pith lies
/// `frame.pith_mm` from the body's middle (across its width, across its thickness), and grain coordinates
/// along the axis count from the body's end, so a body that grows longer keeps the grain it had.
/// `None` for a body with no area.
pub fn frame_for_mesh(positions: &[Vec3], triangles: &[[u32; 3]], frame: Frame) -> Option<[[f32; 4]; 3]> {
    // Area-weighted centroid and covariance of the surface (triangle vertices as samples).
    let mut area = 0.0;
    let mut c = Vec3::ZERO;
    for t in triangles {
        let [a, b, d] = t.map(|k| positions.get(k as usize).copied());
        let (Some(a), Some(b), Some(d)) = (a, b, d) else { continue };
        let w = (b - a).cross(d - a).len() * 0.5;
        area += w;
        c += (a + b + d) * (w / 3.0);
    }
    if !(area > 1e-12) {
        return None;
    }
    c = c * (1.0 / area);
    let mut m = [[0f64; 3]; 3];
    for t in triangles {
        let [a, b, d] = t.map(|k| positions.get(k as usize).copied());
        let (Some(a), Some(b), Some(d)) = (a, b, d) else { continue };
        // The triangle's exact second moment, so the frame does not depend on how the
        // surface happens to be triangulated: A/12 (Σ vᵢvᵢᵀ + s sᵀ), s = Σ vᵢ.
        let w = (b - a).cross(d - a).len() * 0.5 / 12.0;
        let q = [a, b, d].map(|v| [v.x - c.x, v.y - c.y, v.z - c.z]);
        let s = [q[0][0] + q[1][0] + q[2][0], q[0][1] + q[1][1] + q[2][1], q[0][2] + q[1][2] + q[2][2]];
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += w * (q[0][i] * q[0][j] + q[1][i] * q[1][j] + q[2][i] * q[2][j] + s[i] * s[j]);
            }
        }
    }
    let (along, thin) = principal(m);
    let along = match frame.axis {
        Axis::Long => along,
        Axis::Fixed(v) => Vec3::new(v[0], v[1], v[2]).normalized().unwrap_or(along),
    };
    // A steady sign: the principal axis' sign is arbitrary, the grain's is not.
    let along = if along.x + along.y * 1e-3 + along.z * 1e-6 < 0.0 { -along } else { along };
    // The thinnest direction, made perpendicular to the grain.
    let mut thin = thin - along * thin.dot(along);
    if thin.len() < 1e-6 {
        thin = if along.x.abs() < 0.9 { Vec3::X.cross(along) } else { Vec3::Y.cross(along) };
    }
    let thin = thin.normalized().unwrap_or(Vec3::Z);
    let thin = if thin.x * 1e-6 + thin.y * 1e-3 + thin.z < 0.0 { -thin } else { thin };
    let wide = along.cross(thin);
    // Turned as asked: rolled about the grain, then tilted about the width, then swung about
    // the thickness (each about the body's own direction, so the three do not fight).
    let [turn, tilt, roll] = frame.rotate_deg.map(|d| if d.is_finite() { d.to_radians() } else { 0.0 });
    let spin = |v: Vec3, axis: Vec3, angle: f64| {
        let (s, c) = angle.sin_cos();
        v * c + axis.cross(v) * s + axis * (axis.dot(v) * (1.0 - c))
    };
    let turned = |v: Vec3| spin(spin(spin(v, along, roll), wide, tilt), thin, turn);
    let (along, wide, thin) = (turned(along), turned(wide), turned(thin));
    // From the body's start along the grain, so lengthening the far end keeps the pattern.
    let start = positions.iter().map(|p| p.dot(along)).fold(f64::INFINITY, f64::min);
    let pith = c + wide * frame.pith_mm[0] + thin * frame.pith_mm[1];
    let s = 1.0 / frame.scale.clamp(0.01, 100.0);
    let row = |axis: Vec3, offset: f64| [(axis.x * s) as f32, (axis.y * s) as f32, (axis.z * s) as f32, (-offset * s) as f32];
    Some([row(along, start + frame.shift_mm), row(wide, pith.dot(wide)), row(thin, pith.dot(thin))])
}

// The largest and the smallest eigenvector of a symmetric 3×3 matrix (power iteration, from
// fixed starts, so the same input always gives the same frame).
fn principal(m: [[f64; 3]; 3]) -> (Vec3, Vec3) {
    let apply = |m: &[[f64; 3]; 3], v: Vec3| {
        Vec3::new(
            m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
            m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
            m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
        )
    };
    let power = |m: &[[f64; 3]; 3]| {
        // Start from the matrix's largest column, so the start is never orthogonal to the answer.
        let cols = [Vec3::new(m[0][0], m[1][0], m[2][0]), Vec3::new(m[0][1], m[1][1], m[2][1]), Vec3::new(m[0][2], m[1][2], m[2][2])];
        let mut v = cols.iter().copied().fold(Vec3::new(1.0, 0.7, 0.3), |best, c| if c.len() > best.len() { c } else { best });
        v = v.normalized().unwrap_or(Vec3::X);
        for _ in 0..64 {
            let n = apply(m, v);
            match n.normalized() {
                Some(n) => v = n,
                None => break,
            }
        }
        v
    };
    let big = power(&m);
    // The smallest: the largest of (trace·I − m), which has the same eigenvectors, reversed.
    let tr = m[0][0] + m[1][1] + m[2][2];
    let mut flipped = m.map(|r| r.map(|x| -x));
    for (i, row) in flipped.iter_mut().enumerate() {
        row[i] += tr;
    }
    let small = power(&flipped);
    (big, small)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> (Vec<Vec3>, Vec<[u32; 3]>) {
        // A 600 × 100 × 20 box along X.
        let (x, y, z) = (600.0, 100.0, 20.0);
        let p: Vec<Vec3> =
            (0..8).map(|i| Vec3::new(if i & 1 == 0 { 0.0 } else { x }, if i & 2 == 0 { 0.0 } else { y }, if i & 4 == 0 { 0.0 } else { z })).collect();
        let t =
            vec![[0, 2, 1], [1, 2, 3], [4, 5, 6], [5, 7, 6], [0, 1, 4], [1, 5, 4], [2, 6, 3], [3, 6, 7], [0, 4, 2], [2, 4, 6], [1, 3, 5], [3, 7, 5]];
        (p, t)
    }

    #[test]
    fn a_board_runs_lengthwise_with_the_pith_in_its_middle_unless_moved() {
        let (p, t) = board();
        let m = frame_for_mesh(&p, &t, Frame::default()).unwrap();
        // Grain along X.
        assert!((m[0][0].abs() - 1.0).abs() < 1e-4, "{m:?}");
        // Thinnest direction Z: row 2 is ±Z.
        assert!((m[2][2].abs() - 1.0).abs() < 1e-4, "{m:?}");
        // The board's start is grain X 0.
        let w = WoodShade { look: species_look("walnut-dark").unwrap(), to_grain: m };
        let g = w.grain_point(Vec3::new(0.0, 50.0, 10.0));
        assert!(g[0].abs() < 1e-3, "{g:?}");
        assert!(g[1].abs() < 1e-3 && g[2].abs() < 1e-3, "{g:?}");
        // Moved 140 mm across the thickness, the board's middle is 140 mm from the pith.
        let m = frame_for_mesh(&p, &t, Frame { pith_mm: [0.0, 140.0], ..Frame::default() }).unwrap();
        let g = WoodShade { look: w.look, to_grain: m }.grain_point(Vec3::new(0.0, 50.0, 10.0));
        assert!(g[1].abs() < 1e-3 && (g[2].abs() - 140.0).abs() < 1e-2, "{g:?}");
    }

    #[test]
    fn a_longer_board_keeps_its_grain() {
        let (p, t) = board();
        let longer: Vec<Vec3> = p.iter().map(|v| if v.x > 1.0 { Vec3::new(800.0, v.y, v.z) } else { *v }).collect();
        let a = WoodShade { look: species_look("walnut-dark").unwrap(), to_grain: frame_for_mesh(&p, &t, Frame::default()).unwrap() };
        let b = WoodShade { look: a.look, to_grain: frame_for_mesh(&longer, &t, Frame::default()).unwrap() };
        for q in [Vec3::new(10.0, 3.0, 20.0), Vec3::new(300.0, 70.0, 20.0), Vec3::new(590.0, 99.0, 0.0)] {
            assert_eq!(a.sample(q).srgb.map(|v| (v * 255.0).round()), b.sample(q).srgb.map(|v| (v * 255.0).round()), "{q:?}");
        }
    }

    #[test]
    fn a_splayed_leg_runs_along_its_own_axis() {
        // A long thin box turned 7 degrees about X.
        let (p, t) = board();
        let (s, c) = 7f64.to_radians().sin_cos();
        // Long axis X → rotated toward Z.
        let turned: Vec<Vec3> = p.iter().map(|v| Vec3::new(v.x * c - v.z * s, v.y, v.x * s + v.z * c)).collect();
        let m = frame_for_mesh(&turned, &t, Frame::default()).unwrap();
        let axis = Vec3::new(m[0][0] as f64, m[0][1] as f64, m[0][2] as f64);
        assert!(axis.dot(Vec3::new(c, 0.0, s)).abs() > 0.9999, "{axis:?}");
    }

    #[test]
    fn samples_are_in_range_and_deterministic() {
        for sp in SPECIES {
            let mut l = sp.look;
            l.seed = 1234;
            for i in 0..500 {
                let g = [i as f32 * 3.7 - 900.0, (i * 7 % 113) as f32 - 50.0, 140.0 + (i % 19) as f32];
                if i % 2 == 0 {
                    l.stock = [80.0, (i % 3) as f32 * 400.0, 12.0];
                }
                if l.ply[0] > 0.0 {
                    l.ply = [1.4, 135.0, (i % 14) as f32];
                }
                let (fx, fy) = ([(i % 4) as f32, 0.2, 0.0], [0.0, 0.1, (i % 3) as f32 * 0.4]);
                let a = sample(&l, g, fx, fy);
                assert_eq!(a, sample(&l, g, fx, fy));
                assert!(a.srgb.iter().all(|v| (0.0..=1.0).contains(v)) && (0.02..=1.0).contains(&a.roughness), "{} {a:?}", sp.id);
            }
        }
    }

    /// A steep view along the grain (a pixel long along it, narrow across it) keeps its rings
    /// and fibres; a pixel wide across the grain fades them to their average.
    #[test]
    fn detail_fades_by_the_pixels_width_across_the_grain_only() {
        let mut l = species_look("walnut-dark").unwrap();
        l.seed = 99;
        let spread = |fx: [f32; 3], fy: [f32; 3]| {
            let v: Vec<f32> = (0..400).map(|i| sample(&l, [300.0, 30.0 + i as f32 * 0.137, 60.0], fx, fy).srgb[0]).collect();
            let mean = v.iter().sum::<f32>() / v.len() as f32;
            (v.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / v.len() as f32).sqrt()
        };
        let sharp = spread([0.0; 3], [0.0; 3]);
        let steep = spread([6.0, 0.0, 0.0], [0.0, 0.05, 0.0]);
        let far = spread([0.0, 6.0, 0.0], [0.0, 0.0, 6.0]);
        assert!(steep > 0.6 * sharp, "along the grain: {steep} of {sharp}");
        assert!(far < 0.75 * sharp && far < steep, "across the grain: {far} of {sharp} (along it {steep})");
    }

    /// Glued-up stock: each board has its own grain, with a darker glue line between two.
    #[test]
    fn glued_up_boards_differ_and_show_their_glue_lines() {
        let mut l = species_look("maple-light").unwrap();
        l.seed = 5;
        let plain = l;
        l.stock = [100.0, 0.0, 0.0];
        let at = |l: &WoodLook, y: f32| sample(l, [250.0, y, 8.0], [0.0; 3], [0.0; 3]).srgb;
        // Boards are laid out from the body's middle: one spans -50…50, the next 50…150.
        let (a, b): (Vec<_>, Vec<_>) = (0..40).map(|i| (at(&l, -40.0 + i as f32 * 2.0), at(&l, 60.0 + i as f32 * 2.0))).unzip();
        assert_ne!(a, b, "two boards, two grains");
        let lum = |c: [f32; 3]| c[0] + c[1] + c[2];
        let seam = lum(at(&l, 50.0));
        let beside = (lum(at(&l, 48.0)) + lum(at(&l, 52.0))) / 2.0;
        assert!(seam < 0.8 * beside, "the glue line is darker: {seam} beside {beside}");
        assert_ne!(at(&l, 10.0), at(&plain, 10.0));
    }

    /// Plywood: an odd stack of plies that fills the board, whole face veneers on both faces,
    /// glue lines between the plies on an edge, and every second ply's end grain darker there.
    #[test]
    fn plywood_fits_an_odd_stack_of_plies_and_shows_them_on_its_edges() {
        let (p, t) = board();
        let mut look = species_look("starter.wood.birch-plywood").unwrap();
        look.seed = 3;
        let to_grain = frame_for_mesh(&p, &t, Frame::default()).unwrap();
        for (thick, plies) in [(18.0, 13.0), (12.0, 9.0), (6.0, 5.0), (9.0, 7.0), (1.0, 1.0)] {
            let q: Vec<Vec3> = p.iter().map(|v| Vec3::new(v.x, v.y, v.z / 20.0 * thick)).collect();
            let m = frame_for_mesh(&q, &t, Frame::default()).unwrap();
            let fit = fit_plies(&q, &m, 1.4, 1.0);
            assert_eq!(fit[2], plies, "{thick} mm");
            assert!((fit[0] * fit[2] - thick as f32).abs() < 1e-3, "{fit:?}");
        }
        // The 20 mm board: 15 plies of 1.333 mm.
        look.ply = fit_plies(&p, &to_grain, 1.4, 1.0);
        assert_eq!(look.ply[2], 15.0);
        let shade = WoodShade { look, to_grain };
        let lum = |s: WoodSample| s.srgb[0] + s.srgb[1] + s.srgb[2];
        // On the long edge (y = 0, a pixel stepping along x and z): the mean of each ply.
        let ply = |k: usize| {
            let z = (k as f64 + 0.5) * 20.0 / 15.0;
            (0..200)
                .map(|i| lum(shade.sample_seen(Vec3::new(50.0 + f64::from(i) * 2.3, 0.0, z), Vec3::new(0.05, 0.0, 0.0), Vec3::new(0.0, 0.0, 0.05))))
                .sum::<f32>()
                / 200.0
        };
        for k in (1..15).step_by(2) {
            assert!(ply(k) < 0.9 * ply(k - 1) && ply(k) < 0.9 * ply(k + 1), "ply {k}: {} between {} and {}", ply(k), ply(k - 1), ply(k + 1));
        }
        // Around the corner, on the end (x = 0), the other plies show their end grain.
        let end = |k: usize| {
            let z = (k as f64 + 0.5) * 20.0 / 15.0;
            (0..200)
                .map(|i| lum(shade.sample_seen(Vec3::new(0.0, 2.0 + f64::from(i) * 0.47, z), Vec3::new(0.0, 0.05, 0.0), Vec3::new(0.0, 0.0, 0.05))))
                .sum::<f32>()
                / 200.0
        };
        assert!(end(4) < 0.9 * end(5) && end(6) < 0.9 * end(5), "{} {} {}", end(4), end(5), end(6));
        // A glue line between two plies is darker than the wood beside it; the faces have none.
        let at = |z: f64| lum(shade.sample(Vec3::new(300.0, 0.0, z)));
        let line = 4.0 * 20.0 / 15.0;
        assert!(at(line) < 0.8 * at(line + 0.4), "{} beside {}", at(line), at(line + 0.4));
        let face: Vec<f32> = (0..50).map(|i| lum(shade.sample(Vec3::new(100.0 + f64::from(i) * 9.0, 50.0, 20.0)))).collect();
        let inside: Vec<f32> = (0..50).map(|i| lum(shade.sample(Vec3::new(100.0 + f64::from(i) * 9.0, 50.0, 19.6)))).collect();
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        assert!((mean(&face) - mean(&inside)).abs() < 0.05 * mean(&face), "the top face is the top ply, with no glue line");
    }

    /// Sliding the grain along its length moves the same pattern.
    #[test]
    fn a_shift_slides_the_grain_along_the_board() {
        let (p, t) = board();
        let look = species_look("oak-honey").unwrap();
        let a = WoodShade { look, to_grain: frame_for_mesh(&p, &t, Frame::default()).unwrap() };
        let b = WoodShade { look, to_grain: frame_for_mesh(&p, &t, Frame { shift_mm: 40.0, pith_mm: [5.0, -3.0], ..Frame::default() }).unwrap() };
        let along = Vec3::new(f64::from(a.to_grain[0][0]), f64::from(a.to_grain[0][1]), f64::from(a.to_grain[0][2]));
        let wide = Vec3::new(f64::from(a.to_grain[1][0]), f64::from(a.to_grain[1][1]), f64::from(a.to_grain[1][2]));
        let thin = Vec3::new(f64::from(a.to_grain[2][0]), f64::from(a.to_grain[2][1]), f64::from(a.to_grain[2][2]));
        for i in 0..50 {
            let q = Vec3::new(10.0 + f64::from(i) * 7.0, 20.0 + f64::from(i % 7) * 5.0, 3.0);
            let (x, y) = (a.grain_point(q), b.grain_point(q + along * 40.0 + wide * 5.0 - thin * 3.0));
            assert!((0..3).all(|k| (x[k] - y[k]).abs() < 1e-3), "{x:?} {y:?}");
        }
    }

    /// Where the rings end along a line out from the pith, mm (the sharp drop from latewood
    /// to the next year's earlywood).
    fn ring_ends(l: &WoodLook, x: f32) -> Vec<f32> {
        let lum = |r: f32| sample(l, [x, r * 0.6, r * 0.8], [0.0; 3], [0.0; 3]).srgb[0];
        let mut ends = Vec::new();
        let mut last = lum(20.0);
        for i in 1..12000 {
            let r = 20.0 + i as f32 * 0.02;
            let v = lum(r);
            if v > last + 0.04 && ends.last().is_none_or(|e| r - e > 0.3) {
                ends.push(r);
            }
            last = v;
        }
        ends
    }

    /// Real growth is uneven: some years are several times as wide as others. With no
    /// variation the rings keep an even spacing.
    #[test]
    fn ring_widths_vary_from_year_to_year() {
        let mut l = species_look("ash-pale").unwrap();
        (l.seed, l.pores, l.streaks, l.figure_mm) = (77, 0.0, 0.0, 0.0);
        let spread = |l: &WoodLook| {
            let e = ring_ends(l, 300.0);
            let w: Vec<f32> = e.windows(2).map(|p| p[1] - p[0]).collect();
            assert!(w.len() > 30, "{} rings", w.len());
            let mean = w.iter().sum::<f32>() / w.len() as f32;
            (w.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / w.len() as f32).sqrt() / mean
        };
        let natural = spread(&l);
        l.variation = 0.0;
        let even = spread(&l);
        assert!(natural > 0.3, "uneven years: {natural}");
        assert!(even < 0.15 && natural > 2.5 * even, "even years: {even} (natural {natural})");
    }

    /// The same ring is not a circle: followed around the pith, it lies nearer on some sides.
    #[test]
    fn a_log_is_not_round() {
        let mut l = species_look("ash-pale").unwrap();
        (l.seed, l.pores, l.streaks, l.figure_mm) = (77, 0.0, 0.0, 0.0);
        let count = |l: &WoodLook, (c, s): (f32, f32)| {
            let lum = |r: f32| sample(l, [300.0, r * c, r * s], [0.0; 3], [0.0; 3]).srgb[0];
            (1..10000).filter(|i| lum(20.0 + *i as f32 * 0.02) > lum(20.0 + (*i - 1) as f32 * 0.02) + 0.04).count() as f32
        };
        let sides = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (0.6, 0.8), (-0.8, 0.6)];
        let n: Vec<f32> = sides.iter().map(|d| count(&l, *d)).collect();
        let (lo, hi) = n.iter().fold((f32::MAX, 0f32), |(a, b), v| (a.min(*v), b.max(*v)));
        assert!(hi - lo >= 4.0, "rings met out to 220 mm on six sides: {n:?}");
    }

    /// Knots and burl are off unless asked for, and change the wood when they are.
    #[test]
    fn knots_and_burl_are_optional() {
        let mut l = species_look("oak-honey").unwrap();
        (l.seed, l.pores, l.streaks) = (3, 0.0, 0.0);
        // A sleeve 40 mm around the pith, 1.2 m long: every branch of that length passes through it.
        let picture = |l: &WoodLook| {
            (0..24000)
                .map(|i| {
                    let (sin, cos) = ((i / 400) as f32 * 0.1047).sin_cos();
                    sample(l, [(i % 400) as f32 * 3.0, 40.0 * cos, 40.0 * sin], [0.0; 3], [0.0; 3]).srgb[0]
                })
                .collect::<Vec<f32>>()
        };
        let clear = picture(&l);
        let knotty = picture(&WoodLook { knots: 1.0, ..l });
        let burl = picture(&WoodLook { burl: 1.0, ..l });
        let differ = |a: &[f32], b: &[f32]| a.iter().zip(b).filter(|(x, y)| (*x - *y).abs() > 0.02).count();
        // A knot is local: most of the board stays as it was.
        assert!((100..12000).contains(&differ(&clear, &knotty)), "{}", differ(&clear, &knotty));
        // Burl is everywhere.
        assert!(differ(&clear, &burl) > 12000, "{}", differ(&clear, &burl));
        // A knot has no outline and no blot: it is the branch's own, tighter rings, darker
        // on the whole than the trunk's wood (seen here without the rings themselves).
        let flat = WoodLook { contrast: 0.0, ..l };
        let (was, is): (f32, f32) = (picture(&flat).iter().sum(), picture(&WoodLook { knots: 1.0, ..flat }).iter().sum());
        assert!(is < was - 1.0, "where a knot is, the wood is darker: {is} against {was}");
        // And no branch shows past its own 300 mm of the log: the wood has no step there.
        let l = WoodLook { knots: 1.0, ..l };
        for i in 0..4000 {
            let (x, a) = (300.0 * (1 + i % 8) as f32, i as f32 * 0.0731);
            let r = 5.0 + (i % 97) as f32 * 2.5;
            let at = |x: f32| sample(&l, [x, r * a.cos(), r * a.sin()], [0.0; 3], [0.0; 3]).srgb[0];
            assert!((at(x - 0.004) - at(x + 0.004)).abs() < 0.03, "a step at {x}, {r} mm out");
        }
    }

    /// Turning the grain 90 degrees on a board's face runs it across the board; a roll keeps
    /// the grain direction and turns the rings.
    #[test]
    fn the_grain_turns_tilts_and_rolls() {
        let (p, t) = board();
        let row = |m: &[[f32; 4]; 3], k: usize| Vec3::new(f64::from(m[k][0]), f64::from(m[k][1]), f64::from(m[k][2]));
        let plain = frame_for_mesh(&p, &t, Frame::default()).unwrap();
        let turn = frame_for_mesh(&p, &t, Frame { rotate_deg: [90.0, 0.0, 0.0], ..Frame::default() }).unwrap();
        assert!(row(&turn, 0).dot(row(&plain, 1)).abs() > 0.9999, "grain across the width: {turn:?}");
        assert!(row(&turn, 2).dot(row(&plain, 2)) > 0.9999, "{turn:?}");
        let tilt = frame_for_mesh(&p, &t, Frame { rotate_deg: [0.0, 30.0, 0.0], ..Frame::default() }).unwrap();
        assert!((row(&tilt, 0).dot(row(&plain, 2)).abs() - 0.5).abs() < 1e-4 && row(&tilt, 1).dot(row(&plain, 1)) > 0.9999, "{tilt:?}");
        let roll = frame_for_mesh(&p, &t, Frame { rotate_deg: [0.0, 0.0, 90.0], ..Frame::default() }).unwrap();
        assert!(row(&roll, 0).dot(row(&plain, 0)) > 0.9999 && row(&roll, 1).dot(row(&plain, 2)).abs() > 0.9999, "{roll:?}");
        // The pith stays in the body's middle however the grain is turned.
        for m in [turn, tilt, roll, frame_for_mesh(&p, &t, Frame { rotate_deg: [37.0, -12.0, 140.0], ..Frame::default() }).unwrap()] {
            let g = WoodShade { look: species_look("walnut-dark").unwrap(), to_grain: m }.grain_point(Vec3::new(300.0, 50.0, 10.0));
            assert!(g[1].abs() < 1e-3 && g[2].abs() < 1e-3, "{g:?}");
        }
    }

    #[test]
    fn every_starter_wood_has_a_species() {
        for id in ["oak-honey", "oak-smoked", "walnut-dark", "maple-light", "cherry-warm", "ash-pale", "birch-cream", "birch-plywood", "teak-golden"]
        {
            let finished = species_look(&format!("starter.wood.{id}")).unwrap();
            let raw = species_look(&format!("starter.wood.{id}-raw")).unwrap();
            // Raw timber is matt; a finish gives it a sheen.
            assert!(raw.roughness[0] > finished.roughness[0] + 0.3, "{id}");
        }
    }
}
