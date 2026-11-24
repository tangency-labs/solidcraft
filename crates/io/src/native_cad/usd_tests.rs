//! OpenUSD export and import, checked against analytically known boxes and placements, the
//! OpenUSD file headers, and the sRGB transfer function. No expected value is taken from
//! SolidCraft's own output.

use super::*;
use solidcraft_doc::appearance::Look;
use solidcraft_doc::{Document, FeatureKind, Model, ModelState, Operation, point_expr};

/// A 10 × 20 × 30 mm box with its corner at `at`, as a new body in component `component`.
fn add_box(doc: &mut Document, at: Vec3, component: u64) {
    let id = doc
        .add_feature(
            FeatureKind::Box {
                corner: point_expr(at),
                length: "10".into(),
                width: "20".into(),
                height: "30".into(),
                operation: Operation::NewBody,
                centred: false,
            },
            None,
        )
        .unwrap();
    doc.feature_mut(id).unwrap().component = component;
}

/// One box at the origin, and its model.
fn one_box() -> (Document, Model) {
    let mut doc = Document::new("Box");
    add_box(&mut doc, Vec3::ZERO, 0);
    let mut model = Model::new();
    model.evaluate(&doc);
    (doc, model)
}

/// The design as an OpenUSD file called `file`, as `file.export` writes it: `bodies` chosen (all
/// when empty), at `quality`.
fn usd_at(doc: &Document, model: &Model, bodies: &[String], file: &str, quality: crate::MeshQuality) -> (Vec<u8>, Vec<String>) {
    let world = solidcraft_doc::world_state(doc, &model.state()).unwrap();
    crate::usd_from_design(doc, &model.state(), &world, bodies, &crate::CadmpegOptions::default(), file, quality).unwrap()
}

/// The whole design as an OpenUSD file called `file`, at standard quality.
fn usd_of(doc: &Document, model: &Model, file: &str) -> (Vec<u8>, Vec<String>) {
    usd_at(doc, model, &[], file, crate::MeshQuality::Standard)
}

/// [`usd_of`] as text.
fn usda(doc: &Document, model: &Model) -> String {
    String::from_utf8(usd_of(doc, model, "design.usda").0).unwrap()
}

/// The design's model as CADIR, as an OpenUSD export builds it.
fn cadir(doc: &Document, model: &Model) -> super::super::cadir_out::Built {
    let world = solidcraft_doc::world_state(doc, &model.state()).unwrap();
    super::super::cadir_out::design(doc, &model.state(), &world, &[], &crate::CadmpegOptions::default(), "design", crate::MeshQuality::Standard)
        .unwrap()
}

/// The values of input `input` of every shader in `text`.
fn inputs(text: &str, input: &str) -> Vec<f64> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix(&format!("float inputs:{input} = ")))
        .map(|v| v.trim().parse().unwrap_or_else(|_| panic!("{input} = {v}")))
        .collect()
}

/// A frame part holding a needle part placed twice: the first needle turned a quarter about Z and
/// moved 100 mm along X, the second moved 50 mm along Y (as the GLB tests' galvanometer).
fn galvanometer() -> (Document, Model) {
    let mut doc = Document::new("Galvanometer");
    let frame = doc.add_component(Some("Frame"), 0).unwrap();
    let needle = doc.add_component(Some("Needle"), frame).unwrap();
    add_box(&mut doc, Vec3::ZERO, frame);
    add_box(&mut doc, Vec3::new(5.0, 0.0, 0.0), needle);
    let turned = solidcraft_doc::rigid(Vec3::new(100.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2);
    doc.occurrences.iter_mut().find(|o| o.component == needle).unwrap().transform = turned;
    doc.add_occurrence(needle, frame, solidcraft_doc::rigid(Vec3::new(0.0, 50.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0)).unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    (doc, model)
}

/// A chain of components nested two levels past [`solidcraft_doc::MAX_PLACEMENT_DEPTH`] is
/// written as far as the design's own walk (the display's) reaches it: one occurrence for each
/// placement that walk holds, its deepest one level past the depth, and a warning for the rest.
#[test]
fn placements_are_written_as_deep_as_the_design_walks_them() {
    let mut doc = Document::new("Chain");
    add_box(&mut doc, Vec3::ZERO, 0);
    let mut parent = 0;
    for k in 0..solidcraft_doc::MAX_PLACEMENT_DEPTH + 2 {
        parent = doc.add_component(Some(&format!("Level {k}")), parent).unwrap();
    }
    let mut model = Model::new();
    model.evaluate(&doc);
    let walked = doc.placements();
    assert_eq!(walked.iter().map(|(_, path, _)| path.len()).max(), Some(solidcraft_doc::MAX_PLACEMENT_DEPTH + 1));
    let built = cadir(&doc, &model);
    // The design's walk holds the root too, as the written root occurrence.
    assert_eq!(built.ir.model.occurrences.len(), walked.len(), "{:?}", built.warnings);
    assert!(built.warnings.iter().any(|w| w.contains("too large or too deep")), "{:?}", built.warnings);
}

/// Each mesh body's box, `[min, max]` in millimetres, sorted, with the import flattened so every
/// mesh is where the file places it.
fn boxes(mut g: GltfImport) -> Vec<[[f64; 3]; 2]> {
    g.flatten();
    let FeatureKind::MeshImport { meshes, .. } = &g.feature.kind else { panic!("not a mesh import") };
    let mut out: Vec<[[f64; 3]; 2]> = meshes
        .iter()
        .map(|m| {
            let mut b = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
            for p in m.positions.as_chunks::<3>().0 {
                for k in 0..3 {
                    b[0][k] = b[0][k].min(p[k]);
                    b[1][k] = b[1][k].max(p[k]);
                }
            }
            b
        })
        .collect();
    out.sort_by(|a, b| a.partial_cmp(b).unwrap());
    out
}

fn assert_box(got: &[[f64; 3]; 2], want: [[f64; 3]; 2]) {
    for (g, w) in got.iter().flatten().zip(want.iter().flatten()) {
        // USD mesh points are single precision.
        assert!((g - w).abs() < 1e-3, "{got:?} is not {want:?}");
    }
}

#[test]
fn extensions() {
    for p in ["a.usd", "b.USDA", "c.usdc", "d.UsdZ"] {
        assert!(is_usd_path(p), "{p}");
        assert!(crate::is_mesh_path(p), "{p}");
        assert_eq!(crate::Format::from_name(p).unwrap(), crate::Format::Usd, "{p}");
    }
    assert!(!is_usd_path("e.us") && !is_usd_path("usd") && !is_usd_path("f.step"));
    assert_eq!([target("x.usda"), target("x.usdc"), target("x.usd"), target("x.usdz")], [Some("usda"), Some("usdc"), Some("usdc"), Some("usdz")]);
}

/// Each extension writes its encoding, with the header OpenUSD gives it: `#usda 1.0` for text,
/// `PXR-USDC` for binary, and a ZIP for a package. The layer says it is in metres with Z up, and
/// each encoding reads back as the 10 × 20 × 30 mm box.
#[test]
fn a_box_exports_as_each_encoding_and_reads_back() {
    let (doc, model) = one_box();
    for (file, magic) in [("box.usda", &b"#usda 1.0"[..]), ("box.usdc", b"PXR-USDC"), ("box.usd", b"PXR-USDC"), ("box.usdz", b"PK\x03\x04")] {
        let (bytes, warnings) = usd_of(&doc, &model, file);
        assert!(bytes.starts_with(magic), "{file}: {:?}", bytes.get(..16));
        assert!(warnings.is_empty(), "{file}: {warnings:?}");
        let (g, y_up) = crate::read_usd(&bytes, file).unwrap();
        assert!(!y_up, "{file}: the layer says Z is up");
        let got = boxes(g);
        assert_eq!(got.len(), 1, "{file}");
        assert_box(&got[0], [[0.0, 0.0, 0.0], [10.0, 20.0, 30.0]]);
    }
    let text = usda(&doc, &model);
    assert!(text.contains("metersPerUnit = 1\n") && text.contains("upAxis = \"Z\""), "{text}");
    // The exact B-rep goes with the mesh.
    assert!(text.contains("def BrepArray") && text.contains("def Mesh"), "{text}");
    // The root prim is named after the design, not `Occurrence`.
    assert!(text.contains("defaultPrim = \"Box\""), "{}", &text[..200.min(text.len())]);
}

/// A part placed twice is written once, under the abstract `Prototypes` prim, and referenced at
/// each placement; read back, each body is where the design places it.
#[test]
fn an_assembly_keeps_its_components_and_placements() {
    let (doc, model) = galvanometer();
    let (bytes, warnings) = usd_of(&doc, &model, "galvanometer.usda");
    assert!(warnings.is_empty(), "{warnings:?}");
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert_eq!(text.matches("def Mesh").count(), 2, "the frame's mesh and the needle's, once:\n{text}");
    assert_eq!(text.matches("prepend references = </Prototypes/").count(), 2, "{text}");
    let (g, _) = crate::read_usd(&bytes, "galvanometer.usda").unwrap();
    assert!(g.plan.slots.iter().any(|s| s.starts_with("Frame")), "{:?}", g.plan.slots);
    assert!(g.plan.slots.iter().any(|s| s.starts_with("Needle")), "{:?}", g.plan.slots);
    // Built at its first placement only, the needle's second placement is another occurrence.
    let needle = g.plan.slots.iter().position(|s| s.starts_with("Needle")).unwrap();
    assert_eq!(g.plan.places.iter().filter(|p| p.slot == needle).count(), 2);
    // The frame box; the needle (5 to 15 along X) turned a quarter about Z, then moved 100 mm
    // along X: X from 80 to 100 and Y from 5 to 15. Only its first placement is built.
    let got = boxes(g);
    assert_eq!(got.len(), 2, "{got:?}");
    assert_box(&got[0], [[0.0, 0.0, 0.0], [10.0, 20.0, 30.0]]);
    assert_box(&got[1], [[80.0, 5.0, 0.0], [100.0, 15.0, 30.0]]);
}

/// A body's sRGB colour is written as the linear colour USD expects, and reads back as the same
/// sRGB colour.
#[test]
fn colours_are_linear_in_the_file_and_srgb_when_read() {
    let (mut doc, model) = one_box();
    doc.appearances.bodies.insert(model.state().bodies[0].name.clone(), Look::custom([255, 128, 0], 1.0));
    let (bytes, _) = usd_of(&doc, &model, "orange.usda");
    let text = String::from_utf8(bytes.clone()).unwrap();
    // sRGB 128/255 is linear ((128/255 + 0.055) / 1.055)^2.4 = 0.21586.
    let line = text.lines().find(|l| l.contains("inputs:diffuseColor")).unwrap_or_else(|| panic!("no diffuseColor:\n{text}"));
    let green: f64 = line.split(['(', ',']).nth(2).unwrap().trim().parse().unwrap();
    assert!((green - 0.215_86).abs() < 1e-4, "{line}");
    let (g, _) = crate::read_usd(&bytes, "orange.usda").unwrap();
    let look = g.looks[0].as_ref().unwrap();
    assert_eq!(look.color, [255, 128, 0]);
    assert_eq!(look.opacity, 1.0);
}

/// A layer written by hand, as another tool writes one: in centimetres with Y up, a group moved
/// along X holding a mesh, a mesh placed twice through instancing, a mesh scaled by its prim,
/// a material with metallic and roughness, and a prim that is invisible.
const HAND_WRITTEN: &str = r#"#usda 1.0
(
    defaultPrim = "Scene"
    metersPerUnit = 0.01
    upAxis = "Y"
)

def Material "Steel"
{
    token outputs:surface.connect = </Steel/Surface.outputs:surface>
    def Shader "Surface"
    {
        uniform token info:id = "UsdPreviewSurface"
        color3f inputs:diffuseColor = (0.21586, 0.21586, 0.21586)
        float inputs:metallic = 1
        float inputs:roughness = 0.25
        token outputs:surface
    }
}

class "Proto"
{
    def Mesh "Leg"
    {
        int[] faceVertexCounts = [3, 3, 3, 3]
        int[] faceVertexIndices = [0, 2, 1, 0, 1, 3, 1, 2, 3, 2, 0, 3]
        point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0), (0, 0, 1)]
        uniform token subdivisionScheme = "none"
    }
}

