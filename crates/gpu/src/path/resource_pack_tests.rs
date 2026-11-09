//! Actual GPU production smoke coverage of every authored starter surface and
//! native composition. Not a visual certification or all-pairs material matrix.
use super::*;
use serde_json::json;
use solidcraft_render::job::{JobId, Phase};

#[test]
fn all_authored_surfaces_and_native_studios_render_from_saved_exact_bindings() {
    let Some(executor) = executor() else { return };
    let host = ImageHost::with_executor(Arc::new(executor)).unwrap();
    let output = std::env::var_os("SOLIDCRAFT_RESOURCE_EVIDENCE_DIR").map(std::path::PathBuf::from);
    if let Some(folder) = &output {
        assert!(folder.is_absolute() && !folder.exists(), "fresh evidence directory required");
        std::fs::create_dir_all(folder).unwrap();
    }
    let mut receipts = Vec::new();
    for (kind, count) in [("surface", 48), ("studio", 25)] {
        for index in 1..=count {
            let mut session = solidcraft_engine::Session::default();
            session.register_render_job_host(host.clone()).unwrap();
            session.execute("solid.box", &json!({"length":20,"width":15,"height":4,"body_name":"Original coupon"})).unwrap();
            let geometry = session.doc.features.clone();
            let materials = session.doc.materials.clone();
            let camera = session.execute("render.camera.fit", &json!({})).unwrap()["camera"].clone();
            session
                .execute(
                    "render.settings.set",
                    &json!({"name":"Smoke","set":{"width":48,"height":32,"camera":camera,
                "job":{"method":"gpu_path","options":{"samples":4,"max_depth":3,"seed":20261023}}}}),
                )
                .unwrap();
            let studio = if kind == "studio" { format!("NATIVE-STUDIO-{index:03}") } else { "NATIVE-STUDIO-001".into() };
            session
                .execute(
                    "render.preset.apply",
                    &json!({"name":"Smoke","id":studio,"expected_revision":session.revision,
                "replace_lighting_and_environment":true,"replace_stage":true}),
                )
                .unwrap();
            let identity = if kind == "surface" {
                let surface = format!("SURF-{index:03}");
                session
                    .execute(
                        "render.library.surface.bind",
                        &json!({"name":"Smoke","surface":{"id":surface,"body_index":0,"body_name":"Original coupon"}}),
                    )
                    .unwrap();
                surface
            } else {
                studio
            };
            let reopened = solidcraft_engine::doc::Document::from_json(&session.doc.to_json()).unwrap();
            assert_eq!(reopened.render.setups["Smoke"], session.doc.render.setups["Smoke"]);
            assert_eq!(session.doc.features, geometry);
            assert_eq!(session.doc.materials, materials);
            let check = session.execute("render.preflight", &json!({"setup":"Smoke"})).unwrap();
            assert_eq!(check["resolved"]["executor"], "gpu-path");
            let start = session.execute("render.start", &json!({"setup":"Smoke"})).unwrap();
            let id: JobId = serde_json::from_value(start["job"].clone()).unwrap();
            let handle = session.render_job_handle(id).unwrap();
            let status = handle.wait_timeout(Duration::from_secs(60)).unwrap();
            assert_eq!(status.progress.phase, Phase::Completed, "{identity}: {:?}", status.error);
            if kind == "surface" {
                assert!(status.textures.is_some(), "{identity}: map capture must reach execution");
            }
            let frame = handle.frame().unwrap().unwrap();
            let image = image::load_from_memory(frame.png()).unwrap().to_rgba8();
            assert_eq!(image.dimensions(), (48, 32));
            assert!(image.pixels().all(|pixel| pixel.0[3] == 255));
            assert!(image.pixels().any(|pixel| pixel.0 != image.get_pixel(0, 0).0), "{identity}: empty presentation");
            let receipt = json!({"resource":identity,"kind":kind,"status":"PASS","frame":frame.info,"textures":status.textures,
                "portable_setup_reopened":true,"engineering_geometry_and_materials_preserved":true,"samples":4,"width":48,"height":32});
            if let Some(folder) = &output {
                std::fs::write(folder.join(format!("{identity}.png")), frame.png()).unwrap();
                std::fs::write(folder.join(format!("{identity}.json")), serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
            }
            receipts.push(receipt);
        }
    }
    assert_eq!(receipts.len(), 73);
    eprintln!("authored resource GPU smoke: 48 surfaces and 25 native compositions, 73 completed images");
    if let Some(folder) = output {
        std::fs::write(folder.join("summary.json"), serde_json::to_vec_pretty(&json!({"status":"PASS","surfaces":48,"studios":25,
            "scope":"individual low-resolution GPU execution, exact portable bindings and engineering preservation; not all pairs or final visual certification", "receipts":receipts})).unwrap()).unwrap();
    }
}
