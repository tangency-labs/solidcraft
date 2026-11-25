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
fn reads_the_board_thickness_without_building_the_board() {
    assert_eq!(kicad_board_thickness(BOARD.as_bytes()), Some(1.6));
    assert_eq!(kicad_board_thickness(b"(kicad_pcb (version 20240108))"), None);
    assert_eq!(kicad_board_thickness(b"not a board"), None);
}

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
        name: String::new(),
        fields: Vec::new(),
        uuid: String::new(),
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
fn a_back_side_model_offset_is_turned_over_with_the_model() {
    let model = KicadModel { path: String::new(), at: [-24.6, -12.8, 0.5], scale: [1.0; 3], rotate: [0.0; 3] };
    let origin = solidcraft_doc::apply_point(&model_local_matrix(&model, true), Vec3::ZERO);
    // KiCad 10.0.1 turns a back-side footprint's model over together with its offset: X stays,
    // Y is reversed, and a positive Z offset moves the model away from the board, which is down.
    assert!((origin.x + 24.6).abs() < 1e-9 && (origin.y - 12.8).abs() < 1e-9 && (origin.z + 0.5).abs() < 1e-9, "{origin:?}");
    let front = solidcraft_doc::apply_point(&model_local_matrix(&model, false), Vec3::ZERO);
    assert!((front.x + 24.6).abs() < 1e-9 && (front.y + 12.8).abs() < 1e-9 && (front.z - 0.5).abs() < 1e-9, "{front:?}");
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
fn an_outline_whose_lines_miss_each_other_by_nanometres_is_closed() {
    // A 20 x 10 board as KiCad can save it: every corner has a gap of 1 to 6 nm between the end of
    // one line and the start of the next. KiCad reads this outline as closed.
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_line (start 0 0) (end 20 0) (layer "Edge.Cuts"))
      (gr_line (start 20.000006 0) (end 20 10) (layer "Edge.Cuts"))
      (gr_line (start 20 10.000001) (end 0.000002 10) (layer "Edge.Cuts"))
      (gr_line (start 0 10) (end 0 0.000004) (layer "Edge.Cuts")))"#;
    let board = parse_kicad_board(source.as_bytes(), "gaps.kicad_pcb").unwrap();
    assert!(board.warnings.is_empty(), "{:?}", board.warnings);
    let [outline] = board.outlines.as_slice() else { panic!("expected one outline: {:?}", board.outlines) };
    assert_eq!(outline.len(), 4, "one point per corner: {outline:?}");
    let mesh = kicad_board_mesh(&board, board.thickness).unwrap();
    let body = solidcraft_kernel::mesh_body(&mesh.positions, &mesh.triangles).unwrap();
    let measure = solidcraft_kernel::measure(&body).unwrap();
    assert!((measure.volume - 200.0 * 1.6).abs() < 1e-3, "{measure:?}");
}

#[test]
fn an_outline_piece_shorter_than_the_joining_distance_is_kept() {
    // The outline's last piece is 0.005 mm long, so the corner before it is already within the
    // joining distance of the start. The piece still belongs to the outline: five corners, and
    // nothing left over.
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_line (start 0 0) (end 20 0) (layer "Edge.Cuts"))
      (gr_line (start 20 0) (end 20 10) (layer "Edge.Cuts"))
      (gr_line (start 20 10) (end 0 10) (layer "Edge.Cuts"))
      (gr_line (start 0 10) (end 0 0.005) (layer "Edge.Cuts"))
      (gr_line (start 0 0.005) (end 0 0) (layer "Edge.Cuts")))"#;
    let board = parse_kicad_board(source.as_bytes(), "short.kicad_pcb").unwrap();
    assert!(board.warnings.is_empty(), "{:?}", board.warnings);
    let [outline] = board.outlines.as_slice() else { panic!("expected one outline: {:?}", board.outlines) };
    assert_eq!(outline.len(), 5, "{outline:?}");
    // Short pieces right after the start do not close on themselves either.
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_line (start 0 0) (end 0.003 0) (layer "Edge.Cuts"))
      (gr_line (start 0.003 0) (end 0.006 0) (layer "Edge.Cuts"))
      (gr_line (start 0.006 0) (end 0.009 0) (layer "Edge.Cuts"))
      (gr_line (start 0.009 0) (end 20 0) (layer "Edge.Cuts"))
      (gr_line (start 20 0) (end 20 10) (layer "Edge.Cuts"))
      (gr_line (start 20 10) (end 0 10) (layer "Edge.Cuts"))
      (gr_line (start 0 10) (end 0 0) (layer "Edge.Cuts")))"#;
    let board = parse_kicad_board(source.as_bytes(), "short-start.kicad_pcb").unwrap();
    assert!(board.warnings.is_empty(), "{:?}", board.warnings);
    let [outline] = board.outlines.as_slice() else { panic!("expected one outline: {:?}", board.outlines) };
    assert_eq!(outline.len(), 7, "{outline:?}");
}

#[test]
fn an_outline_with_its_ends_piled_in_one_place_is_given_up_not_searched_forever() {
    // 12,000 short pieces zigzag inside one 0.01 mm square, so every step of the join would look at
    // every end left: about 144 million looks. The join stops at its limit and says so. A closed
    // square drawn before the pile is not kept either: the board gets no outline, not part of one.
    let given_up = "the Edge.Cuts outline has too many ends in one place to join";
    let mut source = String::from("(kicad_pcb (general (thickness 1.6))\n(gr_rect (start 10 10) (end 20 20) (layer \"Edge.Cuts\"))\n");
    let point = |i: usize| (0.002 + 0.002 * (i % 2) as f64, 0.0005 + 5e-7 * i as f64);
    for i in 0..12_000 {
        let ((x0, y0), (x1, y1)) = (point(i), point(i + 1));
        source.push_str(&format!("(gr_line (start {x0:.7} {y0:.7}) (end {x1:.7} {y1:.7}) (layer \"Edge.Cuts\"))\n"));
    }
    source.push(')');
    let board = parse_kicad_board(source.as_bytes(), "pile.kicad_pcb").unwrap();
    assert!(board.outlines.is_empty(), "{} outlines", board.outlines.len());
    assert!(board.warnings.iter().any(|w| w == given_up), "{:?}", board.warnings);

    // Pieces of no length skip the search for a next piece, but taking each one out of the grid
    // still looks through every end in its cell: 100,000 of them in one spot is 2 × 10¹⁰ looks
    // unless that counts toward the limit too.
    let mut source = String::from("(kicad_pcb (general (thickness 1.6))\n");
    for _ in 0..100_000 {
        source.push_str("(gr_line (start 0.005 0.005) (end 0.005 0.005) (layer \"Edge.Cuts\"))\n");
    }
    source.push(')');
    let started = std::time::Instant::now();
    let board = parse_kicad_board(source.as_bytes(), "points.kicad_pcb").unwrap();
    assert!(board.outlines.is_empty(), "{} outlines", board.outlines.len());
    assert!(board.warnings.iter().any(|w| w == given_up), "{:?}", board.warnings.iter().take(3).collect::<Vec<_>>());
    assert!(started.elapsed() < std::time::Duration::from_secs(30), "{:?}", started.elapsed());
}

#[test]
fn an_outline_with_a_real_gap_stays_open() {
    // The last line stops 0.02 mm short of the first corner: more than the 0.01 mm KiCad joins.
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_line (start 0 0) (end 20 0) (layer "Edge.Cuts"))
      (gr_line (start 20 0) (end 20 10) (layer "Edge.Cuts"))
      (gr_line (start 20 10) (end 0 10) (layer "Edge.Cuts"))
      (gr_line (start 0 10) (end 0 0.02) (layer "Edge.Cuts")))"#;
    let board = parse_kicad_board(source.as_bytes(), "open.kicad_pcb").unwrap();
    assert!(board.outlines.is_empty(), "{:?}", board.outlines);
    assert_eq!(board.warnings, ["skipped an open Edge.Cuts contour"]);
}

