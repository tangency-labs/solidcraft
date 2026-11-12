//! Heat conduction through a solid.
//!
//! The method is the standard cell-centred finite-volume discretisation of Fourier's law
//! (Patankar, *Numerical Heat Transfer and Fluid Flow*, §4): energy is conserved over every
//! cell exactly, because what leaves one cell through a face is the same number that enters
//! the next. The discrete equations are symmetric and positive definite once anything fixes
//! the level, so [`Stencil7`] holds them and conjugate gradients solves them.
//!
//! Three decisions carry the accuracy:
//!
//! * **The face conductivity is the harmonic mean** of the two cells', not the arithmetic
//!   mean. Conduction through two layers is two resistances in series, and the harmonic mean
//!   is that series resistance; the arithmetic mean is not, and gets a steel-and-plastic
//!   interface wrong by whatever the ratio of the two conductivities is. With equal cells the
//!   mean is `2·k₁·k₂/(k₁+k₂)`, and a layered wall comes out exact.
//! * **A surface condition sits in series with the half-cell** between the cell's centre and
//!   its wall. A film coefficient `h` and that half-cell give `1/(d/2k + 1/h)`, which tends to
//!   the cell's own conduction as `h` grows -- so holding a face at a temperature and blowing
//!   infinitely hard on it are the same condition, as they should be.
//! * **Radiation is a Robin condition, exactly.** `T⁴ − T∞⁴` factors as
//!   `(T² + T∞²)(T + T∞)(T − T∞)`, so `εσ(T⁴ − T∞⁴)` *is* `h_r·(T − T∞)` with
//!   `h_r = εσ(T² + T∞²)(T + T∞)`. Nothing is dropped. The only approximation is that `h_r`
//!   is evaluated at the last iteration's surface temperature, and the outer loop runs until
//!   that stops moving, so the answer satisfies the full fourth-power law.
//!
//! Temperatures are in kelvin throughout. Radiation is a law about absolute temperature, and a
//! solver that took degrees Celsius anywhere would eventually use them in the wrong place.

use std::collections::HashMap;

use crate::grid::{Occupancy, SurfaceFace};
use crate::linalg::{CgReport, Stencil7};
use crate::{Result, SimError, setup};

/// The Stefan–Boltzmann constant, W/(m²·K⁴). Exact under the 2019 SI, being
/// `2π⁵k⁴/(15h³c²)` with k, h and c all defined.
pub const STEFAN_BOLTZMANN: f64 = 5.670_374_419e-8;

/// What a solid conducts and stores.
///
/// Conductivity is given along the world axes. An orthotropic material whose own axes are
/// turned away from the world's has conduction across axes as well, which this does not carry
/// yet -- [`Conductivity::world_aligned`] refuses such a tensor rather than quietly dropping
/// the terms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolidMaterial {
    /// Thermal conductivity along x, y and z, W/(m·K).
    pub conductivity_w_m_k: [f64; 3],
    /// kg/m³. Only a transient study needs it.
    pub density_kg_m3: f64,
    /// J/(kg·K). Only a transient study needs it.
    pub specific_heat_j_kg_k: f64,
}

impl SolidMaterial {
    /// A material that conducts the same in every direction and stores nothing, for a
    /// steady-state study.
    pub fn isotropic(conductivity_w_m_k: f64) -> SolidMaterial {
        SolidMaterial { conductivity_w_m_k: [conductivity_w_m_k; 3], density_kg_m3: 0.0, specific_heat_j_kg_k: 0.0 }
    }

    /// Heat capacity per cubic metre, J/(m³·K).
    pub fn volumetric_capacity(&self) -> f64 {
        self.density_kg_m3 * self.specific_heat_j_kg_k
    }

    fn along(&self, axis: usize) -> f64 {
        self.conductivity_w_m_k.get(axis).copied().unwrap_or(0.0)
    }

    fn check(&self, which: usize) -> Result<()> {
        for (axis, k) in self.conductivity_w_m_k.iter().enumerate() {
            if !(*k > 0.0) || !k.is_finite() {
                return Err(SimError::Material(format!(
                    "body {which} conducts {k} W/(m·K) along axis {axis}; a conductivity must be a positive number"
                )));
            }
        }
        if !(self.density_kg_m3 >= 0.0 && self.density_kg_m3.is_finite()) {
            return Err(SimError::Material(format!("body {which} has a density of {}", self.density_kg_m3)));
        }
        if !(self.specific_heat_j_kg_k >= 0.0) || !self.specific_heat_j_kg_k.is_finite() {
            return Err(SimError::Material(format!("body {which} has a specific heat of {}", self.specific_heat_j_kg_k)));
        }
        Ok(())
    }
}

