//! A sheet metal part's surfaces and its flat pattern, the record's ([`r`]) and the kernel's
//! ([`k`]), both ways: the same types, field by field. The kernel lays a sheet out
//! ([`solidcore::develop::develop`]) and folds it back ([`solidcore::develop::fold`]) on its own
//! values; `solidcore-ops`' `develop` converts here on the way in and out.

use solidcore::develop as k;
use solidcore_record::develop as r;

use crate::{record_region2, record_seg2, record_vec2, record_vec3, region2, seg2, vec2, vec3};

/// The record's sheet as the kernel's.
#[must_use]
pub fn sheet(s: &r::Sheet) -> k::Sheet {
    k::Sheet {
        thickness: s.thickness,
        k_factor: s.k_factor,
        panels: s.panels.iter().map(panel).collect(),
        joints: s.joints.iter().map(joint).collect(),
        corners: s.corners.iter().map(corner).collect(),
        cuts: s
            .cuts
            .iter()
            .map(|c| k::Cut {
                panel: c.panel,
                region: region2(&c.region),
            })
            .collect(),
    }
}

/// The kernel's sheet as the record's.
#[must_use]
pub fn record_sheet(s: &k::Sheet) -> r::Sheet {
    r::Sheet {
        thickness: s.thickness,
        k_factor: s.k_factor,
        panels: s.panels.iter().map(record_panel).collect(),
        joints: s.joints.iter().map(record_joint).collect(),
        corners: s.corners.iter().map(record_corner).collect(),
        cuts: s
            .cuts
            .iter()
            .map(|c| r::Cut {
                panel: c.panel,
                region: record_region2(&c.region),
            })
            .collect(),
    }
}

fn panel(p: &r::Panel) -> k::Panel {
    k::Panel {
        name: p.name.clone(),
        surface: surface(&p.surface),
        region: region2(&p.region),
    }
}

fn record_panel(p: &k::Panel) -> r::Panel {
    r::Panel {
        name: p.name.clone(),
        surface: record_surface(&p.surface),
        region: record_region2(&p.region),
    }
}

/// The record's panel surface as the kernel's.
#[must_use]
pub fn surface(s: &r::Surface) -> k::Surface {
    match *s {
        r::Surface::Plane => k::Surface::Plane,
        r::Surface::Cylinder {
            radius,
            material_inward,
        } => k::Surface::Cylinder {
            radius,
            material_inward,
        },
        r::Surface::Cone {
            half_angle,
            material_inward,
        } => k::Surface::Cone {
            half_angle,
            material_inward,
        },
    }
}

/// The kernel's panel surface as the record's.
#[must_use]
pub fn record_surface(s: &k::Surface) -> r::Surface {
    match *s {
        k::Surface::Plane => r::Surface::Plane,
        k::Surface::Cylinder {
            radius,
            material_inward,
        } => r::Surface::Cylinder {
            radius,
            material_inward,
        },
        k::Surface::Cone {
            half_angle,
            material_inward,
        } => r::Surface::Cone {
            half_angle,
            material_inward,
        },
    }
}

fn corner(c: &r::Corner) -> k::Corner {
    k::Corner {
        joints: c.joints,
        relief: corner_relief(c.relief),
        size: c.size,
    }
}

fn record_corner(c: &k::Corner) -> r::Corner {
    r::Corner {
        joints: c.joints,
        relief: record_corner_relief(c.relief),
        size: c.size,
    }
}

/// The record's corner relief as the kernel's.
#[must_use]
pub fn corner_relief(c: r::CornerRelief) -> k::CornerRelief {
    match c {
        r::CornerRelief::Notch => k::CornerRelief::Notch,
        r::CornerRelief::Tear => k::CornerRelief::Tear,
        r::CornerRelief::Round => k::CornerRelief::Round,
        r::CornerRelief::Square => k::CornerRelief::Square,
    }
}

/// The kernel's corner relief as the record's.
#[must_use]
pub fn record_corner_relief(c: k::CornerRelief) -> r::CornerRelief {
    match c {
        k::CornerRelief::Notch => r::CornerRelief::Notch,
        k::CornerRelief::Tear => r::CornerRelief::Tear,
        k::CornerRelief::Round => r::CornerRelief::Round,
        k::CornerRelief::Square => r::CornerRelief::Square,
    }
}

fn anchor(a: &r::Anchor) -> k::Anchor {
    k::Anchor {
        panel: a.panel,
        origin: vec2(a.origin),
        dir: vec2(a.dir),
    }
}

fn record_anchor(a: &k::Anchor) -> r::Anchor {
    r::Anchor {
        panel: a.panel,
        origin: record_vec2(a.origin),
        dir: record_vec2(a.dir),
    }
}

