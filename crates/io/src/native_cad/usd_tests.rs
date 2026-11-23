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

/// The bodies of `state` as STEP, as `file.export` writes them.
fn step_of(doc: &Document, state: &ModelState) -> Vec<u8> {
    crate::export(&doc.painted(state), &[], crate::Format::Step, "Box").unwrap()
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
    let step = step_of(&doc, &model.state());
    for (file, magic) in [("box.usda", &b"#usda 1.0"[..]), ("box.usdc", b"PXR-USDC"), ("box.usd", b"PXR-USDC"), ("box.usdz", b"PK\x03\x04")] {
        let (bytes, warnings) = crate::usd_from_step(&step, file).unwrap();
        assert!(bytes.starts_with(magic), "{file}: {:?}", bytes.get(..16));
        assert!(warnings.is_empty(), "{file}: {warnings:?}");
        let (g, y_up) = crate::read_usd(&bytes, file).unwrap();
        assert!(!y_up, "{file}: the layer says Z is up");
        let got = boxes(g);
        assert_eq!(got.len(), 1, "{file}");
        assert_box(&got[0], [[0.0, 0.0, 0.0], [10.0, 20.0, 30.0]]);
    }
    let (text, _) = crate::usd_from_step(&step, "box.usda").unwrap();
    let text = String::from_utf8(text).unwrap();
    assert!(text.contains("metersPerUnit = 1\n") && text.contains("upAxis = \"Z\""), "{text}");
    // The exact B-rep goes with the mesh.
    assert!(text.contains("def BrepArray") && text.contains("def Mesh"), "{text}");
    // The root prim is named after the part, not `Occurrence`.
    assert!(text.contains("defaultPrim = \"Box\""), "{}", &text[..200.min(text.len())]);
}

/// A part placed twice is written once, under the abstract `Prototypes` prim, and referenced at
/// each placement; read back, each body is where the design places it.
#[test]
fn an_assembly_keeps_its_components_and_placements() {
    let (doc, model) = galvanometer();
    let step = crate::step_assembly(&doc, &doc.painted(&model.state()), "Galvanometer").unwrap();
    let (bytes, _) = crate::usd_from_step(&step, "galvanometer.usda").unwrap();
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
    let step = step_of(&doc, &model.state());
    let (bytes, _) = crate::usd_from_step(&step, "orange.usda").unwrap();
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
    assert!(crate::usd_from_step(b"ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n", "x.usda").is_err());
    let (doc, model) = one_box();
    assert!(crate::usd_from_step(&step_of(&doc, &model.state()), "x.step").is_err());
}

/// Mutations of a valid text layer, binary layer and package each give a result or an error,
/// never a panic, within the time budget (#469).
#[test]
fn hostile_usd_never_panics() {
    use solidcraft_testkit::{mutations, survives};
    let read = |file: &'static str| move |b: &[u8]| crate::read_usd(b, file).ok().map(|(g, _)| g.feature.body_names.len());
    survives("OpenUSD (.usda)", &mutations(HAND_WRITTEN.as_bytes(), true, 0x05da), usize::MAX, read("fuzz.usda"));
    let (doc, model) = galvanometer();
    let step = crate::step_assembly(&doc, &doc.painted(&model.state()), "Galvanometer").unwrap();
    for (file, seed) in [("fuzz.usdc", 0x05dc), ("fuzz.usdz", 0x05d2)] {
        let (bytes, _) = crate::usd_from_step(&step, file).unwrap();
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
    let step = step_of(&doc, &model.state());
    for file in ["box.usda", "box.usdc", "box.usd", "box.usdz"] {
        let (bytes, _) = crate::usd_from_step(&step, file).unwrap();
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
    let step = step_of(&doc, &model.state());
    let (bytes, _) = crate::usd_from_step(&step, "cylinder.usda").unwrap();
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
    let step = crate::step_assembly(&doc, &doc.painted(&model.state()), "Galvanometer").unwrap();
    let (bytes, _) = crate::usd_from_step(&step, "galvanometer.usda").unwrap();
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
    let (bytes, _) = crate::usd_from_step(&step_of(&doc, &model.state()), "box.usda").unwrap();
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
    let (bytes, _) = crate::usd_from_step(&step_of(&doc, &model.state()), "box.usda").unwrap();
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
    let (bytes, _) = crate::usd_from_step(&step_of(&doc, &model.state()), "orange.usda").unwrap();
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
    let step = crate::step_assembly(&doc, &doc.painted(&model.state()), "Galvanometer").unwrap();
    for (file, seed) in [("fuzz.usda", 0x05e1), ("fuzz.usdc", 0x05e2), ("fuzz.usdz", 0x05e3)] {
        let (bytes, _) = crate::usd_from_step(&step, file).unwrap();
        assert!(crate::usd_import_feature(&bytes, file, crate::StepNames::default()).is_ok(), "{file}");
        survives(&format!("OpenUSD exact ({file})"), &mutations(&bytes, file.ends_with(".usda"), seed), usize::MAX, read(file));
    }
    for (bytes, file) in [(&b"#usda 1.0\n"[..], "empty.usda"), (b"", "nothing.usdc"), (b"PK\x03\x04garbage", "broken.usdz")] {
        let e = crate::usd_import_feature(bytes, file, crate::StepNames::default()).err().unwrap_or_else(|| panic!("{file} was read"));
        assert!(e.to_string().contains(file), "{file}: {e}");
    }
}
