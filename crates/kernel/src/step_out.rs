//! STEP (ISO 10303-21) export: AP242 product structure, assemblies, names and colours.
//!
//! Each body's record is written as it is, the exact B-rep, by
//! [`solidcraft_step::write::topology`]: its material regions as solids, its open shells as a
//! surface model. Around it this writes one header and context, a product per part with its
//! shape representation, named solids, surface and face colours, and assemblies as
//! next-assembly-usage occurrences placed by item-defined transformations (the structure the
//! CAx-IF recommends and our reader follows). Identical geometric entities (points, directions,
//! placements, curves, surfaces) are written once and referenced; ids are dense from `#1`.

use std::fmt::Write as _;

use solidcraft_step::p21::Param;
use solidcraft_step::write::{GeomWriter, Writer, string, topology};

use crate::body::Body;
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

/// The least distance uncertainty written (mm).
const MIN_UNCERTAINTY: f64 = 1e-6;

fn s(text: &str) -> Param {
    Param::Str(text.to_string())
}
fn r(id: u64) -> Param {
    Param::Ref(id)
}
fn refs(ids: &[u64]) -> Param {
    Param::List(ids.iter().map(|i| r(*i)).collect())
}

/// The placement's origin, z and x axes; refused unless it is a rotation and a translation.
fn rigid_frame(m: &[[f64; 4]; 4]) -> Result<([f64; 3], [f64; 3], [f64; 3])> {
    let x = [m[0][0], m[0][1], m[0][2]];
    let y = [m[1][0], m[1][1], m[1][2]];
    let z = [m[2][0], m[2][1], m[2][2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]];
    let ok = (dot(x, x) - 1.0).abs() < 1e-6 && (dot(y, y) - 1.0).abs() < 1e-6 && dot(x, y).abs() < 1e-6 && dot(cross, z) > 0.999_999;
    if !ok || m.iter().flatten().any(|v| !v.is_finite()) {
        return Err(KernelError::Invalid("component placements must be rigid (rotation and translation) to export as STEP".into()));
    }
    Ok(([m[3][0], m[3][1], m[3][2]], z, x))
}

