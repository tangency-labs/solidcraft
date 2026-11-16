//! Prepare an explicitly shaded OPUS copy and physical CPU/GPU reference setups.
//! No render is started. Geometry and the original OPUS Master setup are retained.
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::{
    Session,
    doc::{Document, FeatureKind, MeshData, render_settings::RenderSetup},
};
use solidcraft_geom::prelude::*;
use std::{error::Error, fs, path::PathBuf, sync::Arc};

fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
    if condition { Ok(()) } else { Err(message.into()) }
}

const CURVED: &[&str] = &[
    "A3 - continuous sixteen facet freeform glass knot",
    "A3 - six lobe quilted aspheric witness optic",
    "Glass - smoked rounded toroid",
    "Glass - clear calibration sphere",
    "O2 - Lens 1 - Biconvex crown",
    "O2 - Lens 2 - Biconcave flint",
    "O2 - Lens 3 - Thick meniscus",
    "O2 - Lens 4 - Plano convex magnifier",
    "O2 - Lens 5 - Aspheric correction optic",
    "R2 - clear chest reactor lens",
    "R2 - near clear glass eye",
    "R2 - far clear glass eye",
    "S2 - amber solid ellipsoid",
    "S2 - cobalt fluted solid decanter",
    "R2 - thorax ivory swept breastplates",
    "S2 - glass gallery turned wood pedestal -217",
    "S2 - glass gallery turned wood pedestal 211",
];

fn scene_counts(scene: &Value) -> Result<(usize, u64), Box<dyn Error>> {
    let bodies = scene["bodies"].as_array().ok_or("missing scene bodies")?;
    let triangles = bodies
        .iter()
        .try_fold(0u64, |sum, body| sum.checked_add(body["triangles"].as_u64().ok_or("missing triangle count")?).ok_or("triangle sum overflow"))?;
    Ok((bodies.len(), triangles))
}

fn physical(master: &RenderSetup, cpu: bool, pilot: bool) -> Result<Value, Box<dyn Error>> {
    let mut setup = serde_json::to_value(master)?;
    let mut optics = setup["optics"].as_array().ok_or("missing optics")?.clone();
    for binding in &mut optics {
        binding["optics"]["shadow_transmission"] = json!(false);
    }
    setup["optics"] = json!(optics);
    setup["mode"] = json!("final");
    setup["width"] = json!(if pilot { 320 } else { 3840 });
    setup["height"] = json!(if pilot { 180 } else { 2160 });
    let samples = if pilot { 8u32 } else { 4096u32 };
    let mut options = setup["job"]["options"].clone();
    options["texture_filtering"] = json!(true);
    options["linear_exr"] = json!(true);
    options["max_depth"] = json!(64);
    options["samples"] = json!(samples);
    options["nested_media"] = json!(true);
    options["denoise"] = json!(false);
    if cpu {
        let lighting = &options["lighting"];
        let area_lights = lighting["area_lights"]
            .as_array()
            .ok_or("missing area lights")?
            .iter()
            .map(|a| {
                let radiance: [f64; 3] = serde_json::from_value(a["radiance"].clone())?;
                let intensity = radiance.into_iter().fold(0.0, f64::max);
                let color = radiance.map(|v| if intensity > 0.0 { v / intensity } else { 0.0 });
                // GPU stores half-edges; the CPU rectangle contract takes full sides.
                let u: [f64; 3] = serde_json::from_value(a["u"].clone())?;
                let v: [f64; 3] = serde_json::from_value(a["v"].clone())?;
                Ok(json!({"center_mm":a["center"],"edge_u_mm":u.map(|x|x*2.0),"edge_v_mm":v.map(|x|x*2.0),"color":color,"intensity":intensity}))
            })
            .collect::<Result<Vec<Value>, serde_json::Error>>()?;
        let cpu_options = json!({
            "budget":"workstation", "texture_filtering":true,
            "progressive":{"samples_per_pixel":samples,"seed":4619},
            "path":{"max_depth":64}, "primary_samples":1, "shadows":true,
            "lights":[], "area_lights":area_lights,"area_samples":1,
            "punctual_lights":lighting["punctual_lights"],
            "environment":{"color":lighting["environment"],"intensity":1.0},
            "aging":options["aging"], "denoise":false,
            "output":{"tone_map":options["tone_map"],"linear_exr":true}
        });
        setup["job"] = json!({"method":"cpu_raytrace","transparent_background":false,"options":cpu_options});
    } else {
        setup["job"]["options"] = options;
    }
    let checked: RenderSetup = serde_json::from_value(setup.clone())?;
    checked.validate()?;
    require(checked.camera == master.camera, "reference camera differs from Master")?;
    require(checked.surfaces == master.surfaces && checked.coats == master.coats, "reference lost surface maps or coats")?;
    Ok(setup)
}