def Xform "Scene"
{
    def Xform "Base"
    {
        double3 xformOp:translate = (10, 0, 0)
        uniform token[] xformOpOrder = ["xformOp:translate"]
        def Mesh "Plate" (
            prepend apiSchemas = ["MaterialBindingAPI"]
        )
        {
            int[] faceVertexCounts = [3, 3, 3, 3]
            int[] faceVertexIndices = [0, 2, 1, 0, 1, 3, 1, 2, 3, 2, 0, 3]
            point3f[] points = [(0, 0, 0), (2, 0, 0), (0, 2, 0), (0, 0, 2)]
            uniform token subdivisionScheme = "none"
            rel material:binding = </Steel>
        }
    }
    def "LegA" (
        instanceable = true
        references = </Proto>
    )
    {
        double3 xformOp:translate = (0, 5, 0)
        uniform token[] xformOpOrder = ["xformOp:translate"]
    }
    def "LegB" (
        instanceable = true
        references = </Proto>
    )
    {
        double3 xformOp:translate = (0, 0, 5)
        uniform token[] xformOpOrder = ["xformOp:translate"]
    }
    def Mesh "Stretched"
    {
        double3 xformOp:scale = (3, 1, 1)
        uniform token[] xformOpOrder = ["xformOp:scale"]
        int[] faceVertexCounts = [3, 3, 3, 3]
        int[] faceVertexIndices = [0, 2, 1, 0, 1, 3, 1, 2, 3, 2, 0, 3]
        point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0), (0, 0, 1)]
        uniform token subdivisionScheme = "none"
    }
    def Mesh "Hidden"
    {
        token visibility = "invisible"
        int[] faceVertexCounts = [3, 3, 3, 3]
        int[] faceVertexIndices = [0, 2, 1, 0, 1, 3, 1, 2, 3, 2, 0, 3]
        point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0), (0, 0, 1)]
        uniform token subdivisionScheme = "none"
    }
}
"#;

#[test]
fn a_hand_written_layer_reads_as_components_in_millimetres() {
    let (g, y_up) = crate::read_usd(HAND_WRITTEN.as_bytes(), "scene.usda").unwrap();
    assert!(y_up, "the layer says Y is up");
    let warnings = g.feature.warnings.join("\n");
    // The leg is one component placed twice.
    let leg = g.plan.slots.iter().position(|s| s.starts_with("Leg")).unwrap_or_else(|| panic!("{:?}", g.plan.slots));
    assert_eq!(g.plan.places.iter().filter(|p| p.slot == leg).count(), 2, "{:?}", g.plan);
    // The plate's material: linear 0.21586 is sRGB 128, metallic 1 and roughness 0.25.
    let FeatureKind::MeshImport { meshes, .. } = &g.feature.kind else { panic!() };
    let plate = meshes.iter().position(|m| m.name == "Plate").unwrap_or_else(|| panic!("{:?}", g.feature.body_names));
    let look = g.looks[plate].as_ref().unwrap();
    assert_eq!(look.color, [128, 128, 128]);
    let pbr = look.pbr.unwrap();
    assert_eq!((pbr.metallic(), pbr.roughness()), (1.0, 0.25));
    assert!(!g.feature.body_names.iter().any(|n| n == "Hidden"), "{:?}", g.feature.body_names);
    assert!(warnings.contains("invisible"), "{warnings}");
    // Centimetres to millimetres: the plate's 2 cm tetrahedron moved 10 cm along X; one leg
    // (1 cm) moved 5 cm along Y (only the first placement is built); the stretched one scaled 3
    // times along X, baked into its mesh.
    let got = boxes(g);
    assert_eq!(got.len(), 3, "{got:?}");
    assert_box(&got[0], [[0.0, 0.0, 0.0], [30.0, 10.0, 10.0]]);
    assert_box(&got[1], [[0.0, 50.0, 0.0], [10.0, 60.0, 10.0]]);
    assert_box(&got[2], [[100.0, 0.0, 0.0], [120.0, 20.0, 20.0]]);
}

