//! Air moving around the model.
//!
//! The method is the lattice Boltzmann method on the D3Q19 velocity set (Qian, d'Humières and
//! Lallemand 1992; Chen and Doolen 1998; Krüger and others, *The Lattice Boltzmann Method:
//! Principles and Practice*, 2017). Rather than solving the Navier–Stokes equations directly it
//! advances a set of nineteen distributions per cell -- how much of the fluid in that cell is
//! heading each way -- by two steps that are each trivially local: every distribution slides
//! one cell along its own direction, and then the nineteen at each cell relax towards the
//! equilibrium set that their own density and momentum imply. A Chapman–Enskog expansion of
//! those two steps *is* the incompressible Navier–Stokes equations to second order, with a
//! viscosity set by how fast the relaxation happens.
//!
//! Three reasons it is the right method here rather than a pressure-projection scheme:
//!
//! * **It takes the lattice the thermal solver already has.** A wall becomes a cell where the
//!   distributions turn round and go back the way they came, which needs no mesh fitted to the
//!   model and cannot fail to be generated.
//! * **There is no pressure equation.** An incompressible finite-volume scheme has to solve a
//!   global Poisson equation every step to make the velocity divergence-free. This gets the
//!   same effect from the relaxation, so a step is local and the cost is predictable.
//! * **The force on the body is exact by construction.** Momentum that arrives at a wall and
//!   leaves again has gone into the wall, and adding it up over the surface gives the drag and
//!   the lift without differentiating anything (Ladd 1994).
//!
//! What it costs: the scheme is only valid at low Mach number, so the lattice velocity has to
//! stay well under the lattice's own speed of sound, and a high Reynolds number therefore needs
//! either many cells or a turbulence model. It is also bounded from *below*, which is less
//! obvious and was got wrong here for a while: the distributions are `f32`, so too slow a flow
//! is round-off rather than physics ([`MIN_LATTICE_SPEED`]). [`Scaling`] works the trade out and refuses a
//! combination it cannot represent rather than returning numbers from an unstable run.
//!
//! Everything in here is in **lattice units**: spacing one, time step one, speed of sound
//! `1/√3`. [`Scaling`] converts to and from metres, seconds and newtons at the edges.
//!
//! How low a relaxation time the scheme can hold -- which is the whole of how high a Reynolds
//! number it can reach -- is a property of the collision operator and not of the method, so
//! [`Collision`] is four operators and [`Collision::min_tau`] is the measured floor of each.
//! Nineteen distributions carry nine moments the fluid does not have, and those are what the
//! scheme comes apart on; the operators differ in which of them they relax, which they throw
//! away, and what that costs at a wall. [`Collision::Regularized`] is the default and explains
//! the trade.

use std::sync::atomic::{AtomicBool, Ordering};

use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::grid::{Grid3, Occupancy};
use crate::library::Fluid;
use crate::par;
use crate::{Result, SimError, setup};

/// Directions in the velocity set.
pub const Q: usize = 19;

/// The D3Q19 velocities, in opposite pairs: 0 stands still, and every odd direction is followed
/// by its own reverse, so turning a distribution round is an increment.
pub const C: [[i32; 3]; Q] = [
    [0, 0, 0],
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
    [1, 1, 0],
    [-1, -1, 0],
    [1, -1, 0],
    [-1, 1, 0],
    [1, 0, 1],
    [-1, 0, -1],
    [1, 0, -1],
    [-1, 0, 1],
    [0, 1, 1],
    [0, -1, -1],
    [0, 1, -1],
    [0, -1, 1],
];

/// The weight of each direction. A third for standing still, a eighteenth for the six faces, a
/// thirty-sixth for the twelve edges; they add to one, and these are the values that make the
/// set's second and fourth moments isotropic, which is what the method rests on.
pub const W: [f64; Q] = [
    1.0 / 3.0,
    1.0 / 18.0,
    1.0 / 18.0,
    1.0 / 18.0,
    1.0 / 18.0,
    1.0 / 18.0,
    1.0 / 18.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
];

/// The square of the lattice's speed of sound. Everything about the pressure and the viscosity
/// is written in terms of it.
pub const CS2: f64 = 1.0 / 3.0;

/// The six odd ghost moments of the velocity set (d'Humières and others, 2002): the energy
/// fluxes `q_a = (5|c|² − 9)c_a` and the third-order moments `m_x = (c_y² − c_z²)c_x` with its
/// two rotations.
///
/// These are every moment of the antisymmetric half that the fluid does not have. The momentum
/// lives in the equilibrium set, so what is left of the odd half is exactly these six — and
/// they are orthogonal to the density, the momentum and every component of the stress, so the
/// rate they relax at cannot move anything physical. [`Collision::Mrt`] gives them one of their
/// own; under two rates they share `ω₋`, which the magic parameter drives to zero as `τ`
/// approaches a half.
///
/// They are sparse: `m_a` is carried only by the twelve diagonals and `q_a` by those and the
/// six axis directions, so projecting all six out costs rather less than a full
/// nineteen-moment transform would. Squared norms are [`GHOST3_NORM`], 40 and 8.
///
/// Orthogonality is checked, not assumed:
/// `the_odd_ghost_moments_are_orthogonal_to_everything_the_fluid_has`.
pub const GHOST3: [[f64; Q]; 6] = [
    // q_x, q_y, q_z = (5|c|² − 9)c_a, the energy fluxes.
    [0., -4., 4., 0., 0., 0., 0., 1., -1., 1., -1., 1., -1., 1., -1., 0., 0., 0., 0.],
    [0., 0., 0., -4., 4., 0., 0., 1., -1., -1., 1., 0., 0., 0., 0., 1., -1., 1., -1.],
    [0., 0., 0., 0., 0., -4., 4., 0., 0., 0., 0., 1., -1., -1., 1., 1., -1., -1., 1.],
    // m_x, m_y, m_z = (c_y² − c_z²)c_x and its rotations, the third-order moments.
    [0., 0., 0., 0., 0., 0., 0., 1., -1., 1., -1., -1., 1., -1., 1., 0., 0., 0., 0.],
    [0., 0., 0., 0., 0., 0., 0., -1., 1., 1., -1., 0., 0., 0., 0., 1., -1., 1., -1.],
    [0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 1., -1., -1., 1., -1., 1., 1., -1.],
];

/// The squared norm of each of [`GHOST3`], which is what a projection onto them divides by.
pub const GHOST3_NORM: [f64; 6] = [40., 40., 40., 8., 8., 8.];

/// The direction that undoes a direction.
pub const fn opposite(i: usize) -> usize {
    if i == 0 {
        0
    } else if i % 2 == 1 {
        i + 1
    } else {
        i - 1
    }
}

/// Most cells a flow may run on.
///
/// A flow keeps nineteen numbers per cell twice over, so this is about three hundred megabytes
/// at four bytes each -- already more than a desktop should take without being asked. A flow
/// that wants more cells wants a different machine.
pub const MAX_FLOW_CELLS: usize = 2_000_000;

/// The lowest relaxation time the scheme is run at.
///
/// `omega = 1/tau` has to stay under two, and a relaxation time a whisker above a half puts it
/// right against that, where the scheme comes apart on the first bit of numerical noise -- in
/// the free stream, before any strain has appeared for a turbulence model to act on.
///
/// Flows at engineering speeds ask for exactly that: a twenty-millimetre part in five metres a
/// second of air wants a relaxation time of 0.50002, and reaching 0.51 honestly would take
/// cells a tenth of a millimetre across, which is a hundred and sixty million of them. So the
/// fluid is made thicker than it is, the flow runs at a lower Reynolds number than the one
/// asked for, and [`Mapping::caveat`] says both, in numbers.
///
/// How low it can go is a property of the collision operator and not of the method, so the
/// figure that is used is [`Collision::min_tau`]. This is the loosest of them, and it is kept
/// as the floor a caller gets who does not say which operator they mean.
pub const MIN_TAU: f64 = 0.51;

/// The rate the three third-order odd moments relax at under [`Collision::Mrt`].
///
/// Swept on the same bluff body the other floors were measured on
/// (`each_collision_operator_holds_the_relaxation_time_it_claims_to`). Unlike the Smagorinsky
/// constant this one is not a model of anything -- those moments are orthogonal to everything
/// the fluid has, so no value of it is more physical than another and only stability chooses.
pub const GHOST_RATE: f64 = 1.0;

/// The lowest relaxation time [`Collision::Mrt`] holds; see [`Collision::min_tau`].
pub const MRT_MIN_TAU: f64 = 0.501;

/// The Smagorinsky constant, the usual value for a free shear flow.
///
/// It is the one adjustable number in the turbulence model and it is not derived from anything;
/// values from 0.1 to 0.2 are what the literature uses, the lower end near a wall.
pub const SMAGORINSKY: f64 = 0.16;

/// The cell Reynolds number a lattice-Boltzmann flow resolves without a turbulence model.
///
/// `Re_cell = U*dx/nu` is how much inertia one cell carries, and it is the number that decides
/// whether a lattice resolves a flow -- not the Reynolds number of the whole body, which can be
/// as large as you like given enough cells. Past about ten, the smallest eddies the flow wants
/// to make are smaller than a cell, and what comes back is a laminar answer to a flow that is
/// not laminar. It is still an answer about *something*; it is not an answer about turbulence.
pub const RESOLVED_CELL_REYNOLDS: f64 = 10.0;

/// The largest share of the lattice's speed of sound a flow may use.
///
/// The method is a low-Mach approximation: the error in treating the fluid as incompressible
/// grows as the square of the Mach number. A tenth of the speed of sound keeps that error under
/// a per cent or so, and is the figure the literature settles on.
pub const MAX_LATTICE_SPEED: f64 = 0.1;

