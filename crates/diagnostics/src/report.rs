//! The JSON report and the printed summary.

use std::fmt::Write;
use std::path::Path;

use serde_json::{json, Value};

use crate::body::{Body, EdgeInfo, FaceInfo, Mesh};
use crate::render::{Frame, View};

/// How many of the largest edge tolerances the summary prints.
const LARGEST: usize = 8;

/// The report: the body's tolerance, every face and edge with its tolerance and the pixel
/// where its label belongs, and the faces the mesher refused.
pub fn build(
    file: &Path,
    body: &Body,
    faces: &[FaceInfo],
    edges: &[EdgeInfo],
    mesh: &Mesh,
    view: View,
    frame: &Frame,
) -> Value {
    let centres = face_centres(mesh, faces);
    let faces: Vec<Value> = faces
        .iter()
        .zip(&centres)
        .map(|(f, c)| {
            json!({
                "index": f.index,
                "surface": f.surface,
                "tolerance": f.tolerance,
                "label_pixel": c.map(|p| round(frame.pixel(p))),
            })
        })
        .collect();
    let edges: Vec<Value> = edges
        .iter()
        .map(|e| {
            json!({
                "index": e.index,
                "tolerance": e.tolerance,
                "faces": e.faces,
                "length": length(&e.points),
                "label_pixel": halfway(&e.points).map(|p| round(frame.pixel(p))),
            })
        })
        .collect();
    let refused: Vec<Value> = mesh
        .refused
        .iter()
        .map(|(f, why)| json!({"face": f, "reason": why}))
        .collect();
    json!({
        "file": file.display().to_string(),
        "body_tolerance": body.brep.tolerance(),
        "view": {"azimuth": view.azimuth, "elevation": view.elevation},
        "mesh": {"triangles": mesh.triangles.len(), "size": mesh.size, "refused": refused},
        "faces": faces,
        "edges": edges,
    })
}

/// The body tolerance, the counts and the largest edge tolerances, one per line.
pub fn summary(body: &Body, edges: &[EdgeInfo], mesh: &Mesh) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "body tolerance {:e}; {} edges; {} triangles; {} faces refused",
        body.brep.tolerance(),
        edges.len(),
        mesh.triangles.len(),
        mesh.refused.len()
    );
    let mut by_size: Vec<&EdgeInfo> = edges.iter().collect();
    by_size.sort_by(|a, b| b.tolerance.total_cmp(&a.tolerance));
    for e in by_size.iter().take(LARGEST) {
        let _ = writeln!(
            s,
            "  edge {:>4}  tolerance {:.3e}  faces {:?}",
            e.index, e.tolerance, e.faces
        );
    }
    s
}

/// Each face's area-weighted centroid, `None` for a face without triangles.
fn face_centres(mesh: &Mesh, faces: &[FaceInfo]) -> Vec<Option<[f64; 3]>> {
    let mut sums = vec![([0.0; 3], 0.0); faces.len()];
    let slot = |f: usize| faces.iter().position(|x| x.index == f);
    let point = |i: u32| mesh.positions.get(i as usize).copied();
    for (t, &f) in mesh.triangles.iter().zip(&mesh.triangle_face) {
        let (Some(k), Some(a), Some(b), Some(c)) = (slot(f), point(t[0]), point(t[1]), point(t[2]))
        else {
            continue;
        };
        let area = triangle_area(a, b, c);
        if let Some((sum, total)) = sums.get_mut(k) {
            let [s0, s1, s2] = *sum;
            *sum = [
                s0 + area * (a[0] + b[0] + c[0]) / 3.0,
                s1 + area * (a[1] + b[1] + c[1]) / 3.0,
                s2 + area * (a[2] + b[2] + c[2]) / 3.0,
            ];
            *total += area;
        }
    }
    sums.into_iter()
        .map(|(s, a)| (a > 0.0).then(|| s.map(|v| v / a)))
        .collect()
}

fn triangle_area(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt()
}

fn length(points: &[[f64; 3]]) -> f64 {
    points
        .windows(2)
        .filter_map(|w| Some(distance(*w.first()?, *w.get(1)?)))
        .sum()
}

/// The point halfway along the polyline.
fn halfway(points: &[[f64; 3]]) -> Option<[f64; 3]> {
    let mut left = length(points) / 2.0;
    for w in points.windows(2) {
        let (a, b) = (*w.first()?, *w.get(1)?);
        let d = distance(a, b);
        if d >= left && d > 0.0 {
            let t = left / d;
            return Some([
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]);
        }
        left -= d;
    }
    points.first().copied()
}

fn round([x, y]: [f64; 2]) -> [f64; 2] {
    [x.round(), y.round()]
}
