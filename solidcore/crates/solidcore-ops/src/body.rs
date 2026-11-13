//! A kernel body and the operations every family shares: primitives, copy and transform, counts
//! and volume, and the result contract ([`finish`]).
//!
//! Operations take a body and change it, as the kernel does; an application that keeps its
//! input calls [`Body::copy`] first.

use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

use solidcore::kernel::Kernel;

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::certified::Verdict;
use solidcore::mass::integrator::properties::{brep_volume, no_unsure_region, PropertyAttributes};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::primitive::api_primitives as primitives;
use solidcore::query::api_queries::brep_is_closed_touching_solids;
use solidcore::topology::brep::mirror::transform_matrix;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p3::{brep_copy, brep_copy_mapped};
use solidcore::topology::brep::{is_outward_solid, is_valid_and_outward};
use solidcore::topology::face::p2 as face_p2;
use solidcore::topology::FaceId;
use solidcore::{Brep, EntityRef, Vec3};

use solidcore_convert::brep::{to_kernel, to_record, Order, Unsupported};
use solidcore_record::brep::Brep as Record;

use crate::error::status;
use crate::session::{measure_body, read_body, run, with_body};
use crate::split::copies_of;
use crate::{Error, Result};

/// A kernel body, with its own lock: the kernel's operations take `&mut Brep`, even to measure.
/// Beside the B-rep it may hold the proof that the B-rep is valid and faces out ([`Outward`]),
/// made on that very B-rep and dropped by any mutable access to it ([`BodyGuard`]): a proof
/// cannot outlive a change of the body it proves (#14830).
#[derive(Debug)]
pub struct Body(Mutex<Held>);

/// What a [`Body`]'s lock holds: the B-rep, and its proof while nothing has changed it.
#[derive(Debug)]
pub(crate) struct Held {
    brep: Brep,
    outward: Option<Outward>,
}

/// The proof that a B-rep is valid and faces out ([`is_valid_and_outward`] held on it), with
/// the kernel settings it was made under. Made only by [`prove`], on the B-rep it is then kept
/// beside; it stands for that B-rep only while it is unchanged ([`BodyGuard`]) and the
/// kernel's settings are the same ([`Outward::holds`]).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Outward {
    settings: Box<Settings>,
}

/// The kernel settings a proof depends on: its tolerances and the upstream fixes (which change
/// the validity check and the enclosure), and the context's model size estimate, size ratio and
/// zone tolerance (which tolerance queries read). A session names its fixes per call
/// ([`crate::session::set_fixes`], [`crate::session::set_thread_fixes`]), so they can differ
/// between two operations; a proof made under other settings is not taken.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Settings {
    tolerances: solidcore::kernel::Tolerances,
    fixes: solidcore::kernel::UpstreamFixes,
    /// The context's stored model size estimate, size ratio and zone tolerance, by their bits.
    context: [u64; 3],
}

impl Settings {
    fn of(k: &Kernel) -> Settings {
        Settings {
            tolerances: k.tolerances,
            fixes: k.fixes,
            context: [
                k.context.stored_model_size_estimate().to_bits(),
                k.context.stored_large_small_size_ratio().to_bits(),
                k.context.this_zone_tol_3d().to_bits(),
            ],
        }
    }
}

impl Outward {
    /// Whether the proof stands under `k`'s settings: the ones it was made under.
    fn holds(&self, k: &Kernel) -> bool {
        *self.settings == Settings::of(k)
    }
}

/// The proof that `b` is valid and faces out, if it is ([`is_valid_and_outward`], the part of
/// the solid tests after their topology test): the only maker of an [`Outward`].
///
/// # Errors
/// The kernel's, from the validity check and the volume's sign.
fn prove(k: &mut Kernel, b: &mut Brep) -> solidcore::Result<Option<Outward>> {
    Ok(is_valid_and_outward(k, b)?.then(|| Outward {
        settings: Box::new(Settings::of(k)),
    }))
}

/// A [`Body`] locked: the B-rep to read through `Deref`; through `DerefMut` it is handed out
/// only once the proof beside it is dropped, so no change of the B-rep keeps a proof made
/// before it.
#[derive(Debug)]
pub(crate) struct BodyGuard<'a>(MutexGuard<'a, Held>);