/// The smallest share of the lattice's speed of sound a flow is worth running at.
///
/// The compressibility error falls as the lattice speed is lowered, so for a long time the
/// advice here was simply that smaller is more accurate. It is not, and the other end has
/// nothing to do with the method: the distributions are `f32`, and the force on a body is a sum
/// of differences of numbers near `w_i·rho` that comes out near `rho·a`. The slower the flow,
/// the more of that difference is cancellation, so the round-off in the answer grows as `1/u`
/// while the compressibility error shrinks as `u²`. Accuracy is therefore a U, not a slope.
///
/// Measured on a forced channel at one fixed lattice, sweeping the forcing over four decades
/// (`zz_wall_precision_floor`): the force's relative error falls by about tenfold for every
/// decade of speed, right up to where the lattice goes supersonic, and the velocity field's
/// error bottoms out around a hundredth of the speed of sound and rises below it -- four times
/// its best value by a three-hundredth, forty times by a three-thousandth. Below this figure
/// the arithmetic, not the scheme, is what the answer is made of.
pub const MIN_LATTICE_SPEED: f64 = 0.01;

/// How a cell takes part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Fluid,
    /// The model. Distributions arriving here turn round and go back.
    Solid,
}

/// What happens at one of the lattice's six sides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Side {
    /// What leaves one side arrives at the other. Exact, and what a slice of an endless channel
    /// needs.
    Periodic,
    /// A wall the fluid sticks to.
    NoSlip,
    /// A wall the fluid slides along: the velocity across the side is turned round and the rest
    /// is kept. A plane of symmetry, or the far wall of a wind tunnel that should not drag.
    FreeSlip,
    /// Fluid arrives at the study's velocity.
    Inlet,
    /// Fluid leaves, with the pressure held at the reference.
    Outlet,
}

/// Where a bounce-back wall is taken to be.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wall {
    /// Half a cell beyond the last fluid node, whatever the model says.
    ///
    /// What plain bounce-back does: a distribution that reaches the model turns round and comes
    /// back, which puts the wall exactly halfway along the link. For a face square to an axis
    /// and landing on the lattice that is where the wall is. For anything else it is not, and
    /// the error does not go away as the lattice is refined -- it is a staircase, not a
    /// discretisation.
    HalfWay,
    /// Where the model's surface actually crosses the link.
    ///
    /// Bouzidi, Firdaouss and Lallemand (2001). Each link from a fluid node into the model is
    /// cut at the fraction `q` where the surface really is, found by bisection against the
    /// model, and what comes back is interpolated so that the wall sits there rather than at a
    /// half:
    ///
    /// ```text
    /// q < 1/2:  f_i(x) = 2q·f*_ī(x) + (1 − 2q)·f*_ī(x + c_i)
    /// q ≥ 1/2:  f_i(x) = f*_ī(x)/(2q) + (1 − 1/(2q))·f*_i(x)
    /// ```
    ///
    /// Both are `f*_ī(x)` at `q = 1/2`, so a wall that really is halfway is unchanged.
    ///
    /// The point is not only accuracy. Under [`Wall::HalfWay`] the wall's position depends on
    /// how the antisymmetric moments are damped, which is why the magic parameter exists and
    /// why relaxing those moments for stability costs the wall ([`Collision::Mrt`]). Here the
    /// position comes from the geometry instead, so the two stop being the same question.
    Interpolated,
}

/// How the nineteen distributions at a cell relax.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Collision {
    /// One relaxation rate for all of them (Bhatnagar, Gross and Krook). The simplest, and
    /// where the wall ends up depends slightly on the viscosity.
    Bgk,
    /// Two rates: one for the symmetric part of each opposite pair, which sets the viscosity,
    /// and one for the antisymmetric part, which sets where a bounce-back wall is.
    ///
    /// `magic` is `Λ = (1/ω₊ − 1/2)(1/ω₋ − 1/2)`. At `3/16` a bounce-back wall sits exactly
    /// half a cell beyond the last fluid cell whatever the viscosity (Ginzburg and d'Humières,
    /// 2003), which is what makes a channel's profile come out right rather than nearly right.
    Trt { magic: f64 },
    /// As [`Collision::Trt`], but the symmetric half of the departure from equilibrium is
    /// rebuilt from the stress it carries before it is relaxed.
    ///
    /// Nineteen distributions carry ten quantities the fluid actually has -- its density, its
    /// momentum and the six components of its stress -- and nine that it does not. Those nine
    /// "ghost" moments are a property of the lattice, not of the fluid, and the Chapman–Enskog
    /// expansion says nothing about what they should be; they are what the scheme carries
    /// around and, at a relaxation time near a half, what it eventually comes apart on
    /// (Lallemand and Luo, 2000).
    ///
    /// This throws the even ones away each step rather than relaxing them: the symmetric half
    /// is replaced by the second-order Hermite reconstruction of `Π^neq`
    /// (Latt and Chopard, 2006),
    ///
    /// ```text
    /// n_i = w_i/(2 c_s⁴) · (c_i ⊗ c_i − c_s² I) : Π^neq
    /// ```
    ///
    /// which is an exact projection: it leaves the density, the momentum and `Π^neq` itself
    /// untouched to the last bit and annihilates everything else. The viscosity is therefore
    /// the one asked for, and a flow the lattice already resolves is unchanged.
    ///
    /// The antisymmetric half is deliberately *not* projected. A second-order reconstruction is
    /// symmetric by construction -- `c_i ⊗ c_i` cannot tell a direction from its reverse -- so
    /// regularising the whole departure would annihilate the antisymmetric half outright, and
    /// that half is what `magic` works on to hold a bounce-back wall still. Full regularisation
    /// buys stability by giving the walls back, which is a bad trade for a tool that measures
    /// drag on a surface. Here the odd moments relax at `ω₋` exactly as under
    /// [`Collision::Trt`], so the wall stays where it was put.
    Regularized { magic: f64 },
    /// As [`Collision::Regularized`], and all six odd ghost moments get a rate of their own as
    /// well: a multiple-relaxation-time operator with three rates rather than two.
    ///
    /// Regularising the symmetric half deals with the three even ghost moments. The six odd
    /// ones -- [`GHOST3`] -- are what is left, and under two rates they are dragged to a
    /// standstill as `τ` approaches a half, because `ω₋ = 1/(1/2 + Λ/(τ − 1/2))` goes to zero.
    /// Here they relax at `ghost` instead. So the rates are `ω₊` for the stress, which is the
    /// viscosity, one for the even ghosts, which is regularisation, and `ghost` for the six odd
    /// ghosts. Nothing physical can move, because [`GHOST3`] is orthogonal to the density, the
    /// momentum and every component of the stress.
    ///
    /// It reaches `τ = 0.501` -- twenty times the Reynolds number two rates hold on the same
    /// lattice -- and that costs the wall: `ω₋` no longer acts on anything, so `magic` no longer
    /// holds a bounce-back wall still. On the channel it is out by a few per cent rather than
    /// four parts in ten thousand, which is better than one rate and nothing like
    /// [`Collision::Regularized`]. Choose it for a flow the others cannot run at all.
    ///
    /// `ghost` at `ω₋` is exactly [`Collision::Regularized`] again, which is the cheapest check
    /// there is that the extra rate is the only difference.
    ///
    /// This replaced regularisation as the literature means it -- dropping the antisymmetric
    /// half outright -- which reaches the same relaxation time and is far worse at a wall. The
    /// reason is that the odd half of `f^neq` is not all ghost when a body force is driving the
    /// flow: the equilibrium is built at `u = m/ρ + a/2`, so the departure carries a first
    /// moment of `−ρa/2`, which is the forcing and not a ghost. Dropping the half deletes it;
    /// projecting onto [`GHOST3`] leaves it exactly alone
    /// (`a_body_force_puts_the_forcing_in_the_first_moment_of_the_departure`).
    Mrt { magic: f64, ghost: f64 },
}

impl Collision {
    /// Two-relaxation-time with the magic parameter that fixes the wall.
    pub const fn trt() -> Collision {
        Collision::Trt { magic: 3.0 / 16.0 }
    }

    /// [`Collision::Regularized`] with the same magic parameter as [`Collision::trt`].
    pub const fn regularized() -> Collision {
        Collision::Regularized { magic: 3.0 / 16.0 }
    }

    /// [`Collision::Mrt`] with the wall-holding magic parameter and the measured ghost rate.
    pub const fn mrt() -> Collision {
        Collision::Mrt { magic: 3.0 / 16.0, ghost: GHOST_RATE }
    }

    /// Whether the symmetric half is rebuilt from the stress before it is relaxed.
    pub const fn regularises(&self) -> bool {
        matches!(self, Collision::Regularized { .. } | Collision::Mrt { .. })
    }

    /// The rate the three third-order odd moments relax at, where they have one of their own.
    pub const fn ghost_rate(&self) -> Option<f64> {
        match self {
            Collision::Mrt { ghost, .. } => Some(*ghost),
            _ => None,
        }
    }

    /// Whether the antisymmetric half -- the other six ghost moments, and the wall's place with
    /// them -- is dropped rather than relaxed.
    pub const fn drops_odd_moments(&self) -> bool {
        false
    }

    /// Whether a bounce-back wall stays put as the viscosity changes.
    ///
    /// True for the operators that relax the antisymmetric half at its own rate. The others
    /// leave the wall to drift with the viscosity, which a flow measured at a surface cannot
    /// afford; [`Collision::Mrt`] says why it gives that up.
    pub const fn holds_the_wall(&self) -> bool {
        matches!(self, Collision::Trt { .. } | Collision::Regularized { .. })
    }

