//! Point cloud deviation (#14356): how far each point of a scanned point cloud lies from a body,
//! measured on the body's exact faces as they are trimmed. The command reads the design and
//! changes nothing in it: no placement, no colour, no undo step.
//!
//! Each distance is the kernel's closest-point solve on the exact surfaces, edges and vertices
//! ([`solidcraft_kernel::point_closest`]), never a tessellation, so the result does not depend on
//! how finely the body is drawn. A sign comes from the kernel's exact point classification
//! ([`solidcraft_kernel::Body::classify_points`]), and only for a body that encloses a volume.

use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_kernel::{Members, PointClass};

use super::CommandSpec;
use crate::params::bad;
use crate::schema::point_cloud_deviation::{DeviationFailure, DeviationPoint, MAX_FACES, PointCloudDeviationParams, PointCloudDeviationResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CONTRACT: TypedContract<PointCloudDeviationParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] =
    &[CommandSpec::new("pointcloud.deviation", "Point Cloud Deviation", crate::checked!("pointcloud.deviation", deviation))
        .icon("compare")
        .noundo()
        .read_only()
        .contract(&CONTRACT)
        .output(output_schema_for::<PointCloudDeviationResult>)
        .params(
            "cloud: id or name; body: body name (not a mesh body); faces?: [face index] (default every face; a face set is always unsigned); \
             tolerance?: mm (0.0001 to 1000, default 0.1); max_samples? (1 to 2000000, default 100000: a larger cloud is sampled at every n-th point); \
             signed?: bool (positive outside the material, negative inside, zero on the boundary; default true for a body that encloses a volume measured \
             on all its faces, refused for any other); worst? (0 to 1000, default 10); per_point?: bool → cloud, body, faces, signed, sign_convention, \
             tolerance_mm, cloud_points, sample_step, sampled, measured, failed, mean_abs_mm, mean_signed_mm, rms_mm, max_abs_mm, within_tolerance, \
             within_tolerance_fraction, outside, inside, on_boundary, worst: [{index, point, closest, distance_mm}], failures: [{index, reason}], \
             distances (with per_point), body_tolerance_mm, point_precision_mm, warnings. Distances are measured on the exact faces, edges and vertices \
             of the body as placed now, from the cloud as placed now; nothing changes. Statistics count each measured point once and are null when no \
             point was measured. The maximum is a sampled maximum, and points far from a part of the body say nothing about that part",
        )];

/// The default tolerance (mm).
const TOLERANCE: f64 = 0.1;
/// The default and largest number of points measured.
const MAX_SAMPLES: usize = 100_000;
const MAX_SAMPLES_LIMIT: usize = 2_000_000;
/// The default number of farthest points listed.
const WORST: usize = 10;
/// The most failures listed.
const MAX_FAILURES: usize = 100;
/// The most threads the closest-point solves run on.
const THREADS: usize = 16;

/// What the sign of a signed distance means.
const SIGN_CONVENTION: &str = "positive outside the body's material, negative inside it, zero on its boundary (within the body's tolerance)";

/// A number parameter in `range`, `default` when absent.
fn number(cmd: &str, p: &Value, key: &str, default: f64, range: std::ops::RangeInclusive<f64>) -> Result<f64> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite() && range.contains(x))
            .ok_or_else(|| bad(cmd, format!("`{key}` must be a number from {} to {}", range.start(), range.end()))),
    }
}

