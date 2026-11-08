//! Claas Kuhnen's Discord comparison of 30 October 2026, supplied by the user.
//!
//! The requested outer top-rim radius was 70 mm. STEP does not retain the command's
//! edge selection; these regressions select all four convex outer arcs. The source
//! operand has failed tolerance and geometry reports. A separate exact reconstruction
//! must pass whole-body validation before/after R70 and after STEP roundtrips.

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::vector2d::Point2;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, face::p2 as face, vertex, Brep, EdgeId};
use solidcore::Kernel;

const TOP: f64 = 458.266019821167;
const CYLINDER_RADIUS: f64 = 304.8;
const RADIUS: f64 = 70.0;

fn operand() -> Result<(Kernel, Brep, Vec<EdgeId>), Box<dyn std::error::Error>> {
    let model = step_import::import(
        include_bytes!("fixtures/discord-2026-10-30-fillet.step"),
        &step_import::Limits::default(),
    )?;
    assert_eq!(model.bodies.len(), 1);
    assert!(model.skipped.is_empty());
    let built = step_brep::build_body(
        model.bodies.first().ok_or("missing body")?,
        step_brep::BuildOptions::default(),
    )?;
    assert!(built.face_failures.is_empty());
    let b = built.brep;
    let k = Kernel::default();
    let mut selected = Vec::new();
    for e in b.edges().ids() {
        let (Some(a), Some(z)) = edge::start_end_vertices(&b, e)? else {
            continue;
        };
        let a = vertex::point(&b, a)?;
        let z = vertex::point(&b, z)?;
        if (a.z - TOP).abs() > 1e-5 || (z.z - TOP).abs() > 1e-5 {
            continue;
        }
        let curve = edge::curve(&b, e)?.ok_or("missing edge curve")?;
        let interval = edge::interval(&b, e)?;
        let mid = curve.evaluate_point(&k, interval.evaluate(0.5))?;
        if ((mid.x * mid.x + mid.y * mid.y).sqrt() - CYLINDER_RADIUS).abs() < 1e-3 {
            selected.push(e);
        }
    }
    assert_eq!(selected.len(), 4);
    Ok((k, b, selected))
}

fn round(
    k: &mut Kernel,
    b: &mut Brep,
    edges: &[EdgeId],
) -> Result<(solidcore::Result<()>, i32), Box<dyn std::error::Error>> {
    let mut session = FilletSession::new(k, b)?;
    session.prepare_per_edge_constant_radius(
        k,
        edges,
        &vec![RADIUS; edges.len()],
        &vec![FilletSurfaceGeneratorType::Circular; edges.len()],
        1,
        1.0,
    )?;
    let result = session.do_fillet(k);
    Ok((result, session.error_info().code))
}

#[test]
fn exact_classes_build_four_rounds_on_the_expected_seventy_mm_torus() {
    let (mut k, mut b, edges) = operand().unwrap();
    k.fixes.fillet_exact_round_classes = true;
    let input_check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(b.faces().ids().all(|f| !matches!(
        face::surface(&b, f).unwrap().unwrap().kind(),
        SurfaceKind::SurfOfRevolution(_) | SurfaceKind::Torus(_)
    )));
    round(&mut k, &mut b, &edges).unwrap().0.unwrap();
    let mut count = 0;
    for f in b.faces().ids() {
        let s = face::surface(&b, f).unwrap().unwrap();
        if !matches!(
            s.kind(),
            SurfaceKind::SurfOfRevolution(_) | SurfaceKind::Torus(_)
        ) {
            continue;
        }
        let d = s.natural_uv_domain_with_nurb(&k).unwrap();
        for i in 0..=10 {
            for j in 0..=10 {
                let u = d.min().x + (d.max().x - d.min().x) * f64::from(i) / 10.0;
                let v = d.min().y + (d.max().y - d.min().y) * f64::from(j) / 10.0;
                let p = s.evaluate_point(&k, Point2::new(u, v)).unwrap();
                let rho = (p.x * p.x + p.y * p.y).sqrt();
                let distance = ((rho - (CYLINDER_RADIUS - RADIUS)).powi(2)
                    + (p.z - (TOP - RADIUS)).powi(2))
                .sqrt();
                assert!(
                    (distance - RADIUS).abs() < 1e-9,
                    "round {f:?} is off its torus: {distance}"
                );
            }
        }
        count += 1;
    }
    assert_eq!(count, 4);
    let output_check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(
        !input_check.is_valid(),
        "source invalidity must remain visible"
    );
    assert!(
        !output_check.is_valid(),
        "operation success does not repair the source"
    );
    eprintln!(
        "whole-body validation failures: input={}, output={}; surface samples are not a validity certificate",
        input_check.failure_count(), output_check.failure_count()
    );
}

fn require_valid(k: &mut Kernel, b: &mut Brep) -> Result<(), Box<dyn std::error::Error>> {
    let report = check_brep(k, b, CheckLevel::Level2)?;
    assert!(report.is_valid(), "{:?}", report.lines());
    Ok(())
}