/// What the world does to a patch of the model's surface.
///
/// Several of these may act on the same patch and then they add, which is how a surface both
/// convects and radiates. [`Film::Fixed`] is the exception: it says what the temperature *is*,
/// so nothing may be added to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Film {
    /// Held at a temperature: a cold plate, a bath, a contact with something far larger.
    Fixed { kelvin: f64 },
    /// Watts per square metre into the surface. Positive heats the model.
    Flux { w_m2: f64 },
    /// Carried away by a fluid: `q = h·(T_surface − T_fluid)`.
    Convection { w_m2_k: f64, kelvin: f64 },
    /// Radiated to surroundings: `q = εσ(T_surface⁴ − T_surroundings⁴)`.
    Radiation { emissivity: f64, kelvin: f64 },
    /// Nothing crosses: a plane of symmetry, or a face against an insulator.
    Insulated,
}

impl Film {
    fn check(&self) -> Result<()> {
        match self {
            Film::Insulated => Ok(()),
            Film::Flux { w_m2 } if w_m2.is_finite() => Ok(()),
            Film::Flux { .. } => setup("a heat flux must be a finite number of watts per square metre"),
            Film::Fixed { kelvin } => absolute(*kelvin),
            Film::Convection { w_m2_k, kelvin } => {
                absolute(*kelvin)?;
                if !(*w_m2_k >= 0.0) || !w_m2_k.is_finite() {
                    return setup(format!("{w_m2_k} W/(m²·K) is not a film coefficient; it may not be negative"));
                }
                Ok(())
            }
            Film::Radiation { emissivity, kelvin } => {
                absolute(*kelvin)?;
                if !(*emissivity >= 0.0 && *emissivity <= 1.0) {
                    return setup(format!("an emissivity of {emissivity}; it is a fraction, from 0 to 1"));
                }
                Ok(())
            }
        }
    }
}

/// A temperature, or a refusal. Kelvin is absolute, so zero and below are not temperatures,
/// and radiation would take a fourth power of them.
fn absolute(kelvin: f64) -> Result<()> {
    if !(kelvin > 0.0) || !kelvin.is_finite() {
        return setup(format!("{kelvin} K is not a temperature; temperatures here are absolute, so above zero"));
    }
    Ok(())
}

/// Which part of the surface a condition applies to.
#[derive(Clone, Debug, PartialEq)]
pub enum Patch {
    /// Every surface face looking along, or against, one axis -- the model's own six sides as
    /// the lattice sees them. Exact for a shape square to the axes, which is what the
    /// verification cases are made of.
    Side { axis: usize, positive: bool },
    /// The faces listed. This is what a face picked in the model becomes.
    Faces(Vec<SurfaceFace>),
    /// Whatever no earlier condition claimed.
    Rest,
}

/// A patch of surface and what happens on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Condition {
    /// A name to report the heat crossing this patch under.
    pub name: String,
    pub patch: Patch,
    /// The films acting here. They add, except that [`Film::Fixed`] must be alone.
    pub films: Vec<Film>,
}

impl Condition {
    pub fn new(name: impl Into<String>, patch: Patch, films: Vec<Film>) -> Condition {
        Condition { name: name.into(), patch, films }
    }
}

/// Whether the study asks for the settled answer or one step of the way there.
#[derive(Clone, Debug, PartialEq)]
pub enum Time {
    /// The temperature the model ends up at, with nothing changing.
    Steady,
    /// One implicit step of `seconds` from the temperatures in `from`, one per equation.
    ///
    /// The step is backward Euler, which is unconditionally stable: a step too long gives a
    /// blurred answer, never a diverging one. That matters more than the extra order a
    /// Crank–Nicolson step would give, because a diverging transient looks like a result.
    Step { seconds: f64, from: Vec<f64> },
}

