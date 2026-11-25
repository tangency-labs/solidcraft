//! Real `.smb` face topology with live native trim-cache operations.
//!
//! The runner reads the file with the Rust reader (`crate::own_builder::read_as`,
//! renumbered into upstream's traversal order) and builds/classifies. Planes stay
//! planes; faces are selected by surface type, loop count and occurrence. The existing synthetic lane and all of its cases are unchanged.

use serde::Deserialize;
use serde_json::Value;

use solidcore::error::EntityRef;
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::Kernel;
use solidcore::query::face::p2::BrepCacheFace;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::surface::surface_cache::face_context::PreparedFaceQueries;
use solidcore::surface::surface_cache::{SurfaceCache, SurfaceCacheOptions};
use solidcore::surface::trim_srf_cache::TrimSrfCache;
use solidcore::topology::face::p4 as face_p4;
use solidcore::topology::{Brep, FaceId};
use solidcore::{Result as SolidResult, SolidError};

use crate::{HarnessError, Output};

const SUCCESS: f64 = 1000.0;
const HEADER: usize = 22;
const PROBE: usize = 21;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)] // independent flags in the native probe protocol
struct Inputs {
    path: String,
    surface_type: u32,
    loops: u32,
    #[serde(default)]
    occurrence: u32,
    build: bool,
    /// `GetTree` after the build step: 0 none, 1 with the editing flag as is, 2 with
    /// editing disabled first.
    #[serde(default)]
    get_tree: u8,
    #[serde(default)]
    editing: bool,
    #[serde(default)]
    modified: bool,
    #[serde(default)]
    test_tol: bool,
    #[serde(default)]
    boundary_probes: bool,
    samples: Vec<[f64; 3]>,
}

fn harness(message: impl Into<String>) -> HarnessError {
    HarnessError::new(message)
}

fn flag(value: bool) -> f64 {
    f64::from(u8::from(value))
}

fn parse(value: &Value) -> Result<Inputs, HarnessError> {
    let input: Inputs = serde_json::from_value(value.clone())
        .map_err(|e| harness(format!("native_face input: {e}")))?;
    if input.get_tree > 2 {
        return Err(harness("native_face get_tree must be 0, 1 or 2"));
    }
    Ok(input)
}

/// Separates all discrete fields from floating values so identities/statuses/counts
/// compare exactly, not using the relative floating-point tolerance.
fn output(flat: &[f64]) -> Result<Output, HarnessError> {
    if flat.first().copied() != Some(SUCCESS) {
        return Err(harness(format!(
            "native_face fixture read failed: {flat:?}"
        )));
    }
    let head = flat
        .get(..HEADER)
        .ok_or_else(|| harness("native_face truncated header"))?;
    let build_status = head
        .get(9)
        .copied()
        .ok_or_else(|| harness("missing build status"))?;
    if build_status != -1.0 && build_status != SUCCESS {
        // A failed native operation is NOT a rejected fixture or a successful
        // status-only parity case. The fixture must exercise a successful build.
        return Err(harness(format!(
            "native_face BuildTree failed: status={build_status}; state={head:?}"
        )));
    }
    let count = head
        .get(21)
        .copied()
        .ok_or_else(|| harness("missing probe count"))?;
    if !count.is_finite() || count.fract() != 0.0 || !(0.0..=128.0).contains(&count) {
        return Err(harness("native_face bad probe count"));
    }
    if flat.len() != HEADER + count as usize * PROBE {
        return Err(harness("native_face probe count/layout mismatch"));
    }
    let mut discrete = Vec::new();
    let mut values = Vec::new();
    for (index, &v) in head.iter().enumerate() {
        if (5..9).contains(&index) || (16..20).contains(&index) {
            values.push(v);
        } else {
            discrete.push(integer(v)?);
        }
    }
    for probe in flat
        .get(HEADER..)
        .ok_or_else(|| harness("missing probes"))?
        .as_chunks::<PROBE>()
        .0
    {
        for (index, &v) in probe.iter().enumerate() {
            if (2..5).contains(&index) {
                values.push(v);
            } else {
                discrete.push(integer(v)?);
            }
        }
    }
    Ok(Output::new()
        .ints("native_face", discrete)
        .values("native_face", values))
}

fn integer(value: f64) -> Result<i64, HarnessError> {
    if !value.is_finite() || value.fract() != 0.0 || value.abs() > (1u64 << 53) as f64 {
        return Err(harness(format!(
            "native_face non-integral discrete result {value}"
        )));
    }
    Ok(value as i64)
}