/// The record's joint as the kernel's.
#[must_use]
pub fn joint(j: &r::Joint) -> k::Joint {
    k::Joint {
        a: anchor(&j.a),
        b: anchor(&j.b),
        kind: match &j.kind {
            r::JointKind::Bend(b) => k::JointKind::Bend(bend(b)),
            r::JointKind::Seam => k::JointKind::Seam,
            r::JointKind::Rip(rip) => k::JointKind::Rip(k::Rip {
                edges: (
                    rip.edges.0.iter().copied().map(vec2).collect(),
                    rip.edges.1.iter().copied().map(vec2).collect(),
                ),
                curves: (
                    rip.curves.0.iter().map(seg2).collect(),
                    rip.curves.1.iter().map(seg2).collect(),
                ),
                turns: rip.turns.clone(),
                lap: lap(rip.lap),
            }),
        },
    }
}

/// The kernel's joint as the record's.
#[must_use]
pub fn record_joint(j: &k::Joint) -> r::Joint {
    r::Joint {
        a: record_anchor(&j.a),
        b: record_anchor(&j.b),
        kind: match &j.kind {
            k::JointKind::Bend(b) => r::JointKind::Bend(record_bend(b)),
            k::JointKind::Seam => r::JointKind::Seam,
            k::JointKind::Rip(rip) => r::JointKind::Rip(r::Rip {
                edges: (
                    rip.edges.0.iter().copied().map(record_vec2).collect(),
                    rip.edges.1.iter().copied().map(record_vec2).collect(),
                ),
                curves: (
                    rip.curves.0.iter().map(record_seg2).collect(),
                    rip.curves.1.iter().map(record_seg2).collect(),
                ),
                turns: rip.turns.clone(),
                lap: record_lap(rip.lap),
            }),
        },
    }
}

/// The record's lap as the kernel's.
#[must_use]
pub fn lap(l: r::Lap) -> k::Lap {
    match l {
        r::Lap::Miter { gap } => k::Lap::Miter { gap },
        r::Lap::Cut(a, b) => k::Lap::Cut(a, b),
    }
}

/// The kernel's lap as the record's.
#[must_use]
pub fn record_lap(l: k::Lap) -> r::Lap {
    match l {
        k::Lap::Miter { gap } => r::Lap::Miter { gap },
        k::Lap::Cut(a, b) => r::Lap::Cut(a, b),
    }
}

/// The record's bend as the kernel's.
#[must_use]
pub fn bend(b: &r::Bend) -> k::Bend {
    k::Bend {
        angle: b.angle,
        radius: b.radius,
        toward_material: b.toward_material,
        datum: match b.datum {
            r::Datum::Sharp => k::Datum::Sharp,
            r::Datum::Tangent => k::Datum::Tangent,
        },
        length: b.length,
        ends: k::ZoneEnds {
            near: b.ends.near,
            far: b.ends.far,
        },
        tails: (tail(b.tails.0), tail(b.tails.1)),
        relief: (relief(b.relief.0), relief(b.relief.1)),
    }
}

/// The kernel's bend as the record's.
#[must_use]
pub fn record_bend(b: &k::Bend) -> r::Bend {
    r::Bend {
        angle: b.angle,
        radius: b.radius,
        toward_material: b.toward_material,
        datum: match b.datum {
            k::Datum::Sharp => r::Datum::Sharp,
            k::Datum::Tangent => r::Datum::Tangent,
        },
        length: b.length,
        ends: r::ZoneEnds {
            near: b.ends.near,
            far: b.ends.far,
        },
        tails: (record_tail(b.tails.0), record_tail(b.tails.1)),
        relief: (record_relief(b.relief.0), record_relief(b.relief.1)),
    }
}

fn tail(t: r::Tail) -> k::Tail {
    match t {
        r::Tail::Keep => k::Tail::Keep,
        r::Tail::Trim => k::Tail::Trim,
    }
}

fn record_tail(t: k::Tail) -> r::Tail {
    match t {
        k::Tail::Keep => r::Tail::Keep,
        k::Tail::Trim => r::Tail::Trim,
    }
}

fn relief(x: r::Relief) -> k::Relief {
    match x {
        r::Relief::None => k::Relief::None,
        r::Relief::Notch => k::Relief::Notch,
        r::Relief::Slot { width, depth } => k::Relief::Slot { width, depth },
        r::Relief::Tear { normal, offset } => k::Relief::Tear {
            normal: vec3(normal),
            offset,
        },
    }
}

fn record_relief(x: k::Relief) -> r::Relief {
    match x {
        k::Relief::None => r::Relief::None,
        k::Relief::Notch => r::Relief::Notch,
        k::Relief::Slot { width, depth } => r::Relief::Slot { width, depth },
        k::Relief::Tear { normal, offset } => r::Relief::Tear {
            normal: record_vec3(normal),
            offset,
        },
    }
}

/// The record's plane motion as the kernel's.
#[must_use]
pub fn rigid2(m: &r::Rigid2) -> k::Rigid2 {
    k::Rigid2 {
        cos: m.cos,
        sin: m.sin,
        shift: vec2(m.shift),
    }
}

/// The kernel's plane motion as the record's.
#[must_use]
pub fn record_rigid2(m: &k::Rigid2) -> r::Rigid2 {
    r::Rigid2 {
        cos: m.cos,
        sin: m.sin,
        shift: record_vec2(m.shift),
    }
}

