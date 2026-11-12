//! The lattice an analysis runs on, and which of its cells the model fills.
//!
//! Cells are cubes, never boxes. Equal spacing along every axis means the truncation error is
//! the same in every direction, so an answer does not depend on which way the part happens to
//! be turned in the document; the flow solver needs it outright, because the lattice
//! Boltzmann scheme is only consistent on an equal-spaced lattice.
//!
//! Lengths here are the document's millimetres. Physics is done in metres, and the solvers
//! convert at the one place they have to -- see [`MM_PER_M`].

use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
use solidcraft_geom::{Aabb3, Mesh};

use crate::{Result, SimError, domain};

/// Millimetres in a metre. The lattice is in the document's millimetres and every material
/// property is per metre, so a solver that forgets this is out by a factor of a thousand.
pub const MM_PER_M: f64 = 1000.0;

/// How much of a cell the model must fill for that cell to be solved for, unless a study says
/// otherwise. Everything the suite measures is on this.
pub const HALF_FULL: f64 = 0.5;

/// Most cells a lattice may hold.
///
/// This is a memory bound rather than a patience bound: a thermal solve keeps about six
/// numbers per cell, so thirty-two million cells is already some two gigabytes of working
/// set, and a flow solve keeps far more.
pub const MAX_CELLS: usize = 32_000_000;

/// Cells across the longest side of the analysis box when a study does not say. Coarse: a
/// first look should come back while someone is still watching.
pub const DEFAULT_SPAN: usize = 48;

/// Most cells across the longest side a study may ask for.
pub const MAX_SPAN: usize = 2048;

/// Sample points along each axis inside a cell when deciding how full it is.
pub const MAX_SAMPLES: usize = 8;

/// A box of model space cut into cubic cells.
///
/// Cell `(i, j, k)` is stored at `i + n0·(j + n1·k)`, i fastest, which is also the order a
/// solver sweeps in: walking x innermost keeps the three cells a stencil reaches along x next
/// to each other in memory.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid3 {
    n: [usize; 3],
    h: f64,
    lo: Vec3,
}

impl Grid3 {
    /// A lattice over `bounds`, `span` cells across its longest side, with `pad` cells of room
    /// added on every side.
    ///
    /// The model is centred in the lattice, so padding does not shift it. Padding is what a
    /// flow study puts the wind tunnel in; a conduction study needs one cell of it, to have
    /// somewhere for the outside of a boundary face to be.
    ///
    /// A study of a real part wants [`Grid3::over_parts`] instead. This lays the lattice the
    /// caller asked for and no more, which on a part square to the axes is worth tens of per
    /// cent of the answer in the wrong direction (`fitted_span`); the span here is honoured
    /// exactly, which is what a verification case with a chosen lattice needs and what a part
    /// does not.
    pub fn over(bounds: Aabb3, span: usize, pad: usize) -> Result<Grid3> {
        if bounds.is_empty() {
            return domain("nothing to analyse: the model has no extent");
        }
        let size = bounds.size();
        let centre = bounds.center();
        if !size.is_finite() || !centre.is_finite() {
            return domain("the model's extent is not a finite box");
        }
        if !(1..=MAX_SPAN).contains(&span) {
            return Err(SimError::Setup(format!("cells across: {span}, and a lattice may have 1 to {MAX_SPAN}")));
        }
        let longest = size.x.max(size.y).max(size.z);
        if !(longest > 0.0) {
            return domain("nothing to analyse: the model is flat in every direction");
        }
        let h = longest / span as f64;
        if !(h > 0.0 && h.is_finite()) {
            return domain("the model's extent does not give a usable cell size");
        }
        Self::laid_out(size, centre, h, pad)
    }

    /// A lattice over a model, with the spacing fitted to the model's own flat faces.
    ///
    /// This is what a study of a part uses, and it differs from [`Grid3::over`] in one way that
    /// is worth a great deal: where the part has a flat face square to an axis, the spacing is
    /// moved by a few per cent so that face lands on a boundary between cells rather than part
    /// way through one. [`fitted_span`] has the measurements; the short version is that a bar
    /// pulled along its axis, whose answer is exactly the traction, reads it exactly when the
    /// cells divide the section and is 8 per cent low to 42 per cent high when they do not,
    /// with no trend as the lattice is refined.
    ///
    /// Only the axes the model really has flat end faces on are fitted. A cylinder's ends are
    /// fitted and its diameter is not: across the diameter there is nothing flat to land on, the
    /// cells on the curved wall are part full whatever the spacing, and insisting on a diameter
    /// that divides would only buy a coarser lattice than the caller asked for.
    pub fn over_parts(parts: &[&Mesh], span: usize, pad: usize) -> Result<Grid3> {
        let mut bounds = Aabb3::EMPTY;
        for m in parts {
            bounds = bounds.union(&m.bounds());
        }
        if bounds.is_empty() {
            return domain("nothing to analyse: the model has no extent");
        }
        let size = bounds.size();
        let centre = bounds.center();
        if !size.is_finite() || !centre.is_finite() {
            return domain("the model's extent is not a finite box");
        }
        if !(1..=MAX_SPAN).contains(&span) {
            return Err(SimError::Setup(format!("cells across: {span}, and a lattice may have 1 to {MAX_SPAN}")));
        }
        let longest = size.x.max(size.y).max(size.z);
        if !(longest > 0.0) {
            return domain("nothing to analyse: the model is flat in every direction");
        }
        let h = longest / span as f64;
        if !(h > 0.0 && h.is_finite()) {
            return domain("the model's extent does not give a usable cell size");
        }
        // A spacing that divides the part is worth much more than the exact count asked for, so
        // try it first and keep the count asked for as the fallback.
        if let Some(fitted) = fitted_span(size, flat_ends(parts, &bounds), longest, span) {
            let fine = longest / fitted as f64;
            if fine > 0.0
                && fine.is_finite()
                && let Ok(grid) = Self::laid_out(size, centre, fine, pad)
            {
                return Ok(grid);
            }
        }
        Self::laid_out(size, centre, h, pad)
    }

