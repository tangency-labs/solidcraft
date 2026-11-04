//! A kernel body and the operations every family shares: primitives, copy and transform, counts
//! and volume, and the result contract ([`finish`]).
//!
//! Operations take a body and change it, as the kernel does; an application that keeps its
//! input calls [`Body::copy`] first.

use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

use solidcore::kernel::Kernel;

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::certified::Verdict;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::primitive::api_primitives as primitives;
use solidcore::topology::brep::is_outward_solid;
use solidcore::topology::brep::mirror::transform_matrix;
use solidcore::topology::brep::p3::brep_copy;
use solidcore::{Brep, Vec3};

use crate::error::status;
use crate::session::{measure_body, read_body, run, with_body};
use crate::{Error, Result};

/// A kernel body, with its own lock: the kernel's operations take `&mut Brep`, even to measure.
#[derive(Debug)]
pub struct Body(Mutex<Brep>);

impl Body {
    /// A body the kernel made.
    #[must_use]
    pub(crate) fn new(b: Brep) -> Body {
        Body(Mutex::new(b))
    }

    /// The kernel's body, locked. A body an earlier panic left unfinished is still handed out;
    /// [`with_body`] refuses it first.
    pub(crate) fn lock(&self) -> MutexGuard<'_, Brep> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The kernel's body, locked, unless a panic left it unfinished or it is held now (never
    /// waited for): why not, if not.
    pub(crate) fn try_read(&self) -> std::result::Result<MutexGuard<'_, Brep>, &'static str> {
        self.0.try_lock().map_err(|e| match e {
            TryLockError::Poisoned(_) => "the body was left unfinished by a panic: not written",
            TryLockError::WouldBlock => "the body was held: not written",
        })
    }

    /// Whether a panic interrupted an edit of this body.
    #[must_use]
    pub(crate) fn is_poisoned(&self) -> bool {
        self.0.is_poisoned()
    }

    /// The kernel's body, taken out.
    #[must_use]
    #[allow(dead_code)] // the families' operations take their operands out
    pub(crate) fn into_inner(self) -> Brep {
        self.0.into_inner().unwrap_or_else(PoisonError::into_inner)
    }

    /// A deep copy (`SmApiBrepCopy`): the same topology in the same order, with the attributes
    /// its entities carry.
    ///
    /// # Errors
    /// Those of the kernel's copy; [`Error::Internal`] for a body a panic left unfinished.
    pub fn copy(&self) -> Result<Body> {
        read_body("SmApiBrepCopy", self, brep_copy).map(Body::new)
    }

    /// Moves this body by the affine `matrix` (`SmBrep::Transform(matrix, TRUE)`): rows 0-2 the
    /// images of x, y and z, row 3 the translation (`p' = p·A + t`), the fourth column
    /// `(0, 0, 0, 1)`. Reflections mirror it, a non-uniform scale or a shear turns its geometry
    /// into NURBS, tolerances scale with it.
    ///
    /// # Errors
    /// [`Error::Argument`], before the body is touched, for a matrix the kernel does not prove
    /// regular ([`solidcore::Transform::is_regular`] at [`REGULAR_RATIO`]: an entry not finite, a
    /// fourth column that is not `(0, 0, 0, 1)`, a smallest scale not proved above the ratio of
    /// the largest: a flattening, a zero scale; a uniform scale of any size is regular); the
    /// kernel's status, the body put back as it was.
    pub fn transform(&self, matrix: &[[f64; 4]; 4]) -> Result<()> {
        let map = solidcore::Transform { m: *matrix };
        if map.is_regular(REGULAR_RATIO) != Verdict::Yes {
            return Err(Error::Argument);
        }
        with_body("SmBrep::Transform", self, |k, b| {
            Ok(transform_matrix(k, b, matrix, true).map_err(status))
        })?
    }

    /// Face, edge and vertex counts.
    ///
    /// # Errors
    /// Those of [`with_body`].
    pub fn counts(&self) -> Result<[usize; 3]> {
        read_body("counts", self, |_, b| {
            Ok([
                b.face_list().len(),
                b.edge_list().len(),
                b.vertex_list().len(),
            ])
        })
    }

    /// The enclosed volume (`SmApiBrepComputeVolume`) at `relative_accuracy`, as the kernel's
    /// contract takes it (ops passes it on as given).
    ///
    /// # Errors
    /// The kernel's (an accuracy it refuses is its status).
    pub fn volume(&self, relative_accuracy: f64) -> Result<f64> {
        measure_body("brep_volume", self, |k, b| {
            let store = AttributeStore::default();
            let attrs = PropertyAttributes {
                store: &store,
                key_of: &|_| ObjectKey(0),
            };
            brep_volume(k, b, relative_accuracy, &attrs)
        })
    }
}

/// The conditioning a body's motion must be proved above, its smallest scale against its
/// largest (a choice of ops: SolidCraft's station maps take the same): a map flattening a body
/// more than this is refused as singular.
pub const REGULAR_RATIO: f64 = 1e-9;

/// What an operation's result must be ([`finish`]), as its input says: a solid input must give
/// a solid. Made from the input ([`Expect::of`]), never chosen by a caller; the two exceptions
/// are an operation's own contract, stated by the operation: [`Expect::FACES`] for one with no
/// solid input, [`Expect::SOLID`] for one that makes a solid from a sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Expect {
    solid: bool,
}

