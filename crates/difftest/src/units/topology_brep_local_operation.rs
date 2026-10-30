//! Differential coverage for `topology.brep_local_operation` (`SmBrep::LocalOperation`).
//!
//! The fixtures are upstream `.smb` solids read by the native `SmBrep::ReadFromFile` and
//! imported into Rust from the native raw graph, as `topology.brep_mirror`'s. A case
//! replaces the surfaces of one to three faces (`GetFaces` order) by planes derived from
//! the faces' own surfaces (the point at the middle of the face's UV domain moved along
//! the normal there, the normal tilted), by the real `SmBrep::LocalOperation` and by
//! `topology::brep::local_operation::local_operation`. A case compares the status, the
//! whole B-rep graph (topology, vertex points, tolerances, intervals, UV domains) and
//! samples of every edge curve, every edge use's UV trim curve and every face surface
//! (type code, natural domain, evaluated points). Inputs the native reader cannot build,
//! and replacements the shim cannot make, are `rejected`, not failed.
//!
//! Generated cases: `genNNNN` on every solid but `Solids/nurb_fillet.smb` and
//! `cone.smb`, and `fcNNNN` on those two, their own seed stream
//! (`<unit>:fillet_cone`, [`FILLET_CONE_CASES`]), so adding them moved no `gen` case.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::error::EntityRef;
use solidcore::geom::attribute::{Attribute, AttributeStore, Behavior, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::knots::Side;
use solidcore::offset::offset_executive::p2::OffsetNotifications;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::local_operation::local_operation;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::face::p2 as face_ops;
use solidcore::topology::topology::{NotifyRecords, Topology};
use solidcore::topology::{Brep, BrepId, EdgeId, RegionId};
use solidcore::Result as SolidResult;
use solidcore_ffi::units::topology_brep_local_operation::{self as ffi, Replacement};
use solidcore_ffi::FfiError;

use super::boolean_merge_p1::{graph_owned, OwnerSource};
use super::topology_brep_mirror::{geometry, herr, normalize_face_shells, status, FILES};
use super::topology_vertexuse::{import_raw_with_owners, path_of, BrepIn};
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};

/// The runner of `SmBrep::LocalOperation`.
pub struct Runner;

/// The solids of the `fcNNNN` cases (left out of the `gen` stream, which predates their
/// fixes).
const FILLET_CONE: [&str; 2] = ["Solids/nurb_fillet.smb", "cone.smb"];

/// Generated `fcNNNN` cases per run, whatever `--seeds` says (it can only raise this).
const FILLET_CONE_CASES: usize = 32;

/// One replaced face (see the module doc).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Rep {
    /// Index of the face in `GetFaces` order.
    face: u32,
    /// Distance of the new plane's origin along the face's surface normal.
    offset: f64,
    /// Added to the surface normal to make the plane normal.
    tilt: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Inputs {
    /// An upstream test file, relative to `upstream/TestFiles/pt_TestFiles`.
    file: String,
    /// Rebuild the B-rep's topology on reading (`ReadFromFile`'s flag).
    rebuild: bool,
    /// The faces to give new surfaces.
    replaced: Vec<Rep>,
    /// Every face and edge first tagged with its own attribute, and the attributes of
    /// the result's faces and edges recorded (shim op 3; issue #1462).
    attributed: bool,
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            file: "Solids/box.smb".into(),
            rebuild: false,
            replaced: vec![Rep {
                face: 0,
                offset: 0.5,
                tilt: [0.; 3],
            }],
            attributed: false,
        }
    }
}

/// The shared B-rep block of `ReadFromFile(file, rebuild)`; no validate-only, no gaps.
fn block(i: &Inputs) -> Vec<f64> {
    vec![0.0, f64::from(i.rebuild), 0.0, 0.0]
}

fn path(i: &Inputs) -> Result<String, HarnessError> {
    path_of(&BrepIn::File {
        file: i.file.clone(),
        rebuild: i.rebuild,
    })
}

/// The native output, or `Err(output)` when the fixture or the replacements are refused.
fn native(i: &Inputs, op: u8) -> Result<Result<Vec<f64>, Output>, HarnessError> {
    let replaced: Vec<Replacement> = i
        .replaced
        .iter()
        .map(|r| Replacement {
            face: r.face,
            offset: r.offset,
            tilt: r.tilt,
        })
        .collect();
    match ffi::eval(op, &path(i)?, &block(i), &replaced) {
        Ok(raw) if raw.first() == Some(&-999.) => Ok(Err(Output::rejected(format!(
            "fixture constructor status {}",
            raw.get(1).copied().unwrap_or(-1.)
        )))),
        Ok(raw) if raw.first() == Some(&-998.) => {
            Ok(Err(Output::rejected("replacement not buildable")))
        }
        Ok(raw) => Ok(Ok(raw)),
        Err(e @ FfiError::InvalidArgument(_)) => {
            Ok(Err(Output::rejected(format!("ffi guard: {e}"))))
        }
        Err(e) => Err(herr(e)),
    }
}

