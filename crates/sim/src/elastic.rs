//! What a part does under load: where it moves, where it is stressed, and where it is close to
//! giving way.
//!
//! The method is linear elasticity by trilinear hexahedral finite elements -- the standard
//! approach for an analysis on a lattice, where every solid cell is already a brick with eight
//! corners and nothing has to be meshed. Each element's stiffness is integrated exactly as
//! textbooks give it (two Gauss points along each axis of `∫BᵀDB dV`), and the global operator
//! is **never assembled**: a multiplication gathers each element's twenty-four freedoms,
//! multiplies by its own twenty-four by twenty-four matrix and scatters the result back. That
//! keeps the memory to one small matrix per material however large the model is, and lets the
//! same conjugate-gradient iteration the conduction solver uses do the work, since elasticity's
//! operator is symmetric and positive definite for the same reason conduction's is.
//!
//! Loads come from three places, and they add: a pressure or a traction on a patch of surface,
//! the weight of the part itself, and -- the one that makes this worth having next to the
//! thermal solver -- **the expansion of a part that has been heated and is not free to grow**.
//! A temperature field from [`crate::thermal`] goes straight in, and what comes out is where
//! the part is being pulled apart by its own warmth.
//!
//! ## What this is honest about
//!
//! A plain trilinear brick is **too stiff in bending**: a field that bends wants the element's
//! sides to curve, they stay straight, and it resists with a shear that is not really there.
//! Each element therefore carries three bubble shapes of its own, condensed out of its stiffness
//! before anything else sees it (see [`element_stiffness`]), which takes a cantilever from more
//! than a seventh short of beam theory to under two per cent at the same spacing. What remains
//! is still short rather than over, as a displacement-based element must be, and still wants a
//! finer lattice for a deflection than for a stress; `docs/analysis-verification.md` records how
//! much, measured.
//!
//! Everything a *uniform* strain field does -- stretching, pressure, free thermal growth,
//! constrained thermal stress -- the element reproduces **exactly**, bubble shapes and all, and
//! those are the cases the tests hold to equality rather than to a bound.

use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::grid::{Occupancy, SurfaceFace};
use crate::linalg::{CgReport, Symmetric};
use crate::thermal::Patch;
use crate::{Result, SimError, setup};

/// Freedoms an element has: eight corners, three each.
const DOFS: usize = 24;

/// Freedoms of the element's own, which no neighbour shares: three bubble shapes, three
/// directions each. See [`element_stiffness`].
const MODES: usize = 9;

/// Columns of the full strain matrix: the corners' freedoms and the element's own.
const FULL: usize = DOFS + MODES;

/// The corners of a hexahedron in the reference cube, in the usual order: the bottom face
/// anticlockwise from `(−1, −1, −1)`, then the top face above it.
pub(crate) const CORNERS: [[f64; 3]; 8] = [
    [-1.0, -1.0, -1.0],
    [1.0, -1.0, -1.0],
    [1.0, 1.0, -1.0],
    [-1.0, 1.0, -1.0],
    [-1.0, -1.0, 1.0],
    [1.0, -1.0, 1.0],
    [1.0, 1.0, 1.0],
    [-1.0, 1.0, 1.0],
];

/// Where the two-point Gauss rule samples, along one axis. Two points integrate a cubic
/// exactly, which is what `BᵀDB` on a trilinear element is.
const GAUSS: [f64; 2] = [-0.577_350_269_189_625_8, 0.577_350_269_189_625_8];

/// What a solid resists being deformed by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElasticMaterial {
    pub young_modulus_pa: f64,
    /// Between −1 and a half. A half exactly is incompressible, which this cannot represent.
    pub poisson_ratio: f64,
    /// Linear expansion per kelvin along the model's axes.
    pub expansion_per_k: [f64; 3],
    pub density_kg_m3: f64,
    /// Where it gives way. Without it there is no safety factor to report.
    pub yield_strength_pa: Option<f64>,
}

impl ElasticMaterial {
    /// A material with a stiffness and nothing else: enough for a load case with no heat and
    /// no weight.
    pub fn new(young_modulus_pa: f64, poisson_ratio: f64) -> ElasticMaterial {
        ElasticMaterial { young_modulus_pa, poisson_ratio, expansion_per_k: [0.0; 3], density_kg_m3: 0.0, yield_strength_pa: None }
    }

    /// Lamé's two constants: `λ` and the shear modulus `μ`.
    pub fn lame(&self) -> (f64, f64) {
        let (e, nu) = (self.young_modulus_pa, self.poisson_ratio);
        let lambda = e * nu / ((1.0 + nu) * (1.0 - 2.0 * nu));
        let mu = e / (2.0 * (1.0 + nu));
        (lambda, mu)
    }

    fn check(&self, which: usize) -> Result<()> {
        if !(self.young_modulus_pa > 0.0) || !self.young_modulus_pa.is_finite() {
            return Err(SimError::Material(format!(
                "body {which} has a Young's modulus of {} Pa; it must be a positive number",
                self.young_modulus_pa
            )));
        }
        let nu = self.poisson_ratio;
        if !nu.is_finite() || nu <= -1.0 || nu >= 0.5 {
            return Err(SimError::Material(format!(
                "body {which} has a Poisson's ratio of {nu}; a stable isotropic material has it between −1 and a half, and a \
                 half exactly is incompressible, which this cannot represent"
            )));
        }
        for a in self.expansion_per_k {
            if !a.is_finite() {
                return Err(SimError::Material(format!("body {which} has an expansion that is not a number")));
            }
        }
        if !(self.density_kg_m3 >= 0.0) || !self.density_kg_m3.is_finite() {
            return Err(SimError::Material(format!("body {which} has a density of {}", self.density_kg_m3)));
        }
        if let Some(y) = self.yield_strength_pa
            && (!(y > 0.0) || !y.is_finite())
        {
            return Err(SimError::Material(format!("body {which} yields at {y} Pa; that is not a strength")));
        }
        Ok(())
    }
}

/// What holds a patch of the surface, or pushes on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Support {
    /// Held still in all three directions: bolted, welded, clamped.
    Fixed,
    /// Held still across the surface, free to slide along it. A plane of symmetry, or a face
    /// resting on something flat.
    Roller,
    /// Pressure in pascals, positive pushing into the model.
    Pressure(f64),
    /// A traction in pascals along the model's axes: a pull, a shear, a distributed load.
    Traction([f64; 3]),
    /// Nothing holds it and nothing pushes on it.
    Free,
}

/// A patch of surface and what happens on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Load {
    pub name: String,
    pub patch: Patch,
    pub support: Support,
}

impl Load {
    pub fn new(name: impl Into<String>, patch: Patch, support: Support) -> Load {
        Load { name: name.into(), patch, support }
    }
}

/// How hard the solver tries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub tolerance: f64,
    pub max_iterations: u32,
}

impl Default for Limits {
    fn default() -> Limits {
        // Elasticity's operator is worse conditioned than conduction's -- three freedoms a node
        // and twenty-six neighbours -- so it wants more iterations and settles for a little
        // less.
        Limits { tolerance: 1e-9, max_iterations: 100_000 }
    }
}

/// A load case.
#[derive(Clone, Debug)]
pub struct Study {
    /// One entry per part, in the order the occupancy's parts were given.
    pub materials: Vec<ElasticMaterial>,
    /// Tried in order; the first load that claims a face owns it.
    pub loads: Vec<Load>,
    /// How much warmer each solid cell is than the temperature it is unstressed at, in kelvin,
    /// one per equation of the occupancy. Empty for a case with no heat in it.
    ///
    /// This is where a thermal study's answer goes: subtract the temperature the part was made
    /// at and hand the difference over.
    pub temperature_rise_k: Vec<f64>,
    /// Gravity along the model's axes, m/s². `[0, 0, -9.80665]` for a part standing up.
    pub gravity_m_s2: [f64; 3],
    /// Where the lattice's nodes have been pulled to, where the study is to be run on elements
    /// that follow the model's surface rather than step across it (see [`crate::snap`]).
    ///
    /// `None` runs on the lattice, which is what everything did before this existed and what
    /// every exact answer in the suite is measured on.
    pub fitted: Option<crate::snap::Snapped>,
    /// How much to scale each surface face's area by, in [`Occupancy::surface`] order: what
    /// [`crate::tag::FaceTags::area_scale`] gives. Empty leaves every face the area of the
    /// lattice's own staircase, which over-states anything not square to an axis -- and with it
    /// every pressure and traction pushing on such a face.
    pub area_scale: Vec<f32>,
    pub limits: Limits,
}

impl Study {
    pub fn new(materials: Vec<ElasticMaterial>) -> Study {
        Study {
            materials,
            loads: Vec::new(),
            temperature_rise_k: Vec::new(),
            gravity_m_s2: [0.0; 3],
            fitted: None,
            area_scale: Vec::new(),
            limits: Limits::default(),
        }
    }

