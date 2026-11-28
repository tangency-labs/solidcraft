use super::*;
use crate::history::BodyHistory;

/// `p` moved by the column-major affine `matrix`.
fn moved(matrix: [[f64; 4]; 4], p: [f64; 3]) -> Vec3 {
    Vec3::new(
        matrix[0][0] * p[0] + matrix[1][0] * p[1] + matrix[2][0] * p[2] + matrix[3][0],
        matrix[0][1] * p[0] + matrix[1][1] * p[1] + matrix[2][1] * p[2] + matrix[3][1],
        matrix[0][2] * p[0] + matrix[1][2] * p[1] + matrix[2][2] * p[2] + matrix[3][2],
    )
}

/// A rigid placement moves the record and nothing else: the same regions, shells, faces' loops
/// (edges, directions and pcurves: each surface moves with its parameters), edges' ends and uses;
/// every vertex moved by the motion; and the history the identity's.
fn assert_topology(source: &Body, placed: &Body, matrix: [[f64; 4]; 4]) {
    let (a, b) = (source.record(), placed.record());
    assert_eq!(a.regions, b.regions);
    assert_eq!(a.shells, b.shells);
    assert_eq!(a.faces.len(), b.faces.len());
    for (fa, fb) in a.faces.iter().zip(&b.faces) {
        assert_eq!(fa.loops, fb.loops);
        assert_eq!(fa.domain, fb.domain);
    }
    assert_eq!(a.edges.len(), b.edges.len());
    for (ea, eb) in a.edges.iter().zip(&b.edges) {
        assert_eq!((ea.vertices, &ea.uses, ea.interval), (eb.vertices, &eb.uses, eb.interval));
    }
    assert_eq!(a.vertices.len(), b.vertices.len());
    for (va, vb) in a.vertices.iter().zip(&b.vertices) {
        assert!(Vec3::from(vb.point).dist(moved(matrix, va.point)) < 1e-9);
    }
    assert_eq!(placed.history(), Some(&<crate::history::History as BodyHistory>::identity(source)));
}

/// A cylinder whose wall is one face with a seam: the seam edge is used twice by the wall's loop.
fn periodic_cylinder() -> Body {
    crate::cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap()
}

#[test]
fn periodic_step_seam_survives_identity_rotation_and_translation() {
    let source = periodic_cylinder();
    let bytes = crate::step_export(&[&source], "placement regression").unwrap();
    let imported = crate::step_import(&bytes).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let body = &imported.bodies.first().unwrap().body;
    // A face whose loop uses an edge twice (the seam).
    let seamed = body.record().faces.iter().any(|f| {
        f.loops.iter().any(|l| match l {
            solidcore_record::brep::Loop::Edges { edges } => edges.iter().any(|u| edges.iter().filter(|w| w.edge == u.edge).count() > 1),
            solidcore_record::brep::Loop::Vertex { .. } => false,
        })
    });
    assert!(seamed);
    let unchanged = crate::step_export(&[body], "source snapshot").unwrap();
    let before = body.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap().measure().volume;
    assert!((before - 2000. * std::f64::consts::PI).abs() < 1.);
    for matrix in [
        [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]],
        [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [10., 10., 0., 1.]],
        [[0., 1., 0., 0.], [-1., 0., 0., 0.], [0., 0., 1., 0.], [10., 10., 30., 1.]],
    ] {
        let moved = transform_matrix(body, matrix).unwrap();
        assert_topology(body, &moved, matrix);
        let after = moved.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap().measure().volume;
        assert!((after - before).abs() < 1e-5, "{after} vs {before}");
        let roundtrip = crate::step_import(&crate::step_export(&[&moved], "placement regression").unwrap()).unwrap();
        assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
        assert!((roundtrip.bodies.first().unwrap().body.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap().measure().volume - before).abs() < 1e-4);
    }
    let rigid = transform(body, Vec3::new(10., 10., 30.), Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2).unwrap();
    assert_topology(body, &rigid, [[0., 1., 0., 0.], [-1., 0., 0., 0.], [0., 0., 1., 0.], [10., 10., 30., 1.]]);
    assert_eq!(crate::step_export(&[body], "source snapshot").unwrap(), unchanged);
}

