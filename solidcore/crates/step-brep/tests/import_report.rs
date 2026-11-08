//! Import reports preserve mapper findings and the built body's geometry and topology.

use solidcore::topology::Brep;
use solidcore::topology::{edge, vertex};
use step_brep::import_with_report;
use step_import::Limits;

const TRIANGLE: &str = "#6=FACE('',(#7));#7=FACE_OUTER_BOUND('',#8,.T.);#8=POLY_LOOP('',(#9,#10,#11));#9=CARTESIAN_POINT('',(0.,0.,0.));#10=CARTESIAN_POINT('',(3.,0.,0.));#11=CARTESIAN_POINT('',(0.,4.,0.));#5=SHELL_BASED_SURFACE_MODEL('',(#13));#13=OPEN_SHELL('',(#6));#20=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#21=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#20)) REPRESENTATION_CONTEXT('',''));#22=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#5),#21);";
const UNSUPPORTED: &str = "#101=MANIFOLD_SOLID_BREP('',#102);#102=CLOSED_SHELL('',(#103));#103=ADVANCED_FACE('',(),#104,.T.);#104=OFFSET_SURFACE('',#105,1.,.F.);";
const BUILD_FAIL: &str = "#201=MANIFOLD_SOLID_BREP('',#202);#202=CLOSED_SHELL('',(#203));#203=ADVANCED_FACE('',(),#204,.T.);#204=SPHERICAL_SURFACE('',#205,1.E-20);#205=AXIS2_PLACEMENT_3D('',#206,$,$);#206=CARTESIAN_POINT('',(0.,0.,0.));";

fn input(data: &str) -> Vec<u8> {
    format!("ISO-10303-21;HEADER;FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));ENDSEC;DATA;{data}ENDSEC;END-ISO-10303-21;").into_bytes()
}

#[derive(Debug, PartialEq)]
struct BodySummary {
    counts: (usize, usize, usize),
    vertices: Vec<[f64; 3]>,
    edge_endpoints: Vec<(usize, usize)>,
}

fn summary(bodies: &[Brep]) -> Result<Vec<BodySummary>, Box<dyn std::error::Error>> {
    bodies
        .iter()
        .map(|b| {
            let vertices = b
                .vertex_list()
                .iter()
                .map(|&id| {
                    let point = vertex::point(b, id)?;
                    Ok([point.x, point.y, point.z])
                })
                .collect::<solidcore::Result<Vec<_>>>()?;
            // Compare topology by list positions, not arena IDs from separate builds.
            let edge_endpoints = b
                .edge_list()
                .iter()
                .map(|&id| {
                    let (start, end) = edge::start_end_vertices(b, id)?;
                    let position = |id| {
                        b.vertex_list()
                            .iter()
                            .position(|&v| Some(v) == id)
                            .ok_or("edge endpoint missing from vertex list")
                    };
                    Ok((position(start)?, position(end)?))
                })
                .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
            Ok(BodySummary {
                counts: (
                    b.face_list().len(),
                    b.edge_list().len(),
                    b.vertex_list().len(),
                ),
                vertices,
                edge_endpoints,
            })
        })
        .collect()
}