    pub fn with(mut self, load: Load) -> Study {
        self.loads.push(load);
        self
    }
}

/// Where a part is worst off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Worst {
    /// The equation, and so the cell, it is at.
    pub equation: usize,
    pub at: Vec3,
    pub value: f64,
}

/// What a load case came out as.
#[derive(Clone, Debug)]
pub struct Solution {
    /// Three numbers per node equation: how far that corner moved, in metres.
    pub displacement: Vec<f64>,
    /// Von Mises stress in each solid cell, in pascals.
    pub von_mises_pa: Vec<f64>,
    /// The full stress in each solid cell: `xx, yy, zz, xy, yz, zx`, in pascals.
    pub stress_pa: Vec<[f64; 6]>,
    pub linear: CgReport,
    pub worst_stress: Option<Worst>,
    /// The least `yield / von Mises` anywhere, and where. `None` when no material says where
    /// it gives way.
    pub least_safety_factor: Option<Worst>,
    /// How far the furthest-moved corner went, in metres, and where it started.
    pub most_movement: Option<Worst>,
    /// Every force applied to the part, added up, in newtons.
    pub applied_n: [f64; 3],
    /// Every reaction the supports provide, added up, in newtons.
    pub reaction_n: [f64; 3],
    /// What is left over as a fraction of the largest force in the case.
    ///
    /// A body in equilibrium has the applied forces and the reactions cancel exactly, and a
    /// rigid translation being in the operator's null space makes that an identity rather than
    /// an approximation. Anything here beyond the solver's own tolerance is a mistake.
    pub imbalance: f64,
}

impl Solution {
    /// The von Mises stress in a cell of the lattice, or `None` where the model is not.
    pub fn stress_at_cell(&self, occ: &Occupancy, cell: usize) -> Option<f64> {
        self.von_mises_pa.get(occ.eq_of(cell)?).copied()
    }
}

/// The corners of the solid cells, and which of them the solver solves for.
///
/// A node belongs to the problem when any of the up to eight cells around it is solid. The node
/// lattice is one larger than the cell lattice along each axis, which is the only reason this
/// is its own type rather than the occupancy's own numbering.
#[derive(Clone, Debug)]
pub struct Nodes {
    n: [usize; 3],
    eq: Vec<u32>,
    nodes: Vec<u32>,
}

const NO_NODE: u32 = u32::MAX;

/// How many cells a fitted study may keep a stiffness of its own for.
///
/// A cell that follows the surface cannot share its part's element, so it carries a whole
/// twenty-four by twenty-four of its own -- four and a half kilobytes. Only the cells on the
/// wall need one, which is a surface rather than a volume, but a fine lattice over a rough part
/// still gets to a great many of them. Past this the study says so and asks for fewer cells
/// rather than quietly taking a third of a gigabyte.
const MAX_FITTED_CELLS: usize = 64_000;

impl Nodes {
    pub fn of(occ: &Occupancy) -> Nodes {
        let cells = occ.grid().count();
        let n = [cells[0] + 1, cells[1] + 1, cells[2] + 1];
        let total = n[0] * n[1] * n[2];
        let mut used = vec![false; total];
        for e in 0..occ.equations() {
            let Some(cell) = occ.cell_of(e) else { continue };
            let Some(at) = occ.grid().ijk(cell) else { continue };
            for corner in CORNERS {
                let node = [at[0] + usize::from(corner[0] > 0.0), at[1] + usize::from(corner[1] > 0.0), at[2] + usize::from(corner[2] > 0.0)];
                if let Some(slot) = index(&n, node).and_then(|i| used.get_mut(i)) {
                    *slot = true;
                }
            }
        }
        let mut eq = vec![NO_NODE; total];
        let mut nodes = Vec::new();
        for (i, on) in used.iter().enumerate() {
            if !*on {
                continue;
            }
            if let Some(slot) = eq.get_mut(i) {
                *slot = nodes.len() as u32;
            }
            nodes.push(i as u32);
        }
        Nodes { n, eq, nodes }
    }

    pub fn count(&self) -> usize {
        self.nodes.len()
    }

    /// Freedoms: three per node.
    pub fn freedoms(&self) -> usize {
        self.nodes.len() * 3
    }

    /// The equation a node lattice position is solved by.
    pub fn at(&self, node: [usize; 3]) -> Option<usize> {
        match index(&self.n, node).and_then(|i| self.eq.get(i)).copied() {
            Some(NO_NODE) | None => None,
            Some(e) => Some(e as usize),
        }
    }

    /// Where a node equation sits on the node lattice.
    pub fn position(&self, eq: usize) -> Option<[usize; 3]> {
        let i = *self.nodes.get(eq)? as usize;
        let plane = self.n[0] * self.n[1];
        if plane == 0 {
            return None;
        }
        let k = i / plane;
        let rest = i - k * plane;
        Some([rest % self.n[0], rest / self.n[0], k])
    }

    /// The eight node equations of a cell, in [`CORNERS`] order.
    pub fn of_cell(&self, occ: &Occupancy, cell: usize) -> Option<[usize; 8]> {
        let at = occ.grid().ijk(cell)?;
        let mut out = [0usize; 8];
        for (c, corner) in CORNERS.iter().enumerate() {
            let node = [at[0] + usize::from(corner[0] > 0.0), at[1] + usize::from(corner[1] > 0.0), at[2] + usize::from(corner[2] > 0.0)];
            *out.get_mut(c)? = self.at(node)?;
        }
        Some(out)
    }

    /// The four node equations of one face of one cell.
    pub fn of_face(&self, occ: &Occupancy, face: SurfaceFace) -> Option<[usize; 4]> {
        let at = occ.grid().ijk(face.cell as usize)?;
        let (axis, far) = (face.axis(), face.positive());
        let mut out = Vec::with_capacity(4);
        for corner in CORNERS {
            if (corner.get(axis).copied().unwrap_or(0.0) > 0.0) != far {
                continue;
            }
            let node = [at[0] + usize::from(corner[0] > 0.0), at[1] + usize::from(corner[1] > 0.0), at[2] + usize::from(corner[2] > 0.0)];
            out.push(self.at(node)?);
        }
        match out[..] {
            [a, b, c, d] => Some([a, b, c, d]),
            _ => None,
        }
    }
}

fn index(n: &[usize; 3], at: [usize; 3]) -> Option<usize> {
    if at[0] >= n[0] || at[1] >= n[1] || at[2] >= n[2] {
        return None;
    }
    Some(at[0] + n[0] * (at[1] + n[1] * at[2]))
}

/// The six-by-six elasticity matrix of an isotropic material, in the order
/// `xx, yy, zz, xy, yz, zx` with engineering shear strains.
pub(crate) fn elasticity_matrix(material: &ElasticMaterial) -> [[f64; 6]; 6] {
    let (lambda, mu) = material.lame();
    let mut d = [[0.0f64; 6]; 6];
    for a in 0..3 {
        for b in 0..3 {
            if let Some(slot) = d.get_mut(a).and_then(|row| row.get_mut(b)) {
                *slot = lambda + if a == b { 2.0 * mu } else { 0.0 };
            }
        }
    }
    for a in 3..6 {
        if let Some(slot) = d.get_mut(a).and_then(|row| row.get_mut(a)) {
            *slot = mu;
        }
    }
    d
}

/// The strain-displacement matrix of a trilinear brick of side `h` at a point of the reference
/// cube: six rows of strain, twenty-four columns for the corners' freedoms and nine more for the
/// element's own (see [`element_stiffness`]).
fn strain_matrix_full(at: [f64; 3], h: f64) -> [[f64; FULL]; 6] {
    let mut b = [[0.0f64; FULL]; 6];
    // A cube maps from the reference cube with dx/dξ = h/2 along every axis, so a derivative
    // with respect to the reference becomes one with respect to space on multiplying by 2/h.
    let scale = 2.0 / h;
    // A displacement in direction `axis` whose spatial gradient is `d` puts these entries in the
    // six rows of strain. A corner's shape function and one of the element's own both go through
    // here, because the only thing that differs between them is the gradient.
    let put = |b: &mut [[f64; FULL]; 6], column: usize, axis: usize, d: [f64; 3]| {
        let rows: [(usize, f64); 3] = match axis {
            0 => [(0, d[0]), (3, d[1]), (5, d[2])],
            1 => [(1, d[1]), (3, d[0]), (4, d[2])],
            _ => [(2, d[2]), (4, d[1]), (5, d[0])],
        };
        for (row, v) in rows {
            if let Some(slot) = b.get_mut(row).and_then(|r| r.get_mut(column)) {
                *slot += v;
            }
        }
    };

    for (i, corner) in CORNERS.iter().enumerate() {
        let mut d = [0.0f64; 3];
        for a in 0..3 {
            let mut v = corner.get(a).copied().unwrap_or(0.0) / 8.0;
            for other in 0..3 {
                if other == a {
                    continue;
                }
                let (Some(c), Some(x)) = (corner.get(other), at.get(other)) else { continue };
                v *= 1.0 + c * x;
            }
            if let Some(slot) = d.get_mut(a) {
                *slot = v * scale;
            }
        }
        for axis in 0..3 {
            put(&mut b, i * 3 + axis, axis, d);
        }
    }

    // The element's own three shapes, `1 − ξ²` along each axis. They vanish on every face, so no
    // neighbour sees them and no freedom is shared; what they add is the ability to curve, which
    // is exactly what a trilinear element lacks and what makes it stiff in bending.
    for m in 0..3 {
        let mut d = [0.0f64; 3];
        if let (Some(slot), Some(x)) = (d.get_mut(m), at.get(m)) {
            *slot = -2.0 * x * scale;
        }
        for axis in 0..3 {
            put(&mut b, DOFS + m * 3 + axis, axis, d);
        }
    }
    b
}