    /// The lowest relaxation time this operator holds a bluff body at.
    ///
    /// Measured, not reasoned: a six-cell cube in a 40×20×20 tunnel at a twentieth of a cell a
    /// step, run four thousand steps at falling `τ` until the distributions stop being numbers
    /// (`each_collision_operator_holds_the_relaxation_time_it_claims_to`). The lowest that held
    /// was 0.505 for one rate, 0.52 for two, 0.51 once the three even ghost moments are thrown
    /// away, and 0.501 once the six odd ones get a rate of their own as well. Since
    /// `ν = (τ − 1/2)/3`, those are Reynolds numbers in the ratio 4 : 1 : 2 : 20 for the same
    /// lattice and speed.
    ///
    /// Two rates are the *least* stable of them, which is worth saying plainly because it is
    /// the opposite of what "better operator" suggests. `ω₋ = 1/(1/2 + Λ/(τ − 1/2))` goes to
    /// zero as `τ` goes to a half, so the magic parameter that holds the wall still stops
    /// damping the antisymmetric ghost moments at all just when they most need it. All six of
    /// them are ghosts -- the momentum is in the equilibrium set -- so there is nothing
    /// physical keeping them in check either.
    ///
    /// This is one case on one lattice, so it is a floor that was observed to hold rather than
    /// one proven to. [`Mapping::caveat`] reports whenever it had to be used.
    pub const fn min_tau(&self) -> f64 {
        match self {
            Collision::Bgk => 0.505,
            Collision::Trt { .. } => 0.52,
            Collision::Regularized { .. } => 0.51,
            Collision::Mrt { .. } => MRT_MIN_TAU,
        }
    }

    /// The name this operator goes by in a study's parameters and in its reply.
    pub const fn name(&self) -> &'static str {
        match self {
            Collision::Bgk => "bgk",
            Collision::Trt { .. } => "trt",
            Collision::Regularized { .. } => "regularized",
            Collision::Mrt { .. } => "mrt",
        }
    }
}

/// A flow study, in lattice units.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowStudy {
    /// Relaxation time. The viscosity it gives is `(τ − 1/2)/3`, so `τ` must be above a half,
    /// and a `τ` very close to it is a flow the lattice cannot resolve.
    pub tau: f64,
    pub collision: Collision,
    /// A uniform acceleration driving the flow: what a pressure gradient along a channel is.
    pub accel: [f64; 3],
    /// What arrives at a side set to [`Side::Inlet`].
    pub inlet: [f64; 3],
    /// The six sides, in the order −x, +x, −y, +y, −z, +z.
    pub sides: [Side; 6],
    /// Most steps to take.
    pub steps: u32,
    /// Stop early once the largest change in any cell's velocity over `check_every` steps falls
    /// below this. Zero runs every step.
    pub settle: f64,
    pub check_every: u32,
    /// Where a bounce-back wall is taken to be. [`Wall::Interpolated`] needs the model's
    /// surface, so it only does anything for a flow built with [`Flow::around_surface`].
    pub wall: Wall,
    /// The Smagorinsky constant of a subgrid turbulence model, or `None` to run laminar.
    ///
    /// Without this, a flow is only representable while a single cell carries little inertia
    /// (see [`RESOLVED_CELL_REYNOLDS`]), and for anything at engineering speeds that means a
    /// lattice nobody can afford -- the relaxation time falls to a half and the scheme comes
    /// apart. The model stands in for the eddies smaller than a cell by adding viscosity where
    /// the flow is being strained, which is where those eddies would have been. It is a model,
    /// not a solution: it does not say what the small eddies did, only roughly how much
    /// momentum they would have moved.
    ///
    /// Where a flow *is* resolved the added viscosity is negligible and the answer is the
    /// laminar one, which is the property that makes it safe to leave on.
    pub turbulence: Option<f64>,
}

impl FlowStudy {
    /// A study with nothing happening: periodic all round, at rest. Something has to be added
    /// to it -- an acceleration, or an inlet -- or nothing will move.
    pub fn new(tau: f64) -> FlowStudy {
        FlowStudy {
            tau,
            // Two rates with the even ghost moments thrown away: the wall of
            // [`Collision::Trt`] and a lower relaxation time than it can hold. See
            // [`Collision::min_tau`].
            collision: Collision::regularized(),
            wall: Wall::HalfWay,
            accel: [0.0; 3],
            inlet: [0.0; 3],
            sides: [Side::Periodic; 6],
            steps: 10_000,
            settle: 1e-7,
            check_every: 100,
            turbulence: None,
        }
    }

    /// The same study with a subgrid turbulence model.
    pub fn turbulent(mut self, constant: f64) -> FlowStudy {
        self.turbulence = Some(constant);
        self
    }

    /// The kinematic viscosity this study's relaxation time gives, in lattice units.
    pub fn viscosity(&self) -> f64 {
        CS2 * (self.tau - 0.5)
    }

    /// The magic parameter, or `None` for one rate.
    fn magic(&self) -> Option<f64> {
        match self.collision {
            Collision::Bgk => None,
            Collision::Trt { magic } | Collision::Regularized { magic } | Collision::Mrt { magic, .. } => Some(magic),
            // Nothing antisymmetric survives for a second rate to act on.
        }
    }

    fn check(&self) -> Result<()> {
        if !(self.tau > 0.5) || !self.tau.is_finite() {
            return setup(format!("a relaxation time of {}; it must be above a half, since the viscosity it gives is (τ − 1/2)/3", self.tau));
        }
        if self.tau > 1e4 {
            return setup(format!("a relaxation time of {}, which is a fluid thicker than anything", self.tau));
        }
        for v in self.accel.iter().chain(&self.inlet) {
            if !v.is_finite() {
                return setup("an acceleration or an inlet velocity that is not a number");
            }
        }
        if speed(self.inlet) > MAX_LATTICE_SPEED {
            return setup(format!(
                "an inlet of {:.3} lattice units, and the method holds only well under the lattice's own speed of sound; \
                 keep it under {MAX_LATTICE_SPEED}",
                speed(self.inlet)
            ));
        }
        if let Collision::Trt { magic } = self.collision
            && (!(magic > 0.0) || !magic.is_finite())
        {
            return setup(format!("a magic parameter of {magic}; it must be above zero (3/16 puts a wall exactly halfway)"));
        }
        if self.steps == 0 {
            return setup("a flow of no steps");
        }
        if !(self.settle >= 0.0) || !self.settle.is_finite() {
            return setup("a settling threshold that is not a number");
        }
        if let Some(c) = self.turbulence
            && (!(0.0..=1.0).contains(&c) || !c.is_finite())
        {
            return setup(format!("a Smagorinsky constant of {c}; the usual range is 0.1 to 0.2, and it may not be negative"));
        }
        Ok(())
    }
}

fn speed(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// How a flow went.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlowReport {
    pub steps: u32,
    /// The largest change in any cell's velocity over the last stretch of steps. Small means
    /// the flow has stopped changing; a flow that sheds a wake never gets small, and that is
    /// not a failure.
    pub change: f64,
    pub settled: bool,
    /// The largest speed anywhere, in lattice units. Well under the lattice's speed of sound is
    /// what keeps the answer meaningful.
    pub fastest: f64,
    /// Total mass on the lattice. It should not move: a flow that is losing or gaining mass has
    /// a boundary condition that does not add up.
    pub mass: f64,
}

/// The nineteen distributions of every cell of a lattice, and the two steps that advance them.
#[derive(Clone, Debug)]
pub struct Flow {
    grid: Grid3,
    cell: Vec<Cell>,
    /// `Q` numbers per cell. Single precision: the distributions carry about seven digits, and
    /// the velocity they encode is a part in ten thousand of that, which is far finer than the
    /// discretisation itself -- and it halves what a flow costs to hold.
    f: Vec<f32>,
    next: Vec<f32>,
    /// Cells whose sliding needs thinking about: ones on the edge of the lattice, and ones with
    /// the model next to them. Everywhere else a distribution comes from a fixed offset in the
    /// array, which is the fast path and is nearly all of it.
    special: Vec<bool>,
    /// Where each cell's link cuts sit in `cut`, or `u32::MAX` for a cell with none.
    cut_of: Vec<u32>,
    /// For every cell that has a link into the model, the fraction along each link at which the
    /// model's surface really is. A half everywhere is plain bounce-back.
    cut: Vec<[f32; Q]>,
    /// Links from a fluid cell into the model, as (cell, direction). Where the force is.
    walls: Vec<(u32, u8)>,
    /// The index offset of one step along each direction.
    offset: [isize; Q],
    /// Momentum the solid has taken from the fluid since the last reading.
    force: [f64; 3],
}

impl Flow {
    /// A flow around the model the occupancy holds.
    pub fn around(occ: &Occupancy, study: &FlowStudy) -> Result<Flow> {
        let grid = occ.grid().clone();
        Flow::build(grid, |c| occ.is_solid(c), study)
    }

    /// A flow around the model, with each wall link cut where the surface really crosses it.
    ///
    /// `inside` answers whether a point in millimetres is in the model, which is what
    /// `solidcraft_sim::cut::inside_of` gives for a mesh. It is asked for about twenty points
    /// per link, once, when the flow is built; after that the cuts are a table.
    ///
    /// Only [`Wall::Interpolated`] reads them, but they are measured either way, so a study can
    /// be run both ways on one lattice and compared.
    pub fn around_surface(occ: &Occupancy, inside: &dyn Fn(Vec3) -> bool, study: &FlowStudy) -> Result<Flow> {
        let mut flow = Flow::around(occ, study)?;
        flow.measure_cuts(inside);
        Ok(flow)
    }

