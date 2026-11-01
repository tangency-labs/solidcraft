//! Primitives exported with their pcurves and imported again. The exporter writes the
//! pcurves in the file's STEP parameters (ISO 10303-42: angles in radians, a cylinder's
//! or cone's v in millimetres), converted from the kernel's NURB parameters, so the
//! importer's checks accept them.

use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives as api;
use solidcore::topology::Brep;
use step_brep::{
    brep_to_model, build_with_file_pcurves, validate_for_display, BuildMode, BuildOptions,
};

/// Stitched builds only (the file's pcurves are used on the stitched build; the
/// topology-first build would hide them), with or without the file's pcurves.
fn stitched(file_pcurves: bool) -> BuildOptions {
    BuildOptions {
        file_pcurves,
        mode: BuildMode::StitchOnly,
        ..BuildOptions::default()
    }
}

/// Faces that took their file pcurves, after checking none was found off its edge and
/// that every body the plain build makes display-ready still is.
fn round_trip(brep: &Brep) -> Result<usize, String> {
    let model = brep_to_model(brep).map_err(|e| e.to_string())?;
    let text = step_export::write(&model).map_err(|e| e.to_string())?;
    let model = step_import::import(text.as_bytes(), &step_import::Limits::default())
        .map_err(|e| e.to_string())?;
    assert!(
        model.file_pcurves.diagnostics.is_empty(),
        "{:?}",
        model.file_pcurves.diagnostics
    );
    assert!(!model.file_pcurves.edges.is_empty());
    let options = stitched(true);
    let mut faces = 0;
    for body in &model.bodies {
        // Only a body the plain build makes display-ready is compared.
        let plain = build_with_file_pcurves(body, &model.file_pcurves, stitched(false));
        if !plain.is_ok_and(|(built, _)| validate_for_display(&built).is_ok()) {
            continue;
        }
        let (built, stats) = build_with_file_pcurves(body, &model.file_pcurves, options)
            .map_err(|e| e.to_string())?;
        validate_for_display(&built).map_err(|e| e.to_string())?;
        for wrong in ["edge off its pcurve", "pcurve off the edge's 3D curve"] {
            assert!(!stats.fallbacks.contains_key(wrong), "{stats:?}");
        }
        faces += stats.faces;
    }
    Ok(faces)
}

#[test]
fn exported_cylinder_pcurves_check_out_on_import() {
    let mut kernel = Kernel::default();
    let cylinder =
        api::create_cylinder(&mut kernel, Point3::new(0.0, 0.0, -2.0), 2.0, 4.0).unwrap();
    // The side face takes its pcurves; the planar caps carry none.
    assert_eq!(round_trip(&cylinder), Ok(1));
}

#[test]
fn exported_cone_torus_and_sphere_pcurves_are_on_their_edges() {
    let mut kernel = Kernel::default();
    let cone = api::create_cone(&mut kernel, Point3::new(0.0, 0.0, 0.0), 3.0, 1.0, 4.0).unwrap();
    let torus = api::create_torus(&mut kernel, Point3::new(0.0, 0.0, 0.0), 5.0, 1.5).unwrap();
    let sphere = api::create_sphere(&mut kernel, Point3::new(0.0, 0.0, 0.0), 2.0).unwrap();
    for brep in [&cone, &torus, &sphere] {
        round_trip(brep).unwrap();
    }
}