fn identity(entity: EntityRef) -> (i64, u32) {
    match entity {
        EntityRef::Face(id) => (16002, id.index()),
        EntityRef::Loop(id) => (16003, id.index()),
        EntityRef::Loopuse(id) => (16004, id.index()),
        EntityRef::Edgeuse(id) => (16005, id.index()),
        EntityRef::Vertexuse(id) => (16006, id.index()),
        EntityRef::Brep(id) => (16011, id.index()),
        EntityRef::Region(id) => (16012, id.index()),
        EntityRef::Shell(id) => (16013, id.index()),
        EntityRef::Edge(id) => (16016, id.index()),
        EntityRef::Vertex(id) => (16021, id.index()),
        EntityRef::Faceuse(id) => (16022, id.index()),
    }
}

// The renumbered read (`Conventions::TOPOLOGY_FIXTURE`) owns each face's surface
// under key 1 + traversal-order face index.
// This mapping is explicit; kernel cache code never decodes an ObjectKey as an id.
fn key_of(entity: EntityRef) -> ObjectRef {
    let (type_code, index) = identity(entity);
    ObjectRef {
        type_code,
        key: ObjectKey(1 + u64::from(index)),
    }
}

fn object(out: &mut Vec<f64>, value: Option<EntityRef>) {
    match value {
        Some(value) => {
            let (kind, index) = identity(value);
            out.extend([kind as f64, f64::from(index)]);
        }
        None => out.extend([-1.0, -1.0]),
    }
}

/// An exact snapshot of the INPUT surface, retaining its real face owner/tolerance.
/// Planes are rebuilt from precisely these analytic members (without a NURB); do not invoke `Surface::clone`, which fixes B-spline knots again.
fn snapshot(surface: &Surface) -> SolidResult<Surface> {
    let mut copy = match surface.kind() {
        SurfaceKind::BSpline(s) => Surface::from(s.clone_exact()),
        SurfaceKind::Plane(s) => {
            let p = s.position();
            Surface::from(Plane::new(
                p.origin(),
                p.x_axis(),
                p.y_axis(),
                s.uv_scale(),
                &s.step_uv_domain(),
            ))
        }
        _ => {
            return Err(SolidError::invalid_input(
                "difftest::native_face",
                "inexact surface input",
            ))
        }
    };
    copy.set_owner(surface.owner());
    copy.set_owner_zone_tol_3d(surface.owner_zone_tol_3d());
    Ok(copy)
}

#[derive(Clone, Copy)]
struct Probe {
    mode: u8,
    edgeuse: i64,
    uv: Point2,
    tolerance: f64,
}

fn probes(k: &Kernel, brep: &Brep, face: FaceId, input: &Inputs) -> SolidResult<Vec<Probe>> {
    let domain = brep.face(face)?.uv_domain;
    let mut probes: Vec<_> = input
        .samples
        .iter()
        .map(|&[u, v, tolerance]| Probe {
            mode: 0,
            edgeuse: -1,
            uv: Point2::new(
                domain.min().x + u * (domain.max().x - domain.min().x),
                domain.min().y + v * (domain.max().y - domain.min().y),
            ),
            tolerance,
        })
        .collect();
    if input.boundary_probes {
        let loops = face_p4::upward_loopuses(brep, face, false, None)?;
        if loops.len() > 32 {
            return Err(SolidError::invalid_input(
                "difftest::native_face",
                "more than 32 boundary loops",
            ));
        }
        for lu in loops {
            let edges = solidcore::topology::loopuse::edgeuses(brep, lu, None, None)?;
            let eu = edges
                .first()
                .copied()
                .ok_or_else(|| SolidError::null_pointer("difftest::native_face::edgeuse"))?;
            let curve = solidcore::topology::edgeuse::p1::uv_trim_curve(brep, eu)?
                .0
                .ok_or_else(|| SolidError::null_pointer("difftest::native_face::uv_trim_curve"))?;
            let interval = curve.natural_interval();
            for mode in [1, 2] {
                let t = if mode == 1 {
                    (interval.min() + interval.max()) / 2.0
                } else {
                    interval.min()
                };
                let p = curve.evaluate_point(k, t)?;
                probes.push(Probe {
                    mode,
                    edgeuse: i64::from(eu.index()),
                    uv: Point2::new(p.x, p.y),
                    tolerance: 1e-5,
                });
            }
        }
    }
    Ok(probes)
}