    /// The lattice of a given spacing over a model's box, padded by whole cells on every side.
    fn laid_out(size: Vec3, centre: Vec3, h: f64, pad: usize) -> Result<Grid3> {
        let mut n = [0usize; 3];
        let mut total: usize = 1;
        for (a, s) in [size.x, size.y, size.z].into_iter().enumerate() {
            // ceil, and never below one cell: a part can be a plate with no thickness to speak
            // of, and it still has an inside. A ratio that is a whole number to within what f64
            // can carry is that whole number: the point of fitting the spacing is that it comes
            // out whole, and `ceil` on 10.000000000000002 would add the overhang straight back.
            let ratio = s / h;
            let across = if (ratio - ratio.round()).abs() <= 1e-9 * ratio.abs().max(1.0) { ratio.round() } else { ratio.ceil() };
            let across = if across.is_finite() && across >= 1.0 { across as usize } else { 1 };
            let with_pad = across.checked_add(pad.saturating_mul(2)).ok_or(SimError::TooLarge { cells: u64::MAX, limit: MAX_CELLS as u64 })?;
            let Some(t) = total.checked_mul(with_pad) else {
                return Err(SimError::TooLarge { cells: u64::MAX, limit: MAX_CELLS as u64 });
            };
            if t > MAX_CELLS {
                return Err(SimError::TooLarge { cells: t as u64, limit: MAX_CELLS as u64 });
            }
            total = t;
            *n.get_mut(a).ok_or(SimError::NotFinite("lattice axis"))? = with_pad;
        }
        let lo = Vec3::new(centre.x - n[0] as f64 * h / 2.0, centre.y - n[1] as f64 * h / 2.0, centre.z - n[2] as f64 * h / 2.0);
        Ok(Grid3 { n, h, lo })
    }

    /// A lattice of a given shape, spacing and lower corner.
    ///
    /// [`Grid3::over_parts`] is what a study of a model uses; this is for a domain whose shape
    /// is chosen rather than derived -- a channel a few cells wide and many long, which is what
    /// a verification case needs.
    pub fn new(n: [usize; 3], spacing: f64, origin: Vec3) -> Result<Grid3> {
        if !(spacing > 0.0) || !spacing.is_finite() {
            return domain(format!("a cell size of {spacing}"));
        }
        if !origin.is_finite() {
            return domain("a lattice corner that is not a point");
        }
        let mut total: usize = 1;
        for across in n {
            if across == 0 {
                return domain("a lattice with no cells along one of its axes");
            }
            let Some(t) = total.checked_mul(across) else {
                return Err(SimError::TooLarge { cells: u64::MAX, limit: MAX_CELLS as u64 });
            };
            if t > MAX_CELLS {
                return Err(SimError::TooLarge { cells: t as u64, limit: MAX_CELLS as u64 });
            }
            total = t;
        }
        Ok(Grid3 { n, h: spacing, lo: origin })
    }

    /// Cells along x, y and z.
    pub fn count(&self) -> [usize; 3] {
        self.n
    }

    /// The length of a cell's side, in millimetres.
    pub fn spacing(&self) -> f64 {
        self.h
    }

    /// The lower corner of cell `(0, 0, 0)`.
    pub fn origin(&self) -> Vec3 {
        self.lo
    }

    /// How many cells the lattice holds.
    pub fn cells(&self) -> usize {
        self.n[0].saturating_mul(self.n[1]).saturating_mul(self.n[2])
    }

    /// The box the lattice covers.
    pub fn bounds(&self) -> Aabb3 {
        Aabb3 { min: self.lo, max: self.lo + Vec3::new(self.n[0] as f64 * self.h, self.n[1] as f64 * self.h, self.n[2] as f64 * self.h) }
    }

