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
use solidcraft_geom::prelude::*;

use crate::SurfacePlan;

/// The name of the texture-coordinate set written.
const COORDINATES: &str = "st";

/// The texture coordinates of `positions` (in the frame `to_mesh` takes the body's definition
/// frame to) under `plan`'s projection; `None` when the projection or a point is not finite.
pub(super) fn coordinates(positions: &[solidcore_record::Vec3], plan: &SurfacePlan, to_mesh: &Mat) -> Option<TextureCoordinates> {
    let v3 = |a: [f64; 3]| solidcore_record::Vec3::new(a[0], a[1], a[2]);
    let origin = apply_point(to_mesh, v3(plan.origin));
    let (u, v) = (apply_vector(to_mesh, v3(plan.u)), apply_vector(to_mesh, v3(plan.v)));
    let (uu, vv) = (u.dot(u), v.dot(v));
    if !(uu > 0.0 && vv > 0.0 && uu.is_finite() && vv.is_finite()) {
        return None;
    }
    let mut values = Vec::new();
    values.try_reserve_exact(positions.len()).ok()?;
    for p in positions {
        let d = *p - origin;
        let s = d.dot(u) / uu;
        let along_v = d.dot(v) / vv;
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

/// The normal map `plan` writes: only one the file shows as the renderer does (full strength,
/// the image's v flipped as the built-in surfaces have it).
fn written_normal(plan: &SurfacePlan) -> Option<&crate::SurfaceMap> {
    plan.normal.as_ref().filter(|_| plan.flip_v && plan.normal_strength == 1.0)
}

/// Whether `a` and `b` texture a body alike: the same projection and the same images written (a
/// normal map left out of both does not tell them apart).
pub(super) fn same(a: &SurfacePlan, b: &SurfacePlan) -> bool {
    let sha = |m: Option<&crate::SurfaceMap>| m.map(|m| m.sha256.clone());
    (a.origin, a.u, a.v, a.flip_v) == (b.origin, b.u, b.v, b.flip_v)
        && [sha(a.base_color.as_ref()), sha(written_normal(a)), sha(a.roughness.as_ref()), sha(a.metallic.as_ref())]
            == [sha(b.base_color.as_ref()), sha(written_normal(b)), sha(b.roughness.as_ref()), sha(b.metallic.as_ref())]
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