    /// Find, for every link from a fluid cell into the model, how far along it the surface is.
    ///
    /// A link runs from the fluid node to the solid node next to it. Walking out from the fluid
    /// node, the fraction wanted is where `inside` first becomes true, which bisection finds to
    /// a millionth in twenty steps. Where the solid node's own centre is not inside the model --
    /// a cell counted solid because most of it is, with its middle in the air -- there is no
    /// crossing to find on this link and it keeps the half it had.
    fn measure_cuts(&mut self, inside: &dyn Fn(Vec3) -> bool) {
        let h = self.grid.spacing();
        for (c, i) in self.walls.clone() {
            let (c, i) = (c as usize, i as usize);
            let Some(block) = self.cut_of.get(c).copied().filter(|b| *b != u32::MAX) else { continue };
            let (Some(from), Some(dir)) = (self.grid.centre(c), C.get(opposite(i))) else { continue };
            let step = Vec3::new(dir[0] as f64 * h, dir[1] as f64 * h, dir[2] as f64 * h);
            if !inside(from + step) || inside(from) {
                continue;
            }
            let (mut lo, mut hi) = (0.0f64, 1.0f64);
            for _ in 0..20 {
                let mid = 0.5 * (lo + hi);
                if inside(from + step * mid) { hi = mid } else { lo = mid }
            }
            let q = (0.5 * (lo + hi)).clamp(1e-3, 1.0);
            if let Some(slot) = self.cut.get_mut(block as usize).and_then(|b| b.get_mut(i)) {
                *slot = q as f32;
            }
        }
    }

    /// A flow with nothing in the way: a channel, or a box of fluid.
    pub fn through(grid: Grid3, study: &FlowStudy) -> Result<Flow> {
        Flow::build(grid, |_| false, study)
    }

    fn build(grid: Grid3, solid: impl Fn(usize) -> bool, study: &FlowStudy) -> Result<Flow> {
        study.check()?;
        let cells = grid.cells();
        if cells > MAX_FLOW_CELLS {
            return Err(SimError::TooLarge { cells: cells as u64, limit: MAX_FLOW_CELLS as u64 });
        }
        let Some(total) = cells.checked_mul(Q) else {
            return Err(SimError::TooLarge { cells: cells as u64, limit: MAX_FLOW_CELLS as u64 });
        };
        let cell: Vec<Cell> = (0..cells).map(|c| if solid(c) { Cell::Solid } else { Cell::Fluid }).collect();
        if !cell.contains(&Cell::Fluid) {
            return Err(SimError::Domain("the model fills the lattice, so there is nowhere for the air to be".into()));
        }
        let n = grid.count();
        let mut offset = [0isize; Q];
        for (i, c) in C.iter().enumerate() {
            let Some(slot) = offset.get_mut(i) else { continue };
            *slot = c[0] as isize + (n[0] as isize) * (c[1] as isize + (n[1] as isize) * c[2] as isize);
        }
        // The sides and the model do not move, so which cells need the careful treatment is
        // settled once. Everywhere else, what arrives along a direction is simply what sat one
        // fixed offset back in the array.
        let mut special = vec![false; cells];
        let mut walls: Vec<(u32, u8)> = Vec::new();
        for c in 0..cells {
            if cell.get(c).copied() != Some(Cell::Fluid) {
                continue;
            }
            let Some(at) = grid.ijk(c) else { continue };
            let edge = at.iter().zip(&n).any(|(p, across)| *p == 0 || p + 1 >= *across);
            let mut touching = false;
            for i in 1..Q {
                match step_back(at, i, n, &study.sides) {
                    From::Cell(src) if cell.get(src).copied() == Some(Cell::Solid) => {
                        touching = true;
                        walls.push((c as u32, i as u8));
                    }
                    From::Cell(_) => {}
                    _ => {}
                }
            }
            if let Some(slot) = special.get_mut(c) {
                *slot = edge || touching;
            }
        }
        // Every link is cut halfway until a surface says otherwise, which is plain bounce-back.
        let mut cut_of = vec![u32::MAX; cells];
        let mut cut: Vec<[f32; Q]> = Vec::new();
        for (c, _) in walls.iter() {
            let c = *c as usize;
            if cut_of.get(c).copied() == Some(u32::MAX) {
                let Ok(at) = u32::try_from(cut.len()) else { break };
                if let Some(slot) = cut_of.get_mut(c) {
                    *slot = at;
                }
                cut.push([0.5; Q]);
            }
        }
        let mut flow = Flow { grid, cell, f: vec![0.0; total], next: vec![0.0; total], special, cut_of, cut, walls, offset, force: [0.0; 3] };
        flow.rest();
        Ok(flow)
    }

    /// Set every cell to fluid at rest and the reference density.
    pub fn rest(&mut self) {
        self.set_everywhere(|_| (1.0, Vec3::ZERO));
    }

    /// Set every cell to the equilibrium of a density and a velocity worked out from its
    /// position. This is how a verification case starts from a known field.
    pub fn set_everywhere(&mut self, mut state: impl FnMut(usize) -> (f64, Vec3)) {
        for c in 0..self.cell.len() {
            let (rho, u) = state(c);
            let eq = equilibrium(rho, u);
            for (i, v) in eq.iter().enumerate() {
                if let Some(slot) = self.f.get_mut(c * Q + i) {
                    *slot = *v as f32;
                }
            }
        }
    }

    pub fn grid(&self) -> &Grid3 {
        &self.grid
    }

    pub fn is_solid(&self, cell: usize) -> bool {
        self.cell.get(cell).copied() == Some(Cell::Solid)
    }

    /// How many cells the fluid has.
    pub fn fluid_cells(&self) -> usize {
        self.cell.iter().filter(|c| **c == Cell::Fluid).count()
    }

    /// The density at a cell: the sum of its nineteen distributions.
    pub fn density(&self, cell: usize) -> f64 {
        let Some(f) = self.f.get(cell * Q..cell * Q + Q) else { return 0.0 };
        f.iter().map(|v| *v as f64).sum()
    }

    /// The velocity at a cell, in lattice units: its momentum over its density, with half the
    /// body force added, which is where Guo's forcing scheme puts it.
    pub fn velocity(&self, cell: usize, accel: [f64; 3]) -> Vec3 {
        let Some(f) = self.f.get(cell * Q..cell * Q + Q) else { return Vec3::ZERO };
        let (rho, m) = moments(f);
        if !(rho > 0.0) {
            return Vec3::ZERO;
        }
        Vec3::new(m[0] / rho + accel[0] / 2.0, m[1] / rho + accel[1] / 2.0, m[2] / rho + accel[2] / 2.0)
    }

    /// The pressure at a cell relative to the reference, in lattice units.
    pub fn pressure(&self, cell: usize) -> f64 {
        CS2 * (self.density(cell) - 1.0)
    }

    /// The momentum the solid has taken from the fluid since this was last read, in lattice
    /// units: the drag and the lift, by Ladd's momentum exchange.
    pub fn force(&self) -> Vec3 {
        Vec3::new(self.force[0], self.force[1], self.force[2])
    }

    /// Advance one step: relax, read off what the walls took, then slide.
    pub fn step(&mut self, study: &FlowStudy) -> Result<()> {
        let (omega_plus, omega_minus) = rates(study)?;
        self.collide(study, omega_plus, omega_minus)?;
        self.tally_walls(study);
        self.stream(study);
        std::mem::swap(&mut self.f, &mut self.next);
        self.boundaries(study);
        Ok(())
    }