    /// The volume of one cell, in cubic metres.
    pub fn cell_volume_m3(&self) -> f64 {
        let h = self.h / MM_PER_M;
        h * h * h
    }

    /// The area of one face of one cell, in square metres.
    pub fn face_area_m2(&self) -> f64 {
        let h = self.h / MM_PER_M;
        h * h
    }

    /// The distance between the centres of two cells that share a face, in metres.
    pub fn pitch_m(&self) -> f64 {
        self.h / MM_PER_M
    }

    /// The index of cell `(i, j, k)`, or `None` when it is off the lattice.
    pub fn at(&self, i: usize, j: usize, k: usize) -> Option<usize> {
        if i >= self.n[0] || j >= self.n[1] || k >= self.n[2] {
            return None;
        }
        Some(i + self.n[0] * (j + self.n[1] * k))
    }

    /// Which cell an index stands for, or `None` when the index is past the end.
    pub fn ijk(&self, cell: usize) -> Option<[usize; 3]> {
        if cell >= self.cells() {
            return None;
        }
        let (plane, nx) = (self.n[0] * self.n[1], self.n[0]);
        if plane == 0 || nx == 0 {
            return None;
        }
        let k = cell / plane;
        let rest = cell - k * plane;
        Some([rest % nx, rest / nx, k])
    }

    /// The centre of a cell, in model space.
    pub fn centre(&self, cell: usize) -> Option<Vec3> {
        let [i, j, k] = self.ijk(cell)?;
        Some(self.lo + Vec3::new((i as f64 + 0.5) * self.h, (j as f64 + 0.5) * self.h, (k as f64 + 0.5) * self.h))
    }

    /// How far apart in the index two cells one step along `axis` are.
    pub fn stride(&self, axis: usize) -> Option<usize> {
        match axis {
            0 => Some(1),
            1 => Some(self.n[0]),
            2 => Some(self.n[0] * self.n[1]),
            _ => None,
        }
    }

    /// The next cell along `axis`, or `None` at the far side of the lattice.
    pub fn next(&self, cell: usize, axis: usize) -> Option<usize> {
        let ijk = self.ijk(cell)?;
        let at = *ijk.get(axis)?;
        if at + 1 >= *self.n.get(axis)? {
            return None;
        }
        Some(cell + self.stride(axis)?)
    }

    /// The previous cell along `axis`, or `None` at the near side of the lattice.
    pub fn prev(&self, cell: usize, axis: usize) -> Option<usize> {
        let ijk = self.ijk(cell)?;
        if *ijk.get(axis)? == 0 {
            return None;
        }
        Some(cell - self.stride(axis)?)
    }

    /// The middle of one face of one cell: the cell's centre, half a cell along the way out.
    pub fn face_centre(&self, face: SurfaceFace) -> Option<Vec3> {
        let centre = self.centre(face.cell as usize)?;
        Some(centre + face.outward() * (self.h / 2.0))
    }

    /// The cell a point falls in, or `None` when the point is outside the lattice.
    pub fn cell_at(&self, p: Vec3) -> Option<usize> {
        let d = p - self.lo;
        let mut ijk = [0usize; 3];
        for (a, v) in [d.x, d.y, d.z].into_iter().enumerate() {
            let t = (v / self.h).floor();
            if !t.is_finite() || t < 0.0 {
                return None;
            }
            *ijk.get_mut(a)? = t as usize;
        }
        self.at(ijk[0], ijk[1], ijk[2])
    }
}

/// The part number of a cell no part fills.
pub const NO_PART: u16 = u16::MAX;

/// The equation number of a cell the solver does not solve for.
pub const NO_EQ: u32 = u32::MAX;

/// Most parts one analysis may tell apart.
pub const MAX_PARTS: usize = NO_PART as usize;

/// Which cells of a lattice the model fills, and with which part.
///
/// A cell belongs to the part that fills most of it, and counts as solid when a part fills at
/// least half of it. The fraction itself is kept, and with it each cell's *share*: the fraction
/// scaled so that a part's cells add up to the volume the part really has, thin cells included.
/// Weigh anything spread through the material by that share and a round or slanted part carries
/// the right amount of it, rather than the amount a staircase of whole cells would hold.
#[derive(Clone, Debug)]
pub struct Occupancy {
    grid: Grid3,
    fill: Vec<f32>,
    part: Vec<u16>,
    eq: Vec<u32>,
    cells: Vec<u32>,
    /// How much material each solid cell carries, as a multiple of a whole cell's worth.
    ///
    /// This is the cell's own fraction, scaled so that each part's cells add up to the volume
    /// the part really has. Without that scaling, counting each cell by its fraction would lose
    /// the thin cells the half-full rule throws away and leave every part light; the scaling
    /// gives the lost material back to the cells that remain.
    share: Vec<f32>,
    /// Where the material sits inside a cell a wall crosses, in the cell's own coordinates:
    /// zero at the cell's middle, plus or minus one at its faces.
    ///
    /// A cell the model fills whole has its material centred and is left out, so this holds
    /// only the cells on a wall -- the surface of the model rather than its volume, which is
    /// why a map is cheaper here than a field. [`Occupancy::material_centre_local`] answers the
    /// middle for anything not listed.
    cut_centre: std::collections::HashMap<u32, [f32; 3]>,
}

