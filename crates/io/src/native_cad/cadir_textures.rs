//! The image textures a render setup gives a body, as CADIR texture coordinates, assets and
//! texture references, for an OpenUSD package ([`crate::CadmpegOptions::surfaces`]).
//!
//! Coordinates: the setup projects its images onto the body along a plane, so a point's place in
//! the images is a linear function of the point, and the coordinates of the mesh's vertices
//! reproduce the projection exactly. The renderer reads an image from its top-left corner with v
//! downwards; a `UsdUVTexture` reads it from its bottom-left with t upwards, so t is the
//! renderer's v negated (the images repeat, so no offset is needed).
//!
//! Maps: base colour drives the diffuse colour, roughness and metallic their inputs (from the
//! red channel, as the renderer reads them), and the normal map the normal. A normal map is
//! written only where the file shows it as the renderer does: at full strength, with the image's
//! v flipped as the built-in surfaces have it (then the map's green points along the
//! projection's v in both). Any other normal map is left out, with a warning.

use std::collections::HashMap;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::appearance::{TextureMap2d, TextureRef};
use cadmpeg_ir::assets::{Asset, AssetContent, AssetData, AssetId};
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length};
use cadmpeg_ir::tessellation::{TextureCoordinateAddressing, TextureCoordinates};
use cadmpeg_ir::units::FinitePoint2;
use solidcraft_doc::{Mat, apply_point, apply_vector};

use crate::SurfacePlan;

/// The name of the texture-coordinate set written.
const COORDINATES: &str = "st";

/// The texture coordinates of `positions` (in the frame `to_mesh` takes the body's definition
/// frame to) under `plan`'s projection; `None` when the projection or a point is not finite.
pub(super) fn coordinates(positions: &[solidcore_record::Vec3], plan: &SurfacePlan, to_mesh: &Mat) -> Option<TextureCoordinates> {
    let v3 = |a: [f64; 3]| solidcore_record::Vec3::new(a[0], a[1], a[2]);
    let origin = apply_point(to_mesh, v3(plan.origin));
    let (u, v) = (apply_vector(to_mesh, v3(plan.u)), apply_vector(to_mesh, v3(plan.v)));
    // The projection's own layout maths, written out: no model decision is made here.
    let dot = |a: solidcore_record::Vec3, b: solidcore_record::Vec3| a.x * b.x + a.y * b.y + a.z * b.z;
    let (uu, vv) = (dot(u, u), dot(v, v));
    if !(uu > 0.0 && vv > 0.0 && uu.is_finite() && vv.is_finite()) {
        return None;
    }
    let mut values = Vec::new();
    values.try_reserve_exact(positions.len()).ok()?;
    for p in positions {
        let d = *p - origin;
        let s = dot(d, u) / uu;
        let along_v = dot(d, v) / vv;
        let t = if plan.flip_v { along_v } else { -along_v };
        values.push(FinitePoint2::new(cadmpeg_ir::math::Point2::new(s, t))?);
    }
    Some(TextureCoordinates { name: NonBlankString::new(COORDINATES)?, addressing: TextureCoordinateAddressing::Vertex, values })
}

/// The assets written, by the SHA-256 of their bytes.
#[derive(Default)]
pub(super) struct Assets {
    by_sha: HashMap<String, AssetId>,
}

impl Assets {
    /// The asset holding `map`, added to `assets` the first time.
    fn asset(&mut self, assets: &mut Vec<Asset>, map: &crate::SurfaceMap) -> Option<AssetId> {
        if let Some(id) = self.by_sha.get(&map.sha256) {
            return Some(id.clone());
        }
        let (extension, media) = if map.bytes.starts_with(b"\x89PNG") {
            ("png", "image/png")
        } else if map.bytes.starts_with(&[0xFF, 0xD8]) {
            ("jpg", "image/jpeg")
        } else {
            return None;
        };
        let id = AssetId::try_from(format!("solidcraft:design:texture#{}", map.sha256)).ok()?;
        let content = AssetContent::Embedded { data: AssetData::new(map.bytes.to_vec())? };
        let asset = Asset::try_new(id.clone(), Some(format!("{}.{extension}", map.sha256)), Some(media.into()), content, None).ok()?;
        assets.push(asset);
        self.by_sha.insert(map.sha256.clone(), id.clone());
        Some(id)
    }
}