/// A mesh's authored normals turn with its prim's rotation when the import is flattened, as
/// its points do: a face normal along the file's +X, under a quarter turn about Z, points along
/// +Y.
#[test]
fn authored_normals_turn_with_their_prim_when_flattened() {
    let layer = "#usda 1.0\n(\n    metersPerUnit = 0.001\n    upAxis = \"Z\"\n)\ndef Xform \"Turned\"\n{\n    \
        float xformOp:rotateZ = 90\n    uniform token[] xformOpOrder = [\"xformOp:rotateZ\"]\n    def Mesh \"Tet\"\n    {\n        \
        int[] faceVertexCounts = [3, 3, 3, 3]\n        int[] faceVertexIndices = [0, 2, 1, 0, 1, 3, 1, 2, 3, 2, 0, 3]\n        \
        point3f[] points = [(0, 0, 0), (10, 0, 0), (0, 10, 0), (0, 0, 10)]\n        \
        normal3f[] normals = [(1, 0, 0), (1, 0, 0), (1, 0, 0), (1, 0, 0)] (\n            interpolation = \"vertex\"\n        )\n    }\n}\n";
    let (mut g, _) = crate::read_usd(layer.as_bytes(), "turned.usda").unwrap();
    let FeatureKind::MeshImport { meshes, .. } = &g.feature.kind else { panic!() };
    assert!(!meshes[0].corner_normals.is_empty(), "the file's normals are read");
    g.flatten();
    let FeatureKind::MeshImport { meshes, .. } = &g.feature.kind else { panic!() };
    for n in meshes[0].corner_normals.as_chunks::<3>().0 {
        assert!(n[0].abs() < 1e-6 && (n[1] - 1.0).abs() < 1e-6 && n[2].abs() < 1e-6, "{n:?}");
    }
}

/// What cannot be read is refused or left out with a message, never a panic.
#[test]
fn other_contents_and_empty_layers_are_refused() {
    for (bytes, file) in [
        (&b"ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n"[..], "part.usda"),
        (b"#usda 1.0\n", "empty.usda"),
        (b"", "nothing.usdc"),
        (b"PK\x03\x04garbage", "broken.usdz"),
        (b"#usda 1.0\ndef Cube \"C\"\n{\n}\n", "cube.usda"),
    ] {
        let e = crate::read_usd(bytes, file).err().unwrap_or_else(|| panic!("{file} was read"));
        assert!(e.to_string().contains(file), "{file}: {e}");
    }
    let (doc, model) = one_box();
    let st = model.state();
    let e = crate::usd_from_design(&doc, &st, &st, &[], &crate::CadmpegOptions::default(), "x.step", crate::MeshQuality::Standard).unwrap_err();
    assert!(e.contains("not an OpenUSD file name"), "{e}");
    // Nothing to write: an empty design, or a body it does not have.
    let empty = Document::new("Empty");
    let none = ModelState::default();
    assert!(crate::usd_from_design(&empty, &none, &none, &[], &crate::CadmpegOptions::default(), "x.usda", crate::MeshQuality::Standard).is_err());
    assert!(
        crate::usd_from_design(&doc, &st, &st, &["Nothing".into()], &crate::CadmpegOptions::default(), "x.usda", crate::MeshQuality::Standard)
            .is_err()
    );
}

/// Mutations of a valid text layer, binary layer and package each give a result or an error,
/// never a panic, within the time budget (#469).
#[test]
fn hostile_usd_never_panics() {
    use solidcraft_testkit::{mutations, survives};
    let read = |file: &'static str| move |b: &[u8]| crate::read_usd(b, file).ok().map(|(g, _)| g.feature.body_names.len());
    survives("OpenUSD (.usda)", &mutations(HAND_WRITTEN.as_bytes(), true, 0x05da), usize::MAX, read("fuzz.usda"));
    let (doc, model) = galvanometer();
    for (file, seed) in [("fuzz.usdc", 0x05dc), ("fuzz.usdz", 0x05d2)] {
        let (bytes, _) = usd_of(&doc, &model, file);
        assert!(crate::read_usd(&bytes, file).is_ok(), "{file}");
        survives(&format!("OpenUSD ({file})"), &mutations(&bytes, false, seed), usize::MAX, read(file));
    }
}

/// A cylinder of radius 5 mm and height 20 mm standing on the origin.
fn one_cylinder() -> (Document, Model) {
    let mut doc = Document::new("Cylinder");
    doc.add_feature(
        FeatureKind::Cylinder {
            base: point_expr(Vec3::ZERO),
            axis: solidcraft_doc::dir_expr(Vec3::Z),
            radius: "5".into(),
            height: "20".into(),
            operation: Operation::NewBody,
            references: Vec::new(),
        },
        None,
    )
    .unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    (doc, model)
}

/// An OpenUSD file read for its exact geometry, as an Import feature keeping records.
fn exact(bytes: &[u8], file: &str) -> (crate::StepFeature, bool) {
    crate::usd_import_feature(bytes, file, crate::StepNames::default()).unwrap_or_else(|e| panic!("{file}: {e}"))
}

/// Each body the import built: whether it is a mesh, its volume and its box, `[min, max]` in
/// millimetres, sorted by box.
fn exact_bodies(sf: &crate::StepFeature) -> Vec<(bool, f64, [[f64; 3]; 2])> {
    let mut out: Vec<(bool, f64, [[f64; 3]; 2])> = sf
        .import
        .bodies
        .iter()
        .map(|b| {
            let m = solidcraft_kernel::measure(&b.body).unwrap();
            (b.body.is_mesh(), m.volume, [[m.bbox.min.x, m.bbox.min.y, m.bbox.min.z], [m.bbox.max.x, m.bbox.max.y, m.bbox.max.z]])
        })
        .collect();
    out.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());
    out
}

fn assert_close(got: &[[f64; 3]; 2], want: [f64; 6], tol: f64) {
    for (g, w) in got.iter().flatten().zip(want) {
        assert!((g - w).abs() < tol, "{got:?} is not {want:?}");
    }
}

/// Read for its exact geometry, SolidCraft's box comes back from each encoding as one B-rep body
/// (not its mesh, which only shows the B-rep and is not read a second time), with the box's
/// volume, 10 × 20 × 30 = 6000 mm³, its corners, and its name.
#[test]
fn exact_reads_a_box_as_its_brep_once() {
    let (doc, model) = one_box();
    for file in ["box.usda", "box.usdc", "box.usd", "box.usdz"] {
        let (bytes, _) = usd_of(&doc, &model, file);
        let (sf, y_up) = exact(&bytes, file);
        assert!(!y_up, "{file}: the layer says Z is up");
        let FeatureKind::Import { records: Some(records), .. } = &sf.kind else { panic!("{file}: not an Import feature with records") };
        assert!(records.bodies.iter().all(|b| b.brep.is_some() && b.mesh.is_none()), "{file}");
        let got = exact_bodies(&sf);
        assert_eq!(got.len(), 1, "{file}: {got:?} {:?}", sf.warnings);
        let (mesh, volume, bbox) = got[0];
        assert!(!mesh, "{file}");
        assert!((volume - 6000.0).abs() < 1e-6, "{file}: {volume}");
        assert_close(&bbox, [0.0, 0.0, 0.0, 10.0, 20.0, 30.0], 1e-9);
        assert_eq!(sf.name, "Box", "{file}");
        assert!(sf.import.bodies[0].closed && sf.import.bodies[0].refused.is_none(), "{file}");
    }
}

/// A B-rep the kernel cannot build still comes in, as the mesh the file shows it with, saying
/// why. cadmpeg 0.6.12's USD writer widens a full turn past 2π (cadmpeg/cadmpeg#1648): the
/// cylinder's side is written as a rational NURBS whose U runs from -6.3e-6 to 2π + 6.3e-6, so
/// it overlaps itself and does not close, and the kernel rightly refuses it. It comes back as its
/// mesh, the cylinder of radius 5 mm and height 20 mm, a little inside π·5²·20 mm³; once the
/// writer is fixed this reads exact (300π mm³).
#[test]
fn exact_reads_a_brep_it_cannot_build_as_its_mesh() {
    let (doc, model) = one_cylinder();
    let (bytes, _) = usd_of(&doc, &model, "cylinder.usda");
    let (sf, _) = exact(&bytes, "cylinder.usda");
    let got = exact_bodies(&sf);
    assert_eq!(got.len(), 1, "{got:?} {:?}", sf.warnings);
    let want = std::f64::consts::PI * 25.0 * 20.0;
    let (mesh, volume, bbox) = got[0];
    if mesh {
        assert!(sf.warnings.iter().any(|w| w.contains("came in as the mesh")), "{:?}", sf.warnings);
        assert!(volume < want && volume > 0.98 * want, "{volume} vs {want}");
        assert_close(&bbox, [-5.0, -5.0, 0.0, 5.0, 5.0, 20.0], 0.05);
    } else {
        // Once the seam closes, the exact cylinder.
        assert!((volume - want).abs() < 1e-6 * want, "{volume} vs {want}");
        assert_close(&bbox, [-5.0, -5.0, 0.0, 5.0, 5.0, 20.0], 1e-6);
    }
}