/// One face of one solid cell, as a number small enough to list by the thousand.
///
/// `dir` is `axis·2 + 1` for the face looking along the axis and `axis·2` for the one looking
/// against it, so the six directions pack into a byte and a patch of surface is cheap to hold,
/// to compare and to send.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SurfaceFace {
    pub cell: u32,
    pub dir: u8,
}

impl SurfaceFace {
    pub fn new(cell: usize, axis: usize, positive: bool) -> SurfaceFace {
        SurfaceFace { cell: cell as u32, dir: (axis as u8) * 2 + u8::from(positive) }
    }

    pub fn axis(&self) -> usize {
        (self.dir / 2) as usize
    }

    /// Whether the face looks along its axis or against it.
    pub fn positive(&self) -> bool {
        self.dir % 2 == 1
    }

    /// The way out of the solid through this face.
    pub fn outward(&self) -> Vec3 {
        let sign = if self.positive() { 1.0 } else { -1.0 };
        match self.axis() {
            0 => Vec3::new(sign, 0.0, 0.0),
            1 => Vec3::new(0.0, sign, 0.0),
            _ => Vec3::new(0.0, 0.0, sign),
        }
    }
}

/// A face of a solid cell with nothing solid on the other side: where the model meets the
/// world, as the lattice sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundaryFace {
    pub cell: usize,
    /// 0, 1 or 2 for x, y or z.
    pub axis: usize,
    /// Whether the face looks along the axis or against it.
    pub positive: bool,
}

impl Occupancy {
    /// Which cells each part's mesh fills, sampling `samples` points along each axis of every
    /// cell (so `samples³` points per cell).
    ///
    /// One sample is the cell's centre, which is the usual staircase test; more sample points
    /// give the fraction, and the fraction is what decides a cell on the wall. Four is enough
    /// for the fraction to settle to a couple of per cent.
    pub fn of(grid: Grid3, parts: &[&Mesh], samples: usize) -> Result<Occupancy> {
        Occupancy::of_keeping(grid, parts, samples, HALF_FULL)
    }