/// The replacement planes of `i` on `b`, as the shim makes them; `None` where it refuses.
fn planes(
    k: &Kernel,
    b: &Brep,
    i: &Inputs,
) -> SolidResult<Option<(Vec<solidcore::topology::FaceId>, Vec<Surface>)>> {
    let all = bp1::faces(b, None)?;
    let (mut faces, mut surfaces) = (Vec::new(), Vec::new());
    for r in &i.replaced {
        let Some(&face) = all.get(r.face as usize) else {
            return Ok(None);
        };
        let Some(surface) = face_ops::surface(b, face)? else {
            return Ok(None);
        };
        let uv = b.face(face)?.uv_domain.evaluate(0.5, 0.5);
        let (Ok(p), Ok(n)) = (
            surface.evaluate_point(k, uv),
            // (the shim's `EvaluateNormal(uv, TRUE, TRUE, ..)`: from the left)
            surface.evaluate_normal(k, uv, Side::Left, Side::Left),
        ) else {
            return Ok(None);
        };
        let origin = Point3::new(
            p.x + n.x * r.offset,
            p.y + n.y * r.offset,
            p.z + n.z * r.offset,
        );
        let tilted = Vec3::new(n.x + r.tilt[0], n.y + r.tilt[1], n.z + r.tilt[2]);
        let n2 = tilted.x * tilted.x + tilted.y * tilted.y + tilted.z * tilted.z;
        if n2.is_nan() || n2 <= 1e-12 {
            return Ok(None);
        }
        faces.push(face);
        surfaces.push(Surface::from(Plane::from_normal(origin, tilted)));
    }
    Ok(Some((faces, surfaces)))
}

fn notification_key(entity: EntityRef, namespace: u64) -> ObjectRef {
    let (index, type_code) = match entity {
        EntityRef::Brep(id) => (id.index(), solidcore::geom::a_object::BREP_TYPE),
        EntityRef::Region(id) => (id.index(), solidcore::topology::Region::type_code()),
        EntityRef::Shell(id) => (id.index(), solidcore::topology::Shell::type_code()),
        EntityRef::Face(id) => (id.index(), solidcore::topology::face::p2::TYPE),
        EntityRef::Faceuse(id) => (id.index(), 16022),
        EntityRef::Loopuse(id) => (id.index(), solidcore::topology::loopuse::TYPE),
        EntityRef::Edge(id) => (id.index(), solidcore::topology::edge::TYPE),
        EntityRef::Edgeuse(id) => (id.index(), solidcore::topology::edgeuse::p1::TYPE),
        EntityRef::Vertex(id) => (id.index(), solidcore::topology::vertex::TYPE),
        EntityRef::Vertexuse(id) => (id.index(), solidcore::topology::vertexuse::TYPE),
        EntityRef::Loop(id) => (id.index(), solidcore::topology::topology::TYPE),
    };
    ObjectRef {
        key: ObjectKey((namespace << 60) | ((type_code as u64) << 32) | u64::from(index)),
        type_code,
    }
}