/// How hard the solver tries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// Relative residual the linear solve must reach.
    pub tolerance: f64,
    pub max_iterations: u32,
    /// How close two outer radiation iterations must land, in kelvin.
    pub radiation_tolerance_k: f64,
    pub max_outer: u32,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { tolerance: 1e-12, max_iterations: 20_000, radiation_tolerance_k: 1e-6, max_outer: 50 }
    }
}

/// A conduction study: what the model is made of, and what the world does to it.
#[derive(Clone, Debug)]
pub struct Study {
    /// One entry per part, in the order the occupancy's parts were given.
    pub materials: Vec<SolidMaterial>,
    /// Watts per cubic metre generated inside each part. One entry per part, or empty for none.
    pub generation_w_m3: Vec<f64>,
    /// Tried in order; the first condition that claims a face owns it.
    pub conditions: Vec<Condition>,
    /// What happens on the surface no condition claimed.
    pub rest: Vec<Film>,
    pub time: Time,
    /// How much to scale each surface face's area by, in [`Occupancy::surface`] order: what
    /// [`crate::tag::FaceTags::area_scale`] gives. Empty leaves every face the area of the
    /// lattice's own staircase, which over-states anything not square to an axis.
    pub area_scale: Vec<f32>,
    pub limits: Limits,
}

impl Study {
    /// A study of `materials` with nothing but insulated surface, to be filled in.
    pub fn new(materials: Vec<SolidMaterial>) -> Study {
        Study {
            materials,
            generation_w_m3: Vec::new(),
            conditions: Vec::new(),
            rest: vec![Film::Insulated],
            time: Time::Steady,
            area_scale: Vec::new(),
            limits: Limits::default(),
        }
    }

    pub fn with(mut self, condition: Condition) -> Study {
        self.conditions.push(condition);
        self
    }
}

/// How much heat crossed one patch of surface, and how warm it got.
#[derive(Clone, Debug, PartialEq)]
pub struct PatchReport {
    pub name: String,
    /// Watts into the model across this patch. Negative means the model lost heat here.
    pub watts: f64,
    /// Area of the patch, m²: the model's own, not the staircase's, when the study was given
    /// [`Study::area_scale`].
    pub area_m2: f64,
    pub min_kelvin: f64,
    pub max_kelvin: f64,
}

/// What a conduction study came out as.
#[derive(Clone, Debug)]
pub struct Solution {
    /// Temperature at each equation's cell, in kelvin.
    pub kelvin: Vec<f64>,
    /// Temperature of the wall itself at each face of the surface, in kelvin, in the order
    /// [`Occupancy::surface`] gives them.
    ///
    /// Not the same as the cell behind it: with a film on the face the two differ by the drop
    /// across the half-cell, and the wall is the one somebody touches and the one a picture
    /// should be coloured by.
    pub wall_kelvin: Vec<f64>,
    pub linear: CgReport,
    /// Outer radiation iterations taken; 1 when nothing radiates.
    pub outer: u32,
    pub patches: Vec<PatchReport>,
    pub generated_w: f64,
    /// Watts unaccounted for, as a fraction of the largest flow in the study.
    ///
    /// A steady answer must balance: what is generated plus what comes in through the surface
    /// has to be what goes out. The discretisation conserves energy exactly, so anything here
    /// beyond the linear solver's own tolerance is a mistake, and this is how a caller sees it.
    pub imbalance: f64,
    pub min_kelvin: f64,
    pub max_kelvin: f64,
}

impl Solution {
    /// The temperature in a cell of the lattice, or `None` where the model is not.
    pub fn at_cell(&self, occ: &Occupancy, cell: usize) -> Option<f64> {
        self.kelvin.get(occ.eq_of(cell)?).copied()
    }
}

