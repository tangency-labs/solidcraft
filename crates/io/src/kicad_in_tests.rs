use super::*;

const BOARD: &str = r#"(kicad_pcb
  (version 20250101)
  (general (thickness 1.6))
  (gr_line (start 0 0) (end 20 0) (layer "Edge.Cuts"))
  (gr_line (start 20 10) (end 0 10) (layer "Edge.Cuts"))
  (gr_line (start 20 0) (end 20 10) (layer "Edge.Cuts"))
  (gr_line (start 0 10) (end 0 0) (layer "Edge.Cuts"))
  (footprint "Device:R_0603" (layer "F.Cu") (at 10.25 20.5 90)
    (property "Reference" "R7")
    (property "Value" "10k")
    (model "${KICAD9_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603.step"
      (offset (xyz 0 0 0.1))
      (scale (xyz 1 1 1))
      (rotate (xyz 0 0 90))
    )
  )
)"#;

#[test]
fn parses_thickness_footprint_properties_and_model_transform() {
    let board = parse_kicad_board(BOARD.as_bytes(), "test.kicad_pcb").unwrap();
    assert_eq!(board.name, "test");
    assert_eq!(board.thickness, 1.6);
    let [footprint] = board.footprints.as_slice() else { panic!("expected one footprint") };
    assert_eq!(footprint.reference, "R7");
    assert_eq!(footprint.value, "10k");
    assert_eq!(footprint.layer, "F.Cu");
    assert_eq!(footprint.at, [10.25, 20.5, 90.0]);
    let [model] = footprint.models.as_slice() else { panic!("expected one model") };
    assert_eq!(model.path, "${KICAD9_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603.step");
    assert_eq!(model.at, [0.0, 0.0, 0.1]);
    assert_eq!(model.scale, [1.0; 3]);
    assert_eq!(model.rotate, [0.0, 0.0, 90.0]);
    assert_eq!(board.outlines.len(), 1);
    assert_eq!(board.outlines[0].len(), 4);
}

#[test]
fn footprint_rotation_maps_from_kicads_y_down_frame() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (footprint "test:rotated" (layer "F.Cu") (at 10 20 90)))"#;
    let board = parse_kicad_board(source.as_bytes(), "rotation.kicad_pcb").unwrap();
    let footprint = board.footprints.first().expect("footprint");
    let point = solidcraft_doc::apply_point(&footprint_place(footprint, false, 1.6), Vec3::new(1.0, 0.0, 0.0));
    // KiCad turns a footprint at +90 a quarter turn counterclockwise, so its own +X points up
    // the board: board Y 20 goes to 19, which is world Y -19 once KiCad's Y down is flipped.
    assert!((point.x - 10.0).abs() < 1e-9, "{point:?}");
    assert!((point.y + 19.0).abs() < 1e-9, "{point:?}");
}

#[test]
fn models_stand_on_the_board_face_they_are_on() {
    let footprint = |layer: &str| KicadFootprint {
        reference: String::new(),
        value: String::new(),
        layer: layer.into(),
        at: [10.0, 20.0, 90.0],
        models: Vec::new(),
        pads: Vec::new(),
        graphics: Vec::new(),
        texts: Vec::new(),
    };
    let front = solidcraft_doc::apply_point(&footprint_place(&footprint("F.Cu"), false, 1.6), Vec3::ZERO);
    let back = solidcraft_doc::apply_point(&footprint_place(&footprint("B.Cu"), true, 1.6), Vec3::ZERO);
    assert!((front.z - 1.6).abs() < 1e-9, "a front model stands on the top face: {front:?}");
    assert!(back.z.abs() < 1e-9, "a back model starts at the bottom face: {back:?}");
}

#[test]
fn model_offset_accepts_kicads_legacy_inches_token() {
    let source = r#"(kicad_pcb (footprint "part" (at 0 0)
      (model "part.step" (at (xyz 1 0 0)))
      (model "part2.step" (offset (xyz 1 0 0)))))"#;
    let board = parse_kicad_board(source.as_bytes(), "inches.kicad_pcb").unwrap();
    let models = &board.footprints[0].models;
    // KiCad's `at` token still carries the legacy inch offset; `offset` is millimetres.
    assert!((models[0].at[0] - 25.4).abs() < 1e-9, "{:?}", models[0].at);
    assert!((models[1].at[0] - 1.0).abs() < 1e-9, "{:?}", models[1].at);
}

#[test]
fn model_rotation_is_applied_in_its_local_frame() {
    let model = KicadModel { path: String::new(), at: [0.0; 3], scale: [1.0; 3], rotate: [0.0, 0.0, 90.0] };
    // KiCad's +90 turns the model the other way in its Y-up world, so its own +X points to -Y.
    let point = solidcraft_doc::apply_point(&model_local_matrix(&model, false), Vec3::X);
    assert!(point.x.abs() < 1e-9, "{point:?}");
    assert!((point.y + 1.0).abs() < 1e-9, "{point:?}");
}