/// Just the corners' columns, for reading a strain back out of a solved displacement.
fn strain_matrix(at: [f64; 3], h: f64) -> [[f64; DOFS]; 6] {
    let full = strain_matrix_full(at, h);
    let mut b = [[0.0f64; DOFS]; 6];
    for (row, out) in full.iter().zip(b.iter_mut()) {
        for (v, slot) in row.iter().zip(out.iter_mut()) {
            *slot = *v;
        }
    }
    b
}

/// Cholesky's lower factor of a symmetric positive definite matrix, or `None` when it is not
/// one.
fn cholesky<const N: usize>(a: &[[f64; N]; N]) -> Option<[[f64; N]; N]> {
    let mut l = [[0.0f64; N]; N];
    for i in 0..N {
        for j in 0..=i {
            let mut sum = *a.get(i)?.get(j)?;
            for k in 0..j {
                sum -= l.get(i)?.get(k)? * l.get(j)?.get(k)?;
            }
            if i == j {
                if !(sum > 0.0) || !sum.is_finite() {
                    return None;
                }
                *l.get_mut(i)?.get_mut(j)? = sum.sqrt();
            } else {
                let d = *l.get(j)?.get(j)?;
                if !(d.abs() > 0.0) {
                    return None;
                }
                *l.get_mut(i)?.get_mut(j)? = sum / d;
            }
        }
    }
    Some(l)
}

/// Solve `L·Lᵀ·x = b` for one right-hand side.
fn solve_cholesky<const N: usize>(l: &[[f64; N]; N], b: &[f64; N]) -> Option<[f64; N]> {
    let mut y = [0.0f64; N];
    for i in 0..N {
        let mut sum = *b.get(i)?;
        for k in 0..i {
            sum -= l.get(i)?.get(k)? * y.get(k)?;
        }
        *y.get_mut(i)? = sum / l.get(i)?.get(i)?;
    }
    let mut x = [0.0f64; N];
    for i in (0..N).rev() {
        let mut sum = *y.get(i)?;
        for k in (i + 1)..N {
            sum -= l.get(k)?.get(i)? * x.get(k)?;
        }
        *x.get_mut(i)? = sum / l.get(i)?.get(i)?;
    }
    Some(x)
}

/// A brick's stiffness: `∫BᵀDB dV` over a cube of side `h`, by two Gauss points along each
/// axis, with the element's own bubble modes condensed out.
///
/// Two points are not a short cut: `BᵀDB` on a trilinear element is a cubic in each reference
/// coordinate, and a two-point rule integrates a cubic exactly, so the integral is exact.
///
/// The condensation is what makes this element worth having. A trilinear brick's sides stay
/// straight, so a field that bends makes it shear when it should not and it comes out far
/// stiffer than the part is. Wilson's remedy (Wilson, Taylor, Doherty and Ghaboussi, 1973) gives
/// each element three shapes of its own, `1 − ξ²` along each axis, which vanish on every face
/// and so are shared with no neighbour. Writing the full stiffness as
///
/// ```text
/// [ K_uu  K_ua ] [ u ]   [ f ]
/// [ K_au  K_aa ] [ a ] = [ 0 ]
/// ```
///
/// -- nothing loads the element's own freedoms -- the second row gives `a = −K_aa⁻¹·K_au·u`, and
/// substituting leaves `(K_uu − K_ua·K_aa⁻¹·K_au)·u = f`. That is a twenty-four by twenty-four
/// matrix again, so everything downstream of here is unchanged; the element simply stopped being
/// stiff. On a cube the Jacobian is constant, which is the condition under which these elements
/// still reproduce a uniform strain exactly -- so every case that was exact stays exact, which
/// the tests hold them to.
pub fn element_stiffness(material: &ElasticMaterial, h: f64) -> [[f64; DOFS]; DOFS] {
    let d = elasticity_matrix(material);
    let weight = (h / 2.0).powi(3);
    let mut full = [[0.0f64; FULL]; FULL];
    for x in GAUSS {
        for y in GAUSS {
            for z in GAUSS {
                let b = strain_matrix_full([x, y, z], h);
                // db = D·B, then K += Bᵀ·db·|J|.
                let mut db = [[0.0f64; FULL]; 6];
                for r in 0..6 {
                    for c in 0..FULL {
                        let mut v = 0.0;
                        for s in 0..6 {
                            let (Some(drs), Some(bsc)) = (d.get(r).and_then(|x| x.get(s)), b.get(s).and_then(|x| x.get(c))) else {
                                continue;
                            };
                            v += drs * bsc;
                        }
                        if let Some(slot) = db.get_mut(r).and_then(|x| x.get_mut(c)) {
                            *slot = v;
                        }
                    }
                }
                for i in 0..FULL {
                    for j in 0..FULL {
                        let mut v = 0.0;
                        for r in 0..6 {
                            let (Some(bri), Some(drj)) = (b.get(r).and_then(|x| x.get(i)), db.get(r).and_then(|x| x.get(j))) else {
                                continue;
                            };
                            v += bri * drj;
                        }
                        if let Some(slot) = full.get_mut(i).and_then(|x| x.get_mut(j)) {
                            *slot += v * weight;
                        }
                    }
                }
            }
        }
    }
    condense(&full)
}

/// The map from the reference cube to where an element actually is, at one point: its inverse
/// and its determinant, which are the two things an integral over the element needs.
///
/// `det` is the volume a unit of reference cube stands for there, so it is the weight; the
/// inverse turns a derivative with respect to the reference into one with respect to space.
/// On a cube both are constant -- `det = (h/2)³`, the inverse `2/h` down the diagonal -- which
/// is why [`element_stiffness`] can do without either.
#[derive(Clone, Copy, Debug)]
struct Jacobian {
    inverse: [[f64; 3]; 3],
    det: f64,
}

/// `dN_i/dξ` for every corner at a reference point: the gradient of the trilinear shape
/// functions, still in the reference cube's own coordinates.
fn corner_gradients(at: [f64; 3]) -> Option<[[f64; 3]; 8]> {
    let mut g = [[0.0f64; 3]; 8];
    for (i, corner) in CORNERS.iter().enumerate() {
        for a in 0..3 {
            let mut v = *corner.get(a)? / 8.0;
            for b in 0..3 {
                if b != a {
                    v *= 1.0 + corner.get(b)? * at.get(b)?;
                }
            }
            *g.get_mut(i)?.get_mut(a)? = v;
        }
    }
    Some(g)
}

/// The element's map at a reference point, from where its eight corners are.
///
/// `None` where the element is turned inside out or squashed flat: a determinant that is zero or
/// negative is a hexahedron whose corners cross over, and no integral over it means anything.
/// Returning nothing rather than going on is what keeps a bad cell out of the solve instead of
/// letting it poison the answer.
fn jacobian(at: [f64; 3], corners: &[Vec3; 8]) -> Option<Jacobian> {
    let g = corner_gradients(at)?;
    // j[a][b] = dx_b/dξ_a.
    let mut j = [[0.0f64; 3]; 3];
    for (i, x) in corners.iter().enumerate() {
        let gi = g.get(i)?;
        for a in 0..3 {
            let ga = *gi.get(a)?;
            let row = j.get_mut(a)?;
            *row.get_mut(0)? += ga * x.x;
            *row.get_mut(1)? += ga * x.y;
            *row.get_mut(2)? += ga * x.z;
        }
    }
    let det = j[0][0] * (j[1][1] * j[2][2] - j[1][2] * j[2][1]) - j[0][1] * (j[1][0] * j[2][2] - j[1][2] * j[2][0])
        + j[0][2] * (j[1][0] * j[2][1] - j[1][1] * j[2][0]);
    if !(det > 0.0) || !det.is_finite() {
        return None;
    }
    let mut inverse = [[0.0f64; 3]; 3];
    for a in 0..3 {
        for b in 0..3 {
            // The cofactor of (b, a), transposed in place: the adjugate over the determinant.
            let (r1, r2) = ((b + 1) % 3, (b + 2) % 3);
            let (c1, c2) = ((a + 1) % 3, (a + 2) % 3);
            let cof = j.get(r1)?.get(c1)? * j.get(r2)?.get(c2)? - j.get(r1)?.get(c2)? * j.get(r2)?.get(c1)?;
            *inverse.get_mut(a)?.get_mut(b)? = cof / det;
        }
    }
    Some(Jacobian { inverse, det })
}