/// A body mirrored in SMLib reads back from its record as the same valid solid: a plate with a
/// through hole (the hole's cylinder comes out inside out) and a box with two rounds meeting at a
/// corner (their intersection edge has a tolerance of its own, wider than the body's). Read back
/// from the record, the hole's cylinder swept the wrong way about its axis 30 mm off its NURBS,
/// and the rounds' shared edge lost its tolerance to the body's.
#[test]
fn mirrored_bodies_read_back_from_their_records_valid() {
    let plate = crate::box_solid(Vec3::ZERO, Vec3::new(30., 40., 20.)).unwrap();
    let hole = crate::cylinder(Vec3::new(20., 20., -5.), Vec3::Z, 5.0, 30.0).unwrap();
    let holed = crate::boolean(&plate, &hole, crate::BoolOp::Cut).unwrap().unwrap();
    let rounded = crate::fillet(&plate, &[Vec3::new(15., 0., 20.), Vec3::new(0., 0., 10.)], 3.0).unwrap();
    // The mirror in x = 30.
    let mirror = [[-1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [60., 0., 0., 1.]];
    for (what, body) in [("holed plate", holed), ("rounded box", rounded)] {
        let mirrored = transform_matrix(&body, mirror).unwrap();
        let read_back = Body::from_record(mirrored.record().clone()).unwrap();
        assert!(matches!(crate::validity(&read_back).smlib, Some(crate::SmlibCheck::Valid)), "{what}: {:?}", crate::validity(&read_back).smlib);
        let (want, got) = (crate::measure(&body).unwrap().volume, crate::measure(&read_back).unwrap().volume);
        assert!((got - want).abs() < 1e-6 * want, "{what}: volume {got}, expected {want}");
    }
}

/// A long body mirrored in SMLib measures as its source does, exactly. SMLib's mirror gives each
/// line as a B-spline over the line's whole parameter range (control points 1,234,567 edge
/// lengths out): a box 50 x 900 x 50 mirrored in x = 0 could not be measured ("invalid B-rep
/// record: flat arrays", past 1e9 from an edge of 810 mm; #14413). Every straight edge comes
/// back as the line between its vertices.
#[test]
fn a_long_mirrored_body_measures_as_its_source() {
    let mirror = [[-1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];
    for (what, corner, size) in [
        ("short box", Vec3::new(100., 0., 0.), Vec3::new(50., 500., 50.)),
        ("box along y", Vec3::new(100., 0., 0.), Vec3::new(50., 900., 50.)),
        ("box along y, far from the plane", Vec3::new(3000., -450., 0.), Vec3::new(50., 1100., 50.)),
        ("box along x", Vec3::new(100., 0., 0.), Vec3::new(2000., 50., 50.)),
        ("box along z", Vec3::new(100., 0., 0.), Vec3::new(50., 50., 2000.)),
    ] {
        let body = crate::box_solid(corner, corner + size).unwrap();
        let mirrored = transform_matrix(&body, mirror).unwrap();
        assert!(
            mirrored.record().edges.iter().all(|e| matches!(e.curve, solidcore_record::brep::Curve::Line { .. }) && e.nurbs.is_none()),
            "{what}: a straight edge is not a line"
        );
        let (want, got) = (crate::measure(&body).unwrap(), crate::measure(&mirrored).unwrap_or_else(|e| panic!("{what}: {e}")));
        assert!((got.volume - want.volume).abs() < 1e-9 * want.volume, "{what}: volume {}, expected {}", got.volume, want.volume);
        let (lo, hi) = (got.bbox.min, got.bbox.max);
        let (wlo, whi) = (want.bbox.min, want.bbox.max);
        let expected = (Vec3::new(-whi.x, wlo.y, wlo.z), Vec3::new(-wlo.x, whi.y, whi.z));
        assert!(lo.dist(expected.0) < 1e-9 && hi.dist(expected.1) < 1e-9, "{what}: box {lo:?}..{hi:?}, expected {expected:?}");
        let read_back = Body::from_record(mirrored.record().clone()).unwrap();
        assert!(matches!(crate::validity(&read_back).smlib, Some(crate::SmlibCheck::Valid)), "{what}: {:?}", crate::validity(&read_back).smlib);
    }
}

/// A cylinder mirrored in x = 0 keeps its two end circles as circles of the same radius, centred
/// on the mirrored axis, not as the rational B-splines of their NURBs (WR-21139); its volume is
/// the source's, pi * 10^2 * 100, and it reads back valid.
#[test]
fn a_mirrored_cylinder_keeps_its_circles() {
    let mirror = [[-1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];
    let body = crate::cylinder(Vec3::new(50., 0., 0.), Vec3::Z, 10.0, 100.0).unwrap();
    let mirrored = transform_matrix(&body, mirror).unwrap();
    let mut circles = 0;
    for e in &mirrored.record().edges {
        match &e.curve {
            solidcore_record::brep::Curve::Line { .. } => {}
            solidcore_record::brep::Curve::Circle { frame, radius } => {
                circles += 1;
                assert!((radius - 10.).abs() < 1e-12, "radius {radius}");
                let [x, y, _] = frame.origin;
                assert!((x + 50.).abs() < 1e-12 && y.abs() < 1e-12, "centre {:?}", frame.origin);
            }
            other => panic!("a curved edge of the mirrored cylinder is not a circle: {other:?}"),
        }
    }
    assert_eq!(circles, 2);
    let want = std::f64::consts::PI * 100. * 100.;
    let got = crate::measure(&mirrored).unwrap().volume;
    assert!((got - want).abs() < 1e-9 * want, "volume {got}, expected {want}");
    let read_back = Body::from_record(mirrored.record().clone()).unwrap();
    assert!(matches!(crate::validity(&read_back).smlib, Some(crate::SmlibCheck::Valid)), "{:?}", crate::validity(&read_back).smlib);
}

/// A scaling or shear of a box maps its record as a rigid motion does: its faces stay planes, on
/// the same parameters, and the volume is the determinant's (#14422). A reflection, or a body
/// with a cylinder, still goes through SMLib and keeps the right volume.
#[test]
fn a_box_scaled_or_sheared_keeps_its_planes() {
    let planes = |b: &Body| b.record().faces.iter().all(|f| matches!(f.surface, solidcore_record::brep::Surface::Plane { .. }));
    let volume = |b: &Body| b.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap().measure().volume;
    let source = crate::box_solid(Vec3::ZERO, Vec3::new(10., 10., 10.)).unwrap();
    assert!(planes(&source));
    for (matrix, want) in [
        ([[2., 0., 0., 0.], [0., 3., 0., 0.], [0., 0., 0.5, 0.], [0., 0., 0., 1.]], 3000.),
        ([[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1.0001, 0.], [5., 0., 0., 1.]], 1000.1),
        ([[1., 0., 0., 0.], [0.5, 1., 0., 0.], [0., 0., 2., 0.], [0., 0., 0., 1.]], 2000.),
    ] {
        let scaled = transform_matrix(&source, matrix).unwrap();
        assert_topology(&source, &scaled, matrix);
        assert!(planes(&scaled));
        assert!((volume(&scaled) - want).abs() < 1e-6, "{} vs {want}", volume(&scaled));
        assert!(scaled.record().tolerance >= source.record().tolerance);
    }
    let mirrored = transform_matrix(&source, [[-2., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]]).unwrap();
    assert!((volume(&mirrored) - 2000.).abs() < 1e-6);
    let stretched = transform_matrix(&periodic_cylinder(), [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 2., 0.], [0., 0., 0., 1.]]).unwrap();
    assert!((volume(&stretched) - 4000. * std::f64::consts::PI).abs() < 5., "{}", volume(&stretched));
}

/// The kind of each blend face's definition: "procedural" or "exact".
fn blend_kinds(body: &Body) -> Vec<&'static str> {
    use solidcore_record::brep::{Surface, blend::BlendDefinition};
    body.record()
        .faces
        .iter()
        .filter_map(|f| match &f.surface {
            Surface::Blend { definition } => Some(match definition.as_ref() {
                BlendDefinition::Procedural(_) => "procedural",
                BlendDefinition::ExactRound(_) => "exact",
            }),
            _ => None,
        })
        .collect()
}

/// Rounded bodies, by what their rounds are: a cylinder's rim (the 900-copy pattern's part), a
/// box's edge, a boss on a plate, and a round where a slanted plane cuts a cylinder.
fn rounded_bodies() -> Vec<(&'static str, Body)> {
    let mut out = Vec::new();
    let rim = crate::cylinder(Vec3::ZERO, Vec3::Z, 3.0, 6.0).unwrap();
    out.push(("cylinder rim", crate::fillet(&rim, &[Vec3::new(3.0, 0.0, 6.0)], 1.0).unwrap()));
    let block = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
    out.push(("box edge", crate::fillet(&block, &[Vec3::new(5.0, 0.0, 10.0)], 2.0).unwrap()));
    let plate = crate::box_solid(Vec3::new(-10.0, -10.0, -4.0), Vec3::new(10.0, 10.0, 0.0)).unwrap();
    let boss = crate::cylinder(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, 4.0, 7.0).unwrap();
    let bossed = boolean(&plate, &boss, BoolOp::Union).unwrap().unwrap();
    out.push(("boss on a plate", crate::fillet(&bossed, &[Vec3::new(4.0, 0.0, 0.0)], 1.0).unwrap()));
    // Two cylinders meeting at a tee: the round along their quartic edge is of no exact class.
    let post = crate::cylinder(Vec3::ZERO, Vec3::Z, 5.0, 20.0).unwrap();
    let arm = crate::cylinder(Vec3::new(0.0, 0.0, 10.0), Vec3::X, 3.0, 15.0).unwrap();
    let tee = boolean(&post, &arm, BoolOp::Union).unwrap().unwrap();
    let foot = Vec3::new((25.0f64 - 9.0).sqrt(), 0.0, 13.0);
    out.push(("tee", crate::fillet(&tee, &[foot], 1.0).unwrap()));
    let tube = crate::cylinder(Vec3::ZERO, Vec3::Z, 5.0, 20.0).unwrap();
    let slant = solidcore_record::Plane { origin: Vec3::new(0.0, 0.0, 10.0), x: Vec3::X, y: Vec3::new(0.0, 1.0, -0.5).normalized().unwrap() };
    if let Ok(parts) = split_by_plane(&tube, &slant)
        && let Some(lower) = parts.into_iter().find(|p| p.mesh(&crate::MeshTolerance::chord(0.1)).is_ok_and(|m| m.bounds().min.z < 1.0))
        && let Ok(rounded) = crate::fillet(&lower, &[Vec3::new(5.0, 0.0, 10.0)], 1.5)
    {
        out.push(("slanted cut", rounded));
    }
    out
}

/// A rounded body moves as exactly as any other: its rounds' definitions move with their nets
/// (supports, leader and centre; a procedural round's bound takes in the move's rounding), so a
/// copy is made without the kernel's rebuild, the kernel takes it as it is, and it is the body the
/// kernel's own move makes. A pattern of 900 rounded cylinders sent each copy through the kernel.
#[test]
fn a_rounded_body_moves_exactly_as_the_kernel_moves_it() {
    let (c, s) = (0.7f64.cos(), 0.7f64.sin());
    let matrices = [
        [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [290., 290., 0., 1.]],
        [[c, s, 0., 0.], [-s, c, 0., 0.], [0., 0., 1., 0.], [10., -20., 30., 1.]],
    ];
    let mut kinds = Vec::new();
    for (what, body) in rounded_bodies() {
        let found = blend_kinds(&body);
        kinds.extend(found.iter().copied());
        let before = body.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap().measure().volume;
        for m in matrices {
            let motion = crate::placement::Rigid::of_matrix(&m).unwrap();
            assert!(crate::placement::moved(body.record(), &motion).is_some(), "{what} {found:?}: moved exactly");
            let placed = transform_matrix(&body, m).unwrap();
            assert_topology(&body, &placed, m);
            assert_eq!(blend_kinds(&placed), found, "{what}: the rounds stay rounds by their definitions");
            assert!(placed.smlib().is_ok(), "{what}: the kernel takes the moved body: {:?}", placed.smlib_refusal());
            let after = placed.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap().measure().volume;
            assert!((after - before).abs() < 1e-6 * before, "{what}: {after} vs {before}");
            // The kernel's own move gives the same body.
            let kernels = transformed(&body, &m).unwrap();
            for (a, b) in placed.record().vertices.iter().zip(&kernels.record().vertices) {
                assert!(Vec3::from(a.point).dist(Vec3::from(b.point)) < 1e-9, "{what}");
            }
            // It is an operand like any other: cut through a round, as the source is cut.
            let corner = crate::box_solid(Vec3::new(-50.0, -50.0, 3.0), Vec3::new(50.0, 1.0, 50.0)).unwrap();
            let cut = |b: &Body, tool: &Body| {
                boolean(b, tool, BoolOp::Cut).unwrap().unwrap().mesh(&crate::MeshTolerance::chord(1e-3)).unwrap().measure().volume
            };
            let moved_cut = cut(&placed, &transform_matrix(&corner, m).unwrap());
            assert!((moved_cut - cut(&body, &corner)).abs() < 1e-5 * before, "{what}: cut {moved_cut}");
        }
        if found.contains(&"procedural") {
            // The bound takes in the move's rounding: it only widens.
            let bounds = |b: &Body| -> Vec<f64> {
                b.record()
                    .faces
                    .iter()
                    .filter_map(|f| match &f.surface {
                        solidcore_record::brep::Surface::Blend { definition } => match definition.as_ref() {
                            solidcore_record::brep::blend::BlendDefinition::Procedural(d) => Some(d.bound),
                            _ => None,
                        },
                        _ => None,
                    })
                    .collect()
            };
            let placed = transform_matrix(&body, matrices[0]).unwrap();
            assert!(bounds(&body).iter().zip(bounds(&placed)).all(|(a, b)| b > *a), "{what}: {:?} -> {:?}", bounds(&body), bounds(&placed));
        }
        if !found.is_empty() {
            // A rotation only near orthonormal would move the net and its definition apart by
            // more than rounding: that is left to the kernel, as before.
            let mut near = matrices[1];
            near[0][0] *= 1.0 + 1e-12;
            let motion = crate::placement::Rigid::of_matrix(&near).unwrap();
            assert!(crate::placement::moved(body.record(), &motion).is_none(), "{what}");
        }
    }
    assert!(kinds.contains(&"exact") && kinds.contains(&"procedural"), "{kinds:?}");
}

/// A body placed by a rigid motion keeps its part's record until its own geometry is read: its
/// counts, history and face sources come from the part's, and nothing is moved for them. Read,
/// its record is the one an eager move makes. Placed again (a part in a placed subassembly), it
/// moves by both motions at once. A motion orthonormal only to the looser bound is moved now, as
/// before. Moving every placement of a large assembly placed a hundred times took tens of
/// gigabytes.
#[test]
fn a_placed_body_is_moved_only_when_its_geometry_is_read() {
    let source = periodic_cylinder();
    let (c, s) = (0.6f64.cos(), 0.6f64.sin());
    let turn = [[c, s, 0.0, 0.0], [-s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [5.0, -3.0, 2.0, 1.0]];
    let shift = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [100.0, 0.0, 0.0, 1.0]];
    let eager = |m: &[[f64; 4]; 4]| crate::placement::moved(source.record(), &crate::placement::Rigid::of_matrix(m).unwrap()).unwrap();
    let placed = transform_matrix(&source, turn).unwrap();
    assert!(placed.record.pending(), "placed without moving its record");
    assert_eq!((placed.face_count(), placed.edge_count(), placed.vertex_count()), (source.face_count(), source.edge_count(), source.vertex_count()));
    assert_eq!(placed.history(), Some(&<crate::history::History as BodyHistory>::identity(&source)));
    assert!(placed.record.pending(), "counts and history read the part's record");
    // Its outline (which frames the view while a design opens) is the part's, moved: it holds
    // the moved body and does not move the record.
    let outline = placed.outline_bounds();
    assert!(placed.record.pending(), "the outline does not move the record");
    for v in &eager(&turn).vertices {
        let p = Vec3::from(v.point);
        assert!(p.x >= outline.min.x - 1e-9 && p.x <= outline.max.x + 1e-9 && p.y >= outline.min.y - 1e-9 && p.y <= outline.max.y + 1e-9);
    }
    // Every placement of the part shares one history (a move keeps every face and edge).
    let elsewhere = transform_matrix(&source, shift).unwrap();
    assert!(std::sync::Arc::ptr_eq(placed.history.as_ref().unwrap(), elsewhere.history.as_ref().unwrap()), "one history for every placement");
    // An imported part's face sources hold no plane: its placements share them, unmoved.
    let imported = |face: usize| Some(crate::FaceSource::Imported { item: 1, face: face as u64, payload_sha256: None, producer: None });
    let tagged = source.clone().with_face_sources(Some(std::sync::Arc::new((0..source.face_count()).map(imported).collect())));
    let (a, b) = (transform_matrix(&tagged, turn).unwrap(), transform_matrix(&tagged, shift).unwrap());
    assert!(std::sync::Arc::ptr_eq(a.face_sources.as_ref().unwrap(), b.face_sources.as_ref().unwrap()), "imported sources shared");
    // Placed again: both motions at once, still not moved.
    let twice = transform_matrix(&placed, shift).unwrap();
    assert!(twice.record.pending() && placed.record.pending());
    // Read: exactly the eager move's record.
    let eager = |m: &[[f64; 4]; 4]| crate::placement::moved(source.record(), &crate::placement::Rigid::of_matrix(m).unwrap()).unwrap();
    assert_eq!(*placed.record(), eager(&turn));
    assert_topology(&source, &placed, turn);
    let both: [[f64; 4]; 4] = std::array::from_fn(|i| std::array::from_fn(|j| (0..4).map(|k| turn[i][k] * shift[k][j]).sum()));
    for v in &twice.record().vertices {
        assert!(source.record().vertices.iter().any(|s| Vec3::from(v.point).dist(moved(both, s.point)) < 1e-9), "moved by both motions");
    }
    assert!(placed.lazy_move_made() && twice.lazy_move_made(), "each moved once it was read");
    let volume = |b: &Body| b.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap().measure().volume;
    assert!((volume(&twice) - volume(&source)).abs() < 1e-3 * volume(&source));
    // Orthonormal only to the looser bound (an imported assembly's rotations are, written to
    // a dozen digits): a part with no blend by its definition is still placed lazily, and reads
    // as the eager move; a rounded part is moved now, as before.
    let near = [[1.0 + 1e-12, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [1.0, 0.0, 0.0, 1.0]];
    let later = transform_matrix(&source, near).unwrap();
    assert!(later.record.pending(), "a part with no blend is placed lazily by a near-rigid motion");
    assert_eq!(*later.record(), eager(&near));
    let rounded = rounded_bodies().into_iter().find(|(_, b)| !blend_kinds(b).is_empty()).map(|(_, b)| b).unwrap();
    let now = transform_matrix(&rounded, near).unwrap();
    assert!(!now.record.pending(), "a blend moves with its net only by an exact motion: moved now");
    assert!(transform_matrix(&rounded, shift).unwrap().record.pending(), "and lazily by an exact one");
    // A copy that shows its part's display (an assembly's instance) keeps showing it when it is
    // placed again, placed by both motions, and is meshed from it without moving its record.
    let copy = source.clone().with_display_share(&crate::DisplayShare::new(source.clone()), turn);
    let again = transform_matrix(&copy, shift).unwrap();
    assert!(again.record.pending(), "a copy sharing its part's display is placed lazily");
    let (_, shown) = again.display_share().expect("still shares its part's display");
    assert_eq!(shown, both);
    let _ = again.mesh(&crate::MeshTolerance::chord(1e-2)).unwrap();
    assert!(again.record.pending(), "meshed from the part's display");
}

/// A translation that lands a vertex past `validate_built`'s ceiling (finite, absolute
/// value at most 1e12) is an error. The same cylinder translated by 1e9, or by 10 mm, stays a
/// lazy record: 1e9 plus the radius is still under the ceiling, which is not the 1e9 parameter cap.
#[test]
fn a_translation_past_the_coordinate_ceiling_is_refused() {
    let body = crate::cylinder(Vec3::ZERO, Vec3::Z, 4.0, 8.0).unwrap();
    let shift = |tx: f64| [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [tx, 0.0, 0.0, 1.0]];
    let err = transform_matrix(&body, shift(1e12)).unwrap_err();
    assert!(err.to_string().contains("excessive coordinate"), "{err}");
    let near = transform_matrix(&body, shift(1e9)).unwrap();
    assert!(near.record.pending(), "a 1e9 translation stays lazy");
    let short = transform_matrix(&body, shift(10.0)).unwrap();
    assert!(short.record.pending(), "a 10 mm translation stays lazy");
}