fn status(error: SolidError) -> SolidResult<f64> {
    match error {
        e if crate::status::is_kernel(&e, solidcore::error::upstream_status(&e).is_some()) => {
            Ok(f64::from(crate::status::kind_slot(&e, solidcore::error::upstream_status(&e).unwrap_or(-1))))
        }
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => Ok(1007.0),
        other => Err(other),
    }
}

fn test(out: &mut Vec<f64>, result: SolidResult<(bool, Option<EntityRef>)>) -> SolidResult<()> {
    match result {
        Ok((ok, value)) => {
            out.extend([crate::status::ok_slot(SUCCESS), flag(ok)]);
            object(out, value);
        }
        Err(error) => {
            out.extend([status(error)?, 1.0]);
            object(out, None);
        }
    }
    Ok(())
}

fn classify(out: &mut Vec<f64>, result: SolidResult<(i32, Option<EntityRef>)>) -> SolidResult<()> {
    match result {
        Ok((class, value)) => {
            out.extend([crate::status::ok_slot(SUCCESS), f64::from(class)]);
            object(out, value);
        }
        Err(error) => {
            out.extend([status(error)?, 100.0]);
            object(out, None);
        }
    }
    Ok(())
}

fn state(out: &mut Vec<f64>, cache: &SurfaceCache<EntityRef>, editing: bool) {
    out.extend([
        flag(editing),
        flag(cache.face_was_modified),
        flag(cache.process_boundary_curves),
        flag(cache.point_test_enabled),
        flag(cache.have_t_surface_cache),
        super::containment_code(cache.face_containment_done),
    ]);
    super::push_dom(out, &cache.uv_domain());
    out.push(cache.tree().map_or(0, |tree| tree.nodes().len()) as f64);
}

fn rust_record(input: &Inputs, brep: &mut Brep, face: FaceId) -> SolidResult<Vec<f64>> {
    let k = Kernel::default();
    let record = brep.face(face)?;
    let source = record
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::null_pointer("difftest::native_face"))?;
    let mut out = vec![
        crate::status::ok_slot(SUCCESS),
        f64::from(face.index()),
        source.type_code() as f64,
        f64::from(input.loops),
        flag(source.owner() == Some(key_of(EntityRef::Face(face)))),
    ];
    super::push_dom(&mut out, &record.uv_domain);
    let surface = snapshot(source)?;
    let mut cache: TrimSrfCache<EntityRef> =
        TrimSrfCache::new(surface, SurfaceCacheOptions::default());
    let tolerance = record.tolerance;
    cache.base_mut().set_owner_zone_tol_3d(tolerance);
    brep.set_editing_enabled(input.editing);
    let handle_of = |entity| Ok(entity);
    cache.bind_face(
        key_of(EntityRef::Face(face)),
        EntityRef::Face(face),
        tolerance,
    )?;
    if input.build || input.get_tree != 0 {
        let mut built = if input.build {
            cache.build_tree_with_face(&k, &mut BrepCacheFace { b: brep, f: face })
        } else {
            Ok(())
        };
        if built.is_ok() && input.get_tree != 0 {
            if input.get_tree == 2 {
                brep.set_editing_enabled(false);
            }
            built = cache
                .get_tree_with_face(&k, &mut BrepCacheFace { b: brep, f: face })
                .map(|_| ());
        }
        let build_status = match built {
            Ok(()) => SUCCESS,
            Err(error) => status(error)?,
        };
        out.push(build_status);
        cache.base_mut().face_was_modified = input.modified;
        state(&mut out, cache.base(), brep.editing_enabled());
        if build_status != SUCCESS {
            out.push(0.0);
            return Ok(out);
        }
    } else {
        out.push(-1.0);
        cache.base_mut().face_was_modified = input.modified;
        state(&mut out, cache.base(), brep.editing_enabled());
    }
    let probes = probes(&k, brep, face, input)?;
    let queries = PreparedFaceQueries::new(brep, face, &handle_of)?;
    out.push(probes.len() as f64);
    for probe in &probes {
        out.extend([
            f64::from(probe.mode),
            probe.edgeuse as f64,
            probe.uv.x,
            probe.uv.y,
            probe.tolerance,
        ]);
        let tolerance = input.test_tol.then_some(probe.tolerance);
        test(
            &mut out,
            cache.point_test_with(&k, probe.uv, tolerance, Some(&queries)),
        )?;
        classify(
            &mut out,
            cache.point_classify_with(
                &k,
                probe.uv,
                probe.tolerance,
                EntityRef::Face(face),
                &queries,
            ),
        )?;
    }
    // Consume into the base, exactly the shape retained by numerical surface solvers.
    // Do not rebuild or reconstruct a plain SurfaceCache and lose trimmed dispatch.
    let retained = cache.into_base();
    let mut at = HEADER;
    for probe in probes {
        let mut record = Vec::new();
        let tolerance = input.test_tol.then_some(probe.tolerance);
        test(
            &mut record,
            retained.point_test_with(&k, probe.uv, tolerance, Some(&queries)),
        )?;
        classify(
            &mut record,
            retained.point_classify_with(&k, probe.uv, probe.tolerance, &queries),
        )?;
        // The direct loop emitted 13 fields per probe; append each base record in
        // place to obtain the same 21-field native layout without reclassification.
        at += 13;
        out.splice(at..at, record);
        at += 8;
    }
    Ok(out)
}