/// The determinant of a hexahedron's map at a reference point: the volume a unit of reference
/// cube stands for there, and `None` where the element has folded through itself.
///
/// This is what a snap has to check. Moving a node onto the surface is only safe while every
/// element around it still maps the right way out, and the determinant is the whole of that
/// question -- see [`crate::snap`].
pub(crate) fn jacobian_det(at: [f64; 3], corners: &[Vec3; 8]) -> Option<f64> {
    jacobian(at, corners).map(|j| j.det)
}

/// A gradient in the reference cube turned into one in space: `J⁻¹ · g`.
fn to_space(inverse: &[[f64; 3]; 3], g: [f64; 3]) -> [f64; 3] {
    let mut d = [0.0f64; 3];
    for (b, slot) in d.iter_mut().enumerate() {
        let mut v = 0.0;
        for (a, ga) in g.iter().enumerate() {
            if let Some(row) = inverse.get(b) {
                v += row.get(a).copied().unwrap_or(0.0) * ga;
            }
        }
        *slot = v;
    }
    d
}

/// The strain-displacement matrix of a hexahedron that is not a cube, at a reference point, with
/// the determinant that weights it.
///
/// The corners go through the element's own map. The three bubble shapes do not: they go through
/// the map taken at the element's *centre*, scaled by `det(J₀)/det(J)`. That is Taylor,
/// Beresford and Wilson's correction (1976), and without it this element stops reproducing a
/// uniform strain the moment it is not a cube -- which is to say it stops being usable for the
/// one thing the bubbles were added for. The correction costs a 3×3 inverse per element and
/// changes nothing on a cube, where `J` does not vary and the ratio is one.
fn strain_matrix_hex(at: [f64; 3], corners: &[Vec3; 8], centre: &Jacobian) -> Option<([[f64; FULL]; 6], f64)> {
    let j = jacobian(at, corners)?;
    let mut b = [[0.0f64; FULL]; 6];
    let put = |b: &mut [[f64; FULL]; 6], column: usize, axis: usize, d: [f64; 3]| {
        let rows: [(usize, f64); 3] = match axis {
            0 => [(0, d[0]), (3, d[1]), (5, d[2])],
            1 => [(1, d[1]), (3, d[0]), (4, d[2])],
            _ => [(2, d[2]), (4, d[1]), (5, d[0])],
        };
        for (row, v) in rows {
            if let Some(slot) = b.get_mut(row).and_then(|r| r.get_mut(column)) {
                *slot += v;
            }
        }
    };

    let g = corner_gradients(at)?;
    for i in 0..8 {
        let d = to_space(&j.inverse, *g.get(i)?);
        for axis in 0..3 {
            put(&mut b, i * 3 + axis, axis, d);
        }
    }

    let ratio = centre.det / j.det;
    for m in 0..3 {
        let mut g = [0.0f64; 3];
        *g.get_mut(m)? = -2.0 * at.get(m)?;
        let mut d = to_space(&centre.inverse, g);
        for v in d.iter_mut() {
            *v *= ratio;
        }
        for axis in 0..3 {
            put(&mut b, DOFS + m * 3 + axis, axis, d);
        }
    }
    Some((b, j.det))
}

/// A hexahedron's stiffness from where its eight corners are, rather than from a side length.
///
/// This is [`element_stiffness`] without the assumption that the element is a cube, and it is
/// what lets the lattice's boundary cells be pulled onto the model's own surface instead of
/// standing as a staircase. Everything else is the same: two Gauss points an axis, the element's
/// own bubble shapes condensed out, the same twenty-four by twenty-four matrix at the end.
///
/// `None` for a hexahedron turned inside out, which a snap must never make and must be caught if
/// it does.
///
/// Corners are in [`CORNERS`] order: the `-ζ` face anticlockwise seen from outside, then the
/// `+ζ` face the same way.
pub fn element_stiffness_hex(material: &ElasticMaterial, corners: &[Vec3; 8]) -> Option<[[f64; DOFS]; DOFS]> {
    let d = elasticity_matrix(material);
    let centre = jacobian([0.0; 3], corners)?;
    let mut full = [[0.0f64; FULL]; FULL];
    for x in GAUSS {
        for y in GAUSS {
            for z in GAUSS {
                let (b, det) = strain_matrix_hex([x, y, z], corners, &centre)?;
                let mut db = [[0.0f64; FULL]; 6];
                for r in 0..6 {
                    for c in 0..FULL {
                        let mut v = 0.0;
                        for t in 0..6 {
                            let (Some(drt), Some(btc)) = (d.get(r).and_then(|x| x.get(t)), b.get(t).and_then(|x| x.get(c))) else {
                                continue;
                            };
                            v += drt * btc;
                        }
                        if let Some(slot) = db.get_mut(r).and_then(|x| x.get_mut(c)) {
                            *slot = v;
                        }
                    }
                }
                for i in 0..FULL {
                    for jj in 0..FULL {
                        let mut v = 0.0;
                        for r in 0..6 {
                            let (Some(bri), Some(drj)) = (b.get(r).and_then(|x| x.get(i)), db.get(r).and_then(|x| x.get(jj))) else {
                                continue;
                            };
                            v += bri * drj;
                        }
                        if let Some(slot) = full.get_mut(i).and_then(|x| x.get_mut(jj)) {
                            *slot += v * det;
                        }
                    }
                }
            }
        }
    }
    Some(condense(&full))
}

/// Freedoms of a tetrahedron: four corners, three each.
pub const TET_DOFS: usize = 12;

/// A tetrahedron's strain-displacement matrix, which is the same everywhere in it.
///
/// A linear tetrahedron's displacement is linear, so its strain is constant -- there is no
/// reference point to ask for and no quadrature rule to choose. That is the whole of why this
/// element is worth having beside the brick: a cell the surface cuts can be filled with
/// tetrahedra whose faces lie *on* that surface, where a brick can only stand a sliver past it.
///
/// `None` for a tetrahedron with no volume, or one wound the other way: both make the gradients
/// meaningless, and the second quietly flips the sign of everything built from them.
pub(crate) fn tet_strain_matrix_of(corners: &[Vec3; 4]) -> Option<([[f64; TET_DOFS]; 6], f64)> {
    let (p0, p1, p2, p3) = (corners[0], corners[1], corners[2], corners[3]);
    // x = p0 + J*xi, so d(xi)/dx = J inverse, whose rows are the gradients of the three
    // barycentric coordinates; the fourth is minus their sum.
    let j = [[p1.x - p0.x, p1.y - p0.y, p1.z - p0.z], [p2.x - p0.x, p2.y - p0.y, p2.z - p0.z], [p3.x - p0.x, p3.y - p0.y, p3.z - p0.z]];
    let det = j[0][0] * (j[1][1] * j[2][2] - j[1][2] * j[2][1]) - j[0][1] * (j[1][0] * j[2][2] - j[1][2] * j[2][0])
        + j[0][2] * (j[1][0] * j[2][1] - j[1][1] * j[2][0]);
    if !(det > 0.0) || !det.is_finite() {
        return None;
    }
    let mut inv = [[0.0f64; 3]; 3];
    for a in 0..3 {
        for b in 0..3 {
            let (r1, r2) = ((b + 1) % 3, (b + 2) % 3);
            let (c1, c2) = ((a + 1) % 3, (a + 2) % 3);
            let cof = j.get(r1)?.get(c1)? * j.get(r2)?.get(c2)? - j.get(r1)?.get(c2)? * j.get(r2)?.get(c1)?;
            *inv.get_mut(a)?.get_mut(b)? = cof / det;
        }
    }
    // Gradient of each corner's shape function: rows of the inverse for 1, 2, 3; minus their
    // sum for 0, since the four add to one everywhere.
    let mut grad = [[0.0f64; 3]; 4];
    for i in 0..3 {
        for a in 0..3 {
            let v = *inv.get(a)?.get(i)?;
            *grad.get_mut(i + 1)?.get_mut(a)? = v;
            *grad.get_mut(0)?.get_mut(a)? -= v;
        }
    }

    let mut b = [[0.0f64; TET_DOFS]; 6];
    for (i, d) in grad.iter().enumerate() {
        for axis in 0..3 {
            let rows: [(usize, f64); 3] = match axis {
                0 => [(0, d[0]), (3, d[1]), (5, d[2])],
                1 => [(1, d[1]), (3, d[0]), (4, d[2])],
                _ => [(2, d[2]), (4, d[1]), (5, d[0])],
            };
            for (row, v) in rows {
                if let Some(slot) = b.get_mut(row).and_then(|r| r.get_mut(i * 3 + axis)) {
                    *slot += v;
                }
            }
        }
    }
    Some((b, det / 6.0))
}

