//! Shell, thicken, sheet offset and stitch on solidcore. The offset executive reports its edits to
//! caller-held notification stores, which these calls build as throwaway stores: the source's
//! objects, the result's and the working copy's each in a namespace of their own.

use solidcore::curve::curve::Curve;
use solidcore::error::EntityRef;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::offset::offset_executive::p2::{self, OffsetNotifications, OffsetOptions, ShellCopyIdentities, ShellOptions};
use solidcore::query::api_queries::BrepRef;
use solidcore::sweep::topology_sweep::store::SweepIdentities;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{StitchAndOrientOptions, stitch_and_orient};
use solidcore::topology::topology::Topology;
use solidcore::topology::{Brep, BrepId, EdgeId, Region, RegionId, Shell};

use super::ops::{ERR_NO_ROOM, PickError};
use super::rs::{State, state, status};
use super::rs_blend::{check_args, pick_faces, working};
use super::rs_ops::{copy_of, result};
use super::{ERR_ARG, Handle};

const SOURCE: BrepId = BrepId::from_index(17);

fn notification_key(entity: EntityRef, namespace: u64) -> ObjectRef {
    let (index, type_code) = match entity {
        EntityRef::Brep(id) => (id.index(), solidcore::geom::a_object::BREP_TYPE),
        EntityRef::Region(id) => (id.index(), Region::type_code()),
        EntityRef::Shell(id) => (id.index(), Shell::type_code()),
        EntityRef::Face(id) => (id.index(), solidcore::topology::face::p2::TYPE),
        EntityRef::Faceuse(id) => (id.index(), 16022),
        EntityRef::Loopuse(id) => (id.index(), solidcore::topology::loopuse::TYPE),
        EntityRef::Edge(id) => (id.index(), solidcore::topology::edge::TYPE),
        EntityRef::Edgeuse(id) => (id.index(), solidcore::topology::edgeuse::p1::TYPE),
        EntityRef::Vertex(id) => (id.index(), solidcore::topology::vertex::TYPE),
        EntityRef::Vertexuse(id) => (id.index(), solidcore::topology::vertexuse::TYPE),
        EntityRef::Loop(id) => (id.index(), solidcore::topology::topology::TYPE),
    };
    ObjectRef { key: ObjectKey((namespace << 60) | ((type_code as u64) << 32) | u64::from(index)), type_code }
}

fn original_key(entity: EntityRef) -> ObjectRef {
    notification_key(entity, 1)
}

fn result_key(entity: EntityRef) -> ObjectRef {
    notification_key(entity, 3)
}

fn original_curve(edge: EdgeId, type_code: i64) -> ObjectRef {
    ObjectRef { key: ObjectKey((5_u64 << 60) | ((type_code as u64) << 32) | u64::from(edge.index())), type_code }
}

fn result_curve(edge: EdgeId, type_code: i64) -> ObjectRef {
    ObjectRef { key: ObjectKey((4_u64 << 60) | ((type_code as u64) << 32) | u64::from(edge.index())), type_code }
}

fn source_lists(result: bool, region: RegionId) -> [ObjectRef; 2] {
    let namespace = if result { 7_u64 } else { 6_u64 };
    [0_u64, 1].map(|list| ObjectRef { key: ObjectKey((namespace << 60) | (list << 32) | u64::from(region.index())), type_code: 0 })
}

/// The identities of the shell's private working copy: namespace 0, class 0 of [`MergeBodies`]'s keys.
fn copy_key(entity: EntityRef) -> ObjectRef {
    let local = notification_key(entity, 0);
    ObjectRef {
        key: MergeBodies::key(0, BrepId::from_index(0), local.type_code as u64, (local.key.0 & 0xffff_ffff) as u32),
        type_code: local.type_code,
    }
}

fn copy_curve(edge: EdgeId, type_code: i64) -> ObjectRef {
    ObjectRef { key: MergeBodies::key(0, BrepId::from_index(1), type_code as u64, edge.index()), type_code }
}

fn copy_lists(region: RegionId) -> [ObjectRef; 2] {
    [0_u64, 1].map(|list| ObjectRef { key: MergeBodies::key(0, BrepId::from_index(2), list, region.index()), type_code: 0 })
}