/// A texture reading the `COORDINATES` set as they are: no offset, scale or turn, repeating.
fn mapping() -> TextureMap2d {
    TextureMap2d {
        map_channel: 0,
        uvw_source: 0,
        u_offset: FiniteReal::ZERO,
        v_offset: FiniteReal::ZERO,
        u_scale: FiniteReal::ONE,
        v_scale: FiniteReal::ONE,
        rotation: Angle::ZERO,
        repeat_u: true,
        repeat_v: true,
        real_world_offset_x: Length::ZERO,
        real_world_offset_y: Length::ZERO,
        real_world_scale_x: Length::ZERO,
        real_world_scale_y: Length::ZERO,
        coordinates: NonBlankString::new(COORDINATES),
    }
}

/// The texture references of `plan`'s maps, their assets added to `assets`; what is left out is
/// a warning naming `shown`.
pub(super) fn references(
    plan: &SurfacePlan,
    registry: &mut Assets,
    assets: &mut Vec<Asset>,
    shown: &str,
    warnings: &mut Vec<String>,
) -> Vec<TextureRef> {
    if plan.normal.is_some() && written_normal(plan).is_none() {
        warnings.push(format!(
            "{shown}: the normal map of render setup `{}` is left out: OpenUSD shows only a full-strength map whose image's v is flipped as the renderer reads it",
            plan.setup
        ));
    }
    let maps = [
        ("diffuseColor", plan.base_color.as_ref()),
        ("normal", written_normal(plan)),
        ("roughness.r", plan.roughness.as_ref()),
        ("metallic.r", plan.metallic.as_ref()),
    ];
    let mut out = Vec::new();
    for (slot, map) in maps {
        let Some(map) = map else { continue };
        let Some(asset) = registry.asset(assets, map) else {
            warnings.push(format!("{shown}: an image of render setup `{}` is neither PNG nor JPEG: left out", plan.setup));
            continue;
        };
        out.push(TextureRef {
            asset_guid: asset.to_string(),
            slot: slot.to_string(),
            schema: "UsdPreviewSurface".into(),
            paths: Vec::new(),
            urn: None,
            mapping: mapping(),
            bump: None,
        });
    }
    out
}

/// How far a decal's quad is lifted off the face it lies on, each side, mm: as URDF export's.
const DECAL_LIFT_MM: f64 = 0.2;

/// A decal's quad lifted clear of its face on both sides: its points (the front four, then the
/// back four), their normals, its triangles (the back wound the other way, so one side faces out
/// whichever way the face does) and its texture coordinates (the image's bottom-left at (0, 0),
/// its top-right at (1, 1)). `None` when the corners give no plane.
pub(super) fn decal_quad(corners: [[f64; 3]; 4]) -> Option<(Vec<[f64; 3]>, Vec<[f64; 3]>, Vec<[u32; 3]>, TextureCoordinates)> {
    let (normal, front, back) = solidcraft_kernel::affine::offset_quad(corners, DECAL_LIFT_MM)?;
    let points: Vec<[f64; 3]> = front.into_iter().chain(back).collect();
    let flipped = normal.map(|x| -x);
    let normals: Vec<[f64; 3]> = std::iter::repeat_n(normal, 4).chain(std::iter::repeat_n(flipped, 4)).collect();
    let triangles = vec![[0, 1, 2], [0, 2, 3], [4, 6, 5], [4, 7, 6]];
    let corner_uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let values = corner_uv
        .iter()
        .chain(corner_uv.iter())
        .map(|[u, v]| FinitePoint2::new(cadmpeg_ir::math::Point2::new(*u, *v)))
        .collect::<Option<Vec<_>>>()?;
    let coordinates = TextureCoordinates { name: NonBlankString::new(COORDINATES)?, addressing: TextureCoordinateAddressing::Vertex, values };
    Some((points, normals, triangles, coordinates))
}