/// A tetrahedron's volume, or `None` where it has none or is wound the other way.
///
/// The winding matters and is not a detail: the four corners describe the same shape either way
/// round, but only one of them has a positive determinant, and everything built from the
/// gradients changes sign with it. [`crate::cut`] uses this to settle which way it produced one.
pub fn tet_volume(corners: &[Vec3; 4]) -> Option<f64> {
    tet_strain_matrix_of(corners).map(|(_, v)| v)
}

/// A tetrahedron's stiffness: `V·BᵀDB`, exactly, no quadrature.
///
/// Beside [`element_stiffness_hex`] this looks crude -- a constant strain across the whole
/// element, where the brick carries bending shapes of its own. It is not here to be better than
/// the brick. It is here because a tetrahedron can have a face *on* the model's surface, and the
/// measurements say that is what the lattice's remaining error at a curved wall is: a conforming
/// mesh of first-order tetrahedra reads 3.242 on the hole against the lattice's 3.59, and a
/// second-order one reads 3.249, so the order was never what was missing
/// (docs/analysis-verification.md).
///
/// Corners wind so that the fourth is on the side the first three turn towards; the other way
/// round gives `None`, as does a tetrahedron squashed flat.
pub fn tet_stiffness(material: &ElasticMaterial, corners: &[Vec3; 4]) -> Option<[[f64; TET_DOFS]; TET_DOFS]> {
    let d = elasticity_matrix(material);
    let (b, volume) = tet_strain_matrix_of(corners)?;
    let mut db = [[0.0f64; TET_DOFS]; 6];
    for r in 0..6 {
        for c in 0..TET_DOFS {
            let mut v = 0.0;
            for t in 0..6 {
                let (Some(drt), Some(btc)) = (d.get(r).and_then(|x| x.get(t)), b.get(t).and_then(|x| x.get(c))) else {
                    continue;
                };
                v += drt * btc;
            }
            if let Some(slot) = db.get_mut(r).and_then(|x| x.get_mut(c)) {
                *slot = v;
            }
        }
    }
    let mut k = [[0.0f64; TET_DOFS]; TET_DOFS];
    for i in 0..TET_DOFS {
        for jj in 0..TET_DOFS {
            let mut v = 0.0;
            for r in 0..6 {
                let (Some(bri), Some(drj)) = (b.get(r).and_then(|x| x.get(i)), db.get(r).and_then(|x| x.get(jj))) else {
                    continue;
                };
                v += bri * drj;
            }
            if let Some(slot) = k.get_mut(i).and_then(|x| x.get_mut(jj)) {
                *slot = v * volume;
            }
        }
    }
    Some(k)
}

/// Take the element's own freedoms back out of its stiffness. See [`element_stiffness`].
fn condense(full: &[[f64; FULL]; FULL]) -> [[f64; DOFS]; DOFS] {
    let mut k = [[0.0f64; DOFS]; DOFS];
    for i in 0..DOFS {
        for j in 0..DOFS {
            if let (Some(slot), Some(v)) = (k.get_mut(i).and_then(|r| r.get_mut(j)), full.get(i).and_then(|r| r.get(j))) {
                *slot = *v;
            }
        }
    }
    let mut kaa = [[0.0f64; MODES]; MODES];
    for i in 0..MODES {
        for j in 0..MODES {
            if let (Some(slot), Some(v)) = (kaa.get_mut(i).and_then(|r| r.get_mut(j)), full.get(DOFS + i).and_then(|r| r.get(DOFS + j))) {
                *slot = *v;
            }
        }
    }
    // A material the element cannot be strained in gives no factor, and then there is nothing to
    // condense: the plain trilinear stiffness is returned, and the element is the stiffer one it
    // was before rather than a wrong one.
    let Some(l) = cholesky(&kaa) else { return k };
    for j in 0..DOFS {
        let mut column = [0.0f64; MODES];
        for m in 0..MODES {
            if let (Some(slot), Some(v)) = (column.get_mut(m), full.get(DOFS + m).and_then(|r| r.get(j))) {
                *slot = *v;
            }
        }
        let Some(x) = solve_cholesky(&l, &column) else { return k };
        for i in 0..DOFS {
            let mut v = 0.0;
            for m in 0..MODES {
                let (Some(kua), Some(xm)) = (full.get(i).and_then(|r| r.get(DOFS + m)), x.get(m)) else { continue };
                v += kua * xm;
            }
            if let Some(slot) = k.get_mut(i).and_then(|r| r.get_mut(j)) {
                *slot -= v;
            }
        }
    }
    k
}

/// The nodal forces a uniform temperature rise puts on a brick: `∫BᵀDε dV` with `ε` the free
/// expansion the rise would give.
///
/// A part free to grow feels these and no stress; a part that is held feels the stress instead.
/// Both come out of the same term, which is why one load vector covers the two.
pub fn thermal_forces(material: &ElasticMaterial, h: f64, rise_k: f64) -> [f64; DOFS] {
    let d = elasticity_matrix(material);
    let free = [material.expansion_per_k[0] * rise_k, material.expansion_per_k[1] * rise_k, material.expansion_per_k[2] * rise_k, 0.0, 0.0, 0.0];
    // The stress a fully held element would carry: D·ε_free.
    let mut held = [0.0f64; 6];
    for r in 0..6 {
        let mut v = 0.0;
        for s in 0..6 {
            let (Some(drs), Some(e)) = (d.get(r).and_then(|x| x.get(s)), free.get(s)) else { continue };
            v += drs * e;
        }
        if let Some(slot) = held.get_mut(r) {
            *slot = v;
        }
    }
    let weight = (h / 2.0).powi(3);
    let mut f = [0.0f64; DOFS];
    for x in GAUSS {
        for y in GAUSS {
            for z in GAUSS {
                let b = strain_matrix([x, y, z], h);
                for c in 0..DOFS {
                    let mut v = 0.0;
                    for r in 0..6 {
                        let (Some(brc), Some(s)) = (b.get(r).and_then(|x| x.get(c)), held.get(r)) else { continue };
                        v += brc * s;
                    }
                    if let Some(slot) = f.get_mut(c) {
                        *slot += v * weight;
                    }
                }
            }
        }
    }
    f
}

/// Von Mises stress from the six components.
///
/// The one number that says how close a ductile material is to giving way, because yielding
/// depends on the shear rather than on the pressure: a part squeezed equally from every side
/// carries no von Mises stress at all, however hard it is squeezed, and does not yield.
pub fn von_mises(s: [f64; 6]) -> f64 {
    let [xx, yy, zz, xy, yz, zx] = s;
    let normal = (xx - yy).powi(2) + (yy - zz).powi(2) + (zz - xx).powi(2);
    let shear = xy * xy + yz * yz + zx * zx;
    (0.5 * normal + 3.0 * shear).max(0.0).sqrt()
}

/// The elasticity operator, never assembled.
pub struct Operator<'a> {
    occ: &'a Occupancy,
    nodes: &'a Nodes,
    /// One element stiffness per part, for the cells that are still cubes.
    element: Vec<[[f64; DOFS]; DOFS]>,
    /// The stiffness of a cell that has been pulled onto the surface, worked out for the shape
    /// it actually is. It needs no scaling: it is the integral over the material that is there.
    fitted: std::collections::HashMap<u32, Box<[[f64; DOFS]; DOFS]>>,
    /// Freedoms a support holds still.
    held: Vec<bool>,
    diag: Vec<f64>,
}

impl<'a> Operator<'a> {
    /// The stiffness to use for a cell and what to scale it by: a cell still on the lattice
    /// takes its part's element scaled by the material it carries, and one pulled onto the
    /// surface takes its own, which is already the stiffness of the shape it has.
    fn stiffness_of(&self, cell: usize) -> Option<(&[[f64; DOFS]; DOFS], f64)> {
        match self.fitted.get(&(cell as u32)) {
            Some(k) => Some((k, 1.0)),
            None => {
                let part = self.occ.part_of(cell).unwrap_or(0) as usize;
                let k = self.element.get(part).or_else(|| self.element.first())?;
                Some((k, self.occ.share_of(cell)))
            }
        }
    }