/// Solve a conduction study over an occupancy.
pub fn solve(occ: &Occupancy, study: &Study) -> Result<Solution> {
    let n = occ.equations();
    if n == 0 {
        return setup("nothing to solve: no cell of the lattice is inside the model");
    }
    if study.materials.is_empty() {
        return Err(SimError::Material("no material was given for the bodies".into()));
    }
    for (which, m) in study.materials.iter().enumerate() {
        m.check(which)?;
    }
    for g in &study.generation_w_m3 {
        if !g.is_finite() {
            return setup("heat generation must be a finite number of watts per cubic metre");
        }
    }
    for c in &study.conditions {
        check_films(&c.films, &c.name)?;
    }
    check_films(&study.rest, "the rest of the surface")?;
    let limits = study.limits;
    if !(limits.max_outer >= 1) {
        return setup("a study must allow at least one iteration");
    }

    let faces = assign(occ, study)?;
    let grid = occ.grid().clone();
    let area = grid.face_area_m2();
    let volume = grid.cell_volume_m3();
    let pitch = grid.pitch_m();
    let half = pitch / 2.0;

    // The capacity term of a transient step, and the temperatures it starts from.
    let (capacity_over_dt, previous) = match &study.time {
        Time::Steady => (None, None),
        Time::Step { seconds, from } => {
            if !(*seconds > 0.0) || !seconds.is_finite() {
                return setup("a time step must be a positive number of seconds");
            }
            if from.len() != n {
                return setup(format!("the previous temperatures have {} values and the lattice has {n} cells", from.len()));
            }
            if !from.iter().all(|t| t.is_finite() && *t > 0.0) {
                return setup("a previous temperature that is not an absolute temperature");
            }
            (Some(*seconds), Some(from.clone()))
        }
    };

    // Interior conduction never changes between outer iterations, so it is assembled once and
    // the surface is added to a copy.
    let interior = interior_operator(occ, study, pitch, area)?;
    let mut body_source = vec![0.0; n];
    for (e, slot) in body_source.iter_mut().enumerate() {
        let Some(cell) = occ.cell_of(e) else { continue };
        let part = occ.part_of(cell).unwrap_or(0) as usize;
        let generated = study.generation_w_m3.get(part).copied().unwrap_or(0.0);
        // A cell the model only half fills holds half the material, so it generates half the
        // heat and stores half the warmth.
        *slot = generated * volume * occ.share_of(cell);
    }

    // Surface temperature of each radiating face, which the outer loop refines. Starting from
    // the surroundings' own temperature is the right end to start from: it is where a surface
    // that radiates freely ends up.
    let mut surface_k: Vec<f64> = faces.iter().map(|f| f.start_kelvin()).collect();
    let mut kelvin = vec![faces.iter().map(|f| f.start_kelvin()).fold(0.0, f64::max).max(293.15); n];
    let mut outer = 0u32;

    let linear = loop {
        outer += 1;
        let mut operator = interior.clone();
        let mut rhs = body_source.clone();
        let mut anchored = false;

        if let (Some(dt), Some(prev)) = (capacity_over_dt, previous.as_ref()) {
            for (e, old) in prev.iter().enumerate() {
                let Some(cell) = occ.cell_of(e) else { continue };
                let part = occ.part_of(cell).unwrap_or(0) as usize;
                let Some(material) = study.materials.get(part).or_else(|| study.materials.first()) else { continue };
                let capacity = material.volumetric_capacity() * volume * occ.share_of(cell);
                if !(capacity > 0.0) {
                    return Err(SimError::Material(
                        "a transient study needs a density and a specific heat, and one of the bodies has neither".into(),
                    ));
                }
                let term = capacity / dt;
                operator.add_diagonal(e, term);
                if let Some(r) = rhs.get_mut(e) {
                    *r += term * old;
                }
                anchored = true;
            }
        }

        for (index, face) in faces.iter().enumerate() {
            let Some(cell) = occ.cell_of(face.eq) else { continue };
            let part = occ.part_of(cell).unwrap_or(0) as usize;
            let Some(material) = study.materials.get(part).or_else(|| study.materials.first()) else { continue };
            let k = material.along(face.face.axis());
            let surface = surface_k.get(index).copied().unwrap_or(293.15);
            let area = area * scale_at(study, index);
            let (conductance, source, fixes) = face.contribution(k, half, area, surface);
            if !conductance.is_finite() || !source.is_finite() {
                return Err(SimError::NotFinite("a surface condition"));
            }
            operator.add_diagonal(face.eq, conductance);
            if let Some(r) = rhs.get_mut(face.eq) {
                *r += source;
            }
            anchored |= fixes;
        }

        if !anchored {
            return Err(SimError::Underdetermined(
                "nothing in this study settles the temperature: give at least one face a fixed temperature, a convection or a \
                 radiation condition, or run it as a transient from a starting temperature"
                    .into(),
            ));
        }

        let linear = operator.solve(&rhs, &mut kelvin, limits.tolerance, limits.max_iterations)?;
        if !kelvin.iter().all(|t| t.is_finite()) {
            return Err(SimError::NotFinite("the temperatures"));
        }

        // Refine each radiating face's surface temperature from the cell it sits on, and stop
        // when they have stopped moving. Faces that do not radiate have nothing to refine.
        let mut moved = 0.0f64;
        let mut radiates = false;
        for (index, face) in faces.iter().enumerate() {
            if !face.radiates() {
                continue;
            }
            radiates = true;
            let Some(cell) = occ.cell_of(face.eq) else { continue };
            let part = occ.part_of(cell).unwrap_or(0) as usize;
            let Some(material) = study.materials.get(part).or_else(|| study.materials.first()) else { continue };
            let k = material.along(face.face.axis());
            let inner = kelvin.get(face.eq).copied().unwrap_or(293.15);
            let was = surface_k.get(index).copied().unwrap_or(293.15);
            let now = face.surface_temperature(k, half, was, inner);
            moved = moved.max((now - was).abs());
            if let Some(slot) = surface_k.get_mut(index) {
                *slot = now;
            }
        }
        if !radiates || moved <= limits.radiation_tolerance_k {
            break linear;
        }
        if outer >= limits.max_outer {
            return Err(SimError::NoConvergence { iterations: outer, residual: moved });
        }
    };

    report(occ, study, &faces, &kelvin, &surface_k, linear, outer, area, volume, half)
}