/// The frame and its needle placed twice come back as B-rep bodies where the design places
/// them, in a product tree that names the frame and the needle; the needle's second placement is
/// the same part placed again.
#[test]
fn exact_reads_an_assembly_with_its_placements() {
    let (doc, model) = galvanometer();
    let (bytes, _) = usd_of(&doc, &model, "galvanometer.usda");
    let (sf, _) = exact(&bytes, "galvanometer.usda");
    let FeatureKind::Import { records: Some(records), .. } = &sf.kind else { panic!("not an Import feature with records") };
    assert_eq!(records.bodies.iter().filter(|b| b.copy_of.is_some()).count(), 1, "the needle's second placement is a copy");
    let got = exact_bodies(&sf);
    // The frame box; the needle (5 to 15 along X) turned a quarter about Z and moved 100 mm
    // along X (X from 80 to 100, Y from 5 to 15); and moved 50 mm along Y instead.
    let want = [[0.0, 0.0, 0.0, 10.0, 20.0, 30.0], [5.0, 50.0, 0.0, 15.0, 70.0, 30.0], [80.0, 5.0, 0.0, 100.0, 15.0, 30.0]];
    assert_eq!(got.len(), 3, "{got:?} {:?}", sf.warnings);
    for ((mesh, volume, bbox), w) in got.iter().zip(want) {
        assert!(!mesh && (volume - 6000.0).abs() < 1e-6, "{got:?}");
        assert_close(bbox, w, 1e-6);
    }
    fn walk(nodes: &[solidcraft_kernel::StepNode], out: &mut Vec<String>) {
        for n in nodes {
            out.push(n.name.clone());
            walk(&n.children, out);
        }
    }
    let mut names = Vec::new();
    walk(&sf.import.tree, &mut names);
    assert!(names.iter().any(|n| n.starts_with("Frame")) && names.iter().any(|n| n.starts_with("Needle")), "{names:?}");
}

/// A 2 mm tetrahedron at 100 mm along X, as a mesh prim of a millimetre layer.
const TET_PRIM: &str = "def Mesh \"Tet\"\n{\n    int[] faceVertexCounts = [3, 3, 3, 3]\n    \
    int[] faceVertexIndices = [0, 2, 1, 0, 1, 3, 1, 2, 3, 2, 0, 3]\n    \
    point3f[] points = [(100, 0, 0), (102, 0, 0), (100, 2, 0), (100, 0, 2)]\n    uniform token subdivisionScheme = \"none\"\n}\n";

/// A layer holding SolidCraft's box (a B-rep with its mesh) and a mesh prim with no B-rep: the
/// box comes in as its B-rep, and the mesh as a mesh body, each once. A layer of meshes alone
/// comes in as mesh bodies.
#[test]
fn exact_reads_a_mesh_with_no_brep_as_a_mesh_body() {
    let (doc, model) = one_box();
    let (bytes, _) = usd_of(&doc, &model, "box.usda");
    // The box layer is in metres: the tetrahedron's points are too.
    let tet = TET_PRIM.replace("(100, 0, 0), (102, 0, 0), (100, 2, 0), (100, 0, 2)", "(0.1, 0, 0), (0.102, 0, 0), (0.1, 0.002, 0), (0.1, 0, 0.002)");
    let text = format!("{}\n{tet}", String::from_utf8(bytes).unwrap());
    let (sf, _) = exact(text.as_bytes(), "mixed.usda");
    let got = exact_bodies(&sf);
    assert_eq!(got.len(), 2, "{got:?} {:?}", sf.warnings);
    assert!(!got[0].0 && (got[0].1 - 6000.0).abs() < 1e-6, "{got:?}");
    // The tetrahedron's volume is 2³ / 6 mm³ (single-precision points).
    assert!(got[1].0 && (got[1].1 - 8.0 / 6.0).abs() < 1e-4, "{got:?}");
    assert_close(&got[1].2, [100.0, 0.0, 0.0, 102.0, 2.0, 2.0], 1e-4);

    let (sf, y_up) = exact(HAND_WRITTEN.as_bytes(), "scene.usda");
    assert!(y_up, "the layer says Y is up");
    assert!(!sf.import.bodies.is_empty() && sf.import.bodies.iter().all(|b| b.body.is_mesh()), "{:?}", sf.warnings);
}

/// A flat 10 × 10 mm bilinear `NurbsPatch`, in metres.
const PATCH_PRIM: &str = "def NurbsPatch \"Sheet\"\n{\n    int uVertexCount = 2\n    int vVertexCount = 2\n    int uOrder = 2\n    int vOrder = 2\n    \
    double[] uKnots = [0, 0, 1, 1]\n    double[] vKnots = [0, 0, 1, 1]\n    double2 uRange = (0, 1)\n    double2 vRange = (0, 1)\n    \
    point3f[] points = [(0.2, 0, 0), (0.21, 0, 0), (0.2, 0.01, 0), (0.21, 0.01, 0)]\n}\n";

/// cadmpeg 0.6.12 reads a `NurbsPatch` as a sheet whose one face has no boundary (its trim curves
/// are not read), which the records cannot build yet: the patch is left out naming it and why,
/// and the rest of the stage comes in. A stage of nothing else is refused, saying why.
#[test]
fn exact_leaves_out_a_nurbs_patch_saying_why() {
    let (doc, model) = one_box();
    let (bytes, _) = usd_of(&doc, &model, "box.usda");
    let text = format!("{}\n{PATCH_PRIM}", String::from_utf8(bytes).unwrap());
    let (sf, _) = exact(text.as_bytes(), "patched.usda");
    let got = exact_bodies(&sf);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(!got[0].0 && (got[0].1 - 6000.0).abs() < 1e-6, "{got:?}");
    assert!(sf.warnings.iter().any(|w| w.contains("Sheet") && w.contains("no boundary")), "{:?}", sf.warnings);
    let alone = format!("#usda 1.0\n(\n    metersPerUnit = 1\n    upAxis = \"Z\"\n)\n{PATCH_PRIM}");
    let e = crate::usd_import_feature(alone.as_bytes(), "sheet.usda", crate::StepNames::default()).err().unwrap().to_string();
    assert!(e.contains("sheet.usda") && e.contains("no boundary"), "{e}");
}

/// A material's linear colour comes in as the sRGB colour it shows: SolidCraft's orange box
/// (sRGB 255, 128, 0) reads back orange through the exact route too.
#[test]
fn exact_reads_colours_as_srgb() {
    let (mut doc, model) = one_box();
    doc.appearances.bodies.insert(model.state().bodies[0].name.clone(), Look::custom([255, 128, 0], 1.0));
    let (bytes, _) = usd_of(&doc, &model, "orange.usda");
    let (sf, _) = exact(&bytes, "orange.usda");
    let c = sf.import.bodies[0].color.unwrap_or_else(|| panic!("no colour: {:?}", sf.warnings));
    assert!((c[0] - 1.0).abs() < 1e-3 && (c[1] - 128.0 / 255.0).abs() < 1e-3 && c[2].abs() < 1e-3, "{c:?}");
}

/// Mutations of a text layer, binary layer and package holding B-reps each give a result or an
/// error through the exact route, never a panic; what cannot be read is refused naming the file.
#[test]
fn hostile_usd_through_exact_never_panics() {
    use solidcraft_testkit::{mutations, survives};
    let read =
        |file: &'static str| move |b: &[u8]| crate::usd_import_feature(b, file, crate::StepNames::default()).ok().map(|(f, _)| f.body_names.len());
    let (doc, model) = galvanometer();
    for (file, seed) in [("fuzz.usda", 0x05e1), ("fuzz.usdc", 0x05e2), ("fuzz.usdz", 0x05e3)] {
        let (bytes, _) = usd_of(&doc, &model, file);
        assert!(crate::usd_import_feature(&bytes, file, crate::StepNames::default()).is_ok(), "{file}");
        survives(&format!("OpenUSD exact ({file})"), &mutations(&bytes, file.ends_with(".usda"), seed), usize::MAX, read(file));
    }
    for (bytes, file) in [(&b"#usda 1.0\n"[..], "empty.usda"), (b"", "nothing.usdc"), (b"PK\x03\x04garbage", "broken.usdz")] {
        let e = crate::usd_import_feature(bytes, file, crate::StepNames::default()).err().unwrap_or_else(|| panic!("{file} was read"));
        assert!(e.to_string().contains(file), "{file}: {e}");
    }
}

