//! The ssi-corpus runner for this crate, to the contract of
//! github.com/jyviko/ssi-corpus:
//!
//! ```text
//! ssi_corpus_runner <case.json> <mode> <approx_tol|default> <out.json>
//! ```
//!
//! Each case surface becomes one exact rational B-spline patch (`ssi::shapes`
//! for the analytic ones; a NURBS is taken as given). The patches are
//! intersected with `intersect_bodies`, and every curve is written with its
//! points, both surfaces' (u, v) in the patches' own knot domains, and each
//! patch evaluated there. A port of Form Studio's `tools/ssi-corpus-runner.mjs`.
//!
//! Modes: `fs` only. With a numeric tolerance, features down to that size are
//! resolved (`shapes::resolving`); `default` runs the default options. Either
//! way the curves are marched at the default turn and, after the timer stops,
//! filled in to 0.01 rad by `densify`, so a chord stays within the corpus's
//! coverage tolerance: as the corpus's SMLib runner samples its B-spline
//! curves after its timer.
//!
//! Status: 1000 (the corpus's `SM_SUCCESS`) when curves come back; 2000 with the
//! refusal's code as `status_name` when the kernel refuses (`BOOLEAN_TANGENT`);
//! 3000 when it fails another way.

// `!(t > 0.0)` rejects NaN as well as non-positive tolerances.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

mod json;

use std::fmt::Write as _;
use std::time::Instant;

use json::Json;
use ssi::piece::V3;
use ssi::{intersect_bodies, shapes, Error, Options, Patch};

/// One output list of a curve: its key and each point's values.
type Column<'p> = Box<dyn Fn(&ssi::CurvePoint) -> Vec<f64> + 'p>;

const SUCCESS: i32 = 1000;
const REFUSED: i32 = 2000;
const FAILED: i32 = 3000;

fn v3(j: Option<&Json>, what: &str) -> Result<V3, String> {
    let v = j
        .and_then(Json::nums)
        .ok_or_else(|| format!("{what} is not a number array"))?;
    v.try_into()
        .map_err(|_| format!("{what} is not a 3-vector"))
}
fn pair(j: Option<&Json>, what: &str) -> Result<[f64; 2], String> {
    let v = j
        .and_then(Json::nums)
        .ok_or_else(|| format!("{what} is not a number array"))?;
    v.try_into().map_err(|_| format!("{what} is not a pair"))
}
fn number(j: Option<&Json>, what: &str) -> Result<f64, String> {
    j.and_then(Json::num)
        .ok_or_else(|| format!("{what} is not a number"))
}

/// One case surface as an exact patch.
fn patch_from_case(s: &Json) -> Result<Patch, String> {
    let kind = s
        .get("type")
        .and_then(Json::str)
        .ok_or("a surface has no type")?;
    if kind == "nurbs" {
        let degree = pair(s.get("degree"), "degree")?;
        let points: Vec<V3> = s
            .get("points")
            .and_then(Json::arr)
            .ok_or("points")?
            .iter()
            .map(|p| v3(Some(p), "a control point"))
            .collect::<Result<_, _>>()?;
        return Ok(Patch {
            u_degree: degree[0] as usize,
            v_degree: degree[1] as usize,
            u_count: number(s.get("nu"), "nu")? as usize,
            v_count: number(s.get("nv"), "nv")? as usize,
            poles: points,
            weights: Some(s.get("weights").and_then(Json::nums).ok_or("weights")?),
            u_knots: s.get("knots_u").and_then(Json::nums).ok_or("knots_u")?,
            v_knots: s.get("knots_v").and_then(Json::nums).ok_or("knots_v")?,
            analytic: None,
        });
    }
    let (x, y) = (v3(s.get("xaxis"), "xaxis")?, v3(s.get("yaxis"), "yaxis")?);
    Ok(match kind {
        "plane" => shapes::plane(
            v3(s.get("origin"), "origin")?,
            x,
            y,
            pair(s.get("u"), "u")?,
            pair(s.get("v"), "v")?,
        ),
        "cylinder" => shapes::cylinder(
            v3(s.get("origin"), "origin")?,
            x,
            y,
            number(s.get("radius"), "radius")?,
            pair(s.get("angle"), "angle")?,
            pair(s.get("height"), "height")?,
        ),
        "cone" => shapes::cone(
            v3(s.get("apex"), "apex")?,
            x,
            y,
            number(s.get("half_angle_deg"), "half_angle_deg")?,
            pair(s.get("height"), "height")?,
        ),
        "sphere" => shapes::sphere(
            v3(s.get("center"), "center")?,
            x,
            y,
            number(s.get("radius"), "radius")?,
            pair(s.get("lon"), "lon")?,
            pair(s.get("lat"), "lat")?,
        ),
        "torus" => shapes::torus(
            v3(s.get("center"), "center")?,
            x,
            y,
            number(s.get("R"), "R")?,
            number(s.get("r"), "r")?,
            pair(s.get("major"), "major")?,
            pair(s.get("minor"), "minor")?,
        ),
        other => return Err(format!("unknown surface type: {other}")),
    })
}