fn crease_audit(mesh: &MeshData) -> Result<Value, Box<dyn Error>> {
    let positions: Vec<Vec3> = mesh.positions.as_chunks::<3>().0.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect();
    let triangles = mesh.triangles.as_chunks::<3>().0;
    let mut weld = std::collections::HashMap::new();
    let mut canonical = Vec::with_capacity(positions.len());
    for p in &positions {
        let next = weld.len();
        canonical.push(*weld.entry([(p.x + 0.0).to_bits(), (p.y + 0.0).to_bits(), (p.z + 0.0).to_bits()]).or_insert(next));
    }
    let mut edges = std::collections::HashMap::<(usize, usize), (usize, Option<usize>, usize)>::new();
    let mut face_normals = Vec::with_capacity(triangles.len());
    for (i, t) in triangles.iter().enumerate() {
        let [a, b, c] = t.map(|k| positions.get(k as usize).copied());
        let (Some(a), Some(b), Some(c)) = (a, b, c) else { return Err("audit index missing".into()) };
        face_normals.push((b - a).cross(c - a).normalized().ok_or("audit triangle degenerate")?);
        for k in 0..3 {
            let (a, b) = (canonical[t[k] as usize], canonical[t[(k + 1) % 3] as usize]);
            edges
                .entry((a.min(b), a.max(b)))
                .and_modify(|e| {
                    e.1 = Some(i * 3 + k);
                    e.2 += 1;
                })
                .or_insert((i * 3 + k, None, 1));
        }
    }
    let normal = |corner: usize| -> Result<Vec3, Box<dyn Error>> {
        let n = mesh.corner_normals.get(corner * 3..corner * 3 + 3).ok_or("audit normal missing")?;
        Ok(Vec3::new(n[0], n[1], n[2]))
    };
    let mut bins = [0u64; 7];
    let mut sharp = 0u64;
    let mut sharp_discontinuous = 0u64;
    let mut boundary_or_nonmanifold = 0u64;
    for (_, (a, b, count)) in edges {
        let Some(b) = b.filter(|_| count == 2) else {
            boundary_or_nonmanifold += 1;
            continue;
        };
        let angle = face_normals[a / 3].dot(face_normals[b / 3]).clamp(-1.0, 1.0).acos().to_degrees();
        let bin = [1.0, 5.0, 10.0, 22.5, 45.0, 90.0].iter().position(|&max| angle <= max).unwrap_or(6);
        bins[bin] += 1;
        if angle > 10.0 + 1e-9 {
            sharp += 1;
            let (ak, bk) = (a % 3, b % 3);
            let same_a = normal(a)?.dist(normal((b / 3) * 3 + (bk + 1) % 3)?) <= 1e-8;
            let same_b = normal((a / 3) * 3 + (ak + 1) % 3)?.dist(normal(b)?) <= 1e-8;
            if !same_a || !same_b {
                sharp_discontinuous += 1;
            }
        }
    }
    Ok(json!({"manifold_edge_angle_bins_degrees":[[0,1],[1,5],[5,10],[10,22.5],[22.5,45],[45,90],[90,180]],
        "manifold_edge_counts":bins,"geometric_sharp_edges_above_10_degrees_before":sharp,
        "geometric_sharp_edges_above_10_degrees_after":sharp,"sharp_edges_with_shading_discontinuity":sharp_discontinuous,
        "boundary_or_nonmanifold_edges":boundary_or_nonmanifold}))
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let baseline = PathBuf::from(args.next().ok_or("usage: opus_quality_fixture BASELINE.sc NEW_OUTPUT_DIRECTORY")?);
    let output = PathBuf::from(args.next().ok_or("missing new output directory")?);
    require(args.next().is_none(), "unexpected argument")?;
    fs::create_dir(&output)?;
    let original = fs::read(&baseline)?;
    let mut doc: Document = solidcraft_io::read_design(&original)?;
    let master = doc.render.setups.get("OPUS Master").cloned().ok_or("missing OPUS Master")?;
    require(master.surfaces.len() == 104 && master.optics.len() == 30 && master.coats.len() == 6, "fixture bindings differ from OPUS baseline")?;
    require(doc.components.len() == 82 && doc.assembly.joints.len() == 81 && doc.assembly.links.len() == 15, "fixture assembly counts differ")?;
    let baseline_doc = doc.clone();
    let mut baseline_session = Session::new(doc.clone());
    let baseline_scene = baseline_session.execute("render.scene", &json!({}))?;
    require(scene_counts(&baseline_scene)? == (511, 729874), "baseline scene geometry counts differ")?;
    let mut edits = Vec::new();
    for feature in &mut doc.features {
        if let FeatureKind::MeshImport { meshes, .. } = &mut feature.kind {
            for mesh in meshes {
                if CURVED.contains(&mesh.name.as_str()) {
                    let positions = mesh.positions.clone();
                    let triangles = mesh.triangles.clone();
                    mesh.generate_crease_normals(10.0)?;
                    require(
                        Arc::ptr_eq(&positions, &mesh.positions) && Arc::ptr_eq(&triangles, &mesh.triangles),
                        "normal generation changed geometry arrays",
                    )?;
                    edits.push(json!({"feature":feature.id,"body":mesh.name,"crease_angle_degrees":10.0,"triangles":mesh.triangles.len()/3,"normal_values":mesh.corner_normals.len(),"crease_audit":crease_audit(mesh)?}));
                }
            }
        }
    }
    require(edits.len() == CURVED.len(), "a curated normal target is missing or duplicated")?;
    doc.name = format!("{} — curated 10 degree shading reference", doc.name);
    let mut session = Session::new(doc);
    let mut setup_receipts = Vec::new();
    for (cpu, pilot, name) in [
        (true, false, "OPUS Physical CPU 4K"),
        (false, false, "OPUS Physical GPU 4K"),
        (true, true, "OPUS Physical CPU Pilot"),
        (false, true, "OPUS Physical GPU Pilot"),
    ] {
        let setup = physical(&master, cpu, pilot)?;
        setup_receipts.push(session.execute("render.settings.set", &json!({"name":name,"set":setup}))?);
    }
    require(session.doc.render.setups.get("OPUS Master") == Some(&master), "original Master setup changed")?;
    let scene = session.execute("render.scene", &json!({}))?;
    require(scene == baseline_scene, "shading changed captured geometry, names, bounds or stable targets")?;
    require(
        session.doc.components == baseline_doc.components
            && session.doc.occurrences == baseline_doc.occurrences
            && session.doc.assembly == baseline_doc.assembly,
        "assembly, pose or occurrence metadata changed",
    )?;
    let copy_path = output.join("OPUS-04-physical-reference.sc");
    let copy = solidcraft_io::write_design_container(&session.doc)?;
    fs::write(&copy_path, &copy)?;
    let reopened = solidcraft_io::read_design(&copy)?;
    require(reopened == *session.doc, "copy changed on cold reopen")?;
    fs::write(output.join("baseline-scene.json"), serde_json::to_vec_pretty(&baseline_scene)?)?;
    fs::write(output.join("reference-setups.json"), serde_json::to_vec_pretty(&setup_receipts)?)?;
    let report = json!({
        "baseline":baseline,"reference":copy_path,"baseline_bytes":original.len(),"reference_bytes":copy.len(),
        "geometry":{"bodies":511,"triangles":729874,"unchanged":true},
        "assembly":{"components":82,"joints":81,"couplings":15,"unchanged":true},
        "bindings_per_reference":{"surfaces":104,"optics":30,"coats":6,"aging":7,"area_lights":4,"punctual_lights":7},
        "curated_normal_edits":edits,"master_unchanged":true,"camera_unchanged":true,
        "assembly_and_saved_pose_unchanged":true,"recorded_motion_step15_applied":false,
        "reference_shadow_transmission":false,"render_started":false,
        "qualification":"fixture preparation and public settings/scene commands only; no transport convergence claim"
    });
    fs::write(output.join("fixture-manifest.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