#[test]
fn model_rotation_order_is_x_then_y_then_z() {
    let model = KicadModel { path: String::new(), at: [0.0; 3], scale: [1.0; 3], rotate: [90.0, 0.0, 90.0] };
    let m = model_local_matrix(&model, false);
    let x = solidcraft_doc::apply_vector(&m, Vec3::X);
    let z = solidcraft_doc::apply_vector(&m, Vec3::Z);
    // KiCad 10.0.1 puts this model's +X at world -Y and its +Z at world +X.
    assert!(x.x.abs() < 1e-9 && (x.y + 1.0).abs() < 1e-9 && x.z.abs() < 1e-9, "{x:?}");
    assert!((z.x - 1.0).abs() < 1e-9 && z.y.abs() < 1e-9 && z.z.abs() < 1e-9, "{z:?}");
}

#[test]
fn a_back_side_model_is_turned_over_under_the_board() {
    let model = KicadModel { path: String::new(), at: [0.0; 3], scale: [1.0; 3], rotate: [0.0; 3] };
    let m = model_local_matrix(&model, true);
    let x = solidcraft_doc::apply_vector(&m, Vec3::X);
    let z = solidcraft_doc::apply_vector(&m, Vec3::Z);
    // A half turn about X keeps +X and takes +Z down, as KiCad does for the back.
    assert!((x.x - 1.0).abs() < 1e-9 && x.y.abs() < 1e-9 && x.z.abs() < 1e-9, "{x:?}");
    assert!(z.x.abs() < 1e-9 && z.y.abs() < 1e-9 && (z.z + 1.0).abs() < 1e-9, "{z:?}");
}

#[test]
fn builds_a_closed_mesh_from_a_concave_outline() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_line (start 0 0) (end 20 0) (layer "Edge.Cuts"))
      (gr_line (start 20 0) (end 20 10) (layer "Edge.Cuts"))
      (gr_line (start 20 10) (end 10 10) (layer "Edge.Cuts"))
      (gr_line (start 10 10) (end 10 20) (layer "Edge.Cuts"))
      (gr_line (start 10 20) (end 0 20) (layer "Edge.Cuts"))
      (gr_line (start 0 20) (end 0 0) (layer "Edge.Cuts")))"#;
    let board = parse_kicad_board(source.as_bytes(), "board.kicad_pcb").unwrap();
    let mesh = kicad_board_mesh(&board, board.thickness).unwrap();
    assert!(!mesh.positions.is_empty());
    assert!(mesh.triangles.iter().flatten().all(|index| (*index as usize) < mesh.positions.len()));
    let body = solidcraft_kernel::mesh_body(&mesh.positions, &mesh.triangles).unwrap();
    let measure = solidcraft_kernel::measure(&body).unwrap();
    assert!((measure.volume - 300.0 * 1.6).abs() < 1.0, "{measure:?}");
    assert!((measure.bbox.min.y + 20.0).abs() < 1e-9, "KiCad's Y is mapped to -Y: {measure:?}");
}

#[test]
fn curved_outlines_and_circular_cutouts_make_an_annular_board() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_circle (center 0 0) (end 10 0) (layer "Edge.Cuts"))
      (gr_circle (center 0 0) (end 3 0) (layer "Edge.Cuts")))"#;
    let board = parse_kicad_board(source.as_bytes(), "ring.kicad_pcb").unwrap();
    assert_eq!(board.outlines.len(), 2);
    let mesh = kicad_board_mesh(&board, 1.6).unwrap();
    let body = solidcraft_kernel::mesh_body(&mesh.positions, &mesh.triangles).unwrap();
    let measure = solidcraft_kernel::measure(&body).unwrap();
    let expected = std::f64::consts::PI * (100.0 - 9.0) * 1.6;
    assert!((measure.volume - expected).abs() < expected * 0.01, "{measure:?}");
}

#[test]
fn drilled_pads_cut_holes_in_the_substrate() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (footprint "hole" (at 10 5) (layer "F.Cu")
        (pad "1" thru_hole circle (at 0 0) (size 2 2) (drill 1) (layers "*.Cu" "*.Mask"))))"#;
    let board = parse_kicad_board(source.as_bytes(), "holes.kicad_pcb").unwrap();
    assert_eq!(board.drills.len(), 1);
    assert_eq!(board.drills[0].center, [10.0, -5.0]);
    let mesh = kicad_board_mesh(&board, 1.6).unwrap();
    let body = solidcraft_kernel::mesh_body(&mesh.positions, &mesh.triangles).unwrap();
    let measure = solidcraft_kernel::measure(&body).unwrap();
    let expected = (200.0 - std::f64::consts::PI * 0.25) * 1.6;
    assert!((measure.volume - expected).abs() < expected * 0.01, "{measure:?}");
}

#[test]
fn missing_model_is_warned_and_the_board_is_kept() {
    let mut load = |_: &str| Err("model file was not found".to_string());
    let imported = read_kicad(BOARD.as_bytes(), "test.kicad_pcb", &KicadOptions::default(), &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    assert_eq!(meshes.len(), 1, "only the board substrate should remain");
    assert!(imported.plan.places.is_empty());
    assert!(imported.feature.warnings.iter().any(|warning| warning.contains("model file was not found")));
}

#[test]
fn skips_footprints_with_invalid_models_and_keeps_other_parts() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (footprint "bad" (at 1 2)
        (model "bad.step" (scale (xyz 0 1 1)))
        (model "good.step"))
      (footprint "good" (at 3 4 0) (property "Reference" "U1")))"#;
    let board = parse_kicad_board(source.as_bytes(), "board.kicad_pcb").unwrap();
    assert_eq!(board.footprints.len(), 2);
    assert_eq!(board.footprints[0].models.len(), 1);
    assert_eq!(board.footprints[0].models[0].path, "good.step");
    assert_eq!(board.footprints[1].reference, "U1");
    assert_eq!(board.warnings.len(), 1);
}