    fn new(
        occ: &'a Occupancy,
        nodes: &'a Nodes,
        element: Vec<[[f64; DOFS]; DOFS]>,
        fitted: std::collections::HashMap<u32, Box<[[f64; DOFS]; DOFS]>>,
        held: Vec<bool>,
    ) -> Operator<'a> {
        // The diagonal is what Jacobi needs, and for an operator applied element by element it
        // is the one part worth gathering up front: each element adds its own diagonal entry to
        // each of its freedoms.
        let mut diag = vec![0.0; nodes.freedoms()];
        let mut this = Operator { occ, nodes, element, fitted, held, diag: Vec::new() };
        for e in 0..occ.equations() {
            let Some(cell) = occ.cell_of(e) else { continue };
            let Some(corners) = nodes.of_cell(occ, cell) else { continue };
            let Some((k, share)) = this.stiffness_of(cell) else { continue };
            for (local, node) in corners.iter().enumerate() {
                for axis in 0..3 {
                    let row = local * 3 + axis;
                    let Some(v) = k.get(row).and_then(|r| r.get(row)) else { continue };
                    if let Some(slot) = diag.get_mut(node * 3 + axis) {
                        *slot += v * share;
                    }
                }
            }
        }
        for (dof, slot) in diag.iter_mut().enumerate() {
            if this.held.get(dof).copied().unwrap_or(false) || !(*slot > 0.0) {
                // A held freedom has an identity row, and a freedom no element reaches has
                // nothing to say; either way the diagonal is one, so Jacobi has something to
                // divide by.
                *slot = 1.0;
            }
        }
        this.diag = diag;
        this
    }

    /// Multiply without the identity rows a support puts in: the internal force the elements
    /// carry, which at a held freedom is the reaction.
    fn internal(&self, x: &[f64], out: &mut [f64]) -> Result<()> {
        if x.len() != self.diag.len() || out.len() != self.diag.len() {
            return setup("the solver was handed a vector of the wrong length");
        }
        out.fill(0.0);
        for e in 0..self.occ.equations() {
            let Some(cell) = self.occ.cell_of(e) else { continue };
            let Some(corners) = self.nodes.of_cell(self.occ, cell) else { continue };
            // Only the material in the cell resists. A cell still on the lattice is its part's
            // element scaled by what it carries -- a cell the model fills a third of is a third
            // as stiff, and one it fills whole is scaled by one, so a part square to the axes is
            // untouched and stays exact. A cell pulled onto the surface carries its own.
            let Some((k, share)) = self.stiffness_of(cell) else { continue };
            // Gather, multiply, scatter. A held freedom contributes nothing, which is what
            // takes its column out of the operator.
            let mut local = [0.0f64; DOFS];
            for (c, node) in corners.iter().enumerate() {
                for axis in 0..3 {
                    let dof = node * 3 + axis;
                    if self.held.get(dof).copied().unwrap_or(false) {
                        continue;
                    }
                    if let (Some(slot), Some(v)) = (local.get_mut(c * 3 + axis), x.get(dof)) {
                        *slot = *v;
                    }
                }
            }
            for (c, node) in corners.iter().enumerate() {
                for axis in 0..3 {
                    let row = c * 3 + axis;
                    let Some(krow) = k.get(row) else { continue };
                    let mut v = 0.0;
                    for (col, u) in krow.iter().zip(&local) {
                        v += col * u;
                    }
                    if let Some(slot) = out.get_mut(node * 3 + axis) {
                        *slot += v * share;
                    }
                }
            }
        }
        Ok(())
    }
}

impl Symmetric for Operator<'_> {
    fn equations(&self) -> usize {
        self.diag.len()
    }

    fn mul(&self, x: &[f64], out: &mut [f64]) -> Result<()> {
        self.internal(x, out)?;
        // A held freedom gets an identity row, so the operator stays symmetric and positive
        // definite and the answer at that freedom is whatever the right-hand side says, which
        // is nothing.
        for (dof, held) in self.held.iter().enumerate() {
            if !*held {
                continue;
            }
            let v = x.get(dof).copied().unwrap_or(0.0);
            if let Some(slot) = out.get_mut(dof) {
                *slot = v;
            }
        }
        Ok(())
    }

    fn diagonal(&self) -> &[f64] {
        &self.diag
    }
}

/// Solve a load case over an occupancy.
pub fn solve(occ: &Occupancy, study: &Study) -> Result<Solution> {
    if occ.equations() == 0 {
        return setup("nothing to solve: no cell of the lattice is inside the model");
    }
    if study.materials.is_empty() {
        return Err(SimError::Material("no material was given for the bodies".into()));
    }
    for (which, m) in study.materials.iter().enumerate() {
        m.check(which)?;
    }
    for g in study.gravity_m_s2 {
        if !g.is_finite() {
            return setup("a gravity that is not a number");
        }
    }
    if !study.temperature_rise_k.is_empty() && study.temperature_rise_k.len() != occ.equations() {
        return setup(format!(
            "the temperature rise has {} values and the lattice has {} cells inside the model",
            study.temperature_rise_k.len(),
            occ.equations()
        ));
    }
    if !study.temperature_rise_k.iter().all(|t| t.is_finite()) {
        return setup("a temperature rise that is not a number");
    }
    let limits = study.limits;
    if !(limits.tolerance > 0.0 && limits.tolerance < 1.0) {
        return setup("the solver's tolerance must be between 0 and 1");
    }
    for load in &study.loads {
        check_support(&load.support, &load.name)?;
    }

    let nodes = Nodes::of(occ);
    let freedoms = nodes.freedoms();
    if freedoms == 0 {
        return setup("nothing to solve: the model's cells have no corners on this lattice");
    }
    let h = occ.grid().spacing() / crate::grid::MM_PER_M;
    let element: Vec<[[f64; DOFS]; DOFS]> = study.materials.iter().map(|m| element_stiffness(m, h)).collect();

    // Which face of the surface each load owns, and so which freedoms are held and which are
    // pushed. Loads are tried in order and the first to claim a face keeps it, as a thermal
    // study's conditions are.
    let faces: Vec<SurfaceFace> = occ.surface().collect();
    if faces.is_empty() {
        return setup("the model has no surface on this lattice");
    }
    let owner = claim_faces(&faces, &study.loads)?;

    let mut held = vec![false; freedoms];
    let mut force = vec![0.0f64; freedoms];
    let area = occ.grid().face_area_m2();
    let mut applied = [0.0f64; 3];

    for (index, face) in faces.iter().enumerate() {
        let Some(load) = owner.get(index).copied().and_then(|o| study.loads.get(o)) else { continue };
        let Some(corners) = nodes.of_face(occ, *face) else { continue };
        match load.support {
            Support::Free => {}
            Support::Fixed => {
                for node in corners {
                    for axis in 0..3 {
                        if let Some(slot) = held.get_mut(node * 3 + axis) {
                            *slot = true;
                        }
                    }
                }
            }
            Support::Roller => {
                for node in corners {
                    if let Some(slot) = held.get_mut(node * 3 + face.axis()) {
                        *slot = true;
                    }
                }
            }
            Support::Pressure(pa) => {
                // Pressure pushes along the inward normal, so against the face's own way out.
                //
                // And it gets no area correction, which is not an oversight. The push of a
                // pressure is an area times a direction, and the sum of area times direction
                // over a patch of surface depends only on the rim of that patch -- so the
                // staircase, whose rim is the model's own, already pushes exactly as hard and
                // exactly which way the model's face does. Its steps are too large by the same
                // factor as they are tilted the wrong way, and the two cancel. Scaling it would
                // leave the push short by that factor instead.
                let out = face.outward();
                let traction = [-pa * out.x, -pa * out.y, -pa * out.z];
                spread(&mut force, &mut applied, &corners, traction, area);
            }
            // A traction is a force per unit area in a direction of its own, so nothing
            // cancels: it needs the area the model's face really has.
            Support::Traction(t) => spread(&mut force, &mut applied, &corners, t, area * scale_at(study, index)),
        }
    }

    // The weight of the part, and the push of its own warmth. Both are shared out over each
    // element's eight corners.
    let volume = occ.grid().cell_volume_m3();
    for e in 0..occ.equations() {
        let Some(cell) = occ.cell_of(e) else { continue };
        let Some(corners) = nodes.of_cell(occ, cell) else { continue };
        let part = occ.part_of(cell).unwrap_or(0) as usize;
        let Some(material) = study.materials.get(part).or_else(|| study.materials.first()) else { continue };
        let share = occ.share_of(cell);
        if material.density_kg_m3 > 0.0 && study.gravity_m_s2 != [0.0; 3] {
            // A cell the model only half fills weighs half as much.
            let mass = material.density_kg_m3 * volume * share;
            for axis in 0..3 {
                let share = mass * study.gravity_m_s2[axis] / 8.0;
                if let Some(slot) = applied.get_mut(axis) {
                    *slot += share * 8.0;
                }
                for node in corners {
                    if let Some(slot) = force.get_mut(node * 3 + axis) {
                        *slot += share;
                    }
                }
            }
        }
        let rise = study.temperature_rise_k.get(e).copied().unwrap_or(0.0);
        if rise != 0.0 && material.expansion_per_k.iter().any(|a| *a != 0.0) {
            // And pushes as hard as there is material in it to push, the same share its
            // stiffness is scaled by, so that a free part under a uniform warming comes out
            // unstressed however the lattice cuts it.
            let f = thermal_forces(material, h, rise);
            for (c, node) in corners.iter().enumerate() {
                for axis in 0..3 {
                    let Some(v) = f.get(c * 3 + axis) else { continue };
                    if let Some(slot) = force.get_mut(node * 3 + axis) {
                        *slot += v * share;
                    }
                }
            }
        }
    }

    if !held.iter().any(|h| *h) {
        return Err(SimError::Underdetermined(
            "nothing in this case holds the part still, so it would simply drift: give at least one face a `fixed` or a \
             `roller` support"
                .into(),
        ));
    }
    if !force.iter().all(|f| f.is_finite()) {
        return Err(SimError::NotFinite("a nodal force"));
    }
    // The load as it stands is what is reported; the right-hand side the solver gets is the
    // same with the held freedoms emptied, since those are prescribed to be still and a load
    // put on one never reaches the part. What that support then has to provide is the internal
    // force less the load already sitting on it, and with the reaction worked out that way the
    // whole load and the whole reaction cancel exactly -- including the weight of the material
    // right at a clamp, and a pressure on a face that is clamped as well.
    let load = force;
    let mut rhs = load.clone();
    for (dof, h) in held.iter().enumerate() {
        if *h && let Some(slot) = rhs.get_mut(dof) {
            *slot = 0.0;
        }
    }

    // Pulling cells onto the surface changes what a cell *is*, so everything worked out over a
    // cell has to be worked out the same way. The stiffness and the strain are; the weight of
    // the material, the push of its own warmth, and the area a pressure or a traction acts over
    // are not -- they are still the lattice's. Rather than leave the two inconsistent, which is
    // how a freely warmed part comes out stressed, a study that needs both is refused.
    let fitted = fitted_stiffnesses(occ, &nodes, study, &faces, &owner, h)?;
    let operator = Operator::new(occ, &nodes, element, fitted, held);
    let mut displacement = vec![0.0; freedoms];
    let linear = crate::linalg::solve(&operator, &rhs, &mut displacement, limits.tolerance, limits.max_iterations)?;
    if !displacement.iter().all(|u| u.is_finite()) {
        return Err(SimError::NotFinite("the displacements"));
    }

    // The reaction at each held freedom, and with it the check that the case is in balance.
    let mut internal = vec![0.0; freedoms];
    operator.internal(&displacement, &mut internal)?;
    let mut reaction = [0.0f64; 3];
    // How much force is moving about in each direction, taking no account of which way it
    // points: every nodal load, and every reaction. This is what the balance is measured
    // against, and it has to be, because the sums themselves can be nothing in a case that is
    // perfectly in balance -- a bar held at both ends while it warms up passes a great deal of
    // force through its supports and the sum is zero, and a part left free to grow has loads of
    // tens of kilonewtons inside it that cancel to nothing at all. Dividing by either sum would
    // read a ratio of one tiny number to another and call it a failure.
    let mut through = [0.0f64; 3];
    for (dof, f) in load.iter().enumerate() {
        if let Some(slot) = through.get_mut(dof % 3) {
            *slot += f.abs();
        }
    }
    for (dof, h) in operator.held.iter().enumerate() {
        if !*h {
            continue;
        }
        let axis = dof % 3;
        let (Some(v), Some(already)) = (internal.get(dof), load.get(dof)) else { continue };
        let r = v - already;
        if let Some(slot) = reaction.get_mut(axis) {
            *slot += r;
        }
        if let Some(slot) = through.get_mut(axis) {
            *slot += r.abs();
        }
    }

    let (stress_pa, von_mises_pa) = stresses(occ, &nodes, study, &displacement, h);
    let worst_stress = worst(occ, &von_mises_pa, |v| v);
    let least_safety_factor = safety(occ, study, &von_mises_pa);
    let most_movement = furthest(occ, &nodes, &displacement);

    // A body in equilibrium has the whole load and the whole reaction cancel, in each
    // direction, exactly: a rigid translation is in the operator's null space, which makes that
    // an identity rather than an approximation.
    //
    // The leftover is measured against the largest force anywhere in the case rather than
    // against each direction's own, because a direction can have nothing happening in it at all
    // -- a bar pulled along its length has no lateral load and no lateral reaction worth the
    // name -- and a ratio of one tiny number to another says nothing about anything.
    //
    let scale = through.iter().fold(0.0, |a: f64, b| a.max(*b));
    let net = applied.iter().zip(&reaction).map(|(a, r)| (a + r).abs()).fold(0.0, f64::max);
    let imbalance = if scale > 0.0 { net / scale } else { 0.0 };

    Ok(Solution {
        displacement,
        von_mises_pa,
        stress_pa,
        linear,
        worst_stress,
        least_safety_factor,
        most_movement,
        applied_n: applied,
        reaction_n: reaction,
        imbalance,
    })
}