/// The texture references of a decal's image: its colour, and its alpha as the opacity when the
/// image has one (a PNG of an alpha colour type, or with a transparency chunk).
pub(super) fn decal_references(image: &crate::SurfaceMap, registry: &mut Assets, assets: &mut Vec<Asset>) -> Option<Vec<TextureRef>> {
    let asset = registry.asset(assets, image)?;
    let reference = |slot: &str| TextureRef {
        asset_guid: asset.to_string(),
        slot: slot.to_string(),
        schema: "UsdPreviewSurface".into(),
        paths: Vec::new(),
        urn: None,
        mapping: mapping(),
        bump: None,
    };
    let mut out = vec![reference("diffuseColor")];
    if png_has_alpha(&image.bytes) {
        out.push(reference("opacity.a"));
    }
    Some(out)
}

/// Whether `bytes` are a PNG with an alpha channel: a colour type with alpha (grey or RGB with
/// alpha, IHDR's) or a transparency chunk (`tRNS`, which comes before the image data). Only the
/// chunks before the first `IDAT` are walked, by their lengths, so the work is bounded by the
/// header chunks and not the image; anything else (a JPEG, a damaged PNG) has none.
fn png_has_alpha(bytes: &[u8]) -> bool {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    let Some(mut rest) = bytes.strip_prefix(SIGNATURE) else { return false };
    // Each chunk: its data's length (4 bytes, big-endian), its type (4), the data, a CRC (4).
    // IHDR's data: width and height (8), bit depth (1), colour type (1).
    while let Some((head, after)) = rest.split_first_chunk::<8>() {
        let length = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        let Some(data) = after.get(..length) else { return false };
        match &head[4..] {
            b"IHDR" if matches!(data.get(9), Some(4 | 6)) => return true,
            b"tRNS" => return true,
            b"IDAT" | b"IEND" => return false,
            _ => {}
        }
        let Some(next) = length.checked_add(4).and_then(|n| after.get(n..)) else { return false };
        rest = next;
    }
    false
}

/// The `MaterialX` OpenPBR shader cadmpeg writes beside the preview surface: an appearance
/// property `<shader>/<input>` is that shader's input.
const OPEN_PBR: &str = "ND_open_pbr_surface_surfaceshader";
/// The depth at which a transmission colour is reached, in the layer's units (metres): 1 mm.
const TRANSMISSION_DEPTH_M: f64 = 1e-3;

/// The appearance properties of `optics`: its index of refraction (the preview surface's `ior`,
/// which cadmpeg also gives OpenPBR as `specular_ior`), and OpenPBR's transmission: full, with the
/// colour the absorption leaves after 1 mm (Beer–Lambert, e^(−absorption · 1 mm)) at a depth of
/// 1 mm, thin-walled for a thin sheet. OpenPBR's metalness is 0, as a metal does not transmit:
/// cadmpeg would otherwise give it the look's `metallic`, and base metalness takes the
/// transmission away (the preview surface, which has no transmission, keeps the look's).
pub(super) fn optics_properties(optics: &crate::OpticsPlan) -> Vec<(String, f64)> {
    let mut out =
        vec![("ior".to_string(), optics.ior), (format!("{OPEN_PBR}/transmission_weight"), 1.0), (format!("{OPEN_PBR}/base_metalness"), 0.0)];
    for (channel, a) in ["r", "g", "b"].iter().zip(optics.absorption_per_mm) {
        out.push((format!("{OPEN_PBR}/transmission_color.{channel}"), (-a).exp()));
    }
    out.push((format!("{OPEN_PBR}/transmission_depth"), TRANSMISSION_DEPTH_M));
    if optics.thin_sheet {
        out.push((format!("{OPEN_PBR}/geometry_thin_walled"), 1.0));
    }
    out
}

/// The normal map `plan` writes: only one the file shows as the renderer does (full strength,
/// the image's v flipped as the built-in surfaces have it).
fn written_normal(plan: &SurfacePlan) -> Option<&crate::SurfaceMap> {
    plan.normal.as_ref().filter(|_| plan.flip_v && plan.normal_strength == 1.0)
}

