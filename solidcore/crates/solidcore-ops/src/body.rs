//! A kernel body and the operations every family shares: primitives, copy and transform, counts
//! and volume, and the result contract ([`finish`]).
//!
//! Operations take a body and change it, as the kernel does; an application that keeps its
//! input calls [`Body::copy`] first.

use std::collections::BTreeSet;
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

use solidcore::kernel::Kernel;

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::certified::Verdict;
use solidcore::mass::integrator::properties::{brep_volume, no_unsure_region, PropertyAttributes};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::primitive::api_primitives as primitives;
use solidcore::query::api_queries::brep_is_closed_touching_solids;
use solidcore::topology::brep::mirror::transform_matrix;
use solidcore::topology::brep::p2::{create_missing_uv_trim_curves, is_manifold_solid};
use solidcore::topology::brep::p3::{brep_copy, brep_copy_mapped};
use solidcore::topology::brep::primitive_outward::{is_outward_primitive, Template};
use solidcore::topology::brep::sheet::turn_over_sheet;
use solidcore::topology::brep::{is_outward_solid, valid_and_outward_failure, SolidTest};
use solidcore::topology::face::p2 as face_p2;
use solidcore::topology::FaceId;
use solidcore::{Brep, EntityRef, Vec3};

/// The faces a record's open shells use backward, which a body built from its data turns over
/// ([`Body::turn_over_listed`]).
pub use solidcore_convert::brep::backward_sheet_faces;
use solidcore_convert::brep::{to_kernel, to_record, turn_to_record_side, Order, Unsupported};
use solidcore_record::brep::Brep as Record;

use crate::error::status;
use crate::session::{measure_body, read_body, read_held, run, with_body};
use crate::split::copies_of;
use crate::{Error, Result};

/// A kernel body, with its own lock: the kernel's operations take `&mut Brep`, even to measure.
/// Beside the B-rep it may hold the proof that the B-rep is valid and faces out ([`Outward`]),
/// made on that very B-rep and dropped by any mutable access to it ([`BodyGuard`]): a proof
/// cannot outlive a change of the body it proves (#14830).
#[derive(Debug)]
pub struct Body(Mutex<Held>);

/// What a [`Body`]'s lock holds: the B-rep, and its proof while nothing has changed it, or the
/// template a kernel constructor built it from while nothing has changed it (#14830: proved
/// from the template's closed form when an operation first takes it, [`Expect::of_held`]).
#[derive(Debug)]
pub(crate) struct Held {
    brep: Brep,
    outward: Option<Outward>,
    primitive: Option<Template>,
}

/// What a body taken out of its lock brings for its proof ([`Body::take_held`]): the proof it
/// held, the template it was built from, both or neither.
#[derive(Debug, Default)]
pub(crate) struct Proof {
    outward: Option<Outward>,
    primitive: Option<Template>,
}

/// The proof that a B-rep is valid and faces out ([`valid_and_outward_failure`] answering
/// `None` for it), with
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

/// The proof that `b` is valid and faces out, if it is ([`valid_and_outward_failure`], the part
/// of the solid tests after their topology test): the only maker of an [`Outward`].
///
/// # Errors
/// The kernel's, from the validity check and the volume's sign.
fn prove(k: &mut Kernel, b: &mut Brep) -> solidcore::Result<Option<Outward>> {
    Ok(prove_naming(k, b)?.ok())
}

/// As [`prove`], naming the test the body fails where it is not a valid outward solid: the
/// kernel's [`SolidTest`], so a caller can tell a body that is not a solid from one whose
/// volume sign the kernel could not decide ([`SolidTest::VolumeUndecided`]).
///
/// # Errors
/// The kernel's, from the validity check and the volume's sign.
fn prove_naming(
    k: &mut Kernel,
    b: &mut Brep,
) -> solidcore::Result<std::result::Result<Outward, SolidTest>> {
    give_missing_trims(k, b)?;
    match valid_and_outward_failure(k, b)? {
        None => Ok(Ok(Outward {
            settings: Box::new(Settings::of(k)),
        })),
        Some(test) => Ok(Err(test)),
    }
}

