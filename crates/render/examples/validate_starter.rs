//! Native conformance gate for the authored starter pack, not an application importer.
//! Reads one bounded resource at a time and never changes a document or source asset.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo)]

use image::ImageDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use solidcraft_geom::Vec3;
use solidcraft_render::{
    lighting::{DecodedHdr, HdrDecodeTask, HdrSource, HdrTaskPoll},
    surface::unit_direction,
    texture_input::{
        MapRole, TextureSource,
        normalization::{ColorConversion, NormalizationOptions, NormalizationSource, UnprofiledColor},
    },
    texture_sampling::{
        TextureView,
        mips::{MAX_MIP_TAIL_BYTES, MipChain},
    },
};
use std::{
    error::Error,
    f64::consts::PI,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    process::ExitCode,
};

const PACK: &str = "assets/render/starter";
const LIMIT: usize = 16 * 1024 * 1024;
const NORMAL_CONVENTION: &str = "OpenGL +Y, +U right, +V image-up; renormalize after sampling";
type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn require(ok: bool, message: impl Into<String>) -> Result<()> {
    if ok { Ok(()) } else { Err(io::Error::new(io::ErrorKind::InvalidData, message.into()).into()) }
}
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Only repository-relative paths with portable, unambiguous components are admitted.
fn relative_path(path: &str) -> Result<PathBuf> {
    require(!path.is_empty() && path.len() <= 240, "invalid resource path length")?;
    require(path.bytes().all(|c| c.is_ascii_alphanumeric() || b"/_.-".contains(&c)), "nonportable resource path")?;
    require(path.split('/').all(|c| !c.is_empty() && c != "." && c != ".."), "resource path traversal")?;
    Ok(PathBuf::from(path))
}
fn read_bounded(root: &Path, path: &str, limit: usize) -> Result<Vec<u8>> {
    let relative = relative_path(path)?;
    let root = root.canonicalize()?;
    let mut target = root.clone();
    for component in relative.components() {
        target.push(component.as_os_str());
        require(!target.symlink_metadata()?.file_type().is_symlink(), "resource symlink refused")?;
    }
    require(target.canonicalize()?.starts_with(&root), "resource resolves outside checkout")?;
    let file = File::open(target)?;
    let meta = file.metadata()?;
    require(meta.is_file() && meta.len() > 0 && meta.len() <= limit as u64, "resource byte budget exceeded")?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(meta.len() as usize)?;
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    require(bytes.len() == meta.len() as usize && bytes.len() <= limit, "resource changed size while reading")?;
    Ok(bytes)
}
fn verify_bytes(bytes: &[u8], length: usize, digest: &str) -> Result<()> {
    require(digest.len() == 64 && digest.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)), "invalid SHA-256 receipt")?;
    require(bytes.len() == length && sha(bytes) == digest, "resource byte/hash receipt mismatch")
}
fn pinned_source(root: &Path, path: &str, digest: &str) -> Result<()> {
    let bytes = read_bounded(root, path, 4 * 1024 * 1024)?;
    verify_bytes(&bytes, bytes.len(), digest)
}

