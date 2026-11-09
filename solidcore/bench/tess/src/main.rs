//! meshbake FILE.nb [--mode display|export] [--no-dev] : mesh one body with SolidCraft's mesher (`Body::mesh_plan`) and
//! solidcore-tess's (`solidcore_tess::plan`), grade each, print JSON lines. `MB_ONLY=solidcraft|solidcore-tess` runs one.
use solidcore::eval::SurfaceEval;
use solidcore_record::brep::Brep;
use std::collections::HashMap;
use std::time::Instant;

struct M {
    pos: Vec<[f64; 3]>,
    tris: Vec<[u32; 3]>,
    tri_face: Vec<u32>,
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn len(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn pct(v: &mut Vec<f64>, p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() - 1) as f64 * p) as usize]
}

fn bbox_diag(rec: &Brep) -> f64 {
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for v in &rec.vertices {
        for k in 0..3 {
            lo[k] = lo[k].min(v.point[k]);
            hi[k] = hi[k].max(v.point[k]);
        }
    }
    len(sub(hi, lo))
}

/// Grade a mesh: topology, slivers, angle, deviation.
fn grade(m: &M, rec: &Brep, size: f64, chord: f64, dev: bool) -> serde_json::Value {
    use serde_json::json;
    let eps = size * 1e-7;
    // weld
    let mut cells: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::new();
    let mut reps: Vec<[f64; 3]> = Vec::new();
    let mut id: Vec<u32> = Vec::with_capacity(m.pos.len());
    for p in &m.pos {
        let c = [(p[0] / eps).floor() as i64, (p[1] / eps).floor() as i64, (p[2] / eps).floor() as i64];
        let mut found = None;
        'o: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(l) = cells.get(&(c[0] + dx, c[1] + dy, c[2] + dz)) {
                        for &r in l {
                            if len(sub(reps[r as usize], *p)) <= eps {
                                found = Some(r);
                                break 'o;
                            }
                        }
                    }
                }
            }
        }
        let r = found.unwrap_or_else(|| {
            let r = reps.len() as u32;
            reps.push(*p);
            cells.entry((c[0], c[1], c[2])).or_default().push(r);
            r
        });
        id.push(r);
    }
    let mut edges: HashMap<(u32, u32), (u32, i32, Vec<usize>)> = HashMap::new();
    let mut degenerate = 0usize;
    let mut normals: Vec<[f64; 3]> = Vec::with_capacity(m.tris.len());
    let (mut minang_all, mut over1, mut over5) = (180.0f64, 0usize, 0usize);
    let mut minangs = Vec::new();
    let mut aspects = Vec::new();
    let mut area = 0.0;
    for (ti, t) in m.tris.iter().enumerate() {
        let (a, b, c) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
        let n = cross(sub(b, a), sub(c, a));
        let ar = len(n) / 2.0;
        area += ar;
        let w = [id[t[0] as usize], id[t[1] as usize], id[t[2] as usize]];
        if ar <= 1e-12 * size * size || w[0] == w[1] || w[1] == w[2] || w[0] == w[2] {
            degenerate += 1;
            normals.push([0.0; 3]);
            continue;
        }
        normals.push([n[0] / (2.0 * ar), n[1] / (2.0 * ar), n[2] / (2.0 * ar)]);
        let (la, lb, lc) = (len(sub(b, c)), len(sub(a, c)), len(sub(a, b)));
        let ang = |x: f64, y: f64, z: f64| ((y * y + z * z - x * x) / (2.0 * y * z)).clamp(-1.0, 1.0).acos().to_degrees();
        let ma = ang(la, lb, lc).min(ang(lb, la, lc)).min(ang(lc, la, lb));
        minang_all = minang_all.min(ma);
        if ma < 1.0 {
            over1 += 1;
        }
        if ma < 5.0 {
            over5 += 1;
        }
        minangs.push(ma);
        aspects.push(la.max(lb).max(lc) * (la + lb + lc) / (4.0 * 3f64.sqrt() * ar));
        for (x, y) in [(w[0], w[1]), (w[1], w[2]), (w[2], w[0])] {
            let (k, s) = if x < y { ((x, y), 1) } else { ((y, x), -1) };
            let e = edges.entry(k).or_insert((0, 0, Vec::new()));
            e.0 += 1;
            e.1 += s;
            e.2.push(ti);
        }
    }
    let (mut open, mut nonman, mut flipped) = (0usize, 0usize, 0usize);
    let mut max_dihedral = 0.0f64;
    for (_, (c, s, ts)) in &edges {
        match *c {
            1 => open += 1,
            2 => {
                if *s != 0 {
                    flipped += 1;
                }
                let (a, b) = (ts[0], ts[1]);
                if m.tri_face[a] == m.tri_face[b] {
                    let d = dot(normals[a], normals[b]).clamp(-1.0, 1.0).acos().to_degrees();
                    max_dihedral = max_dihedral.max(d);
                }
            }
            _ => nonman += 1,
        }
    }
    // open edges that are geometrically sealed by another open edge (T-junctions), vs true gaps
    let open_list: Vec<(u32, u32)> = edges.iter().filter(|(_, v)| v.0 == 1).map(|(k, _)| *k).collect();
    let mut unsealed = -1i64;
    if open_list.len() <= 20000 {
        unsealed = 0;
        let pt = |i: u32| reps[i as usize];
        let seg_dist = |p: [f64; 3], a: [f64; 3], b: [f64; 3]| {
            let ab = sub(b, a);
            let l2 = dot(ab, ab);
            let t = if l2 > 0.0 { (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
            len(sub(p, [a[0] + t * ab[0], a[1] + t * ab[1], a[2] + t * ab[2]]))
        };
        let tol = size * 1e-6;
        for (i, e) in open_list.iter().enumerate() {
            let (a, b) = (pt(e.0), pt(e.1));
            let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0];
            let sealed = open_list.iter().enumerate().any(|(j, f)| j != i && seg_dist(mid, pt(f.0), pt(f.1)) <= tol);
            if !sealed {
                unsealed += 1;
            }
        }
    }
    let brep_boundary = rec.edges.iter().filter(|e| e.uses.len() == 1).count();
    let nfaces = rec.faces.len();
    let mut has = vec![false; nfaces];
    for f in &m.tri_face {
        if let Some(h) = has.get_mut(*f as usize) {
            *h = true;
        }
    }
    let empty_faces = has.iter().filter(|h| !**h).count();
    let mut out = json!({
        "tris": m.tris.len(), "verts": m.pos.len(), "faces": nfaces, "faces_without_tris": empty_faces,
        "open_edges": open, "open_unsealed": unsealed, "nonmanifold_edges": nonman, "flipped_edges": flipped, "degenerate_tris": degenerate,
        "brep_boundary_edges": brep_boundary, "area": area,
        "min_angle_deg": minang_all, "tris_lt1deg": over1, "tris_lt5deg": over5,
        "median_min_angle": pct(&mut minangs, 0.5), "p99_aspect": pct(&mut aspects.clone(), 0.99), "max_aspect": aspects.iter().cloned().fold(0.0, f64::max),
        "max_dihedral_deg": max_dihedral,
    });
    if dev {
        // deviation: sample up to 40 tris per face, 7 points each, project on the exact surface.
        let mut per_face: HashMap<u32, Vec<usize>> = HashMap::new();
        for (ti, f) in m.tri_face.iter().enumerate() {
            per_face.entry(*f).or_default().push(ti);
        }
        let mut devs: Vec<f64> = Vec::new();
        let mut unproj = 0usize;
        let mut faces: Vec<_> = per_face.into_iter().collect();
        faces.sort();
        let budget = 4000usize / faces.len().max(1);
        let per = budget.clamp(8, 60);
        for (fi, tl) in faces {
            let Some(face) = rec.faces.get(fi as usize) else { continue };
            let Ok(ks) = solidcore_convert::surface(&face.surface) else { continue };
            let Ok(se) = SurfaceEval::new(&ks) else { continue };
            let starts = solidcore::eval::uv::Starts::default();
            let step = (tl.len() / per).max(1);
            for &ti in tl.iter().step_by(step).take(per) {
                let t = m.tris[ti];
                let (a, b, c) = (m.pos[t[0] as usize], m.pos[t[1] as usize], m.pos[t[2] as usize]);
                let bary = [[1. / 3., 1. / 3., 1. / 3.], [0.5, 0.5, 0.0], [0.0, 0.5, 0.5], [0.5, 0.0, 0.5], [0.6, 0.2, 0.2], [0.2, 0.6, 0.2], [0.2, 0.2, 0.6]];
                for w in bary {
                    let p = [0, 1, 2].map(|k| w[0] * a[k] + w[1] * b[k] + w[2] * c[k]);
                    let pv = solidcore::Vec3::new(p[0], p[1], p[2]);
                    let q = se
                        .project_with(&starts, pv, None, (5.0 * chord).max(size * 1e-3))
                        .or_else(|| se.project_with(&starts, pv, None, f64::INFINITY));
                    match q.and_then(|uv| se.point(uv)) {
                        Some(s) => devs.push(((s.x - p[0]).powi(2) + (s.y - p[1]).powi(2) + (s.z - p[2]).powi(2)).sqrt()),
                        None => unproj += 1,
                    }
                }
            }
        }
        let n = devs.len();
        let mean = devs.iter().sum::<f64>() / n.max(1) as f64;
        let maxd = devs.iter().cloned().fold(0.0, f64::max);
        let over = devs.iter().filter(|d| **d > chord * 1.0000001).count();
        let p99 = pct(&mut devs, 0.99);
        out["dev_samples"] = json!(n);
        out["dev_unprojected"] = json!(unproj);
        out["dev_max"] = json!(maxd);
        out["dev_mean"] = json!(mean);
        out["dev_p99"] = json!(p99);
        out["dev_over_chord"] = json!(over);
    }
    out
}