/// The step a body takes before its proof (#21105): every use on a face gets its UV trim
/// ([`create_missing_uv_trim_curves`]), once and in that call's order, as an edit of the body,
/// with the tolerance raises attaching a trim makes. The validity check builds a missing trim
/// only on a copy and leaves the body as it was, so without this step every proof of a body
/// that lacks trims (one made by [`Body::from_record`], which makes none, or an operation's
/// result) would build them again; after it the check runs on the body and builds none, nor
/// does a later proof.
///
/// A trim that cannot be built stays missing (the call stops at the first one), and its failure
/// is not this step's answer: the check builds the same trim on its copy and answers for it as
/// it did on the body before, by the rule it applies to that build (the face walk and the
/// pointer pass's face check go on past every failure but a cancellation or an unported step).
/// Those two stop the proof here too.
///
/// # Errors
/// The kernel's cancellation and unported statuses.
fn give_missing_trims(k: &Kernel, b: &mut Brep) -> solidcore::Result<()> {
    match create_missing_uv_trim_curves(k, b) {
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Ok(()) | Err(_) => Ok(()),
    }
}

/// A [`Body`] locked: the B-rep to read through `Deref`; through `DerefMut` it is handed out
/// only once the proof beside it is dropped, so no change of the B-rep keeps a proof made
/// before it.
#[derive(Debug)]
#[doc(hidden)] // public only for the bridge below ([`Body::kernel`])
pub struct BodyGuard<'a>(MutexGuard<'a, Held>);

impl std::ops::Deref for BodyGuard<'_> {
    type Target = Brep;
    fn deref(&self) -> &Brep {
        &self.0.brep
    }
}

impl std::ops::DerefMut for BodyGuard<'_> {
    fn deref_mut(&mut self) -> &mut Brep {
        self.0.outward = None;
        self.0.primitive = None;
        &mut self.0.brep
    }
}

/// A temporary bridge for SolidCraft's kernel wrapper (`crates/smlib`), which still calls the
/// kernel crate itself for the operation families not yet moved onto this crate: it makes a
/// body from a kernel B-rep and locks a body for its B-rep. Not part of the API: no other
/// client may use it, and it is deleted with the last family (tangency-labs/solidcore#3304).
#[doc(hidden)]
impl Body {
    /// A body of the kernel B-rep `b`, with no proof ([`Body::new`]).
    #[must_use]
    pub fn from_kernel(b: Brep) -> Body {
        Body::new(b)
    }

