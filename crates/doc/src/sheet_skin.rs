//! A sheet body as a skin (#6748): its panels in charts of their own, joined by bends anchored at
//! their virtual sharps, and laid out flat by `solidcraft-develop`. While the sheet model keeps
//! the flat pattern as its master, this is a check that the skin gives the same flat: setting
//! `SOLIDCRAFT_SKIN_REPORT` to a file makes every sheet body that is built append one line to it,
//! naming the thread (a test's name under `cargo test`) and how far the two flats differ.

use solidcraft_develop::{Anchor, Bend, Datum, Development, Joint, JointKind, Panel, Rigid2, Skin, Tail, ZoneEnds, develop};
use solidcraft_geom::{Loop2, Region2, Seg2, Vec2};
use solidcraft_kernel::BoolOp;

use crate::sheet::{FlatWork, SheetBody, SheetFlange, material_boolean, material_union};
use crate::sheet_flat_lineage::{self, Resolution};
use crate::{DocError, Result};

/// Bends closer to a half turn than this (radians) have no usable sharp: they are anchored at
/// their tangent lines.
const HALF_TURN_TOL: f64 = 1e-9;

/// The most the two flats may differ, as the area between them over the length of the sheet's
/// outline: their mean distance apart (mm).
const SAME_FLAT: f64 = 1e-6;