#[test]
fn an_exact_continuation_is_joined_before_one_across_a_gap() {
    // Two squares whose corners are 5 nm apart: each closes on its own lines, not on the other's.
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_line (start 0 0) (end 10 0) (layer "Edge.Cuts"))
      (gr_line (start 10.000005 0) (end 20 0) (layer "Edge.Cuts"))
      (gr_line (start 10 0) (end 10 10) (layer "Edge.Cuts"))
      (gr_line (start 20 0) (end 20 10) (layer "Edge.Cuts"))
      (gr_line (start 10 10) (end 0 10) (layer "Edge.Cuts"))
      (gr_line (start 20 10) (end 10.000005 10) (layer "Edge.Cuts"))
      (gr_line (start 0 10) (end 0 0) (layer "Edge.Cuts"))
      (gr_line (start 10.000005 10) (end 10.000005 0) (layer "Edge.Cuts")))"#;
    let board = parse_kicad_board(source.as_bytes(), "pair.kicad_pcb").unwrap();
    assert!(board.warnings.is_empty(), "{:?}", board.warnings);
    assert_eq!(board.outlines.len(), 2, "{:?}", board.outlines);
    assert!(board.outlines.iter().all(|outline| outline.len() == 4), "{:?}", board.outlines);
}

#[test]
fn the_import_reports_the_outlines_lower_left_corner() {
    // A board drawn away from KiCad's origin: world X 30..90, and KiCad's Y 40..70 becomes
    // world Y -70..-40, so the lower-left corner is (30, -70).
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 30 40) (end 90 70) (layer "Edge.Cuts")))"#;
    let mut load = |_: &str| Err("no models".to_string());
    let imported = read_kicad(source.as_bytes(), "corner.kicad_pcb", &KicadOptions::default(), &mut load).unwrap();
    assert_eq!(imported.corner, Some([30.0, -70.0]), "the corner the caller shifts the board by");
}

#[test]
fn a_model_that_stays_out_says_why_in_a_few_words() {
    // The kernel's own words, with its layers of context and the resolved path taken off.
    assert_eq!(short_reason("kernel internal error: STEP body preparation: This wire is not simple."), "this wire is not simple");
    assert_eq!(
        short_reason(r"C:\models\part.step: The system cannot find the file specified. (os error 2)"),
        "the system cannot find the file specified"
    );
    assert_eq!(short_reason("   "), "unsupported");
    assert!(short_reason(&"a very long reason ".repeat(20)).chars().count() <= 61);
    // A model that only partly came in says how much, and why; one that gave nothing says why.
    assert_eq!(
        model_loss_warning("DIN41612_C_2x16.step", 1, 33, "this wire is not simple"),
        "DIN41612_C_2x16.step: 1 of 33 solids not imported (this wire is not simple)"
    );
    assert_eq!(
        model_loss_warning("DIN41612_C_2x16.step", 33, 33, "this wire is not simple"),
        "DIN41612_C_2x16.step: not imported (this wire is not simple)"
    );
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
    assert!(meshes.is_empty(), "only the solid board should remain");
    assert!(imported.board_step.is_some(), "the board is not a solid");
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
fn a_board_over_the_old_token_cap_but_under_the_size_cap_is_read() {
    // 4 tokens per indented `(zz 1)` line, as KiCad writes them: just over 2 000 000 tokens in
    // about 4.5 MB, far below the size cap.
    let mut board = String::from("(kicad_pcb (general (thickness 1.6))\n");
    for _ in 0..500_100 {
        board.push_str("\t\t(zz 1)\n");
    }
    board.push(')');
    assert!(board.len() < MAX_KICAD_BYTES);
    assert!(tokenize(&board).map(|t| t.len() > MIN_TOKEN_CAP).unwrap_or(false));
    assert!(parse_kicad_board(board.as_bytes(), "big.kicad_pcb").is_ok());
}

#[test]
fn the_token_cap_follows_the_size_and_still_refuses_dense_junk() {
    assert_eq!(token_cap(0), MIN_TOKEN_CAP);
    assert_eq!(token_cap(MAX_KICAD_BYTES), MAX_KICAD_BYTES / 2);
    // One token per byte is denser than any board: refused by count, quickly.
    let junk = format!("(kicad_pcb {})", "()".repeat(MIN_TOKEN_CAP));
    assert!(parse_kicad_board(junk.as_bytes(), "junk.kicad_pcb").is_err());
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
    assert!(!names.contains(&"board"), "the board is a solid, not a mesh: {names:?}");
    assert!(imported.board_step.is_some(), "the board is not a solid");
    assert!(names.contains(&"Copper F.Cu"), "{names:?}");
    assert!(names.contains(&"Silkscreen F.SilkS"), "{names:?}");
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
fn copper_defaults_to_an_overlay_and_silkscreen_to_on() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (gr_line (start 1 1) (end 10 1) (layer "F.SilkS") (width 0.2))
      (segment (start 1 2) (end 10 2) (width 0.3) (layer "F.Cu") (net 1)))"#;
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "board.kicad_pcb", &KicadOptions::default(), &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    let names: Vec<&str> = meshes.iter().map(|mesh| mesh.name.as_str()).collect();
    assert!(names.contains(&"Copper F.Cu"), "the default copper overlay is missing: {names:?}");
    assert!(names.iter().any(|name| name.contains("Silk")), "the default silkscreen is missing: {names:?}");
    assert!(imported.board_step.is_some(), "the board is not a solid");
}

#[test]
fn the_board_comes_in_as_a_solid_with_its_thickness() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts")))"#;
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "solid.kicad_pcb", &KicadOptions::default(), &mut load).unwrap();
    let step = imported.board_step.expect("the board is a solid");
    let imp = solidcraft_kernel::step_import_shared(&step).expect("the board is a STEP file");
    assert_eq!(imp.bodies.len(), 1, "one board, one solid");
    let volume = solidcraft_kernel::measure(&imp.bodies[0].body).expect("bounds").volume;
    // 20 × 10 mm, 1.6 mm thick: a body whose faces can be picked.
    assert!((volume - 20.0 * 10.0 * 1.6).abs() < 1e-6, "{volume}");
    // The mesh representation is still there for scripts that ask for it.
    let options = KicadOptions { representation: KicadRepresentation::Mesh, ..Default::default() };
    let imported = read_kicad(source.as_bytes(), "solid.kicad_pcb", &options, &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    assert_eq!(meshes.len(), 1, "the mesh board");
    assert!(imported.board_step.is_none());
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
    assert_eq!(meshes.len(), 1, "the VRML model");
    assert!(imported.board_step.is_some(), "the board is a solid");
    assert_eq!(imported.plan.slots, vec!["part".to_string()]);
    assert_eq!(imported.plan.places.len(), 1);
    assert_eq!(imported.plan.places[0].name.as_deref(), Some("U1"));
    assert!(imported.feature.warnings.is_empty(), "{:?}", imported.feature.warnings);
}

/// KiCad's VRML unit is 0.1 inch (2.54 mm): a 10 x 10 x 5 mm box written in that unit imports at
/// 10 x 10 x 5 mm, before the footprint's own scale and offset are applied.
#[test]
fn vrml_models_are_in_kicad_units_of_a_tenth_of_an_inch() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (footprint "part" (at 5 5) (layer "F.Cu") (property "Reference" "U1")
        (model "box.wrl" (offset (xyz 0 0 0)) (scale (xyz 1 1 1)) (rotate (xyz 0 0 0)))))"#;
    let mut load = |path: &str| {
        Ok(KicadModelFile { path: path.to_string(), bytes: b"#VRML V2.0 utf8\nShape { geometry Box { size 3.937008 3.937008 1.968504 } }".to_vec() })
    };
    let imported = read_kicad(source.as_bytes(), "board.kicad_pcb", &KicadOptions::default(), &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    assert_eq!(meshes.len(), 1, "the VRML model");
    let mut low = [f64::MAX; 3];
    let mut high = [f64::MIN; 3];
    for point in meshes[0].positions.as_chunks::<3>().0 {
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    // The file states each size to six decimals: half a unit in the last place, times 2.54 mm.
    let tolerance = 0.5e-6 * 2.54 + 1e-9;
    for (axis, want) in [10.0, 10.0, 5.0].into_iter().enumerate() {
        let size = high[axis] - low[axis];
        assert!((size - want).abs() <= tolerance, "axis {axis}: {size} mm, want {want} mm");
    }
}

/// A footprint can name several models (a body and, say, a pin-1 marker). One of them missing
/// costs that model alone: the others still come in.
#[test]
fn a_missing_model_keeps_the_other_models_of_its_footprint() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (footprint "part" (at 5 5) (layer "F.Cu") (property "Reference" "U1")
        (model "gone.step")
        (model "body.wrl" (offset (xyz 0 0 0)) (scale (xyz 1 1 1)) (rotate (xyz 0 0 0)))))"#;
    let mut load = |path: &str| {
        if path.contains("gone") {
            return Err("model file was not found".to_string());
        }
        Ok(KicadModelFile {
            path: path.to_string(),
            bytes: br#"#VRML V2.0 utf8
      Shape { geometry Box { size 2 2 2 } }"#
                .to_vec(),
        })
    };
    let imported = read_kicad(source.as_bytes(), "board.kicad_pcb", &KicadOptions::default(), &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    assert_eq!(meshes.len(), 1, "the model that is here still comes in: {meshes:?}");
    assert_eq!(imported.plan.slots, vec!["body".to_string()], "the model that is here is placed");
    assert_eq!(imported.plan.places.len(), 1);
    assert!(
        imported.feature.warnings.iter().any(|warning| warning.contains("gone.step") && warning.contains("was not found")),
        "the missing model is reported: {:?}",
        imported.feature.warnings
    );
}