/// The names the design gives reach the prims: each occurrence's prim is named after the
/// occurrence (`:` is not allowed in a prim name, so `_` stands for it), the part placed twice is
/// written under `Prototypes` named after its component, and the frame's body prim is named after
/// the body. (A part of one body placed twice is written as that body's prim, which cadmpeg names
/// after the component.)
#[test]
fn component_occurrence_and_body_names_reach_the_prims() {
    let (doc, model) = galvanometer();
    let text = usda(&doc, &model);
    for o in &doc.occurrences {
        let prim = format!("def \"{}\"", o.name.replace(':', "_"));
        let xform = format!("def Xform \"{}\"", o.name.replace(':', "_"));
        assert!(text.contains(&prim) || text.contains(&xform), "no prim named {:?}:\n{text}", o.name);
    }
    let prototypes = text.find("class \"Prototypes\"").unwrap_or_else(|| panic!("no Prototypes:\n{text}"));
    assert!(text[prototypes..].contains("def Xform \"Needle\""), "{text}");
    let frame = doc.components.iter().find(|c| c.name == "Frame").unwrap().id;
    let frame_body = model.state().bodies.iter().find(|b| doc.model_body_component(b) == frame).unwrap().name.clone();
    assert!(text[..prototypes].contains(&format!("def Xform \"{frame_body}\"")), "no prim named after body {frame_body}:\n{text}");
    // Read back, the components keep their names.
    let (g, _) = crate::read_usd(text.as_bytes(), "galvanometer.usda").unwrap();
    assert!(g.plan.slots.iter().any(|s| s.starts_with("Frame")), "{:?}", g.plan.slots);
}

/// Chosen bodies are written where the design shows them, each a prim named after it, under one
/// root named after the design.
#[test]
fn chosen_bodies_are_named_after_themselves() {
    let mut doc = Document::new("Pair");
    add_box(&mut doc, Vec3::ZERO, 0);
    add_box(&mut doc, Vec3::new(40.0, 0.0, 0.0), 0);
    let mut model = Model::new();
    model.evaluate(&doc);
    let names: Vec<String> = model.state().bodies.iter().map(|b| b.name.clone()).collect();
    let (bytes, warnings) = usd_at(&doc, &model, &names, "pair.usda", crate::MeshQuality::Standard);
    assert!(warnings.is_empty(), "{warnings:?}");
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("defaultPrim = \"Pair\""), "{text}");
    for n in &names {
        assert!(text.contains(&format!("def Xform \"{n}\"")), "no prim named {n}:\n{text}");
    }
    let (g, _) = crate::read_usd(text.as_bytes(), "pair.usda").unwrap();
    let got = boxes(g);
    assert_eq!(got.len(), 2);
    assert_box(&got[0], [[0.0, 0.0, 0.0], [10.0, 20.0, 30.0]]);
    assert_box(&got[1], [[40.0, 0.0, 0.0], [50.0, 20.0, 30.0]]);
}

/// A look's physical parameters are written as the `UsdPreviewSurface` inputs of the same names,
/// and a transparent look's opacity as `opacity`, with the values the looks were given.
#[test]
fn pbr_and_transparent_looks_write_their_values() {
    let mut doc = Document::new("Looks");
    add_box(&mut doc, Vec3::ZERO, 0);
    add_box(&mut doc, Vec3::new(40.0, 0.0, 0.0), 0);
    let mut model = Model::new();
    model.evaluate(&doc);
    let names: Vec<String> = model.state().bodies.iter().map(|b| b.name.clone()).collect();
    let mut steel = Look::custom([128, 128, 128], 1.0);
    steel.name = "Steel".into();
    steel.pbr = Some(solidcraft_doc::pbr_appearance::PbrParameters::new(0.75, 0.25, 1.5).unwrap());
    let mut glass = Look::custom([0, 128, 255], 0.375);
    glass.name = "Glass".into();
    doc.appearances.bodies.insert(names[0].clone(), steel);
    doc.appearances.bodies.insert(names[1].clone(), glass);
    let (bytes, warnings) = usd_of(&doc, &model, "looks.usda");
    assert!(warnings.is_empty(), "{warnings:?}");
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("def Material \"Steel\"") && text.contains("def Material \"Glass\""), "{text}");
    assert_eq!(inputs(&text, "metallic"), vec![0.75], "{text}");
    assert_eq!(inputs(&text, "roughness"), vec![0.25], "{text}");
    assert_eq!(inputs(&text, "ior"), vec![1.5], "{text}");
    assert_eq!(inputs(&text, "opacity"), vec![0.375], "{text}");
    // Each body is bound to its own material.
    assert!(text.contains("rel material:binding = </Looks/Steel>") || text.contains("/Looks/Steel>"), "{text}");
    assert!(text.contains("/Looks/Glass>"), "{text}");
}

/// A face with a look of its own: its triangles in the mesh are bound to that look's material; the
/// rest keep the body's. The top face of the 10 × 20 × 30 mm
/// box is the one at z = 30, so every triangle of its group lies there.
#[test]
fn a_face_with_its_own_look_gets_its_own_material() {
    let (mut doc, model) = one_box();
    let body = model.state().bodies[0].name.clone();
    doc.appearances.bodies.insert(body.clone(), Look::custom([200, 200, 200], 1.0));
    let mut red = Look::custom([255, 0, 0], 1.0);
    red.name = "Red".into();
    doc.appearances.faces.push(solidcraft_doc::appearance::FaceLook {
        body,
        point: Vec3::new(5.0, 10.0, 30.0),
        look: red,
        name: None,
        strict_name: false,
        certificate: None,
    });
    let built = cadir(&doc, &model);
    let model_ir = &built.ir.model;
    let t = &model_ir.tessellations[0];
    let groups = t.triangle_groups();
    let top = groups.iter().find(|g| g.source_id.is_some()).unwrap_or_else(|| panic!("no face group: {groups:?}"));
    let (points, triangles) = (t.vertices(), t.triangles());
    for &i in &top.triangles {
        for &v in &triangles[i as usize] {
            assert!((points[v as usize].get().z - 30.0).abs() < 1e-9, "a triangle of the face's group is off the top face");
        }
    }
    // The top face is two triangles at least, and the other five faces are not in its group.
    assert!(top.triangles.len() >= 2 && top.triangles.len() < triangles.len());
    let red = model_ir.appearances.iter().find(|a| a.name.as_deref() == Some("Red")).unwrap();
    let bound = |pick: fn(&cadmpeg_ir::appearance::AppearanceTarget) -> bool| {
        model_ir.appearance_bindings.iter().filter(|b| b.appearance == red.id && pick(&b.target)).count()
    };
    use cadmpeg_ir::appearance::AppearanceTarget as T;
    assert_eq!(bound(|t| matches!(t, T::TriangleGroup { .. })), 1);
    assert_eq!(bound(|t| matches!(t, T::Face(_))), 0);
    // In the file: a subset of the mesh bound to the face's material.
    let text = usda(&doc, &model);
    let red_bindings = text.lines().filter(|l| l.contains("rel material:binding") && l.contains("/Looks/Red>")).count();
    assert_eq!(red_bindings, 1, "{text}");
}

/// A placement that is not a rigid one is never written as one: the bodies are written where the
/// world has them, with a warning naming the occurrence.
#[test]
fn a_placement_that_is_not_rigid_is_written_where_the_world_has_it() {
    let (doc, model) = galvanometer();
    let st = model.state();
    let mut doc = doc.clone();
    let o = doc.occurrences.iter_mut().find(|o| o.name.starts_with("Needle")).unwrap();
    o.transform[0][0] = -1.0;
    let name = o.name.clone();
    let world = solidcraft_doc::world_state(&doc, &st).unwrap();
    let (bytes, warnings) =
        crate::usd_from_design(&doc, &st, &world, &[], &crate::CadmpegOptions::default(), "mirror.usda", crate::MeshQuality::Standard).unwrap();
    assert!(warnings.iter().any(|w| w.contains(&name)), "{warnings:?}");
    let mut want: Vec<[[f64; 3]; 2]> = world
        .bodies
        .iter()
        .map(|b| {
            let bb = solidcraft_kernel::measure(&b.body).unwrap().bbox;
            [[bb.min.x, bb.min.y, bb.min.z], [bb.max.x, bb.max.y, bb.max.z]]
        })
        .collect();
    want.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let (g, _) = crate::read_usd(&bytes, "mirror.usda").unwrap();
    let got = boxes(g);
    assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
    for (got, want) in got.iter().zip(&want) {
        for (g, w) in got.iter().flatten().zip(want.iter().flatten()) {
            // USD mesh points are single precision.
            assert!((g - w).abs() < 1e-3, "{got:?} is not {want:?}");
        }
    }
}