/// The skin of a sheet body, its base panel's chart being the flat frame. Panel `0` is the base
/// and panel `1 + i` flange `i`'s panel; each panel's material runs on from its tangent line to
/// the bend's anchor, as the skin keeps it.
pub(crate) fn skin_of(sh: &SheetBody) -> Result<Skin> {
    let n = sh.flanges.len();
    // Each panel's chart, from the flat frame: the base's is the flat frame itself, and a
    // flange's is put at the start of its bend's anchor so that nothing of its flat place is
    // carried over.
    let mut charts = vec![Rigid2::IDENTITY; n + 1];
    let mut regions = vec![sh.base.clone()];
    let mut joints = Vec::with_capacity(n);
    for (i, f) in sh.flanges.iter().enumerate() {
        let frame = BendFrame::of(sh, i, f);
        let chart = Rigid2::onto(frame.b_origin, frame.along, Vec2::ZERO, Vec2::X)
            .ok_or_else(|| DocError::Invalid(format!("flange {i} has no bend direction")))?;
        if let Some(c) = charts.get_mut(i + 1) {
            *c = chart;
        }
        regions.push(if f.leg > 1e-9 { sh.panel(f) } else { Region2::default() });
        joints.push(frame);
    }
    // Run each panel's material on to its anchors: the parent's along the zone, the flange's
    // along its whole edge on the bend.
    let mut work = FlatWork::new();
    for frame in &joints {
        let parent = frame.parent;
        for (panel, band) in [(parent, frame.parent_band()), (frame.panel, frame.flange_band())] {
            let Some(band) = band else { continue };
            let Some(region) = regions.get_mut(panel) else { continue };
            if region.outer.segs.is_empty() {
                continue;
            }
            if let [one] = material_union(vec![region.clone(), band], &mut work)?.as_slice() {
                *region = one.clone();
            }
        }
    }
    let panels = regions
        .iter()
        .zip(&charts)
        .enumerate()
        .map(|(i, (r, c))| Panel { name: if i == 0 { "base".into() } else { format!("flange {}", i - 1) }, region: c.region(r), ..Panel::default() })
        .collect();
    let joints = joints
        .iter()
        .map(|f| {
            let (Some(pa), Some(pb)) = (charts.get(f.parent), charts.get(f.panel)) else { return Err(DocError::Invalid("flange parent".into())) };
            Ok(Joint {
                a: Anchor { panel: f.parent, origin: pa.point(f.a_origin), dir: pa.vector(f.along) },
                b: Anchor { panel: f.panel, origin: pb.point(f.b_origin), dir: pb.vector(f.along) },
                kind: JointKind::Bend(f.bend.clone()),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Skin { thickness: sh.t, k_factor: sh.k, panels, joints })
}

/// One flange's bend, in the sheet's flat frame: the anchors run `along` the bend with the
/// parent's material on their left, so lengths along them are the flange's own lengths along
/// `d`, turned round when `d` runs the other way.
struct BendFrame {
    parent: usize,
    panel: usize,
    along: Vec2,
    toward: Vec2,
    /// Where the parent's flat material ends (the bend's start line) and the anchors start.
    tangent: Vec2,
    a_origin: Vec2,
    b_origin: Vec2,
    setback: f64,
    width: f64,
    /// The flange panel's edge on the bend, in lengths along the anchors.
    edge: (f64, f64),
    bend: Bend,
}

impl BendFrame {
    fn of(sh: &SheetBody, i: usize, f: &SheetFlange) -> BendFrame {
        let angle = f.angle.abs();
        let datum = if angle < std::f64::consts::PI - HALF_TURN_TOL { Datum::Sharp } else { Datum::Tangent };
        let mut bend = Bend {
            angle,
            radius: f.radius,
            toward_material: f.angle >= 0.0,
            datum,
            ends: ZoneEnds { near: (0.0, 0.0), far: (0.0, 0.0) },
            tails: (Tail::Keep, Tail::Trim),
            relieved: (false, false),
        };
        let setback = bend.setback(sh.t);
        let width = sh.allowance(f);
        // The zone along the bend from `p`: its start line's span and, at a torn end, its far
        // edge running on to the panel's end (as the sheet's `zone_ends`).
        let (u0, u1) = sh.zone_span(f);
        let (e0, e1) = (if f.tear.0 { -f.ext.0 } else { u0 }, if f.tear.1 { f.len + f.ext.1 } else { u1 });
        let edge = if f.shape.is_some() || f.exact.is_some() { (e0, e1) } else { (-f.ext.0, f.len + f.ext.1) };
        // The anchors keep the parent's material on their left, so they run along `d` when the
        // flange (`n`) lies on its right, and the other way round otherwise.
        let forward = Vec2::new(f.d.y, -f.d.x).dot(f.n) > 0.0;
        let flip: fn((f64, f64)) -> (f64, f64) = if forward { |u| u } else { |u| (-u.1, -u.0) };
        let along = if forward { f.d } else { -f.d };
        bend.ends = ZoneEnds { near: flip((u0, u1)), far: flip((e0, e1)) };
        BendFrame {
            parent: f.parent.map_or(0, |k| k + 1),
            panel: i + 1,
            along,
            toward: f.n,
            tangent: f.p,
            a_origin: f.p + f.n * setback,
            b_origin: f.p + f.n * (width - setback),
            setback,
            width,
            edge: flip(edge),
            bend,
        }
    }

    /// A band across the bend from the line `from` (lengths across, toward the flange) to `to`,
    /// over `span` along it.
    fn band(&self, span: (f64, f64), from: f64, to: f64) -> Option<Region2> {
        if !(span.1 > span.0 && (to - from).abs() > 1e-12) {
            return None;
        }
        let at = |u: f64, v: f64| self.tangent + self.along * u + self.toward * v;
        let pts = [at(span.0, from), at(span.1, from), at(span.1, to), at(span.0, to)];
        Some(Region2 { outer: Loop2::polygon(&pts).ccw(), holes: Vec::new() })
    }

    /// The parent's material from its tangent line on to the sharp, along the zone.
    fn parent_band(&self) -> Option<Region2> {
        self.band(self.bend.ends.near, 0.0, self.setback)
    }

    /// The flange's material from its tangent line, the zone's width from the parent's, back
    /// to its sharp, along its whole edge.
    fn flange_band(&self) -> Option<Region2> {
        self.band(self.edge, self.width - self.setback, self.width)
    }
}

/// The sheet's flat material before its cut-outs: its panels and bend zones.
fn sheet_flat(sh: &SheetBody, work: &mut FlatWork) -> Result<Vec<Region2>> {
    let mut pieces = vec![sh.base.clone()];
    for f in &sh.flanges {
        if f.leg > 1e-9 {
            pieces.push(sh.panel(f));
        }
        pieces.push(sh.zone(f));
    }
    material_union(pieces, work)
}

/// The skin's flat: each placed panel less its trims, and the zones; with the development it
/// was joined from.
fn skin_flat(skin: &Skin, work: &mut FlatWork) -> Result<(Vec<Region2>, Development)> {
    let dev = develop(skin, 0).map_err(|e| DocError::Invalid(e.to_string()))?;
    Ok((join(&dev, work)?, dev))
}

/// A development's flat: each placed panel less its trims, and the zones, joined.
pub(crate) fn join(dev: &Development, work: &mut FlatWork) -> Result<Vec<Region2>> {
    let mut pieces = Vec::new();
    for (i, flat) in dev.panels.iter().enumerate() {
        let Some(flat) = flat else { continue };
        let placed = on_grid(&flat.region);
        let trims: Vec<Region2> = dev.trims.iter().filter(|t| t.panel == i).map(|t| on_grid(&t.region)).collect();
        if trims.is_empty() {
            pieces.push(placed);
        } else {
            // Until the kernel's planar booleans take several tools that share an edge (#6748):
            // trims of two bends can share an edge and a corner with the panel, which the
            // planar booleans take apart only one tool at a time, so join them first.
            let trims = stage(&format!("joining panel {i}'s trims"), material_union(trims, work))?;
            pieces.extend(stage(&format!("trimming panel {i}"), material_boolean(&[placed], &trims, BoolOp::Cut, work))?);
        }
    }
    pieces.extend(dev.zones.iter().map(|z| on_grid(&z.region)));
    stage("joining the skin's pieces", material_union(pieces, work))
}

/// A region with its points on the sheet's nanometre grid, as the sheet model rounds its
/// corners. Until the kernel's planar booleans weld edges that nearly coincide (#6748): the
/// development is exact, but a corner it reaches by another sum than the sheet model does can
/// land a rounding from the sheet's, and the planar booleans take two edges that nearly
/// coincide for two, where on the grid they coincide.
fn on_grid(r: &Region2) -> Region2 {
    let g = |v: Vec2| Vec2::new((v.x * 1e9).round() / 1e9, (v.y * 1e9).round() / 1e9);
    let seg = |s: &Seg2| match *s {
        Seg2::Line { a, b } => Seg2::Line { a: g(a), b: g(b) },
        Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center: g(center), radius, start, sweep },
        Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic { p0: g(p0), p1: g(p1), p2: g(p2), p3: g(p3) },
        Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: g(a), apex: g(apex), b: g(b), w },
        Seg2::Bezier { n, mut p } => {
            for q in p.iter_mut().take(usize::from(n) + 1) {
                *q = g(*q);
            }
            Seg2::Bezier { n, p }
        }
    };
    let lp = |l: &Loop2| Loop2 { segs: l.segs.iter().map(seg).collect() };
    Region2 { outer: lp(&r.outer), holes: r.holes.iter().map(lp).collect() }
}

/// Names the step of the round trip an error came from.
fn stage<T>(what: &str, r: Result<T>) -> Result<T> {
    r.map_err(|e| DocError::Invalid(format!("{what}: {e}")))
}

/// A sheet's flat laid out again from its skin.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RoundTrip {
    /// How far the skin's flat is from the sheet's: the area between them over the length of
    /// the sheet's outline (mm).
    pub apart: f64,
    /// How many joints close loops.
    pub loops: usize,
    /// How the skin's flat edges resolve to the skin.
    pub edges: Resolution,
}