/// STEP text (AP242) for products with bodies and assembly placements; `root` is the top product.
pub fn step_export_products(products: &[ExportProduct], root: usize, header: &StepHeader) -> Result<String> {
    if products.is_empty() || root >= products.len() {
        return Err(KernelError::Invalid("nothing to export".into()));
    }
    if products.iter().all(|p| p.bodies.is_empty()) {
        return Err(KernelError::Invalid("nothing to export".into()));
    }
    for p in products {
        for b in &p.bodies {
            b.body.require_brep("STEP export")?;
        }
        if p.children.iter().any(|(c, _, _)| *c >= products.len()) {
            return Err(KernelError::Invalid("bad component reference".into()));
        }
    }
    // No product may contain itself.
    fn cyclic(products: &[ExportProduct], p: usize, stack: &mut Vec<usize>) -> bool {
        if stack.contains(&p) || stack.len() > 64 {
            return true;
        }
        stack.push(p);
        let bad = products.get(p).is_some_and(|x| x.children.iter().any(|(c, _, _)| cyclic(products, *c, stack)));
        stack.pop();
        bad
    }
    if cyclic(products, root, &mut Vec::new()) {
        return Err(KernelError::Invalid("the component tree is recursive".into()));
    }
    // Products reachable from the root.
    let mut reach = vec![false; products.len()];
    let mut todo = vec![root];
    while let Some(p) = todo.pop() {
        if let Some(seen) = reach.get_mut(p)
            && !*seen
        {
            *seen = true;
            if let Some(x) = products.get(p) {
                todo.extend(x.children.iter().map(|(c, _, _)| *c));
            }
        }
    }
    let reached: Vec<(usize, &ExportProduct)> = products.iter().enumerate().filter(|(i, _)| reach.get(*i).copied().unwrap_or(false)).collect();
    // The context states the models' accuracy: the loosest of their records' tolerances.
    let uncertainty =
        reached.iter().flat_map(|(_, p)| &p.bodies).map(|b| b.body.record().tolerance).filter(|t| t.is_finite()).fold(MIN_UNCERTAINTY, f64::max);

    let mut w = Writer::new();
    let app = w.add_distinct("APPLICATION_CONTEXT", &[s("managed model based 3d engineering")]);
    w.add_distinct(
        "APPLICATION_PROTOCOL_DEFINITION",
        &[s("international standard"), s("ap242_managed_model_based_3d_engineering"), Param::Int(2014), r(app)],
    );
    let pctx = w.add_distinct("PRODUCT_CONTEXT", &[s(""), r(app), s("mechanical")]);
    let pdctx = w.add_distinct("PRODUCT_DEFINITION_CONTEXT", &[s("part definition"), r(app), s("design")]);
    let mm = w.add_complex(&[
        ("LENGTH_UNIT", vec![]),
        ("NAMED_UNIT", vec![Param::Derived]),
        ("SI_UNIT", vec![Param::Enum("MILLI".into()), Param::Enum("METRE".into())]),
    ]);
    let rad = w.add_complex(&[
        ("NAMED_UNIT", vec![Param::Derived]),
        ("PLANE_ANGLE_UNIT", vec![]),
        ("SI_UNIT", vec![Param::Null, Param::Enum("RADIAN".into())]),
    ]);
    let sr = w.add_complex(&[
        ("NAMED_UNIT", vec![Param::Derived]),
        ("SI_UNIT", vec![Param::Null, Param::Enum("STERADIAN".into())]),
        ("SOLID_ANGLE_UNIT", vec![]),
    ]);
    let unc = w.add_distinct(
        "UNCERTAINTY_MEASURE_WITH_UNIT",
        &[Param::Typed("LENGTH_MEASURE".into(), vec![Param::Real(uncertainty)]), r(mm), s("distance_accuracy_value"), s("confusion accuracy")],
    );
    let ctx = w.add_complex(&[
        ("GEOMETRIC_REPRESENTATION_CONTEXT", vec![Param::Int(3)]),
        ("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT", vec![refs(&[unc])]),
        ("GLOBAL_UNIT_ASSIGNED_CONTEXT", vec![refs(&[mm, rad, sr])]),
        ("REPRESENTATION_CONTEXT", vec![s("Context #1"), s("3D Context with UNIT and UNCERTAINTY")]),
    ]);
    let origin = GeomWriter::new(&mut w).placement(&solidcraft_step::geom::Placement {
        origin: [0.0; 3],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
    });

    // Placements of children inside each parent, made before the parent's representation.
    let mut placements: Vec<Vec<u64>> = vec![Vec::new(); products.len()];
    for &(pi, p) in &reached {
        for (_, m, _) in &p.children {
            let (t, z, x) = rigid_frame(m)?;
            let mut g = GeomWriter::new(&mut w);
            let (pt, dz, dx) = (g.point(t), g.direction(z), g.direction(x));
            // Its own entity: it is an item of the parent's representation.
            let a = w.add_distinct("AXIS2_PLACEMENT_3D", &[s(""), r(pt), r(dz), r(dx)]);
            if let Some(v) = placements.get_mut(pi) {
                v.push(a);
            }
        }
    }
    let mut pd = vec![0u64; products.len()];
    let mut shape_rep = vec![0u64; products.len()];
    let mut product_ids = Vec::new();
    let mut styled = Vec::new();
    for &(pi, p) in &reached {
        let name = if p.name.trim().is_empty() { "Part".to_string() } else { p.name.clone() };
        let prod = w.add_distinct("PRODUCT", &[s(&name), s(&name), s(""), refs(&[pctx])]);
        product_ids.push(prod);
        let pdf = w.add_distinct("PRODUCT_DEFINITION_FORMATION", &[s(""), s(""), r(prod)]);
        let d = w.add_distinct("PRODUCT_DEFINITION", &[s("design"), s(""), r(pdf), r(pdctx)]);
        let pds = w.add_distinct("PRODUCT_DEFINITION_SHAPE", &[s(""), s(""), r(d)]);
        let mut items = vec![origin];
        items.extend(placements.get(pi).into_iter().flatten().copied());
        let srep = w.add_distinct("SHAPE_REPRESENTATION", &[s(&name), refs(&items), r(ctx)]);
        w.add_distinct("SHAPE_DEFINITION_REPRESENTATION", &[r(pds), r(srep)]);
        if let Some(slot) = pd.get_mut(pi) {
            *slot = d;
        }
        if let Some(slot) = shape_rep.get_mut(pi) {
            *slot = srep;
        }
        let (mut solids, mut sheets) = (Vec::new(), Vec::new());
        for b in &p.bodies {
            let written = topology::brep(&mut GeomWriter::new(&mut w), b.body.record(), &b.name)
                .map_err(|e| KernelError::Failed(format!("STEP export of '{}': {e}", b.name)))?;
            let opacity = b.body.paint().map_or(1.0, |p| p.opacity);
            let items: Vec<u64> = written.solids.iter().copied().chain(written.sheets).collect();
            if let Some(c) = b.color {
                for item in &items {
                    styled.push(style(&mut w, *item, c, opacity));
                }
            }
            // Faces with colours of their own.
            for f in b.body.paint().map(|p| p.faces.as_slice()).unwrap_or(&[]) {
                if let Some(&face) = written.faces.get(f.face).filter(|id| **id != 0) {
                    styled.push(style(&mut w, face, f.color, f.opacity));
                }
            }
            solids.extend(written.solids);
            sheets.extend(written.sheets);
        }
        for (kind, list) in [("ADVANCED_BREP_SHAPE_REPRESENTATION", solids), ("MANIFOLD_SURFACE_SHAPE_REPRESENTATION", sheets)] {
            if list.is_empty() {
                continue;
            }
            let rep = w.add_distinct(kind, &[s(&name), refs(&[list, vec![origin]].concat()), r(ctx)]);
            w.add_distinct("SHAPE_REPRESENTATION_RELATIONSHIP", &[s(""), s(""), r(srep), r(rep)]);
        }
    }
    if !product_ids.is_empty() {
        w.add_distinct("PRODUCT_RELATED_PRODUCT_CATEGORY", &[s("part"), s(""), refs(&product_ids)]);
    }
    // Assembly occurrences.
    for &(pi, p) in &reached {
        for (k, (ci, _, oname)) in p.children.iter().enumerate() {
            let (Some(&parent_pd), Some(&child_pd), Some(&parent_sr), Some(&child_sr), Some(&target)) =
                (pd.get(pi), pd.get(*ci), shape_rep.get(pi), shape_rep.get(*ci), placements.get(pi).and_then(|v| v.get(k)))
            else {
                continue;
            };
            let nm = if oname.trim().is_empty() {
                format!("{}:{}", products.get(*ci).map(|c| c.name.as_str()).unwrap_or(""), k + 1)
            } else {
                oname.clone()
            };
            let nauo = w.add_distinct("NEXT_ASSEMBLY_USAGE_OCCURRENCE", &[s(&nm), s(&nm), s(""), r(parent_pd), r(child_pd), Param::Null]);
            let pds = w.add_distinct("PRODUCT_DEFINITION_SHAPE", &[s(""), s(""), r(nauo)]);
            let idt = w.add_distinct("ITEM_DEFINED_TRANSFORMATION", &[s(""), s(""), r(origin), r(target)]);
            let rr = w.add_complex(&[
                ("REPRESENTATION_RELATIONSHIP", vec![s(""), s(""), r(child_sr), r(parent_sr)]),
                ("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION", vec![r(idt)]),
                ("SHAPE_REPRESENTATION_RELATIONSHIP", vec![]),
            ]);
            w.add_distinct("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION", &[r(rr), r(pds)]);
        }
    }
    if !styled.is_empty() {
        w.add_distinct("MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION", &[s(""), refs(&styled), r(ctx)]);
    }

    let mut text = String::new();
    text.push_str("ISO-10303-21;\nHEADER;\n");
    let description = if header.description.is_empty() { "SolidCraft model" } else { header.description.as_str() };
    let _ = writeln!(text, "FILE_DESCRIPTION(({}),'2;1');", string(description));
    let _ = writeln!(
        text,
        "FILE_NAME({},{},({}),({}),'SolidCraft','SolidCraft','');",
        string(&header.file_name),
        string(&header.time_stamp),
        string(&header.author),
        string(&header.organization)
    );
    text.push_str("FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }'));\nENDSEC;\nDATA;\n");
    text.push_str(&w.data());
    text.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    Ok(text)
}