/// A placement that is not a finite number is never written as one and never panics: the bodies
/// are written where the world has them, with a warning naming the occurrence.
#[test]
fn nonfinite_placements_never_panic() {
    let (doc, model) = galvanometer();
    let st = model.state();
    for bad in [f64::NAN, f64::INFINITY] {
        let mut doc = doc.clone();
        let o = doc.occurrences.iter_mut().find(|o| o.name.starts_with("Needle")).unwrap();
        o.transform[0][0] = bad;
        let name = o.name.clone();
        let (bytes, warnings) =
            crate::usd_from_design(&doc, &st, &st, &[], &crate::CadmpegOptions::default(), "bad.usda", crate::MeshQuality::Standard).unwrap();
        assert!(warnings.iter().any(|w| w.contains(&name)), "{bad}: {warnings:?}");
        assert!(crate::read_usd(&bytes, "bad.usda").is_ok(), "{bad}");
    }
}

/// Placements of a component the design does not have are left out once, and the rest keep their
/// place among all their parent's children: a damaged design with many of them under a part
/// placed twice still finishes, each needle at the ordinal the design gives it.
#[test]
fn placements_of_missing_components_keep_the_others_ordinals() {
    let (mut doc, model) = galvanometer();
    let frame = doc.occurrences.iter().find(|o| o.name.starts_with("Frame")).unwrap().component;
    let needle = doc.occurrences.iter().find(|o| o.name.starts_with("Needle")).unwrap().clone();
    let missing = |i: u64, parent: u64| solidcraft_doc::Occurrence {
        id: u64::MAX - i,
        component: u64::MAX - i,
        parent,
        name: format!("Missing {i}"),
        ..needle.clone()
    };
    // One before the needles in their frame, and many under the needle placed twice.
    let at = doc.occurrences.iter().position(|o| o.component == needle.component).unwrap();
    doc.occurrences.insert(at, missing(0, frame));
    doc.occurrences.extend((1..5_000).map(|i| missing(i, needle.component)));
    let built = cadir(&doc, &model);
    let occurrences = &built.ir.model.occurrences;
    assert!(occurrences.iter().all(|o| !o.name.as_deref().unwrap_or("").starts_with("Missing")));
    let mut ordinals: Vec<u32> =
        occurrences.iter().filter(|o| o.name.as_deref().is_some_and(|n| n.starts_with("Needle"))).map(|o| o.ordinal).collect();
    ordinals.sort_unstable();
    assert_eq!(ordinals, [1, 2]);
}

/// The design as an OpenUSD text file, `bodies` chosen (all when empty), with what `hidden`
/// hides written hidden.
fn usda_hidden(doc: &Document, model: &Model, bodies: &[String], hidden: &crate::Hidden) -> (String, Vec<String>) {
    let world = solidcraft_doc::world_state(doc, &model.state()).unwrap();
    let options = crate::CadmpegOptions { hidden: hidden.clone(), ..Default::default() };
    let (bytes, warnings) =
        crate::usd_from_design(doc, &model.state(), &world, bodies, &options, "design.usda", crate::MeshQuality::Standard).unwrap();
    (String::from_utf8(bytes).unwrap(), warnings)
}

const INVISIBLE: &str = "token visibility = \"invisible\"";

/// The galvanometer's two needle placements, by path, and the needle body's name.
fn needles(doc: &Document, model: &Model) -> (Vec<Vec<u64>>, String) {
    let frame = doc.components.iter().find(|c| c.name == "Frame").unwrap().id;
    let needle = doc.components.iter().find(|c| c.name == "Needle").unwrap().id;
    let frame_at = doc.occurrences.iter().find(|o| o.component == frame).unwrap().id;
    let paths = doc.occurrences.iter().filter(|o| o.component == needle).map(|o| vec![frame_at, o.id]).collect();
    let body = model.state().bodies.iter().find(|b| doc.model_body_component(b) == needle).unwrap().name.clone();
    (paths, body)
}

/// A chosen body the view hides is written invisible; the other is not.
#[test]
fn a_hidden_body_is_written_invisible() {
    let mut doc = Document::new("Pair");
    add_box(&mut doc, Vec3::ZERO, 0);
    add_box(&mut doc, Vec3::new(40.0, 0.0, 0.0), 0);
    let mut model = Model::new();
    model.evaluate(&doc);
    let names: Vec<String> = model.state().bodies.iter().map(|b| b.name.clone()).collect();
    let hidden = crate::Hidden { bodies: [names[0].clone()].into(), placements: Vec::new() };
    let (text, warnings) = usda_hidden(&doc, &model, &names, &hidden);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(text.matches(INVISIBLE).count(), 1, "{text}");
    let first = text.find(&format!("def Xform \"{}\"", names[0])).unwrap();
    let second = text.find(&format!("def Xform \"{}\"", names[1])).unwrap();
    let invisible = text.find(INVISIBLE).unwrap();
    assert!(first < invisible && invisible < second, "the first body's prim holds it:\n{text}");
}

/// A placement the view hides is written as an invisible placement, once.
#[test]
fn a_hidden_placement_is_written_invisible() {
    let (doc, model) = galvanometer();
    let (paths, _) = needles(&doc, &model);
    let hidden = crate::Hidden { bodies: Default::default(), placements: vec![paths[0].clone()] };
    let (text, warnings) = usda_hidden(&doc, &model, &[], &hidden);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(text.matches(INVISIBLE).count(), 1, "{text}");
}

/// A component's body hidden in every placement is written hidden once, in the component; hidden
/// in one placement of two, it is written shown, with a warning.
#[test]
fn a_body_hidden_in_its_placements_is_written_hidden_and_in_some_warns() {
    let (doc, model) = galvanometer();
    let (paths, body) = needles(&doc, &model);
    let needle = doc.components.iter().find(|c| c.name == "Needle").unwrap().id;
    let state = model.state();
    let names = solidcraft_doc::WorldNames::new(&doc, &state);
    let placed: Vec<String> = paths.iter().map(|p| names.name(&body, needle, p)).collect();
    let all = crate::Hidden { bodies: placed.iter().cloned().collect(), placements: Vec::new() };
    let (text, warnings) = usda_hidden(&doc, &model, &[], &all);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(text.matches(INVISIBLE).count(), 1, "{text}");
    let one = crate::Hidden { bodies: [placed[0].clone()].into(), placements: Vec::new() };
    let (text, warnings) = usda_hidden(&doc, &model, &[], &one);
    assert_eq!(text.matches(INVISIBLE).count(), 0, "{text}");
    assert!(warnings.iter().any(|w| w.contains(&body) && w.contains("hidden in 1 of the 2 shown placements")), "{warnings:?}");
}

/// A component's only body keeps its name when the component is placed once, and the design's
/// only body when it is exported alone: a prim named after the body inside the placement's or
/// the design's.
#[test]
fn an_only_body_keeps_its_name_when_placed_once() {
    let mut doc = Document::new("Holder");
    let bracket = doc.add_component(Some("Bracket"), 0).unwrap();
    add_box(&mut doc, Vec3::ZERO, bracket);
    let mut model = Model::new();
    model.evaluate(&doc);
    let body = model.state().bodies[0].name.clone();
    let placement = doc.occurrences.iter().find(|o| o.component == bracket).unwrap().name.replace(':', "_");
    let text = usda(&doc, &model);
    let at = text.find(&format!("\"{placement}\"")).unwrap_or_else(|| panic!("no prim {placement}:\n{text}"));
    assert!(text[at..].contains(&format!("def Xform \"{body}\"")), "{text}");

    let (doc, model) = one_box();
    let body = model.state().bodies[0].name.clone();
    let text = usda(&doc, &model);
    assert!(text.contains("defaultPrim = \"Box\""), "{text}");
    let at = text.find("def Xform \"Box\"").unwrap_or_else(|| panic!("no prim Box:\n{text}"));
    assert!(text[at..].contains(&format!("def Xform \"{body}\"")), "{text}");
}

/// With physics, a frame placed twice grounds its grounded needle in both placements, as the
/// design does, and every placement of a part with mass is a rigid body, joined or not: the two
/// frames and the four needles.
#[test]
fn physics_grounds_every_placement_and_makes_each_placement_a_rigid_body() {
    let (mut doc, _) = galvanometer();
    let frame = doc.find_component("Frame").unwrap();
    doc.add_occurrence(frame, 0, solidcraft_doc::rigid(Vec3::new(0.0, 200.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0)).unwrap();
    // Only the needle is grounded (the design grounds its first part, the frame, by itself).
    let needle = doc.find_component("Needle").unwrap();
    doc.occurrences.iter_mut().for_each(|o| o.grounded = false);
    doc.occurrences.iter_mut().find(|o| o.component == needle).unwrap().grounded = true;
    let mut model = Model::new();
    model.evaluate(&doc);
    let world = solidcraft_doc::world_state(&doc, &model.state()).unwrap();
    let options = crate::CadmpegOptions { physics: true, ..Default::default() };
    let built = super::super::cadir_out::design(&doc, &model.state(), &world, &[], &options, "design", crate::MeshQuality::Standard).unwrap();
    let grounded = built.ir.model.assembly_joints.iter().filter(|j| j.native_ref.as_deref().is_some_and(|r| r.ends_with(" grounded"))).count();
    assert_eq!(grounded, 2, "{:?}", built.warnings);
    assert_eq!(built.ir.model.rigid_bodies.len(), 6, "{:?}", built.warnings);
}

/// A 2 × 2 RGBA PNG.
fn tiny_png() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[255u8; 16]).unwrap();
    }
    out
}