pub(crate) fn round_trip(sh: &SheetBody) -> Result<RoundTrip> {
    compare(sh, &stage("building the skin", skin_of(sh))?)
}

fn compare(sh: &SheetBody, skin: &Skin) -> Result<RoundTrip> {
    let mut work = FlatWork::new();
    let mine = stage("joining the sheet's pieces", sheet_flat(sh, &mut work))?;
    let (theirs, dev) = skin_flat(skin, &mut work)?;
    let area = |rs: &[Region2]| rs.iter().map(Region2::area).sum::<f64>();
    let apart = area(&stage("comparing", material_boolean(&mine, &theirs, BoolOp::Cut, &mut work))?)
        + area(&stage("comparing", material_boolean(&theirs, &mine, BoolOp::Cut, &mut work))?);
    let outline: f64 = mine.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).flat_map(|l| &l.segs).map(|s| s.length()).sum();
    let edges = sheet_flat_lineage::resolve(&theirs, &sheet_flat_lineage::sources(&dev));
    Ok(RoundTrip { apart: apart / outline.max(1e-9), loops: dev.loops.len(), edges })
}

/// Appends this sheet's round trip to the file `SOLIDCRAFT_SKIN_REPORT` names, when it is set.
pub(crate) fn report(sh: &SheetBody) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Write;
        let Some(path) = std::env::var_os("SOLIDCRAFT_SKIN_REPORT") else { return };
        let what = sh
            .flanges
            .iter()
            .map(|f| {
                let mut s = String::new();
                for (on, c) in [
                    (f.parent.is_some(), 'r'),
                    (f.angle.abs() >= std::f64::consts::PI - HALF_TURN_TOL, 'h'),
                    (f.tear.0 || f.tear.1, 't'),
                    (f.slope != (0.0, 0.0), 's'),
                    (f.ext != (0.0, 0.0), 'e'),
                    (f.shape.is_some(), 'f'),
                    (f.exact.is_some(), 'x'),
                ] {
                    if on {
                        s.push(c);
                    }
                }
                if s.is_empty() { "-".to_string() } else { s }
            })
            .collect::<Vec<_>>()
            .join(",");
        let result = match round_trip(sh) {
            Ok(rt) => {
                let e = rt.edges;
                let same = if rt.apart <= SAME_FLAT { "same" } else { "differs" };
                format!("{same}\t{:.3e}\t{}\t{}\t{}\t{}\t{}", rt.apart, rt.loops, e.exact, e.merged, e.pieces, e.geometry_only)
            }
            Err(e) => format!("error\t{}", e.to_string().replace(['\t', '\n'], " ")),
        };
        let thread = std::thread::current().name().unwrap_or("main").to_string();
        let line = format!("{thread}\t{}\t{}\t{what}\t{result}\n", sh.body, sh.flanges.len());
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_2;

    use solidcraft_geom::{Plane, Vec3};

    use super::{SAME_FLAT, round_trip};
    use crate::sheet::{AngleDisplay, ReliefShape, RuleValues, SheetBody, base_flange};

    const T: f64 = 2.0;
    const R: f64 = 2.0;

    /// A 100 x 60 plate of 2 mm sheet bent R 2 at K 0.44, under a rule with `corner`.
    fn plate(corner: ReliefShape) -> SheetBody {
        let rv = RuleValues {
            thickness: T,
            k_factor: 0.44,
            bend_radius: R,
            relief_width: 1.0,
            relief_depth: 0.5,
            corner_relief: 6.0,
            corner_relief_shape: corner,
            hem_gap: 0.01,
            gap: 0.5,
            angle_display: AngleDisplay::Bend,
        };
        let pts = [(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (0.0, 60.0)].map(|(x, y)| solidcraft_geom::Vec2::new(x, y));
        let region = solidcraft_geom::Region2 { outer: solidcraft_geom::Loop2::polygon(&pts).ccw(), holes: Vec::new() };
        base_flange("Plate", "Rule", &rv, Plane::XY, region)
    }

    const EDGES: [Vec3; 4] = [Vec3::new(50.0, 0.0, T), Vec3::new(100.0, 30.0, T), Vec3::new(50.0, 60.0, T), Vec3::new(0.0, 30.0, T)];

    /// The skin's flat is the sheet's, closes no loop, and every edge of it is something in the
    /// skin.
    fn same(sh: &SheetBody) {
        let rt = match round_trip(sh) {
            Ok(rt) => rt,
            Err(e) => panic!("{e}"),
        };
        assert!(rt.apart <= SAME_FLAT && rt.loops == 0, "the skin's flat is {:e} mm from the sheet's ({} loops)", rt.apart, rt.loops);
        assert!(rt.edges.geometry_only == 0 && rt.edges.exact > 0, "{:?}", rt.edges);
    }

    #[test]
    fn a_bracket_and_a_tray_develop_from_their_skins() {
        let mut bracket = plate(ReliefShape::Notch);
        assert!(bracket.add_flange(Vec3::new(50.0, 0.0, T), 20.0, FRAC_PI_2, R, "inside", false).is_ok());
        same(&bracket);
        let mut tray = plate(ReliefShape::Notch);
        assert!(tray.add_flanges(&EDGES, 20.0, FRAC_PI_2, R, "inside", false).is_ok());
        same(&tray);
    }

    /// Bends that turn away from the material, past 90 degrees, and from other positions on the
    /// edge, and a hem, whose anchors are its tangent lines.
    #[test]
    fn bends_either_way_and_a_hem_develop_from_their_skins() {
        let mut sh = plate(ReliefShape::Notch);
        assert!(sh.add_flange(Vec3::new(50.0, 0.0, T), 20.0, 2.0, R, "tangent", false).is_ok());
        assert!(sh.add_flange(Vec3::new(100.0, 30.0, T), 15.0, 1.0, R, "outside", true).is_ok());
        assert!(sh.add_hem(Vec3::new(50.0, 60.0, T), 10.0, 0.5, false).is_ok());
        same(&sh);
    }

    /// Torn corners slant the zones' ends, and a return stands on a flange.
    #[test]
    fn torn_corners_and_a_return_develop_from_their_skins() {
        let mut sh = plate(ReliefShape::Tear);
        assert!(sh.add_flanges(&EDGES, 20.0, FRAC_PI_2, R, "inside", false).is_ok());
        assert!(sh.flanges.iter().any(|f| f.tear.0 || f.tear.1));
        let Some(top) = sh.far_edge_pick(0) else { panic!("the first flange has a far edge") };
        assert!(sh.add_flange(top, 10.0, FRAC_PI_2, R, "inside", false).is_ok());
        same(&sh);
    }

    /// A flap on the end of a wall: the wall's panel loses a band to its own bend and one to
    /// the flap's, which meet at its corner.
    #[test]
    fn a_flap_on_the_end_of_a_wall_develops_from_its_skin() {
        let mut sh = plate(ReliefShape::Notch);
        assert!(sh.add_flange(Vec3::new(0.0, 30.0, T), 40.0, FRAC_PI_2, R, "inside", false).is_ok());
        assert!(sh.add_flange(Vec3::new(1.0, 0.0, 20.0), 40.0, std::f64::consts::FRAC_PI_4, R, "inside", false).is_ok());
        assert!(sh.flanges.get(1).is_some_and(|f| f.parent == Some(0)), "the flap stands on the wall");
        same(&sh);
    }

    /// The check sees a wrong skin: one whose K-factor is off by 0.1 puts the flange 0.1 x T x
    /// pi / 2 from where the sheet has it.
    #[test]
    fn a_skin_with_another_k_factor_does_not_give_the_sheets_flat() {
        let mut sh = plate(ReliefShape::Notch);
        assert!(sh.add_flange(Vec3::new(50.0, 0.0, T), 20.0, FRAC_PI_2, R, "inside", false).is_ok());
        let Ok(mut skin) = super::skin_of(&sh) else { panic!("a skin") };
        skin.k_factor += 0.1;
        let apart = super::compare(&sh, &skin).map_or(0.0, |rt| rt.apart);
        // The flange and its far edge shift 0.1 pi mm: 100 mm long, over a 2 x (100 + 80.something)
        // outline, a fraction of that on average.
        assert!(apart > 0.01, "{apart}");
    }
}