#[test]
fn rejects_malformed_and_wrong_root_documents() {
    assert!(parse_kicad_board(b"(kicad_pcb (general", "bad.kicad_pcb").is_err());
    assert!(parse_kicad_board(b"(kicad_pro)", "bad.kicad_pcb").is_err());
}

#[test]
fn handles_escaped_model_paths_and_kicad_extensions_case_insensitively() {
    let source = r#"(kicad_pcb
      (footprint "part" (at 0 0)
        (model "${KIPRJMOD}/models/a\\b\"c.step")))"#;
    let board = parse_kicad_board(source.as_bytes(), "board.kicad_pcb").unwrap();
    assert_eq!(board.footprints[0].models[0].path, "${KIPRJMOD}/models/a\\b\"c.step");
    assert!(is_kicad_path("demo.KICAD_PRO"));
    assert!(is_kicad_path("demo.KICAD_PCB"));
    assert!(!is_kicad_path("demo.step"));
}

#[test]
fn resolves_project_to_same_stem_board_and_keeps_direct_board() {
    assert_eq!(kicad_board_path(Path::new("folder/demo.kicad_pro")).unwrap(), Path::new("folder/demo.kicad_pcb"));
    assert_eq!(kicad_board_path(Path::new("folder/demo.kicad_pcb")).unwrap(), Path::new("folder/demo.kicad_pcb"));
    assert!(kicad_board_path(Path::new("folder/demo.step")).is_err());
}

#[test]
fn rejects_non_finite_footprint_placement() {
    let source = b"(kicad_pcb (footprint \"bad\" (at NaN 1)))";
    let board = parse_kicad_board(source, "board.kicad_pcb").unwrap();
    assert!(board.footprints.is_empty());
    assert_eq!(board.warnings.len(), 1);
}

#[test]
fn import_options_build_copper_silk_and_a_colour_and_keep_the_plan_flat() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (gr_line (start 1 1) (end 10 1) (layer "F.SilkS") (width 0.2))
      (segment (start 1 2) (end 10 2) (width 0.3) (layer "F.Cu") (net 1))
      (footprint "part" (at 5 5) (layer "F.Cu")
        (pad "1" smd rect (at 0 0) (size 1 2) (layers "F.Cu" "F.Paste"))))"#;
    let options =
        KicadOptions { copper: KicadCopper::Mesh, silkscreen: true, components: false, color: Some([0x11, 0x22, 0x33]), ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "board.kicad_pcb", &options, &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    let names: Vec<&str> = meshes.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"board"), "{names:?}");
    assert!(names.contains(&"Copper F.Cu"), "{names:?}");
    assert!(names.contains(&"Silkscreen F.SilkS"), "{names:?}");
    let board = meshes.iter().find(|m| m.name == "board").unwrap();
    assert_eq!(board.color, Some([0x11u8 as f32 / 255.0, 0x22u8 as f32 / 255.0, 0x33u8 as f32 / 255.0]));
    assert!(imported.plan.places.is_empty());
    assert!(imported.plan.slots.is_empty());
    assert!(imported.feature.warnings.is_empty(), "{:?}", imported.feature.warnings);
}

#[test]
fn a_filled_zone_adds_copper_next_to_tracks() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start -30 -20) (end 30 20) (layer "Edge.Cuts"))
      (segment (start -20 -12) (end 0 -12) (width 1.2) (layer "B.Cu") (net 1))
      (zone (net 1) (net_name "GND") (layer "B.Cu")
        (filled_polygon (layer "B.Cu") (pts (xy -24 16) (xy -14 16) (xy -14 18) (xy -24 18)))))"#;
    let options = KicadOptions { copper: KicadCopper::Mesh, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "board.kicad_pcb", &options, &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    let copper = meshes.iter().find(|mesh| mesh.name == "Copper B.Cu").expect("the back copper");
    // The track is at world y = +12; the zone is at world y = -18..-16.
    let ys: Vec<f64> = copper.positions.as_chunks::<3>().0.iter().map(|p| p[1]).collect();
    assert!(ys.iter().any(|y| *y < -15.0), "the zone is missing: {ys:?}");
    let vertices = copper.positions.len() / 3;
    assert!(copper.triangles.iter().all(|index| (*index as usize) < vertices), "the zone's triangles point at the wrong vertices");
}

#[test]
fn copper_and_silkscreen_are_off_by_default() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (gr_line (start 1 1) (end 10 1) (layer "F.SilkS") (width 0.2))
      (segment (start 1 2) (end 10 2) (width 0.3) (layer "F.Cu") (net 1)))"#;
    let mut load = |_: &str| Err("not used".to_string());
    let options = KicadOptions { silkscreen: false, ..Default::default() };
    let imported = read_kicad(source.as_bytes(), "board.kicad_pcb", &options, &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    assert_eq!(meshes.len(), 1);
    assert_eq!(meshes[0].name, "board");
}