/// Identities of the B-reps a merging `PiecewiseMerge` makes: namespace 0, classes 1 to 3.
struct MergeBodies;

impl MergeBodies {
    fn key(class: u64, brep: BrepId, kind: u64, index: u32) -> ObjectKey {
        ObjectKey((class << 58) | ((u64::from(brep.index()) & 0xfff) << 46) | ((kind & 0x3fff) << 32) | u64::from(index))
    }
}

impl SweepIdentities for MergeBodies {
    fn topology(&self, brep: BrepId, entity: EntityRef) -> ObjectRef {
        let local = notification_key(entity, 0);
        ObjectRef { key: Self::key(1, brep, local.type_code as u64, (local.key.0 & 0xffff_ffff) as u32), type_code: local.type_code }
    }
    fn owned_auxiliary_objects(&self, brep: BrepId, model: &Brep) -> Vec<ObjectKey> {
        model.edges().iter().filter_map(|(edge, e)| Some(self.curve(brep, edge, e.curve.as_ref()?).key)).collect()
    }
    fn curve(&self, brep: BrepId, edge: EdgeId, curve: &Curve) -> ObjectRef {
        let type_code = curve.type_code();
        ObjectRef { key: Self::key(2, brep, type_code as u64, edge.index()), type_code }
    }
    fn new_curve(&self, brep: BrepId, edge: EdgeId, type_code: i64) -> solidcore::Result<ObjectRef> {
        Ok(ObjectRef { key: Self::key(2, brep, type_code as u64, edge.index()), type_code })
    }
    fn source_lists(&self, brep: BrepId, region: RegionId) -> solidcore::Result<[ObjectRef; 2]> {
        Ok([0_u64, 1].map(|list| ObjectRef { key: Self::key(3, brep, list, region.index()), type_code: 0 }))
    }
}

/// Records for every object of `brep` under the source's keys, and the key of the B-rep itself.
fn register(records: &mut solidcore::topology::topology::NotifyRecords, brep: &mut Brep) {
    let mut add = |entity| {
        records.insert(original_key(entity).key, Topology::new());
    };
    let root = EntityRef::Brep(BrepId::from_index(0));
    brep.set_key(original_key(root).key);
    add(root);
    brep.regions().iter().for_each(|(id, _)| add(EntityRef::Region(id)));
    brep.shells().iter().for_each(|(id, _)| add(EntityRef::Shell(id)));
    brep.faces().iter().for_each(|(id, _)| add(EntityRef::Face(id)));
    brep.faceuses().iter().for_each(|(id, _)| add(EntityRef::Faceuse(id)));
    brep.loopuses().iter().for_each(|(id, _)| add(EntityRef::Loopuse(id)));
    brep.edges().iter().for_each(|(id, _)| add(EntityRef::Edge(id)));
    brep.edgeuses().iter().for_each(|(id, _)| add(EntityRef::Edgeuse(id)));
    brep.vertices().iter().for_each(|(id, _)| add(EntityRef::Vertex(id)));
    brep.vertexuses().iter().for_each(|(id, _)| add(EntityRef::Vertexuse(id)));
}