fn round_selected(
    k: &mut Kernel,
    b: &mut Brep,
    radius: f64,
    select: impl Fn(solidcore::math::Point3) -> bool,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut edges = Vec::new();
    for e in b.edges().ids() {
        let curve = edge::curve(b, e)?.ok_or("missing edge curve")?;
        if select(curve.evaluate_point(k, edge::interval(b, e)?.evaluate(0.5))?) {
            edges.push(e);
        }
    }
    let mut session = FilletSession::new(k, b)?;
    session.prepare_per_edge_constant_radius(
        k,
        &edges,
        &vec![radius; edges.len()],
        &vec![FilletSurfaceGeneratorType::Circular; edges.len()],
        1,
        1.0,
    )?;
    session.do_fillet(k)?;
    Ok(edges.len())
}

fn validated_roundtrip(b: &Brep) -> Result<Brep, Box<dyn std::error::Error>> {
    let exported = step_export::write(&step_brep::brep_to_model(b)?)?;
    let parsed = step_import::import(exported.as_bytes(), &step_import::Limits::default())?;
    assert!(parsed.skipped.is_empty());
    let body = parsed.bodies.first().ok_or("missing exported body")?;
    let built = step_brep::build_body(body, step_brep::BuildOptions::default())?;
    assert!(built.face_failures.is_empty());
    let mut read = built.brep;
    assert_eq!(read.face_list().len(), b.face_list().len());
    assert_eq!(read.edge_list().len(), b.edge_list().len());
    assert_eq!(read.vertex_list().len(), b.vertex_list().len());
    require_valid(&mut Kernel::default(), &mut read)?;
    Ok(read)
}

/// Rebuild the source's analytic dimensions, replacing its inconsistent exported
/// lower blends with exact circular sections. This is deliberately a test recipe,
/// not an import repair or a special-case kernel construction.
#[test]
fn exact_reconstruction_completes_r70_and_validates_through_step(
) -> Result<(), Box<dyn std::error::Error>> {
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::math::Point3;
    use solidcore::primitive::api_primitives::create_cylinder;

    let model = step_import::import(
        include_bytes!("fixtures/discord-2026-10-30-fillet.step"),
        &step_import::Limits::default(),
    )?;
    let source = model.bodies.first().ok_or("missing source body")?;
    let cut_radius = 89.38861960394327;
    let floor = 189.75418806267385;
    let centers: Vec<_> = source
        .faces
        .iter()
        .filter_map(|f| match &f.surface {
            step_import::model::Surface::Cylinder { frame, radius }
                if (*radius - cut_radius).abs() < 1e-9 =>
            {
                Some((frame.origin[0], frame.origin[1]))
            }
            _ => None,
        })
        .collect();
    assert_eq!(centers.len(), 4);
    let mut k = Kernel::default();
    k.fixes.fillet_exact_round_classes = true;
    k.fixes.fillet_exact_arc_sections = true;
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), CYLINDER_RADIUS, TOP)?;
    for &(x, y) in &centers {
        let tool = create_cylinder(
            &mut k,
            Point3::new(x, y, floor),
            cut_radius,
            TOP - floor + 1.0,
        )?;
        b = boolean_difference(&mut k, b, tool)?;
        require_valid(&mut k, &mut b)?;
    }
    let count = round_selected(&mut k, &mut b, 241.50994792771928 - floor, |p| {
        (p.z - floor).abs() < 1e-6
            && centers.iter().any(|&(x, y)| {
                (((p.x - x).powi(2) + (p.y - y).powi(2)).sqrt() - cut_radius).abs() < 1e-6
            })
    })?;
    assert_eq!(count, 5, "one notch crosses the cylinder seam");
    require_valid(&mut k, &mut b)?;
    // Exercise the actual imported operand, not just its in-memory counterpart.
    b = validated_roundtrip(&b)?;
    assert_eq!(
        round_selected(&mut k, &mut b, RADIUS, |p| {
            (p.z - TOP).abs() < 1e-6
                && ((p.x * p.x + p.y * p.y).sqrt() - CYLINDER_RADIUS).abs() < 1e-6
        })?,
        4
    );
    require_valid(&mut k, &mut b)?;
    validated_roundtrip(&b)?;
    Ok(())
}

#[test]
fn cylindrical_cut_preserves_inward_normals_through_step() -> Result<(), Box<dyn std::error::Error>>
{
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::math::Point3;
    use solidcore::primitive::api_primitives::create_cylinder;

    for (x, z, height) in [(10.0, 5.0, 6.0), (10.0, -1.0, 12.0), (0.0, -1.0, 12.0)] {
        let mut k = Kernel::default();
        let outer = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0)?;
        let tool = create_cylinder(&mut k, Point3::new(x, 0.0, z), 3.0, height)?;
        let mut b = boolean_difference(&mut k, outer, tool)?;
        require_valid(&mut k, &mut b)?;
        validated_roundtrip(&b)?;
    }
    Ok(())
}