#[test]
fn vrml_models_are_read_and_placed() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (footprint "part" (at 5 5) (layer "F.Cu") (property "Reference" "U1")
        (model "part.wrl" (offset (xyz 0 0 0)) (scale (xyz 1 1 1)) (rotate (xyz 0 0 0)))))"#;
    let mut load = |path: &str| {
        Ok(KicadModelFile {
            path: path.to_string(),
            bytes: br#"#VRML V2.0 utf8
      Shape { geometry Box { size 2 2 2 } }"#
                .to_vec(),
        })
    };
    let imported = read_kicad(source.as_bytes(), "board.kicad_pcb", &KicadOptions::default(), &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    assert_eq!(meshes.len(), 2, "the board and the VRML model");
    assert_eq!(imported.plan.slots, vec!["U1".to_string()]);
    assert_eq!(imported.plan.places.len(), 1);
    assert!(imported.feature.warnings.is_empty(), "{:?}", imported.feature.warnings);
}

#[test]
fn text_on_silk_and_copper_is_built_and_mirrored_text_flips() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start -10 -10) (end 10 10) (layer "Edge.Cuts"))
      (footprint "text" (layer "F.Cu") (at 0 0)
        (fp_text reference "AB" (at 0 0) (layer "F.SilkS")
          (effects (font (size 2 2) (thickness 0.2)))))
      (gr_text "AB" (at 0 0) (layer "B.SilkS")
        (effects (font (size 2 2) (thickness 0.2)) (justify mirror)))
      (gr_text "CU" (at 0 4) (layer "F.Cu")
        (effects (font (size 2 2) (thickness 0.2)))))"#;
    let options = KicadOptions { copper: KicadCopper::Mesh, silkscreen: true, components: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "text.kicad_pcb", &options, &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    let find = |name: &str| meshes.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("{name} is missing"));
    let front = bounds(find("Silkscreen F.SilkS"));
    let back = bounds(find("Silkscreen B.SilkS"));
    let copper = bounds(find("Copper F.Cu"));
    assert!(front[2] > front[0] && front[3] > front[1], "{front:?}");
    assert!(copper[2] > copper[0] && copper[3] > copper[1], "{copper:?}");
    assert!((back[0] + front[2]).abs() < 1e-6 && (back[2] + front[0]).abs() < 1e-6, "mirrored: {front:?} {back:?}");
}

fn bounds(mesh: &solidcraft_doc::MeshData) -> [f64; 4] {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for point in mesh.positions.as_chunks::<3>().0 {
        min_x = min_x.min(point[0]);
        min_y = min_y.min(point[1]);
        max_x = max_x.max(point[0]);
        max_y = max_y.max(point[1]);
    }
    [min_x, min_y, max_x, max_y]
}

/// The area of the copper mesh called `name` (its volume over its thickness).
fn copper_area(imported: &KicadImport, name: &str, thickness: f64) -> f64 {
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = &imported.feature.kind else { panic!("expected a mesh import") };
    let mesh = meshes.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("{name} is missing"));
    let positions: Vec<Vec3> = mesh.positions.as_chunks::<3>().0.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect();
    let triangles: Vec<[u32; 3]> = mesh.triangles.as_chunks::<3>().0.to_vec();
    let body = solidcraft_kernel::mesh_body(&positions, &triangles).unwrap();
    solidcraft_kernel::measure(&body).unwrap().volume / thickness
}

#[test]
fn hidden_text_is_skipped_and_text_boxes_are_warned_about() {
    let hidden = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (gr_text "AB" (at 10 5) (layer "F.SilkS") hide
        (effects (font (size 2 2) (thickness 0.2)))))"#;
    let mut load = |_: &str| Err("not used".to_string());
    let options = KicadOptions { silkscreen: true, components: false, ..Default::default() };
    let imported = read_kicad(hidden.as_bytes(), "hidden.kicad_pcb", &options, &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    assert_eq!(meshes.len(), 1, "hidden text builds nothing");
    assert!(imported.feature.warnings.is_empty(), "{:?}", imported.feature.warnings);

    let boxes = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (gr_text_box "AB" (start 2 2) (end 10 5) (layer "F.SilkS")
        (effects (font (size 1 1) (thickness 0.15)))))"#;
    let imported = read_kicad(boxes.as_bytes(), "boxes.kicad_pcb", &options, &mut load).unwrap();
    assert!(imported.feature.warnings.iter().any(|w| w.contains("text boxes")), "{:?}", imported.feature.warnings);
}

#[test]
fn modern_property_fields_on_a_text_layer_are_text() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start -10 -10) (end 10 10) (layer "Edge.Cuts"))
      (footprint "field" (layer "F.Cu") (at 0 0)
        (property "Reference" "R1" (at 0 -3) (layer "F.SilkS")
          (effects (font (size 1 1) (thickness 0.15))))
        (property "Value" "10k" (at 0 3) (layer "F.Fab")
          (effects (font (size 1 1) (thickness 0.15))))
        (property "Footprint" "R_0603")))"#;
    let board = parse_kicad_board(source.as_bytes(), "fields.kicad_pcb").unwrap();
    let [footprint] = board.footprints.as_slice() else { panic!("expected one footprint") };
    assert_eq!(footprint.texts.len(), 1, "{:?}", footprint.texts);
    assert_eq!(footprint.texts[0].text, "R1");
    assert_eq!(footprint.texts[0].layer, "F.SilkS");
    assert_eq!(footprint.texts[0].at, [0.0, -3.0, 0.0]);
}