fn options_for(tol: &str, patches: &[&Patch]) -> Result<Options, String> {
    if tol == "default" {
        return Ok(Options::default());
    }
    let t: f64 = tol
        .parse()
        .map_err(|_| format!("approx_tol must be a number or \"default\": {tol}"))?;
    if !(t > 0.0) {
        return Err(format!("approx_tol must be positive: {tol}"));
    }
    Ok(shapes::resolving(t, patches))
}

fn run(args: &[String]) -> Result<(), String> {
    let [case_path, mode, tol, out_path] = args else {
        return Err("usage: ssi_corpus_runner case.json mode approx_tol|default out.json".into());
    };
    if mode != "fs" {
        return Err(format!("unknown mode {mode} (only fs)"));
    }
    let text = std::fs::read_to_string(case_path).map_err(|e| format!("{case_path}: {e}"))?;
    let case = json::parse(&text)?;
    let surfaces = case
        .get("surfaces")
        .and_then(Json::arr)
        .ok_or("the case has no surfaces")?;
    let [first, second] = surfaces else {
        return Err("a case has two surfaces".into());
    };
    let (patch_a, patch_b) = (patch_from_case(first)?, patch_from_case(second)?);
    let options = options_for(tol, &[&patch_a, &patch_b])?;

    let (body_a, body_b) = (
        std::slice::from_ref(&patch_a),
        std::slice::from_ref(&patch_b),
    );
    let started = Instant::now();
    let mut result = intersect_bodies(body_a, body_b, &options);
    let ms = started.elapsed().as_secs_f64() * 1e3;
    if let Ok(found) = &mut result {
        ssi::densify(found, body_a, body_b, 0.01).map_err(|e| e.to_string())?;
    }

    let mut out = String::new();
    out.push_str("{\"case\":");
    json::string(&mut out, case.get("id").and_then(Json::str).unwrap_or(""));
    out.push_str(",\"mode\":\"fs\",\"approx_tol\":");
    match tol.parse::<f64>() {
        Ok(t) => json::num(&mut out, t),
        Err(_) => out.push_str("\"default\""),
    }
    let (status, name, detail) = match &result {
        Ok(_) => (SUCCESS, "SUCCESS".to_string(), None),
        Err(e @ Error::Tangent { .. }) => (REFUSED, e.code().to_string(), Some(e.to_string())),
        Err(e) => (FAILED, format!("FAILED: {}", e.code()), Some(e.to_string())),
    };
    let _ = write!(out, ",\"status\":{status},\"status_name\":");
    json::string(&mut out, &name);
    if let Some(detail) = detail {
        out.push_str(",\"detail\":");
        json::string(&mut out, &detail);
    }
    out.push_str(",\"time_ms\":");
    json::num(&mut out, ms);
    out.push_str(",\"curves\":[");
    if let Ok(found) = &result {
        for (index, curve) in found.curves.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let mut points = curve.points.clone();
            if curve.closed {
                points.extend(curve.points.first());
            }
            let _ = write!(out, "{{\"type\":\"CURVE\",\"closed\":{}", curve.closed);
            let lists: [(&str, Column); 5] = [
                ("xyz", Box::new(|q| q.p.to_vec())),
                ("uv1", Box::new(|q| vec![q.a.u, q.a.v])),
                (
                    "s1_at_uv",
                    Box::new(|q| {
                        patch_a
                            .evaluate(q.a.u, q.a.v)
                            .unwrap_or([f64::NAN; 3])
                            .to_vec()
                    }),
                ),
                ("uv2", Box::new(|q| vec![q.b.u, q.b.v])),
                (
                    "s2_at_uv",
                    Box::new(|q| {
                        patch_b
                            .evaluate(q.b.u, q.b.v)
                            .unwrap_or([f64::NAN; 3])
                            .to_vec()
                    }),
                ),
            ];
            for (key, value) in lists {
                let _ = write!(out, ",\"{key}\":[");
                for (k, q) in points.iter().enumerate() {
                    if k > 0 {
                        out.push(',');
                    }
                    json::point(&mut out, &value(q));
                }
                out.push(']');
            }
            out.push('}');
        }
    }
    out.push_str("],\"points\":[");
    if let Ok(found) = &result {
        // Touch points, then the curves' vertices (crossings and meetings).
        for (index, p) in found
            .points
            .iter()
            .map(|q| q.p)
            .chain(found.vertices.iter().copied())
            .enumerate()
        {
            if index > 0 {
                out.push(',');
            }
            json::point(&mut out, &p);
        }
    }
    out.push(']');
    if let Ok(found) = &result {
        let s = found.stats;
        let _ = write!(
            out,
            ",\"stats\":{{\"piecesA\":{},\"piecesB\":{},\"candidates\":{},\"pairs\":{},\"leaves\":{},\"arcs\":{}}}",
            s.pieces_a, s.pieces_b, s.candidates, s.pairs, s.leaves, s.arcs
        );
    }
    out.push_str("}\n");
    std::fs::write(out_path, out).map_err(|e| format!("{out_path}: {e}"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(message) = run(&args) {
        eprintln!("ssi_corpus_runner: {message}");
        std::process::exit(2);
    }
}
