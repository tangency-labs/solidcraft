//! STEP (ISO 10303-21) export: AP242 product structure, assemblies, names and colours.
//!
//! Each body's record is written as it is, the exact B-rep, by
//! [`topology`]: its material regions as solids, its open shells as a
//! surface model. Around it this writes one header and context, a product per part with its
//! shape representation, named solids, surface and face colours, and assemblies as
//! next-assembly-usage occurrences placed by item-defined transformations (the structure the
//! CAx-IF recommends and our reader follows). Identical geometric entities (points, directions,
//! placements, curves, surfaces) are written once and referenced; ids are dense from `#1`.
//!
//! [`products`] writes products and assemblies, [`bodies`] the bodies of one part, and
//! [`solidcore_bodies`] solidcore bodies (`solidcore_ops::Body`) as one part, in one call.
//!
//! Origin: moved from SolidCraft's STEP export adapter (MIT OR Apache-2.0) with its history
//! (RFC 0003, 1.7).

use std::fmt::Write as _;

use solidcore_record::Vec3;
use solidcore_record::brep::Brep;

use super::{GeomWriter, Writer, string, topology};
use crate::p21::Param;
use crate::vec::Vec3Geom as _;

/// A surface colour: sRGB as 0..1, its opacity (1 = opaque) and its name (`COLOUR_RGB`'s name,
/// empty for none).
#[derive(Clone, Debug, PartialEq)]
pub struct Colour {
    /// Red, green and blue, each from 0 to 1; anything else is refused ([`ExportError::Colour`]).
    pub rgb: [f32; 3],
    /// Opacity from 0 (invisible) to 1 (opaque); anything else is refused.
    pub opacity: f32,
    /// The colour's name (`COLOUR_RGB`'s name), empty for none.
    pub name: String,
}

/// A named body to export (coordinates in its part's frame, mm): its record, its colour if it has
/// one, and the faces with colours of their own (the record's face index and its colour).
pub struct ExportBody<'a> {
    /// The body's name: its solids' and surface model's name.
    pub name: String,
    /// The body's B-rep record, written as it is.
    pub record: &'a Brep,
    /// The colour of the whole body, if it has one.
    pub color: Option<Colour>,
    /// Faces with colours of their own: an index into the record's faces (one out of range is
    /// refused, [`ExportError::Face`]) and its colour.
    pub faces: Vec<(usize, Colour)>,
}

/// A part or assembly: its own bodies and placed child products.
pub struct ExportProduct<'a> {
    /// The product's name; `Part` when blank.
    pub name: String,
    /// The product's own bodies, in its frame.
    pub bodies: Vec<ExportBody<'a>>,
    /// (child product index, rigid placement in this product, column-major 4×4, occurrence name).
    pub children: Vec<(usize, [[f64; 4]; 4], String)>,
}

/// Header fields. Each is written as given; an empty field is written as an empty string.
#[derive(Clone, Debug, Default)]
pub struct Header {
    /// FILE_NAME name (the file's own name).
    pub file_name: String,
    /// FILE_NAME time stamp (ISO 8601), empty when unknown.
    pub time_stamp: String,
    /// FILE_NAME author.
    pub author: String,
    /// FILE_NAME organization.
    pub organization: String,
    /// FILE_DESCRIPTION text.
    pub description: String,
    /// FILE_NAME preprocessor version: the program that wrote the file.
    pub preprocessor_version: String,
    /// FILE_NAME originating system: the system the model comes from.
    pub originating_system: String,
}