impl std::ops::Deref for BodyGuard<'_> {
    type Target = Brep;
    fn deref(&self) -> &Brep {
        &self.0.brep
    }
}

impl std::ops::DerefMut for BodyGuard<'_> {
    fn deref_mut(&mut self) -> &mut Brep {
        self.0.outward = None;
        &mut self.0.brep
    }
}

impl Body {
    /// A body the kernel made.
    #[must_use]
    pub(crate) fn new(b: Brep) -> Body {
        Body::held(b, None)
    }

    /// A body with the proof [`finish_proved`] made on it, if any.
    #[must_use]
    pub(crate) fn held(brep: Brep, outward: Option<Outward>) -> Body {
        Body(Mutex::new(Held { brep, outward }))
    }

    /// The kernel's body, locked. A body an earlier panic left unfinished is still handed out;
    /// [`with_body`] refuses it first.
    pub(crate) fn lock(&self) -> BodyGuard<'_> {
        BodyGuard(self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The kernel's body, locked, unless a panic left it unfinished or it is held now (never
    /// waited for): why not, if not.
    pub(crate) fn try_read(&self) -> std::result::Result<BodyGuard<'_>, &'static str> {
        self.0.try_lock().map(BodyGuard).map_err(|e| match e {
            TryLockError::Poisoned(_) => "the body was left unfinished by a panic: not written",
            TryLockError::WouldBlock => "the body was held: not written",
        })
    }

    /// The kernel's body taken out, leaving it empty, with its proof: the one way a proof
    /// leaves a body (under one lock, before anything can change the B-rep).
    pub(crate) fn take_held(&self) -> (Brep, Option<Outward>) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let outward = held.outward.take();
        (std::mem::replace(&mut held.brep, Brep::new()), outward)
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
        self.0
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner)
            .brep
    }

    /// A deep copy (`SmApiBrepCopy`): the same topology in the same order, with the attributes
    /// its entities carry.
    ///
    /// # Errors
    /// Those of the kernel's copy; [`Error::Internal`] for a body a panic left unfinished.
    pub fn copy(&self) -> Result<Body> {
        read_body("SmApiBrepCopy", self, brep_copy).map(Body::new)
    }

    /// A deep copy with `order` mapped to the copy's own entity IDs. The ordering of each
    /// vector is preserved, including an importer's order that differs from the body's lists.
    /// No correspondence by numeric ID or list position is assumed.
    ///
    /// # Errors
    /// Those of [`Self::copy`]; [`Error::Record`] when an entity in `order` has no copy.
    pub fn copy_with_order(&self, order: &Order) -> Result<(Body, Order)> {
        let (copy, order) = read_body("SmApiBrepCopy", self, |k, b| {
            let (copy, map) = brep_copy_mapped(k, b)?;
            let Some(faces) = copies_of(&map, &order.faces, EntityRef::Face, |e| match e {
                EntityRef::Face(f) => Some(f),
                _ => None,
            }) else {
                return Ok(Err(Error::Record(
                    "copy record order names a face without a copy".to_owned(),
                )));
            };
            let Some(edges) = copies_of(&map, &order.edges, EntityRef::Edge, |e| match e {
                EntityRef::Edge(x) => Some(x),
                _ => None,
            }) else {
                return Ok(Err(Error::Record(
                    "copy record order names an edge without a copy".to_owned(),
                )));
            };
            let Some(vertices) = copies_of(&map, &order.vertices, EntityRef::Vertex, |e| match e {
                EntityRef::Vertex(v) => Some(v),
                _ => None,
            }) else {
                return Ok(Err(Error::Record(
                    "copy record order names a vertex without a copy".to_owned(),
                )));
            };
            Ok(Ok((
                copy,
                Order {
                    faces,
                    edges,
                    vertices,
                },
            )))
        })??;
        Ok((Body::new(copy), order))
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
    /// [`Error::UnsureRegion`] for a region the kernel is unsure is material (never read as
    /// void, a volume of 0); the kernel's (an accuracy it refuses is its status).
    pub fn volume(&self, relative_accuracy: f64) -> Result<f64> {
        measure_body("brep_volume", self, |k, b| {
            no_unsure_region(b)?;
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

/// A conversion's refusal as the operation's: the kernel's error by its type ([`status`]), the
/// record's by what it is.
fn refused(e: Unsupported) -> Error {
    match e {
        Unsupported::Kernel(e) => status(e),
        Unsupported::Invalid(w) => Error::Record(w.to_owned()),
        Unsupported::NotYet(w) => Error::Unsupported(w),
    }
}

impl Body {
    /// The record as a kernel body ([`to_kernel`]), and the kernel entity each record face,
    /// edge and vertex is: picks and history name entities in record order, and [`Order`] takes
    /// them to the kernel's. `trusted`: the record comes from a modeller that keeps the kernel's
    /// rules, so the kernel's healer is skipped.
    ///
    /// # Errors
    /// [`Error::Record`] for a record that fails its validation or the kernel's limits;
    /// [`Error::Unsupported`] for what the conversion does not carry yet; the kernel's error by
    /// its type.
    pub fn from_record(record: &Record, trusted: bool) -> Result<(Body, Order)> {
        let mut out = None;
        let mut refusal = None;
        let made = run("brep::to_kernel", |k| match to_kernel(k, record, trusted) {
            Ok((b, order)) => {
                out = Some(order);
                Ok(Some(b))
            }
            Err(Unsupported::Kernel(e)) => Err(e),
            Err(e) => {
                refusal = Some(refused(e));
                Ok(None)
            }
        });
        if let Some(e) = refusal {
            return Err(e);
        }
        match (made?, out) {
            (Some(b), Some(order)) => Ok((Body::new(b), order)),
            _ => Err(Error::Exception),
        }
    }

    /// This body as the record in canonical order ([`to_record`]), and the kernel entity each
    /// record face, edge and vertex is. The kernel gives every use on a face its UV trim first,
    /// which is why the body is edited.
    ///
    /// # Errors
    /// [`Error::Unsupported`] for what the record cannot carry (yet); [`Error::Record`] for
    /// kernel data that does not make a valid record; the kernel's error by its type.
    pub fn to_record(&self) -> Result<(Record, Order)> {
        let mut out = None;
        let mut refusal = None;
        with_body("brep::to_record", self, |k, b| {
            match to_record(k, b) {
                Ok(made) => out = Some(made),
                Err(Unsupported::Kernel(e)) => return Err(e),
                Err(e) => refusal = Some(refused(e)),
            }
            Ok(())
        })?;
        if let Some(e) = refusal {
            return Err(e);
        }
        out.ok_or(Error::Exception)
    }
}

/// What an operation's result must be ([`finish`]), as its input says: a solid input must give
/// a solid. Made from the input ([`Expect::of`]), never chosen by a caller; the two exceptions
/// are an operation's own contract, stated by the operation: [`Expect::FACES`] for one with no
/// solid input, [`Expect::SOLID`] for one that makes a solid from a sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Expect {
    solid: bool,
    /// A solid result may be solids touching along edges or at vertices (a boolean's).
    touching: bool,
}

impl Expect {
    /// What a result of an operation on `input` must be.
    ///
    /// # Errors
    /// The kernel's, from the solid test.
    pub(crate) fn of(k: &mut Kernel, input: &mut Brep) -> solidcore::Result<Expect> {
        Ok(Expect {
            solid: is_outward_solid(k, input)?,
            touching: false,
        })
    }

    /// As [`Expect::of`], for an operand taken out of its body with the proof it held
    /// ([`Body::take_held`]): the manifold test always, then that proof where it stands under
    /// `k`'s settings, else a new one (`is_outward_solid`'s, step by step). A proof is only
    /// ever taken in place of the same test passing on the same B-rep: none, or one made under
    /// other settings, proves as [`Expect::of`] does.
    ///
    /// # Errors
    /// As [`Expect::of`].
    pub(crate) fn of_held(
        k: &mut Kernel,
        input: &mut Brep,
        outward: Option<Outward>,
    ) -> solidcore::Result<Expect> {
        let solid = is_manifold_solid(input)?
            && (outward.is_some_and(|o| o.holds(k)) || prove(k, input)?.is_some());
        Ok(Expect {
            solid,
            touching: false,
        })
    }

    /// As [`Expect::of`], a solid's result allowed to be solids touching along edges or at
    /// vertices (closed, `brep_is_closed_touching_solids`), as a boolean's may be.
    pub(crate) fn touching(self) -> Expect {
        Expect {
            touching: true,
            ..self
        }
    }

    /// A result that must be a solid (an operation whose every input is one).
    pub(crate) const SOLID: Expect = Expect {
        solid: true,
        touching: false,
    };

    /// A result of an operation with no solid input (a primitive, a sheet's).
    pub(crate) const FACES: Expect = Expect {
        solid: false,
        touching: false,
    };

    /// Whether a result must be a solid: closed, manifold, valid and facing out.
    #[must_use]
    pub(crate) const fn solid(self) -> bool {
        self.solid
    }
}

/// The result contract: a result has faces ([`Error::Empty`] if not), and a solid input gives a
/// solid ([`Error::NotSolid`] if not): closed, manifold, valid and oriented out.
///
/// # Errors
/// The kernel's error from the solid test.
pub(crate) fn finish(k: &mut Kernel, b: Brep, expect: Expect) -> solidcore::Result<Result<Brep>> {
    Ok(finish_proved(k, b, expect)?.map(|(b, _)| b))
}

/// As [`finish`], with the proof its solid test made on the result, if it made one (a solid
/// result: the topology test, then [`prove`]), for the result's [`Body`] to keep
/// ([`Body::held`]).
///
/// # Errors
/// The kernel's error from the solid test.
pub(crate) fn finish_proved(
    k: &mut Kernel,
    mut b: Brep,
    expect: Expect,
) -> solidcore::Result<Result<(Brep, Option<Outward>)>> {
    if b.face_list().is_empty() {
        return Ok(Err(Error::Empty));
    }
    if !expect.solid {
        return Ok(Ok((b, None)));
    }
    // is_outward_touching_solids / is_outward_solid, step by step, keeping the proof
    let closed = if expect.touching {
        is_manifold_solid(&b)? || brep_is_closed_touching_solids(&b)?
    } else {
        is_manifold_solid(&b)?
    };
    let outward = if closed { prove(k, &mut b)? } else { None };
    if outward.is_none() {
        return Ok(Err(Error::NotSolid));
    }
    Ok(Ok((b, outward)))
}

/// Copies of each attribute `from` carries in `src`.
fn attribute_copies(src: &Brep, from: EntityRef) -> Vec<solidcore::geom::attribute::Attribute> {
    src.entity_attributes(from)
        .into_iter()
        .map(|(_, a)| a.make_copy())
        .collect()
}

/// `to` (in `into`) takes a copy of each attribute of `from` (in `src`): a rebuilt entity
/// stands for the one it replaces.
///
/// # Errors
/// The kernel's, from adding an attribute.
pub(crate) fn copy_attributes(
    src: &Brep,
    from: EntityRef,
    into: &mut Brep,
    to: EntityRef,
) -> solidcore::Result<()> {
    for a in attribute_copies(src, from) {
        into.add_attribute(to, a)?;
    }
    Ok(())
}

/// [`copy_attributes`] within one body.
///
/// # Errors
/// The kernel's, from adding an attribute.
pub(crate) fn copy_attributes_within(
    b: &mut Brep,
    from: EntityRef,
    to: EntityRef,
) -> solidcore::Result<()> {
    for a in attribute_copies(b, from) {
        b.add_attribute(to, a)?;
    }
    Ok(())
}

/// The orientation of `f`'s upward faceuse.
///
/// # Errors
/// The kernel's, for a face not in `b`.
pub(crate) fn upward(b: &Brep, f: FaceId) -> solidcore::Result<Option<OrientType>> {
    Ok(match face_p2::upward_faceuse(b, f)? {
        Some(fu) => Some(b.faceuse(fu)?.orientation),
        None => None,
    })
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