#[test]
fn copper_overlay_is_a_flat_face_on_the_board() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (segment (start 1 2) (end 10 2) (width 0.3) (layer "F.Cu") (net 1))
      (zone (net 1) (net_name "VCC") (layer "B.Cu") (filled_polygon
        (layer "B.Cu") (pts (xy 1 5) (xy 5 5) (xy 5 8) (xy 1 8)))))"#;
    let options = KicadOptions { copper: KicadCopper::Overlay, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "overlay.kicad_pcb", &options, &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    let find = |name: &str| meshes.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("{name} is missing"));
    let front = find("Copper F.Cu");
    let back = find("Copper B.Cu");
    let zs: Vec<f64> = front.positions.as_chunks::<3>().0.iter().map(|p| p[2]).collect();
    assert!(zs.iter().all(|z| (z - 1.6).abs() < 1e-9 || (z - 1.601).abs() < 1e-9), "{zs:?}");
    let zs: Vec<f64> = back.positions.as_chunks::<3>().0.iter().map(|p| p[2]).collect();
    assert!(zs.iter().all(|z| z.abs() < 1e-9 || (z + 0.001).abs() < 1e-9), "{zs:?}");
    // The film is a thin closed solid: it builds, with a tiny (positive) volume.
    let positions: Vec<Vec3> = front.positions.as_chunks::<3>().0.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect();
    let triangles: Vec<[u32; 3]> = front.triangles.as_chunks::<3>().0.to_vec();
    let body = solidcraft_kernel::mesh_body(&positions, &triangles).unwrap();
    let measure = solidcraft_kernel::measure(&body).unwrap();
    assert!(measure.volume > 0.0 && measure.volume < 0.1, "{measure:?}");
}

#[test]
fn trace_into_a_via_stops_at_its_drill() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (via (at 10 5) (size 1.2) (drill 0.6) (layers "F.Cu" "B.Cu") (net 1))
      (segment (start 10 5) (end 16 5) (width 0.4) (layer "F.Cu") (net 1)))"#;
    let options = KicadOptions { copper: KicadCopper::Mesh, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "via.kicad_pcb", &options, &mut load).unwrap();
    let area = copper_area(&imported, "Copper F.Cu", 0.035);
    // The via's ring plus the trace from 10.5 (its 0.3 drill plus half the 0.4 stroke) to 16:
    // nothing over the hole, not even the stroke's corner.
    let expected = std::f64::consts::PI * (0.6 * 0.6 - 0.3 * 0.3) + 5.5 * 0.4;
    assert!((area - expected).abs() < 0.03, "{area} vs {expected}");
}

#[test]
fn a_trace_crossing_a_hole_is_cut_at_the_hole() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (via (at 10 5) (size 1.2) (drill 0.6) (layers "F.Cu" "B.Cu") (net 1))
      (segment (start 6 5) (end 14 5) (width 0.4) (layer "F.Cu") (net 1)))"#;
    let options = KicadOptions { copper: KicadCopper::Mesh, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "via.kicad_pcb", &options, &mut load).unwrap();
    let area = copper_area(&imported, "Copper F.Cu", 0.035);
    // The ring plus the trace on either side of the hole (6 → 9.5 and 10.5 → 14).
    let expected = std::f64::consts::PI * (0.6 * 0.6 - 0.3 * 0.3) + 2.0 * 3.5 * 0.4;
    assert!((area - expected).abs() < 0.03, "{area} vs {expected}");
}

#[test]
fn traces_stop_at_drilled_holes() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (segment (start 2 5) (end 18 5) (width 0.4) (layer "F.Cu") (net 1))
      (footprint "test:TH" (layer "F.Cu") (at 10 5)
        (pad "1" thru_hole circle (at 0 0) (size 3 3) (drill 1.2) (layers "F.Cu" "B.Cu"))))"#;
    // Only the trace's copper: the pad's own ring is not built.
    let options = KicadOptions { copper: KicadCopper::Mesh, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "trace.kicad_pcb", &options, &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    let copper = meshes.iter().find(|m| m.name == "Copper F.Cu").expect("front copper");
    let positions: Vec<Vec3> = copper.positions.as_chunks::<3>().0.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect();
    let triangles: Vec<[u32; 3]> = copper.triangles.as_chunks::<3>().0.to_vec();
    let body = solidcraft_kernel::mesh_body(&positions, &triangles).unwrap();
    let area = solidcraft_kernel::measure(&body).unwrap().volume / 0.035;
    // The trace stops at the pad; its copper and the remaining trace are one joined plate.
    assert!(area > 10.5 && area < 11.5, "trace and pad copper area: {area}");
}

#[test]
fn closed_silk_shapes_are_drawn_all_the_way_round() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (gr_rect (start 5 5) (end 15 10) (layer "F.SilkS") (width 0.2) (fill none)))"#;
    let options = KicadOptions { copper: KicadCopper::None, components: false, silkscreen: true, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "silk.kicad_pcb", &options, &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    let silk = meshes.iter().find(|m| m.name == "Silkscreen F.SilkS").expect("front silk");
    let positions: Vec<Vec3> = silk.positions.as_chunks::<3>().0.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect();
    let triangles: Vec<[u32; 3]> = silk.triangles.as_chunks::<3>().0.to_vec();
    let body = solidcraft_kernel::mesh_body(&positions, &triangles).unwrap();
    let volume = solidcraft_kernel::measure(&body).unwrap().volume;
    // A 10 × 5 mm rectangle: perimeter 30 mm × 0.2 mm wide × 0.01 mm thick, every side drawn.
    assert!((volume - 0.06).abs() < 1e-6, "{volume}");
}