fn check_films(films: &[Film], where_: &str) -> Result<()> {
    for f in films {
        f.check()?;
    }
    let fixed = films.iter().filter(|f| matches!(f, Film::Fixed { .. })).count();
    if fixed > 1 {
        return setup(format!("{where_} is held at two temperatures at once"));
    }
    if fixed == 1 && films.len() > 1 {
        return setup(format!(
            "{where_} is held at a temperature and also given another condition; a held temperature already says what the \
             surface does"
        ));
    }
    Ok(())
}

/// The conduction between cells, which no boundary condition touches.
fn interior_operator(occ: &Occupancy, study: &Study, pitch: f64, area: f64) -> Result<Stencil7> {
    let mut operator = Stencil7::over(occ);
    for e in 0..occ.equations() {
        let Some(cell) = occ.cell_of(e) else { continue };
        let here = occ.part_of(cell).unwrap_or(0) as usize;
        let Some(mine) = study.materials.get(here).or_else(|| study.materials.first()) else {
            return Err(SimError::Material(format!("body {here} has no material")));
        };
        for axis in 0..3 {
            let Some(to) = operator.forward_of(e, axis) else { continue };
            let Some(other_cell) = occ.cell_of(to) else { continue };
            let there = occ.part_of(other_cell).unwrap_or(0) as usize;
            let Some(theirs) = study.materials.get(there).or_else(|| study.materials.first()) else { continue };
            let face_k = harmonic(mine.along(axis), theirs.along(axis));
            // Conductance of one face: k·A/d. With cubic cells A/d is the cell's side, so this
            // is k·h -- but written the long way, because the flow solver will not have cubes.
            let conductance = face_k * area / pitch;
            if !conductance.is_finite() {
                return Err(SimError::NotFinite("a face conductance"));
            }
            operator.couple(e, axis, conductance);
        }
    }
    Ok(operator)
}

/// How much of a cell's face area the model's own face actually has there. One where nothing
/// says otherwise.
fn scale_at(study: &Study, face: usize) -> f64 {
    match study.area_scale.get(face).copied() {
        Some(v) if v.is_finite() && v > 0.0 => v as f64,
        _ => 1.0,
    }
}

/// The series resistance of two equal thicknesses of `a` and `b`.
///
/// This is the whole reason a layered wall comes out right: resistances in series add, so
/// their conductances combine harmonically. Zero anywhere means no heat crosses at all.
fn harmonic(a: f64, b: f64) -> f64 {
    if !(a > 0.0) || !(b > 0.0) {
        return 0.0;
    }
    2.0 * a * b / (a + b)
}

/// A surface face with the films that act on it resolved.
#[derive(Clone, Debug)]
struct Resolved {
    eq: usize,
    face: SurfaceFace,
    /// Which condition claimed it, for reporting.
    patch: usize,
    films: Vec<Film>,
}

