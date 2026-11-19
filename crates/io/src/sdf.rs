//! A Gazebo model folder of the design (docs/sdf.md): `model.config`, `model.sdf`, a binary STL
//! in metres for each body and, for each decal, its image and a COLLADA quad that lays the image
//! on the face. This is the layout of a model in a Gazebo model path: the folder is the model,
//! `model.config` names it and its SDF file, and `model.sdf` names every other file relative to
//! itself (or as `model://<name>/<file>`).
//!
//! The model is one link in the design's frame (Z up, metres): one visual per body in the body's
//! colour, a collision per closed body, and one visual per decal whose material takes the image
//! as its albedo map, as Gazebo's PBR materials do.

use solidcore_record::Vec3;
use solidcraft_doc::{Document, IDENTITY, ModelBody, ModelState};
use solidcraft_geom::Mesh;
use solidcraft_geom::prelude::*;

use crate::urdf::{BodyMass, Names, attr, decal_dae, image_kind, inertial, metres, num, token, triple};
use crate::{IoError, MeshQuality, Result};

/// The SDFormat version written: every element used is in it, and Gazebo Fortress and later read
/// it (newer readers convert it as they load it).
pub const SDF_VERSION: &str = "1.9";

/// Choices for [`sdf_model`] (`file.export` parameters).
#[derive(Clone, Debug)]
pub struct SdfOptions {
    /// The model's name, in `model.config` and `model.sdf`; also its folder's name.
    pub name: String,
    /// Scenery Gazebo never moves (`<static>`), the default; false gives the link the bodies'
    /// mass and inertia.
    pub is_static: bool,
    /// Name each file as `model://<folder>/<file>`, as a model found through
    /// `GZ_SIM_RESOURCE_PATH` is named; false names them relative to `model.sdf`.
    pub model_uris: bool,
    /// The model folder's name, which a `model://` URI names.
    pub folder: String,
    pub quality: MeshQuality,
    /// The bodies to write, by name; empty: every body.
    pub bodies: Vec<String>,
    /// The design's shown decals.
    pub decals: Vec<SdfDecal>,
    /// `model.config`'s description.
    pub description: String,
}

/// A decal for [`sdf_model`]: an image on a quad, where the design shows it.
#[derive(Clone, Debug)]
pub struct SdfDecal {
    pub name: String,
    /// The image's bottom-left, bottom-right, top-right and top-left corners in the design's
    /// frame (mm).
    pub corners: [Vec3; 4],
    /// The image file, PNG or JPEG, written as it is.
    pub image: Vec<u8>,
}

/// The model folder's files (name in the folder, bytes): meshes and images first, then
/// `model.config`, and `model.sdf` last.
#[derive(Clone, Debug, Default)]
pub struct SdfPackage {
    pub files: Vec<(String, Vec<u8>)>,
    /// How many bodies and decals the model draws.
    pub bodies: usize,
    pub decals: usize,
    pub warnings: Vec<String>,
}