#[test]
fn via_copper_is_a_ring_around_its_drill() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (via (at 10 5) (size 1.2) (drill 0.6) (layers "F.Cu" "B.Cu") (net 1)))"#;
    let options = KicadOptions { copper: KicadCopper::Mesh, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "via.kicad_pcb", &options, &mut load).unwrap();
    let expected = std::f64::consts::PI * (0.6 * 0.6 - 0.3 * 0.3);
    for layer in ["Copper F.Cu", "Copper B.Cu"] {
        // Copper where the copper is: a 1.2 mm ring around a 0.6 mm drill.
        let area = copper_area(&imported, layer, 0.035);
        assert!((area - expected).abs() < 0.01, "{layer}: {area} vs {expected}");
    }
}

#[test]
fn through_hole_pad_copper_has_the_drill_through_it() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (footprint "test:TH" (layer "F.Cu") (at 10 5)
        (property "Reference" "J1")
        (pad "1" thru_hole circle (at 0 0) (size 2 2) (drill 1) (layers "F.Cu" "B.Cu"))))"#;
    let options = KicadOptions { copper: KicadCopper::Mesh, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "pad.kicad_pcb", &options, &mut load).unwrap();
    let expected = std::f64::consts::PI * (1.0 - 0.25);
    for layer in ["Copper F.Cu", "Copper B.Cu"] {
        // A 2 mm pad with its 1 mm drill open.
        let area = copper_area(&imported, layer, 0.035);
        assert!((area - expected).abs() < 0.05, "{layer}: {area} vs {expected}");
    }
}

#[test]
fn a_stroke_is_one_outline_with_mitered_corners() {
    let region = stroke_region(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], 2.0, false).expect("a stroke outline");
    assert!(region.holes.is_empty());
    let outline: Vec<[f64; 2]> = region.outer.polyline(1e-3).to_vec().into_iter().map(|p| [p.x, p.y]).collect();
    let has = |x: f64, y: f64| outline.iter().any(|p| (p[0] - x).abs() < 1e-9 && (p[1] - y).abs() < 1e-9);
    // The corner reaches its full width on both sides: no notch between the two legs.
    assert!(has(11.0, -1.0), "{outline:?}");
    assert!(has(9.0, 1.0), "{outline:?}");
    assert!(has(0.0, 1.0) && has(0.0, -1.0), "{outline:?}");
    // One closed contour, not two rectangles.
    assert_eq!(outline.len(), 6, "{outline:?}");

    // A trace that comes back to its start is a ring, mitred at the seam as well.
    let ring = stroke_region(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]], 2.0, true).expect("a ring outline");
    assert_eq!(ring.holes.len(), 1, "the inside of the ring is a hole");
    let outer: Vec<[f64; 2]> = ring.outer.polyline(1e-3).to_vec().into_iter().map(|p| [p.x, p.y]).collect();
    let inner: Vec<[f64; 2]> = ring.holes[0].polyline(1e-3).to_vec().into_iter().map(|p| [p.x, p.y]).collect();
    let has = |pts: &[[f64; 2]], x: f64, y: f64| pts.iter().any(|p| (p[0] - x).abs() < 1e-9 && (p[1] - y).abs() < 1e-9);
    assert!(has(&outer, 11.0, 11.0) && has(&outer, -1.0, -1.0), "{outer:?}");
    assert!(has(&inner, 9.0, 9.0) && has(&inner, 1.0, 1.0), "{inner:?}");
    assert_eq!((outer.len(), inner.len()), (4, 4), "{outer:?} {inner:?}");
}

#[test]
fn track_legs_that_continue_each_other_are_one_trace() {
    let track = |a: [f64; 2], b: [f64; 2], width: f64, layer: &str| KicadStroke {
        layer: layer.to_string(),
        points: vec![a, b],
        closed: false,
        filled: true,
        width,
    };
    let traces = track_polylines(&[
        track([0.0, 0.0], [10.0, 0.0], 0.25, "F.Cu"),
        track([10.0, 0.0], [10.0, 10.0], 0.25, "F.Cu"),
        track([10.0, 10.0], [20.0, 10.0], 0.25, "F.Cu"),
    ]);
    assert_eq!(traces.len(), 1, "{traces:?}");
    let (front, width, points) = &traces[0];
    assert!(*front);
    assert!((*width - 0.25).abs() < 1e-12);
    // Board frame: KiCad's Y is negated, and the legs are one 4-point polyline.
    assert_eq!(points, &[[0.0, 0.0], [10.0, 0.0], [10.0, -10.0], [20.0, -10.0]]);

    // A leg of another width is a trace of its own; the two same-width legs through the junction
    // still continue each other (they are one net), so there are two traces, not three.
    let separate = track_polylines(&[
        track([0.0, 0.0], [10.0, 0.0], 0.25, "F.Cu"),
        track([10.0, 0.0], [20.0, 0.0], 0.5, "F.Cu"),
        track([10.0, 0.0], [10.0, 10.0], 0.25, "F.Cu"),
    ]);
    assert_eq!(separate.len(), 2, "{separate:?}");
    assert!(separate.iter().any(|(_, width, points)| (*width - 0.5).abs() < 1e-12 && points.len() == 2), "{separate:?}");
    assert!(separate.iter().any(|(_, width, points)| (*width - 0.25).abs() < 1e-12 && points.len() == 3), "{separate:?}");
}