/// A surface colour for an item (STYLED_ITEM chain; the styles are shared).
fn style(w: &mut Writer, item: u64, c: [f32; 3], opacity: f32) -> u64 {
    let rgb = w.add("COLOUR_RGB", &[s(""), Param::Real(c[0] as f64), Param::Real(c[1] as f64), Param::Real(c[2] as f64)]);
    let fasc = w.add("FILL_AREA_STYLE_COLOUR", &[s(""), r(rgb)]);
    let fas = w.add("FILL_AREA_STYLE", &[s(""), refs(&[fasc])]);
    let ssfa = w.add("SURFACE_STYLE_FILL_AREA", &[r(fas)]);
    // See-through looks add a transparency (1 = invisible) to the side style.
    let sides = if opacity < 1.0 {
        let tr = w.add("SURFACE_STYLE_TRANSPARENT", &[Param::Real((1.0 - opacity as f64).clamp(0.0, 1.0))]);
        vec![ssfa, tr]
    } else {
        vec![ssfa]
    };
    let sss = w.add("SURFACE_SIDE_STYLE", &[s(""), refs(&sides)]);
    let ssu = w.add("SURFACE_STYLE_USAGE", &[Param::Enum("BOTH".into()), r(sss)]);
    let psa = w.add("PRESENTATION_STYLE_ASSIGNMENT", &[refs(&[ssu])]);
    w.add_distinct("STYLED_ITEM", &[s("color"), refs(&[psa]), r(item)])
}