    /// Relax every cell's nineteen towards their own equilibrium.
    ///
    /// Nothing here looks at a neighbour, so the cells divide over cores with nothing to
    /// coordinate; a number that stops being a number is recorded and the step refused after,
    /// since a parallel loop has nowhere to return from.
    fn collide(&mut self, study: &FlowStudy, omega_plus: f64, omega_minus: f64) -> Result<()> {
        let accel = study.accel;
        let forced = accel != [0.0; 3];
        let (tau, magic, smagorinsky) = (study.tau, study.magic(), study.turbulence);
        let (regularises, drops_odd) = (study.collision.regularises(), study.collision.drops_odd_moments());
        let ghost_rate = study.collision.ghost_rate();
        let Flow { cell, f, .. } = self;
        let cell = &*cell;
        let lost = AtomicBool::new(false);
        par::for_each_chunk_mut(f, Q, |c, f| {
            if cell.get(c).copied() != Some(Cell::Fluid) {
                return;
            }
            let (rho, m) = moments(f);
            if !(rho > 0.0) {
                lost.store(true, Ordering::Relaxed);
                return;
            }
            let u = Vec3::new(m[0] / rho + accel[0] / 2.0, m[1] / rho + accel[1] / 2.0, m[2] / rho + accel[2] / 2.0);
            let eq = equilibrium(rho, u);
            // With a turbulence model the relaxation is a local matter: the eddy viscosity is
            // read off how hard this cell is being strained, which the departure from
            // equilibrium already carries.
            // The stress is what both the turbulence model and the regularisation read, so it
            // is gathered once and only where one of them wants it.
            let pi = (regularises || smagorinsky.is_some()).then(|| stress(f, &eq));
            let (omega_plus, omega_minus) = match (smagorinsky, &pi) {
                (Some(constant), Some(pi)) => {
                    // Both rates follow the thickened relaxation time. Holding the
                    // antisymmetric one at the fluid's own viscosity instead was tried and is
                    // markedly less stable: at a relaxation time near the floor it comes out
                    // around a twentieth, which barely relaxes the antisymmetric modes at all.
                    let total = eddy_relaxation(tau, constant, rho, strain_norm(pi));
                    match pair(total, magic) {
                        Some(rates) => rates,
                        None => {
                            lost.store(true, Ordering::Relaxed);
                            return;
                        }
                    }
                }
                _ => (omega_plus, omega_minus),
            };
            // The symmetric half each distribution is relaxed from, where it is being rebuilt
            // from the stress rather than taken as it stands.
            let rebuilt = pi.as_ref().filter(|_| regularises).map(regularised);
            // How much of the antisymmetric half lies along the three moments that get a rate
            // of their own, where there is one.
            let share = ghost_rate.map(|_| odd_ghosts(f, &eq));
            let mut out = [0.0f64; Q];
            for i in 0..Q {
                let (fi, ei) = (f.get(i).copied().unwrap_or(0.0) as f64, eq.get(i).copied().unwrap_or(0.0));
                let o = opposite(i);
                let (fo, eo) = (f.get(o).copied().unwrap_or(0.0) as f64, eq.get(o).copied().unwrap_or(0.0));
                // The symmetric half of an opposite pair relaxes at one rate and the
                // antisymmetric half at the other. With one rate for both this is plain BGK.
                let minus = 0.5 * ((fi - ei) - (fo - eo));
                let mut v = match &rebuilt {
                    None => {
                        let plus = 0.5 * ((fi - ei) + (fo - eo));
                        fi - omega_plus * plus - omega_minus * minus
                    }
                    // With the symmetric half rebuilt, the distribution has to be built back up
                    // from equilibrium rather than stepped down from where it was: what it was
                    // carries the ghost moments that are being thrown away. For the half that
                    // is not rebuilt the two forms are the same sum in a different order.
                    Some(rebuilt) => {
                        let plus = rebuilt.get(i).copied().unwrap_or(0.0);
                        // Dropping the antisymmetric half drops the remaining six ghost moments
                        // with it, and where the wall sits along with them.
                        let minus = if drops_odd { 0.0 } else { minus };
                        match (ghost_rate, &share) {
                            // The part of the antisymmetric half lying along the three
                            // third-order moments is taken out and put back at its own rate;
                            // what is left is the energy fluxes, which keep `ω₋` and the wall.
                            (Some(rate), Some(share)) => {
                                let along: f64 = (0..6)
                                    .map(|a| share.get(a).copied().unwrap_or(0.0) * GHOST3.get(a).and_then(|b| b.get(i)).copied().unwrap_or(0.0))
                                    .sum();
                                ei + (1.0 - omega_plus) * plus + (1.0 - omega_minus) * (minus - along) + (1.0 - rate) * along
                            }
                            _ => ei + (1.0 - omega_plus) * plus + (1.0 - omega_minus) * minus,
                        }
                    }
                };
                if forced {
                    v += forcing(i, u, accel, omega_plus, omega_minus);
                }
                if let Some(slot) = out.get_mut(i) {
                    *slot = v;
                }
            }
            if !out.iter().all(|v| v.is_finite()) {
                lost.store(true, Ordering::Relaxed);
                return;
            }
            for (i, v) in out.iter().enumerate() {
                if let Some(slot) = f.get_mut(i) {
                    *slot = *v as f32;
                }
            }
        });
        if lost.load(Ordering::Relaxed) {
            return Err(SimError::NotFinite("a distribution"));
        }
        Ok(())
    }

    /// Add up the momentum the model took this step.
    ///
    /// Along a link from a fluid cell into the model, the fluid's own momentum turns from
    /// `−c·back` to `+c·back`, a change of `2·c·back`, and by conservation the body took the
    /// opposite of that. This is Ladd's momentum exchange (1994) and it is exact: nothing is
    /// differentiated and no stress is reconstructed, the force is simply the momentum that
    /// changed hands. It is read after the relaxation and before the sliding, which is when the
    /// distribution heading for the wall is the one about to make the trip.
    fn tally_walls(&mut self, study: &FlowStudy) {
        let mut taken = [0.0f64; 3];
        let interpolate = study.wall == Wall::Interpolated;
        for (c, i) in &self.walls {
            let (c, i) = (*c as usize, *i as usize);
            // What arrived at the wall, and what the wall sent back. Those are the same number
            // only when the wall is halfway along the link.
            let sent = self.f.get(c * Q + opposite(i)).copied().unwrap_or(0.0) as f64;
            let back = if interpolate { off_the_wall(&self.f, &self.cell, &self.offset, &self.cut_of, &self.cut, c, i) as f64 } else { sent };
            let ci = C.get(i).copied().unwrap_or([0; 3]);
            for a in 0..3 {
                if let (Some(t), Some(cc)) = (taken.get_mut(a), ci.get(a)) {
                    *t -= (sent + back) * (*cc as f64);
                }
            }
        }
        for a in 0..3 {
            if let (Some(f), Some(t)) = (self.force.get_mut(a), taken.get(a)) {
                *f += t;
            }
        }
    }

    /// Slide every distribution one cell along its own direction, turning round at a wall.
    ///
    /// Pulling rather than pushing -- each cell gathers what is arriving instead of scattering
    /// what is leaving -- means every cell writes only its own nineteen, so this divides over
    /// cores as cleanly as the relaxation does. Away from the lattice's edges and the model, a
    /// distribution comes from a fixed offset back in the array and the whole question is a
    /// subtraction; only the cells marked special have to work out where they are.
    fn stream(&mut self, study: &FlowStudy) {
        let n = self.grid.count();
        let grid = &self.grid;
        let sides = &study.sides;
        let Flow { cell, f, next, special, offset, cut_of, cut, .. } = self;
        let (cell, f, special, offset) = (&*cell, &*f, &*special, &*offset);
        let (cut_of, cut) = (&*cut_of, &*cut);
        let interpolate = study.wall == Wall::Interpolated;
        par::for_each_chunk_mut(next, Q, |c, out| {
            if cell.get(c).copied() != Some(Cell::Fluid) {
                out.fill(0.0);
                return;
            }
            if !special.get(c).copied().unwrap_or(true) {
                for i in 0..Q {
                    let Some(step) = offset.get(i) else { continue };
                    let src = c as isize - step;
                    let Some(v) = usize::try_from(src).ok().and_then(|s| f.get(s * Q + i)) else { continue };
                    if let Some(slot) = out.get_mut(i) {
                        *slot = *v;
                    }
                }
                return;
            }
            let Some(at) = grid.ijk(c) else { return };
            // What plain bounce-back would have returned, less what the interpolation actually
            // returns. Bouzidi's rule does not give back exactly what was sent, so without this
            // a closed lattice bleeds: a third of its mass over four hundred thousand steps.
            let mut owed = 0.0f32;
            for i in 0..Q {
                let value = match step_back(at, i, n, sides) {
                    From::Cell(src) if cell.get(src).copied() == Some(Cell::Fluid) => f.get(src * Q + i).copied().unwrap_or(0.0),
                    // The model is behind us: what we sent that way comes straight back.
                    // `tally_walls` has already taken the momentum of the round trip.
                    From::Cell(_) if interpolate => {
                        let back = off_the_wall(f, cell, offset, cut_of, cut, c, i);
                        owed += f.get(c * Q + opposite(i)).copied().unwrap_or(0.0) - back;
                        back
                    }
                    From::Cell(_) | From::Bounce => f.get(c * Q + opposite(i)).copied().unwrap_or(0.0),
                    From::Mirror(j) => f.get(c * Q + j).copied().unwrap_or(0.0),
                    From::Open => f.get(c * Q + i).copied().unwrap_or(0.0),
                };
                if let Some(slot) = out.get_mut(i) {
                    *slot = value;
                }
            }
            // Put it back in the one direction that stands still, so the node's mass is what
            // bounce-back would have left it and its momentum is untouched: `C[0]` is the zero
            // vector, so this is a density correction and not a force. What it adds is the size
            // of the interpolation's own correction, which is second order in the spacing, so
            // it cannot cost the accuracy the interpolation buys.
            if owed != 0.0
                && let Some(slot) = out.get_mut(0)
            {
                *slot += owed;
            }
        });
    }

    /// Hold the inlet and the outlet, after the sliding.
    ///
    /// Guo's non-equilibrium extrapolation: the cell takes the equilibrium of what is being
    /// held, plus the departure from equilibrium of the fluid cell just inside. That keeps the
    /// scheme second order at the boundary, where simply setting the equilibrium would not.
    fn boundaries(&mut self, study: &FlowStudy) {
        let n = self.grid.count();
        for (which, side) in study.sides.iter().enumerate() {
            let (axis, far) = (which / 2, which % 2 == 1);
            let hold = match side {
                Side::Inlet => Hold::Velocity(Vec3::new(study.inlet[0], study.inlet[1], study.inlet[2])),
                Side::Outlet => Hold::Density(1.0),
                _ => continue,
            };
            let Some(across) = n.get(axis).copied() else { continue };
            if across < 2 {
                continue;
            }
            let layer = if far { across - 1 } else { 0 };
            let inward: i64 = if far { -1 } else { 1 };
            for b in 0..n.get((axis + 1) % 3).copied().unwrap_or(0) {
                for d in 0..n.get((axis + 2) % 3).copied().unwrap_or(0) {
                    let mut at = [0usize; 3];
                    if let Some(s) = at.get_mut(axis) {
                        *s = layer;
                    }
                    if let Some(s) = at.get_mut((axis + 1) % 3) {
                        *s = b;
                    }
                    if let Some(s) = at.get_mut((axis + 2) % 3) {
                        *s = d;
                    }
                    let Some(c) = self.grid.at(at[0], at[1], at[2]) else { continue };
                    if self.cell.get(c).copied() != Some(Cell::Fluid) {
                        continue;
                    }
                    let mut inner = at;
                    let Some(slot) = inner.get_mut(axis) else { continue };
                    let moved = *slot as i64 + inward;
                    if moved < 0 || moved as usize >= across {
                        continue;
                    }
                    *slot = moved as usize;
                    let Some(nb) = self.grid.at(inner[0], inner[1], inner[2]) else { continue };
                    if self.cell.get(nb).copied() != Some(Cell::Fluid) {
                        continue;
                    }
                    let (rho_in, u_in) = self.state(nb, study.accel);
                    let (rho, u) = match hold {
                        Hold::Velocity(v) => (rho_in, v),
                        Hold::Density(r) => (r, u_in),
                    };
                    let eq_here = equilibrium(rho, u);
                    let eq_there = equilibrium(rho_in, u_in);
                    for i in 0..Q {
                        let neq = self.f.get(nb * Q + i).copied().unwrap_or(0.0) as f64 - eq_there.get(i).copied().unwrap_or(0.0);
                        let v = eq_here.get(i).copied().unwrap_or(0.0) + neq;
                        if let Some(dst) = self.f.get_mut(c * Q + i) {
                            *dst = v as f32;
                        }
                    }
                }
            }
        }
    }