/// Why products could not be written.
#[derive(Clone, Debug, PartialEq)]
pub enum ExportError {
    /// No product, a root that is not one of them, or no body with a face in the products the
    /// root reaches.
    Nothing,
    /// A child that is not one of the products.
    BadReference,
    /// A product that contains itself (or a tree deeper than 64).
    Recursive,
    /// A placement that is not a rotation and a translation.
    NotRigid,
    /// A body's record that does not write.
    Body {
        /// The body's name.
        name: String,
        /// Why the writer refused it.
        reason: String,
    },
    /// A solidcore body whose record could not be made.
    Record {
        /// The body's name.
        name: String,
        /// Why its record could not be made.
        reason: String,
    },
    /// A body or face colour with a component or opacity that is not a number from 0 to 1.
    Colour {
        /// The body's name.
        name: String,
    },
    /// A body whose geometry holds a number that is not finite (a coordinate, a weight, a knot):
    /// STEP has no form for it.
    NotFinite {
        /// The body's name.
        name: String,
    },
    /// A face colour for a face the body's record does not have.
    Face {
        /// The body's name.
        name: String,
        /// The face index given.
        face: usize,
        /// How many faces the record has.
        faces: usize,
    },
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nothing => f.write_str("nothing to export"),
            Self::BadReference => f.write_str("bad component reference"),
            Self::Recursive => f.write_str("the component tree is recursive"),
            Self::NotRigid => f.write_str("component placements must be rigid (rotation and translation) to export as STEP"),
            Self::Body { name, reason } => write!(f, "STEP export of '{name}': {reason}"),
            Self::Record { name, reason } => write!(f, "STEP export of '{name}': no record: {reason}"),
            Self::Colour { name } => write!(f, "STEP export of '{name}': a colour or opacity is not a number from 0 to 1"),
            Self::NotFinite { name } => write!(f, "STEP export of '{name}': its geometry holds a number that is not finite"),
            Self::Face { name, face, faces } => write!(f, "STEP export of '{name}': face {face} has a colour, but the body has {faces} faces"),
        }
    }
}

impl std::error::Error for ExportError {}

type Result<T> = std::result::Result<T, ExportError>;

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
///
/// `m` is column-major: `m[0]`, `m[1]` and `m[2]` are the x, y and z axes and `m[3]` the
/// origin, each with its homogeneous entry last. The axes must be unit length, mutually
/// orthogonal and right-handed, and the bottom row (`m[0..3][3]`, `m[3][3]`) must be `0 0 0 1`,
/// each within 1e-6.
fn rigid_frame(m: &[[f64; 4]; 4]) -> Result<([f64; 3], [f64; 3], [f64; 3])> {
    let axis = |c: &[f64; 4]| Vec3::new(c[0], c[1], c[2]);
    let (x, y, z) = (axis(&m[0]), axis(&m[1]), axis(&m[2]));
    // Products by the kernel (`Vec3Geom`); here they are only compared with the tolerances.
    let dot = |a: Vec3, b: Vec3| a.dot(b);
    let cross = x.cross(y);
    let unit = |a: Vec3| (dot(a, a) - 1.0).abs() < 1e-6;
    let rotation =
        unit(x) && unit(y) && unit(z) && dot(x, y).abs() < 1e-6 && dot(x, z).abs() < 1e-6 && dot(y, z).abs() < 1e-6 && dot(cross, z) > 0.999_999;
    let affine = m[0][3].abs() < 1e-6 && m[1][3].abs() < 1e-6 && m[2][3].abs() < 1e-6 && (m[3][3] - 1.0).abs() < 1e-6;
    if !rotation || !affine || m.iter().flatten().any(|v| !v.is_finite()) {
        return Err(ExportError::NotRigid);
    }
    Ok(([m[3][0], m[3][1], m[3][2]], [z.x, z.y, z.z], [x.x, x.y, x.z]))
}