/// Runs `f` with throwaway notification stores keyed by entity id, as callers with no
/// identities of their own do.
fn with_notifications<R>(
    b: &mut Brep,
    f: impl FnOnce(&mut Brep, &mut OffsetNotifications<'_>) -> R,
) -> R {
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let root = EntityRef::Brep(BrepId::from_index(0));
    let mut entities = vec![root];
    entities.extend(b.regions().iter().map(|(id, _)| EntityRef::Region(id)));
    entities.extend(b.shells().iter().map(|(id, _)| EntityRef::Shell(id)));
    entities.extend(b.faces().iter().map(|(id, _)| EntityRef::Face(id)));
    entities.extend(b.faceuses().iter().map(|(id, _)| EntityRef::Faceuse(id)));
    entities.extend(b.loopuses().iter().map(|(id, _)| EntityRef::Loopuse(id)));
    entities.extend(b.edges().iter().map(|(id, _)| EntityRef::Edge(id)));
    entities.extend(b.edgeuses().iter().map(|(id, _)| EntityRef::Edgeuse(id)));
    entities.extend(b.vertices().iter().map(|(id, _)| EntityRef::Vertex(id)));
    entities.extend(
        b.vertexuses()
            .iter()
            .map(|(id, _)| EntityRef::Vertexuse(id)),
    );
    for entity in entities {
        records.insert(notification_key(entity, 1).key, Topology::new());
    }
    let result_brep = notification_key(root, 3);
    records.insert(result_brep.key, Topology::new());
    let original = |e| notification_key(e, 1);
    let result = |e| notification_key(e, 3);
    let original_curve = |e: EdgeId, t: i64| ObjectRef {
        key: ObjectKey((5_u64 << 60) | ((t as u64) << 32) | u64::from(e.index())),
        type_code: t,
    };
    let result_curve = |e: EdgeId, t: i64| ObjectRef {
        key: ObjectKey((4_u64 << 60) | ((t as u64) << 32) | u64::from(e.index())),
        type_code: t,
    };
    let lists = |_: bool, region: RegionId| {
        [0_u64, 1].map(|list| ObjectRef {
            key: ObjectKey((6_u64 << 60) | (list << 32) | u64::from(region.index())),
            type_code: 0,
        })
    };
    let mut notifications = OffsetNotifications {
        records: &mut records,
        attributes: &mut attributes,
        original_key_of: &original,
        result_key_of: &result,
        original_curve_ref: &original_curve,
        result_curve_ref: &result_curve,
        boolean_source_lists: &lists,
        result_brep,
        merge_identities: None,
    };
    f(b, &mut notifications)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_local_operation"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let raw = match native(&i, if i.attributed { 3 } else { 1 })? {
            Ok(raw) => raw,
            Err(refused) => return Ok(refused),
        };
        let n = *raw.first().ok_or_else(|| herr("short record"))? as usize;
        let record = raw.get(1..=n).ok_or_else(|| herr("short record"))?.to_vec();
        let (mut b, owners) =
            import_raw_with_owners(raw.get(n + 1..).ok_or_else(|| herr("short graph"))?)?;
        strip_unrecorded(&mut b).map_err(herr)?;
        normalize_face_shells(&mut b).map_err(herr)?;
        graph_owned(&b, record, OwnerSource::Native(&owners))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let mut k = Kernel::default();
        // the Rust reader's B-rep of the same file, its geometry owners named by the
        // Rust reader (no C++ export or owner reaches the Rust side)
        let mut b = match crate::own_reader::read_file_rebuild(&mut k, &path(&i)?, i.rebuild)? {
            Ok(b) => b,
            Err(e) => {
                return Output::unported(&e).ok_or_else(|| herr(format!("rust read: {e}")));
            }
        };
        let mut run = || -> SolidResult<Option<Vec<f64>>> {
            let Some((faces, surfaces)) = planes(&k, &b, &i)? else {
                return Ok(None);
            };
            if i.attributed {
                tag(&mut b)?;
            }
            let st = status(with_notifications(&mut b, |b, notes| {
                local_operation(&mut k, b, &faces, surfaces, notes)
            }))?;
            let mut record = vec![st];
            record.extend(geometry(&k, &b, true)?);
            if i.attributed {
                put_tags(&mut record, &b)?;
            }
            Ok(Some(record))
        };
        match run() {
            Ok(None) => Ok(Output::new()),
            Ok(Some(record)) => {
                strip_unrecorded(&mut b).map_err(herr)?;
                normalize_face_shells(&mut b).map_err(herr)?;
                // (the serial whose topology keys name the edit's new geometry)
                let serial = solidcore::topology::brep::p3::notify_serial(&mut k.context, &mut b)
                    .map_err(herr)?;
                graph_owned(&b, record, OwnerSource::Rust { serial: Some(serial) })
            }
            Err(e) => Output::unported(&e).ok_or_else(|| herr(e)),
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        // `Solids/nurb_fillet.smb` and `cone.smb` have their own stream (`fcNNNN`)
        let solids: Vec<_> = FILES.iter().filter(|f| !FILLET_CONE.contains(*f)).collect();
        generate_on(rng, &solids)
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut main = ctx.clone();
        main.seeds = main.seeds.max(96);
        let mut v = crate::runner::default_cases(self, &main)?;
        // nurb_fillet and cone: their own seed stream (the `gen` cases are unchanged),
        // named `fcNNNN`
        let fc = CaseContext {
            seeds: ctx.seeds.max(FILLET_CONE_CASES),
            ..ctx.clone()
        };
        let solids: Vec<_> = FILLET_CONE.iter().collect();
        for mut c in fc.generated_cases("topology.brep_local_operation:fillet_cone", |rng| {
            generate_on(rng, &solids)
        }) {
            c.name = c.name.replacen("gen", "fc", 1);
            c.unit = self.id().to_string();
            v.push(c);
        }
        Ok(v)
    }
}