    /// The same, saying how much of a cell the model must fill for that cell to be solved for.
    ///
    /// [`HALF_FULL`] is the rule everything is measured on. Lowering it keeps the thin cells a
    /// wall leaves, which makes the outline of the solved part finer-grained -- and on its own
    /// that is worth nothing at all: measured on a plate with a hole, 3.591 at a half, 3.645 at
    /// three tenths, 3.559 at 0.15, no trend. It pays only together with the surface being
    /// followed, where the two reach 3.448 against CalculiX's 3.242 where either alone manages
    /// 3.56 or worse. It costs something too: the cells it keeps are slivers, and on a part whose
    /// stress is uniform there is no outline to improve and the slivers only read high.
    ///
    /// See docs/analysis-verification.md. There is no setting that is best for both kinds of
    /// part, which is why this is a choice and not a better default.
    pub fn of_keeping(grid: Grid3, parts: &[&Mesh], samples: usize, least_fill: f64) -> Result<Occupancy> {
        if parts.is_empty() {
            return domain("nothing to analyse: no bodies were given");
        }
        if parts.len() > MAX_PARTS {
            return Err(SimError::Setup(format!("{} bodies, and an analysis may tell {MAX_PARTS} apart", parts.len())));
        }
        let samples = samples.clamp(1, MAX_SAMPLES);
        let total = grid.cells();
        let mut fill = vec![0f32; total];
        let mut part = vec![NO_PART; total];
        let mut cut_centre: std::collections::HashMap<u32, [f32; 3]> = std::collections::HashMap::new();
        let h = grid.spacing();
        let per = 1.0 / samples as f64;
        let weight = 1.0 / (samples * samples * samples) as f64;

        for (p, mesh) in parts.iter().enumerate() {
            let bounds = mesh.bounds();
            if bounds.is_empty() {
                continue;
            }
            // Only the cells the part's own box reaches can hold any of it. One cell of slack
            // on each side keeps a wall that sits exactly on a cell boundary.
            let Some((from, to)) = cell_range(&grid, &bounds) else { continue };
            let inside = mesh.inside_index();
            for k in from[2]..=to[2] {
                for j in from[1]..=to[1] {
                    for i in from[0]..=to[0] {
                        let Some(cell) = grid.at(i, j, k) else { continue };
                        let base = grid.origin() + Vec3::new(i as f64 * h, j as f64 * h, k as f64 * h);
                        let mut hits = 0u32;
                        // Where in the cell the samples that landed in the model were, summed in
                        // the cell's own coordinates. Divided by the count it is the centre of
                        // the material, which is not the centre of the cell once a wall crosses
                        // it, and is the point a reading taken for the cell belongs at.
                        let mut found = [0.0f64; 3];
                        for sz in 0..samples {
                            for sy in 0..samples {
                                for sx in 0..samples {
                                    let frac = [(sx as f64 + 0.5) * per, (sy as f64 + 0.5) * per, (sz as f64 + 0.5) * per];
                                    let at = base + Vec3::new(frac[0] * h, frac[1] * h, frac[2] * h);
                                    if inside.contains(at) {
                                        hits += 1;
                                        for (slot, f) in found.iter_mut().zip(&frac) {
                                            *slot += 2.0 * f - 1.0;
                                        }
                                    }
                                }
                            }
                        }
                        if hits == 0 {
                            continue;
                        }
                        let f = (hits as f64 * weight) as f32;
                        // The part that fills most of a cell owns it. Bodies that overlap are
                        // an interference the model has, not something to average over.
                        if let (Some(have), Some(owner)) = (fill.get_mut(cell), part.get_mut(cell))
                            && f > *have
                        {
                            // Whether an earlier part had already claimed this cell, and so may
                            // have left a centre behind for it. Nothing else can have, so a cell
                            // first seen here needs no lookup at all -- which is all but the
                            // surface of a part, and every cell of the only part when there is
                            // one.
                            let claimed = *have > 0.0;
                            *have = f;
                            *owner = p as u16;
                            // A cell filled whole has its material centred, and leaving it out
                            // keeps that exact rather than to the rounding of a sum.
                            if f < 1.0 && hits > 0 {
                                let n = f64::from(hits);
                                cut_centre.insert(cell as u32, [(found[0] / n) as f32, (found[1] / n) as f32, (found[2] / n) as f32]);
                            } else if claimed {
                                cut_centre.remove(&(cell as u32));
                            }
                        }
                    }
                }
            }
        }

        // What each part's own cells add up to before the half-full rule throws the thin ones
        // away: the volume the lattice says that part has, in whole cells.
        let mut whole = vec![0.0f64; parts.len().max(1)];
        for c in 0..total {
            let f = fill.get(c).copied().unwrap_or(0.0) as f64;
            if f <= 0.0 {
                continue;
            }
            let p = part.get(c).copied().unwrap_or(NO_PART) as usize;
            if let Some(slot) = whole.get_mut(p) {
                *slot += f;
            }
        }

        let mut eq = vec![NO_EQ; total];
        let mut cells = Vec::new();
        for c in 0..total {
            let full = f64::from(fill.get(c).copied().unwrap_or(0.0)) >= least_fill;
            if !full {
                if let Some(owner) = part.get_mut(c) {
                    *owner = NO_PART;
                }
                continue;
            }
            if cells.len() >= u32::MAX as usize - 1 {
                return Err(SimError::TooLarge { cells: cells.len() as u64, limit: MAX_CELLS as u64 });
            }
            if let Some(slot) = eq.get_mut(c) {
                *slot = cells.len() as u32;
            }
            cells.push(c as u32);
        }
        if cells.is_empty() {
            return domain("nothing to analyse: the lattice is too coarse to hold the model, so no cell is inside it");
        }
        // How much material each cell carries. A cell's own fraction says how much of it the
        // model fills, but a cell the model fills less than half of is not solved for at all,
        // and what it held would simply go missing. So each part's fractions are scaled up to
        // the volume the part has over all of the lattice, thin cells included: the material is
        // put back into the cells that remain, nearest to where it was.
        let mut share = vec![0.0f32; total];
        let mut kept = vec![0.0f64; whole.len()];
        for c in cells.iter() {
            let c = *c as usize;
            let f = fill.get(c).copied().unwrap_or(0.0) as f64;
            let p = part.get(c).copied().unwrap_or(NO_PART) as usize;
            if let Some(slot) = kept.get_mut(p) {
                *slot += f;
            }
        }
        // Only a sane correction is worth making: a part whose cells the lattice barely caught,
        // or one another body has swallowed, keeps its cells as they are rather than having them
        // stretched to cover what is not there.
        let scale: Vec<f64> = whole.iter().zip(&kept).map(|(w, k)| if *k > 0.0 && *w > 0.0 { (w / k).clamp(0.5, 2.0) } else { 1.0 }).collect();
        for c in cells.iter() {
            let c = *c as usize;
            let f = fill.get(c).copied().unwrap_or(0.0) as f64;
            let p = part.get(c).copied().unwrap_or(NO_PART) as usize;
            if let Some(slot) = share.get_mut(c) {
                *slot = (f * scale.get(p).copied().unwrap_or(1.0)) as f32;
            }
        }

        // Only the cells that are solved for will ever be asked about.
        cut_centre.retain(|c, _| eq.get(*c as usize).copied().is_some_and(|e| e != NO_EQ));

        Ok(Occupancy { grid, fill, part, eq, cells, share, cut_centre })
    }