/// STEP text (AP242) for products with bodies and assembly placements; `root` is the top product.
///
/// # Errors
///
/// [`ExportError`]: nothing to write, a bad child reference, a recursive tree, a placement that
/// is not rigid, or a body whose record the writer refuses.
pub fn products(products: &[ExportProduct], root: usize, header: &Header) -> Result<String> {
    if products.is_empty() || root >= products.len() {
        return Err(ExportError::Nothing);
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
    // Only these are written: bodies in products the root does not reach do not count, and
    // neither do bodies with no face (they write no shape).
    if !reached.iter().flat_map(|(_, p)| &p.bodies).any(|b| !b.record.faces.is_empty()) {
        return Err(ExportError::Nothing);
    }
    for p in products {
        if p.children.iter().any(|(c, _, _)| *c >= products.len()) {
            return Err(ExportError::BadReference);
        }
    }
    if recursive(products, root) {
        return Err(ExportError::Recursive);
    }
    for b in reached.iter().flat_map(|(_, p)| &p.bodies) {
        let colours = b.color.iter().chain(b.faces.iter().map(|(_, c)| c));
        if colours.flat_map(|c| c.rgb.iter().chain(std::iter::once(&c.opacity))).any(|v| !(0.0..=1.0).contains(v)) {
            return Err(ExportError::Colour { name: b.name.clone() });
        }
        if b.record.vertices.iter().any(|v| v.point.iter().any(|x| !x.is_finite())) {
            return Err(ExportError::NotFinite { name: b.name.clone() });
        }
        let faces = b.record.faces.len();
        if let Some(&(face, _)) = b.faces.iter().find(|(i, _)| *i >= faces) {
            return Err(ExportError::Face { name: b.name.clone(), face, faces });
        }
    }
    // The context states the models' accuracy: the loosest tolerance of their records, edges
    // and vertices. STEP keeps no tolerance per edge, so an edge that lies off its faces (an
    // intersection curve fitted within its tolerance) is only as good as the file says (#11997).
    let uncertainty = reached
        .iter()
        .flat_map(|(_, p)| &p.bodies)
        .flat_map(|b| {
            let r = b.record;
            std::iter::once(r.tolerance).chain(r.edges.iter().map(|e| e.tolerance)).chain(r.vertices.iter().map(|v| v.tolerance))
        })
        .filter(|t| t.is_finite())
        .fold(MIN_UNCERTAINTY, f64::max);

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
    let origin =
        GeomWriter::new(&mut w).placement(&crate::geom::Placement { origin: [0.0; 3], axis: [0.0, 0.0, 1.0], ref_direction: [1.0, 0.0, 0.0] });

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
        let name = product_name(p);
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
            let before = w.non_finite();
            let written = topology::brep(&mut GeomWriter::new(&mut w), b.record, &b.name)
                .map_err(|reason| ExportError::Body { name: b.name.clone(), reason })?;
            if w.non_finite() != before {
                return Err(ExportError::NotFinite { name: b.name.clone() });
            }
            let items: Vec<u64> = written.solids.iter().copied().chain(written.sheets).collect();
            if let Some(c) = &b.color {
                for item in &items {
                    styled.push(style(&mut w, *item, c));
                }
            }
            // Faces with colours of their own.
            for (index, c) in &b.faces {
                if let Some(&face) = written.faces.get(*index).filter(|id| **id != 0) {
                    styled.push(style(&mut w, face, c));
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
                format!("{}:{}", products.get(*ci).map(product_name).unwrap_or_default(), k + 1)
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
    let _ = writeln!(text, "FILE_DESCRIPTION(({}),'2;1');", string(&header.description));
    let _ = writeln!(
        text,
        "FILE_NAME({},{},({}),({}),{},{},'');",
        string(&header.file_name),
        string(&header.time_stamp),
        string(&header.author),
        string(&header.organization),
        string(&header.preprocessor_version),
        string(&header.originating_system)
    );
    text.push_str("FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }'));\nENDSEC;\nDATA;\n");
    text.push_str(&w.data());
    text.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    Ok(text)
}

/// A product's name as written: `Part` when it is blank.
fn product_name(p: &ExportProduct) -> String {
    if p.name.trim().is_empty() { "Part".to_string() } else { p.name.clone() }
}

/// The deepest assembly written: a product tree with more levels is refused as recursive.
const MAX_LEVELS: usize = 65;

/// Whether the tree under `root` contains a product inside itself, or has more than
/// [`MAX_LEVELS`] levels. Every child must be one of the products.
///
/// A depth-first walk that colours products as it goes (unvisited, on the current path, done)
/// and keeps for each done product its height (the levels from it down, itself included). A
/// product shared by several parents is walked once; reaching it again at a level checks the
/// levels below it from its height. The recursion is at most [`MAX_LEVELS`] deep.
fn recursive(products: &[ExportProduct], root: usize) -> bool {
    walk_tree(products, root).0
}

/// [`recursive`]'s answer and how many times the walk entered a product: at most once per
/// product plus once per child reference.
fn walk_tree(products: &[ExportProduct], root: usize) -> (bool, usize) {
    #[derive(Clone, Copy)]
    enum Mark {
        New,
        OnPath,
        Done(usize),
    }
    /// The height of `p`, entered at `level` (the root at 0); `None` for a cycle or too many levels.
    fn walk(products: &[ExportProduct], marks: &mut [Mark], visits: &mut usize, p: usize, level: usize) -> Option<usize> {
        *visits += 1;
        if level >= MAX_LEVELS {
            return None;
        }
        match marks.get(p).copied()? {
            Mark::OnPath => return None,
            Mark::Done(height) => return (level + height <= MAX_LEVELS).then_some(height),
            Mark::New => {}
        }
        if let Some(m) = marks.get_mut(p) {
            *m = Mark::OnPath;
        }
        let mut below = 0;
        for (c, _, _) in products.get(p).map(|x| x.children.as_slice()).unwrap_or_default() {
            below = below.max(walk(products, marks, visits, *c, level + 1)?);
        }
        if let Some(m) = marks.get_mut(p) {
            *m = Mark::Done(below + 1);
        }
        Some(below + 1)
    }
    let mut visits = 0;
    let bad = walk(products, &mut vec![Mark::New; products.len()], &mut visits, root, 0).is_none();
    (bad, visits)
}

/// A surface colour for an item (STYLED_ITEM chain; the styles are shared), named as its look is
/// (`COLOUR_RGB`'s name, read back on import) when it has a name.
fn style(w: &mut Writer, item: u64, colour: &Colour) -> u64 {
    let Colour { rgb: c, opacity, name } = colour;
    let opacity = *opacity;
    let rgb = w.add("COLOUR_RGB", &[s(name), Param::Real(c[0] as f64), Param::Real(c[1] as f64), Param::Real(c[2] as f64)]);
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

/// STEP text for the bodies of one part, named after the file (`Part` when it has no name).
///
/// # Errors
///
/// As [`products`].
pub fn bodies(bodies: Vec<ExportBody<'_>>, header: &Header) -> Result<String> {
    let name =
        if header.file_name.is_empty() { "Part".to_string() } else { strip_suffix(strip_suffix(&header.file_name, ".step"), ".stp").to_string() };
    let p = ExportProduct { name, bodies, children: Vec::new() };
    products(&[p], 0, header)
}

/// `name` without every trailing `suffix`, compared without regard to ASCII case.
fn strip_suffix<'a>(mut name: &'a str, suffix: &str) -> &'a str {
    while let Some(cut) = name.len().checked_sub(suffix.len())
        && let (Some(head), Some(tail)) = (name.get(..cut), name.get(cut..))
        && tail.eq_ignore_ascii_case(suffix)
    {
        name = head;
    }
    name
}

/// STEP text for solidcore bodies as one part, each named: their records
/// ([`solidcore_ops::Body::to_record`]) written by [`bodies`], with no colours.
///
/// # Errors
///
/// [`ExportError::Record`] where a body's record cannot be made; otherwise as [`products`].
pub fn solidcore_bodies(named: &[(&str, &solidcore_ops::Body)], header: &Header) -> Result<String> {
    let mut records = Vec::new();
    for (name, body) in named {
        let (record, _) = body.to_record().map_err(|e| ExportError::Record { name: (*name).to_string(), reason: e.to_string() })?;
        records.push(record);
    }
    let list = named
        .iter()
        .zip(&records)
        .map(|((name, _), record)| ExportBody { name: (*name).to_string(), record, color: None, faces: Vec::new() })
        .collect();
    bodies(list, header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p21;
    use crate::write::real;

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

    /// A solidcore box and a cut cylinder written in one call: one part with two named solids,
    /// AP242, and the errors for nothing to write.
    #[test]
    fn solidcore_bodies_write_as_one_part() {
        let bx = solidcore_ops::make_box([0.0; 3], [10.0, 20.0, 5.0]).unwrap();
        let cyl = solidcore_ops::make_cylinder([30.0, 0.0, 0.0], 2.0, 8.0).unwrap();
        let text = solidcore_bodies(&[("Block", &bx), ("Pin", &cyl)], &Header { file_name: "parts.step".into(), ..Default::default() }).unwrap();
        assert!(text.contains("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF"));
        assert_eq!(text.matches("MANIFOLD_SOLID_BREP(").count(), 2, "{text}");
        assert!(text.contains("MANIFOLD_SOLID_BREP('Block'") && text.contains("MANIFOLD_SOLID_BREP('Pin'"));
        assert!(text.contains("PRODUCT('parts','parts'"));
        assert_eq!(solidcore_bodies(&[], &Header::default()), Err(ExportError::Nothing));
    }

    /// An assembly placing a part (a box) once by `m` in an empty root.
    fn placed(m: [[f64; 4]; 4]) -> Result<String> {
        let bx = solidcore_ops::make_box([0.0; 3], [1.0, 2.0, 3.0]).unwrap();
        let (record, _) = bx.to_record().unwrap();
        let part = ExportProduct {
            name: "Part".into(),
            bodies: vec![ExportBody { name: "B".into(), record: &record, color: None, faces: Vec::new() }],
            children: vec![],
        };
        let root = ExportProduct { name: "Root".into(), bodies: vec![], children: vec![(1, m, String::new())] };
        products(&[root, part], 0, &Header::default())
    }

    const IDENTITY: [[f64; 4]; 4] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

    /// A rotation about z by a quarter turn and a translation is rigid and writes.
    #[test]
    fn a_rotation_and_translation_writes() {
        let m = [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [50.0, -3.0, 7.0, 1.0]];
        let text = placed(m).unwrap();
        assert!(text.contains("NEXT_ASSEMBLY_USAGE_OCCURRENCE("), "{text}");
        assert!(placed(IDENTITY).is_ok());
    }

    /// A z axis that is not the unit x × y is refused, though x and y are a rotation's and z lies
    /// along x × y enough for `(x × y)·z` to pass: `[100, 0, 1]` and `[0, 0, 2]` were written as
    /// placement axes.
    #[test]
    fn a_z_axis_that_is_not_x_cross_y_is_not_rigid() {
        for z in [[100.0, 0.0, 1.0], [0.0, 0.0, 2.0], [0.0, 1e-3, 1.0]] {
            let mut m = IDENTITY;
            m[2] = [z[0], z[1], z[2], 0.0];
            assert_eq!(placed(m), Err(ExportError::NotRigid), "{z:?}");
        }
    }

    /// A homogeneous bottom row other than `0 0 0 1` (column-major: `m[0..3][3]` and `m[3][3]`) is
    /// refused: it was dropped, and the part written at an identity placement.
    #[test]
    fn a_non_affine_matrix_is_not_rigid() {
        for (col, v) in [(3, 2.0), (3, 0.0), (0, 0.5), (1, -1e-3), (2, 1.0)] {
            let mut m = IDENTITY;
            if let Some(slot) = m.get_mut(col).and_then(|c| c.get_mut(3)) {
                *slot = v;
            }
            assert_eq!(placed(m), Err(ExportError::NotRigid), "m[{col}][3] = {v}");
        }
    }

    /// A body only in a product the root does not reach is not written, so there is nothing to
    /// export: it wrote a file with no body.
    #[test]
    fn a_body_the_root_does_not_reach_is_nothing_to_export() {
        let bx = solidcore_ops::make_box([0.0; 3], [1.0, 2.0, 3.0]).unwrap();
        let (record, _) = bx.to_record().unwrap();
        let root = ExportProduct { name: "Root".into(), bodies: vec![], children: vec![] };
        let stray = ExportProduct {
            name: "Stray".into(),
            bodies: vec![ExportBody { name: "B".into(), record: &record, color: None, faces: Vec::new() }],
            children: vec![],
        };
        let list = [root, stray];
        let err = products(&list, 0, &Header::default()).unwrap_err();
        assert_eq!(err, ExportError::Nothing);
        assert_eq!(err.to_string(), "nothing to export");
        // From the product that holds it, it writes.
        assert!(products(&list, 1, &Header::default()).is_ok());
    }

    /// The header names no application: a default header writes empty fields, and every field is
    /// written as the caller gives it.
    #[test]
    fn the_header_is_the_callers() {
        let bx = solidcore_ops::make_box([0.0; 3], [1.0, 2.0, 3.0]).unwrap();
        let text = solidcore_bodies(&[("Block", &bx)], &Header::default()).unwrap();
        assert!(text.contains("FILE_DESCRIPTION((''),'2;1');\nFILE_NAME('','',(''),(''),'','','');\n"), "{text}");
        assert!(!text.contains("SolidCraft"), "{text}");
        let header = Header {
            file_name: "f.step".into(),
            time_stamp: "2026-11-12T00:00:00".into(),
            author: "a".into(),
            organization: "o".into(),
            description: "d".into(),
            preprocessor_version: "p".into(),
            originating_system: "s".into(),
        };
        let text = solidcore_bodies(&[("Block", &bx)], &header).unwrap();
        assert!(text.contains("FILE_DESCRIPTION(('d'),'2;1');\nFILE_NAME('f.step','2026-11-12T00:00:00',('a'),('o'),'p','s','');\n"), "{text}");
    }

    /// A box's record, and a body of it named `name`.
    fn box_record() -> Brep {
        solidcore_ops::make_box([0.0; 3], [1.0, 2.0, 3.0]).unwrap().to_record().unwrap().0
    }
    fn body<'a>(name: &str, record: &'a Brep) -> ExportBody<'a> {
        ExportBody { name: name.into(), record, color: None, faces: Vec::new() }
    }

    /// `levels` products in a chain, each placing the next twice (a shared child), the last
    /// holding a box.
    fn shared_chain(record: &Brep, levels: usize) -> Vec<ExportProduct<'_>> {
        (0..levels)
            .map(|i| {
                let last = i + 1 == levels;
                ExportProduct {
                    name: format!("P{i}"),
                    bodies: if last { vec![body("B", record)] } else { vec![] },
                    children: if last { vec![] } else { vec![(i + 1, IDENTITY, String::new()), (i + 1, IDENTITY, String::new())] },
                }
            })
            .collect()
    }

    /// A tree that shares its children is checked once per product: the walk enters a product at
    /// most once per product and child reference, not once per path. 20 levels, each placing the
    /// next twice, have 2^19 paths (the old walk entered 2^20 - 1 times, and took hours from 40
    /// levels); 65 levels (2^64 paths) are allowed and 66 refused as too deep, as before.
    #[test]
    fn a_shared_tree_is_checked_in_linear_time() {
        let record = box_record();
        for levels in [20, 65, 66] {
            let tree = shared_chain(&record, levels);
            let references: usize = tree.iter().map(|p| p.children.len()).sum();
            let (bad, visits) = walk_tree(&tree, 0);
            assert_eq!(bad, levels > 65, "{levels}");
            assert!(visits <= tree.len() + references, "{levels} levels: {visits} visits");
        }
        // The writer itself: 65 products, 128 occurrences.
        let text = products(&shared_chain(&record, 65), 0, &Header::default()).unwrap();
        assert_eq!(text.matches("NEXT_ASSEMBLY_USAGE_OCCURRENCE(").count(), 128);
        assert_eq!(products(&shared_chain(&record, 66), 0, &Header::default()), Err(ExportError::Recursive));
    }

    /// A product reached first near the root and again deeper counts its levels from the deeper
    /// place: a 30-level chain under the root, reached again through 34 more levels, makes 65
    /// levels (allowed); through 35, 66 (refused). A cycle is refused.
    #[test]
    fn a_shared_product_counts_its_levels_where_it_is_deepest() {
        let record = box_record();
        let tree = |m: usize| -> Vec<ExportProduct<'_>> {
            let mut list = vec![ExportProduct {
                name: "Root".into(),
                bodies: vec![],
                children: vec![(1, IDENTITY, String::new()), (31, IDENTITY, String::new())],
            }];
            for i in 1..=30 {
                let children = if i == 30 { vec![] } else { vec![(i + 1, IDENTITY, String::new())] };
                list.push(ExportProduct { name: format!("X{i}"), bodies: if i == 30 { vec![body("B", &record)] } else { vec![] }, children });
            }
            for j in 0..m {
                let next = if j + 1 == m { 1 } else { 32 + j };
                list.push(ExportProduct { name: format!("Y{j}"), bodies: vec![], children: vec![(next, IDENTITY, String::new())] });
            }
            list
        };
        assert!(!recursive(&tree(34), 0));
        assert!(recursive(&tree(35), 0));
        let mut cycle = tree(3);
        if let Some(last) = cycle.get_mut(30) {
            last.children.push((31, IDENTITY, String::new()));
        }
        assert!(recursive(&cycle, 0));
    }

    /// A body whose record has no face writes no shape, so it is nothing to export.
    #[test]
    fn a_body_with_no_face_is_nothing_to_export() {
        let full = box_record();
        let empty = Brep { regions: vec![], shells: vec![], faces: vec![], edges: vec![], vertices: vec![], ..full.clone() };
        let header = Header::default();
        let err = bodies(vec![body("Empty", &empty)], &header).unwrap_err();
        assert_eq!(err, ExportError::Nothing);
        assert_eq!(err.to_string(), "nothing to export");
        assert!(bodies(vec![body("Empty", &empty), body("Box", &full)], &header).is_ok());
    }

    /// An unnamed occurrence of a blank-named product is named after the product as written,
    /// `Part:1`, not `:1`.
    #[test]
    fn an_unnamed_occurrence_of_a_blank_product_is_named_part() {
        let record = box_record();
        let list = [
            ExportProduct { name: "Root".into(), bodies: vec![], children: vec![(1, IDENTITY, String::new())] },
            ExportProduct { name: " ".into(), bodies: vec![body("B", &record)], children: vec![] },
        ];
        let text = products(&list, 0, &Header::default()).unwrap();
        assert!(text.contains("PRODUCT('Part','Part'"), "{text}");
        assert!(text.contains("NEXT_ASSEMBLY_USAGE_OCCURRENCE('Part:1','Part:1'"), "{text}");
    }

    /// A colour component or opacity that is not a number from 0 to 1 is refused (it was written
    /// as `0.` or out of range), and so is a face colour for a face the record does not have (it
    /// was dropped).
    #[test]
    fn bad_colours_and_faces_are_refused() {
        let record = box_record();
        let faces = record.faces.len();
        let ok = Colour { rgb: [0.0, 0.5, 1.0], opacity: 0.25, name: String::new() };
        let with = |color: Option<Colour>, face: Vec<(usize, Colour)>| {
            bodies(vec![ExportBody { name: "B".into(), record: &record, color, faces: face }], &Header::default())
        };
        assert!(with(Some(ok.clone()), vec![(faces - 1, ok.clone())]).is_ok());
        for bad in [[f32::NAN, 0.0, 0.0], [1.5, 0.0, 0.0], [0.0, -0.1, 0.0], [0.0, 0.0, f32::INFINITY]] {
            let c = Colour { rgb: bad, ..ok.clone() };
            assert_eq!(with(Some(c.clone()), vec![]), Err(ExportError::Colour { name: "B".into() }), "{bad:?}");
            assert_eq!(with(None, vec![(0, c)]), Err(ExportError::Colour { name: "B".into() }), "face {bad:?}");
        }
        for opacity in [f32::NAN, 2.0, -1.0] {
            assert_eq!(with(Some(Colour { opacity, ..ok.clone() }), vec![]), Err(ExportError::Colour { name: "B".into() }), "{opacity}");
        }
        assert_eq!(with(None, vec![(faces, ok.clone())]), Err(ExportError::Face { name: "B".into(), face: faces, faces }));
    }

    /// The part is named after the file without its `.step` or `.stp`, in any case.
    #[test]
    fn the_part_name_drops_the_extension_in_any_case() {
        let record = box_record();
        for (file, part) in [
            ("a.step", "a"),
            ("PARTS.STEP", "PARTS"),
            ("b.Stp", "b"),
            ("c.step.STEP", "c"),
            ("d.step.stp", "d.step"),
            ("é.STP", "é"),
            ("step", "step"),
        ] {
            let text = bodies(vec![body("B", &record)], &Header { file_name: file.into(), ..Default::default() }).unwrap();
            assert!(text.contains(&format!("PRODUCT({0},{0}", string(part))), "{file}: {text}");
        }
    }

    /// Geometry with a number that is not finite is refused, not written as `0.`: a vertex
    /// (checked before anything is written) and a plane's origin (counted by the writer).
    #[test]
    fn geometry_that_is_not_finite_is_refused() {
        let good = box_record();
        let mut vertex = good.clone();
        if let Some(v) = vertex.vertices.first_mut() {
            v.point[1] = f64::NAN;
        }
        assert_eq!(bodies(vec![body("V", &vertex)], &Header::default()), Err(ExportError::NotFinite { name: "V".into() }));
        let mut plane = good.clone();
        let face = plane.faces.iter_mut().find(|f| matches!(f.surface, solidcore_record::brep::Surface::Plane { .. })).unwrap();
        if let solidcore_record::brep::Surface::Plane { origin, .. } = &mut face.surface {
            origin[2] = f64::INFINITY;
        }
        assert_eq!(bodies(vec![body("P", &plane)], &Header::default()), Err(ExportError::NotFinite { name: "P".into() }));
        assert!(bodies(vec![body("B", &good)], &Header::default()).is_ok());
        // The writer counts every one, in lists and typed values too.
        let mut w = Writer::new();
        w.add("A", &[Param::Real(1.0), Param::Real(f64::NAN), Param::List(vec![Param::Real(f64::NEG_INFINITY)])]);
        w.add_complex(&[("B", vec![Param::Typed("T".into(), vec![Param::Real(f64::INFINITY)])])]);
        assert_eq!(w.non_finite(), 3);
    }

    /// The round of a cylinder's rim (R 10, round 2.5), as the application's kernel builds it, is
    /// a round of an exact class (a revolution, a torus in all but its label) in the record, whose net a reader measures a little
    /// off (volume, bounding box). STEP gets the torus (major radius 7.5, minor 2.5) the net
    /// proves as, and the record is unchanged.
    #[test]
    fn a_cylinder_rims_round_is_written_as_its_torus() {
        use solidcore_ops::blend::{Blend, Corners, blend};
        use solidcore_record::brep::Surface;
        use solidcore_record::brep::blend::{BlendDefinition, ExactNetKindRecord};
        solidcore_ops::session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
        let mut found = None;
        for edge in 0..3 {
            let puck = solidcore_ops::make_cylinder([0.0; 3], 10.0, 8.0).unwrap();
            if blend(&puck, Blend::Round, &[edge], &[2.5], &[], Corners::Filled).is_ok() {
                found = Some(puck);
                break;
            }
        }
        let puck = found.unwrap();
        let (record, _) = puck.to_record().unwrap();
        let exact_torus = |f: &solidcore_record::brep::Face| match &f.surface {
            Surface::Blend { definition } => {
                matches!(&**definition, BlendDefinition::ExactRound(e) if e.radius == 2.5 && matches!(e.net_kind, ExactNetKindRecord::Torus | ExactNetKindRecord::Revolution))
            }
            _ => false,
        };
        let shown = || record.faces.iter().map(|f| format!("{:.80?}", f.surface)).collect::<Vec<_>>();
        assert_eq!(record.faces.iter().filter(|f| exact_torus(f)).count(), 1, "{:?}", shown());
        let text = solidcore_bodies(&[("Puck", &puck)], &Header::default()).unwrap();
        assert_eq!(text.matches("TOROIDAL_SURFACE(").count(), 1, "{text}");
        assert_eq!(text.matches("B_SPLINE_SURFACE").count(), 0, "{text}");
        let torus = text.split("TOROIDAL_SURFACE(").nth(1).and_then(|t| t.split(");").next()).unwrap_or_default();
        let radii: Vec<f64> = torus.split(',').skip(2).filter_map(|v| v.trim().parse().ok()).collect();
        assert!(matches!(radii.as_slice(), [major, minor] if (major - 7.5).abs() < 1e-9 && (minor - 2.5).abs() < 1e-9), "{torus}");
        assert_eq!(text.matches("CYLINDRICAL_SURFACE(").count(), 1, "{text}");
    }

    /// A rounded rim whose round is bent off its torus by one control point is no quadric: its
    /// form proves as no kind, and the face stays the B-spline surface it is.
    #[test]
    fn a_free_form_round_stays_a_b_spline_surface() {
        use solidcore_ops::blend::{Blend, Corners, blend};
        use solidcore_record::brep::Surface;
        solidcore_ops::session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
        let mut found = None;
        for edge in 0..3 {
            let puck = solidcore_ops::make_cylinder([0.0; 3], 10.0, 8.0).unwrap();
            if blend(&puck, Blend::Round, &[edge], &[2.5], &[], Corners::Filled).is_ok() {
                found = Some(puck);
                break;
            }
        }
        let (mut record, _) = found.unwrap().to_record().unwrap();
        let mut bent = 0;
        for face in &mut record.faces {
            if !matches!(face.surface, Surface::Blend { .. }) {
                continue;
            }
            let Some(Surface::RationalSpline { u_knots, v_knots, mut homogeneous }) = face.nurbs.take() else { continue };
            if let Some(p) = homogeneous.get_mut(2).and_then(|row| row.get_mut(1)) {
                p[0] += 0.3;
            }
            face.surface = Surface::RationalSpline { u_knots, v_knots, homogeneous };
            bent += 1;
        }
        let shown = record.faces.iter().map(|f| format!("{:.50?} nurbs {}", f.surface, f.nurbs.is_some())).collect::<Vec<_>>();
        assert_eq!(bent, 1, "{shown:?}");
        let text = bodies(vec![body("Bent", &record)], &Header::default()).unwrap();
        // one rational B-spline surface (a complex entity naming its several parts), no torus
        assert_eq!(text.matches("(BOUNDED_SURFACE()B_SPLINE_SURFACE(").count(), 1, "{text}");
        assert_eq!(text.matches("TOROIDAL_SURFACE(").count(), 0, "{text}");
    }
}