    fn state(&self, cell: usize, accel: [f64; 3]) -> (f64, Vec3) {
        let Some(f) = self.f.get(cell * Q..cell * Q + Q) else { return (1.0, Vec3::ZERO) };
        let (rho, m) = moments(f);
        if !(rho > 0.0) {
            return (1.0, Vec3::ZERO);
        }
        (rho, Vec3::new(m[0] / rho + accel[0] / 2.0, m[1] / rho + accel[1] / 2.0, m[2] / rho + accel[2] / 2.0))
    }

    /// Run until the flow stops changing or the steps run out.
    pub fn run(&mut self, study: &FlowStudy) -> Result<FlowReport> {
        study.check()?;
        let every = study.check_every.max(1);
        let mut was: Vec<[f32; 3]> = self.snapshot(study.accel);
        let mut change = f64::INFINITY;
        let mut taken = 0u32;
        // The force is wanted for the developed flow, not averaged over the transient getting
        // there, so the tally starts over once the run is half done. A body that sheds a wake
        // never settles and its instantaneous force swings with the shedding; the mean over the
        // second half of the run is the number that means something, and a reading over the
        // last few steps is not.
        let mut since = 0u32;
        self.force = [0.0; 3];
        while taken < study.steps {
            self.step(study)?;
            taken += 1;
            if taken * 2 == study.steps {
                self.force = [0.0; 3];
                since = taken;
            }
            if taken.is_multiple_of(every) || taken == study.steps {
                let now = self.snapshot(study.accel);
                change = was
                    .iter()
                    .zip(&now)
                    .map(|(a, b)| {
                        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                        ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) as f64).sqrt()
                    })
                    .fold(0.0, f64::max);
                was = now;
                // Keep starting the tally over while the flow is still in its first half, so a
                // run that settles early reports the force of the settled flow rather than one
                // averaged over its own transient -- which, starting from rest, reads low. A
                // run that goes the distance is unaffected: its last restart is the halfway one
                // above, and the tally covers the second half as before.
                // `since > 0` holds off the stop for one interval on a flow that would settle
                // at its very first check. That is the one case the reset below cannot reach,
                // and without it such a run reports a force averaged from step one -- which is
                // the whole transient. A threshold loose enough to trip that early is the way
                // in: the extra interval costs `check_every` steps and buys a window that at
                // least excludes the startup. No case in the suite reaches it.
                if study.settle > 0.0 && change <= study.settle && since > 0 {
                    break;
                }
                if taken * 2 < study.steps {
                    self.force = [0.0; 3];
                    since = taken;
                }
            }
        }
        let mut fastest = 0.0f64;
        let mut mass = 0.0f64;
        for c in 0..self.cell.len() {
            if self.cell.get(c).copied() != Some(Cell::Fluid) {
                continue;
            }
            mass += self.density(c);
            fastest = fastest.max(self.velocity(c, study.accel).len());
        }
        if !fastest.is_finite() || !mass.is_finite() {
            return Err(SimError::NotFinite("the flow field"));
        }
        // Each step added the momentum that changed hands in it, so the force is the mean over
        // the steps since the tally started over.
        let over = (taken - since).max(1);
        for v in self.force.iter_mut() {
            *v /= over as f64;
        }
        Ok(FlowReport { steps: taken, change, settled: study.settle > 0.0 && change <= study.settle, fastest, mass })
    }

    fn snapshot(&self, accel: [f64; 3]) -> Vec<[f32; 3]> {
        (0..self.cell.len())
            .map(|c| {
                let u = self.velocity(c, accel);
                [u.x as f32, u.y as f32, u.z as f32]
            })
            .collect()
    }
}

enum Hold {
    Velocity(Vec3),
    Density(f64),
}

/// Where a distribution arriving along `i` came from.
enum From {
    /// A cell of the lattice (which may be solid).
    Cell(usize),
    /// Off the lattice at a no-slip side: it comes back the way it went.
    Bounce,
    /// Off the lattice at a free-slip side: it comes back mirrored, along direction `j`.
    Mirror(usize),
    /// Off the lattice at an inlet or an outlet: left as it was and then overwritten by the
    /// boundary pass.
    Open,
}

/// One step back along direction `i` from cell `at`, with the sides applied.
fn step_back(at: [usize; 3], i: usize, n: [usize; 3], sides: &[Side; 6]) -> From {
    let c = C.get(i).copied().unwrap_or([0; 3]);
    let mut to = [0i64; 3];
    for a in 0..3 {
        let (Some(slot), Some(p), Some(ci)) = (to.get_mut(a), at.get(a), c.get(a)) else { return From::Open };
        *slot = *p as i64 - *ci as i64;
    }
    // Wrap the periodic axes first, so a corner between a periodic side and a wall is handled
    // by the wall rather than by whichever axis is listed first.
    for a in 0..3 {
        let Some(across) = n.get(a).copied() else { return From::Open };
        let Some(v) = to.get_mut(a) else { return From::Open };
        if *v >= 0 && (*v as usize) < across {
            continue;
        }
        let low = *v < 0;
        let which = a * 2 + usize::from(!low);
        if sides.get(which).copied() == Some(Side::Periodic) {
            *v = if low { across as i64 - 1 } else { 0 };
        }
    }
    for a in 0..3 {
        let Some(across) = n.get(a).copied() else { return From::Open };
        let Some(v) = to.get(a).copied() else { return From::Open };
        if v >= 0 && (v as usize) < across {
            continue;
        }
        let low = v < 0;
        let which = a * 2 + usize::from(!low);
        return match sides.get(which).copied() {
            Some(Side::NoSlip) => From::Bounce,
            Some(Side::FreeSlip) => From::Mirror(mirror(i, a)),
            Some(Side::Inlet) | Some(Side::Outlet) => From::Open,
            // Periodic was wrapped above; a side that is somehow still out is treated as a wall
            // rather than read off the end of the lattice.
            _ => From::Bounce,
        };
    }
    let (Some(x), Some(y), Some(z)) = (to.first(), to.get(1), to.get(2)) else { return From::Open };
    let (Some(nx), Some(ny)) = (n.first(), n.get(1)) else { return From::Open };
    From::Cell(*x as usize + nx * (*y as usize + ny * *z as usize))
}

/// The direction `i` becomes when it is reflected in the plane square to `axis`: the component
/// along the axis turns round and the rest is kept. That is what sliding along a wall means.
fn mirror(i: usize, axis: usize) -> usize {
    let mut want = C.get(i).copied().unwrap_or([0; 3]);
    if let Some(v) = want.get_mut(axis) {
        *v = -*v;
    }
    C.iter().position(|c| *c == want).unwrap_or(i)
}

/// The equilibrium set for a density and a velocity: the second-order expansion of a Maxwellian
/// on this velocity set, which is what the Chapman–Enskog analysis needs it to be.
pub fn equilibrium(rho: f64, u: Vec3) -> [f64; Q] {
    let uu = u.dot(u);
    let mut out = [0.0; Q];
    for i in 0..Q {
        let c = C.get(i).copied().unwrap_or([0; 3]);
        let cu = c[0] as f64 * u.x + c[1] as f64 * u.y + c[2] as f64 * u.z;
        let w = W.get(i).copied().unwrap_or(0.0);
        if let Some(slot) = out.get_mut(i) {
            *slot = w * rho * (1.0 + 3.0 * cu + 4.5 * cu * cu - 1.5 * uu);
        }
    }
    out
}

/// Guo, Zheng and Shi's forcing term (2002): the one that puts a body force into the scheme
/// without leaving a spurious term in the momentum equation.
///
/// The source has to be relaxed the way the distributions are, and with two relaxation rates
/// that means splitting it. Of its three terms, `3w(c·F)` is odd under reversing the direction
/// and the other two are even, so the odd one is relaxed at the rate that governs the
/// antisymmetric part of a pair and the even ones at the rate that governs the symmetric part
/// (Ginzburg, Verhaeghe and d'Humières, 2008). With one rate for both this is Guo's own
/// `(1 − 1/2τ)·S`, unchanged.
///
/// Getting this wrong is not subtle in its effect and is not visible in the equations either:
/// the channel comes out with the right shape and the wrong flow rate, by a factor that moves
/// with the viscosity. A body force driving a channel is what catches it.
fn forcing(i: usize, u: Vec3, accel: [f64; 3], omega_plus: f64, omega_minus: f64) -> f64 {
    let c = C.get(i).copied().unwrap_or([0; 3]);
    let g = Vec3::new(accel[0], accel[1], accel[2]);
    let cu = c[0] as f64 * u.x + c[1] as f64 * u.y + c[2] as f64 * u.z;
    let cg = c[0] as f64 * g.x + c[1] as f64 * g.y + c[2] as f64 * g.z;
    let w = W.get(i).copied().unwrap_or(0.0);
    let odd = 3.0 * cg;
    let even = 9.0 * cu * cg - 3.0 * u.dot(g);
    w * ((1.0 - omega_minus / 2.0) * odd + (1.0 - omega_plus / 2.0) * even)
}