/// A boolean parameter, `None` when absent.
fn flag(cmd: &str, p: &Value, key: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// The cloud's index in the document, by `cloud`: its id or its name. A number is an id first
/// and a name only when no cloud has that id; a string is a name first and an id only when no
/// cloud has that name. A cloud named "2" then never hides the cloud with id 2 from `cloud: 2`.
fn find_cloud(s: &Session, p: &Value, cmd: &str) -> Result<usize> {
    let (key, id_first) = match p.get("cloud") {
        Some(Value::Number(n)) => (n.to_string(), true),
        Some(Value::String(x)) => (x.clone(), false),
        _ => return Err(bad(cmd, "`cloud` must be an id or name")),
    };
    let clouds = &s.doc.point_clouds;
    let by_id = || clouds.iter().position(|c| c.id.to_string() == key);
    let by_name = || clouds.iter().position(|c| c.name == key);
    let found = if id_first { by_id().or_else(by_name) } else { by_name().or_else(by_id) };
    found.ok_or_else(|| bad(cmd, format!("no point cloud `{key}`")))
}

/// `faces`: the face indices, each below `count`, without repeats, in the order given.
fn face_list(cmd: &str, p: &Value, count: usize) -> Result<Option<Vec<usize>>> {
    let list = match p.get("faces") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Array(a)) if a.len() <= MAX_FACES => a,
        Some(_) => return Err(bad(cmd, format!("`faces` must be a list of face indices (whole numbers from 0), at most {MAX_FACES}"))),
    };
    let mut out: Vec<usize> = Vec::with_capacity(list.len());
    // Which faces are in `out` already: one flag per face of the body, so a repeat is found
    // without searching `out`.
    let mut taken = vec![false; count];
    for v in list {
        let i = v
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .ok_or_else(|| bad(cmd, format!("`faces` must be a list of face indices (whole numbers from 0), at most {MAX_FACES}")))?;
        if i >= count {
            return Err(bad(cmd, format!("`faces`: the body has {count} faces, so there is no face {i}")));
        }
        if let Some(seen) = taken.get_mut(i)
            && !*seen
        {
            *seen = true;
            out.push(i);
        }
    }
    if out.is_empty() {
        return Err(bad(cmd, "`faces` must name at least one face"));
    }
    Ok(Some(out))
}

/// The step that keeps `n` points within `max`: the smallest whole number `k` with
/// ceil(n / k) <= max.
fn sample_step(n: usize, max: usize) -> usize {
    if n <= max || max == 0 { 1 } else { n.div_ceil(max) }
}

/// How far a point stored as three 32-bit float coordinates, each of magnitude at most `m`, may
/// lie from the point it stands for. Each coordinate is off by at most half a unit in the last
/// place, at most `m` × 2⁻²⁴, so the point is off by at most √3 × `m` × 2⁻²⁴.
fn f32_precision(m: f64) -> f64 {
    3.0_f64.sqrt() * m.abs() * (1.0 / 16_777_216.0)
}

/// One sampled point's outcome.
enum Outcome {
    /// The unsigned distance, the point and the closest point.
    Measured {
        distance: f64,
        point: Vec3,
        closest: Vec3,
    },
    Failed(String),
}

