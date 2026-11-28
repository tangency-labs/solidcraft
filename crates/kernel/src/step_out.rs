//! STEP (ISO 10303-21) export of SolidCraft bodies: AP242 product structure, assemblies, names,
//! colours and layers, written by solidcore-step ([`solidcore_step::write::product`]) from each body's
//! record and paint.

use solidcore_step::write::product::{self, Colour, ExportError};

use crate::body::{Body, colour_name};
use crate::{KernelError, Result};

/// A named, optionally coloured body to export (coordinates in its part's frame, mm).
pub struct ExportBody<'a> {
    pub name: String,
    pub body: &'a Body,
    pub color: Option<[f32; 3]>,
}

/// A part or assembly: its own bodies and placed child products.
pub struct ExportProduct<'a> {
    pub name: String,
    pub bodies: Vec<ExportBody<'a>>,
    /// (child product index, rigid placement in this product, column-major 4×4, occurrence name).
    pub children: Vec<(usize, [[f64; 4]; 4], String)>,
}

/// Header fields.
#[derive(Clone, Debug, Default)]
pub struct StepHeader {
    /// FILE_NAME name (the file's own name).
    pub file_name: String,
    /// FILE_NAME time stamp (ISO 8601), empty when unknown.
    pub time_stamp: String,
    pub author: String,
    pub organization: String,
    /// FILE_DESCRIPTION text; `SolidCraft model` when empty.
    pub description: String,
}

/// STEP text and the exact written identity of each input body's faces.
#[derive(Clone, Debug)]
pub struct StepExport {
    /// The complete STEP payload.
    pub text: String,
    /// `ADVANCED_FACE` ids indexed by input body, then source record face.
    /// A face used by no shell has id 0.
    pub face_ids: Vec<Vec<u64>>,
}

/// The header solidcore-step writes: SolidCraft as the program and system that wrote the file.
fn product_header(h: &StepHeader) -> product::Header {
    product::Header {
        file_name: h.file_name.clone(),
        time_stamp: h.time_stamp.clone(),
        author: h.author.clone(),
        organization: h.organization.clone(),
        description: if h.description.is_empty() { "SolidCraft model".into() } else { h.description.clone() },
        preprocessor_version: "SolidCraft".into(),
        originating_system: "SolidCraft".into(),
    }
}

/// The body as solidcore-step writes it: its record, its colour with the paint's opacity and
/// name, and its faces' own colours. A colour's name is cleaned as an imported one is
/// ([`colour_name`]).
fn export_body<'a>(b: &ExportBody<'a>) -> product::ExportBody<'a> {
    let paint = b.body.paint();
    let colour = |rgb: [f32; 3], opacity: f32, name: Option<&str>| Colour { rgb, opacity, name: name.and_then(colour_name).unwrap_or_default() };
    product::ExportBody {
        name: b.name.clone(),
        record: b.body.record(),
        color: b.color.map(|c| colour(c, paint.map_or(1.0, |p| p.opacity), paint.and_then(|p| p.name.as_deref()))),
        faces: paint.map(|p| p.faces.iter().map(|f| (f.face, colour(f.color, f.opacity, f.name.as_deref()))).collect()).unwrap_or_default(),
        layer: paint.and_then(|p| p.layer.clone()).unwrap_or_default(),
    }
}

fn kernel_error(e: ExportError) -> KernelError {
    match e {
        ExportError::Body { .. } | ExportError::Record { .. } => KernelError::Failed(e.to_string()),
        ExportError::Nothing
        | ExportError::BadReference
        | ExportError::Recursive
        | ExportError::NotRigid
        | ExportError::Colour { .. }
        | ExportError::Face { .. }
        | ExportError::NotFinite { .. } => KernelError::Invalid(e.to_string()),
    }
}

/// STEP text (AP242) for products with bodies and assembly placements; `root` is the top product.
/// A mesh body is refused before anything is written.
pub fn step_export_products(products: &[ExportProduct], root: usize, header: &StepHeader) -> Result<String> {
    let _span = solidcraft_profile::span("kernel", "step_export_products");
    for b in products.iter().flat_map(|p| &p.bodies) {
        b.body.require_brep("STEP export")?;
    }
    let list: Vec<product::ExportProduct> = products
        .iter()
        .map(|p| product::ExportProduct { name: p.name.clone(), bodies: p.bodies.iter().map(export_body).collect(), children: p.children.clone() })
        .collect();
    product::products(&list, root, &product_header(header)).map_err(kernel_error)
}