impl Expect {
    /// What a result of an operation on `input` must be.
    ///
    /// # Errors
    /// The kernel's, from the solid test.
    #[allow(dead_code)] // the families' operations derive it from their inputs
    pub(crate) fn of(k: &mut Kernel, input: &mut Brep) -> solidcore::Result<Expect> {
        Ok(Expect {
            solid: is_outward_solid(k, input)?,
        })
    }

    /// A result of an operation with no solid input (a primitive, a sheet's).
    #[allow(dead_code)]
    pub(crate) const FACES: Expect = Expect { solid: false };

    /// Whether a result must be a solid: closed, manifold, valid and facing out.
    #[must_use]
    pub(crate) const fn solid(self) -> bool {
        self.solid
    }

    /// A result that must be a solid whatever its input, for an operation that makes one
    /// from a sheet (a thickened sheet): the operation states it, as its own contract.
    #[allow(dead_code)] // the families' operations that make a solid from a sheet
    pub(crate) const SOLID: Expect = Expect { solid: true };
}

/// The result contract: a result has faces ([`Error::Empty`] if not), and a solid input gives a
/// solid ([`Error::NotSolid`] if not): closed, manifold, valid and oriented out.
///
/// # Errors
/// The kernel's error from the solid test.
#[allow(dead_code)] // the families' operations finish with it
pub(crate) fn finish(
    k: &mut Kernel,
    mut b: Brep,
    expect: Expect,
) -> solidcore::Result<Result<Brep>> {
    if b.face_list().is_empty() {
        return Ok(Err(Error::Empty));
    }
    if expect.solid && !is_outward_solid(k, &mut b)? {
        return Ok(Err(Error::NotSolid));
    }
    Ok(Ok(b))
}

fn point(p: [f64; 3]) -> Result<Vec3> {
    if !p.iter().all(|x| x.is_finite()) {
        return Err(Error::Argument);
    }
    Ok(Vec3::new(p[0], p[1], p[2]))
}

fn positive(x: f64) -> bool {
    x.is_finite() && x > 0.0
}

/// An axis-aligned box from its lower corner, every edge length positive (`SmApiCreateBox`).
///
/// # Errors
/// [`Error::Argument`] for a number that is not finite or a length that is not positive.
pub fn make_box(origin: [f64; 3], size: [f64; 3]) -> Result<Body> {
    let o = point(origin)?;
    if !size.iter().all(|&s| positive(s)) {
        return Err(Error::Argument);
    }
    let [x, y, z] = size;
    run("create_box", |k| primitives::create_box(k, o, x, y, z)).map(Body::new)
}

/// A cylinder about +Z from its base centre, radius and height positive (`SmApiCreateCylinder`).
///
/// # Errors
/// [`Error::Argument`] for a number out of range.
pub fn make_cylinder(origin: [f64; 3], radius: f64, height: f64) -> Result<Body> {
    let o = point(origin)?;
    if !(positive(radius) && positive(height)) {
        return Err(Error::Argument);
    }
    run("create_cylinder", |k| {
        primitives::create_cylinder(k, o, radius, height)
    })
    .map(Body::new)
}

/// A sphere, radius positive (`SmApiCreateSphere`).
///
/// # Errors
/// [`Error::Argument`] for a number out of range.
pub fn make_sphere(center: [f64; 3], radius: f64) -> Result<Body> {
    let o = point(center)?;
    if !positive(radius) {
        return Err(Error::Argument);
    }
    run("create_sphere", |k| primitives::create_sphere(k, o, radius)).map(Body::new)
}

/// A cone or frustum about +Z (`SmApiCreateCone`): base radius and height positive, top radius
/// at least 0 and not the base radius.
///
/// # Errors
/// [`Error::Argument`] for a number out of range.
pub fn make_cone(origin: [f64; 3], base_radius: f64, top_radius: f64, height: f64) -> Result<Body> {
    let o = point(origin)?;
    let ok = positive(base_radius)
        && positive(height)
        && top_radius.is_finite()
        && top_radius >= 0.0
        && top_radius != base_radius;
    if !ok {
        return Err(Error::Argument);
    }
    run("create_cone", |k| {
        primitives::create_cone(k, o, base_radius, top_radius, height)
    })
    .map(Body::new)
}

/// A ring torus about +Z, major radius > minor radius > 0 (`SmApiCreateTorus`).
///
/// # Errors
/// [`Error::Argument`] for a number out of range.
pub fn make_torus(center: [f64; 3], major_radius: f64, minor_radius: f64) -> Result<Body> {
    let o = point(center)?;
    if !(positive(minor_radius) && minor_radius < major_radius && major_radius.is_finite()) {
        return Err(Error::Argument);
    }
    run("create_torus", |k| {
        primitives::create_torus(k, o, major_radius, minor_radius)
    })
    .map(Body::new)
}