fn check_support(support: &Support, name: &str) -> Result<()> {
    match support {
        Support::Fixed | Support::Roller | Support::Free => Ok(()),
        Support::Pressure(pa) if pa.is_finite() => Ok(()),
        Support::Pressure(pa) => setup(format!("`{name}` has a pressure of {pa} Pa, which is not a number")),
        Support::Traction(t) if t.iter().all(|v| v.is_finite()) => Ok(()),
        Support::Traction(_) => setup(format!("`{name}` has a traction that is not three numbers")),
    }
}

/// Which load owns each face of the surface. `usize::MAX` for one nothing claimed, which is
/// left free.
pub fn claim_faces(faces: &[SurfaceFace], loads: &[Load]) -> Result<Vec<usize>> {
    let mut owner = vec![usize::MAX; faces.len()];
    for (l, load) in loads.iter().enumerate() {
        match &load.patch {
            Patch::Side { axis, positive } => {
                if *axis > 2 {
                    return setup(format!("axis {axis}; a side is 0, 1 or 2 for x, y or z"));
                }
                for (i, face) in faces.iter().enumerate() {
                    if face.axis() == *axis && face.positive() == *positive {
                        claim(&mut owner, i, l);
                    }
                }
            }
            Patch::Faces(list) => {
                for (i, face) in faces.iter().enumerate() {
                    if list.contains(face) {
                        claim(&mut owner, i, l);
                    }
                }
            }
            Patch::Rest => {
                for i in 0..faces.len() {
                    claim(&mut owner, i, l);
                }
            }
        }
    }
    Ok(owner)
}

fn claim(owner: &mut [usize], face: usize, load: usize) {
    if let Some(slot) = owner.get_mut(face)
        && *slot == usize::MAX
    {
        *slot = load;
    }
}

/// How much of a cell's face area the model's own face actually has there. One where nothing
/// says otherwise.
fn scale_at(study: &Study, face: usize) -> f64 {
    match study.area_scale.get(face).copied() {
        Some(v) if v.is_finite() && v > 0.0 => v as f64,
        _ => 1.0,
    }
}

/// Share a traction over the four corners of a cell face.
fn spread(force: &mut [f64], applied: &mut [f64; 3], corners: &[usize; 4], traction: [f64; 3], area: f64) {
    for axis in 0..3 {
        let total = traction.get(axis).copied().unwrap_or(0.0) * area;
        if let Some(slot) = applied.get_mut(axis) {
            *slot += total;
        }
        for node in corners {
            if let Some(slot) = force.get_mut(node * 3 + axis) {
                *slot += total / 4.0;
            }
        }
    }
}

/// A cell's eight corners in metres, but only where the cell has actually been pulled off the
/// lattice. `None` for one still sitting where the lattice put it, which is every cell of a part
/// square to the axes and every cell inside any part at all.
fn moved_cell(occ: &Occupancy, nodes: &Nodes, fitted: &crate::snap::Snapped, cell: usize) -> Option<[Vec3; 8]> {
    let corners = fitted.cell(occ, nodes, cell)?;
    let eqs = nodes.of_cell(occ, cell)?;
    let g = occ.grid();
    let sp = g.spacing();
    let moved = eqs.iter().zip(&corners).any(|(eq, at)| {
        let Some(ijk) = nodes.position(*eq) else { return false };
        let home = g.origin() + Vec3::new(ijk[0] as f64 * sp, ijk[1] as f64 * sp, ijk[2] as f64 * sp);
        at.dist(home) > 1e-12
    });
    moved.then(|| std::array::from_fn(|i| corners[i] / crate::grid::MM_PER_M))
}

