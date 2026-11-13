//! The cell grid's record data and the kernel's, both ways, field for field: the spec goes to
//! the kernel, the plan comes back (`solidcore::cell_grid`).

use solidcore::cell_grid as k;
use solidcore_record::cell_grid::{
    CellGridPlan, CellGridSpec, CellKind, CellShape, Clip, DotGrid, GridCell,
};

use crate::{loop2, record_loop2, record_vec2, vec2};

/// A record cell shape as the kernel's.
#[must_use]
pub fn shape(s: CellShape) -> k::CellShape {
    match s {
        CellShape::Hexagon => k::CellShape::Hexagon,
        CellShape::Square => k::CellShape::Square,
        CellShape::Triangle => k::CellShape::Triangle,
        CellShape::Circle => k::CellShape::Circle,
        CellShape::Polygon => k::CellShape::Polygon,
    }
}

/// A kernel cell shape as the record's.
#[must_use]
pub fn record_shape(s: k::CellShape) -> CellShape {
    match s {
        k::CellShape::Hexagon => CellShape::Hexagon,
        k::CellShape::Square => CellShape::Square,
        k::CellShape::Triangle => CellShape::Triangle,
        k::CellShape::Circle => CellShape::Circle,
        k::CellShape::Polygon => CellShape::Polygon,
    }
}

/// A record dot grid as the kernel's.
#[must_use]
pub fn dot_grid(g: DotGrid) -> k::DotGrid {
    match g {
        DotGrid::Hexagonal => k::DotGrid::Hexagonal,
        DotGrid::Square => k::DotGrid::Square,
    }
}

/// A kernel dot grid as the record's.
#[must_use]
pub fn record_dot_grid(g: k::DotGrid) -> DotGrid {
    match g {
        k::DotGrid::Hexagonal => DotGrid::Hexagonal,
        k::DotGrid::Square => DotGrid::Square,
    }
}

/// A record clip mode as the kernel's.
#[must_use]
pub fn clip(c: Clip) -> k::Clip {
    match c {
        Clip::Whole => k::Clip::Whole,
        Clip::Snap => k::Clip::Snap,
        Clip::Free => k::Clip::Free,
    }
}

/// A kernel clip mode as the record's.
#[must_use]
pub fn record_clip(c: k::Clip) -> Clip {
    match c {
        k::Clip::Whole => Clip::Whole,
        k::Clip::Snap => Clip::Snap,
        k::Clip::Free => Clip::Free,
    }
}

/// A kernel cell kind as the record's.
#[must_use]
pub fn record_kind(c: k::CellKind) -> CellKind {
    match c {
        k::CellKind::Whole => CellKind::Whole,
        k::CellKind::Exact => CellKind::Exact,
        k::CellKind::Fallback => CellKind::Fallback,
        k::CellKind::Free => CellKind::Free,
    }
}

/// A record cell kind as the kernel's.
#[must_use]
pub fn kind(c: CellKind) -> k::CellKind {
    match c {
        CellKind::Whole => k::CellKind::Whole,
        CellKind::Exact => k::CellKind::Exact,
        CellKind::Fallback => k::CellKind::Fallback,
        CellKind::Free => k::CellKind::Free,
    }
}

/// A record cell as the kernel's.
#[must_use]
pub fn cell(c: &GridCell) -> k::GridCell {
    k::GridCell {
        i: c.i,
        j: c.j,
        kind: kind(c.kind),
        center: vec2(c.center),
        loops: field(&c.loops),
        area: c.area,
    }
}

/// A record cell grid spec as the kernel's.
#[must_use]
pub fn spec(s: &CellGridSpec) -> k::CellGridSpec {
    k::CellGridSpec {
        shape: shape(s.shape),
        sides: s.sides,
        dot_grid: dot_grid(s.dot_grid),
        size: s.size,
        web: s.web,
        angle: s.angle,
        anchor: vec2(s.anchor),
        clip: clip(s.clip),
    }
}

/// A kernel cell grid spec as the record's.
#[must_use]
pub fn record_spec(s: &k::CellGridSpec) -> CellGridSpec {
    CellGridSpec {
        shape: record_shape(s.shape),
        sides: s.sides,
        dot_grid: record_dot_grid(s.dot_grid),
        size: s.size,
        web: s.web,
        angle: s.angle,
        anchor: record_vec2(s.anchor),
        clip: record_clip(s.clip),
    }
}

/// A kernel cell as the record's.
#[must_use]
pub fn record_cell(c: &k::GridCell) -> GridCell {
    GridCell {
        i: c.i,
        j: c.j,
        kind: record_kind(c.kind),
        center: record_vec2(c.center),
        loops: c.loops.iter().map(record_loop2).collect(),
        area: c.area,
    }
}

/// A kernel cell grid plan as the record's.
#[must_use]
pub fn record_plan(p: &k::CellGridPlan) -> CellGridPlan {
    CellGridPlan {
        cells: p.cells.iter().map(record_cell).collect(),
        whole: p.whole,
        partial: p.partial,
        fallback: p.fallback,
        open_area: p.open_area,
        warnings: p.warnings.clone(),
    }
}

/// The record's field loops as the kernel's.
#[must_use]
pub fn field(loops: &[solidcore_record::Loop2]) -> Vec<solidcore::prism::profile::Loop2> {
    loops.iter().map(loop2).collect()
}