/// The density and the momentum a set of distributions carries.
fn moments(f: &[f32]) -> (f64, [f64; 3]) {
    let mut rho = 0.0;
    let mut m = [0.0f64; 3];
    for (i, v) in f.iter().enumerate() {
        let v = *v as f64;
        rho += v;
        let c = C.get(i).copied().unwrap_or([0; 3]);
        for a in 0..3 {
            if let (Some(slot), Some(ci)) = (m.get_mut(a), c.get(a)) {
                *slot += v * *ci as f64;
            }
        }
    }
    (rho, m)
}

/// The two relaxation rates a study asks for.
fn rates(study: &FlowStudy) -> Result<(f64, f64)> {
    pair(study.tau, study.magic()).ok_or_else(|| crate::SimError::Setup(format!("a relaxation time of {} gives no usable pair of rates", study.tau)))
}

/// The pair of rates a relaxation time and a magic parameter give, or `None` when they give
/// none that the scheme can use.
///
/// `Λ = (1/ω₊ − 1/2)(1/ω₋ − 1/2)`, solved for `ω₋`. A magic parameter of `None` means one rate
/// for both, which is plain BGK.
fn pair(tau: f64, magic: Option<f64>) -> Option<(f64, f64)> {
    let plus = 1.0 / tau;
    let half = tau - 0.5;
    if !(half > 0.0) {
        return None;
    }
    let minus = match magic {
        None => plus,
        Some(magic) => {
            let inverse = 0.5 + magic / half;
            if !(inverse > 0.0) || !inverse.is_finite() {
                return None;
            }
            1.0 / inverse
        }
    };
    if !(plus > 0.0 && plus < 2.0) || !(minus > 0.0 && minus < 2.0) {
        return None;
    }
    Some((plus, minus))
}

/// How hard a cell is being strained, as the norm of its non-equilibrium stress.
///
/// `Π_ab = Σ_k c_ka·c_kb·(f_k − f_k^eq)` is the momentum flux the cell carries over and above
/// what its own density and velocity account for, and to first order in the Chapman–Enskog
/// expansion it is `−2ρ·c_s²·τ·S_ab` -- so the strain rate is already in the distributions and
/// needs no finite differences and no neighbours. What is returned is
/// `sqrt(2·Σ_ab Π_ab²)`, which pairs with `|S| = sqrt(2·Σ S_ab²)`.
fn strain_norm(pi: &[[f64; 3]; 3]) -> f64 {
    let sum: f64 = pi.iter().flatten().map(|v| v * v).sum();
    (2.0 * sum).sqrt()
}

/// The stress the departure from equilibrium carries: `Π^neq_ab = Σ_i c_ia c_ib (f_i − f_i^eq)`.
///
/// This is the one piece of the departure from equilibrium that the fluid actually has. A
/// Chapman–Enskog expansion makes it `−2ρν·S_ab/ω` to leading order, so the strain rate reads
/// off it, the eddy viscosity reads off it, and so does the reconstruction that
/// [`Collision::Regularized`] puts in place of the symmetric half.
///
/// Only the symmetric half of each opposite pair reaches it: `c_ia c_ib` cannot tell a
/// direction from its reverse, so the antisymmetric half contributes nothing to it either way.
pub fn stress(f: &[f32], eq: &[f64; Q]) -> [[f64; 3]; 3] {
    let mut pi = [[0.0f64; 3]; 3];
    for i in 0..Q {
        let neq = f.get(i).copied().unwrap_or(0.0) as f64 - eq.get(i).copied().unwrap_or(0.0);
        if neq == 0.0 {
            continue;
        }
        let c = C.get(i).copied().unwrap_or([0; 3]);
        for a in 0..3 {
            for b in 0..3 {
                let (Some(ca), Some(cb)) = (c.get(a), c.get(b)) else { continue };
                if let Some(slot) = pi.get_mut(a).and_then(|row| row.get_mut(b)) {
                    *slot += neq * (*ca as f64) * (*cb as f64);
                }
            }
        }
    }
    pi
}

/// What comes back into `c` along `i` from a wall, given the post-collision distributions.
///
/// With `q` a half this is plain bounce-back: what was sent the other way comes straight back.
/// Otherwise it is Bouzidi's interpolation, which puts the wall where `q` says rather than
/// halfway along the link. [`Wall::Interpolated`] has the two cases.
///
/// Both the sliding and the force tally go through here, so the momentum the wall is credited
/// with is the momentum it actually took.
#[allow(clippy::too_many_arguments)]
fn off_the_wall(f: &[f32], cell: &[Cell], offset: &[isize; Q], cut_of: &[u32], cut: &[[f32; Q]], c: usize, i: usize) -> f32 {
    let back = f.get(c * Q + opposite(i)).copied().unwrap_or(0.0);
    let q = cut_of.get(c).copied().filter(|b| *b != u32::MAX).and_then(|b| cut.get(b as usize)).and_then(|b| b.get(i)).copied().unwrap_or(0.5) as f64;
    if q == 0.5 {
        return back;
    }
    if q < 0.5 {
        // The round trip overshoots this node, so what lands here is read between here and the
        // next node away from the model.
        let beyond = offset
            .get(i)
            .map(|step| c as isize + step)
            .and_then(|a| usize::try_from(a).ok())
            .filter(|a| cell.get(*a).copied() == Some(Cell::Fluid))
            .and_then(|a| f.get(a * Q + opposite(i)))
            .copied();
        match beyond {
            Some(v) => (2.0 * q * back as f64 + (1.0 - 2.0 * q) * v as f64) as f32,
            // No node to read from: the best left is the plain round trip.
            None => back,
        }
    } else {
        let here = f.get(c * Q + i).copied().unwrap_or(0.0);
        ((back as f64) / (2.0 * q) + (1.0 - 1.0 / (2.0 * q)) * here as f64) as f32
    }
}

/// How much of a departure from equilibrium lies along each of [`GHOST3`].
///
/// A plain orthogonal projection, `⟨M_a, n⟩ / ‖M_a‖²`. The even half of `n` contributes nothing
/// because [`GHOST3`] is odd, so this may be handed the whole departure rather than its
/// antisymmetric half.
fn odd_ghosts(f: &[f32], eq: &[f64; Q]) -> [f64; 6] {
    let mut out = [0.0f64; 6];
    for (a, basis) in GHOST3.iter().enumerate() {
        let mut dot = 0.0;
        for i in 0..Q {
            let m = basis.get(i).copied().unwrap_or(0.0);
            if m != 0.0 {
                dot += m * (f.get(i).copied().unwrap_or(0.0) as f64 - eq.get(i).copied().unwrap_or(0.0));
            }
        }
        if let Some(slot) = out.get_mut(a) {
            *slot = dot / GHOST3_NORM.get(a).copied().unwrap_or(1.0);
        }
    }
    out
}

/// The symmetric half of a departure from equilibrium that carries `pi` and nothing else.
///
/// The second-order Hermite reconstruction, `n_i = w_i/(2c_s⁴)·(c_i ⊗ c_i − c_s² I) : Π^neq`
/// (Latt and Chopard, 2006). Two lattice identities,
///
/// ```text
/// Σ_i w_i c_ia c_ib = c_s² δ_ab
/// Σ_i w_i c_ia c_ib c_ic c_id = c_s⁴ (δ_ab δ_cd + δ_ac δ_bd + δ_ad δ_bc)
/// ```
///
/// make it an exact projection rather than an approximation: feeding its own second moment back
/// through gives `½(Π_ab + Π_ba)`, which is `Π_ab` because the stress is symmetric. The zeroth
/// and first moments come out at exactly zero -- the first because the weights have no third
/// moment, the zeroth because `c_s² δ_cd − δ_cd/3` is nothing when `c_s²` is a third -- so
/// putting this in place of the symmetric half moves neither the density nor the momentum.
///
/// What it does not carry is the even ghost moments, which is the point: they are thrown away
/// rather than relaxed. [`Collision::Regularized`] says why that is worth doing.
pub fn regularised(pi: &[[f64; 3]; 3]) -> [f64; Q] {
    let mut out = [0.0f64; Q];
    for i in 0..Q {
        let c = C.get(i).copied().unwrap_or([0; 3]);
        let w = W.get(i).copied().unwrap_or(0.0);
        let mut sum = 0.0;
        for a in 0..3 {
            for b in 0..3 {
                let (Some(ca), Some(cb)) = (c.get(a), c.get(b)) else { continue };
                let Some(p) = pi.get(a).and_then(|row| row.get(b)) else { continue };
                // (c ⊗ c − c_s² I) : Π, a term at a time.
                sum += ((*ca as f64) * (*cb as f64) - if a == b { CS2 } else { 0.0 }) * p;
            }
        }
        if let Some(slot) = out.get_mut(i) {
            *slot = w * sum / (2.0 * CS2 * CS2);
        }
    }
    out
}

/// The relaxation time a cell gets once the subgrid eddies are allowed for.
///
/// The Smagorinsky model adds `ν_t = (C·Δ)²·|S|` to the viscosity, with `Δ` one cell. Writing
/// the strain in terms of the non-equilibrium stress and the total viscosity in terms of the
/// total relaxation time turns that into a quadratic whose positive root is
///
/// ```text
/// τ = ½·(τ₀ + sqrt(τ₀² + 18·C²·|Π|/ρ))
/// ```
///
/// (Hou, Sterling, Chen and Doolen, 1996.) It never falls below `τ₀`, and with no strain at all
/// it is exactly `τ₀` -- which is why leaving the model on does not disturb a flow the lattice
/// already resolves.
pub fn eddy_relaxation(tau: f64, constant: f64, density: f64, strain: f64) -> f64 {
    if !(constant > 0.0) || !(density > 0.0) || !(strain > 0.0) {
        return tau;
    }
    let added = 18.0 * constant * constant * strain / density;
    if !added.is_finite() {
        return tau;
    }
    let root = (tau * tau + added).sqrt();
    if !root.is_finite() { tau } else { 0.5 * (tau + root) }
}