/// A stiffness of its own for every cell that has been pulled onto the surface, or nothing at
/// all where the study is to run on the lattice.
///
/// The refusals are the point. A fitted cell is no longer a box of side `h`, so anything else
/// worked out from `h` over that cell is wrong for it: its weight, the force its own warmth
/// pushes with, the area of its faces. The stiffness and the strain are done again here; the
/// others are not, and a study that would need them is told so rather than given an answer that
/// mixes the two. That is the mistake the cut-cell quadrature made (see
/// docs/analysis-verification.md), where a freely warmed cylinder came out at 134 MPa.
fn fitted_stiffnesses(
    occ: &Occupancy,
    nodes: &Nodes,
    study: &Study,
    faces: &[SurfaceFace],
    owner: &[usize],
    h: f64,
) -> Result<std::collections::HashMap<u32, Box<[[f64; DOFS]; DOFS]>>> {
    let mut out = std::collections::HashMap::new();
    let Some(fitted) = study.fitted.as_ref().filter(|f| !f.is_lattice()) else { return Ok(out) };
    let _ = h;

    if study.gravity_m_s2 != [0.0; 3] {
        return setup(
            "not supported yet: a study that follows the model's surface and also carries the part's own weight -- the              weight is still shared out as if every cell were a box, which would not match the elements. Run it without              gravity, or without surface fitting",
        );
    }
    if study.temperature_rise_k.iter().any(|t| *t != 0.0) {
        return setup(
            "not supported yet: a study that follows the model's surface and is also warmed -- the push of the warmth is              still worked out as if every cell were a box, which would not match the elements. Run it without a              temperature rise, or without surface fitting",
        );
    }

    // A load acts over the area of a face, and that area is still the lattice's. Where the face
    // has moved, the two disagree.
    for (index, face) in faces.iter().enumerate() {
        let Some(load) = owner.get(index).copied().and_then(|o| study.loads.get(o)) else { continue };
        if !matches!(load.support, Support::Pressure(_) | Support::Traction(_)) {
            continue;
        }
        let Some(corners) = nodes.of_face(occ, *face) else { continue };
        let moved = corners.iter().any(|eq| {
            let (Some(at), Some(ijk)) = (fitted.node(*eq), nodes.position(*eq)) else { return false };
            let g = occ.grid();
            let sp = g.spacing();
            let home = g.origin() + Vec3::new(ijk[0] as f64 * sp, ijk[1] as f64 * sp, ijk[2] as f64 * sp);
            at.dist(home) > 1e-12
        });
        if moved {
            return setup(
                "not supported yet: a pressure or a traction on a face that follows the model's surface -- the load is                  still spread over the area the lattice's own face had. Put the load on a face square to the axes, or                  run without surface fitting",
            );
        }
    }

    // Only the cells that actually moved need one.
    let mut want: Vec<(usize, [Vec3; 8])> = Vec::new();
    for e in 0..occ.equations() {
        let Some(cell) = occ.cell_of(e) else { continue };
        if let Some(corners) = moved_cell(occ, nodes, fitted, cell) {
            want.push((cell, corners));
        }
    }
    if want.len() > MAX_FITTED_CELLS {
        return Err(SimError::Setup(format!(
            "{} cells follow the model's surface, and a study may hold a stiffness of its own for {MAX_FITTED_CELLS}.              Run it with fewer `cells`",
            want.len()
        )));
    }

    for (cell, m) in want {
        let part = occ.part_of(cell).unwrap_or(0) as usize;
        let Some(material) = study.materials.get(part).or_else(|| study.materials.first()) else { continue };
        match element_stiffness_hex(material, &m) {
            Some(k) => {
                out.insert(cell as u32, Box::new(k));
            }
            // The snap is meant to have caught this; if one slipped through, the cell stays on
            // the lattice rather than the study failing.
            None => continue,
        }
    }
    Ok(out)
}

/// The stress in each solid cell, read at its centre.
fn stresses(occ: &Occupancy, nodes: &Nodes, study: &Study, displacement: &[f64], h: f64) -> (Vec<[f64; 6]>, Vec<f64>) {
    let mut stress = vec![[0.0f64; 6]; occ.equations()];
    let mut mises = vec![0.0f64; occ.equations()];
    for e in 0..occ.equations() {
        let Some(cell) = occ.cell_of(e) else { continue };
        // A cell pulled onto the surface is read with the element it actually is, at its own
        // middle: there is no staircase left to read around, and the material centre below is a
        // correction for one. Reading it any other way would be reading a strain the stiffness
        // that solved it never had.
        let b = match study.fitted.as_ref().and_then(|f| moved_cell(occ, nodes, f, cell)) {
            Some(m) => {
                let Some(centre) = jacobian([0.0; 3], &m) else { continue };
                let Some((full, _)) = strain_matrix_hex([0.0; 3], &m, &centre) else { continue };
                let mut b = [[0.0f64; DOFS]; 6];
                for (row, out) in full.iter().zip(b.iter_mut()) {
                    for (v, slot) in row.iter().zip(out.iter_mut()) {
                        *slot = *v;
                    }
                }
                b
            }
            None => strain_matrix(occ.material_centre_local(cell), h),
        };
        // Read the strain where the material is. For a cell the model fills whole that is the
        // middle of the reference cube, where a trilinear element's strain is least wrong --
        // it is the element's own average. For a cell a wall crosses the middle can be out in
        // the void, and the element's displacement there is extrapolation: it is held up by a
        // stiffness scaled down to the sliver of material the cell has, so the field over the
        // empty part of the cell is whatever the neighbours impose and its gradient is not
        // bounded by anything the part does. That is where the staircase's false peak comes
        // from, and reading at the centre of the material instead is the finite cell method's
        // own answer to it: take the stress only where there is material to carry it.
        let Some(corners) = nodes.of_cell(occ, cell) else { continue };
        let part = occ.part_of(cell).unwrap_or(0) as usize;
        let Some(material) = study.materials.get(part).or_else(|| study.materials.first()) else { continue };
        let d = elasticity_matrix(material);
        let mut local = [0.0f64; DOFS];
        for (c, node) in corners.iter().enumerate() {
            for axis in 0..3 {
                if let (Some(slot), Some(v)) = (local.get_mut(c * 3 + axis), displacement.get(node * 3 + axis)) {
                    *slot = *v;
                }
            }
        }
        let mut strain = [0.0f64; 6];
        for r in 0..6 {
            let Some(row) = b.get(r) else { continue };
            let mut v = 0.0;
            for (col, u) in row.iter().zip(&local) {
                v += col * u;
            }
            if let Some(slot) = strain.get_mut(r) {
                *slot = v;
            }
        }
        // The free expansion is taken off before the stress: a part that grew exactly as much
        // as it wanted to carries none.
        let rise = study.temperature_rise_k.get(e).copied().unwrap_or(0.0);
        for axis in 0..3 {
            if let Some(slot) = strain.get_mut(axis) {
                *slot -= material.expansion_per_k.get(axis).copied().unwrap_or(0.0) * rise;
            }
        }
        let mut s = [0.0f64; 6];
        for r in 0..6 {
            let mut v = 0.0;
            for c in 0..6 {
                let (Some(drc), Some(ec)) = (d.get(r).and_then(|x| x.get(c)), strain.get(c)) else { continue };
                v += drc * ec;
            }
            if let Some(slot) = s.get_mut(r) {
                *slot = v;
            }
        }
        if let Some(slot) = stress.get_mut(e) {
            *slot = s;
        }
        if let Some(slot) = mises.get_mut(e) {
            *slot = von_mises(s);
        }
    }
    (stress, mises)
}

fn worst(occ: &Occupancy, values: &[f64], of: impl Fn(f64) -> f64) -> Option<Worst> {
    let (equation, value) = values.iter().enumerate().map(|(e, v)| (e, of(*v))).filter(|(_, v)| v.is_finite()).max_by(|a, b| a.1.total_cmp(&b.1))?;
    let at = occ.grid().centre(occ.cell_of(equation)?)?;
    Some(Worst { equation, at, value })
}

fn safety(occ: &Occupancy, study: &Study, mises: &[f64]) -> Option<Worst> {
    let mut least: Option<Worst> = None;
    for (e, s) in mises.iter().enumerate() {
        let Some(cell) = occ.cell_of(e) else { continue };
        let part = occ.part_of(cell).unwrap_or(0) as usize;
        let Some(material) = study.materials.get(part).or_else(|| study.materials.first()) else { continue };
        let Some(yield_pa) = material.yield_strength_pa else { continue };
        // A cell carrying no stress at all has no end of safety in it; reporting infinity
        // there would drown out everywhere that matters.
        if !(*s > 0.0) {
            continue;
        }
        let factor = yield_pa / s;
        if least.is_none_or(|l| factor < l.value) {
            let Some(at) = occ.grid().centre(cell) else { continue };
            least = Some(Worst { equation: e, at, value: factor });
        }
    }
    least
}

fn furthest(occ: &Occupancy, nodes: &Nodes, displacement: &[f64]) -> Option<Worst> {
    let grid = occ.grid();
    let h = grid.spacing();
    let lo = grid.origin();
    let mut most: Option<Worst> = None;
    for eq in 0..nodes.count() {
        let Some(u) = displacement.get(eq * 3..eq * 3 + 3) else { continue };
        let moved = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt();
        if !moved.is_finite() || most.is_some_and(|m| moved <= m.value) {
            continue;
        }
        let Some(at) = nodes.position(eq) else { continue };
        // Nodes sit on the corners of the cells, not at their centres.
        let point = lo + Vec3::new(at[0] as f64 * h, at[1] as f64 * h, at[2] as f64 * h);
        most = Some(Worst { equation: eq, at: point, value: moved });
    }
    most
}

#[cfg(test)]
#[path = "elastic_tests.rs"]
mod tests;