    pub fn grid(&self) -> &Grid3 {
        &self.grid
    }

    /// How many cells the solver solves for.
    pub fn equations(&self) -> usize {
        self.cells.len()
    }

    /// The cell an equation stands for.
    pub fn cell_of(&self, eq: usize) -> Option<usize> {
        self.cells.get(eq).map(|c| *c as usize)
    }

    /// The equation a cell is solved by, or `None` when the cell is not solid.
    pub fn eq_of(&self, cell: usize) -> Option<usize> {
        match self.eq.get(cell).copied() {
            Some(NO_EQ) | None => None,
            Some(e) => Some(e as usize),
        }
    }

    /// Whether the model fills a cell.
    pub fn is_solid(&self, cell: usize) -> bool {
        self.eq_of(cell).is_some()
    }

    /// Which part fills a cell, or `None` when none does.
    pub fn part_of(&self, cell: usize) -> Option<u16> {
        match self.part.get(cell).copied() {
            Some(NO_PART) | None => None,
            Some(p) => Some(p),
        }
    }

    /// The fraction of a cell the model fills, 0 to 1.
    pub fn fill_of(&self, cell: usize) -> f64 {
        self.fill.get(cell).copied().unwrap_or(0.0) as f64
    }

    /// Where the material sits inside a cell, in the cell's own coordinates: zero at the cell's
    /// middle, plus or minus one at its faces.
    ///
    /// For a cell the model fills whole this is the middle, which is where a trilinear element's
    /// strain is closest to its own average. For a cell a wall crosses it is the centre of the
    /// material alone, and the difference matters: the middle of such a cell can be in the void
    /// outside the model, where the element's displacement field is extrapolation rather than
    /// anything the part does, and a stress read there says more about the lattice than about
    /// the part.
    pub fn material_centre_local(&self, cell: usize) -> [f64; 3] {
        match self.cut_centre.get(&(cell as u32)) {
            Some([x, y, z]) => [f64::from(*x), f64::from(*y), f64::from(*z)],
            None => [0.0; 3],
        }
    }

    /// How much material a cell carries, as a multiple of a whole cell's worth.
    ///
    /// One for a cell the model fills, less for one a wall crosses, and more than its own
    /// fraction by however much the part lost to cells too thin to be solved for. Weight
    /// anything spread through the material -- mass, heat generated, warmth stored, stiffness --
    /// by this, and the part's total comes out right.
    pub fn share_of(&self, cell: usize) -> f64 {
        self.share.get(cell).copied().unwrap_or(0.0) as f64
    }

    /// The volume the analysis actually carries, in cubic metres: every cell it solves for,
    /// counted by its share.
    ///
    /// This is [`Occupancy::volume_mm3`] in other units, up to the clamp on parts the lattice
    /// barely caught: the material the half-full rule loses is given back to the cells that
    /// remain rather than dropped.
    pub fn solid_volume_m3(&self) -> f64 {
        let v = self.grid.cell_volume_m3();
        self.cells.iter().map(|c| self.share_of(*c as usize) * v).sum()
    }

    /// The volume the lattice says the model has, in cubic millimetres, from the fractions
    /// rather than from whole cells.
    pub fn volume_mm3(&self) -> f64 {
        let h = self.grid.spacing();
        self.fill.iter().map(|f| *f as f64).sum::<f64>() * h * h * h
    }

    /// Every face of a solid cell with nothing solid behind it, as [`SurfaceFace`].
    pub fn surface(&self) -> impl Iterator<Item = SurfaceFace> + '_ {
        self.boundary().map(|b| SurfaceFace::new(b.cell, b.axis, b.positive))
    }

    /// Every face of a solid cell with nothing solid behind it.
    pub fn boundary(&self) -> impl Iterator<Item = BoundaryFace> + '_ {
        self.cells.iter().flat_map(move |c| {
            let cell = *c as usize;
            (0..3usize).flat_map(move |axis| {
                [true, false].into_iter().filter_map(move |positive| {
                    let across = if positive { self.grid.next(cell, axis) } else { self.grid.prev(cell, axis) };
                    match across {
                        Some(n) if self.is_solid(n) => None,
                        _ => Some(BoundaryFace { cell, axis, positive }),
                    }
                })
            })
        })
    }
}