pub(super) fn run_rust(value: &Value) -> Result<Output, HarnessError> {
    let input = parse(value)?;
    // The input B-rep is the Rust reader's own read of the file (no C++ state),
    // renumbered into the shim's `sc_topology_fixture.h` traversal order, its faces'
    // surfaces owned under the `(SmFace_TYPE, 1 + index)` identities `key_of` uses.
    let mut k = Kernel::default();
    let built = crate::own_builder::read_as(
        &mut k,
        &input.path,
        false,
        false,
        false,
        &crate::own_builder::Conventions::TOPOLOGY_FIXTURE,
    )?;
    let mut brep = match built {
        crate::own_builder::Built::Brep(b) => *b,
        crate::own_builder::Built::Failed(e) => {
            return Output::unported(&e).map_or_else(
                || Err(harness(format!("native_face Rust read failed: {e}"))),
                Ok,
            )
        }
    };
    let mut occurrence = input.occurrence;
    let mut selected = None;
    for (face, record) in brep.faces().iter() {
        let Some(surface) = record.surface.as_ref() else {
            continue;
        };
        if surface.type_code() != i64::from(input.surface_type) {
            continue;
        }
        let loops = face_p4::upward_loopuses(&brep, face, false, None)
            .map_err(|e| harness(e.to_string()))?;
        if loops.len() != input.loops as usize {
            continue;
        }
        if occurrence == 0 {
            selected = Some(face);
            break;
        }
        occurrence -= 1;
    }
    let face = selected
        .ok_or_else(|| harness("native_face selector did not match a face"))?;
    match rust_record(&input, &mut brep, face) {
        Ok(flat) => output(&flat),
        Err(error) => Output::unported(&error).map_or_else(
            || {
                Err(harness(format!(
                    "native_face Rust operation failed: {error}"
                )))
            },
            Ok,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_status_only_and_failed_build_records() {
        assert!(output(&[SUCCESS]).is_err());
        let mut failed = vec![0.0; HEADER];
        failed[0] = SUCCESS;
        failed[9] = 1001.0;
        assert!(output(&failed).is_err());
    }

    #[test]
    fn discrete_identity_changes_are_not_float_tolerated() {
        let mut record = vec![0.0; HEADER + PROBE];
        record[0] = SUCCESS;
        record[9] = SUCCESS;
        record[21] = 1.0;
        let original = output(&record).unwrap();
        record[HEADER + 8] = 1.0;
        let changed = output(&record).unwrap();
        assert_ne!(original.ints, changed.ints);
        assert_eq!(original.values, changed.values);
    }

    /// The seven rows #20179 moved (the point test asks the face, not the cache build,
    /// whether to test): main's row digest (`MAIN`, the discrete entries' sum and
    /// weighted sum, from main's `expected/surface.trim_srf_cache.jsonl`) and this
    /// branch's (`NOW`). On each face (it has loop uses) the cache now reports point
    /// testing on, and every recorded point test is the face's own classification at
    /// the effective tolerance; with testing off (main, whose build stored it off on
    /// these editing-mode builds) each test keeps the point with no object. Rebuilding
    /// main's row that way from this branch's run gives main's digest exactly.
    #[test]
    fn moved_rows_are_the_face_classification_with_testing_on() {
        use solidcore::surface::surface_cache::face_context::FaceQueries;
        const MAIN: [(&str, [i64; 2]); 7] = [
            ("native_face_bspline_editing_does_not_implant_trims", [129988, 6374787]),
            ("native_face_bspline_get_tree_after_editing_implants_trims", [170740, 11001666]),
            ("native_face_bspline_get_tree_editing_rebuilds_standalone", [170392, 10997135]),
            ("native_face_holed_plane_editing_keeps_standalone_flags", [206685, 17613067]),
            ("native_face_holed_plane_get_tree_after_editing_implants_trims", [247085, 25574742]),
            ("native_face_holed_plane_get_tree_editing_rebuilds_standalone", [247085, 25574735]),
            ("native_face_holed_plane_get_tree_unbuilt_editing", [247085, 25574735]),
        ];
        const NOW: [[i64; 2]; 7] = [
            [226083, 11468789], [298845, 19715002], [298497, 19710471], [366938, 32263095],
            [439344, 46729948], [439344, 46729941], [439344, 46729941],
        ];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let fx: Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("fixtures/surface.trim_srf_cache/native_faces.json")).unwrap(),
        ).unwrap();
        let summ = |xs: &[i64]| {
            let (mut s, mut w) = (0i64, 0i64);
            for (j, &x) in xs.iter().enumerate() { s += x; w += (j as i64 + 1) * x; }
            [s, w]
        };
        let discrete = |flat: &[f64]| -> Vec<i64> {
            let mut d = Vec::new();
            for (i, &v) in flat[..HEADER].iter().enumerate() {
                if !((5..9).contains(&i) || (16..20).contains(&i)) { d.push(v as i64); }
            }
            for p in flat[HEADER..].chunks(PROBE) {
                for (i, &v) in p.iter().enumerate() { if !(2..5).contains(&i) { d.push(v as i64); } }
            }
            d
        };
        for (ci, (name, old_sum)) in MAIN.iter().enumerate() {
            let case = fx["cases"].as_array().unwrap().iter().find(|c| c["name"] == *name).unwrap();
            let raw = crate::fixtures::expand_inputs(&root, case["inputs"]["native_face"].clone()).unwrap();
            let input = parse(&raw).unwrap();
            let mut k = Kernel::default();
            let crate::own_builder::Built::Brep(b) = crate::own_builder::read_as(&mut k, &input.path, false, false, false, &crate::own_builder::Conventions::TOPOLOGY_FIXTURE).unwrap() else { panic!() };
            let mut brep = *b;
            let mut occ = input.occurrence; let mut sel = None;
            for (face, rec) in brep.faces().iter() {
                let Some(sf) = rec.surface.as_ref() else { continue };
                if sf.type_code() != i64::from(input.surface_type) { continue; }
                if face_p4::upward_loopuses(&brep, face, false, None).unwrap().len() != input.loops as usize { continue; }
                if occ == 0 { sel = Some(face); break; } occ -= 1;
            }
            let face = sel.unwrap();
            let flat = rust_record(&input, &mut brep, face).unwrap();
            let d_new = discrete(&flat);
            assert_eq!(summ(&d_new), NOW[ci], "{name}: the run is not this branch's row");
            // each test triple derived on its own: a point test on a face with loop uses
            // is the face classification at the effective tolerance
            assert!(face_p4::has_loopuses(&brep, face).unwrap());
            assert_eq!(flat[13], 1.0, "{name}: point testing is on");
            let handle_of = |e| Ok(e);
            let q = PreparedFaceQueries::new(&brep, face, &handle_of).unwrap();
            let mut old_flat = flat.clone();
            old_flat[13] = 0.0;
            let mut changed = 0usize;
            for (pi, p) in flat[HEADER..].chunks(PROBE).enumerate() {
                let uv = Point2::new(p[2], p[3]);
                let tol = if input.test_tol { p[4] } else { q.face_zone_tol_3d(EntityRef::Face(face)).unwrap() };
                let (class, obj) = q.face_point_classify(&k, EntityRef::Face(face), uv, tol).unwrap();
                let mut want = vec![SUCCESS, flag(matches!(class, 102..=104))];
                object(&mut want, obj);
                for off in [5usize, 13] {
                    assert_eq!(&p[off..off + 4], &want[..], "{name} probe {pi} off {off}");
                    if p[off + 1] == 0.0 { changed += 1; }
                    // main: testing off keeps the point, no object
                    let at = HEADER + pi * PROBE + off;
                    old_flat[at..at + 4].copy_from_slice(&[SUCCESS, 1.0, -1.0, -1.0]);
                }
            }
            assert_eq!(
                summ(&discrete(&old_flat)),
                *old_sum,
                "{name}: with testing off the row is not main's"
            );
            assert!(changed > 0, "{name}: no probe is culled now");
        }
    }
}