/// Turning a real flow into lattice numbers and back again.
///
/// The lattice has its own units: a cell is one long, a step takes one, and the speed of sound
/// is `1/√3`. A study in metres and seconds has to be mapped onto that, and the mapping is not
/// free -- it is the whole of why a high Reynolds number is expensive. Given the spacing and
/// the speed, choosing how fast the flow runs *on the lattice* fixes the time step, and the
/// Reynolds number then fixes the relaxation time. Ask for too much and the relaxation time
/// falls to a half, where the scheme stops being stable; this refuses rather than running.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scaling {
    /// A cell's side, in metres.
    pub spacing_m: f64,
    /// A step's length, in seconds.
    pub step_s: f64,
    /// The fluid's density, kg/m³.
    pub density_kg_m3: f64,
}

impl Scaling {
    /// Work out the mapping for a flow at `speed_m_s` over a lattice of `spacing_m` cells,
    /// running at `lattice_speed` on the lattice.
    ///
    /// A smaller `lattice_speed` is slower, because the time step shrinks with it, and it is
    /// more accurate only down to a point: the compressibility error goes as its square, but
    /// the round-off in the force goes as its reciprocal, because the distributions are `f32`.
    /// Accuracy is a U between [`MIN_LATTICE_SPEED`] and [`MAX_LATTICE_SPEED`], and a twentieth
    /// is the usual starting point. This refuses a speed above the upper figure; below the
    /// lower one it returns a mapping, because the study still runs and the caller is better
    /// placed to say so, and `sim.flow` warns.
    ///
    /// `collision` is wanted because the relaxation time this can reach is the operator's
    /// property and not the method's ([`Collision::min_tau`]), and so therefore is the
    /// Reynolds number the study ends up running at.
    pub fn for_flow(fluid: &Fluid, spacing_m: f64, speed_m_s: f64, lattice_speed: f64, collision: Collision) -> Result<Mapping> {
        if !(spacing_m > 0.0) || !spacing_m.is_finite() {
            return setup("a lattice spacing that is not a length");
        }
        if !(speed_m_s > 0.0) || !speed_m_s.is_finite() {
            return setup("a flow speed that is not a speed; nothing moves");
        }
        if !(lattice_speed > 0.0 && lattice_speed <= MAX_LATTICE_SPEED) {
            return setup(format!(
                "a lattice speed of {lattice_speed}; it must be above zero and at most {MAX_LATTICE_SPEED}, well under the \
                 lattice's own speed of sound"
            ));
        }
        let nu = fluid.kinematic_viscosity_m2_s();
        if !(nu > 0.0) {
            return Err(SimError::Material(format!("{} has no viscosity, so it cannot be made to flow", fluid.name)));
        }
        // u_lattice = u_physical · Δt/Δx fixes the step from the speed.
        let step_s = lattice_speed * spacing_m / speed_m_s;
        // ν_lattice = ν · Δt/Δx², and τ = 3ν_lattice + 1/2.
        let nu_lattice = nu * step_s / (spacing_m * spacing_m);
        let tau = nu_lattice / CS2 + 0.5;
        if !(tau > 0.5) || !tau.is_finite() {
            return setup("this flow gives no usable relaxation time");
        }
        let cell_reynolds = speed_m_s * spacing_m / nu;
        let resolved_spacing_m = RESOLVED_CELL_REYNOLDS * nu / speed_m_s;
        Ok(Mapping {
            // The fluid is made thicker than it is where the lattice cannot hold it as given;
            // `Mapping::caveat` says so, in numbers. How thick is the chosen operator's
            // business, not the method's: see [`Collision::min_tau`].
            tau: tau.max(collision.min_tau()),
            physical_tau: tau,
            lattice_speed,
            scaling: Scaling { spacing_m, step_s, density_kg_m3: fluid.density_kg_m3 },
            cell_reynolds,
            resolved_spacing_m,
        })
    }

    /// A speed on the lattice, in metres a second.
    pub fn velocity_m_s(&self, lattice: Vec3) -> Vec3 {
        lattice * (self.spacing_m / self.step_s)
    }

    /// A speed in metres a second, on the lattice.
    pub fn to_lattice(&self, m_s: Vec3) -> Vec3 {
        m_s * (self.step_s / self.spacing_m)
    }

    /// A force on the lattice, in newtons. Force is `ρ·Δx⁴/Δt²`, the lattice's density being
    /// one.
    pub fn force_n(&self, lattice: Vec3) -> Vec3 {
        let k = self.density_kg_m3 * self.spacing_m.powi(4) / (self.step_s * self.step_s);
        lattice * k
    }

    /// A pressure on the lattice, in pascals.
    pub fn pressure_pa(&self, lattice: f64) -> f64 {
        lattice * self.density_kg_m3 * (self.spacing_m / self.step_s).powi(2)
    }

    /// How long a run of `steps` covers, in seconds.
    pub fn duration_s(&self, steps: u32) -> f64 {
        self.step_s * steps as f64
    }
}

/// A real flow mapped onto a lattice, and how well the lattice holds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mapping {
    pub scaling: Scaling,
    /// The relaxation time the flow is run at. Above [`Mapping::physical_tau`] when the fluid
    /// had to be made thicker than it is for the lattice to hold the flow at all.
    pub tau: f64,
    /// The relaxation time the fluid's own viscosity asks for.
    pub physical_tau: f64,
    /// How fast the flow runs on the lattice.
    pub lattice_speed: f64,
    /// `U*dx/nu`: how much inertia a single cell carries. See [`RESOLVED_CELL_REYNOLDS`].
    pub cell_reynolds: f64,
    /// The spacing at which this flow would be resolved. Often far finer than a model's own
    /// size makes practical, which is the honest state of affairs and not a fault of the
    /// mapping.
    pub resolved_spacing_m: f64,
}

impl Mapping {
    /// Whether the lattice resolves the flow, or is returning a coarser answer to it.
    pub fn resolved(&self) -> bool {
        self.cell_reynolds <= RESOLVED_CELL_REYNOLDS
    }

    /// Whether the fluid had to be made thicker than it is. See [`MIN_TAU`].
    pub fn thickened(&self) -> bool {
        self.tau > self.physical_tau + 1e-12
    }

    /// How much thicker than itself the fluid was made: the ratio of the viscosity run to the
    /// fluid's own.
    pub fn thickening(&self) -> f64 {
        let (run, own) = (self.tau - 0.5, self.physical_tau - 0.5);
        if own > 0.0 { (run / own).max(1.0) } else { f64::INFINITY }
    }

    /// How much finer the lattice would have to be to resolve the flow.
    pub fn refinement_wanted(&self) -> f64 {
        if self.resolved_spacing_m > 0.0 { (self.scaling.spacing_m / self.resolved_spacing_m).max(1.0) } else { 1.0 }
    }

    /// The Reynolds number the lattice actually runs, for a body `length_cells` across.
    ///
    /// `Re = u·L/ν` in lattice units, which is the same dimensionless number as in metres and
    /// seconds -- that is the point of it. It falls short of
    /// [`Mapping::requested_reynolds`] by exactly the factor the fluid was thickened by.
    pub fn effective_reynolds(&self, length_cells: f64) -> f64 {
        reynolds(self.lattice_speed, length_cells, CS2 * (self.tau - 0.5))
    }

    /// The Reynolds number the flow as given would have.
    pub fn requested_reynolds(&self, length_cells: f64) -> f64 {
        reynolds(self.lattice_speed, length_cells, CS2 * (self.physical_tau - 0.5))
    }

    /// What has to be said about this mapping, or `None` when it holds the flow as given.
    ///
    /// `length_cells` is how many cells the body spans across the flow, which is what a
    /// Reynolds number is measured against.
    pub fn caveat(&self, length_cells: f64) -> Option<String> {
        if !self.thickened() && self.resolved() {
            return None;
        }
        let mut said = Vec::new();
        if self.thickened() {
            said.push(format!(
                "this lattice cannot hold this flow as given, so the fluid was made {:.0} times thicker than it is and the \
                 flow ran at a Reynolds number of about {:.0} rather than {:.0}. For a bluff body that is more useful than \
                 it sounds, since a drag coefficient hardly moves over decades of Reynolds number, but for anything that \
                 turns on a boundary layer staying attached it is not. Cells of about {:.4} mm would hold it as given",
                self.thickening(),
                self.effective_reynolds(length_cells),
                self.requested_reynolds(length_cells),
                self.scaling.spacing_m / self.thickening() * 1000.0
            ));
        }
        if !self.resolved() {
            said.push(format!(
                "a cell carries a Reynolds number of {:.0}, and about {RESOLVED_CELL_REYNOLDS} is what a lattice resolves \
                 outright; cells of {:.4} mm, {:.0} times finer than these, would resolve it",
                self.cell_reynolds,
                self.resolved_spacing_m * 1000.0,
                self.refinement_wanted()
            ));
        }
        Some(said.join("; "))
    }
}

fn reynolds(speed: f64, length: f64, viscosity: f64) -> f64 {
    if viscosity > 0.0 { speed * length / viscosity } else { f64::INFINITY }
}

#[cfg(test)]
#[path = "flow_tests.rs"]
mod tests;