/// A footprint that names both formats of one model (`….wrl` and `….step`) has one model: the
/// missing `….wrl` is answered by the `….step` beside it, and that file is placed once.
#[test]
fn one_model_named_in_both_formats_is_placed_once() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (footprint "part" (at 5 5) (layer "F.Cu") (property "Reference" "U1")
        (model "part.wrl")
        (model "part.step")))"#;
    // Only `part.wrl` is installed: a request for either name is answered by it.
    let mut load = |_: &str| {
        Ok(KicadModelFile {
            path: "part.wrl".to_string(),
            bytes: br#"#VRML V2.0 utf8
      Shape { geometry Box { size 2 2 2 } }"#
                .to_vec(),
        })
    };
    let imported = read_kicad(source.as_bytes(), "board.kicad_pcb", &KicadOptions::default(), &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    assert_eq!(meshes.len(), 1, "one file, one model: {meshes:?}");
    assert_eq!(imported.plan.slots, vec!["part".to_string()]);
    assert_eq!(imported.plan.places.len(), 1, "placed once, not once per reference");
}

/// Two resistors share one model: it is one component, named after the model, placed twice, and
/// each placement carries its own footprint's reference (not both the first one's).
#[test]
fn footprints_sharing_a_model_are_placed_under_their_own_references() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (footprint "R_0603" (at 5 5) (layer "F.Cu") (property "Reference" "R1")
        (model "${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.wrl"))
      (footprint "R_0603" (at 12 5) (layer "F.Cu") (property "Reference" "R33")
        (model "${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.wrl")))"#;
    let mut load = |path: &str| {
        Ok(KicadModelFile {
            path: path.to_string(),
            bytes: br#"#VRML V2.0 utf8
      Shape { geometry Box { size 2 2 2 } }"#
                .to_vec(),
        })
    };
    let imported = read_kicad(source.as_bytes(), "board.kicad_pcb", &KicadOptions::default(), &mut load).unwrap();
    assert_eq!(imported.plan.slots, vec!["R_0603_1608Metric".to_string()], "one model, one component");
    let places: Vec<(usize, Option<&str>)> = imported.plan.places.iter().map(|p| (p.slot, p.name.as_deref())).collect();
    assert_eq!(places, vec![(0, Some("R1")), (0, Some("R33"))]);
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    assert_eq!(meshes.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), vec!["R_0603_1608Metric"], "the shared body is not named after R1");
}

/// KiCad 10's 3D library ships `….stpZ`: a zip archive holding one STEP file.
fn stpz(step: &str) -> Vec<u8> {
    use std::io::Write;
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    zip.start_file("part.step", zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).unwrap();
    zip.write_all(step.as_bytes()).unwrap();
    zip.finish().unwrap();
    out.into_inner()
}

#[test]
fn a_zipped_stpz_model_is_read_as_step() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (footprint "part" (at 5 5) (layer "F.Cu") (property "Reference" "U1")
        (model "${KICAD10_3DMODEL_DIR}/Package.3dshapes/part.stpZ")))"#;
    let bytes = stpz(include_str!("../../../tests/step-conformance/cylinder.step"));
    let mut load = |path: &str| Ok(KicadModelFile { path: path.to_string(), bytes: bytes.clone() });
    let imported = read_kicad(source.as_bytes(), "board.kicad_pcb", &KicadOptions::default(), &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    assert_eq!(meshes.len(), 1, "the zipped STEP comes in: {meshes:?} {:?}", imported.feature.warnings);
    assert_eq!(imported.plan.slots, vec!["part".to_string()]);
    // The model is placed for U1, and its mesh goes into the model's component, not the board's.
    assert_eq!(imported.plan.places.len(), 1, "{:?}", imported.plan.places);
    assert_eq!(imported.plan.places[0].slot, 0);
    assert_eq!(imported.plan.bodies, vec![Some(0)], "{:?}", imported.plan.bodies);
    assert!(imported.feature.warnings.iter().all(|w| !w.contains("part.stpZ")), "{:?}", imported.feature.warnings);
}

/// A `….wrl` reference that the installed library only has as `….stpZ` (KiCad 10) is answered
/// by the zipped STEP: the format follows the file that was read.
#[test]
fn a_wrl_reference_answered_by_an_stpz_file_is_read_as_step() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (footprint "part" (at 5 5) (layer "F.Cu") (property "Reference" "U1")
        (model "part.wrl")))"#;
    let bytes = stpz(include_str!("../../../tests/step-conformance/cylinder.step"));
    let mut load = |_: &str| Ok(KicadModelFile { path: "part.stpZ".to_string(), bytes: bytes.clone() });
    let imported = read_kicad(source.as_bytes(), "board.kicad_pcb", &KicadOptions::default(), &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    assert_eq!(meshes.len(), 1, "{:?}", imported.feature.warnings);
    // The model is placed for U1, and its mesh goes into the model's component, not the board's.
    assert_eq!(imported.plan.slots, vec!["part".to_string()]);
    assert_eq!(imported.plan.places.len(), 1, "{:?}", imported.plan.places);
    assert_eq!(imported.plan.places[0].slot, 0);
    assert_eq!(imported.plan.bodies, vec![Some(0)], "{:?}", imported.plan.bodies);
}

#[test]
fn a_corrupt_stpz_is_warned_and_the_board_is_kept() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (footprint "part" (at 5 5) (layer "F.Cu") (property "Reference" "U1")
        (model "part.stpZ")))"#;
    let mut load = |path: &str| Ok(KicadModelFile { path: path.to_string(), bytes: b"not a zip".to_vec() });
    let imported = read_kicad(source.as_bytes(), "board.kicad_pcb", &KicadOptions::default(), &mut load).unwrap();
    assert!(imported.plan.slots.is_empty());
    assert!(imported.feature.warnings.iter().any(|w| w.starts_with("part.stpZ: not imported")), "{:?}", imported.feature.warnings);
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
    assert!(meshes.is_empty(), "hidden text builds nothing");
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