fn med(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let mode = args.iter().position(|a| a == "--mode").map(|i| args[i + 1].clone()).unwrap_or("display".into());
    let dev = !args.iter().any(|a| a == "--no-dev");
    let bytes = std::fs::read(path).expect("read");
    let name = std::path::Path::new(path).file_stem().unwrap().to_string_lossy().to_string();
    let load = || solidcraft_kernel::decode_native_body(&bytes);
    let body = match load() {
        Ok(b) => b,
        Err(e) => {
            println!("{}", serde_json::json!({"body": name, "mode": mode, "error": format!("load: {e}")}));
            return;
        }
    };
    let rec: Brep = body.record().clone();
    let size = body.size();
    let (sc_tol, core_tol, chord) = if mode == "display" {
        let t = solidcraft_kernel::MeshTolerance::display_of(size);
        (t, solidcore_tess::MeshTolerance::display_of(size), t.chord)
    } else {
        let t = solidcraft_kernel::MeshTolerance::export_of(size);
        (t, solidcore_tess::MeshTolerance::export_of(size), t.chord)
    };
    let _ = bbox_diag(&rec);
    let head = serde_json::json!({"body": name, "mode": mode, "size": size, "chord": chord, "faces": rec.faces.len(), "angle_deg": sc_tol.angle.map(|a| a.to_degrees())});
    println!("{}", serde_json::json!({"kind": "body", "info": head}));
    let mut surf_kinds: Vec<String> = rec.faces.iter().map(|f| format!("{:?}", f.surface).split(|c: char| !c.is_alphanumeric()).next().unwrap_or("").to_string()).collect();
    surf_kinds.sort();
    surf_kinds.dedup();
    println!("{}", serde_json::json!({"kind": "kinds", "body": name, "kinds": surf_kinds}));

    // --- mesher: SolidCraft kernel
    let only = std::env::var("MB_ONLY").ok().filter(|o| !o.is_empty());
    for which in ["solidcraft", "solidcore-tess"] {
        if only.as_deref().is_some_and(|o| o != which) {
            continue;
        }
        let run = |which: &str| -> Result<(M, f64, usize), String> {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(M, usize), String> {
                if which == "solidcraft" {
                    let b = &body;
                    let plan = b.mesh_plan(&sc_tol).map_err(|e| e.to_string())?;
                    // Before the one mesher, SolidCraft's plan evaluates to a mesh; after, it is
                    // solidcore-tess's plan and evaluates to a mesh and its refused faces.
                    #[cfg(old_geom_tess)]
                    let mesh = plan.evaluate();
                    #[cfg(not(old_geom_tess))]
                    let (mesh, _refused) = plan.evaluate();
                    let un = b.unmeshed_faces();
                    Ok((M { pos: mesh.positions.iter().map(|p| [p.x, p.y, p.z]).collect(), tris: mesh.triangles.clone(), tri_face: mesh.tri_face.clone() }, un))
                } else {
                    let (plan, stats) = solidcore_tess::plan(&rec, &core_tol, size).map_err(|e| e.to_string())?;
                    let (mesh, refused) = plan.evaluate();
                    let un = stats.unmeshed.len() + refused.len();
                    Ok((M { pos: mesh.positions.iter().map(|p| [p.x, p.y, p.z]).collect(), tris: mesh.triangles.clone(), tri_face: mesh.tri_face.clone() }, un))
                }
            }));
            match r {
                Ok(Ok((m, un))) => Ok((m, 0.0, un)),
                Ok(Err(e)) => Err(e),
                Err(_) => Err("panic".into()),
            }
        };
        // time
        let mut times = Vec::new();
        let first = {
            let t0 = Instant::now();
            let r = run(which);
            times.push(t0.elapsed().as_secs_f64() * 1e3);
            r
        };
        match first {
            Err(e) => {
                println!("{}", serde_json::json!({"kind": "result", "body": name, "mode": mode, "mesher": which, "ok": false, "error": e, "ms": times[0]}));
            }
            Ok((m, _, un)) => {
                if times[0] < 150.0 {
                    for _ in 0..4 {
                        let t0 = Instant::now();
                        let _ = run(which);
                        times.push(t0.elapsed().as_secs_f64() * 1e3);
                    }
                }
                let ms = med(times.clone());
                let mut g = grade(&m, &rec, size, chord, dev);
                g["kind"] = "result".into();
                g["body"] = name.clone().into();
                g["mode"] = mode.clone().into();
                g["mesher"] = which.into();
                g["ok"] = true.into();
                g["ms"] = ms.into();
                g["ms_first"] = times[0].into();
                g["unmeshed_reported"] = un.into();
                println!("{}", g);
            }
        }
    }
}