/// One box's surface plan: a base-colour image projected along x and y, 10 mm a repeat along x
/// and 20 mm along y, the image's v as stored.
fn box_surface(body: &str) -> crate::SurfacePlan {
    let bytes = tiny_png();
    crate::SurfacePlan {
        setup: "Studio".into(),
        definition: body.into(),
        component: 0,
        path: Vec::new(),
        world: body.into(),
        origin: [0.0; 3],
        u: [10.0, 0.0, 0.0],
        v: [0.0, 20.0, 0.0],
        flip_v: false,
        normal_strength: 1.0,
        base_color: Some(crate::SurfaceMap { sha256: "f00d".into(), bytes: std::sync::Arc::from(bytes) }),
        normal: None,
        roughness: None,
        metallic: None,
    }
}

/// The member names of the package `bytes`.
fn members(bytes: &[u8]) -> Vec<String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().unwrap().to_string()).collect()
}

/// The text of `name` inside the package `bytes`.
fn member(bytes: &[u8], name: &str) -> Option<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut file = zip.by_name(name).ok()?;
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut out).unwrap();
    Some(out)
}

/// The values of the array attribute `head` (such as `point3f[] points`) in `text`, flat.
fn array(text: &str, head: &str) -> Vec<f64> {
    let at = text.find(head).unwrap_or_else(|| panic!("no {head}:\n{text}"));
    let rest = &text[at + head.len()..];
    let body = &rest[rest.find('[').unwrap() + 1..rest.find(']').unwrap()];
    body.split(['(', ')', ',']).filter_map(|v| v.trim().parse().ok()).collect()
}

/// A render setup's image is written into a package with the mesh's coordinates in it: each
/// point's (s, t) is its x over 10 mm and minus its y over 20 mm (the image read from its
/// bottom-left in the file, from its top-left by the renderer), and the image is a member.
#[test]
fn a_render_setup_image_is_packaged_with_the_projections_coordinates() {
    let (doc, model) = one_box();
    let body = model.state().bodies[0].name.clone();
    let world = solidcraft_doc::world_state(&doc, &model.state()).unwrap();
    let options = crate::CadmpegOptions { surfaces: vec![box_surface(&body)], ..Default::default() };
    let (bytes, warnings) = crate::usd_from_design(&doc, &model.state(), &world, &[], &options, "box.usdz", crate::MeshQuality::Standard).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let root = members(&bytes).into_iter().find(|n| n.ends_with(".usda") || n.ends_with(".usdc")).unwrap();
    let text = String::from_utf8(member(&bytes, &root).unwrap()).unwrap();
    assert!(text.contains("UsdUVTexture"), "{text}");
    assert_eq!(member(&bytes, "textures/f00d.png"), Some(tiny_png()));
    let points = array(&text, "point3f[] points");
    let st = array(&text, "texCoord2f[] primvars:st");
    assert_eq!(points.len() / 3, st.len() / 2, "{text}");
    for (p, c) in points.chunks(3).zip(st.chunks(2)) {
        // The layer is in metres.
        let (x, y) = (p[0] * 1000.0, p[1] * 1000.0);
        assert!((c[0] - x / 10.0).abs() < 1e-4 && (c[1] + y / 20.0).abs() < 1e-4, "{p:?} gives {c:?}");
    }
}

/// A render setup's images are written only in a package: a text layer warns and keeps the look.
#[test]
fn a_render_setup_image_outside_a_package_warns() {
    let (doc, model) = one_box();
    let body = model.state().bodies[0].name.clone();
    let world = solidcraft_doc::world_state(&doc, &model.state()).unwrap();
    let options = crate::CadmpegOptions { surfaces: vec![box_surface(&body)], ..Default::default() };
    let (bytes, warnings) = crate::usd_from_design(&doc, &model.state(), &world, &[], &options, "box.usda", crate::MeshQuality::Standard).unwrap();
    assert!(warnings.iter().any(|w| w.contains("written only in a .usdz package")), "{warnings:?}");
    assert!(!String::from_utf8(bytes).unwrap().contains("UsdUVTexture"));
}

/// A component placed twice with an image on one placement only is written with its look, with a
/// warning: the file gives a component's body one mesh.
#[test]
fn an_image_on_one_placement_of_two_warns() {
    let (doc, model) = galvanometer();
    let (paths, body) = needles(&doc, &model);
    let needle = doc.components.iter().find(|c| c.name == "Needle").unwrap().id;
    let mut plan = box_surface(&body);
    plan.component = needle;
    plan.path = paths[0].clone();
    let world = solidcraft_doc::world_state(&doc, &model.state()).unwrap();
    let options = crate::CadmpegOptions { surfaces: vec![plan], ..Default::default() };
    let (bytes, warnings) = crate::usd_from_design(&doc, &model.state(), &world, &[], &options, "g.usdz", crate::MeshQuality::Standard).unwrap();
    assert!(warnings.iter().any(|w| w.contains(&body) && w.contains("different images")), "{warnings:?}");
    assert!(!members(&bytes).iter().any(|n| n.starts_with("textures/")));
}

/// The galvanometer built for OpenUSD with `options`.
fn built_with(doc: &Document, model: &Model, world: &ModelState, options: &crate::CadmpegOptions) -> super::super::cadir_out::Built {
    super::super::cadir_out::design(doc, &model.state(), world, &[], options, "design", crate::MeshQuality::Standard).unwrap()
}

/// `plan` with its base-colour image named `sha`.
fn with_image(mut plan: crate::SurfacePlan, sha: &str) -> crate::SurfacePlan {
    plan.base_color = Some(crate::SurfaceMap { sha256: sha.into(), bytes: std::sync::Arc::from(tiny_png()) });
    plan
}

/// A hidden placement's image does not count: the needle's shown placement has image B, its
/// hidden one image A, and B is written with no warning.
#[test]
fn the_shown_placements_image_is_written_when_a_hidden_one_differs() {
    let (doc, model) = galvanometer();
    let (paths, body) = needles(&doc, &model);
    let needle = doc.components.iter().find(|c| c.name == "Needle").unwrap().id;
    let plan = |path: &Vec<u64>, sha: &str| {
        let mut p = with_image(box_surface(&body), sha);
        p.component = needle;
        p.path = path.clone();
        p
    };
    let world = solidcraft_doc::world_state(&doc, &model.state()).unwrap();
    let options = crate::CadmpegOptions {
        hidden: crate::Hidden { placements: vec![paths[0].clone()], ..Default::default() },
        surfaces: vec![plan(&paths[0], "aaaa"), plan(&paths[1], "bbbb")],
        ..Default::default()
    };
    let built = built_with(&doc, &model, &world, &options);
    assert!(!built.warnings.iter().any(|w| w.contains("different images")), "{:?}", built.warnings);
    let assets: Vec<&str> = built.ir.model.assets.iter().map(|a| a.id.as_str()).collect();
    assert!(assets.iter().any(|a| a.ends_with("bbbb")) && !assets.iter().any(|a| a.ends_with("aaaa")), "{assets:?}");
}

/// Two placements with the same base colour and different half-strength normal maps, which are
/// left out of both, share the base colour.
#[test]
fn normal_maps_left_out_do_not_tell_placements_apart() {
    let (doc, model) = galvanometer();
    let (paths, body) = needles(&doc, &model);
    let needle = doc.components.iter().find(|c| c.name == "Needle").unwrap().id;
    let plan = |path: &Vec<u64>, normal: &str| {
        let mut p = box_surface(&body);
        p.component = needle;
        p.path = path.clone();
        p.normal_strength = 0.5;
        p.normal = Some(crate::SurfaceMap { sha256: normal.into(), bytes: std::sync::Arc::from(tiny_png()) });
        p
    };
    let world = solidcraft_doc::world_state(&doc, &model.state()).unwrap();
    let options = crate::CadmpegOptions { surfaces: vec![plan(&paths[0], "n1"), plan(&paths[1], "n2")], ..Default::default() };
    let built = built_with(&doc, &model, &world, &options);
    assert!(!built.warnings.iter().any(|w| w.contains("different images")), "{:?}", built.warnings);
    assert!(built.ir.model.assets.iter().any(|a| a.id.as_str().ends_with("f00d")), "{:?}", built.warnings);
}

