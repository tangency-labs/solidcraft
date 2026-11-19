//! SDF model export (docs/sdf.md): the folder, its files and where the meshes put the body and
//! the decal, against values worked out by hand.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// A fresh folder for one test's files.
fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("solidcraft-sdf-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap_or_else(|e| panic!("{e}"));
    d
}

fn files_in(d: &Path) -> Vec<String> {
    let mut files: Vec<String> = std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    files.sort();
    files
}

/// The smallest and largest corner of a binary STL's triangles.
fn stl_bounds(bytes: &[u8]) -> ([f32; 3], [f32; 3]) {
    let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
    assert_eq!(bytes.len(), 84 + n * 50, "binary STL");
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for t in 0..n {
        for v in 0..3 {
            for k in 0..3 {
                let at = 84 + t * 50 + 12 + v * 12 + k * 4;
                let x = f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
                lo[k] = lo[k].min(x);
                hi[k] = hi[k].max(x);
            }
        }
    }
    (lo, hi)
}

/// A 40 x 30 x 10 mm box with an 8 x 4 mm decal centred on its top face: the folder holds the
/// box's STL in metres, the decal's image and its quad 0.2 mm above and below the face, and
/// `model.config` and `model.sdf` naming them. An image placed as a canvas is written the same
/// way; a hidden one is not.
#[test]
fn a_model_folder_holds_the_body_the_decal_and_the_files_naming_them() {
    use image::ImageEncoder;
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "corner": [0, 0, 0]}));
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png).write_image(&[200u8; 4 * 8], 4, 2, image::ExtendedColorType::Rgba8).unwrap();
    let data = solidcraft_doc::canvas::base64_encode(&png);
    run(&mut s, "canvas.decal", json!({"data": data, "face": [20, 15, 10], "width": 8, "name": "Aruco"}));
    // An image placed as a canvas is a texture of the model too; a hidden one is not.
    run(&mut s, "canvas.insert", json!({"data": data, "plane": "XY", "width": 8, "opacity": 1, "name": "Tag"}));
    run(&mut s, "canvas.insert", json!({"data": data, "plane": "XY", "width": 8, "name": "Hidden"}));
    run(&mut s, "canvas.edit", json!({"canvas": "Hidden", "visible": false}));
    let body = s.model.state().bodies[0].name.clone();
    let d = temp("box");
    // The save picker's `.sdf` is dropped: the model is the folder `Panel`.
    let r = run(&mut s, "file.export", json!({"path": d.join("Panel.sdf").to_string_lossy()}));
    let folder = d.join("Panel");
    assert_eq!(files_in(&d), ["Panel"], "{r}");
    let stl = format!("{}.stl", body.replace(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-', "_"));
    let mut want =
        vec!["Aruco.dae".to_string(), "Aruco.png".into(), "Tag.dae".into(), "Tag.png".into(), stl.clone(), "model.config".into(), "model.sdf".into()];
    want.sort();
    assert_eq!(files_in(&folder), want, "{r}");
    assert_eq!(
        (r["format"].as_str(), r["name"].as_str(), r["bodies"].as_u64(), r["decals"].as_u64()),
        (Some("Sdf"), Some("Panel"), Some(1), Some(2)),
        "{r}"
    );
    assert!(std::fs::read_to_string(folder.join("model.sdf")).unwrap().contains("<albedo_map>Tag.png</albedo_map>"), "the canvas's texture");
    assert!(r["files"].as_array().unwrap().last().unwrap().as_str().unwrap().ends_with("model.sdf"), "model.sdf last: {r}");
    assert_eq!(std::fs::read(folder.join("Aruco.png")).unwrap(), png, "the image as it is");

    // The box in metres.
    let (lo, hi) = stl_bounds(&std::fs::read(folder.join(&stl)).unwrap());
    for (got, want) in lo.iter().chain(&hi).zip([0.0, 0.0, 0.0, 0.04, 0.03, 0.01]) {
        assert!((got - want).abs() < 1e-6, "{lo:?} {hi:?}");
    }

    let config = std::fs::read_to_string(folder.join("model.config")).unwrap();
    assert!(config.contains("<name>Panel</name>") && config.contains("<sdf version=\"1.9\">model.sdf</sdf>"), "{config}");
    let sdf = std::fs::read_to_string(folder.join("model.sdf")).unwrap();
    for part in [
        "<model name=\"Panel\">",
        "<static>true</static>",
        &format!("<geometry><mesh><uri>{stl}</uri></mesh></geometry>"),
        "<uri>Aruco.dae</uri>",
        "<albedo_map>Aruco.png</albedo_map>",
    ] {
        assert!(sdf.contains(part), "no {part}: {sdf}");
    }
    assert_eq!(sdf.matches("<collision ").count(), 1, "the closed box collides, the decal does not: {sdf}");

    // The decal's front quad: 8 x 4 mm about (20, 15), along the face's axes either way round,
    // 0.2 mm above the top face; the back quad 0.2 mm below it.
    let dae = std::fs::read_to_string(folder.join("Aruco.dae")).unwrap();
    assert!(dae.contains("<init_from>Aruco.png</init_from>"), "{dae}");
    let positions: Vec<f64> = dae
        .split(r#"<float_array id="decal-positions-array" count="24">"#)
        .nth(1)
        .and_then(|x| x.split('<').next())
        .map(|x| x.split_whitespace().filter_map(|v| v.parse().ok()).collect())
        .unwrap_or_default();
    assert_eq!(positions.len(), 24);
    let corners: Vec<[f64; 3]> = positions.chunks(3).map(|p| [p[0] * 1e3, p[1] * 1e3, p[2] * 1e3]).collect();
    let mut front: Vec<(i64, i64)> = corners[..4].iter().map(|p| ((p[0] * 1e3).round() as i64, (p[1] * 1e3).round() as i64)).collect();
    front.sort();
    let along_x = [(16000, 13000), (16000, 17000), (24000, 13000), (24000, 17000)];
    let along_y = [(18000, 11000), (18000, 19000), (22000, 11000), (22000, 19000)];
    assert!(front == along_x || front == along_y, "{corners:?}");
    assert!(corners[..4].iter().all(|p| (p[2] - 10.2).abs() < 1e-9) && corners[4..].iter().all(|p| (p[2] - 9.8).abs() < 1e-9), "{corners:?}");

    // Again into the same folder, named through the model path, and not static.
    let r = run(&mut s, "file.export_sdf", json!({"path": folder.to_string_lossy(), "model_uris": true, "static": false}));
    let sdf = std::fs::read_to_string(folder.join("model.sdf")).unwrap();
    assert!(sdf.contains("<albedo_map>model://Panel/Aruco.png</albedo_map>") && sdf.contains("<uri>model://Panel/Aruco.dae</uri>"), "{sdf}");
    // Not static: a mass, its centre in the middle of the box, (20, 15, 5) mm.
    assert!(sdf.contains("<static>false</static>") && sdf.contains("<mass>"), "{r}: {sdf}");
    let pose: Vec<f64> = sdf
        .split("<inertial>")
        .nth(1)
        .and_then(|x| x.split("<pose>").nth(1))
        .and_then(|x| x.split('<').next())
        .map(|x| x.split_whitespace().filter_map(|v| v.parse().ok()).collect())
        .unwrap_or_default();
    assert_eq!(pose.len(), 6, "{sdf}");
    for (got, want) in pose.iter().zip([0.02, 0.015, 0.005, 0.0, 0.0, 0.0]) {
        assert!((got - want).abs() < 1e-9, "{pose:?}");
    }
    assert_eq!(files_in(&folder), want, "the same files, replaced");

    // A file where the folder goes is refused, and nothing is written.
    let file = d.join("Taken");
    std::fs::write(&file, b"x").unwrap();
    let e = s.execute("file.export_sdf", &json!({"path": file.to_string_lossy()})).unwrap_err().to_string();
    assert!(e.contains("folder"), "{e}");
    assert_eq!(std::fs::read(&file).unwrap(), b"x");
    let _ = std::fs::remove_dir_all(&d);
}
