//! Editable subdivision cages in Blender documents. The SDNA table describes
//! the file's own layouts; no Blender implementation is linked or required.
//! Format reference: <https://developer.blender.org/docs/features/core/dna/>.
use crate::{IoError, Result};
use solidcraft_geom::subdivision::Cage;
use std::io::Read;

mod dna;
mod mesh;
mod write;

pub const MAX_BLEND_BYTES: usize = 256 << 20;

#[derive(Clone, Debug)]
pub struct Object {
    pub name: String,
    /// Millimetres in the document's world frame, with reflected winding fixed.
    pub cage: Cage,
    pub levels: usize,
}

/// An evaluated CAD display mesh, exported without a subdivision modifier.
#[derive(Clone, Debug)]
pub struct SurfaceObject {
    pub name: String,
    pub mesh: std::sync::Arc<solidcraft_geom::Mesh>,
}

#[derive(Clone, Debug, Default)]
pub struct Import {
    pub objects: Vec<Object>,
    /// Unsupported objects are reported individually, never silently flattened.
    pub rejected: Vec<String>,
}

fn bad(message: impl Into<String>) -> IoError {
    IoError::Invalid(format!("Blender: {}", message.into()))
}

fn expanded(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() > MAX_BLEND_BYTES {
        return Err(bad("file exceeds 256 MiB"));
    }
    let mut out = Vec::new();
    if bytes.starts_with(&[0x1f, 0x8b]) {
        flate2::read::MultiGzDecoder::new(bytes).take(MAX_BLEND_BYTES as u64 + 1).read_to_end(&mut out).map_err(|e| bad(e.to_string()))?;
    } else if bytes.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        let mut remaining = bytes;
        let mut frames = 0;
        while !remaining.is_empty() {
            frames += 1;
            if frames > 100_000 {
                return Err(bad("too many compression frames"));
            }
            let before = remaining.len();
            if remaining.len() >= 8 {
                let magic = u32::from_le_bytes(remaining[..4].try_into().map_err(|_| bad("compression header"))?);
                if (0x184d2a50..=0x184d2a5f).contains(&magic) {
                    let len = u32::from_le_bytes(remaining[4..8].try_into().map_err(|_| bad("compression skip length"))?) as usize;
                    remaining = remaining
                        .get(8usize.checked_add(len).ok_or_else(|| bad("compression skip overflow"))?..)
                        .ok_or_else(|| bad("truncated compression skip frame"))?;
                    continue;
                }
            }
            let mut decoder =
                ruzstd::decoding::StreamingDecoder::new_with_max_window_size(remaining, MAX_BLEND_BYTES as u64).map_err(|e| bad(e.to_string()))?;
            let budget = MAX_BLEND_BYTES.saturating_sub(out.len()) as u64 + 1;
            decoder.by_ref().take(budget).read_to_end(&mut out).map_err(|e| bad(e.to_string()))?;
            if out.len() > MAX_BLEND_BYTES {
                return Err(bad("expanded file exceeds 256 MiB"));
            }
            remaining = decoder.into_inner();
            if remaining.len() >= before {
                return Err(bad("compression decoder made no progress"));
            }
        }
    } else {
        return Ok(bytes.to_vec());
    }
    if out.len() > MAX_BLEND_BYTES {
        return Err(bad("expanded file exceeds 256 MiB"));
    }
    Ok(out)
}

/// Read all supported mesh objects as independent editable Forms. Unsupported
/// modifiers are explicit rejections, so their evaluated appearance is not lost
/// without notice. Loading a file never executes its scripts.
pub fn read(bytes: &[u8]) -> Result<Import> {
    let bytes = expanded(bytes)?;
    let file = dna::File::read(&bytes)?;
    mesh::read(&file)
}

/// Export editable cages with Catmull-Clark modifiers and normalized creases.
/// Millimetres are retained through the scene unit scale.
pub fn write(objects: &[Object]) -> Result<Vec<u8>> {
    write::write(objects)
}

/// Export evaluated surfaces as triangle meshes. This preserves the displayed
/// shape of constrained and locally refined Forms without substituting a cage.
pub fn write_surfaces(objects: &[SurfaceObject]) -> Result<Vec<u8>> {
    write::write_surfaces(objects)
}
