//! STEP (ISO 10303-21) export of SolidCraft bodies: AP242 product structure, assemblies, names
//! and colours, written by solidcore-step ([`solidcore_step::write::product`]) from each body's
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
    let _span = solidcraft_profile::span("kernel", "step_export_bodies");
    for b in bodies {
        b.body.require_brep("STEP export")?;
    }
    product::bodies(bodies.iter().map(export_body).collect(), &product_header(header)).map_err(kernel_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::Vec3;
    use solidcore_step::p21;
    use solidcraft_geom::prelude::*;

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

    /// Faces on procedural sweeps are written as their certified nets and the edges on them as
    /// the nets' sides: an M20×2.5 thread cut four turns into a bar, and a square coil, each go
    /// out as one closed solid and read back with their exact volumes, to within the nets'
    /// certified bound times the area they move (and the measure's own accuracy).
    #[test]
    fn screw_swept_bodies_export_as_one_closed_solid() {
        use solidcore_ops::boolean::{BooleanOp, boolean as cut};
        use solidcore_ops::sweep::{Curve as C, DraftCorner, extrude, screw_sweep};
        use solidcore_record::brep as b;
        use std::f64::consts::TAU;
        let polygon = |p: &[[f64; 3]]| -> Vec<C> { (0..p.len()).map(|i| C::Line { start: p[i], end: p[(i + 1) % p.len()] }).collect() };
        // A polygon in the meridian plane y = 0 swept `turns` turns about z: Pappus, 2π turns ∫ x dA.
        let pappus = |p: &[[f64; 3]], turns: f64| {
            let moment: f64 = (0..p.len())
                .map(|i| {
                    let (a, b) = (p[i], p[(i + 1) % p.len()]);
                    (a[0] + b[0]) * (a[0] * b[2] - b[0] * a[2]) / 6.0
                })
                .sum();
            TAU * turns * moment.abs()
        };
        let export = |body: solidcore_ops::Body, exact: f64, what: &str| {
            let (record, _) = body.to_record().unwrap();
            let bound = record
                .faces
                .iter()
                .filter_map(|f| match &f.surface {
                    b::Surface::ProceduralSweep(s) => Some(s.bound),
                    _ => None,
                })
                .fold(0.0_f64, f64::max);
            assert!(bound > 0.0, "{what}: walls keep their definitions");
            assert!(record.edges.iter().any(|e| matches!(e.curve, b::Curve::OnSurface { .. } | b::Curve::OnSurfaceRef { .. })));
            let text = crate::step_export(&[&crate::Body::from_record(record).unwrap()], "test").unwrap();
            crate::step_validate(&text).unwrap();
            assert_eq!(text.matches("MANIFOLD_SOLID_BREP(").count(), 1, "{what}: one solid");
            let read = crate::step_import(&text).unwrap();
            assert_eq!(read.bodies.len(), 1, "{what}");
            let back = &read.bodies[0].body;
            assert!(!back.is_surface(), "{what}: a closed solid");
            let m = crate::measure_mass(back).unwrap();
            let allowed = bound * m.area + 1e-6 * exact;
            assert!((m.volume - exact).abs() <= allowed, "{what}: volume {} against {exact} (allowed {allowed})", m.volume);
        };

        // M20×2.5: the groove from the minor radius to past the major one, cut into a bar of
        // radius 10 that holds it from end to end. What it removes is the groove's profile
        // clipped to r <= 10, swept the same four turns.
        let (pitch, turns) = (2.5, 4.0);
        let minor = 10.0 - 5.0 * pitch * 3.0_f64.sqrt() / 16.0;
        let high = 10.0 + 0.05 * pitch;
        let half_at = |r: f64| pitch / 8.0 + (r - minor) / 3.0_f64.sqrt();
        let groove = [[minor, 0.0, -half_at(minor)], [high, 0.0, -half_at(high)], [high, 0.0, half_at(high)], [minor, 0.0, half_at(minor)]];
        let removed = [[minor, 0.0, -half_at(minor)], [10.0, 0.0, -half_at(10.0)], [10.0, 0.0, half_at(10.0)], [minor, 0.0, half_at(minor)]];
        let circle = [C::Arc { center: [0.0, 0.0, -pitch], axis: [0.0, 0.0, 1.0], reference: [1.0, 0.0, 0.0], radius: 10.0, start: 0.0, end: TAU }];
        let length = pitch * (turns + 2.0);
        let bar = extrude(&circle, [0.0, 0.0, 1.0], length, 0.0, DraftCorner::Curvature, true).unwrap();
        let thread = screw_sweep(&polygon(&groove), [0.0; 3], [0.0, 0.0, 1.0], pitch, turns, None, pitch * 1e-4).unwrap();
        let body = cut(bar, thread, BooleanOp::Subtract).unwrap().unwrap();
        export(body, std::f64::consts::PI * 100.0 * length - pappus(&removed, turns), "M20 thread");

        // A coil of square wire: two walls are exact cylinders, two are screw sweeps.
        let square = [[4.1, 0.0, 0.0], [5.0, 0.0, 0.0], [5.0, 0.0, 0.9], [4.1, 0.0, 0.9]];
        let coil = screw_sweep(&polygon(&square), [0.0; 3], [0.0, 0.0, 1.0], 1.5, 2.5, None, 1.5e-4).unwrap();
        export(coil, pappus(&square, 2.5), "coil");
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
}
