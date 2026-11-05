//! Original public-command wood fixture and cold offline CPU render acceptance.
use serde_json::json;
use solidcraft_engine::{
    Session,
    doc::Document,
    render::job::{CpuHost, Phase},
};
use std::{error::Error, fs, path::Path, time::Duration};

fn render(document: Document, output: &Path) -> Result<(), Box<dyn Error>> {
    let mut session = Session::new(document);
    session.set_render_job_host(Some(CpuHost::for_platform()?))?;
    session.execute("render.scene", &json!({}))?;
    let preflight = session.execute("render.preflight", &json!({"setup":"Wood"}))?;
    let started = session.execute("render.start", &json!({"setup":"Wood"}))?;
    let id = serde_json::from_value(started["job"].clone())?;
    let handle = session.render_job_handle(id)?;
    let status = handle.wait_timeout(Duration::from_secs(120))?;
    if status.progress.phase != Phase::Completed {
        return Err(format!("wood rendering failed: {:?}", status.error).into());
    }
    let frame = handle.frame()?.ok_or("wood frame missing")?;
    fs::write(output, frame.png())?;
    println!("{}", serde_json::to_string_pretty(&json!({"preflight":preflight,"frame":frame.info,"textures":status.textures}))?);
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 3 && args.first().is_some_and(|value| value == "--reopen") {
        let bytes = fs::read(Path::new(&args[1]))?;
        return render(solidcraft_io::read_design(&bytes)?, Path::new(&args[2]));
    }
    let output = args.first().map(Path::new).ok_or("usage: wood_texture_workflow NEW_OUTPUT_DIRECTORY [TEXTURE_DIRECTORY] | --reopen DESIGN PNG")?;
    let source = args.get(1).map(Path::new);
    let photographic = source.is_some();
    let dimensions = if photographic { [900.0, 300.0, 25.0] } else { [220.0, 100.0, 12.0] };
    let provenance = if let Some(source) = source {
        serde_json::from_slice::<serde_json::Value>(&fs::read(source.join("provenance.json"))?)?
    } else {
        json!({"author":"SolidCraft contributors","source":"Original SURF-027 procedural wood illustration; repository ATTRIBUTION.md","license":"MIT OR Apache-2.0"})
    };
    fs::create_dir(output)?;
    let mut session = Session::default();
    session.execute("solid.box", &json!({"length":dimensions[0],"width":dimensions[1],"height":dimensions[2],"body_name":"Board"}))?;
    let physical = (session.doc.features.clone(), session.doc.materials.clone());
    let map = output.join("source-only-before-save.png");
    let read = |name: &str, fallback: &[u8]| -> Result<Vec<u8>, Box<dyn Error>> {
        Ok(if let Some(source) = source { fs::read(source.join(name))? } else { fallback.to_vec() })
    };
    fs::write(&map, read("base_color.png", include_bytes!("../../../assets/render/starter/materials/exports/SURF-027/base_color.png"))?)?;
    let normal = output.join("normal-only-before-save.png");
    fs::write(&normal, read("normal_open_gl.png", include_bytes!("../../../assets/render/starter/materials/exports/SURF-027/normal_open_gl.png"))?)?;
    let roughness = output.join("roughness-only-before-save.png");
    fs::write(&roughness, read("roughness.png", include_bytes!("../../../assets/render/starter/materials/exports/SURF-027/roughness.png"))?)?;
    let frame = if photographic {
        json!({"origin":[0,300,25],"u":[0,-1500,0],"v":[1500,0,0]})
    } else {
        json!({"origin":[0,0,12],"u":[60,0,0],"v":[0,120,0]})
    };
    session.execute("render.surface.import", &json!({
        "name":"Wood","body_index":0,"body_name":"Board",
        "set":{"width":960,"height":540,"camera":{"target":[dimensions[0]*0.5,dimensions[1]*0.5,dimensions[2]*0.5],"yaw":0.35,"pitch":1.05,"distance":if photographic {1500.0} else {420.0},"fov":0.6},
            "job":{"method":"cpu_raytrace","options":{"budget":"studio","primary_samples":4}}},
        "frame":frame,
        "maps":{"base_color_srgb":{"path":map},"normal_opengl":{"path":normal},"roughness_linear":{"path":roughness}},
        "provenance":provenance,
        "flip_v":true,"normal_strength":0.4
    }))?;
    if (session.doc.features.clone(), session.doc.materials.clone()) != physical {
        return Err("surface import changed CAD geometry or physical materials".into());
    }
    let json = session.doc.to_json();
    fs::write(output.join("wood.json"), &json)?;
    fs::write(output.join("wood.sc"), solidcraft_io::write_design_container(&session.doc)?)?;
    fs::remove_file(map)?;
    fs::remove_file(normal)?;
    fs::remove_file(roughness)?;
    render(Document::from_json(&json)?, &output.join("wood.png"))?;
    Ok(())
}