#[derive(Deserialize)]
struct Surfaces {
    schema: String,
    generator_version: String,
    license: String,
    source_path: String,
    source_sha256: String,
    generator_sha256: String,
    items: Vec<Surface>,
}
#[derive(Deserialize)]
struct Surface {
    id: String,
    runtime_status: String,
    normal_convention: String,
    tile_mm: [f64; 2],
    maps: Maps,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Maps {
    base_color: Map,
    normal_open_gl: Map,
    roughness: Map,
    metallic: Map,
    height: Map,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Map {
    path: String,
    sha256: String,
    decoded_sha256: String,
    size_bytes: usize,
    width: u32,
    height: u32,
    channels: usize,
    bit_depth: u8,
    encoding: String,
    minimum: u16,
    maximum: u16,
    consumer: String,
}
#[derive(Deserialize)]
struct Environments {
    schema: String,
    version: String,
    license: String,
    runtime_ready: bool,
    recipe_source: String,
    recipe_sha256: String,
    generator: String,
    generator_sha256: String,
    items: Vec<Environment>,
}
#[derive(Deserialize)]
struct Environment {
    id: String,
    path: String,
    sha256: String,
    bytes: usize,
    dimensions: [u32; 2],
    runtime_ready: bool,
    role: String,
    color_space: String,
    transfer_function: String,
    header_orientation: String,
    primaries_xy: [f64; 8],
    exposure: f64,
    color_correction: [f64; 3],
    solar_disk: bool,
    energy_owner: String,
    statistics: Statistics,
}
#[derive(Deserialize)]
struct Statistics {
    minimum_channel: f64,
    maximum_channel: f64,
    solid_angle_weighted_mean_rgb: [f64; 3],
    hemisphere_cosine_integrals_rgb: std::collections::BTreeMap<String, [f64; 3]>,
}
#[derive(Serialize)]
struct TextureReceipt {
    surface: String,
    role: String,
    source_sha256: String,
    normalized_sha256: String,
    mip_levels: usize,
    mip_tail_bytes: usize,
}
#[derive(Serialize)]
struct HdrReceipt {
    environment: String,
    source_sha256: String,
    decoded_f32_le_sha256: String,
    pixels: usize,
}
#[derive(Default, Serialize)]
struct Report {
    schema: &'static str,
    runtime_ready: bool,
    surfaces: usize,
    texture_inputs: usize,
    height_sources_verified_not_bound: usize,
    hdr_inputs: usize,
    mip_levels: usize,
    encoded_resource_bytes: usize,
    textures: Vec<TextureReceipt>,
    environments: Vec<HdrReceipt>,
}

fn map_bytes(root: &Path, surface: &Surface, name: &str, map: &Map, bits: u8, channels: usize) -> Result<Vec<u8>> {
    require(map.path == format!("{PACK}/materials/exports/{}/{name}.png", surface.id), "map path/role mismatch")?;
    require((map.width, map.height, map.bit_depth, map.channels) == (512, 512, bits, channels), "map shape/precision mismatch")?;
    require(map.minimum <= map.maximum && (bits == 16 || map.maximum <= 255), "invalid map range")?;
    let encoding = if name == "base_color" { "srgb" } else { "linear_data" };
    require(map.encoding == encoding, "map color/data role mismatch")?;
    let consumer =
        if name == "height" { "authoring_only_requires_explicit_height_adapter" } else { "bounded_texture_input_compatible_not_runtime_bound" };
    require(map.consumer == consumer, "unsupported map availability")?;
    let bytes = read_bounded(root, &map.path, LIMIT)?;
    verify_bytes(&bytes, map.size_bytes, &map.sha256)?;
    Ok(bytes)
}
fn srgb_reference(encoded: u8) -> f64 {
    let value = f64::from(encoded) / 255.0;
    if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
}
/// Native sampling uses image-down V; authored texture/tangent coordinates use V-up.
fn sample_authored(view: TextureView<'_>, uv: [f64; 2]) -> Result<[f64; 3]> {
    Ok(view.sample_repeat_opaque([uv[0], 1.0 - uv[1]])?)
}
fn normal(rgb: [f64; 3]) -> Result<Vec3> {
    Ok(unit_direction(Vec3::new(2.0 * rgb[0] - 1.0, 2.0 * rgb[1] - 1.0, 2.0 * rgb[2] - 1.0))?)
}
fn check_texture(root: &Path, surface: &Surface, name: &str, role: MapRole, map: &Map) -> Result<TextureReceipt> {
    let channels = if matches!(role, MapRole::BaseColor | MapRole::NormalOpenGl) { 3 } else { 1 };
    let bytes = map_bytes(root, surface, name, map, 8, channels)?;
    let strict = TextureSource::new(bytes.clone(), role)?.decode(|| false)?;
    require(
        strict.info().role == role && (strict.info().width, strict.info().height) == (map.width, map.height),
        "native texture metadata mismatch",
    )?;
    let mut hash = Sha256::new();
    let (mut minimum, mut maximum) = (u16::MAX, 0u16);
    for pixel in strict.rgba().as_chunks::<4>().0.iter() {
        let [r, g, b, a] = pixel;
        require(*a == 255, "opaque source unexpectedly has alpha")?;
        if channels == 1 {
            require(r == g && g == b, "scalar source gained color")?;
            hash.update([*r]);
        } else {
            hash.update([*r, *g, *b]);
        }
        for value in [*r, *g, *b] {
            minimum = minimum.min(u16::from(value));
            maximum = maximum.max(u16::from(value));
        }
    }
    require(format!("{:x}", hash.finalize()) == map.decoded_sha256, "native decoded pixels differ from authoring receipt")?;
    require((minimum, maximum) == (map.minimum, map.maximum), "native channel range mismatch")?;
    let options = NormalizationOptions { unprofiled_color: UnprofiledColor::DeclareSrgbPrimaries, ..Default::default() };
    let normalized = NormalizationSource::new(bytes, role, options)?.decode(|| false)?;
    let receipt = normalized.receipt();
    require(receipt.source_sha256 == map.sha256 && receipt.output_dimensions == [map.width, map.height], "normalization identity mismatch")?;
    require(receipt.clamped_rgb_channels == 0 && !receipt.orientation_applied && receipt.source_channel_bits == 8, "unexpected lossy normalization")?;
    let expected_conversion = if role == MapRole::BaseColor { ColorConversion::DeclaredSrgb } else { ColorConversion::RawData };
    require(receipt.conversion == expected_conversion, "unexpected color conversion")?;
    require(normalized.rgba().len() == strict.rgba().len(), "normalization changed sample count")?;
    let mut mean = [0.0; 3];
    for (raw, actual) in strict.rgba().as_chunks::<4>().0.iter().zip(normalized.rgba().as_chunks::<4>().0.iter()) {
        for (channel, (&raw, &actual)) in raw.iter().zip(actual).enumerate() {
            let expected = if channel < 3 && role == MapRole::BaseColor { srgb_reference(raw) } else { f64::from(raw) / 255.0 };
            require((f64::from(actual) - expected).abs() <= 6e-8, "native linear/data sample mismatch")?;
            if let Some(total) = mean.get_mut(channel) {
                *total += f64::from(actual);
            }
        }
    }
    let count = f64::from(map.width) * f64::from(map.height);
    for value in &mut mean {
        *value /= count;
    }
    let view = TextureView::new(normalized.rgba(), [map.width, map.height])?;
    let chain = MipChain::prepare(view, MAX_MIP_TAIL_BYTES, || false)?;
    require(chain.level_count() == 10 && chain.dimensions(9) == Some([1, 1]), "incomplete native mip chain")?;
    let final_mean = chain.sample_repeat_lod_opaque([0.5, 0.5], 9.0)?;
    for (actual, expected) in final_mean.into_iter().zip(mean) {
        require((actual - expected).abs() < 2e-7, "native final mip changed channel mean")?;
    }
    for uv in [[0.125, 0.375], [-0.125, 1.25], [0.0, 1.0], [0.5, 0.5]] {
        let native = sample_authored(view, uv)?;
        for lod in [0.0, 2.5, 9.0] {
            let a = chain.sample_repeat_lod_opaque([uv[0], 1.0 - uv[1]], lod)?;
            let b = chain.sample_repeat_lod_opaque([uv[0] + 1.0, -uv[1]], lod)?;
            for (a, b) in a.into_iter().zip(b) {
                require((a - b).abs() < 1e-12, "native texture repeat mismatch")?;
            }
            if lod == 0.0 {
                require(a == native, "level-zero authored V mapping mismatch")?;
            }
            if role == MapRole::NormalOpenGl {
                let n = normal(a)?;
                require(n.z > 0.0 && (n.len() - 1.0).abs() < 1e-12, "invalid filtered tangent normal")?;
            }
        }
    }
    Ok(TextureReceipt {
        surface: surface.id.clone(),
        role: name.into(),
        source_sha256: map.sha256.clone(),
        normalized_sha256: receipt.normalized_sha256.clone(),
        mip_levels: chain.level_count(),
        mip_tail_bytes: chain.tail_bytes(),
    })
}
fn check_height(root: &Path, surface: &Surface) -> Result<()> {
    let map = &surface.maps.height;
    let bytes = map_bytes(root, surface, "height", map, 16, 1)?;
    // Safety probe only: no height role exists, so never relabel a height as roughness.
    match TextureSource::new(bytes.clone(), MapRole::Roughness) {
        Err(error) if error.0.contains("8-bit") => {}
        _ => return Err("16-bit source unexpectedly passed the strict eight-bit route".into()),
    }
    let mut decoder = image::codecs::png::PngDecoder::new(io::Cursor::new(bytes))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(512);
    limits.max_image_height = Some(512);
    limits.max_alloc = Some(LIMIT as u64);
    decoder.set_limits(limits)?;
    require(decoder.dimensions() == (512, 512) && decoder.color_type() == image::ColorType::L16, "height is not grayscale 16-bit")?;
    let image = image::DynamicImage::from_decoder(decoder)?;
    let pixels = image.as_luma16().ok_or("height precision changed")?;
    let mut hash = Sha256::new();
    let (mut low, mut high) = (u16::MAX, 0u16);
    for sample in pixels.as_raw() {
        hash.update(sample.to_le_bytes());
        low = low.min(*sample);
        high = high.max(*sample);
    }
    require(
        format!("{:x}", hash.finalize()) == map.decoded_sha256 && (low, high) == (map.minimum, map.maximum),
        "height native 16-bit receipt mismatch",
    )
}
fn decode_hdr(source: HdrSource) -> Result<DecodedHdr> {
    let mut task = HdrDecodeTask::new(source)?;
    let mut previous = 0;
    for _ in 0..1024 {
        match task.step(16384, || false)? {
            HdrTaskPoll::Complete(image) => return Ok(image),
            HdrTaskPoll::Pending(progress) => {
                require(progress.decoded_pixels >= previous && progress.decoded_pixels <= progress.total_pixels, "nonmonotonic HDR progress")?;
                previous = progress.decoded_pixels;
            }
        }
    }
    Err("native HDR task exceeded bounded poll count".into())
}
fn check_hdr(root: &Path, entry: &Environment) -> Result<HdrReceipt> {
    require(entry.path.starts_with("exports/") && entry.path.ends_with(".hdr"), "HDR export path mismatch")?;
    require(entry.dimensions == [1024, 512] && !entry.runtime_ready, "unsupported HDR shape/availability")?;
    require(
        entry.role == "environment_radiance" && entry.color_space == "linear_srgb" && entry.transfer_function == "linear",
        "HDR radiance semantics mismatch",
    )?;
    require(
        entry.header_orientation == "-Y +X"
            && !entry.solar_disk
            && entry.energy_owner == if entry.id == "HDR-024" { "none" } else { "environment_map" },
        "HDR orientation/energy ownership mismatch",
    )?;
    let path = format!("{PACK}/environments/{}", entry.path);
    let bytes = read_bounded(root, &path, LIMIT)?;
    verify_bytes(&bytes, entry.bytes, &entry.sha256)?;
    let source = HdrSource::new(bytes)?;
    require([source.info().width, source.info().height] == entry.dimensions, "native HDR dimensions mismatch")?;
    require(
        source.info().exposure == entry.exposure
            && source.info().color_correction == entry.color_correction
            && source.info().primaries == Some(entry.primaries_xy),
        "native HDR color metadata mismatch",
    )?;
    let image = decode_hdr(source)?;
    require(image.pixels().len() == 1024 * 512, "incomplete native HDR pixels")?;
    let mut hash = Sha256::new();
    let (mut low, mut high) = (f64::INFINITY, 0.0_f64);
    let mut mean = [0.0; 3];
    let mut integrals = [[0.0; 3]; 6];
    let width = entry.dimensions[0] as usize;
    let height = entry.dimensions[1] as usize;
    let angles: Vec<_> = (0..width).map(|x| (2.0 * PI * ((x as f64 + 0.5) / width as f64 - 0.5)).sin_cos()).collect();
    for (y, row) in image.pixels().chunks_exact(width).enumerate() {
        let theta = PI * (y as f64 + 0.5) / height as f64;
        let (st, ct) = theta.sin_cos();
        let omega = 2.0 * PI / width as f64 * ((PI * y as f64 / height as f64).cos() - (PI * (y + 1) as f64 / height as f64).cos());
        for (pixel, (sp, cp)) in row.iter().zip(&angles) {
            let direction = [st * cp, -st * cp, st * sp, -st * sp, ct, -ct];
            for (channel, value) in pixel.iter().enumerate() {
                require(value.is_finite() && (0.0..=1e8).contains(value), "nonfinite/out-of-range native HDR radiance")?;
                hash.update(value.to_le_bytes());
                let value = f64::from(*value);
                low = low.min(value);
                high = high.max(value);
                if let Some(total) = mean.get_mut(channel) {
                    *total += value * omega / (4.0 * PI);
                }
                for (integral, cosine) in integrals.iter_mut().zip(direction) {
                    if let Some(total) = integral.get_mut(channel) {
                        *total += value * omega * cosine.max(0.0);
                    }
                }
            }
        }
    }
    require(low == entry.statistics.minimum_channel && high == entry.statistics.maximum_channel, "native HDR channel range mismatch")?;
    if entry.id == "HDR-024" {
        require(low == 0.0 && high == 0.0, "vacuum must contribute exactly zero radiance")?;
    }
    if entry.id == "HDR-005" {
        require(low == 1.0 && high == 1.0, "uniform diagnostic must preserve unit radiance")?;
    }
    for (a, b) in mean.into_iter().zip(entry.statistics.solid_angle_weighted_mean_rgb) {
        require((a - b).abs() < 6e-8, "native HDR solid-angle mean mismatch")?;
    }
    require(entry.statistics.hemisphere_cosine_integrals_rgb.len() == 6, "six directional HDR receipts required")?;
    for (axis, actual) in ["+X", "-X", "+Y", "-Y", "+Z", "-Z"].into_iter().zip(integrals) {
        let expected = entry.statistics.hemisphere_cosine_integrals_rgb.get(axis).ok_or("missing HDR direction receipt")?;
        for (a, b) in actual.into_iter().zip(expected) {
            require((a - b).abs() < 6e-8, format!("native HDR direction mismatch: {} {axis}", entry.id))?;
        }
    }
    Ok(HdrReceipt {
        environment: entry.id.clone(),
        source_sha256: entry.sha256.clone(),
        decoded_f32_le_sha256: format!("{:x}", hash.finalize()),
        pixels: image.pixels().len(),
    })
}
fn validate(root: &Path) -> Result<Report> {
    let mut report = Report { schema: "solidcraft-native-starter-check/1", ..Default::default() };
    let surfaces: Surfaces = serde_json::from_slice(&read_bounded(root, &format!("{PACK}/materials/manifest.json"), 4 * 1024 * 1024)?)?;
    require(
        surfaces.schema == "solidcraft-authored-surface-manifest/1"
            && surfaces.generator_version == "1.0.0"
            && surfaces.license == "MIT OR Apache-2.0",
        "unknown surface manifest",
    )?;
    require(
        surfaces.items.len() == 48 && surfaces.source_path == format!("{PACK}/materials/source/recipes.json"),
        "surface inventory/source mismatch",
    )?;
    pinned_source(root, &surfaces.source_path, &surfaces.source_sha256)?;
    pinned_source(root, &format!("{PACK}/tools/materials.py"), &surfaces.generator_sha256)?;
    for (index, surface) in surfaces.items.iter().enumerate() {
        require(surface.id == format!("SURF-{:03}", index + 1) && surface.runtime_status == "unbound", "surface identity/availability mismatch")?;
        require(
            surface.normal_convention == NORMAL_CONVENTION && surface.tile_mm.iter().all(|x| x.is_finite() && *x > 0.0 && *x <= 10000.0),
            "surface basis/scale mismatch",
        )?;
        for (name, role, map) in [
            ("base_color", MapRole::BaseColor, &surface.maps.base_color),
            ("normal_open_gl", MapRole::NormalOpenGl, &surface.maps.normal_open_gl),
            ("roughness", MapRole::Roughness, &surface.maps.roughness),
            ("metallic", MapRole::Metallic, &surface.maps.metallic),
        ] {
            let receipt = check_texture(root, surface, name, role, map)?;
            report.texture_inputs += 1;
            report.encoded_resource_bytes += map.size_bytes;
            report.mip_levels += receipt.mip_levels;
            report.textures.push(receipt);
        }
        check_height(root, surface)?;
        report.encoded_resource_bytes += surface.maps.height.size_bytes;
        report.height_sources_verified_not_bound += 1;
        report.surfaces += 1;
        if (index + 1) % 12 == 0 {
            eprintln!("Native material admission: {}/48", index + 1);
        }
    }
    let environments: Environments = serde_json::from_slice(&read_bounded(root, &format!("{PACK}/environments/manifest.json"), 4 * 1024 * 1024)?)?;
    require(
        environments.schema == "solidcraft.render-environment-manifest/1"
            && environments.version == "1.0.0"
            && environments.license == "MIT OR Apache-2.0"
            && !environments.runtime_ready,
        "unknown HDR manifest",
    )?;
    require(
        environments.items.len() == 24 && environments.recipe_source == "source/recipes.json" && environments.generator == "../tools/environments.py",
        "HDR inventory/source mismatch",
    )?;
    pinned_source(root, &format!("{PACK}/environments/source/recipes.json"), &environments.recipe_sha256)?;
    pinned_source(root, &format!("{PACK}/tools/environments.py"), &environments.generator_sha256)?;
    for (index, entry) in environments.items.iter().enumerate() {
        require(entry.id == format!("HDR-{:03}", index + 1), "HDR identity mismatch")?;
        report.environments.push(check_hdr(root, entry)?);
        report.hdr_inputs += 1;
        report.encoded_resource_bytes += entry.bytes;
    }
    Ok(report)
}
fn root() -> Result<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).ok_or("missing repository root")?.to_path_buf())
}
fn run() -> Result<()> {
    require(std::env::args_os().count() == 1, "usage: cargo run --locked --profile ci -p solidcraft-render --example validate_starter")?;
    let report = validate(&root()?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Native starter validation failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_render::{lighting::HdrErrorCode, texture_sampling::mips::MipError};

    #[test]
    fn portable_paths_reject_escapes_and_ambiguous_windows_forms() {
        for path in ["", "/tmp/x", "../x", "a/../x", "a//x", "a/./x", "C:/x", "C:x", "a\\x", "a/x:stream", "a/", "x\0y"] {
            assert!(relative_path(path).is_err(), "{path:?}");
        }
        assert!(relative_path("assets/render/starter/materials/exports/SURF-001/base_color.png").is_ok());
    }
    #[test]
    fn receipts_reject_corruption_wrong_size_and_malformed_hashes() -> Result<()> {
        let bytes = b"original";
        verify_bytes(bytes, bytes.len(), &sha(bytes))?;
        assert!(verify_bytes(b"modified", bytes.len(), &sha(bytes)).is_err());
        assert!(verify_bytes(bytes, bytes.len() + 1, &sha(bytes)).is_err());
        assert!(verify_bytes(bytes, bytes.len(), &"F".repeat(64)).is_err());
        Ok(())
    }
    #[test]
    fn bounded_reads_refuse_small_budgets() -> Result<()> {
        assert!(read_bounded(&root()?, &format!("{PACK}/pack.json"), 2).is_err());
        assert!(read_bounded(&root()?, "../Cargo.toml", LIMIT).is_err());
        Ok(())
    }
    #[test]
    fn authored_v_up_is_mapped_once_and_green_normal_sign_is_preserved() -> Result<()> {
        let pixels = [0.5, 0.75, 1.0, 1.0, 0.5, 0.75, 1.0, 1.0, 0.5, 0.25, 1.0, 1.0, 0.5, 0.25, 1.0, 1.0];
        let view = TextureView::new(&pixels, [2, 2])?;
        let top = normal(sample_authored(view, [0.25, 0.75])?)?;
        let bottom = normal(sample_authored(view, [0.25, 0.25])?)?;
        assert!(top.y > 0.0 && bottom.y < 0.0 && top.z > 0.0 && bottom.z > 0.0);
        assert!((top.len() - 1.0).abs() < 1e-12);
        Ok(())
    }
    #[test]
    fn srgb_reference_anchors_distinguish_color_from_data() {
        assert_eq!(srgb_reference(0), 0.0);
        assert_eq!(srgb_reference(255), 1.0);
        assert!((srgb_reference(128) - 0.21586050011389926).abs() < 1e-14);
        assert!((srgb_reference(10) - (10.0 / 255.0) / 12.92).abs() < 1e-15);
    }
    #[test]
    fn texture_and_normalization_cancellation_keep_source_reusable() -> Result<()> {
        let bytes = read_bounded(&root()?, &format!("{PACK}/materials/exports/SURF-001/base_color.png"), LIMIT)?;
        let strict = TextureSource::new(bytes.clone(), MapRole::BaseColor)?;
        assert!(strict.decode(|| true).is_err());
        assert_eq!(strict.decode(|| false)?.info().width, 512);
        let options = NormalizationOptions { unprofiled_color: UnprofiledColor::DeclareSrgbPrimaries, ..Default::default() };
        let source = NormalizationSource::new(bytes, MapRole::BaseColor, options)?;
        assert!(source.decode(|| true).is_err());
        assert_eq!(source.decode(|| false)?.receipt().output_dimensions, [512, 512]);
        Ok(())
    }
    #[test]
    fn mip_cancellation_does_not_return_partial_chain() -> Result<()> {
        let pixels = [0.5; 16];
        let view = TextureView::new(&pixels, [2, 2])?;
        assert!(matches!(MipChain::prepare(view, MAX_MIP_TAIL_BYTES, || true), Err(MipError::Cancelled)));
        assert_eq!(MipChain::prepare(view, MAX_MIP_TAIL_BYTES, || false)?.level_count(), 2);
        Ok(())
    }
    #[test]
    fn hdr_cancellation_consumes_only_its_task_not_the_source() -> Result<()> {
        let bytes = read_bounded(&root()?, &format!("{PACK}/environments/exports/vacuum-black.hdr"), LIMIT)?;
        let source = HdrSource::new(bytes)?;
        let mut task = HdrDecodeTask::new(source.clone())?;
        assert!(matches!(task.step(64, || true), Err(e) if e.code == HdrErrorCode::Cancelled));
        assert!(matches!(task.step(64, || false), Err(e) if e.code == HdrErrorCode::Consumed));
        assert!(decode_hdr(source)?.pixels().iter().all(|p| *p == [0.0; 3]));
        Ok(())
    }
    #[test]
    fn map_contract_rejects_duplicate_known_fields_and_unknown_maps() -> Result<()> {
        let bytes = read_bounded(&root()?, &format!("{PACK}/materials/manifest.json"), 4 * 1024 * 1024)?;
        let source: serde_json::Value = serde_json::from_slice(&bytes)?;
        let maps = source.get("items").and_then(|v| v.get(0)).and_then(|v| v.get("maps")).ok_or("missing fixture maps")?;
        let valid = serde_json::to_string(maps)?;
        assert!(serde_json::from_str::<Maps>(&valid).is_ok());
        let prefix = valid.strip_suffix('}').ok_or("incomplete fixture")?;
        let base = serde_json::to_string(maps.get("base_color").ok_or("missing base color")?)?;
        let duplicate = format!("{prefix},\"base_color\":{base}}}");
        let error = serde_json::from_str::<Maps>(&duplicate).err().ok_or("duplicate accepted")?;
        assert!(error.to_string().contains("duplicate field"));
        let extra = format!("{prefix},\"emission\":null}}");
        let error = serde_json::from_str::<Maps>(&extra).err().ok_or("extra role accepted")?;
        assert!(error.to_string().contains("unknown field"));
        Ok(())
    }
}