/// The range of cells a box reaches, with a cell of slack, or `None` when it misses the
/// lattice altogether.
pub(crate) fn cell_range(grid: &Grid3, bounds: &Aabb3) -> Option<([usize; 3], [usize; 3])> {
    let n = grid.count();
    let h = grid.spacing();
    let lo = grid.origin();
    let (mut from, mut to) = ([0usize; 3], [0usize; 3]);
    for (a, (min, max)) in [(bounds.min.x, bounds.max.x), (bounds.min.y, bounds.max.y), (bounds.min.z, bounds.max.z)].into_iter().enumerate() {
        let origin = *[lo.x, lo.y, lo.z].get(a)?;
        let across = *n.get(a)?;
        let first = ((min - origin) / h).floor() - 1.0;
        let last = ((max - origin) / h).floor() + 1.0;
        if !first.is_finite() || !last.is_finite() || last < 0.0 || first >= across as f64 {
            return None;
        }
        *from.get_mut(a)? = first.max(0.0) as usize;
        *to.get_mut(a)? = (last.min(across as f64 - 1.0)).max(0.0) as usize;
    }
    Some((from, to))
}

/// What the lattice failed to land on, where it failed, for a study to say so.
///
/// [`Grid3::over_parts`] moves the cell count a little to make the part a whole number of cells
/// across, and gives up rather than wander far from the count asked for. When it gives up the
/// caller is left solving a part that is bigger than the one they drew, and in silence, which is
/// the one thing this must not do: the overhang is material (`fitted_span` has what that is
/// worth) and nothing in the answer says it is there.
#[derive(Clone, Debug, PartialEq)]
pub struct Misfit {
    /// Per axis, how much longer the part is solved than it is, as a fraction of its own size.
    /// `None` on an axis with no flat end to land on, and on one the lattice already divides.
    pub oversize: [Option<f64>; 3],
    /// A count that would have landed on every axis that wants it, looked for further out than
    /// the fit itself will go: what to ask for instead. `None` where there is no such count.
    pub would_fit: Option<usize>,
}

/// How far `grid` is from landing on `parts`, or `None` where it lands on all of them.
///
/// The lattice has `ceil(size/h)` cells along each axis and the overhang is split between the
/// two ends, so each end cell holds `h - overhang/2` of material. The overhang is under one cell
/// by construction, so an end cell is always over half full, and `least_fill` is at most a half:
/// every one of those cells is solved as material whatever the setting, and the part comes out
/// `ceil(size/h) * h` long on that axis.
pub fn misfit(parts: &[&Mesh], grid: &Grid3, span: usize) -> Option<Misfit> {
    let mut bounds = Aabb3::EMPTY;
    for m in parts {
        bounds = bounds.union(&m.bounds());
    }
    if bounds.is_empty() {
        return None;
    }
    let size = bounds.size();
    let h = grid.spacing();
    if !size.is_finite() || !(h > 0.0) || !h.is_finite() {
        return None;
    }
    let wanted = flat_ends(parts, &bounds);
    let extents = [size.x, size.y, size.z];
    let mut oversize = [None; 3];
    for axis in 0..3 {
        let (Some(&s), Some(&want)) = (extents.get(axis), wanted.get(axis)) else { continue };
        if !want || !(s > 0.0) || !s.is_finite() {
            continue;
        }
        let across = s / h;
        if !across.is_finite() || (across - across.round()).abs() <= 1e-9 * across.abs().max(1.0) {
            continue;
        }
        let solved = across.ceil() * h;
        if solved > s
            && let Some(slot) = oversize.get_mut(axis)
        {
            *slot = Some((solved - s) / s);
        }
    }
    if !oversize.iter().any(Option::is_some) {
        return None;
    }
    let longest = size.x.max(size.y).max(size.z);
    // Further than the fit will go, because this is advice rather than a lattice: somebody who
    // is told their part is solved a tenth too thick wants to know what to ask for, even if the
    // answer is half as many cells again.
    let would_fit = fitted_within(size, wanted, longest, span, span.max(16));
    Some(Misfit { oversize, would_fit })
}