impl Resolved {
    fn radiates(&self) -> bool {
        self.films.iter().any(|f| matches!(f, Film::Radiation { .. }))
    }

    /// Where to start a radiating face's surface temperature, and the temperature a cell is
    /// first guessed at: whatever the surroundings are.
    fn start_kelvin(&self) -> f64 {
        self.films
            .iter()
            .filter_map(|f| match f {
                Film::Fixed { kelvin } | Film::Convection { kelvin, .. } | Film::Radiation { kelvin, .. } => Some(*kelvin),
                _ => None,
            })
            .next()
            .unwrap_or(293.15)
    }

    /// What this face adds to its cell's equation: a conductance on the diagonal, a source on
    /// the right-hand side, and whether it settles the temperature at all.
    ///
    /// Films on one face act on the *same* surface, so their coefficients add and the sum goes
    /// in series with the half-cell once. Giving each film its own half-cell would put that
    /// resistance in parallel with itself and let more heat through than the solid can carry --
    /// which matters as soon as a surface both convects and radiates, the ordinary case.
    ///
    /// Writing `a = k/(d/2)` for the half-cell and `h` for the films' sum, the surface balance
    /// `a(T_cell − T_s) = h(T_s − T_film) − q` gives, after eliminating `T_s`,
    ///
    /// ```text
    /// into the cell = U·(T_film − T_cell) + q·a/(a + h),   U = 1/(1/a + 1/h)
    /// ```
    ///
    /// so a prescribed flux arriving at a surface that also has a film on it is shared: the
    /// part that goes straight back out through the film never enters the solid.
    ///
    /// `surface` is the last iteration's surface temperature, which only radiation uses.
    fn contribution(&self, k: f64, half: f64, area: f64, surface: f64) -> (f64, f64, bool) {
        let a = if half > 0.0 && k > 0.0 { k / half } else { 0.0 };
        if let Some(kelvin) = self.held() {
            // A held temperature is a film of no resistance at all: only the half-cell is left.
            return (a * area, a * area * kelvin, a > 0.0);
        }
        let (h, weighted, flux) = self.films_at(surface);
        if !(h > 0.0) {
            // Nothing but a flux, or nothing at all: a Neumann condition, which settles
            // nothing on its own.
            return (0.0, flux * area, false);
        }
        let fluid = weighted / h;
        let u = if a > 0.0 { 1.0 / (1.0 / a + 1.0 / h) } else { 0.0 };
        let share = if a + h > 0.0 { a / (a + h) } else { 1.0 };
        (u * area, u * area * fluid + share * flux * area, u > 0.0)
    }

    /// The temperature this face is held at, if it is.
    fn held(&self) -> Option<f64> {
        self.films.iter().find_map(|f| match f {
            Film::Fixed { kelvin } => Some(*kelvin),
            _ => None,
        })
    }

    /// The films' total coefficient, that coefficient weighted by each film's own temperature,
    /// and the prescribed flux, at a given surface temperature.
    fn films_at(&self, surface: f64) -> (f64, f64, f64) {
        let (mut h, mut weighted, mut flux) = (0.0, 0.0, 0.0);
        for film in &self.films {
            match film {
                Film::Insulated | Film::Fixed { .. } => {}
                Film::Flux { w_m2 } => flux += w_m2,
                Film::Convection { w_m2_k, kelvin } => {
                    h += w_m2_k;
                    weighted += w_m2_k * kelvin;
                }
                Film::Radiation { emissivity, kelvin } => {
                    // The exact equivalent coefficient, from the fourth-power identity.
                    let hr = radiative_coefficient(*emissivity, surface, *kelvin);
                    h += hr;
                    weighted += hr * kelvin;
                }
            }
        }
        (h, weighted, flux)
    }