/// STEP text for bodies of one part (names `Body1`…).
pub fn step_export_bodies(bodies: &[ExportBody], header: &StepHeader) -> Result<String> {
    let name = if header.file_name.is_empty() {
        "Part".to_string()
    } else {
        header.file_name.trim_end_matches(".step").trim_end_matches(".stp").to_string()
    };
    let p = ExportProduct {
        name,
        bodies: bodies.iter().map(|b| ExportBody { name: b.name.clone(), body: b.body, color: b.color }).collect(),
        children: Vec::new(),
    };
    step_export_products(&[p], 0, header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Vec3;
    use solidcraft_step::p21;
    use solidcraft_step::write::real;

    #[test]
    fn islands_are_solids_of_their_own_not_voids() {
        use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
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

    #[test]
    fn numbers_and_strings() {
        assert_eq!(real(1.0), "1.0");
        assert_eq!(real(1e-6), "1.E-6");
        assert_eq!(real(-2.5e20), "-2.5E20");
        assert_eq!(real(3.0e100), "3.E100");
        assert_eq!(string("it's"), "'it''s'");
        assert_eq!(string("é"), "'\\X2\\00E9\\X0\\'");
        let ex = p21::parse(&format!("ISO-10303-21;DATA;#1=A({},{});ENDSEC;END-ISO-10303-21;", string("a'b\\é"), real(1e-6))).unwrap();
        let p = ex.get(1).unwrap().params();
        assert_eq!(p[0].as_str(), Some("a'b\\é"));
        assert_eq!(p[1].as_f64(), Some(1e-6));
        // Names of several non-ASCII characters in a row read back whole (#2344).
        for name in ["圆角", "凸台-拉伸7", "ネジ穴", "Gehäuse 😀"] {
            let ex = p21::parse(&format!("ISO-10303-21;DATA;#1=A({});ENDSEC;END-ISO-10303-21;", string(name))).unwrap();
            assert_eq!(ex.get(1).unwrap().params()[0].as_str(), Some(name));
        }
    }

    #[test]
    fn assembly_names_colours_round_trip() {
        use crate::{box_solid, cylinder, measure, step_import, step_validate};
        let bx = box_solid(solidcraft_geom::Vec3::ZERO, solidcraft_geom::Vec3::new(10.0, 20.0, 5.0)).unwrap();
        let pin = cylinder(solidcraft_geom::Vec3::ZERO, solidcraft_geom::Vec3::Z, 2.0, 8.0).unwrap();
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
        assert!(m[2].centroid.dist(solidcraft_geom::Vec3::new(50.0, 0.0, 4.0)) < 1e-3, "{:?}", m[2].centroid);
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