/// Whether `a` and `b` texture a body alike: the same projection and the same images written (a
/// normal map left out of both does not tell them apart). A glass body's metallic map is not
/// written either (`glass`), so it does not tell two placements apart.
pub(super) fn same(a: &SurfacePlan, b: &SurfacePlan, glass: bool) -> bool {
    let sha = |m: Option<&crate::SurfaceMap>| m.map(|m| m.sha256.clone());
    let metallic = |p: &SurfacePlan| if glass { None } else { sha(p.metallic.as_ref()) };
    (a.origin, a.u, a.v, a.flip_v) == (b.origin, b.u, b.v, b.flip_v)
        && [sha(a.base_color.as_ref()), sha(written_normal(a)), sha(a.roughness.as_ref()), metallic(a)]
            == [sha(b.base_color.as_ref()), sha(written_normal(b)), sha(b.roughness.as_ref()), metallic(b)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::Vec3;

    fn plan(flip_v: bool) -> SurfacePlan {
        SurfacePlan {
            setup: "Studio".into(),
            definition: "Body1".into(),
            component: 0,
            path: Vec::new(),
            world: "Body1".into(),
            origin: [10.0, 0.0, 0.0],
            u: [20.0, 0.0, 0.0],
            v: [0.0, 0.0, 5.0],
            flip_v,
            normal_strength: 1.0,
            base_color: None,
            normal: None,
            roughness: None,
            metallic: None,
        }
    }

    /// A point's coordinates are its offsets from the origin along u and v in repeats of their
    /// lengths: (30, 7, 2.5) is one repeat of u (20 mm) and half a repeat of v (5 mm) from
    /// (10, 0, 0); t is v negated unless the image's v is flipped. A point off the plane (y = 7)
    /// projects along its normal.
    #[test]
    fn coordinates_follow_the_projection() {
        let points = [Vec3::new(10.0, 0.0, 0.0), Vec3::new(30.0, 7.0, 2.5)];
        let st = |flip: bool| -> Vec<[f64; 2]> {
            coordinates(&points, &plan(flip), &solidcraft_doc::IDENTITY).unwrap().values.iter().map(|p| [p.get().u, p.get().v]).collect()
        };
        assert_eq!(st(true), [[0.0, 0.0], [1.0, 0.5]]);
        assert_eq!(st(false), [[0.0, 0.0], [1.0, -0.5]]);
    }

    /// Moving the body moves the projection with it: the definition frame taken 100 mm along x.
    #[test]
    fn coordinates_follow_the_body_frame() {
        let mut moved = solidcraft_doc::IDENTITY;
        moved[3][0] = 100.0;
        let points = [Vec3::new(130.0, 0.0, 2.5)];
        let got = coordinates(&points, &plan(true), &moved).unwrap();
        let p = got.values[0].get();
        assert_eq!([p.u, p.v], [1.0, 0.5]);
    }

    /// Optics as properties: the index, full transmission, the colour 1 mm of absorption leaves
    /// (e^(−a)), and thin walls for a thin sheet only.
    #[test]
    fn optics_become_ior_and_openpbr_transmission() {
        let mut optics = crate::OpticsPlan {
            setup: "Glass".into(),
            definition: "Body1".into(),
            component: 0,
            path: Vec::new(),
            world: "Body1".into(),
            ior: 1.52,
            absorption_per_mm: [0.0, 0.1, 2.0],
            thin_sheet: false,
        };
        let get = |p: &[(String, f64)], k: &str| p.iter().find(|(n, _)| n.ends_with(k)).map(|(_, v)| *v);
        let p = optics_properties(&optics);
        assert_eq!(get(&p, "ior"), Some(1.52));
        assert_eq!(get(&p, "transmission_weight"), Some(1.0));
        assert_eq!(get(&p, "base_metalness"), Some(0.0));
        assert_eq!(get(&p, "transmission_color.r"), Some(1.0));
        assert!((get(&p, "transmission_color.g").unwrap() - (-0.1f64).exp()).abs() < 1e-15);
        assert!((get(&p, "transmission_color.b").unwrap() - (-2.0f64).exp()).abs() < 1e-15);
        assert_eq!(get(&p, "transmission_depth"), Some(1e-3));
        assert_eq!(get(&p, "geometry_thin_walled"), None);
        optics.thin_sheet = true;
        assert_eq!(get(&optics_properties(&optics), "geometry_thin_walled"), Some(1.0));
    }

    /// A decal's quad: eight points, two lifted 0.2 mm each side of the corners' plane, with the
    /// image's corners at its corners.
    #[test]
    fn a_decal_is_a_two_sided_quad_with_its_image_corners() {
        let corners = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 5.0, 0.0], [0.0, 5.0, 0.0]];
        let (points, normals, triangles, st) = decal_quad(corners).unwrap();
        assert_eq!(points.len(), 8);
        assert!(points[..4].iter().all(|p| (p[2] - 0.2).abs() < 1e-12) && points[4..].iter().all(|p| (p[2] + 0.2).abs() < 1e-12), "{points:?}");
        assert_eq!(normals[0], [0.0, 0.0, 1.0]);
        assert_eq!(triangles.len(), 4);
        let uv: Vec<[f64; 2]> = st.values.iter().map(|p| [p.get().u, p.get().v]).collect();
        assert_eq!(uv[2], [1.0, 1.0]);
        assert_eq!(uv[6], [1.0, 1.0]);
        assert!(decal_quad([[0.0; 3]; 4]).is_none());
    }

    /// A PNG of `colour` type, with a transparency chunk for RGB when `trns`.
    fn png(colour: png::ColorType, trns: bool) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 1, 1);
            encoder.set_color(colour);
            encoder.set_depth(png::BitDepth::Eight);
            if trns {
                // One transparent colour as PNG writes it for RGB: two bytes for each channel,
                // whatever the bit depth.
                encoder.set_trns(vec![0u8; 6]);
            }
            let mut writer = encoder.write_header().unwrap();
            let size = match colour {
                png::ColorType::Rgba => 4,
                png::ColorType::Rgb => 3,
                png::ColorType::GrayscaleAlpha => 2,
                _ => 1,
            };
            writer.write_image_data(&vec![255u8; size]).unwrap();
        }
        out
    }

    /// Alpha is read from the PNG's header chunks: an RGBA or grey-and-alpha PNG has it, and an
    /// RGB one has it only with a transparency chunk. A JPEG whose 26th byte (a quantization value)
    /// reads as an alpha colour type, and a PNG cut off in its header, have none.
    #[test]
    fn png_alpha_is_read_from_its_header_chunks() {
        assert!(png_has_alpha(&png(png::ColorType::Rgba, false)));
        assert!(png_has_alpha(&png(png::ColorType::GrayscaleAlpha, false)));
        assert!(!png_has_alpha(&png(png::ColorType::Rgb, false)));
        assert!(png_has_alpha(&png(png::ColorType::Rgb, true)));
        let mut jpeg = vec![0u8; 64];
        jpeg[..4].copy_from_slice(&[0xFF, 0xD8, 0xFF, 0xDB]);
        jpeg[25] = 6;
        assert!(!png_has_alpha(&jpeg));
        let rgba = png(png::ColorType::Rgba, false);
        assert!(!png_has_alpha(&rgba[..20]));
    }

    /// A normal map at less than full strength, or with v as stored, is left out with a warning.
    #[test]
    fn only_a_faithful_normal_map_is_written() {
        let map = crate::SurfaceMap { sha256: "ab".into(), bytes: std::sync::Arc::from(&b"\x89PNG\r\n\x1a\n"[..]) };
        for (flip, strength, written) in [(true, 1.0, true), (false, 1.0, false), (true, 0.5, false)] {
            let mut p = plan(flip);
            p.normal_strength = strength;
            p.normal = Some(map.clone());
            let (mut registry, mut assets, mut warnings) = (Assets::default(), Vec::new(), Vec::new());
            let refs = references(&p, &mut registry, &mut assets, "Body1", &mut warnings);
            assert_eq!(refs.iter().any(|r| r.slot == "normal"), written, "{flip} {strength}");
            assert_eq!(warnings.is_empty(), written, "{warnings:?}");
        }
    }
}