    /// The surface temperature this face settles at, given the temperature of the cell behind
    /// it: the point where the heat the half-cell delivers is the heat the films carry away.
    ///
    /// Both sides rise with the surface temperature, so the balance is crossed once and a
    /// bisection on a bracket that always contains it cannot be led astray. Fifty halvings of
    /// a bracket that starts at a few thousand kelvin is far below a millikelvin.
    fn surface_temperature(&self, k: f64, half: f64, was: f64, inner: f64) -> f64 {
        let delivered = |t: f64| if half > 0.0 { k / half * (inner - t) } else { 0.0 };
        let carried = |t: f64| {
            self.films
                .iter()
                .map(|film| match film {
                    Film::Insulated | Film::Fixed { .. } => 0.0,
                    Film::Flux { w_m2 } => -w_m2,
                    Film::Convection { w_m2_k, kelvin } => w_m2_k * (t - kelvin),
                    Film::Radiation { emissivity, kelvin } => emissivity * STEFAN_BOLTZMANN * (t.powi(4) - kelvin.powi(4)),
                })
                .sum::<f64>()
        };
        let residual = |t: f64| delivered(t) - carried(t);
        let (mut lo, mut hi) = (1.0f64, inner.max(was).max(1.0) * 4.0 + 1000.0);
        if !(residual(lo) > 0.0) || !(residual(hi) < 0.0) {
            // The bracket does not hold a crossing, which means the cell is colder than
            // everything around it or the numbers are extreme. Leave the estimate alone
            // rather than invent one; the outer loop will stop on it.
            return was;
        }
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if residual(mid) > 0.0 { lo = mid } else { hi = mid }
        }
        0.5 * (lo + hi)
    }
}

/// The film coefficient radiation is exactly equivalent to at a surface temperature.
///
/// `εσ(T⁴ − T∞⁴) = εσ(T² + T∞²)(T + T∞)·(T − T∞)`, an identity, so nothing is linearised
/// away: the factor is the film coefficient, and it only depends on where the surface is.
pub fn radiative_coefficient(emissivity: f64, surface_k: f64, surroundings_k: f64) -> f64 {
    if !(emissivity > 0.0) {
        return 0.0;
    }
    let (t, a) = (surface_k.max(0.0), surroundings_k.max(0.0));
    let h = emissivity * STEFAN_BOLTZMANN * (t * t + a * a) * (t + a);
    if h.is_finite() { h } else { 0.0 }
}

/// Work out which condition owns each surface face: conditions in order, first claim wins,
/// and whatever is left gets the study's default.
fn assign(occ: &Occupancy, study: &Study) -> Result<Vec<Resolved>> {
    let mut all: Vec<SurfaceFace> = Vec::new();
    let mut index: HashMap<SurfaceFace, usize> = HashMap::new();
    for face in occ.surface() {
        index.insert(face, all.len());
        all.push(face);
    }
    if all.is_empty() {
        return setup("the model has no surface on this lattice");
    }
    let mut owner = vec![usize::MAX; all.len()];
    for (c, condition) in study.conditions.iter().enumerate() {
        match &condition.patch {
            Patch::Side { axis, positive } => {
                if *axis > 2 {
                    return setup(format!("axis {axis}; a side is 0, 1 or 2 for x, y or z"));
                }
                for (i, face) in all.iter().enumerate() {
                    if face.axis() == *axis && face.positive() == *positive {
                        claim(&mut owner, i, c);
                    }
                }
            }
            Patch::Faces(list) => {
                for face in list {
                    if let Some(i) = index.get(face) {
                        claim(&mut owner, *i, c);
                    }
                }
            }
            Patch::Rest => {
                for i in 0..all.len() {
                    claim(&mut owner, i, c);
                }
            }
        }
    }
    let rest = study.conditions.len();
    Ok(all
        .into_iter()
        .zip(&owner)
        .filter_map(|(face, patch)| {
            let eq = occ.eq_of(face.cell as usize)?;
            let films = match study.conditions.get(*patch) {
                Some(c) => c.films.clone(),
                None => study.rest.clone(),
            };
            Some(Resolved { eq, face, patch: if *patch == usize::MAX { rest } else { *patch }, films })
        })
        .collect())
}

fn claim(owner: &mut [usize], face: usize, condition: usize) {
    if let Some(slot) = owner.get_mut(face)
        && *slot == usize::MAX
    {
        *slot = condition;
    }
}