/// `f` with notification stores that know `source`'s objects.
pub(super) fn notified<R>(source: &mut Brep, f: impl FnOnce(&mut Brep, &mut OffsetNotifications<'_>) -> R) -> R {
    let mut records = solidcore::topology::topology::NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    register(&mut records, source);
    let result_brep = result_key(EntityRef::Brep(BrepId::from_index(0)));
    records.insert(result_brep.key, Topology::new());
    let mut notifications = OffsetNotifications {
        records: &mut records,
        attributes: &mut attributes,
        original_key_of: &original_key,
        result_key_of: &result_key,
        original_curve_ref: &original_curve,
        result_curve_ref: &result_curve,
        boolean_source_lists: &source_lists,
        result_brep,
        merge_identities: Some(&MergeBodies),
    };
    f(source, &mut notifications)
}

fn offset_options(distance: f64) -> OffsetOptions {
    OffsetOptions { distance, extend_convex_edges: true, merge_results: true }
}

/// `SmApiShellBrepFull` of `b` (a working copy) by `options`, open at `faces`.
fn shell_of(s: &mut State, b: &mut Brep, faces: &[solidcore::topology::FaceId], options: ShellOptions) -> Result<Brep, i32> {
    let keys: Vec<_> = faces.iter().map(|&f| Some((SOURCE, f))).collect();
    let copy = ShellCopyIdentities { key_of: &copy_key, curve_ref: &copy_curve, boolean_source_lists: &copy_lists };
    let kernel = &mut s.kernel;
    notified(b, |b, notes| p2::api_shell_brep_full(kernel, Some(BrepRef { brep: &*b, handle: SOURCE }), options, &keys, &copy, notes))
        .map_err(|e| status(&e))
}

fn min_extent(s: &State, b: &mut Brep) -> Result<f64, i32> {
    use solidcore::query::brep::{BoundingBoxOptions, calculate_bounding_box};
    let e = calculate_bounding_box(&s.kernel, b, BoundingBoxOptions::default()).map_err(|e| status(&e))?;
    Ok((0..3).map(|i| e.max_at(i) - e.min_at(i)).fold(f64::INFINITY, f64::min))
}

/// A hollowed `b`, open at the faces under `points`.
pub(super) fn shell(s: &mut State, mut b: Brep, domain: u64, points: &[[f64; 3]], tolerance: f64, thickness: f64) -> Result<Handle, PickError> {
    let faces = pick_faces(&s.kernel, &mut b, domain, points, tolerance)?;
    // ponytail: a wall at least half the smallest extent leaves no room; the exact test is the
    // cavity the offset leaves, which only the offset knows.
    if thickness < 0.0 && 2.0 * -thickness >= min_extent(s, &mut b).map_err(|c| (c, None))? {
        return Err((ERR_NO_ROOM, None));
    }
    let options = ShellOptions { offset: offset_options(thickness), create_offset_solid: false, merge_sheet_side_walls: false };
    let r = shell_of(s, &mut b, &faces, options).map_err(|c| (c, None))?;
    result(s, r).map_err(|c| (c, None))
}

pub fn thicken(h: &Handle, thickness: f64, symmetric: bool) -> Result<Handle, i32> {
    check_args(&[], 1.0, &[thickness]).map_err(|e| e.0)?;
    if thickness == 0.0 {
        return Err(ERR_ARG);
    }
    let mut s = state();
    let (mut b, _) = working(&mut s, h).map_err(|e| e.0)?;
    if is_manifold_solid(&b).map_err(|e| status(&e))? {
        return Err(ERR_ARG);
    }
    if symmetric {
        // Half each way: the sheet moved back by half, then thickened the whole way.
        b = offset(&mut s, &mut b, -thickness / 2.0)?;
    }
    let options = ShellOptions { offset: offset_options(thickness), create_offset_solid: true, merge_sheet_side_walls: true };
    let r = shell_of(&mut s, &mut b, &[], options)?;
    result(&mut s, r)
}

/// `SmApiOffsetBrepFull`: every face of `b` moved by `distance` along its normal.
fn offset(s: &mut State, b: &mut Brep, distance: f64) -> Result<Brep, i32> {
    let kernel = &mut s.kernel;
    notified(b, |b, notes| p2::api_offset_brep_full(kernel, Some(b), offset_options(distance), notes)).map_err(|e| status(&e))
}

pub fn offset_sheet(h: &Handle, distance: f64) -> Result<Handle, i32> {
    check_args(&[], 1.0, &[distance]).map_err(|e| e.0)?;
    let mut s = state();
    let (mut b, _) = working(&mut s, h).map_err(|e| e.0)?;
    if is_manifold_solid(&b).map_err(|e| status(&e))? {
        return Err(ERR_ARG);
    }
    let r = offset(&mut s, &mut b, distance)?;
    result(&mut s, r)
}

pub fn stitch(h: &Handle, tolerance: f64) -> Result<(Handle, bool), i32> {
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(ERR_ARG);
    }
    let mut s = state();
    let (mut b, _) = copy_of(&mut s, h)?;
    let options = StitchAndOrientOptions { stitch_tolerance: tolerance, ..StitchAndOrientOptions::default() };
    stitch_and_orient(&mut s.kernel, &mut b, &options).map_err(|e| status(&e))?;
    let closed = is_manifold_solid(&b).map_err(|e| status(&e))?;
    Ok((result(&mut s, b)?, closed))
}