/// A generated case on one of `solids`: one to three faces replaced by planes offset
/// along and tilted from their surface normals.
fn generate_on(rng: &mut Rng, solids: &[&&str]) -> Option<Value> {
    let file = solids.get(rng.below(solids.len() as u64) as usize)?;
    let faces = match **file {
        // (cone.smb and Solids/nurb_fillet.smb: their files' "Faces In Brep" counts)
        "Solids/sphere.smb" | "plane_circ.smb" | "cone.smb" => 1,
        "disk.smb" => 2,
        "Solids/cylinder.smb" => 3,
        "Solids/nurb_fillet.smb" => 20,
        _ => 5,
    };
    let count = 1 + rng.below(3) as usize;
    let mut replaced = Vec::new();
    for _ in 0..count {
        // an index past the solid's faces is rejected
        let face = rng.below(faces) as u32;
        let offset = match rng.below(4) {
            0 => 0.0,
            _ => rng.uniform(-1.5, 1.5),
        };
        let tilt = if rng.below(3) == 0 {
            [
                rng.uniform(-0.3, 0.3),
                rng.uniform(-0.3, 0.3),
                rng.uniform(-0.3, 0.3),
            ]
        } else {
            [0.; 3]
        };
        replaced.push(Rep { face, offset, tilt });
    }
    serde_json::to_value(Inputs {
        file: (**file).into(),
        rebuild: false,
        replaced,
        attributed: false,
    })
    .ok()
}

/// The shim's op 3 tags: every face (`GetFaces` order) and edge (`GetEdges` order) its
/// own copy attribute `10007 + 500 kind + index` (kind 0 face, 1 edge).
fn tag(b: &mut Brep) -> SolidResult<()> {
    for (i, f) in bp1::faces(b, None)?.into_iter().enumerate() {
        let id = 10007 + i as u64;
        b.add_attribute(
            EntityRef::Face(f),
            Attribute::long(id, id as i64, Behavior::Copy),
        )?;
    }
    for (i, e) in bp1::edges(b, None)?.into_iter().enumerate() {
        let id = 10507 + i as u64;
        b.add_attribute(
            EntityRef::Edge(e),
            Attribute::long(id, id as i64, Behavior::Copy),
        )?;
    }
    Ok(())
}

/// The shim's op 3 tail: `nf (n id*)* ne (n id*)*`, the ascending attribute ids of every
/// face and edge in list order.
fn put_tags(o: &mut Vec<f64>, b: &Brep) -> SolidResult<()> {
    let ids = |o: &mut Vec<f64>, e: EntityRef| {
        let mut v: Vec<u64> = b
            .entity_attributes(e)
            .into_iter()
            .map(|(_, a)| a.attribute_id())
            .collect();
        v.sort_unstable();
        o.push(v.len() as f64);
        o.extend(v.into_iter().map(|x| x as f64));
    };
    let faces = bp1::faces(b, None)?;
    o.push(faces.len() as f64);
    for f in faces {
        ids(o, EntityRef::Face(f));
    }
    let edges = bp1::edges(b, None)?;
    o.push(edges.len() as f64);
    for e in edges {
        ids(o, EntityRef::Edge(e));
    }
    Ok(())
}

/// Drops what the comparison does not record: the faces' `loops` and the loops' `edges`
/// lists (Rust-only logical records the native export walk never has, so the import
/// leaves them empty; `local_operation` refreshes them before it returns, as
/// `topology.brep_mirror`'s `clear_logical_lists` clears both). The edge uses' UV trim
/// curves are compared through the geometry record (sampled on both sides as stored,
/// `m_pUVTrimCurve`; nothing is made).
fn strip_unrecorded(b: &mut Brep) -> SolidResult<()> {
    let faces: Vec<_> = b.faces().ids().collect();
    for id in faces {
        b.face_mut(id)?.loops.clear();
    }
    let loops: Vec<_> = b.loops().ids().collect();
    for id in loops {
        b.loop_mut(id)?.edges.clear();
    }
    Ok(())
}