#[allow(clippy::too_many_arguments)]
fn report(
    occ: &Occupancy,
    study: &Study,
    faces: &[Resolved],
    kelvin: &[f64],
    surface_k: &[f64],
    linear: CgReport,
    outer: u32,
    area: f64,
    volume: f64,
    half: f64,
) -> Result<Solution> {
    let names: Vec<String> = study.conditions.iter().map(|c| c.name.clone()).chain(std::iter::once("the rest of the surface".to_string())).collect();
    let mut wall_kelvin = vec![0.0; faces.len()];
    let mut watts = vec![0.0; names.len()];
    let mut areas = vec![0.0; names.len()];
    let mut lowest = vec![f64::INFINITY; names.len()];
    let mut highest = vec![f64::NEG_INFINITY; names.len()];

    for (i, face) in faces.iter().enumerate() {
        let Some(cell) = occ.cell_of(face.eq) else { continue };
        let part = occ.part_of(cell).unwrap_or(0) as usize;
        let Some(material) = study.materials.get(part).or_else(|| study.materials.first()) else { continue };
        let k = material.along(face.face.axis());
        let inner = kelvin.get(face.eq).copied().unwrap_or(0.0);
        let surface = surface_k.get(i).copied().unwrap_or(inner);
        let area = area * scale_at(study, i);
        let (conductance, source, _) = face.contribution(k, half, area, surface);
        // What the equation received: source − conductance·T. That is exactly the heat the
        // condition put into the cell, which is what a balance has to be made of.
        let into = source - conductance * inner;
        if let Some(w) = watts.get_mut(face.patch) {
            *w += into;
        }
        if let Some(a) = areas.get_mut(face.patch) {
            *a += area;
        }
        // The temperature reported for a patch is the wall's, not the cell centre's: with a
        // film on it the two differ, and the wall is the one someone touches.
        let wall = wall_temperature(face, k, half, area, surface, inner);
        if let Some(slot) = wall_kelvin.get_mut(i) {
            *slot = wall;
        }
        if let Some(v) = lowest.get_mut(face.patch) {
            *v = v.min(wall);
        }
        if let Some(v) = highest.get_mut(face.patch) {
            *v = v.max(wall);
        }
    }

    let mut generated_w = 0.0;
    for e in 0..occ.equations() {
        let Some(cell) = occ.cell_of(e) else { continue };
        let part = occ.part_of(cell).unwrap_or(0) as usize;
        generated_w += study.generation_w_m3.get(part).copied().unwrap_or(0.0) * volume * occ.share_of(cell);
    }

    let crossing: f64 = watts.iter().map(|w| w.abs()).sum::<f64>() + generated_w.abs();
    let net: f64 = watts.iter().sum::<f64>() + generated_w;
    let imbalance = match &study.time {
        // A transient step stores energy, so the surface and the generation are not meant to
        // balance; only a settled answer is.
        Time::Step { .. } => 0.0,
        Time::Steady if crossing > 0.0 => net.abs() / crossing,
        Time::Steady => 0.0,
    };

    let patches = names
        .into_iter()
        .enumerate()
        .filter_map(|(i, name)| {
            let a = areas.get(i).copied().unwrap_or(0.0);
            if a <= 0.0 {
                return None;
            }
            Some(PatchReport {
                name,
                watts: watts.get(i).copied().unwrap_or(0.0),
                area_m2: a,
                min_kelvin: lowest.get(i).copied().unwrap_or(f64::NAN),
                max_kelvin: highest.get(i).copied().unwrap_or(f64::NAN),
            })
        })
        .collect();

    let min_kelvin = kelvin.iter().copied().fold(f64::INFINITY, f64::min);
    let max_kelvin = kelvin.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !min_kelvin.is_finite() || !max_kelvin.is_finite() {
        return Err(SimError::NotFinite("the temperature range"));
    }
    Ok(Solution { kelvin: kelvin.to_vec(), wall_kelvin, linear, outer, patches, generated_w, imbalance, min_kelvin, max_kelvin })
}

/// The temperature of the wall itself: the cell's, less the drop across the half-cell that
/// carries whatever the condition is taking away.
fn wall_temperature(face: &Resolved, k: f64, half: f64, area: f64, surface: f64, inner: f64) -> f64 {
    if !(k > 0.0) || !(half > 0.0) || !(area > 0.0) {
        return inner;
    }
    let (conductance, source, _) = face.contribution(k, half, area, surface);
    let into = source - conductance * inner;
    // `into` watts cross the half-cell, so the wall stands above the centre by q·(d/2)/(kA)
    // when heat is coming in, and below it when heat is leaving.
    inner + into * half / (k * area)
}

#[cfg(test)]
#[path = "thermal_tests.rs"]
mod tests;