/// Two bodies with no look of their own, one imported red and one blue, given the same roughness
/// image, keep their own colours: two textured appearances, not one.
#[test]
fn bodies_of_different_colours_with_one_image_keep_their_colours() {
    let mut doc = Document::new("Pair");
    add_box(&mut doc, Vec3::ZERO, 0);
    add_box(&mut doc, Vec3::new(50.0, 0.0, 0.0), 0);
    let mut model = Model::new();
    model.evaluate(&doc);
    let mut world = solidcraft_doc::world_state(&doc, &model.state()).unwrap();
    for (b, colour) in world.bodies.iter_mut().zip([[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]) {
        b.imported = Some(std::sync::Arc::new(solidcraft_doc::appearance::ImportedLook { color: Some(colour), ..Default::default() }));
    }
    let surfaces = world
        .bodies
        .iter()
        .map(|b| {
            let mut p = box_surface(&b.name);
            p.roughness = p.base_color.take();
            p
        })
        .collect();
    let options = crate::CadmpegOptions { surfaces, ..Default::default() };
    let built = built_with(&doc, &model, &world, &options);
    let textured: Vec<_> = built.ir.model.appearances.iter().filter(|a| !a.textures.is_empty()).map(|a| a.base_color).collect();
    assert_eq!(textured.len(), 2, "{textured:?}");
    assert_ne!(textured[0], textured[1], "{textured:?}");
}

/// The pictures shown on the model are written with the whole design only: a picture belongs to
/// no body, so an export of chosen bodies cannot tell its own from another body's, and leaves them
/// out with a warning.
#[test]
fn chosen_bodies_leave_the_pictures_out() {
    let (doc, model) = one_box();
    let body = model.state().bodies[0].name.clone();
    let world = solidcraft_doc::world_state(&doc, &model.state()).unwrap();
    let corners = [[0.0, 0.0, 30.0], [10.0, 0.0, 30.0], [10.0, 5.0, 30.0], [0.0, 5.0, 30.0]];
    let image = crate::SurfaceMap { sha256: "f00d".into(), bytes: std::sync::Arc::from(tiny_png()) };
    let decals = vec![crate::DecalPlan { name: "Logo".into(), component: 0, local: corners, world: corners, image }];
    let options = crate::CadmpegOptions { decals, ..Default::default() };
    let logo = |built: &super::super::cadir_out::Built| built.ir.model.bodies.iter().any(|b| b.name.as_deref() == Some("Logo"));
    let all = super::super::cadir_out::design(&doc, &model.state(), &world, &[], &options, "design", crate::MeshQuality::Standard).unwrap();
    assert!(logo(&all), "{:?}", all.warnings);
    let chosen = super::super::cadir_out::design(&doc, &model.state(), &world, &[body], &options, "design", crate::MeshQuality::Standard).unwrap();
    assert!(!logo(&chosen));
    assert!(chosen.warnings.iter().any(|w| w.contains("pictures shown on the model are not written")), "{:?}", chosen.warnings);
}

/// `plan`'s optics for body `body`: glass of index `ior`, without absorption.
fn glass(body: &str, ior: f64) -> crate::OpticsPlan {
    crate::OpticsPlan {
        setup: "Glass".into(),
        definition: body.into(),
        component: 0,
        path: Vec::new(),
        world: body.into(),
        ior,
        absorption_per_mm: [0.0; 3],
        thin_sheet: false,
    }
}

/// The OpenPBR transmission properties of the appearances of `built` with optics, by index.
fn optical(built: &super::super::cadir_out::Built) -> Vec<(Option<f64>, Option<[f32; 4]>)> {
    built
        .ir
        .model
        .appearances
        .iter()
        .filter(|a| a.properties.keys().any(|k| k.as_str().ends_with("transmission_weight")))
        .map(|a| {
            let ior = a.properties.iter().find(|(k, _)| k.as_str() == "ior").map(|(_, v)| v.get());
            (ior, a.base_color.map(|c| [c.r(), c.g(), c.b(), c.a()]))
        })
        .collect()
}

/// A hidden placement's optics do not count: the needle's shown placement is glass of index
/// 1.52, its hidden one 1.33, and 1.52 is written with no warning.
#[test]
fn the_shown_placements_optics_are_written_when_a_hidden_one_differs() {
    let (doc, model) = galvanometer();
    let (paths, body) = needles(&doc, &model);
    let needle = doc.components.iter().find(|c| c.name == "Needle").unwrap().id;
    let plan = |path: &Vec<u64>, ior: f64| {
        let mut p = glass(&body, ior);
        p.component = needle;
        p.path = path.clone();
        p
    };
    let world = solidcraft_doc::world_state(&doc, &model.state()).unwrap();
    let options = crate::CadmpegOptions {
        hidden: crate::Hidden { placements: vec![paths[0].clone()], ..Default::default() },
        optics: vec![plan(&paths[0], 1.33), plan(&paths[1], 1.52)],
        ..Default::default()
    };
    let built = built_with(&doc, &model, &world, &options);
    assert!(!built.warnings.iter().any(|w| w.contains("different optics")), "{:?}", built.warnings);
    let iors: Vec<Option<f64>> = optical(&built).into_iter().map(|(ior, _)| ior).collect();
    assert_eq!(iors, [Some(1.52)], "{:?}", built.warnings);
}

/// Two bodies with no look of their own, one imported red and one blue, given the same optics,
/// keep their own colours: two glass appearances, not one.
#[test]
fn bodies_of_different_colours_with_one_glass_keep_their_colours() {
    let mut doc = Document::new("Pair");
    add_box(&mut doc, Vec3::ZERO, 0);
    add_box(&mut doc, Vec3::new(50.0, 0.0, 0.0), 0);
    let mut model = Model::new();
    model.evaluate(&doc);
    let mut world = solidcraft_doc::world_state(&doc, &model.state()).unwrap();
    for (b, colour) in world.bodies.iter_mut().zip([[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]) {
        b.imported = Some(std::sync::Arc::new(solidcraft_doc::appearance::ImportedLook { color: Some(colour), ..Default::default() }));
    }
    let optics = world.bodies.iter().map(|b| glass(&b.name, 1.5)).collect();
    let options = crate::CadmpegOptions { optics, ..Default::default() };
    let built = built_with(&doc, &model, &world, &options);
    let colours: Vec<Option<[f32; 4]>> = optical(&built).into_iter().map(|(_, c)| c).collect();
    assert_eq!(colours.len(), 2, "{colours:?}");
    assert_ne!(colours[0], colours[1], "{colours:?}");
}

/// A metallic look on a glass body is written with OpenPBR's metalness 0 beside its transmission
/// in the glass material, so the file's glass transmits; that material's preview surface keeps
/// the look's metallic 1.
#[test]
fn a_metallic_look_on_glass_is_written_transmitting() {
    let (mut doc, model) = one_box();
    let body = model.state().bodies[0].name.clone();
    let mut steel = Look::custom([128, 128, 128], 1.0);
    steel.name = "Steel".into();
    steel.pbr = Some(solidcraft_doc::pbr_appearance::PbrParameters::new(1.0, 0.25, 1.5).unwrap());
    doc.appearances.bodies.insert(body.clone(), steel);
    let world = solidcraft_doc::world_state(&doc, &model.state()).unwrap();
    let options = crate::CadmpegOptions { optics: vec![glass(&body, 1.52)], ..Default::default() };
    let (bytes, _) = crate::usd_from_design(&doc, &model.state(), &world, &[], &options, "glass.usda", crate::MeshQuality::Standard).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    // The glass material: from the `def Material` before its transmission to the next one.
    let at = text.find("float inputs:transmission_weight = 1").unwrap_or_else(|| panic!("no transmission:\n{text}"));
    let start = text[..at].rfind("def Material").unwrap();
    let end = text[at..].find("def Material").map_or(text.len(), |n| at + n);
    let glass = &text[start..end];
    assert!(glass.contains("float inputs:base_metalness = 0") && !glass.contains("float inputs:base_metalness = 1"), "{glass}");
    assert!(glass.contains("float inputs:metallic = 1"), "{glass}");
}