fn deviation(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "pointcloud.deviation";
    let tolerance = number(cmd, p, "tolerance", TOLERANCE, 0.0001..=1000.0)?;
    let max_samples = number(cmd, p, "max_samples", MAX_SAMPLES as f64, 1.0..=MAX_SAMPLES_LIMIT as f64)? as usize;
    let worst_count = number(cmd, p, "worst", WORST as f64, 0.0..=1000.0)? as usize;
    let want_signed = flag(cmd, p, "signed")?;
    let per_point = flag(cmd, p, "per_point")?.unwrap_or(false);
    let ci = find_cloud(s, p, cmd)?;
    let body_name = match p.get("body") {
        Some(Value::String(n)) => n.clone(),
        _ => return Err(bad(cmd, "`body` must be a body name")),
    };
    let st = s.world_state()?;
    let model = st.body(&body_name).ok_or_else(|| bad(cmd, format!("no body `{body_name}`")))?;
    let body = &model.body;
    if body.is_mesh() {
        return Err(bad(cmd, format!("`{body_name}` is a mesh body: its triangles are not exact faces to measure against")));
    }
    let face_count = body.face_count();
    if face_count == 0 {
        return Err(bad(cmd, format!("`{body_name}` has no faces")));
    }
    let faces = face_list(cmd, p, face_count)?;
    let closed = super::inspect::encloses_volume(body) && faces.is_none();
    let signed = match want_signed {
        Some(true) if !closed => {
            return Err(bad(
                cmd,
                if faces.is_some() {
                    "`signed` needs every face of the body: a face set has no inside".to_string()
                } else {
                    format!("`signed` needs a body that encloses a volume, and `{body_name}` does not: it has no inside")
                },
            ));
        }
        Some(x) => x,
        None => closed,
    };
    let all_faces: Vec<usize>;
    let members: &[usize] = match &faces {
        Some(f) => f,
        None => {
            all_faces = (0..face_count).collect();
            &all_faces
        }
    };
    let cloud = s.doc.point_clouds.get(ci).ok_or_else(|| bad(cmd, "point cloud"))?;
    let cloud_points = cloud.len();
    let step = sample_step(cloud_points, max_samples);
    let indices: Vec<usize> = (0..cloud_points).step_by(step).collect();
    let mut warnings = Vec::new();
    if cloud_points == 0 {
        warnings.push("the cloud has no points, so nothing was measured".to_string());
    }
    if step > 1 {
        warnings.push(format!(
            "the cloud has {cloud_points} points, more than max_samples ({max_samples}): one point in every {step} was measured, {} in all",
            indices.len()
        ));
    }
    if !cloud.filters.is_empty() || !cloud.visible {
        warnings.push("the cloud's filters and visibility change only what the view shows: every point was a candidate to measure".to_string());
    }
    // How exactly the cloud stores its points: its coordinates are 32-bit floats from its origin,
    // then placed.
    let magnitude = cloud.points.bounds().map(|(lo, hi)| lo.iter().chain(hi.iter()).fold(0.0_f64, |m, v| m.max(f64::from(*v).abs()))).unwrap_or(0.0);
    let point_precision_mm = f32_precision(magnitude);

    let outcomes: Vec<Outcome> = solidcraft_kernel::map_on_threads(&indices, THREADS, |&i| {
        let Some(point) = cloud.world_point(i) else { return Outcome::Failed("the point is missing from the cloud".to_string()) };
        if !(point.x.is_finite() && point.y.is_finite() && point.z.is_finite()) {
            return Outcome::Failed("the point's coordinates are not finite".to_string());
        }
        match solidcraft_kernel::point_closest(point, body, Members::Faces(members)) {
            Ok(Some(d)) if d.distance.is_finite() && d.to.is_finite() => Outcome::Measured { distance: d.distance, point, closest: d.to },
            Ok(Some(_)) => Outcome::Failed("the kernel gave a distance that is not finite".to_string()),
            Ok(None) => Outcome::Failed("the body has no exact faces to measure against".to_string()),
            Err(e) => Outcome::Failed(e.to_string()),
        }
    })
    .into_iter()
    .map(|o| o.unwrap_or_else(|| Outcome::Failed("the closest-point solve stopped".to_string())))
    .collect();

    // The sign of each measured point, from one exact classification of the body.
    let measured_points: Vec<Vec3> =
        outcomes.iter().filter_map(|o| if let Outcome::Measured { point, .. } = o { Some(*point) } else { None }).collect();
    let classes: Option<Vec<PointClass>> = if signed && !measured_points.is_empty() {
        match body.classify_points(&measured_points) {
            Ok(c) if c.len() == measured_points.len() => Some(c),
            Ok(_) => return Err(EngineError::Other(format!("{cmd}: the point classification gave the wrong number of answers"))),
            Err(e) => {
                return Err(bad(
                    cmd,
                    format!("the points could not be classified inside or outside `{body_name}` ({e}); measure with signed: false"),
                ));
            }
        }
    } else {
        None
    };

    let mut distances: Vec<Option<f64>> = Vec::with_capacity(outcomes.len());
    let mut listed: Vec<DeviationPoint> = Vec::new();
    let mut failures: Vec<DeviationFailure> = Vec::new();
    let (mut failed, mut within, mut sum_abs, mut sum_signed, mut sum_sq) = (0usize, 0usize, 0.0_f64, 0.0_f64, 0.0_f64);
    let (mut outside, mut inside, mut on_boundary) = (0usize, 0usize, 0usize);
    let mut max_abs: Option<f64> = None;
    let mut k = 0usize;
    for (&index, outcome) in indices.iter().zip(&outcomes) {
        match outcome {
            Outcome::Failed(reason) => {
                failed += 1;
                distances.push(None);
                if failures.len() < MAX_FAILURES {
                    failures.push(DeviationFailure { index, reason: reason.clone() });
                }
            }
            Outcome::Measured { distance, point, closest } => {
                let class = classes.as_ref().and_then(|c| c.get(k).copied());
                k += 1;
                let value = match class {
                    Some(PointClass::Outside) => {
                        outside += 1;
                        *distance
                    }
                    Some(PointClass::Inside) => {
                        inside += 1;
                        -*distance
                    }
                    Some(PointClass::OnBoundary) => {
                        on_boundary += 1;
                        0.0
                    }
                    None => *distance,
                };
                let a = value.abs();
                sum_abs += a;
                sum_signed += value;
                sum_sq += a * a;
                if a <= tolerance {
                    within += 1;
                }
                max_abs = Some(max_abs.map_or(a, |m: f64| m.max(a)));
                distances.push(Some(value));
                listed.push(DeviationPoint { index, point: (*point).into(), closest: (*closest).into(), distance_mm: value });
            }
        }
    }
    let measured = indices.len() - failed;
    // The farthest first; equal distances by index, so the list is the same each time.
    listed.sort_by(|x, y| y.distance_mm.abs().total_cmp(&x.distance_mm.abs()).then(x.index.cmp(&y.index)));
    listed.truncate(worst_count);
    if failed > 0 {
        warnings.push(format!("{failed} of {} sampled points could not be measured and are left out of the statistics", indices.len()));
    }
    if measured > 0 && signed && on_boundary > 0 {
        warnings.push(format!("{on_boundary} points lie on the body's boundary within its tolerance: their distance counts as 0"));
    }
    let mean = |sum: f64| (measured > 0).then(|| sum / measured as f64);
    let out = PointCloudDeviationResult {
        cloud: cloud.id,
        body: body_name,
        faces: faces.map(|f| f.into_iter().map(|i| i as u64).collect()),
        signed,
        sign_convention: signed.then(|| SIGN_CONVENTION.to_string()),
        tolerance_mm: tolerance,
        cloud_points,
        sample_step: step,
        sampled: indices.len(),
        measured,
        failed,
        mean_abs_mm: mean(sum_abs),
        mean_signed_mm: if signed { mean(sum_signed) } else { None },
        rms_mm: mean(sum_sq).map(f64::sqrt),
        max_abs_mm: max_abs,
        within_tolerance: within,
        within_tolerance_fraction: mean(within as f64),
        outside: signed.then_some(outside),
        inside: signed.then_some(inside),
        on_boundary: signed.then_some(on_boundary),
        worst: listed,
        failures,
        distances: per_point.then_some(distances),
        body_tolerance_mm: body.record().tolerance,
        point_precision_mm,
        warnings,
        param_warnings: Vec::new(),
    };
    serde_json::to_value(out).map_err(|e| EngineError::Other(e.to_string()))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::sample_step;
    use crate::Session;

    /// A binary PLY of `points` in a fresh temporary folder; its path.
    fn ply(test: &str, points: &[[f32; 3]]) -> String {
        let dir = std::env::temp_dir().join(format!("solidcraft-pointcloud-deviation-{test}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut b = format!(
            "ply\nformat binary_little_endian 1.0\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\nend_header\n",
            points.len()
        )
        .into_bytes();
        for p in points {
            for v in p {
                b.extend(v.to_le_bytes());
            }
        }
        let path = dir.join("scan.ply");
        std::fs::write(&path, b).unwrap();
        path.to_string_lossy().to_string()
    }

    /// A session with a 20 × 20 × 10 mm box from the origin, named `Box`, and the cloud `points`.
    fn box_and_cloud(test: &str, points: &[[f32; 3]]) -> Session {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 10, "corner": [0, 0, 0], "body_name": "Box"})).unwrap();
        s.execute("pointcloud.insert", &json!({"path": ply(test, points), "name": "scan"})).unwrap();
        s
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-5
    }

    fn f(v: &Value, k: &str) -> f64 {
        v[k].as_f64().unwrap_or(f64::NAN)
    }

    #[test]
    fn points_at_known_offsets_from_a_box_face_give_their_offsets() {
        // Above the top face (z = 10) by 0.05, 0.1 (on the tolerance) and 0.3; one inside by 0.2.
        let pts = [[5.0, 5.0, 10.05], [10.0, 10.0, 10.1], [15.0, 5.0, 10.3], [10.0, 15.0, 9.8]];
        let mut s = box_and_cloud("offsets", &pts);
        let r = s.execute("pointcloud.deviation", &json!({"cloud": "scan", "body": "Box", "per_point": true})).unwrap();
        assert_eq!(r["signed"], json!(true));
        assert_eq!((r["sampled"].as_u64(), r["measured"].as_u64(), r["failed"].as_u64()), (Some(4), Some(4), Some(0)));
        let d: Vec<f64> = r["distances"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        for (got, want) in d.iter().zip([0.05, 0.1, 0.3, -0.2]) {
            assert!((got - want).abs() <= 1e-5, "distance {got}, expected {want}");
        }
        // Points stored as 32-bit floats: compare with the stored values' own error.
        assert!(close(f(&r, "mean_abs_mm"), (0.05 + 0.1 + 0.3 + 0.2) / 4.0));
        assert!(close(f(&r, "mean_signed_mm"), (0.05 + 0.1 + 0.3 - 0.2) / 4.0));
        assert!(close(f(&r, "rms_mm"), ((0.05f64.powi(2) + 0.01 + 0.09 + 0.04) / 4.0).sqrt()));
        assert!(close(f(&r, "max_abs_mm"), 0.3));
        assert_eq!((r["outside"].as_u64(), r["inside"].as_u64()), (Some(3), Some(1)));
        assert_eq!(r["worst"][0]["index"], json!(2));
        let c = &r["worst"][0]["closest"];
        assert!(close(c[0].as_f64().unwrap(), 15.0) && close(c[1].as_f64().unwrap(), 5.0) && close(c[2].as_f64().unwrap(), 10.0));
        // 0.05 is within 0.1; whether 10.1 stored as f32 is is decided by the stored value, so
        // ask with a tolerance clear of it on both sides.
        let r = s.execute("pointcloud.deviation", &json!({"cloud": "scan", "body": "Box", "tolerance": 0.1001})).unwrap();
        assert_eq!(r["within_tolerance"], json!(2));
        assert!(close(f(&r, "within_tolerance_fraction"), 0.5));
        let r = s.execute("pointcloud.deviation", &json!({"cloud": "scan", "body": "Box", "tolerance": 0.0999})).unwrap();
        assert_eq!(r["within_tolerance"], json!(1));
    }

    #[test]
    fn a_point_past_a_face_edge_is_measured_to_the_edge_not_the_plane() {
        // Beyond the box's edge x = 20, z = 10 by 3 in x and 4 in z: 5 mm from the edge, though
        // only 4 mm above the top face's unbounded plane.
        let mut s = box_and_cloud("edge", &[[23.0, 10.0, 14.0]]);
        let r = s.execute("pointcloud.deviation", &json!({"cloud": "scan", "body": "Box"})).unwrap();
        assert!(close(f(&r, "max_abs_mm"), 5.0), "{r}");
        let c = &r["worst"][0]["closest"];
        assert!(close(c[0].as_f64().unwrap(), 20.0) && close(c[2].as_f64().unwrap(), 10.0), "{r}");
        // Against the top face alone (as trimmed) it is still 5 mm, and unsigned.
        let faces = s.execute("model.faces", &json!({"body": "Box"})).unwrap();
        let top = faces["faces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["plane_normal"].as_array().is_some_and(|n| n.len() == 3 && close(n[2].as_f64().unwrap_or(0.0), 1.0)))
            .and_then(|x| x["index"].as_u64())
            .unwrap();
        let r = s.execute("pointcloud.deviation", &json!({"cloud": "scan", "body": "Box", "faces": [top]})).unwrap();
        assert_eq!(r["signed"], json!(false));
        assert!(r["sign_convention"].is_null() && r["inside"].is_null());
        assert!(close(f(&r, "max_abs_mm"), 5.0), "{r}");
    }

    #[test]
    fn a_sphere_gives_radial_offsets_with_their_sign() {
        let mut s = Session::default();
        s.execute("solid.sphere", &json!({"center": [0, 0, 0], "radius": 10, "body_name": "Ball"})).unwrap();
        let pts = [[10.5, 0.0, 0.0], [0.0, -9.0, 0.0], [0.0, 0.0, 12.0]];
        s.execute("pointcloud.insert", &json!({"path": ply("sphere", &pts), "name": "scan"})).unwrap();
        let r = s.execute("pointcloud.deviation", &json!({"cloud": "scan", "body": "Ball", "per_point": true})).unwrap();
        let d: Vec<f64> = r["distances"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        for (got, want) in d.iter().zip([0.5, -1.0, 2.0]) {
            assert!((got - want).abs() <= 1e-5, "distance {got}, expected {want}");
        }
    }

    #[test]
    fn the_cloud_placement_is_used_and_left_unchanged() {
        let mut s = box_and_cloud("placed", &[[5.0, 5.0, 0.0]]);
        // Lift the cloud 12 mm: the point is then 2 mm above the top face.
        let m = json!([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 12]);
        s.execute("pointcloud.edit", &json!({"cloud": "scan", "matrix": m})).unwrap();
        let before = s.doc.point_clouds.clone();
        let undo = s.undo.len();
        let r = s.execute("pointcloud.deviation", &json!({"cloud": "scan", "body": "Box"})).unwrap();
        assert!(close(f(&r, "max_abs_mm"), 2.0), "{r}");
        assert_eq!((&s.doc.point_clouds, s.undo.len()), (&before, undo));
        // Points stored in metres are measured in mm.
        s.execute("pointcloud.edit", &json!({"cloud": "scan", "matrix": [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]})).unwrap();
        let mut s2 = box_and_cloud("metres", &[[0.005, 0.005, 0.012]]);
        s2.execute("pointcloud.edit", &json!({"cloud": "scan", "units": "m"})).unwrap();
        let r = s2.execute("pointcloud.deviation", &json!({"cloud": "scan", "body": "Box"})).unwrap();
        assert!((f(&r, "max_abs_mm") - 2.0).abs() <= 1e-3, "{r}");
    }

    #[test]
    fn sampling_is_every_nth_point_and_counted() {
        let pts: Vec<[f32; 3]> = (0..10).map(|i| [1.0 + i as f32, 5.0, 11.0]).collect();
        let mut s = box_and_cloud("sampled", &pts);
        let r = s.execute("pointcloud.deviation", &json!({"cloud": "scan", "body": "Box", "max_samples": 4, "per_point": true})).unwrap();
        assert_eq!((r["cloud_points"].as_u64(), r["sample_step"].as_u64(), r["sampled"].as_u64()), (Some(10), Some(3), Some(4)));
        assert_eq!(r["distances"].as_array().map(Vec::len), Some(4));
        assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("one point in every 3")));
        assert_eq!((sample_step(10, 4), sample_step(10, 10), sample_step(10, 5), sample_step(0, 5)), (3, 1, 2, 1));
    }

    #[test]
    fn an_empty_cloud_has_no_statistics_not_zero() {
        let mut s = box_and_cloud("empty", &[[5.0, 5.0, 11.0]]);
        s.doc_mut().point_clouds[0].points = solidcraft_doc::point_cloud::Points::new(std::sync::Arc::from(Vec::<f32>::new()));
        let r = s.execute("pointcloud.deviation", &json!({"cloud": "scan", "body": "Box"})).unwrap();
        assert_eq!((r["sampled"].as_u64(), r["measured"].as_u64()), (Some(0), Some(0)));
        for k in ["mean_abs_mm", "rms_mm", "max_abs_mm", "within_tolerance_fraction", "mean_signed_mm"] {
            assert!(r[k].is_null(), "{k} must be null: {r}");
        }
    }

    #[test]
    fn a_number_picks_the_cloud_with_that_id_before_a_cloud_with_that_name() {
        // The first cloud, 1 mm above the box, is named after the second cloud's id; the
        // second cloud lies 3 mm above it.
        let mut s = box_and_cloud("id-first", &[[5.0, 5.0, 11.0]]);
        s.execute("pointcloud.insert", &json!({"path": ply("id-first-2", &[[5.0, 5.0, 13.0]]), "name": "second"})).unwrap();
        let second = s.doc.point_clouds[1].id;
        s.execute("pointcloud.edit", &json!({"cloud": "scan", "name": second.to_string()})).unwrap();
        let r = s.execute("pointcloud.deviation", &json!({"cloud": second, "body": "Box"})).unwrap();
        assert_eq!(r["cloud"], json!(second));
        assert!(close(f(&r, "max_abs_mm"), 3.0), "{r}");
        // A string is a name first: the first cloud.
        let r = s.execute("pointcloud.deviation", &json!({"cloud": second.to_string(), "body": "Box"})).unwrap();
        assert!(close(f(&r, "max_abs_mm"), 1.0), "{r}");
        // A face listed twice is measured once, and the list keeps its first order.
        let r = s.execute("pointcloud.deviation", &json!({"cloud": "second", "body": "Box", "faces": [3, 1, 3]})).unwrap();
        assert_eq!(r["faces"], json!([3, 1]));
    }

    #[test]
    fn bad_requests_are_refused() {
        let mut s = box_and_cloud("refused", &[[5.0, 5.0, 11.0]]);
        for p in [
            json!({"cloud": "scan", "body": "Nope"}),
            json!({"cloud": "nope", "body": "Box"}),
            json!({"cloud": "scan", "body": "Box", "faces": [99]}),
            json!({"cloud": "scan", "body": "Box", "faces": []}),
            json!({"cloud": "scan", "body": "Box", "faces": [0], "signed": true}),
            json!({"cloud": "scan", "body": "Box", "tolerance": 0}),
            json!({"cloud": "scan", "body": "Box", "max_samples": 0}),
        ] {
            assert!(s.execute("pointcloud.deviation", &p).is_err(), "{p} should be refused");
        }
    }
}