fn findings(
    data: &str,
    expected: &[(Option<u64>, &str)],
    skip_ids: &[u64],
) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = input(data);
    let model = step_import::import(&bytes, &Limits::default())?;
    let report = import_with_report(&bytes, &Limits::default())?;
    let diagnostics = |ds: &[step_import::model::Diagnostic]| {
        ds.iter()
            .map(|d| (d.step_id, d.message.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        diagnostics(&report.diagnostics),
        diagnostics(&model.diagnostics)
    );
    assert_eq!(
        diagnostics(&report.diagnostics),
        expected
            .iter()
            .map(|(id, s)| (*id, (*s).to_string()))
            .collect::<Vec<_>>()
    );
    let skipped = |ss: &[step_import::model::Skipped]| {
        ss.iter()
            .map(|s| (s.step_id, s.kind.clone(), s.reason.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(skipped(&report.skipped), skipped(&model.skipped));
    assert_eq!(
        skipped(&report.skipped),
        skip_ids
            .iter()
            .map(|&id| (
                id,
                "MANIFOLD_SOLID_BREP".to_string(),
                "unsupported: OFFSET_SURFACE".to_string()
            ))
            .collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn unsupported_preserves_exact_findings() {
    findings(
        UNSUPPORTED,
        &[
            (
                None,
                "no GLOBAL_UNIT_ASSIGNED_CONTEXT; assuming millimetres/degrees",
            ),
            (
                Some(101),
                "body has no representation context; using the file unit fallback",
            ),
            (Some(104), "dangling reference #105"),
        ],
        &[101],
    )
    .expect("unsupported findings");
    let bytes = input(UNSUPPORTED);
    assert!(import_with_report(&bytes, &Limits::default())
        .expect("report")
        .bodies
        .is_empty());
    assert!(step_brep::import(&bytes, &Limits::default())
        .expect("legacy")
        .is_empty());
}

#[test]
fn body_free_product_retains_diagnostic() {
    let data = "#1=PRODUCT('a','b','',());";
    findings(
        data,
        &[(
            None,
            "no GLOBAL_UNIT_ASSIGNED_CONTEXT; assuming millimetres/degrees",
        )],
        &[],
    )
    .expect("body-free findings");
    assert!(import_with_report(&input(data), &Limits::default())
        .expect("report")
        .bodies
        .is_empty());
    assert!(step_brep::import(&input(data), &Limits::default())
        .expect("legacy")
        .is_empty());
}

#[test]
fn supported_body_builds_and_mixed_retains_same_topology() {
    findings(TRIANGLE, &[], &[]).expect("supported findings");
    let mixed = format!("{TRIANGLE}{UNSUPPORTED}");
    findings(
        &mixed,
        &[
            (
                Some(101),
                "body has no representation context; using the file unit fallback",
            ),
            (Some(104), "dangling reference #105"),
        ],
        &[101],
    )
    .expect("mixed findings");
    let report = import_with_report(&input(TRIANGLE), &Limits::default()).expect("supported");
    let mixed_report = import_with_report(&input(&mixed), &Limits::default()).expect("mixed");
    assert_eq!(report.bodies.len(), 1);
    assert!(!report.bodies[0].face_list().is_empty());
    let control = summary(&report.bodies).expect("supported structure");
    let triangle = control.first().expect("built triangle");
    assert_eq!(triangle.counts, (1, 3, 3));
    assert!(triangle.vertices.iter().flatten().all(|v| v.is_finite()));
    let [a, b, c] = triangle.vertices.as_slice() else {
        panic!("expected three built vertex positions");
    };
    assert_ne!(
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]),
        0.0,
        "built triangle must have non-collinear vertices"
    );
    assert_eq!(triangle.edge_endpoints.len(), 3);
    for &(start, end) in &triangle.edge_endpoints {
        assert_ne!(start, end);
    }
    // Edge geometry orientation is independent of its use around the face.
    for vertex in 0..triangle.vertices.len() {
        assert_eq!(
            triangle
                .edge_endpoints
                .iter()
                .filter(|&&(start, end)| start == vertex || end == vertex)
                .count(),
            2
        );
    }
    assert_eq!(
        summary(&mixed_report.bodies).expect("mixed structure"),
        control
    );
    assert_eq!(
        summary(
            &step_brep::import(&input(TRIANGLE), &Limits::default()).expect("legacy supported")
        )
        .expect("legacy supported structure"),
        control
    );
    assert_eq!(
        summary(&step_brep::import(&input(&mixed), &Limits::default()).expect("legacy mixed"))
            .expect("legacy mixed structure"),
        control
    );
}

#[test]
fn malformed_parse_error_matches_legacy() {
    let bytes = b"\x00\xff garbage";
    let error = import_with_report(bytes, &Limits::default())
        .err()
        .expect("parse error");
    assert_eq!(error.message, "line 1: unexpected character 0x00");
    assert_eq!(
        error.message,
        step_brep::import(bytes, &Limits::default())
            .expect_err("legacy parse error")
            .message
    );
}

#[test]
fn mapped_builder_failure_is_not_a_skip_or_partial_success() {
    for data in [
        BUILD_FAIL.to_string(),
        format!("{TRIANGLE}{BUILD_FAIL}{UNSUPPORTED}"),
    ] {
        let bytes = input(&data);
        let model = step_import::import(&bytes, &Limits::default()).expect("mapped input");
        let failed = model
            .bodies
            .iter()
            .find(|b| b.step_id == 201)
            .expect("mapped sphere");
        assert!(!model.skipped.iter().any(|s| s.step_id == 201));
        let direct = step_brep::build(failed).expect_err("genuine builder failure");
        assert_eq!(
            direct.message,
            "surface::sphere::Sphere::create_canonical: upstream status 1001"
        );
        let error = import_with_report(&bytes, &Limits::default())
            .err()
            .expect("no partial success");
        assert_eq!(error.message, direct.message);
        assert_eq!(
            error.message,
            step_brep::import(&bytes, &Limits::default())
                .expect_err("legacy build failure")
                .message
        );
    }
}