/// KiCad 9 writes a zone spanning both sides as `(layers "F.Cu" "B.Cu")`, one fill per layer:
/// both fills must come in, not be dropped for having no single `(layer …)`.
#[test]
fn a_zone_spanning_both_sides_brings_both_fills() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (zone (net "GND") (layers "F.Cu" "B.Cu") (fill yes)
        (polygon (pts (xy 1 1) (xy 19 1) (xy 19 9) (xy 1 9)))
        (filled_polygon (layer "F.Cu") (pts (xy 1 2) (xy 9 2) (xy 9 8) (xy 1 8)))
        (filled_polygon (layer "B.Cu") (pts (xy 11 2) (xy 19 2) (xy 19 8) (xy 11 8)))))"#;
    let board = parse_kicad_board(source.as_bytes(), "zones.kicad_pcb").unwrap();
    let layers: Vec<&str> = board.zones.iter().map(|z| z.layer.as_str()).collect();
    assert_eq!(layers, ["F.Cu", "B.Cu"], "{layers:?}");
    assert_eq!(board.zones.iter().map(|z| z.contours.len()).sum::<usize>(), 2);

    // Both fills reach the model, as copper on their own side.
    let options = KicadOptions { copper: KicadCopper::Mesh, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "zones.kicad_pcb", &options, &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    for name in ["Copper F.Cu", "Copper B.Cu"] {
        let copper = meshes.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("{name} is missing"));
        let positions: Vec<Vec3> = copper.positions.as_chunks::<3>().0.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect();
        let triangles: Vec<[u32; 3]> = copper.triangles.as_chunks::<3>().0.to_vec();
        let body = solidcraft_kernel::mesh_body(&positions, &triangles).unwrap();
        let area = solidcraft_kernel::measure(&body).unwrap().volume / 0.035;
        assert!((area - 8.0 * 6.0).abs() < 1e-6, "{name}: {area}");
    }
}

/// KiCad's fills leave repeated points, needle tips and straight-run points: extruded as they are
/// they make slivers standing out of the copper, so a contour loses them first.
#[test]
fn a_fill_contour_loses_its_degenerate_details() {
    let points = [[0.0, 0.0], [10.0, 0.0], [10.0, 0.0], [10.0, 5.0], [10.001, 5.0], [10.0, 5.0], [10.0, 10.0], [5.0, 10.0], [0.0, 10.0], [0.0, 5.0]];
    let cleaned = clean_contour(&points).expect("a polygon");
    // The repeated points, the needle at (10.001, 5) and the middles of the straight sides go.
    assert_eq!(cleaned, vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]], "{cleaned:?}");
    assert!(clean_contour(&[[0.0, 0.0], [1.0, 0.0]]).is_none(), "two points are no polygon");
}

/// The solder mask is not modelled, but its look is: copper under the mask takes the board's
/// colour (warmed by the copper's), and a pad whose mask is open comes in as gold plating.
#[test]
fn copper_under_the_mask_takes_the_board_colour_and_pads_are_gold() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (segment (start 2 5) (end 12 5) (width 0.4) (layer "F.Cu") (net 1))
      (footprint "test:SMD" (layer "F.Cu") (at 15 5)
        (property "Reference" "R1")
        (pad "1" smd rect (at 0 0) (size 2 1) (layers "F.Cu" "F.Paste" "F.Mask"))))"#;
    let options = KicadOptions { copper: KicadCopper::Mesh, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "mask.kicad_pcb", &options, &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    let find = |name: &str| meshes.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("{name} is missing"));
    let masked = masked_copper_color(DEFAULT_BOARD_COLOR);
    assert_eq!(find("Copper F.Cu").color, Some(masked), "the trace is under the mask");
    assert_eq!(find("Copper F.Cu pads").color, Some(PAD_COLOR), "the pad's mask is open, and a bare pad is gold");
}

/// A trace that runs into a pad stops at its edge: the pad's own copper (bare metal) carries on,
/// so the masked trace and the open pad never share a face in different colours.
#[test]
fn a_trace_stops_where_a_pads_copper_begins() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (segment (start 2 5) (end 12 5) (width 0.4) (layer "F.Cu") (net 1))
      (footprint "test:SMD" (layer "F.Cu") (at 12 5)
        (property "Reference" "R1")
        (pad "1" smd rect (at 0 0) (size 2 2) (layers "F.Cu" "F.Mask"))))"#;
    let options = KicadOptions { copper: KicadCopper::Mesh, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "joint.kicad_pcb", &options, &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    let extent = |name: &str| {
        let mesh = meshes.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("{name} is missing"));
        let xs: Vec<f64> = mesh.positions.as_chunks::<3>().0.iter().map(|p| p[0]).collect();
        (xs.iter().copied().fold(f64::MAX, f64::min), xs.iter().copied().fold(f64::MIN, f64::max))
    };
    let (_, trace_max) = extent("Copper F.Cu");
    assert!((trace_max - 11.0).abs() < 1e-6, "the trace ends at the pad's edge: {trace_max}");
    let (pad_min, pad_max) = extent("Copper F.Cu pads");
    assert!((pad_min - 11.0).abs() < 1e-6 && (pad_max - 13.0).abs() < 1e-6, "{pad_min}..{pad_max}");
}