/// STEP text for bodies of one part, named after the file.
pub fn step_export_bodies(bodies: &[ExportBody], header: &StepHeader) -> Result<String> {
    step_export_bodies_with_face_ids(bodies, header).map(|written| written.text)
}

/// [`step_export_bodies`] with each source record face's written STEP entity id.
pub fn step_export_bodies_with_face_ids(bodies: &[ExportBody], header: &StepHeader) -> Result<StepExport> {
    let _span = solidcraft_profile::span("kernel", "step_export_bodies");
    for b in bodies {
        b.body.require_brep("STEP export")?;
    }
    let written = product::bodies_with_face_ids(bodies.iter().map(export_body).collect(), &product_header(header)).map_err(kernel_error)?;
    let face_ids = written.face_ids.into_iter().next().ok_or_else(|| KernelError::Failed("STEP export omitted its product face map".into()))?;
    Ok(StepExport { text: written.text, face_ids })
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::Vec3;
    use solidcore_step::p21;
    use solidcraft_geom::prelude::*;

    #[test]
    fn exported_face_ids_find_the_same_faces_after_import() {
        let body = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let written = crate::step_export_with_face_ids(&[&body], "face correspondence").unwrap();
        assert_eq!(written.text, crate::step_export(&[&body], "face correspondence").unwrap());
        let imported = crate::step_import(&written.text).unwrap();
        assert_eq!(imported.bodies.len(), 1);
        let read = &imported.bodies[0];
        let ids = &written.face_ids[0];
        assert_eq!(ids.len(), 6);
        for (index, source) in body.face_sources().unwrap().iter().enumerate() {
            let crate::FaceSource::Box { axis, high, .. } = source.as_ref().unwrap() else { panic!("box face") };
            let mut center = [5.0, 10.0, 15.0];
            center[usize::from(*axis)] = if *high { [10.0, 20.0, 30.0][usize::from(*axis)] } else { 0.0 };
            let center = Vec3::new(center[0], center[1], center[2]);
            let matches: Vec<_> = read.face_origin.iter().enumerate().filter(|(_, id)| **id == ids[index]).collect();
            assert_eq!(matches.len(), 1, "one imported face for each box face");
            assert!(read.body.point_lies_on_face(matches[0].0, center, 1e-8).unwrap().is_some(), "{source:?} at {center:?}");
        }
    }

    /// A body's paint layer is written as the layer it is assigned to, and read back as it.
    #[test]
    fn a_paint_layer_is_written_and_read_back() {
        let paint = crate::Paint { opacity: 1.0, faces: Vec::new(), name: None, layer: Some("Frame/Rails".into()) };
        let on = crate::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap().with_paint(Some(paint));
        let off = crate::box_solid(Vec3::new(5.0, 0.0, 0.0), Vec3::new(6.0, 1.0, 1.0)).unwrap();
        let bodies = [ExportBody { name: "On".into(), body: &on, color: None }, ExportBody { name: "Off".into(), body: &off, color: None }];
        let text = step_export_bodies(&bodies, &StepHeader::default()).unwrap();
        assert_eq!(text.matches("PRESENTATION_LAYER_ASSIGNMENT('Frame/Rails','',(").count(), 1, "{text}");
        let read = crate::step_import(&text).unwrap();
        let layers: Vec<(&str, Option<&str>)> = read.bodies.iter().map(|b| (b.name.as_str(), b.layer.as_deref())).collect();
        assert_eq!(layers, [("On", Some("Frame/Rails")), ("Off", None)], "{text}");
    }

    #[test]
    fn islands_are_solids_of_their_own_not_voids() {
        use solidcore_record::{Loop2, Plane, Region2, Vec2};
        let square = |a: f64, b: f64| Loop2::polygon(&[Vec2::new(a, a), Vec2::new(b, a), Vec2::new(b, b), Vec2::new(a, b)]);
        // A plate with a square hole, and an island inside the hole (a letter's counter).
        let plate = Region2 { outer: square(0.0, 30.0), holes: vec![square(10.0, 20.0).reversed()] };
        let island = Region2 { outer: square(12.0, 18.0), holes: vec![] };
        let body = crate::extrude_planar_regions(&Plane::XY, &[plate, island], 0.0, 2.0).unwrap();
        assert_eq!(body.shell_count(), 2);
        let text = crate::step_export(&[&body], "test").unwrap();
        assert_eq!(text.matches("MANIFOLD_SOLID_BREP(").count(), 2, "{text}");
        assert_eq!(text.matches("BREP_WITH_VOIDS").count(), 0);
        let imp = crate::step_import(&text).unwrap();
        let v: f64 = imp.bodies.iter().map(|b| crate::measure(&b.body).unwrap().volume).sum();
        assert!((v - (800.0 + 36.0) * 2.0).abs() < 1e-6, "{v}");
        // A real void stays a void.
        let hollow = crate::boolean(
            &crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap(),
            &crate::box_solid(Vec3::new(3.0, 3.0, 3.0), Vec3::new(7.0, 7.0, 7.0)).unwrap(),
            crate::BoolOp::Cut,
        )
        .unwrap()
        .unwrap();
        assert_eq!(hollow.shell_count(), 2);
        let text = crate::step_export(&[&hollow], "test").unwrap();
        assert_eq!(text.matches("BREP_WITH_VOIDS").count(), 1, "{text}");
    }

    /// The uncertainty a file states covers every edge and vertex of its bodies (#11997): STEP
    /// keeps no tolerance per edge, so the context's is the only place an edge's deviation goes.
    /// A body whose edge may lie 3e-3 mm off its faces (its tolerance) was written with its
    /// record's tolerance, 2e-5, so a reader takes its geometry as good to 2e-5.
    #[test]
    fn uncertainty_covers_the_edge_and_vertex_tolerances() {
        let bx = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 5.0)).unwrap();
        let mut record = bx.record().clone();
        let own = record.tolerance;
        assert!(own < 1e-4, "{own}");
        let edge = record.edges.first_mut().unwrap();
        edge.tolerance = 3e-3;
        let ends = edge.vertices;
        for v in ends {
            record.vertices.get_mut(v as usize).unwrap().tolerance = 4e-3;
        }
        let tolerant = crate::Body::from_record(record).unwrap();
        let text = crate::step_export(&[&tolerant], "test").unwrap();
        let stated = |text: &str| -> Vec<f64> {
            let ex = p21::parse(text).unwrap();
            ex.entities
                .values()
                .filter(|e| e.name() == "UNCERTAINTY_MEASURE_WITH_UNIT")
                .filter_map(|e| e.params().first().and_then(p21::Param::as_f64))
                .collect()
        };
        assert_eq!(stated(&text), vec![4e-3], "the vertex tolerance");
        // A body without tolerant edges states its own tolerance, as before.
        let text = crate::step_export(&[&bx], "test").unwrap();
        assert_eq!(stated(&text), vec![own]);
    }

    /// Each body comes back as accurate as it was written, whatever is beside it: a box written
    /// beside a box whose edge may lie 0.3 mm off its faces was read back at 0.3 mm (the file
    /// stated one uncertainty, the loosest), and so was every body of an Insert Design beside
    /// one loose body. The bodies come back in the order they were written, with their names.
    #[test]
    fn each_body_reads_back_at_its_own_tolerance_in_order() {
        let bx = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 5.0)).unwrap();
        let own = bx.record().tolerance;
        let mut record = crate::box_solid(Vec3::new(30.0, 0.0, 0.0), Vec3::new(10.0, 20.0, 5.0)).unwrap().record().clone();
        record.edges.first_mut().unwrap().tolerance = 0.3;
        let loose = crate::Body::from_record(record).unwrap();
        let far = crate::box_solid(Vec3::new(60.0, 0.0, 0.0), Vec3::new(10.0, 20.0, 5.0)).unwrap();
        let named = |name: &str, body| ExportBody { name: name.into(), body, color: None };
        let text = step_export_bodies(&[named("First", &bx), named("Loose", &loose), named("Last", &far)], &StepHeader::default()).unwrap();
        let read = crate::step_import(&text).unwrap();
        let names: Vec<&str> = read.bodies.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["First", "Loose", "Last"], "{:?}", read.warnings);
        for b in &read.bodies {
            let r = b.body.record();
            let most = r.edges.iter().map(|e| e.tolerance).chain([r.tolerance]).fold(0.0, f64::max);
            if b.name == "Loose" {
                assert!(most >= 0.3, "{}: {most}", b.name);
            } else {
                assert!(most <= own, "{} read at {most}, written at {own}", b.name);
            }
        }
    }

    #[test]
    fn assembly_names_colours_round_trip() {
        use crate::{box_solid, cylinder, measure, step_import, step_validate};
        let bx = box_solid(solidcore_record::Vec3::ZERO, solidcore_record::Vec3::new(10.0, 20.0, 5.0)).unwrap();
        let pin = cylinder(solidcore_record::Vec3::ZERO, solidcore_record::Vec3::Z, 2.0, 8.0).unwrap();
        let rot90 = [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [50.0, 0.0, 0.0, 1.0]];
        let shift = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 5.0, 1.0]];
        let products = vec![
            ExportProduct {
                name: "Root Ä".into(),
                bodies: vec![ExportBody { name: "Base".into(), body: &bx, color: Some([1.0, 0.0, 0.0]) }],
                children: vec![(1, shift, "Pin:1".into()), (1, rot90, "Pin:2".into())],
            },
            ExportProduct { name: "Pin".into(), bodies: vec![ExportBody { name: "Shaft".into(), body: &pin, color: None }], children: vec![] },
        ];
        let text = step_export_products(&products, 0, &StepHeader { file_name: "a.step".into(), ..Default::default() }).unwrap();
        step_validate(&text).unwrap();
        assert!(text.contains("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF"));
        assert_eq!(text.matches("NEXT_ASSEMBLY_USAGE_OCCURRENCE").count(), 2);
        // Shared geometry: the unit Z direction is written once.
        assert_eq!(text.matches("DIRECTION('',(0.0,0.0,1.0))").count(), 1);
        let imp = step_import(&text).unwrap();
        assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
        assert_eq!(imp.bodies.len(), 3);
        let names: Vec<&str> = imp.bodies.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["Base", "Shaft", "Shaft"]);
        assert_eq!(imp.bodies[0].color, Some([1.0, 0.0, 0.0]));
        assert_eq!(imp.tree[0].name, "Root Ä");
        assert_eq!(imp.tree[0].children.len(), 2);
        let m: Vec<_> = imp.bodies.iter().map(|b| measure(&b.body).unwrap()).collect();
        assert!((m[1].bbox.min.z - 5.0).abs() < 1e-6 && (m[1].bbox.max.z - 13.0).abs() < 1e-6);
        // Rotated 90° about Z and moved 50 along X: centre at (50, 0).
        assert!(m[2].centroid.dist(solidcore_record::Vec3::new(50.0, 0.0, 4.0)) < 1e-3, "{:?}", m[2].centroid);
        // Recursion and bad placements are errors.
        let cyc = vec![ExportProduct {
            name: "A".into(),
            bodies: vec![ExportBody { name: "B".into(), body: &bx, color: None }],
            children: vec![(0, shift, String::new())],
        }];
        assert!(step_export_products(&cyc, 0, &StepHeader::default()).is_err());
        let scaled = [[2.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0; 4]];
        let bad = vec![
            ExportProduct { name: "A".into(), bodies: vec![], children: vec![(1, scaled, String::new())] },
            ExportProduct { name: "B".into(), bodies: vec![ExportBody { name: "B".into(), body: &bx, color: None }], children: vec![] },
        ];
        assert!(step_export_products(&bad, 0, &StepHeader::default()).is_err());
    }

    /// Two equal-diameter perpendicular holes with intersecting axes (#22220): the intersection
    /// curves are analytic ellipses whose minor axis equals the hole radius, so exported `ELLIPSE`
    /// entities must satisfy `semi_axis_1 >= semi_axis_2`, keep vertices on-curve and loops
    /// closed, validate, and round-trip cleanly.
    #[test]
    fn equal_diameter_perpendicular_intersecting_holes_step_export() {
        use solidcore_record::{Loop2, Plane, Region2, Vec2};
        use std::f64::consts::PI;

        let r = 5.0_f64;
        let side = 40.0_f64;
        let expected = side.powi(3) - 2.0 * PI * r * r * side + 16.0 * r.powi(3) / 3.0;

        // Case 1: primitive cylinders along Z and X through the block centre.
        let block = crate::box_solid(Vec3::ZERO, Vec3::new(side, side, side)).unwrap();
        let hole_z = crate::cylinder(Vec3::new(20.0, 20.0, -5.0), Vec3::Z, r, 50.0).unwrap();
        let hole_x = crate::cylinder(Vec3::new(-5.0, 20.0, 20.0), Vec3::X, r, 50.0).unwrap();
        let one = crate::boolean(&block, &hole_z, crate::BoolOp::Cut).unwrap().unwrap();
        let prim_body = crate::boolean(&one, &hole_x, crate::BoolOp::Cut).unwrap().unwrap();

        // Case 2: extruded circular sketches on XY and XZ through the centred block.
        let centered = crate::box_solid(Vec3::new(-20.0, -20.0, -20.0), Vec3::new(20.0, 20.0, 20.0)).unwrap();
        let circle = || Region2 { outer: Loop2::circle(Vec2::ZERO, r), holes: vec![] };
        let sk_z = crate::extrude_planar_regions(&Plane::XY, &[circle()], -25.0, 25.0).unwrap();
        let sk_y = crate::extrude_planar_regions(&Plane::XZ, &[circle()], -25.0, 25.0).unwrap();
        let one = crate::boolean(&centered, &sk_z, crate::BoolOp::Cut).unwrap().unwrap();
        let sketch_body = crate::boolean(&one, &sk_y, crate::BoolOp::Cut).unwrap().unwrap();

        for (label, body) in [("primitives", prim_body), ("sketched", sketch_body)] {
            let text = crate::step_export(&[&body], label).unwrap();
            crate::step_validate(&text).unwrap();
            let ex = p21::parse(&text).unwrap();

            // Every ELLIPSE entity must have semi_axis_1 >= semi_axis_2 > 0 (OpenCascade requirement).
            let mut ellipse_count = 0usize;
            for ent in ex.entities.values().filter(|e| e.name() == "ELLIPSE") {
                let a = ent.params()[2].as_f64().unwrap();
                let b = ent.params()[3].as_f64().unwrap();
                assert!(a >= b && b > 0.0, "{label}: ELLIPSE has semi_axis_1 ({a}) < semi_axis_2 ({b})");
                ellipse_count += 1;
            }
            assert!(ellipse_count >= 4, "{label}: expected at least 4 ELLIPSE entities, found {ellipse_count}");

            // Every ADVANCED_FACE must have closed EDGE_LOOPs and a FACE_OUTER_BOUND.
            for face in ex.entities.values().filter(|e| e.name() == "ADVANCED_FACE") {
                let bounds = face.params()[1].as_list().unwrap();
                let outer_count = bounds.iter().filter(|b| ex.get(b.as_ref_id().unwrap()).unwrap().name() == "FACE_OUTER_BOUND").count();
                assert_eq!(outer_count, 1, "{label}: face without single FACE_OUTER_BOUND");
                for b in bounds {
                    let bound = ex.get(b.as_ref_id().unwrap()).unwrap();
                    let lp = ex.get(bound.params()[1].as_ref_id().unwrap()).unwrap();
                    if lp.name() != "EDGE_LOOP" {
                        continue;
                    }
                    let oes = lp.params()[1].as_list().unwrap();
                    assert!(!oes.is_empty(), "{label}: empty EDGE_LOOP");
                    let ends: Vec<(u64, u64)> = oes
                        .iter()
                        .map(|oe| {
                            let oe = ex.get(oe.as_ref_id().unwrap()).unwrap();
                            let ec = ex.get(oe.params()[3].as_ref_id().unwrap()).unwrap();
                            let (s, e) = (ec.params()[1].as_ref_id().unwrap(), ec.params()[2].as_ref_id().unwrap());
                            if oe.params()[4].as_bool().unwrap() { (s, e) } else { (e, s) }
                        })
                        .collect();
                    for i in 0..ends.len() {
                        let next = (i + 1) % ends.len();
                        assert_eq!(ends[i].1, ends[next].0, "{label}: EDGE_LOOP not closed at edge {i}");
                    }
                }
            }

            // Round-trip import: no warnings, valid body, matching face count and Steinmetz volume.
            let imp = crate::step_import(&text).unwrap();
            assert!(imp.warnings.is_empty(), "{label}: {:?}", imp.warnings);
            assert_eq!(imp.bodies.len(), 1, "{label}");
            assert_eq!(imp.bodies[0].body.face_count(), body.face_count(), "{label}");
            let vol = crate::measure(&imp.bodies[0].body).unwrap().volume;
            assert!((vol - expected).abs() < 1e-3, "{label}: got volume {vol}, expected {expected}");
        }
    }
}