/// The record's motion of space as the kernel's.
#[must_use]
pub fn rigid3(m: &r::Rigid3) -> k::Rigid3 {
    k::Rigid3 {
        x: vec3(m.x),
        y: vec3(m.y),
        z: vec3(m.z),
        shift: vec3(m.shift),
    }
}

/// The kernel's motion of space as the record's.
#[must_use]
pub fn record_rigid3(m: &k::Rigid3) -> r::Rigid3 {
    r::Rigid3 {
        x: record_vec3(m.x),
        y: record_vec3(m.y),
        z: record_vec3(m.z),
        shift: record_vec3(m.shift),
    }
}

/// The record's development as the kernel's.
#[must_use]
pub fn development(d: &r::Development) -> k::Development {
    k::Development {
        placements: d
            .placements
            .iter()
            .map(|p| p.as_ref().map(rigid2))
            .collect(),
        panels: d
            .panels
            .iter()
            .map(|p| {
                p.as_ref().map(|p| k::FlatPanel {
                    region: region2(&p.region),
                    origins: p
                        .origins
                        .iter()
                        .map(|l| l.iter().map(|o| edge_from(*o)).collect())
                        .collect(),
                })
            })
            .collect(),
        pieces: d.pieces.clone(),
        zones: d
            .zones
            .iter()
            .map(|z| k::Zone {
                joint: z.joint,
                region: region2(&z.region),
                near: z.near.map(vec2),
                far: z.far.map(vec2),
                panels: z.panels,
            })
            .collect(),
        trims: d
            .trims
            .iter()
            .map(|t| k::Trim {
                panel: t.panel,
                joint: t.joint,
                region: region2(&t.region),
                line: t.line.map(vec2),
            })
            .collect(),
        loops: d.loops.iter().map(mismatch).collect(),
        cuts: d.cuts.iter().map(region2).collect(),
        unrelieved: d.unrelieved.clone(),
    }
}

/// The kernel's development as the record's.
#[must_use]
pub fn record_development(d: &k::Development) -> r::Development {
    r::Development {
        placements: d
            .placements
            .iter()
            .map(|p| p.as_ref().map(record_rigid2))
            .collect(),
        panels: d
            .panels
            .iter()
            .map(|p| {
                p.as_ref().map(|p| r::FlatPanel {
                    region: record_region2(&p.region),
                    origins: p
                        .origins
                        .iter()
                        .map(|l| l.iter().map(|o| record_edge_from(*o)).collect())
                        .collect(),
                })
            })
            .collect(),
        pieces: d.pieces.clone(),
        zones: d.zones.iter().map(record_zone).collect(),
        trims: d
            .trims
            .iter()
            .map(|t| r::Trim {
                panel: t.panel,
                joint: t.joint,
                region: record_region2(&t.region),
                line: t.line.map(record_vec2),
            })
            .collect(),
        loops: d.loops.iter().map(record_mismatch).collect(),
        cuts: d.cuts.iter().map(record_region2).collect(),
        unrelieved: d.unrelieved.clone(),
    }
}

/// The record's bend zone as the kernel's.
#[must_use]
pub fn zone(z: &r::Zone) -> k::Zone {
    k::Zone {
        joint: z.joint,
        region: region2(&z.region),
        near: z.near.map(vec2),
        far: z.far.map(vec2),
        panels: z.panels,
    }
}

/// The kernel's bend zone as the record's.
#[must_use]
pub fn record_zone(z: &k::Zone) -> r::Zone {
    r::Zone {
        joint: z.joint,
        region: record_region2(&z.region),
        near: z.near.map(record_vec2),
        far: z.far.map(record_vec2),
        panels: z.panels,
    }
}

fn edge_from(e: r::EdgeFrom) -> k::EdgeFrom {
    match e {
        r::EdgeFrom::Own(k) => k::EdgeFrom::Own(k),
        r::EdgeFrom::Rip(j) => k::EdgeFrom::Rip(j),
    }
}

fn record_edge_from(e: k::EdgeFrom) -> r::EdgeFrom {
    match e {
        k::EdgeFrom::Own(k) => r::EdgeFrom::Own(k),
        k::EdgeFrom::Rip(j) => r::EdgeFrom::Rip(j),
    }
}

fn mismatch(m: &r::Mismatch) -> k::Mismatch {
    k::Mismatch {
        joint: m.joint,
        distance: m.distance,
        angle: m.angle,
    }
}

/// The kernel's mismatch as the record's.
#[must_use]
pub fn record_mismatch(m: &k::Mismatch) -> r::Mismatch {
    r::Mismatch {
        joint: m.joint,
        distance: m.distance,
        angle: m.angle,
    }
}

/// Record profile segments as the kernel's, in order.
#[must_use]
pub fn segs(s: &[solidcore_record::Seg2]) -> Vec<solidcore::prism::profile::Seg2> {
    s.iter().map(seg2).collect()
}

/// Kernel profile segments as the record's, in order.
#[must_use]
pub fn record_segs(s: &[solidcore::prism::profile::Seg2]) -> Vec<solidcore_record::Seg2> {
    s.iter().map(record_seg2).collect()
}
