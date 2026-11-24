//! How many STEP faces held as B-splines are planes, cylinders, cones, spheres or tori, and
//! what each recogniser costs per face: the kernel's structural recognisers (the cylinder,
//! cone, sphere and torus `face_surface::analytic_label` proves, the plane `plane_of_net`
//! proves) against `surface::recognise::analytic_of`.
//!
//! `cargo run --release -p goldens --example recognise_corpus -- <tol_mm> <file.stp>...`

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::time::{Duration, Instant};

use solidcore::eval::Surface as Record;
use solidcore::kernel::Kernel;
use solidcore::surface::face_surface::{analytic_label, face_surface};
use solidcore::surface::implicit::{plane_of_net, Implicit};
use solidcore::surface::recognise::analytic_of;
use solidcore::surface::surface::SurfaceKind;
use solidcore_record::step::model::Surface as StepSurface;

fn kind(i: &Implicit) -> &'static str {
    match i {
        Implicit::Plane { .. } => "plane",
        Implicit::Cone { tan_angle, .. } if *tan_angle == 0.0 => "cylinder",
        Implicit::Cone { .. } => "cone",
        Implicit::Sphere { .. } => "sphere",
        Implicit::Torus { .. } => "torus",
    }
}

fn expand(knots: &[f64], mults: &[u32]) -> Vec<f64> {
    knots
        .iter()
        .zip(mults)
        .flat_map(|(&k, &m)| std::iter::repeat_n(k, m as usize))
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let tol: f64 = args[0].parse().unwrap();
    let k = Kernel::default();
    let (mut faces, mut splines, mut kernel_analytic, mut nurbs) = (0usize, 0usize, 0usize, 0usize);
    let (mut old_hits, mut new_exact, mut new_tol, mut new_only, mut old_only) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let (mut t_old, mut t_new) = (Duration::ZERO, Duration::ZERO);
    let mut kinds_new = std::collections::BTreeMap::<&str, usize>::new();
    let mut kinds_old = std::collections::BTreeMap::<&str, usize>::new();
    let mut disagree = 0usize;
    for file in &args[1..] {
        let Ok(bytes) = std::fs::read(file) else {
            continue;
        };
        let Ok(model) = step_export::import(&bytes, &step_export::Limits::default()) else {
            continue;
        };
        for body in &model.bodies {
            for f in &body.faces {
                faces += 1;
                let StepSurface::BSpline(b) = &f.surface else {
                    continue;
                };
                splines += 1;
                let u_knots = expand(&b.u_knots, &b.u_mults);
                let v_knots = expand(&b.v_knots, &b.v_mults);
                let (pu, pv) = (b.u_degree as usize, b.v_degree as usize);
                let (Some(&u0), Some(&v0)) = (u_knots.get(pu), v_knots.get(pv)) else {
                    continue;
                };
                let (Some(&u1), Some(&v1)) = (
                    u_knots.get(u_knots.len().saturating_sub(pu + 1)),
                    v_knots.get(v_knots.len().saturating_sub(pv + 1)),
                ) else {
                    continue;
                };
                let rec = match &b.weights {
                    None => Record::Spline {
                        u_knots,
                        v_knots,
                        points: b
                            .points
                            .iter()
                            .map(|r| r.iter().map(|p| [p.x, p.y, p.z]).collect())
                            .collect(),
                    },
                    Some(w) => Record::RationalSpline {
                        u_knots,
                        v_knots,
                        homogeneous: b
                            .points
                            .iter()
                            .zip(w)
                            .map(|(r, wr)| {
                                r.iter()
                                    .zip(wr)
                                    .map(|(p, &w)| [p.x * w, p.y * w, p.z * w, w])
                                    .collect()
                            })
                            .collect(),
                    },
                };
                let domain = [[u0, u1], [v0, v1]];
                let Ok(made) = face_surface(&k, &rec, None, domain, &[]) else {
                    continue;
                };
                let SurfaceKind::BSpline(net) = made.surface.kind() else {
                    kernel_analytic += 1;
                    continue;
                };
                nurbs += 1;
                // the kernel's structural recognisers
                let t = Instant::now();
                let old = match analytic_label(&k, &rec, domain) {
                    Ok(Some(label)) => Some(match label {
                        Record::Cylinder { .. } => "cylinder",
                        Record::Cone { .. } => "cone",
                        Record::Sphere { .. } => "sphere",
                        Record::Torus { .. } => "torus",
                        _ => "other",
                    }),
                    _ => match plane_of_net(net) {
                        Ok(Some(_)) => Some("plane"),
                        _ => None,
                    },
                };
                t_old += t.elapsed();
                // the new query
                let t = Instant::now();
                let new = analytic_of(&k, &made.surface, tol);
                t_new += t.elapsed();
                let new = match new {
                    Ok(Some(r)) => {
                        if r.exact {
                            new_exact += 1;
                        } else {
                            new_tol += 1;
                        }
                        Some(kind(&r.implicit))
                    }
                    Ok(None) => None,
                    Err(e) => {
                        eprintln!("{file} face #{}: {e}", f.step_id);
                        None
                    }
                };
                if let Some(o) = old {
                    old_hits += 1;
                    *kinds_old.entry(o).or_default() += 1;
                }
                if let Some(n) = new {
                    *kinds_new.entry(n).or_default() += 1;
                }
                match (old, new) {
                    (Some(_), None) => {
                        old_only += 1;
                        eprintln!("old-only {file} face #{} {old:?}", f.step_id);
                    }
                    (None, Some(_)) => new_only += 1,
                    (Some(o), Some(n)) if o != n => {
                        disagree += 1;
                        eprintln!("disagree {file} face #{} old {o} new {n}", f.step_id);
                    }
                    _ => {}
                }
            }
        }
    }
    let per = |t: Duration| {
        if nurbs == 0 {
            0.0
        } else {
            t.as_secs_f64() * 1e6 / nurbs as f64
        }
    };
    println!("files {} faces {faces} step-bspline {splines} kernel-analytic-at-load {kernel_analytic} kernel-nurbs {nurbs}", args.len() - 1);
    println!(
        "structural: recognised {old_hits} {kinds_old:?} cost {:.1} us/face",
        per(t_old)
    );
    println!(
        "recognise (tol {tol}): exact {new_exact} within-tol {new_tol} {kinds_new:?} cost {:.1} us/face",
        per(t_new)
    );
    println!("new-only {new_only} old-only {old_only} disagree {disagree}");
}