/// Which axes the model has a flat end face square to, and so wants the lattice to land on.
///
/// An axis counts where the model has, at either end of its box, triangles square to that axis
/// covering a useful part of the box's cross-section there. A box answers all three, a cylinder
/// on its axis answers that axis alone, and a sphere answers none.
///
/// The threshold is a tenth of the cross-section: enough that a chamfer or a small flat spot on
/// an otherwise round end does not pull the whole lattice about, and little enough that a face
/// with a large hole in it or a few bosses standing off it still counts.
fn flat_ends(parts: &[&Mesh], bounds: &Aabb3) -> [bool; 3] {
    let size = bounds.size();
    let (min, max) = (bounds.min, bounds.max);
    // A face is "at the end" within a thousandth of the part, which is finer than any face the
    // tessellation places and coarser than the noise in it.
    let near = 1e-3 * size.x.max(size.y).max(size.z);
    let mut area = [[0.0f64; 2]; 3];
    for mesh in parts {
        for t in &mesh.triangles {
            let Some([a, b, c]) = mesh.tri(t) else { continue };
            let n = (b - a).cross(c - a);
            let twice = n.len();
            if !(twice > 0.0) || !twice.is_finite() {
                continue;
            }
            for axis in 0..3 {
                let Some(unit) = [n.x, n.y, n.z].get(axis).map(|v| v / twice) else { continue };
                // Square to the axis to within half a degree.
                if unit.abs() < 0.9999 {
                    continue;
                }
                let (Some(pa), Some(pb), Some(pc)) =
                    ([a.x, a.y, a.z].get(axis).copied(), [b.x, b.y, b.z].get(axis).copied(), [c.x, c.y, c.z].get(axis).copied())
                else {
                    continue;
                };
                let at = (pa + pb + pc) / 3.0;
                let (Some(lo), Some(hi)) = ([min.x, min.y, min.z].get(axis).copied(), [max.x, max.y, max.z].get(axis).copied()) else {
                    continue;
                };
                let end = if (at - lo).abs() <= near {
                    0
                } else if (at - hi).abs() <= near {
                    1
                } else {
                    continue;
                };
                if let Some(slot) = area.get_mut(axis).and_then(|e| e.get_mut(end)) {
                    *slot += twice / 2.0;
                }
            }
        }
    }
    std::array::from_fn(|axis| {
        // The box's cross-section across this axis, which a whole flat end would cover.
        let across = match axis {
            0 => size.y * size.z,
            1 => size.x * size.z,
            _ => size.x * size.y,
        };
        if !(across > 0.0) {
            return false;
        }
        area.get(axis).is_some_and(|ends| ends.iter().any(|a| *a >= 0.1 * across))
    })
}

/// How many cells to put across the longest side so that the model's own box is a whole number
/// of cells across every axis it has a flat end face on, or `None` where no count near the one
/// asked for manages it.
///
/// This matters far more than it looks, and it is the largest single source of error in a stress
/// answer on a part square to the axes. A lattice whose cells do not divide the part overhangs
/// it, and the half-full rule turns that overhang into material: a 20 mm bar on a 2.083 mm
/// lattice is solved as a 20.8 mm bar with a ragged edge. Measured on a bar pulled along its
/// axis on rollers -- the case whose answer is exactly the traction, 10 MPa -- the lattice reads
///
/// | cells across the 20 mm section | reads |
/// |---|---|
/// | 3.2 | 12.95 |
/// | 3.6 | 12.41 |
/// | 4 | 10.0000 |
/// | 4.8 | 9.22 |
/// | 6 | 10.0000 |
/// | 7.2 | 14.19 |
/// | 8 | 10.0000 |
/// | 9.6 | 14.08 |
///
/// -- exact to ten figures at every spacing that divides the section, and 8 per cent low to 42
/// per cent high at every spacing that does not, with no trend as the lattice is refined.
/// Refining from 7.2 cells across to 9.6 leaves the error exactly where it was. So the spacing
/// is worth moving by a few per cent to get a count that divides the part, and nothing else
/// here comes close to being worth as much (docs/analysis-verification.md).
///
/// The search runs outwards from the count asked for and takes the first that divides every
/// extent, so the spacing moves as little as it can: for a 100 x 20 x 20 bar, 48 cells across
/// becomes 50. It gives up rather than wander far -- a part whose extents have no common
/// measure near the count asked for keeps the count it was given, which is what there was
/// before this.
fn fitted_span(size: Vec3, wanted: [bool; 3], longest: f64, span: usize) -> Option<usize> {
    // Far enough to find the common measure of ordinary round sizes, near enough that the
    // lattice the caller asked for is still roughly the lattice they get.
    fitted_within(size, wanted, longest, span, (span / 4).max(1))
}

/// As [`fitted_span`], searching `reach` counts to either side of `span`.
fn fitted_within(size: Vec3, wanted: [bool; 3], longest: f64, span: usize, reach: usize) -> Option<usize> {
    if !wanted.iter().any(|w| *w) {
        return None;
    }
    if !(longest > 0.0) || !longest.is_finite() || span == 0 {
        return None;
    }
    let divides = |n: usize| {
        let h = longest / n as f64;
        if !(h > 0.0) || !h.is_finite() {
            return false;
        }
        // Within what f64 carries over the part's own size: these extents come from a
        // tessellation, not from the numbers somebody typed.
        let tol = 1e-9 * longest;
        [size.x, size.y, size.z].into_iter().zip(wanted).all(|(s, want)| {
            if !want {
                return true;
            }
            let whole = (s / h).round();
            whole >= 1.0 && whole.is_finite() && (s - whole * h).abs() <= tol
        })
    };
    // Outwards from the count asked for: the nearest that divides the part wins, and of a pair
    // the same distance away the finer. Somebody who asks for a spacing has asked for at least
    // that much resolution, so the tie goes to the lattice that gives it to them.
    (0..=reach).find_map(|d| [span.checked_add(d), span.checked_sub(d)].into_iter().flatten().find(|n| (1..=MAX_SPAN).contains(n) && divides(*n)))
}

#[cfg(test)]
#[path = "grid_tests.rs"]
mod tests;