/// The bounds of the front pads' copper when `footprint` is the only copper on a 40 mm board.
fn front_pad_bounds(footprint: &str) -> [f64; 4] {
    let source = format!(r#"(kicad_pcb (general (thickness 1.6)) (gr_rect (start -20 -20) (end 20 20) (layer "Edge.Cuts")) {footprint})"#);
    let options = KicadOptions { copper: KicadCopper::Mesh, silkscreen: false, components: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "pads.kicad_pcb", &options, &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    // Plated pads have a body of their own; others are in the layer's copper.
    let pads = meshes.iter().find(|m| m.name == "Copper F.Cu pads").or_else(|| meshes.iter().find(|m| m.name == "Copper F.Cu"));
    bounds(pads.unwrap_or_else(|| panic!("no front copper: {:?}", meshes.iter().map(|m| &m.name).collect::<Vec<_>>())))
}

/// KiCad stores a pad's rotation on the board (the footprint's rotation already added), as KiCad
/// 10 draws it: on a footprint turned a quarter turn, a 2 × 0.5 mm pad written at 90° stands up and
/// one written at 0° lies flat. Its position still turns with the footprint: (3, 0) in a
/// footprint turned 90° is 3 mm up the board.
#[test]
fn pads_on_a_turned_footprint_keep_their_own_rotation() {
    let pad = |at: &str| {
        front_pad_bounds(&format!(r#"(footprint "P" (layer "F.Cu") (at 0 0 90) (pad "1" smd rect (at {at}) (size 2 0.5) (layers "F.Cu")))"#))
    };
    let close = |a: [f64; 4], b: [f64; 4]| a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-6);
    let upright = pad("0 0 90");
    assert!(close(upright, [-0.25, -1.0, 0.25, 1.0]), "{upright:?}");
    let flat = pad("0 0 0");
    assert!(close(flat, [-1.0, -0.25, 1.0, 0.25]), "{flat:?}");
    let moved = pad("3 0 90");
    assert!(close(moved, [-0.25, 2.0, 0.25, 4.0]), "{moved:?}");
}

/// A slotted hole lies along its pad: on a footprint turned a quarter turn, an oval pad and its
/// 2 × 0.8 mm slot written at 90° stand up. The drill's angle, the outline traces stop at, the hole
/// cut through the board, the hole opened in the pad's copper and the pad itself are all checked.
#[test]
fn a_slot_on_a_turned_footprint_lies_along_its_pad() {
    let source = r#"(kicad_pcb (general (thickness 1.6)) (gr_rect (start -20 -20) (end 20 20) (layer "Edge.Cuts"))
      (footprint "S" (layer "F.Cu") (at 0 0 90)
        (pad "1" thru_hole oval (at 0 0 90) (size 3 1.5) (drill oval 2 0.8) (layers "*.Cu" "*.Mask"))))"#;
    let board = parse_kicad_board(source.as_bytes(), "slot.kicad_pcb").unwrap();
    let [drill] = board.drills.as_slice() else { panic!("one drill: {:?}", board.drills) };
    assert!((drill.angle.sin().abs() - 1.0).abs() < 1e-9, "the slot stands up the board: {drill:?}");
    let [outline] = drill_outlines(&board).try_into().unwrap_or_else(|o: Vec<_>| panic!("one outline: {}", o.len()));
    let (xs, ys): (Vec<f64>, Vec<f64>) = outline.iter().map(|p| (p[0], p[1])).unzip();
    let width = xs.iter().cloned().fold(f64::MIN, f64::max) - xs.iter().cloned().fold(f64::MAX, f64::min);
    let height = ys.iter().cloned().fold(f64::MIN, f64::max) - ys.iter().cloned().fold(f64::MAX, f64::min);
    assert!((width - 0.8).abs() < 1e-3 && (height - 2.0).abs() < 1e-3, "{width} × {height}");
    // The hole cut through the board and the hole opened in the pad's copper stand up too: both read
    // the drill's angle, and a hole that ignored it would be 2 mm wide and 0.8 mm tall.
    let corners = [[-20.0, -20.0], [20.0, -20.0], [20.0, 20.0], [-20.0, 20.0]];
    let square = Loop2::polygon(&corners.map(|[x, y]| Vec2::new(x, y)));
    let extent = |hole: &Loop2| {
        let (xs, ys): (Vec<f64>, Vec<f64>) = hole.polyline(1e-3).to_vec().into_iter().map(|p| (p.x, p.y)).unzip();
        let span = |v: &[f64]| v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min);
        (span(&xs), span(&ys))
    };
    let regions = regions_from_loops(vec![square.clone()], Some(&board.drills));
    let [region] = regions.as_slice() else { panic!("one region: {}", regions.len()) };
    let [board_hole] = region.holes.as_slice() else { panic!("one hole in the board: {}", region.holes.len()) };
    let (w, h) = extent(board_hole);
    assert!((w - 0.8).abs() < 1e-3 && (h - 2.0).abs() < 1e-3, "the hole through the board: {w} × {h}");
    let pad_hole = pad_holes(&square, &corners, &board.drills);
    let [pad_hole] = pad_hole.as_slice() else { panic!("one hole in the pad: {}", pad_hole.len()) };
    let (w, h) = extent(pad_hole);
    assert!((w - 0.8).abs() < 1e-3 && (h - 2.0).abs() < 1e-3, "the hole in the pad's copper: {w} × {h}");
    let pads = front_pad_bounds(
        r#"(footprint "S" (layer "F.Cu") (at 0 0 90) (pad "1" thru_hole oval (at 0 0 90) (size 3 1.5) (drill oval 2 0.8) (layers "*.Cu" "*.Mask")))"#,
    );
    assert!((pads[2] - pads[0] - 1.5).abs() < 1e-6 && (pads[3] - pads[1] - 3.0).abs() < 1e-6, "{pads:?}");
}

/// The front silkscreen's bounds when `items` (footprints, board text) are drawn on a 40 mm board.
fn front_silk_bounds(items: &str) -> [f64; 4] {
    let source = format!(r#"(kicad_pcb (general (thickness 1.6)) (gr_rect (start -20 -20) (end 20 20) (layer "Edge.Cuts")) {items})"#);
    let options = KicadOptions { copper: KicadCopper::None, silkscreen: true, components: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "text.kicad_pcb", &options, &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    bounds(meshes.iter().find(|m| m.name == "Silkscreen F.SilkS").unwrap_or_else(|| panic!("no front silk: {meshes:?}")))
}

fn same_bounds(a: [f64; 4], b: [f64; 4]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-6)
}

const FONT: &str = "(effects (font (size 1 1) (thickness 0.15)))";
const FONT_LEFT: &str = "(effects (font (size 1 1) (thickness 0.15)) (justify left))";

/// KiCad stores a footprint text's rotation on the board: on a footprint turned a quarter turn, a
/// reference at 0° reads level, as board text at 0° does. Its position still turns with the
/// footprint: (3, 0) in a footprint turned 90° is (0, -3) on the board.
#[test]
fn footprint_text_keeps_its_own_angle_on_a_turned_footprint() {
    let turned = front_silk_bounds(&format!(
        r#"(footprint "R" (layer "F.Cu") (at 0 0 90) (property "Reference" "ABCDEFGH" (at 3 0 0) (layer "F.SilkS") {FONT}))"#
    ));
    let level = front_silk_bounds(&format!(r#"(gr_text "ABCDEFGH" (at 0 -3 0) (layer "F.SilkS") {FONT})"#));
    assert!(same_bounds(turned, level), "{turned:?} {level:?}");
    assert!(turned[2] - turned[0] > turned[3] - turned[1], "reads level, not up the board: {turned:?}");
}

/// A footprint text turned past a quarter turn is drawn half a turn round so that it reads
/// upright (KiCad's rule for text that is not unlocked), keeping its anchor and justification:
/// 180° is drawn as 0°, 270° as 90°. Board text keeps its rotation.
#[test]
fn locked_footprint_text_past_a_quarter_turn_reads_upright() {
    let footprint = |angle: u32, font: &str| {
        front_silk_bounds(&format!(
            r#"(footprint "R" (layer "F.Cu") (at 0 0) (property "Reference" "ABCDEFGH" (at 0 0 {angle}) (layer "F.SilkS") {font}))"#
        ))
    };
    let board = |angle: u32, font: &str| front_silk_bounds(&format!(r#"(gr_text "ABCDEFGH" (at 0 0 {angle}) (layer "F.SilkS") {font})"#));
    assert!(same_bounds(footprint(180, FONT_LEFT), board(0, FONT_LEFT)), "{:?} {:?}", footprint(180, FONT_LEFT), board(0, FONT_LEFT));
    assert!(same_bounds(footprint(270, FONT), board(90, FONT)), "{:?} {:?}", footprint(270, FONT), board(90, FONT));
    assert!(same_bounds(footprint(90, FONT), board(90, FONT)), "a quarter turn is already upright");
    // Board text at 180° is drawn upside down: left-justified, it runs to the left of its anchor.
    let upside_down = board(180, FONT_LEFT);
    assert!(upside_down[2] < 0.5 && upside_down[0] < -5.0, "{upside_down:?}");
}

/// An unlocked footprint text is drawn at its own rotation, upside down if that is what it says.
#[test]
fn unlocked_footprint_text_keeps_its_upside_down_angle() {
    let unlocked = front_silk_bounds(&format!(
        r#"(footprint "R" (layer "F.Cu") (at 0 0) (property "Reference" "ABCDEFGH" (at 0 0 180) (unlocked yes) (layer "F.SilkS") {FONT_LEFT}))"#
    ));
    let board = front_silk_bounds(&format!(r#"(gr_text "ABCDEFGH" (at 0 0 180) (layer "F.SilkS") {FONT_LEFT})"#));
    assert!(same_bounds(unlocked, board), "{unlocked:?} {board:?}");
}

/// Older files write `unlocked` inside the text's `at`: the position and rotation are still read.
#[test]
fn unlocked_written_in_the_position_keeps_the_position() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start -10 -10) (end 10 10) (layer "Edge.Cuts"))
      (footprint "R" (layer "F.Cu") (at 0 0)
        (fp_text reference "R1" (at 3 1 180 unlocked) (layer "F.SilkS")
          (effects (font (size 1 1) (thickness 0.15))))))"#;
    let board = parse_kicad_board(source.as_bytes(), "legacy.kicad_pcb").unwrap();
    let [footprint] = board.footprints.as_slice() else { panic!("expected one footprint") };
    assert_eq!(footprint.texts.len(), 1, "{:?}", footprint.texts);
    assert_eq!(footprint.texts[0].at, [3.0, 1.0, 180.0]);
    assert!(footprint.texts[0].unlocked);
}

/// The board text and each footprint's text, after the variables are put in.
fn texts_of(items: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let source = format!(r#"(kicad_pcb (general (thickness 1.6)) (gr_rect (start -20 -20) (end 20 20) (layer "Edge.Cuts")) {items})"#);
    let board = parse_kicad_board(source.as_bytes(), "vars.kicad_pcb").unwrap();
    let texts = |list: &[KicadText]| list.iter().map(|t| t.text.clone()).collect::<Vec<_>>();
    (texts(&board.texts), board.footprints.iter().map(|f| texts(&f.texts)).collect())
}

const VAR_FONT: &str = "(effects (font (size 1 1) (thickness 0.15)))";

const TITLE_BLOCK: &str = r#"(title_block (title "Probe board") (date "2026-01-02") (rev "B2") (company "Acme") (comment 1 "first"))"#;

/// A footprint R7 (value 10k, a hidden MPN field) from library `Lib`, with one silk text `text`.
fn footprint_with_text(text: &str) -> String {
    format!(
        r#"(footprint "Lib:Part" (layer "F.Cu") (at 0 0)
          (property "Reference" "R7" (at 0 -3) (layer "F.Fab") (hide yes) {VAR_FONT})
          (property "Value" "10k" (at 0 3) (layer "F.Fab") (hide yes) {VAR_FONT})
          (property "MPN" "XQ-12" (at 0 0) (layer "F.Fab") (hide yes) {VAR_FONT})
          (fp_text user "{text}" (at 0 0) (layer "F.SilkS") {VAR_FONT}))"#
    )
}

/// Each name and its value, as KiCad 10 draws them (checked against kicad-cli renders): the title
/// block anywhere, and in a footprint's text its reference, value, any field by name (hidden ones
/// too), its library and name, and the text's layer by the name KiCad shows.
#[test]
fn reference_and_title_block_variables_are_expanded() {
    let board_text = |text: &str| format!(r#"(gr_text "{text}" (at 0 0) (layer "F.SilkS") {VAR_FONT})"#);
    let (board, _) = texts_of(&format!("{TITLE_BLOCK} {}", board_text("${TITLE} rev ${REVISION}, ${ISSUE_DATE}, ${COMPANY}, ${COMMENT1}, ${LAYER}")));
    assert_eq!(board, vec!["Probe board rev B2, 2026-01-02, Acme, first, F.Silkscreen"]);
    let (_, footprints) = texts_of(&format!(
        "{TITLE_BLOCK} {}",
        footprint_with_text("${REFERENCE}=${VALUE} ${MPN} ${FOOTPRINT_LIBRARY}/${FOOTPRINT_NAME} ${LAYER} ${REVISION}")
    ));
    assert_eq!(footprints, vec![vec!["R7=10k XQ-12 Lib/Part F.Silkscreen B2"]]);
}

/// `${R7:VALUE}` reads another footprint's field, from board text or another footprint's; a
/// reference no footprint has reads as KiCad shows it.
#[test]
fn a_text_variable_can_read_another_footprints_field() {
    let (board, _) = texts_of(&format!(
        r#"{TITLE_BLOCK} {} (gr_text "${{R7:VALUE}} ${{R7:REVISION}} ${{R9:VALUE}}" (at 0 0) (layer "F.SilkS") {VAR_FONT})"#,
        footprint_with_text("x")
    ));
    assert_eq!(board, vec!["10k B2 <Unknown reference: R9>"]);
}

/// KiCad's text dialog saves `${R7:VALUE}` with the footprint's id in place of its reference
/// (`(uuid …)` since KiCad 8, `(tstamp …)` before), and reads either form back.
#[test]
fn a_saved_cross_reference_names_the_footprint_by_its_id() {
    for id in ["uuid", "tstamp"] {
        let footprint =
            footprint_with_text("x").replacen(r#"(layer "F.Cu")"#, &format!(r#"(layer "F.Cu") ({id} "0e467c16-584b-4a66-9b99-5dd5e1ebf143")"#), 1);
        let (board, _) =
            texts_of(&format!(r#"{footprint} (gr_text "${{0e467c16-584b-4a66-9b99-5dd5e1ebf143:VALUE}}" (at 0 0) (layer "F.SilkS") {VAR_FONT})"#));
        assert_eq!(board, vec!["10k"], "{id}");
    }
}

/// A field's own variables are put in where the field is read, in its footprint (KiCad reads a
/// field by its shown text): from the footprint's text, through a cross reference, and through a
/// chain of them.
#[test]
fn a_field_holding_variables_is_expanded_where_it_is_read() {
    let footprint = footprint_with_text("${Label}").replacen(
        r#"(property "MPN""#,
        &format!(r#"(property "Label" "${{REFERENCE}}=${{VALUE}}" (at 0 0) (layer "F.Fab") (hide yes) {VAR_FONT}) (property "MPN""#),
        1,
    );
    let (board, footprints) = texts_of(&format!(r#"{footprint} (gr_text "${{R7:Label}} ${{R7:R7:VALUE}}" (at 0 0) (layer "F.SilkS") {VAR_FONT})"#));
    assert_eq!(footprints, vec![vec!["R7=10k"]]);
    assert_eq!(board, vec!["R7=10k 10k"]);
}

/// A field that names itself, and a cross reference repeated a hundred thousand times, end at the
/// depth limit and stay as written instead of running the stack out.
#[test]
fn variables_that_never_end_stay_as_written() {
    let footprint = footprint_with_text("${Loop}").replacen(
        r#"(property "MPN""#,
        &format!(r#"(property "Loop" "${{Loop}}" (at 0 0) (layer "F.Fab") (hide yes) {VAR_FONT}) (property "MPN""#),
        1,
    );
    let chain = format!("${{{}VALUE}}", "R7:".repeat(100_000));
    let (board, footprints) = texts_of(&format!(r#"{footprint} (gr_text "{chain}" (at 0 0) (layer "F.SilkS") {VAR_FONT})"#));
    assert_eq!(board, vec![chain]);
    assert_eq!(footprints, vec![vec!["${Loop}"]]);
}

/// Fields that each read the next a hundred times, eight deep, would take about 100^8 lookups to
/// put in. One work budget for the whole text stops it: the import finishes at once, the text
/// stays within the length limit, and the board says which text was left partly as written.
#[test]
fn deeply_nested_fields_stop_at_the_work_budget() {
    let names = ["A", "B", "C", "D", "E", "F", "G", "H"];
    let mut fields = String::new();
    for (i, name) in names.iter().enumerate() {
        let value = match names.get(i + 1) {
            Some(next) => format!("${{{next}}}").repeat(100),
            None => "x".to_string(),
        };
        fields.push_str(&format!(r#"(property "{name}" "{value}" (at 0 0) (layer "F.Fab") (hide yes) {VAR_FONT}) "#));
    }
    let footprint = footprint_with_text("${A}").replacen(r#"(property "MPN""#, &format!(r#"{fields}(property "MPN""#), 1);
    let source = format!(r#"(kicad_pcb (general (thickness 1.6)) (gr_rect (start -20 -20) (end 20 20) (layer "Edge.Cuts")) {footprint})"#);
    let board = parse_kicad_board(source.as_bytes(), "vars.kicad_pcb").unwrap();
    let text = &board.footprints[0].texts[0].text;
    assert!(text.len() <= 4096 + 8, "{} characters", text.len());
    assert!(board.warnings.iter().any(|w| w.contains("text variables") && w.contains("R7")), "{:?}", board.warnings);
}

/// A value that would take a text past the length limit is not put in: the name stays as written,
/// so a long title read by many texts cannot make each of them long.
#[test]
fn a_value_past_the_length_limit_leaves_its_name() {
    let title = "T".repeat(5000);
    let (board, _) = texts_of(&format!(r#"(title_block (title "{title}")) (gr_text "Board ${{TITLE}}" (at 0 0) (layer "F.SilkS") {VAR_FONT})"#));
    assert_eq!(board, vec!["Board ${TITLE}"]);
}

/// A layer the board's layer table names is shown by that name.
#[test]
fn the_layer_variable_reads_the_boards_own_layer_name() {
    let (board, _) = texts_of(&format!(r#"(layers (5 "F.SilkS" user "Top silk")) (gr_text "${{LAYER}}" (at 0 0) (layer "F.SilkS") {VAR_FONT})"#));
    assert_eq!(board, vec!["Top silk"]);
}

/// A title-block name the board never set is empty, so a text made only of it draws nothing; a
/// text with other characters keeps them.
#[test]
fn empty_text_variables_draw_nothing() {
    let (board, _) = texts_of(&format!(
        r#"(gr_text "${{REVISION}}" (at 0 0) (layer "F.SilkS") {VAR_FONT}) (gr_text "${{ISSUE_DATE}} " (at 0 5) (layer "F.SilkS") {VAR_FONT}) (gr_text "Rev ${{REVISION}}" (at 0 9) (layer "F.SilkS") {VAR_FONT})"#
    ));
    assert_eq!(board, vec!["Rev "]);
}

/// Unknown names, names in the wrong case, a footprint's names in board text and a cross
/// reference to a field the footprint lacks stay as written; a `${` never closed runs to the end
/// of the text, and a name is expanded first, as KiCad reads them.
#[test]
fn unknown_text_variables_stay_literal() {
    let (board, footprints) = texts_of(&format!(
        r#"{TITLE_BLOCK} {} (gr_text "${{NOSUCH}} ${{revision}} ${{REFERENCE}} ${{R7:NOSUCH}}" (at 0 0) (layer "F.SilkS") {VAR_FONT})"#,
        footprint_with_text("${mpn} ${${REFERENCE}} x${REFERENCE")
    ));
    assert_eq!(board, vec!["${NOSUCH} ${revision} ${REFERENCE} ${R7:NOSUCH}"]);
    assert_eq!(footprints, vec![vec!["${mpn} ${R7} xR7"]]);
}

/// A text that names a long title many times grows only so far.
#[test]
fn expanded_text_has_a_length_limit() {
    let title = "T".repeat(1000);
    let text = "${TITLE}".repeat(1000);
    let (board, _) = texts_of(&format!(r#"(title_block (title "{title}")) (gr_text "{text}" (at 0 0) (layer "F.SilkS") {VAR_FONT})"#));
    let [expanded] = board.as_slice() else { panic!("one text: {board:?}") };
    // It stops past 4096 characters; the names after that stay as written.
    assert!(expanded.len() > 4096 && expanded.len() < 4096 + 1000 + 8000, "{}", expanded.len());
    assert!(expanded.starts_with(&title), "the first names are expanded");
    assert!(expanded.ends_with("${TITLE}"), "the rest stays as written");
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
    assert!(zs.iter().all(|z| (z - 1.601).abs() < 1e-9), "{zs:?}");
    let zs: Vec<f64> = back.positions.as_chunks::<3>().0.iter().map(|p| p[2]).collect();
    assert!(zs.iter().all(|z| (z + 0.001).abs() < 1e-9), "{zs:?}");
    for (mesh, sign) in [(front, 1.0), (back, -1.0)] {
        let positions = mesh.positions.as_chunks::<3>().0;
        for triangle in mesh.triangles.as_chunks::<3>().0 {
            let [a, b, c] = triangle.map(|index| positions[index as usize]);
            let cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            assert!(cross * sign > 0.0, "{} has an inward face", mesh.name);
        }
    }
    let positions: Vec<Vec3> = front.positions.as_chunks::<3>().0.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect();
    let triangles: Vec<[u32; 3]> = front.triangles.as_chunks::<3>().0.to_vec();
    let body = solidcraft_kernel::mesh_body(&positions, &triangles).unwrap();
    assert!(!body.is_closed_mesh());
}

#[test]
fn copper_overlay_clips_a_drill_crossing_a_zone_boundary() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 20) (layer "Edge.Cuts"))
      (via (at 2.1 10) (size 1.2) (drill 0.6) (layers "F.Cu" "B.Cu") (net 1))
      (zone (net 1) (net_name "GND") (layer "F.Cu")
        (filled_polygon (layer "F.Cu") (pts (xy 2 2) (xy 18 2) (xy 18 18) (xy 2 18)))))"#;
    let options = KicadOptions { copper: KicadCopper::Overlay, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "crossing.kicad_pcb", &options, &mut load).unwrap();
    assert!(imported.feature.warnings.is_empty(), "{:?}", imported.feature.warnings);
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
    let mesh = meshes.iter().find(|mesh| mesh.name == "Copper F.Cu").unwrap();
    let positions = mesh.positions.as_chunks::<3>().0;
    let area: f64 = mesh
        .triangles
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triangle| {
            let [a, b, c] = triangle.map(|index| positions[index as usize]);
            ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() * 0.5
        })
        .sum();
    assert!(area > 255.0 && area < 258.0, "{area}");
}

#[test]
fn modelled_copper_is_a_closed_prism() {
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
      (segment (start 1 2) (end 10 2) (width 0.3) (layer "F.Cu") (net 1))
      (via (at 14 5) (size 1.2) (drill 0.6) (layers "F.Cu" "B.Cu") (net 1)))"#;
    let options = KicadOptions { copper: KicadCopper::Mesh, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "prism.kicad_pcb", &options, &mut load).unwrap();
    assert!(imported.feature.warnings.is_empty(), "{:?}", imported.feature.warnings);
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = &imported.feature.kind else { panic!("expected a mesh import") };
    let mesh = meshes.iter().find(|mesh| mesh.name == "Copper F.Cu").unwrap();
    let positions: Vec<Vec3> = mesh.positions.as_chunks::<3>().0.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect();
    let triangles: Vec<[u32; 3]> = mesh.triangles.as_chunks::<3>().0.to_vec();
    let body = solidcraft_kernel::mesh_body(&positions, &triangles).unwrap();
    // The caps and the walls meet, so the copper is a solid of the copper's real thickness, not a
    // skin: its volume is the profile's area times the foil.
    assert!(body.is_closed_mesh());
    let expected = (9.0 * 0.3 + std::f64::consts::PI * (0.6 * 0.6 - 0.3 * 0.3)) * 0.035;
    let got = solidcraft_kernel::measure(&body).unwrap().volume;
    assert!((got - expected).abs() < 0.005, "{got} vs {expected}");
}

#[test]
fn a_prism_stays_one_surface_where_its_hole_crosses_the_outline() {
    // A fill whose drill crosses its outline is where a profile carries coincident points and a
    // boundary the triangulation has to split: either could leave a surface with an edge of its
    // own, so the extruded prism is checked as a closed mesh, not by its volume.
    let square = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)];
    for (what, center) in [
        ("a hole inside", Vec2::new(5.0, 5.0)),
        ("a hole crossing the outline", Vec2::new(0.5, 5.0)),
        ("a hole reaching past it", Vec2::new(-1.0, 5.0)),
    ] {
        let region = Region2 { outer: Loop2::polygon(&square), holes: vec![Loop2::circle_from(center, 1.0, 0.0).reversed()] };
        let meshes = extrude_regions(&[region], 0.0, 0.035).unwrap();
        assert_eq!(meshes.len(), 1, "{what} came apart into {} pieces", meshes.len());
        let mesh = meshes.into_iter().next().expect("one prism");
        let body = solidcraft_kernel::mesh_body(&mesh.positions, &mesh.triangles).unwrap();
        assert!(body.is_closed_mesh(), "{what} left a surface that is not one skin");
    }
}

#[test]
fn a_fill_outline_that_touches_itself_keeps_its_area() {
    // A pour's outline runs up a clearance gap and back down it, so it passes the same place twice.
    // Both visits are part of the shape: dropping one cuts a piece out of the fill. Two squares
    // meeting at a corner, as one loop, is the same thing in miniature.
    let square = |x: f64, y: f64| [Vec2::new(x, y), Vec2::new(x + 10.0, y), Vec2::new(x + 10.0, y + 10.0), Vec2::new(x, y + 10.0)];
    let (a, b) = (square(0.0, 0.0), square(10.0, 10.0));
    let points = [a[0], a[1], a[2], b[1], b[2], b[3], b[0], a[3]];
    let region = Region2 { outer: Loop2::polygon(&points), holes: Vec::new() };
    let loops = region_loops(&region).unwrap();
    let (points, triangles) = solidcraft_kernel::services::tess::triangulate_planar(&loops).unwrap();
    let area: f64 = triangles
        .iter()
        .map(|[a, b, c]| {
            let (a, b, c) = (points[*a as usize], points[*b as usize], points[*c as usize]);
            ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() * 0.5
        })
        .sum();
    assert!((area - 200.0).abs() < 1e-6, "the fill lost area: {area}");
}

#[test]
fn a_wide_trace_reaches_the_copper_it_ends_on() {
    // Half of a 1 mm trace is 0.5 mm: clearing the 0.6 mm drill of a via by that much would end the
    // trace 0.2 mm short of the 1.2 mm ring, and a 1.2 mm drill in a 2 mm pad would leave it 0.1 mm
    // short of the pad. The trace has to reach the copper, not the hole's clearance.
    let source = r#"(kicad_pcb (general (thickness 1.6))
      (gr_rect (start 0 0) (end 40 10) (layer "Edge.Cuts"))
      (via (at 10 5) (size 1.2) (drill 0.6) (layers "F.Cu" "B.Cu") (net 1))
      (segment (start 10 5) (end 18 5) (width 1.0) (layer "F.Cu") (net 1))
      (segment (start 20 5) (end 30 5) (width 1.0) (layer "F.Cu") (net 1))
      (footprint "test:TH" (layer "F.Cu") (at 30 5)
        (pad "1" thru_hole circle (at 0 0) (size 2 2) (drill 1.2) (layers "F.Cu" "B.Cu"))))"#;
    let options = KicadOptions { copper: KicadCopper::Mesh, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(source.as_bytes(), "wide.kicad_pcb", &options, &mut load).unwrap();
    let area = layer_area(&imported, "Copper F.Cu", 0.035);
    // The via's ring, the strip from the ring's edge (a hair inside it) to 18, the pad's ring and
    // the strip from 20 to the pad's edge at 29.
    let ring = std::f64::consts::PI * (0.6 * 0.6 - 0.3 * 0.3);
    let strip = (18.0 - 10.6) * 1.0;
    let pad = std::f64::consts::PI * (1.0 - 0.6 * 0.6);
    let to_pad = (29.0 - 20.0) * 1.0;
    let want = ring + strip + pad + to_pad;
    assert!(area > want - 0.02 && area < want + 0.02, "{area} vs {want}");
}

#[test]
fn via_regions_keep_analytic_circles() {
    let region = via_region([10.0, -5.0], 0.6, 0.3).unwrap();
    assert_eq!(region.outer.segs.len(), 1);
    assert_eq!(region.holes.len(), 1);
    assert_eq!(region.holes[0].segs.len(), 1);
    assert!(matches!(region.outer.segs[0], solidcore_record::Seg2::Arc { .. }));
    assert!(matches!(region.holes[0].segs[0], solidcore_record::Seg2::Arc { .. }));
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
    // The via's ring plus the trace from 10.5 (its 0.3 drill plus half the 0.4 stroke) to 16, with
    // nothing over the hole, not even the stroke's corner. They are one copper surface, so the
    // piece of the strip that lies inside the ring's annulus counts once.
    let ring = std::f64::consts::PI * (0.6 * 0.6 - 0.3 * 0.3);
    let strip = 5.5 * 0.4;
    assert!(area > ring + strip - 0.05 && area < ring + strip + 1e-6, "{area} vs {}", ring + strip);
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
    // The ring plus the trace on either side of the hole (6 → 9.5 and 10.5 → 14), each end of the
    // strip lying inside the ring's annulus counted once because it is one copper surface.
    let ring = std::f64::consts::PI * (0.6 * 0.6 - 0.3 * 0.3);
    let strip = 2.0 * 3.5 * 0.4;
    assert!(area > ring + strip - 0.1 && area < ring + strip + 1e-6, "{area} vs {}", ring + strip);
}

/// The copper of one layer as area, over every piece of it: copper that touches itself at a single
/// point has to be more than one body to stay manifold.
fn layer_area(imported: &KicadImport, layer: &str, thickness: f64) -> f64 {
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = &imported.feature.kind else { panic!("expected a mesh import") };
    let piece = format!("{layer} (");
    meshes
        .iter()
        .filter(|mesh| mesh.name == layer || mesh.name.starts_with(&piece))
        .map(|mesh| {
            let positions: Vec<Vec3> = mesh.positions.as_chunks::<3>().0.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect();
            let triangles: Vec<[u32; 3]> = mesh.triangles.as_chunks::<3>().0.to_vec();
            let body = solidcraft_kernel::mesh_body(&positions, &triangles).unwrap();
            solidcraft_kernel::measure(&body).unwrap().volume / thickness
        })
        .sum()
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
    let area = layer_area(&imported, "Copper F.Cu", 0.035);
    // The trace stops at the pad, and the two touch only where the cut meets the pad's round edge,
    // so they are separate surfaces; together they are the trace on either side of it plus the pad.
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

/// `(drill … (offset dx dy))` moves a pad's copper off its hole, in the pad's own frame: the hole
/// stays at the pad's position (checked with kicad-cli 10: its drill file and copper plot). Library
/// footprints use it for hand-soldering pads and module castellations (Adafruit Feather, Raspberry
/// Pi Pico, TO-92 HandSolder); an SMD pad writes it without a drill size.
#[test]
fn a_pad_offset_moves_the_copper_off_its_hole() {
    let front_copper = |at: &str, pad: &str| {
        let source = format!(
            r#"(kicad_pcb (general (thickness 1.6))
              (gr_rect (start 0 0) (end 40 20) (layer "Edge.Cuts"))
              (footprint "test:Offset" (layer "F.Cu") (at {at})
                (property "Reference" "J1")
                {pad}))"#
        );
        let options = KicadOptions { copper: KicadCopper::Mesh, components: false, silkscreen: false, ..Default::default() };
        let mut load = |_: &str| Err("not used".to_string());
        let imported = read_kicad(source.as_bytes(), "offset.kicad_pcb", &options, &mut load).unwrap();
        let board = parse_kicad_board(source.as_bytes(), "offset.kicad_pcb").unwrap();
        let drills: Vec<[f64; 2]> = board.drills.iter().map(|d| d.center).collect();
        let area = copper_area(&imported, "Copper F.Cu", 0.035);
        let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = imported.feature.kind else { panic!("expected a mesh import") };
        let mesh = meshes.iter().find(|m| m.name == "Copper F.Cu").expect("front copper");
        let points = mesh.positions.as_chunks::<3>().0;
        let low = |i: usize| points.iter().map(|p| p[i]).fold(f64::MAX, f64::min);
        let high = |i: usize| points.iter().map(|p| p[i]).fold(f64::MIN, f64::max);
        ([low(0), high(0), low(1), high(1)], area, drills)
    };
    let near = |a: [f64; 4], b: [f64; 4]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-6);

    // A 3 × 2 pad drilled 1 mm, its copper 0.5 mm to the right of the hole: copper from X 9 to 12
    // (KiCad's Y 9 to 11, -11 to -9 here), the hole still at the footprint's origin and inside it.
    let (extent, area, drills) = front_copper("10 10", r#"(pad "1" thru_hole rect (at 0 0) (size 3 2) (drill 1 (offset 0.5 0)) (layers "*.Cu"))"#);
    assert!(near(extent, [9.0, 12.0, -11.0, -9.0]), "{extent:?}");
    assert_eq!(drills, [[10.0, -10.0]], "the hole stays at the pad's position");
    let expected = 6.0 - std::f64::consts::PI * 0.25;
    assert!((area - expected).abs() < 0.05, "the hole opens the shifted copper: {area} vs {expected}");

    // A pad turned a quarter turn turns its offset too: (3, 0) puts the copper 3 mm up the board
    // (KiCad's Y 6 to 8 for a pad at Y 10).
    let (extent, _, drills) = front_copper("10 10", r#"(pad "1" thru_hole rect (at 10 0 90) (size 2 2) (drill 1 (offset 3 0)) (layers "*.Cu"))"#);
    assert!(near(extent, [19.0, 21.0, -8.0, -6.0]), "{extent:?}");
    assert_eq!(drills, [[20.0, -10.0]]);

    // An SMD pad has an offset and no drill: the copper moves and no hole comes in.
    let (extent, _, drills) = front_copper("10 10", r#"(pad "1" smd rect (at 0 5) (size 1 1) (drill (offset 3 0)) (layers "F.Cu"))"#);
    assert!(near(extent, [12.5, 13.5, -15.5, -14.5]), "{extent:?}");
    assert!(drills.is_empty(), "{drills:?}");

    // On a turned footprint the file writes the pad's angle on the board (90° here), and the offset
    // turns with the pad: KiCad draws this copper from X 19 to 21 and Y 6 to 8, hole at (20, 10).
    let (extent, _, drills) = front_copper("20 10 90", r#"(pad "1" thru_hole rect (at 0 0 90) (size 2 2) (drill 1 (offset 3 0)) (layers "*.Cu"))"#);
    assert!(near(extent, [19.0, 21.0, -8.0, -6.0]), "{extent:?}");
    assert_eq!(drills, [[20.0, -10.0]]);
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
        faces: vec![solidcraft_kernel::FacePaint { face: 0, color: [0.0, 0.0, 1.0], opacity: 1.0, name: None }],
        name: None,
    }));
    let meshes = model_body_meshes("part", &painted, Some([1.0, 0.0, 0.0])).expect("model meshes");
    assert_eq!(meshes.len(), 2, "{:?}", meshes.iter().map(|m| m.color).collect::<Vec<_>>());
    let blue = meshes.iter().find(|m| m.color == Some([0.0, 0.0, 1.0])).expect("the styled face");
    let red = meshes.iter().find(|m| m.color == Some([1.0, 0.0, 0.0])).expect("the body colour");
    assert!(!blue.triangles.is_empty() && !red.triangles.is_empty());
    let total: usize = meshes.iter().map(|m| m.triangles.len() / 3).sum();
    assert_eq!(total, painted.mesh(&solidcraft_kernel::MeshTolerance::chord(0.1)).expect("tessellation").triangles.len());
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

/// A pour with a square hole, as KiCad saves a fill: one outline that runs in to the hole along a
/// zero-width bridge, round the hole and back out along the same line.
const BRIDGED_FILL: &str = r#"(kicad_pcb (general (thickness 1.6))
  (gr_rect (start 0 0) (end 20 20) (layer "Edge.Cuts"))
  (zone (net 1) (net_name "GND") (layer "F.Cu")
    (filled_polygon (layer "F.Cu") (pts (xy 2 2) (xy 18 2) (xy 18 18) (xy 2 18) (xy 2 10)
      (xy 8 10) (xy 8 12) (xy 12 12) (xy 12 8) (xy 8 8) (xy 8 10) (xy 2 10)))))"#;

#[test]
fn a_fill_s_bridges_to_its_holes_are_taken_out() {
    let board = parse_kicad_board(BRIDGED_FILL.as_bytes(), "bridged.kicad_pcb").unwrap();
    let [zone] = board.zones.as_slice() else { panic!("one zone") };
    // The outline and the hole, each a loop of its own, and no edge run both ways.
    let mut sizes: Vec<usize> = zone.contours.iter().map(Vec::len).collect();
    sizes.sort_unstable();
    assert_eq!(sizes, vec![4, 4], "{:?}", zone.contours);
    // Drawn, the pour's open edges are the outline and the hole's, not the bridge as well.
    let options = KicadOptions { copper: KicadCopper::Overlay, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(BRIDGED_FILL.as_bytes(), "bridged.kicad_pcb", &options, &mut load).unwrap();
    let solidcraft_doc::FeatureKind::MeshImport { meshes, .. } = &imported.feature.kind else { panic!("expected a mesh import") };
    let mesh = meshes.iter().find(|m| m.name == "Copper F.Cu").expect("the pour");
    let points = mesh.positions.as_chunks::<3>().0;
    let mut uses: HashMap<(u32, u32), i32> = HashMap::new();
    for [a, b, c] in mesh.triangles.as_chunks::<3>().0.iter().copied() {
        for (p, q) in [(a, b), (b, c), (c, a)] {
            *uses.entry((p.min(q), p.max(q))).or_default() += 1;
        }
    }
    let open: f64 = uses
        .iter()
        .filter(|(_, n)| **n == 1)
        .map(|((p, q), _)| {
            let (p, q) = (points[*p as usize], points[*q as usize]);
            ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt()
        })
        .sum();
    assert!((open - (64.0 + 16.0)).abs() < 1e-6, "open edges {open} mm: the bridge is drawn");
}

/// Two holes, the second bridged on from the first, as KiCad's fill joins neighbouring holes.
const CHAINED_FILL: &str = r#"(kicad_pcb (general (thickness 1.6))
  (gr_rect (start 0 0) (end 20 20) (layer "Edge.Cuts"))
  (zone (net 1) (net_name "GND") (layer "F.Cu")
    (filled_polygon (layer "F.Cu") (pts (xy 2 2) (xy 18 2) (xy 18 18) (xy 2 18) (xy 2 6) (xy 4 6) (xy 4 8) (xy 8 8)
      (xy 8 6) (xy 12 6) (xy 12 8) (xy 16 8) (xy 16 4) (xy 12 4) (xy 12 6) (xy 8 6) (xy 8 4) (xy 4 4) (xy 4 6) (xy 2 6)))))"#;

#[test]
fn a_fill_s_holes_bridged_one_to_the_next_are_each_a_loop() {
    let board = parse_kicad_board(CHAINED_FILL.as_bytes(), "chained.kicad_pcb").unwrap();
    let [zone] = board.zones.as_slice() else { panic!("one zone") };
    let mut sizes: Vec<usize> = zone.contours.iter().map(Vec::len).collect();
    sizes.sort_unstable();
    assert_eq!(sizes, vec![4, 4, 4], "the outline and two holes: {:?}", zone.contours);
}

#[test]
fn a_hatched_fill_s_thousands_of_holes_are_taken_apart_in_one_pass() {
    // One pour, 2500 holes each bridged from its bottom edge: 17,500 points in one polygon.
    const HOLES: usize = 2500;
    let width = 2.0 * HOLES as f64 + 1.0;
    let mut pts = String::from("(xy 0 0)");
    for i in 0..HOLES {
        let (x0, x1) = (2.0 * i as f64 + 0.5, 2.0 * i as f64 + 1.5);
        pts += &format!(" (xy {x0} 0) (xy {x0} 1) (xy {x0} 2) (xy {x1} 2) (xy {x1} 1) (xy {x0} 1) (xy {x0} 0)");
    }
    pts += &format!(" (xy {width} 0) (xy {width} 3) (xy 0 3)");
    let text = format!(
        r#"(kicad_pcb (general (thickness 1.6)) (gr_rect (start -1 -1) (end {} 4) (layer "Edge.Cuts"))
  (zone (net 1) (net_name "GND") (layer "F.Cu") (filled_polygon (layer "F.Cu") (pts {pts}))))"#,
        width + 1.0
    );
    let board = parse_kicad_board(text.as_bytes(), "hatched.kicad_pcb").unwrap();
    let [zone] = board.zones.as_slice() else { panic!("one zone") };
    assert_eq!(zone.contours.len(), HOLES + 1, "the outline and every hole");
    assert!(zone.contours.iter().all(|c| c.len() == 4), "each a plain rectangle");
    let options = KicadOptions { copper: KicadCopper::Overlay, components: false, silkscreen: false, ..Default::default() };
    let mut load = |_: &str| Err("not used".to_string());
    let imported = read_kicad(text.as_bytes(), "hatched.kicad_pcb", &options, &mut load).unwrap();
    assert!(imported.feature.warnings.is_empty(), "{:?}", imported.feature.warnings);
}
