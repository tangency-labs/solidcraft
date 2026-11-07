//! One read-only storage buffer: body lookup, planar records and linear RGBA mip chains.
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
use solidcraft_render::job::{
    CancelToken, ErrorCode, JobError, Result,
    textures::{BaseColorMode, RuntimeTextures, TextureMap},
};
use solidcraft_render::texture_sampling::{
    TextureView,
    mips::{MAX_MIP_TAIL_BYTES, MipChain},
};

fn vector(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}
fn write_word(bytes: &mut [u8], word: usize, value: u32) -> Result<()> {
    bytes
        .get_mut(word * 4..word * 4 + 4)
        .ok_or_else(|| JobError::new(ErrorCode::Internal, "texture record outside allocation"))?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}
fn pixels(
    bytes: &mut Vec<u8>,
    map: Option<&TextureMap>,
    cancel: &CancelToken,
    shared: &mut std::collections::BTreeMap<([u8; 32], u8), (u32, [u32; 2])>,
) -> Result<(u32, [u32; 2])> {
    let Some(map) = map else {
        return Ok((0, [0; 2]));
    };
    cancel.check()?;
    if let Some(record) = shared.get(&map.identity()) {
        return Ok(*record);
    }
    let rgba = map.decode(cancel)?;
    let view = TextureView::new(&rgba, map.dimensions()).map_err(|error| JobError::new(ErrorCode::InvalidRequest, error.to_string()))?;
    // Decode roles before averaging: reflectance is linear sRGB; scalar and
    // tangent-normal channels remain linear data. Alpha never participates.
    // Existing temporary and saved built-in maps keep their level-zero sampler.
    // Do not generate or retain an unused tail for those legacy inputs.
    let mips = if map.gpu_mip_filtering() {
        Some(MipChain::prepare(view, MAX_MIP_TAIL_BYTES, || cancel.is_cancelled()).map_err(|error| {
            JobError::new(if cancel.is_cancelled() { ErrorCode::Cancelled } else { ErrorCode::InvalidRequest }, error.to_string())
        })?)
    } else {
        None
    };
    let level_count = mips.as_ref().map_or(1, MipChain::level_count);
    let offset = bytes.len() / 4;
    let descriptor_words = 4 + level_count * 4;
    bytes.resize(bytes.len() + descriptor_words * 4, 0);
    write_word(bytes, offset, level_count as u32)?;
    write_word(bytes, offset + 1, 2)?;
    write_word(bytes, offset + 2, u32::from(map.gpu_mip_filtering()))?;
    for level in 0..level_count {
        let (pixels, dimensions) = match &mips {
            Some(mips) => (
                mips.level(level).ok_or_else(|| JobError::new(ErrorCode::Internal, "missing prepared GPU mip pixels"))?,
                mips.dimensions(level).ok_or_else(|| JobError::new(ErrorCode::Internal, "missing prepared GPU mip dimensions"))?,
            ),
            None => (view, map.dimensions()),
        };
        let descriptor = offset + 4 + level * 4;
        let pixel_offset = u32::try_from(bytes.len() / 4).map_err(|_| JobError::new(ErrorCode::LimitExceeded, "GPU mip buffer offset overflow"))?;
        write_word(bytes, descriptor, pixel_offset)?;
        write_word(bytes, descriptor + 1, dimensions[0])?;
        write_word(bytes, descriptor + 2, dimensions[1])?;
        for (index, value) in pixels.rgba().iter().enumerate() {
            if index % 4096 == 0 {
                cancel.check()?;
            }
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    let record = (u32::try_from(offset).map_err(|_| JobError::new(ErrorCode::LimitExceeded, "texture buffer offset overflow"))?, map.dimensions());
    shared.insert(map.identity(), record);
    Ok(record)
}
pub(super) fn flatten(resources: Option<&RuntimeTextures>, body_count: usize, origin: Vec3, cancel: &CancelToken) -> Result<Vec<u8>> {
    let Some(resources) = resources else {
        return Ok(vec![0; 16]);
    };
    let header = (4 + body_count + 1).next_multiple_of(4);
    let record_words = resources.bindings().len() * 24;
    let capacity = usize::try_from(packed_bytes(resources, body_count))
        .map_err(|_| JobError::new(ErrorCode::LimitExceeded, "GPU mip buffer capacity overflow"))?;
    let mut bytes = super::scene::reserved(capacity)?;
    bytes.resize((header + record_words) * 4, 0);
    write_word(&mut bytes, 0, (body_count + 1) as u32)?;
    let mut shared = std::collections::BTreeMap::new();
    for (index, binding) in resources.bindings().iter().enumerate() {
        cancel.check()?;
        let record = header + index * 24;
        write_word(&mut bytes, 4 + binding.body_index + 1, record as u32)?;
        let delta = vector(binding.frame.origin) - origin;
        if [delta.x, delta.y, delta.z].iter().any(|v| v.abs() > 1e7) {
            return Err(JobError::new(ErrorCode::InvalidScene, "texture frame must be within 1e7 mm of camera target"));
        }
        let u = vector(binding.frame.u);
        let v = vector(binding.frame.v);
        for (offset, p) in [(0, delta), (4, u / u.len2()), (8, v / v.len2())] {
            for (k, c) in [p.x, p.y, p.z].into_iter().enumerate() {
                write_word(&mut bytes, record + offset + k, (c as f32).to_bits())?;
            }
        }
        let (base, base_size) = pixels(&mut bytes, binding.base_color.as_ref(), cancel, &mut shared)?;
        let (normal, normal_size) = pixels(&mut bytes, binding.normal.as_ref(), cancel, &mut shared)?;
        let (roughness, roughness_size) = pixels(&mut bytes, binding.roughness.as_ref(), cancel, &mut shared)?;
        let (metallic, metallic_size) = pixels(&mut bytes, binding.metallic.as_ref(), cancel, &mut shared)?;
        let roles = if metallic != 0 {
            4
        } else if roughness != 0 {
            3
        } else if normal != 0 {
            2
        } else {
            1
        };
        let flags = u32::from(binding.flip_v) | (u32::from(binding.base_color_mode == BaseColorMode::Replace) << 1);
        for (offset, value) in [
            (3, base),
            (7, base_size[0]),
            (11, base_size[1]),
            (12, normal),
            (13, normal_size[0]),
            (14, normal_size[1]),
            (15, (binding.normal_strength as f32).to_bits()),
            (16, roughness),
            (17, roughness_size[0]),
            (18, roughness_size[1]),
            (19, flags),
            (20, metallic),
            (21, metallic_size[0]),
            (22, metallic_size[1]),
            (23, roles),
        ] {
            write_word(&mut bytes, record + offset, value)?;
        }
    }
    cancel.check()?;
    Ok(bytes)
}

/// Exact packed storage admission for the admitted dimensions and filtering policy.
/// Count each content/role/policy once, including only level zero for legacy maps.
pub(super) fn packed_bytes(resources: &RuntimeTextures, body_count: usize) -> u64 {
    let bindings = resources.bindings().len() as u64;
    let mut bytes = 4 * ((5 + body_count as u64).next_multiple_of(4) + bindings * 24);
    let mut seen = std::collections::BTreeSet::new();
    for binding in resources.bindings() {
        for map in [&binding.base_color, &binding.normal, &binding.roughness, &binding.metallic].into_iter().flatten() {
            if !seen.insert(map.identity()) {
                continue;
            }
            let [mut width, mut height] = map.dimensions().map(u64::from);
            let mut levels = 1;
            let mut pixels = width * height;
            if map.gpu_mip_filtering() {
                while width > 1 || height > 1 {
                    width = (width / 2).max(1);
                    height = (height / 2).max(1);
                    pixels += width * height;
                    levels += 1;
                }
            }
            bytes += (4 + levels * 4) * 4 + pixels * 16;
        }
    }
    bytes
}