    /// The kernel's body, locked ([`Body::lock`]): a body an earlier panic left unfinished is
    /// still handed out, so the caller asks [`Body::kernel_left_unfinished`] first.
    pub fn kernel(&self) -> BodyGuard<'_> {
        self.lock()
    }

    /// Whether a panic interrupted an edit of this body ([`Body::is_poisoned`]).
    #[must_use]
    pub fn kernel_left_unfinished(&self) -> bool {
        self.is_poisoned()
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
        Body(Mutex::new(Held {
            brep,
            outward,
            primitive: None,
        }))
    }

    /// A body a kernel constructor has just built from `t`, unproved: an operation that first
    /// takes it proves it from `t`'s closed form ([`Expect::of_held`]), so a body only measured
    /// is never proved.
    #[must_use]
    pub(crate) fn primitive(brep: Brep, t: Template) -> Body {
        Body(Mutex::new(Held {
            brep,
            outward: None,
            primitive: Some(t),
        }))
    }

    /// The kernel's body, locked. A body an earlier panic left unfinished is still handed out;
    /// [`with_body`] refuses it first.
    pub(crate) fn lock(&self) -> BodyGuard<'_> {
        BodyGuard(self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Two different bodies locked at once, returned in the order given. They are locked in one
    /// order whichever is given first (by address, which stays put while both are borrowed), so
    /// two calls holding the same two bodies, on threads of their own, never wait on each other
    /// for good. The only way ops holds two bodies at once.
    pub(crate) fn lock_pair<'a>(a: &'a Body, b: &'a Body) -> (BodyGuard<'a>, BodyGuard<'a>) {
        if std::ptr::from_ref(a) < std::ptr::from_ref(b) {
            let ga = a.lock();
            (ga, b.lock())
        } else {
            let gb = b.lock();
            (a.lock(), gb)
        }
    }

    /// The kernel's body, locked, unless a panic left it unfinished or it is held now (never
    /// waited for): why not, if not.
    pub(crate) fn try_read(&self) -> std::result::Result<BodyGuard<'_>, &'static str> {
        self.0.try_lock().map(BodyGuard).map_err(|e| match e {
            TryLockError::Poisoned(_) => "the body was left unfinished by a panic: not written",
            TryLockError::WouldBlock => "the body was held: not written",
        })
    }

    /// The kernel's body taken out, leaving it empty, with its proof and its template: the one
    /// way either leaves a body (under one lock, before anything can change the B-rep).
    pub(crate) fn take_held(&self) -> (Brep, Proof) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let proof = Proof {
            outward: held.outward.take(),
            primitive: held.primitive.take(),
        };
        (std::mem::replace(&mut held.brep, Brep::new()), proof)
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
    /// its entities carry. The copy holds what the body holds beside its B-rep, read under the
    /// lock the copy is made under: the proof that it is valid and faces out, or the template
    /// it was built from. Both are statements about the B-rep's geometry and topology, which
    /// the copy has unchanged, so an operation that takes the copy does not prove it again.
    ///
    /// # Errors
    /// Those of the kernel's copy; [`Error::Internal`] for a body a panic left unfinished.
    pub fn copy(&self) -> Result<Body> {
        let (brep, outward, primitive) = read_held("SmApiBrepCopy", self, |k, held| {
            Ok(Some((
                brep_copy(k, held)?,
                held.0.outward.clone(),
                held.0.primitive,
            )))
        })?
        .ok_or(Error::Exception)?;
        Ok(Body(Mutex::new(Held {
            brep,
            outward,
            primitive,
        })))
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
                    split_edges: 0,
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

    /// Turns this sheet over ([`turn_over_sheet`], `SmFace::SwapUV` on each face): each face's
    /// side the other way (its surface's u and v swapped, its normal reversed), its edges as they
    /// were, the body valid. Turning a sheet over twice gives it back. `order` is this body's
    /// record order (its record's, from [`Body::from_record`] or [`Body::to_record`]).
    ///
    /// The turned sheet's record, its order and, for each record edge of `order`, the index of
    /// the same edge in the new record: the record numbers edges by its canonical walk, which a
    /// turned face's loops take the other way round (docs/native-brep.md, "Canonical order"), so
    /// picks and history go through this map. The kernel keeps every edge, so the map is a
    /// lookup.
    ///
    /// # Errors
    /// [`Error::Unsupported`] for a body with a closed shell (turned over, a solid would be inside
    /// out), the body as it was; [`Error::Argument`], before the body is touched, for an `order`
    /// that is not this body's (one naming an edge the body does not have); the kernel's status;
    /// those of [`Body::to_record`].
    pub fn turn_over(&self, order: &Order) -> Result<TurnedOver> {
        with_body("turn_over_sheet", self, |k, b| {
            // the kernel keeps every edge, so an order whose edges are all the body's maps
            // through the turned record below; any other is refused before the sheet turns
            let edges: BTreeSet<_> = b.edge_list().iter().collect();
            if !order.edges.iter().all(|e| edges.contains(e)) {
                return Ok(Err(Error::Argument));
            }
            Ok(turn_over_sheet(k, b).map_err(status))
        })??;
        let (record, new_order) = self.to_record()?;
        let now = new_order.record_edges();
        let edges = order
            .edges
            .iter()
            .map(|e| now.get(e).copied().ok_or(Error::Argument))
            .collect::<Result<Vec<u32>>>()?;
        Ok(TurnedOver {
            record,
            order: new_order,
            edges,
        })
    }

    /// Gives the faces at `positions` of this body's face list the side a record gives them by
    /// `forward: false` in an open shell: for a body just built from a record's data (an
    /// application's own import of a record), which lists its faces in the record's order, the
    /// faces [`backward_sheet_faces`] names. The kernel uses every face of an open shell forward
    /// and keeps a sheet face's side in its surface: each face is turned over (`SmFace::SwapUV`)
    /// with its loops started where the record walks them, so the body's record numbers its
    /// faces, edges and vertices as the record does (`solidcore_convert::brep::to_kernel` does
    /// the same for its own import). A position listed twice turns once.
    ///
    /// # Errors
    /// [`Error::Argument`], before the body is touched, for a position past the face list;
    /// [`Error::Unsupported`], before the body is touched, for a face of a closed shell (whose
    /// side its material gives); the kernel's status, the body put back as it was (the call's
    /// rollback).
    pub fn turn_over_listed(&self, positions: &[u32]) -> Result<()> {
        if positions.is_empty() {
            return Ok(());
        }
        with_body("turn_over", self, |k, b| {
            let mut picked = Vec::new();
            for &i in positions {
                let Some(&f) = b.face_list().get(i as usize) else {
                    return Ok(Err(Error::Argument));
                };
                if !picked.contains(&f) {
                    picked.push(f);
                }
            }
            Ok(turn_faces(k, b, &picked))
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

/// Each face of `faces` turned to the side its record gives it
/// ([`turn_to_record_side`]), after checking that every one is a sheet's (both its uses in one
/// shell), so a refusal leaves the body as it was.
fn turn_faces(k: &Kernel, b: &mut Brep, faces: &[FaceId]) -> Result<()> {
    for &f in faces {
        if !sheet_face(b, f).map_err(status)? {
            return Err(Error::Unsupported(
                "turning over a face of a closed shell: its material gives its side",
            ));
        }
    }
    for &f in faces {
        turn_to_record_side(k, b, f).map_err(status)?;
    }
    b.refresh_cached_topology();
    Ok(())
}

/// Whether both uses of face `f` lie in one shell (a sheet's face).
fn sheet_face(b: &Brep, f: FaceId) -> solidcore::Result<bool> {
    let face = b.face(f)?;
    let Some(top) = face.faceuse else {
        return Ok(false);
    };
    let fu = b.faceuse(top)?;
    let Some(mate) = fu.mate else {
        return Ok(false);
    };
    Ok(fu.shell == b.faceuse(mate)?.shell)
}

/// A sheet turned over ([`Body::turn_over`]): its record and order, and where each record edge
/// of the order it was turned with went.
#[derive(Clone, Debug, PartialEq)]
pub struct TurnedOver {
    /// The turned sheet's record.
    pub record: Record,
    /// Its record order.
    pub order: Order,
    /// For each record edge before the turn, its index in `record`.
    pub edges: Vec<u32>,
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

    /// Rebuild a construction in the same canonical order as a saved body
    /// before combining pieces whose caps or seams coincide.
    pub(crate) fn through_record(&self) -> Result<Body> {
        let (record, _) = self.to_record()?;
        Self::from_record(&record, true).map(|(body, _)| body)
    }

    /// This body as the record in canonical order ([`to_record`]), and the kernel entity each
    /// record face, edge and vertex is. The kernel gives every use on a face its UV trim first,
    /// which is why the body is edited.
    ///
    /// # Errors
    /// [`Error::Unsupported`] for what the record cannot carry (yet); [`Error::Record`] for
    /// kernel data that does not make a valid record; the kernel's error by its type.
    pub fn to_record(&self) -> Result<(Record, Order)> {
        self.with_record("brep::to_record", |_, record, order| Ok((record, order)))
    }

    /// `f` on this body's kernel B-rep with its record and [`Order`] ([`Body::to_record`]), in
    /// one call under the body's lock, so the order is the one of the B-rep `f` reads. The body
    /// is edited as [`Body::to_record`] edits it.
    ///
    /// # Errors
    /// Those of [`Body::to_record`]; `f`'s, by its type.
    pub(crate) fn with_record<T>(
        &self,
        call: &'static str,
        f: impl FnOnce(&Brep, Record, Order) -> solidcore::Result<T>,
    ) -> Result<T> {
        let mut out = None;
        let mut refusal = None;
        with_body(call, self, |k, b| {
            match to_record(k, b) {
                Ok((record, order)) => out = Some(f(b, record, order)?),
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

/// What an operation tests of a solid operand and requires of its result: every family's
/// (booleans, blends, sweeps, offsets, splits). An operation's plain call runs under
/// [`Contract::Closed`]; its `_with` call takes the contract.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Contract {
    /// An operand that is a closed manifold solid must give a closed result: a manifold solid
    /// (for a boolean, solids touching along edges or at vertices too). Topology tests only:
    /// no validity check runs and no volume is computed, on the operand or on the result, so
    /// a volume whose sign the kernel cannot decide never refuses a result, and the result
    /// holds no proof. An application that wants a result checked asks for it
    /// ([`Body::check_solid`]) when it chooses.
    #[default]
    Closed,
    /// An operand counts as a solid only when it is closed and proved valid and facing out
    /// (its held proof, its template, or a new proof), and a solid's result must be proved so
    /// too: [`Error::NotSolidBecause`] naming the test it fails where it is not. The result
    /// keeps its proof, so the next proved operation that takes it does not prove it again
    /// (#14830).
    Proved,
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
        // (the trims only for a body the solid test will check: past its topology test)
        let solid = is_manifold_solid(input)? && {
            give_missing_trims(k, input)?;
            is_outward_solid(k, input)?
        };
        Ok(Expect {
            solid,
            touching: false,
        })
    }

    /// As [`Expect::of`], for an operand taken out of its body with what it held
    /// ([`Body::take_held`]): the manifold test always, then the proof where it stands under
    /// `k`'s settings; else, for a body a constructor built and nothing changed since, the
    /// validity check and its template's closed form in place of the enclosure
    /// ([`is_outward_primitive`], #14830); else a new proof (`is_outward_solid`'s, step by
    /// step), which is also what a template whose certificate refuses gets. A proof or a
    /// template is only ever taken in place of the same test on the same B-rep: none, or a
    /// proof made under other settings, proves as [`Expect::of`] does.
    ///
    /// # Errors
    /// As [`Expect::of`]; those of [`is_outward_primitive`].
    pub(crate) fn of_held(
        k: &mut Kernel,
        input: &mut Brep,
        proof: Proof,
    ) -> solidcore::Result<Expect> {
        let solid = is_manifold_solid(input)?
            && (proof.outward.is_some_and(|o| o.holds(k))
                || match proof.primitive {
                    Some(t) => {
                        give_missing_trims(k, input)?;
                        is_outward_primitive(k, input, &t)?
                    }
                    None => false,
                }
                || prove(k, input)?.is_some());
        Ok(Expect {
            solid,
            touching: false,
        })
    }

    /// What a result of an operation on `input` must be under `contract`. [`Contract::Closed`]:
    /// a solid's when `input` is a closed manifold solid, by the topology test alone (the
    /// proof is not looked at). [`Contract::Proved`]: [`Expect::of_held`]'s.
    ///
    /// # Errors
    /// As [`Expect::of_held`].
    pub(crate) fn under(
        k: &mut Kernel,
        input: &mut Brep,
        proof: Proof,
        contract: Contract,
    ) -> solidcore::Result<Expect> {
        match contract {
            Contract::Closed => Ok(Expect {
                solid: is_manifold_solid(input)?,
                touching: false,
            }),
            Contract::Proved => Expect::of_held(k, input, proof),
        }
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
/// solid ([`Error::NotSolidBecause`] naming the test it fails if not): closed, manifold, valid
/// and oriented out.
///
/// # Errors
/// The kernel's error from the solid test.
pub(crate) fn finish(k: &mut Kernel, b: Brep, expect: Expect) -> solidcore::Result<Result<Brep>> {
    Ok(finish_proved(k, b, expect)?.map(|(b, _)| b))
}

/// As [`finish`], with the proof its solid test made on the result, if it made one (a solid
/// result: the topology test, then [`prove_naming`]), for the result's [`Body`] to keep
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
    let outward = if closed {
        match prove_naming(k, &mut b)? {
            Ok(outward) => outward,
            Err(test) => return Ok(Err(Error::NotSolidBecause(test))),
        }
    } else {
        // the topology test failed first, so it is the one named
        return Ok(Err(Error::NotSolidBecause(if expect.touching {
            SolidTest::NotClosedSolids
        } else {
            SolidTest::NotManifold
        })));
    };
    Ok(Ok((b, Some(outward))))
}

/// The result contract under `contract`. [`Contract::Proved`]: [`finish_proved`]'s.
/// [`Contract::Closed`]: a result has faces ([`Error::Empty`] if not), and a solid input gives
/// a closed result ([`Error::NotSolid`] if not: a manifold solid, or where `expect` allows
/// them solids touching along edges or at vertices), by the topology tests alone; no proof is
/// made, so the result brings none.
///
/// # Errors
/// The kernel's error from the solid test.
pub(crate) fn finish_under(
    k: &mut Kernel,
    b: Brep,
    expect: Expect,
    contract: Contract,
) -> solidcore::Result<Result<(Brep, Option<Outward>)>> {
    if contract == Contract::Proved {
        return finish_proved(k, b, expect);
    }
    if b.face_list().is_empty() {
        return Ok(Err(Error::Empty));
    }
    if expect.solid
        && !is_manifold_solid(&b)?
        && !(expect.touching && brep_is_closed_touching_solids(&b)?)
    {
        return Ok(Err(Error::NotSolid));
    }
    Ok(Ok((b, None)))
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
    run("create_box", |k| primitives::create_box(k, o, x, y, z))
        .map(|b| Body::primitive(b, Template::Block { origin: o, size }))
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
    .map(|b| {
        Body::primitive(
            b,
            Template::Cylinder {
                origin: o,
                radius,
                height,
            },
        )
    })
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
    run("create_sphere", |k| primitives::create_sphere(k, o, radius))
        .map(|b| Body::primitive(b, Template::Sphere { center: o, radius }))
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
    .map(|b| {
        Body::primitive(
            b,
            Template::Torus {
                center: o,
                major: major_radius,
                minor: minor_radius,
            },
        )
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use solidcore::topology::trimming_tools::face_uv_curves_present;

    /// A 10 mm cube united with a cylinder of radius 4 standing on its x = 10 face: the
    /// kernel's union, whose new faces have no UV trim curves yet.
    fn united(k: &mut Kernel) -> Brep {
        let bx = primitives::create_box(k, Vec3::ZERO, 10.0, 10.0, 10.0).unwrap();
        let cyl = primitives::create_cylinder(k, Vec3::new(10.0, 5.0, 0.0), 4.0, 10.0).unwrap();
        solidcore::boolean::api_brep::boolean_union(k, bx, cyl).unwrap()
    }

    /// The faces of `b` without their UV trims.
    fn lacking(b: &Brep) -> usize {
        b.face_list()
            .iter()
            .filter(|&&f| !face_uv_curves_present(b, f).unwrap())
            .count()
    }

    /// The trims `b` lacks, counted as the builds that give a copy of it its missing trims.
    fn missing_builds(k: &Kernel, b: &Brep) -> u64 {
        let mut copy = b.clone_standing_for_self().unwrap();
        let before = k.work_counters.uv_trim_builds();
        create_missing_uv_trim_curves(k, &mut copy).unwrap();
        k.work_counters.uv_trim_builds() - before
    }

    /// One proof of `b` ([`prove_naming`]), which must hold, with the UV trims it built and the
    /// certified trim bounds it computed.
    fn proved(k: &mut Kernel, b: &mut Brep) -> (u64, u64) {
        let w = |k: &Kernel| {
            (
                k.work_counters.uv_trim_builds(),
                k.work_counters.trim_deviation_bounds(),
            )
        };
        let before = w(k);
        assert_eq!(prove_naming(k, b).unwrap().err(), None);
        let after = w(k);
        (after.0 - before.0, after.1 - before.1)
    }

    /// #21105: the proof of a body that lacks UV trims gives it its trims once, before its
    /// validity check, so the check runs on the body: the first proof builds each missing trim
    /// once, and a repeat proof builds none and computes no trim bound. For a boolean's result,
    /// and for the same body read from its record with no use's pcurve
    /// (`to_kernel_with_bare_uses`: a record's pcurves become the body's trims, a bare use has
    /// none).
    #[test]
    fn a_proof_builds_each_missing_trim_once() {
        let mut k = Kernel::default();
        let result = united(&mut k);
        let (record, _) =
            to_record(&mut k, &mut result.clone_standing_for_self().unwrap()).unwrap();
        let mut bare = Vec::new();
        for (f, face) in record.faces.iter().enumerate() {
            for (l, lp) in face.loops.iter().enumerate() {
                if let solidcore_record::brep::Loop::Edges { edges } = lp {
                    for p in 0..edges.len() {
                        let at = |i: usize| u32::try_from(i).unwrap();
                        bare.push((at(f), at(l), at(p)));
                    }
                }
            }
        }
        let (read, _) =
            solidcore_convert::brep::to_kernel_with_bare_uses(&mut k, &record, true, &bare)
                .unwrap();
        for (name, mut b) in [("boolean", result), ("record", read)] {
            assert!(lacking(&b) > 0, "{name}: the body already has every trim");
            let needed = missing_builds(&k, &b);
            assert!(needed > 0, "{name}");
            let (builds, _) = proved(&mut k, &mut b);
            assert_eq!(builds, needed, "{name}: the first proof's trim builds");
            assert_eq!(lacking(&b), 0, "{name}: the proof left faces without trims");
            assert_eq!(proved(&mut k, &mut b), (0, 0), "{name}: the repeat proof");
        }
    }
}