/// The bare pad and the masked trace are the same foil, but two coplanar faces in different
/// colours fight for the pixel: the pad stands `PAD_PROUD` above the masked copper.
#[test]
fn a_bare_pad_stands_above_the_masked_copper() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (segment (start 2 5) (end 12 5) (width 0.4) (layer "F.Cu") (net 1))
      (footprint "test:SMD" (layer "F.Cu") (at 12 5)
        (property "Reference" "R1")
        (pad "1" smd rect (at 0 0) (size 2 2) (layers "F.Cu" "F.Mask"))))"#;
    let options = KicadOptions { copper: KicadCopper::Mesh, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "proud.kicad_pcb", &options, &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    let top = |name: &str| {
        let mesh = meshes.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("{name} is missing"));
        mesh.positions.as_chunks::<3>().0.iter().map(|p| p[2]).fold(f64::MIN, f64::max)
    };
    let (trace, pad) = (top("Copper F.Cu"), top("Copper F.Cu pads"));
    assert!((trace - 1.635).abs() < 1e-9, "the masked trace is a foil thick: {trace}");
    assert!((pad - (1.635 + PAD_PROUD)).abs() < 1e-9, "the bare pad stands proud: {pad}");
}

#[test]
fn model_face_colours_become_one_mesh_each() {
    // A box body with a per-face style, as a KiCad package's STEP model has: its bottom face is
    // blue, every other face takes the body colour.
    let square = [Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 2.0), Vec2::new(0.0, 2.0)];
    let region = Region2 { outer: Loop2::polygon(&square), holes: Vec::new() };
    let body = solidcraft_kernel::extrude(&Plane::XY, &[region], 0.0, 1.0).expect("box").into_iter().next().expect("one body");
    let painted = body.with_paint(Some(solidcraft_kernel::Paint {
        opacity: 1.0,
        faces: vec![solidcraft_kernel::FacePaint { face: 0, color: [0.0, 0.0, 1.0], opacity: 1.0 }],
    }));
    let meshes = model_body_meshes("part", &painted, Some([1.0, 0.0, 0.0])).expect("model meshes");
    assert_eq!(meshes.len(), 2, "{:?}", meshes.iter().map(|m| m.color).collect::<Vec<_>>());
    let blue = meshes.iter().find(|m| m.color == Some([0.0, 0.0, 1.0])).expect("the styled face");
    let red = meshes.iter().find(|m| m.color == Some([1.0, 0.0, 0.0])).expect("the body colour");
    assert!(!blue.triangles.is_empty() && !red.triangles.is_empty());
    let total: usize = meshes.iter().map(|m| m.triangles.len() / 3).sum();
    assert_eq!(total, painted.tessellate(0.1).expect("tessellation").triangles.len());
    for mesh in &meshes {
        assert!(mesh.triangles.iter().all(|i| (*i as usize) < mesh.positions.len() / 3), "{} indexes its own vertices", mesh.name);
    }
}

#[test]
fn a_zone_opens_up_for_a_mounting_holes_drill() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 20) (layer "Edge.Cuts"))
      (footprint "MountingHole" (layer "F.Cu") (at 10 10)
        (pad "1" thru_hole circle (at 0 0) (size 6.4 6.4) (drill 3.2) (layers "*.Cu" "*.Mask")))
      (zone (net 1) (net_name "GND") (layer "F.Cu")
        (filled_polygon (layer "F.Cu") (pts (xy 2 2) (xy 18 2) (xy 18 18) (xy 2 18)))))"#;
    let options = KicadOptions { copper: KicadCopper::Mesh, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "mounting.kicad_pcb", &options, &mut load).unwrap();
    let area = copper_area(&imported, "Copper F.Cu", 0.035);
    // The pour with the mounting hole's drill open through it, though the zone fill covers it.
    let expected = 16.0 * 16.0 - std::f64::consts::PI * 1.6 * 1.6;
    assert!((area - expected).abs() < 0.5, "{area} vs {expected}");
}

#[test]
fn a_pad_opens_up_for_any_hole_it_covers() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 20) (layer "Edge.Cuts"))
      (via (at 5 5) (size 1.2) (drill 0.6) (layers "F.Cu" "B.Cu") (net 1))
      (footprint "part" (layer "F.Cu") (at 0 0)
        (pad "1" smd rect (at 5 5) (size 3 3) (layers "F.Cu" "F.Mask"))))"#;
    let options = KicadOptions { copper: KicadCopper::Mesh, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "unplated.kicad_pcb", &options, &mut load).unwrap();
    // A bare pad stands above the masked copper, so it is PAD_PROUD thicker.
    let area = copper_area(&imported, "Copper F.Cu pads", 0.035 + 0.005);
    // The pad with the via's drill open through it: the pad data names no drill of its own.
    let expected = 9.0 - std::f64::consts::PI * 0.3 * 0.3;
    assert!((area - expected).abs() < 0.05, "{area} vs {expected}");
}