/// The design `world` holds (bodies placed, coloured as shown) as a Gazebo model.
pub fn sdf_model(doc: &Document, world: &ModelState, options: &SdfOptions) -> Result<SdfPackage> {
    let name = options.name.trim();
    if name.is_empty() || name.len() > 255 || name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':')) {
        return Err(IoError::Invalid(format!("model name `{name}`: give a name with no `/`, `\\` or `:`")));
    }
    let sel: Vec<&ModelBody> = world.bodies.iter().filter(|b| options.bodies.is_empty() || options.bodies.contains(&b.name)).collect();
    if let Some(missing) = options.bodies.iter().find(|n| !world.bodies.iter().any(|b| &b.name == *n)) {
        return Err(IoError::Invalid(format!("no body `{missing}`")));
    }
    if sel.is_empty() && options.decals.is_empty() {
        return Err(IoError::Empty);
    }
    // A model of decals alone has no bodies to mesh, which `per_placed_body` refuses as empty.
    let (meshed, mut warnings) =
        if sel.is_empty() { (Vec::new(), Vec::new()) } else { crate::per_placed_body(&sel, |b, body| crate::body_mesh(b, body, options.quality))? };
    let uri = |file: &str| if options.model_uris { format!("model://{}/{file}", options.folder) } else { file.to_string() };
    let mut out = SdfPackage::default();
    let mut names = Names::default();
    let mut visuals = String::new();
    let mut masses: Vec<BodyMass> = Vec::new();
    for (b, mesh) in &meshed {
        let (positions, triangles) = crate::weld(&mesh.positions, &mesh.triangles, crate::weld_eps(&b.body));
        let welded = Mesh { positions, triangles, ..Mesh::default() };
        if welded.triangles.is_empty() {
            warnings.push(format!("body `{}` has nothing to draw: left out", b.name));
            continue;
        }
        let file = names.file_with(&b.name, "stl");
        out.files.push((file.clone(), crate::stl_binary(&[metres(&welded)], &b.name)));
        let [r, g, bl] = b.body.color().unwrap_or(crate::GLB_DEFAULT_COLOR.map(|c| f32::from(c) / 255.0));
        let opacity = b.body.paint().map(|p| p.opacity).filter(|o| o.is_finite()).map_or(1.0, |o| o.clamp(0.0, 1.0));
        let colour = format!("{} {} {} 1", num(f64::from(r)), num(f64::from(g)), num(f64::from(bl)));
        let geometry = format!("<geometry><mesh><uri>{}</uri></mesh></geometry>", attr(&uri(&file)));
        let transparency =
            if opacity < 1.0 { format!("\n        <transparency>{}</transparency>", num(f64::from(1.0 - opacity))) } else { String::new() };
        visuals += &format!(
            "      <visual name=\"{}\">\n        {geometry}\n        <material>\n          <ambient>{colour}</ambient>\n          <diffuse>{colour}</diffuse>\n          <specular>0 0 0 1</specular>\n        </material>{transparency}\n      </visual>\n",
            attr(&names.take(&format!("{}_visual", b.name))),
        );
        match welded.mass_properties() {
            Some(mp) => {
                visuals += &format!(
                    "      <collision name=\"{}\">\n        {geometry}\n      </collision>\n",
                    attr(&names.take(&format!("{}_collision", b.name)))
                );
                // g/cm³ to kg/mm³. Materials are assigned to the body's definition, which every
                // occurrence of its component shares, not to the occurrence's display name.
                let rho = doc.density(b.definition_name()) * 1e-6;
                masses.push((rho * mp.volume, mp.centroid, mp.inertia.map(|row| row.map(|x| x * rho))));
            }
            None => warnings.push(format!("body `{}` is not a closed solid: it is drawn, with no collision or mass", b.name)),
        }
        out.bodies += 1;
    }
    for d in &options.decals {
        let Some(ext) = image_kind(&d.image) else {
            warnings.push(format!("decal `{}`: its image is neither PNG nor JPEG: left out", d.name));
            continue;
        };
        let image = names.file_with(&d.name, ext);
        let Some(dae) = decal_dae(&d.name, &d.corners, &image) else {
            warnings.push(format!("decal `{}`: its corners lie on one line or are not finite: left out", d.name));
            continue;
        };
        let mesh = names.file_with(&d.name, "dae");
        out.files.push((image.clone(), d.image.clone()));
        out.files.push((mesh.clone(), dae.into_bytes()));
        // The mesh's own texture coordinates lay the image; the albedo map is the image again, so
        // Gazebo's PBR material shows it (the mesh's own material serves readers without one).
        visuals += &format!(
            "      <visual name=\"{}\">\n        <geometry><mesh><uri>{}</uri></mesh></geometry>\n        <material>\n          <ambient>1 1 1 1</ambient>\n          <diffuse>1 1 1 1</diffuse>\n          <specular>0 0 0 1</specular>\n          <emissive>0 0 0 1</emissive>\n          <pbr>\n            <metal>\n              <albedo_map>{}</albedo_map>\n              <metalness>0</metalness>\n              <roughness>1</roughness>\n            </metal>\n          </pbr>\n        </material>\n        <cast_shadows>false</cast_shadows>\n      </visual>\n",
            attr(&names.take(&format!("{}_visual", d.name))),
            attr(&uri(&mesh)),
            attr(&uri(&image)),
        );
        out.decals += 1;
    }
    let inertial_xml = if options.is_static {
        String::new()
    } else {
        let i = inertial(&masses, &IDENTITY).unwrap_or_else(|| {
            warnings.push("no closed body gives the model a mass: it has a token mass of 1 mg".into());
            token()
        });
        let m = i.inertia;
        format!(
            "      <inertial>\n        <pose>{} 0 0 0</pose>\n        <mass>{}</mass>\n        <inertia>\n          <ixx>{}</ixx>\n          <ixy>{}</ixy>\n          <ixz>{}</ixz>\n          <iyy>{}</iyy>\n          <iyz>{}</iyz>\n          <izz>{}</izz>\n        </inertia>\n      </inertial>\n",
            triple(i.com * 1e-3),
            num(i.mass),
            num(m[0][0]),
            num(m[0][1]),
            num(m[0][2]),
            num(m[1][1]),
            num(m[1][2]),
            num(m[2][2]),
        )
    };
    let (name_xml, version) = (attr(name), SDF_VERSION);
    let config = format!(
        "<?xml version=\"1.0\"?>\n<model>\n  <name>{name_xml}</name>\n  <version>1.0</version>\n  <sdf version=\"{version}\">model.sdf</sdf>\n  <description>{}</description>\n</model>\n",
        attr(&options.description)
    );
    let sdf = format!(
        "<?xml version=\"1.0\"?>\n<!-- Exported by SolidCraft; units: m, kg -->\n<sdf version=\"{version}\">\n  <model name=\"{name_xml}\">\n    <static>{}</static>\n    <link name=\"link\">\n{inertial_xml}{visuals}    </link>\n  </model>\n</sdf>\n",
        options.is_static
    );
    out.files.push(("model.config".into(), config.into_bytes()));
    out.files.push(("model.sdf".into(), sdf.into_bytes()));
    out.warnings = warnings;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::events::Event;

    /// A PNG header (RGBA): what [`image_kind`] reads.
    fn png() -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend(4u32.to_be_bytes());
        b.extend(2u32.to_be_bytes());
        b.extend([8, 6, 0, 0, 0]);
        b
    }

    /// Every element name and text of a well-formed XML document.
    fn read(xml: &str) -> (Vec<String>, Vec<String>) {
        let mut reader = quick_xml::Reader::from_str(xml);
        let (mut elements, mut texts) = (Vec::new(), Vec::new());
        loop {
            match reader.read_event() {
                Ok(Event::Start(e) | Event::Empty(e)) => elements.push(String::from_utf8_lossy(e.local_name().as_ref()).to_string()),
                Ok(Event::Text(t)) => texts.push(String::from_utf8_lossy(&t).trim().to_string()),
                Ok(Event::Eof) => break,
                Err(e) => panic!("{e}: {xml}"),
                _ => {}
            }
        }
        (elements, texts)
    }

    fn options(decals: Vec<SdfDecal>) -> SdfOptions {
        SdfOptions {
            name: "Panel".into(),
            is_static: true,
            model_uris: false,
            folder: "Panel".into(),
            quality: MeshQuality::Standard,
            bodies: Vec::new(),
            decals,
            description: "a <panel>".into(),
        }
    }

    /// A decal becomes its image and a COLLADA quad in the folder, and a visual of the model's one
    /// link whose PBR material takes the image as its albedo map, both named relative to
    /// `model.sdf` (or as `model://Panel/…`); `model.config` names the model and its SDF file.
    #[test]
    fn a_decal_is_a_mesh_visual_whose_albedo_map_is_its_image() {
        let doc = Document::new("t");
        let world = ModelState::default();
        let corners = [Vec3::new(0.0, 0.0, 10.0), Vec3::new(40.0, 0.0, 10.0), Vec3::new(40.0, 20.0, 10.0), Vec3::new(0.0, 20.0, 10.0)];
        let decal = SdfDecal { name: "Aruco 13".into(), corners, image: png() };
        let pkg = sdf_model(&doc, &world, &options(vec![decal.clone()])).unwrap();
        let files: Vec<&str> = pkg.files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(files, ["Aruco_13.png", "Aruco_13.dae", "model.config", "model.sdf"]);
        assert_eq!(pkg.files[0].1, png(), "the image is written as it is");
        assert_eq!((pkg.bodies, pkg.decals), (0, 1));

        let config = String::from_utf8(pkg.files[2].1.clone()).unwrap();
        let (elements, texts) = read(&config);
        assert_eq!(elements, ["model", "name", "version", "sdf", "description"]);
        // The reader hands an escaped character over as an event of its own, so the description
        // is checked as written.
        assert!(texts.iter().any(|t| t == "Panel") && texts.iter().any(|t| t == "model.sdf"), "{config}");
        assert!(config.contains("<description>a &lt;panel&gt;</description>"), "{config}");
        assert!(config.contains("<sdf version=\"1.9\">model.sdf</sdf>"), "{config}");

        let sdf = String::from_utf8(pkg.files[3].1.clone()).unwrap();
        let (elements, texts) = read(&sdf);
        for element in ["sdf", "model", "static", "link", "visual", "mesh", "uri", "material", "pbr", "metal", "albedo_map", "cast_shadows"] {
            assert!(elements.iter().any(|e| e == element), "no <{element}>: {sdf}");
        }
        assert!(!elements.iter().any(|e| e == "inertial" || e == "collision"), "static, and a decal has no collision: {sdf}");
        assert!(sdf.contains("<model name=\"Panel\">") && sdf.contains("<static>true</static>"), "{sdf}");
        assert!(texts.iter().any(|t| t == "Aruco_13.dae") && texts.iter().any(|t| t == "Aruco_13.png"), "{sdf}");

        // The quad: the image's corners in metres, 0.2 mm off the face on each side.
        let dae = String::from_utf8(pkg.files[1].1.clone()).unwrap();
        let (_, texts) = read(&dae);
        assert!(texts.iter().any(|t| t == "Aruco_13.png") && texts.iter().any(|t| t == "0 0 1 0 1 1 0 1"), "{dae}");
        let positions: Vec<f64> = texts
            .iter()
            .find(|t| t.split_whitespace().count() == 24)
            .map(|t| t.split_whitespace().filter_map(|x| x.parse().ok()).collect())
            .unwrap_or_default();
        let z: Vec<f64> = positions.chunks(3).map(|p| p[2]).collect();
        assert!(z.len() == 8 && z[..4].iter().all(|z| (z - 0.0102).abs() < 1e-12) && z[4..].iter().all(|z| (z - 0.0098).abs() < 1e-12), "{z:?}");
        assert!((positions[3] - 0.04).abs() < 1e-12 && positions[4].abs() < 1e-12, "bottom-right at (40, 0) mm");

        // Found through a model path: every file as model://Panel/….
        let pkg = sdf_model(&doc, &world, &SdfOptions { model_uris: true, ..options(vec![decal.clone()]) }).unwrap();
        let sdf = String::from_utf8(pkg.files[3].1.clone()).unwrap();
        assert!(
            sdf.contains("<uri>model://Panel/Aruco_13.dae</uri>") && sdf.contains("<albedo_map>model://Panel/Aruco_13.png</albedo_map>"),
            "{sdf}"
        );

        // Not static: a token mass, with a warning, when nothing gives it one.
        let pkg = sdf_model(&doc, &world, &SdfOptions { is_static: false, ..options(vec![decal.clone()]) }).unwrap();
        let sdf = String::from_utf8(pkg.files[3].1.clone()).unwrap();
        assert!(sdf.contains("<static>false</static>") && sdf.contains("<mass>1e-6</mass>"), "{sdf}");
        assert!(pkg.warnings.iter().any(|w| w.contains("token mass")), "{:?}", pkg.warnings);
    }

    /// A bad decal is left out with a warning; nothing at all to write, a body that is not there
    /// and a name a folder can't have are refused.
    #[test]
    fn what_cannot_be_written_is_left_out_or_refused() {
        let doc = Document::new("t");
        let world = ModelState::default();
        let corners = [Vec3::ZERO, Vec3::X, Vec3::X * 2.0, Vec3::ZERO];
        let bad = vec![
            SdfDecal { name: "Gif".into(), corners: [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y], image: b"GIF89a".to_vec() },
            SdfDecal { name: "Line".into(), corners, image: png() },
        ];
        let pkg = sdf_model(&doc, &world, &options(bad)).unwrap();
        let files: Vec<&str> = pkg.files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(files, ["model.config", "model.sdf"]);
        assert_eq!(pkg.warnings.len(), 2, "{:?}", pkg.warnings);
        assert!(matches!(sdf_model(&doc, &world, &options(Vec::new())), Err(IoError::Empty)));
        assert!(sdf_model(&doc, &world, &SdfOptions { bodies: vec!["Nope".into()], ..options(Vec::new()) }).is_err());
        for name in ["", "  ", "a/b", "a\\b", "c:", "x\ny"] {
            assert!(sdf_model(&doc, &world, &SdfOptions { name: name.into(), ..options(Vec::new()) }).is_err(), "{name:?}");
        }
    }
}
