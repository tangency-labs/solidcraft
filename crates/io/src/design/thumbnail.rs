//! Portable, disposable PNG pictures. Reading never resolves source blobs.
use super::*;
pub const THUMBNAIL_SIZE: [usize; 2] = [240, 150];
const MAX_BYTES: usize = 256 << 10;
const FORMAT: &str = "solidcraft-thumbnail/1";

/// The design's picture, as the shared container keeps it.
const PICTURE: solidcraft_format::container::Thumbnail =
    solidcraft_format::container::Thumbnail { format: FORMAT, app: env!("CARGO_PKG_VERSION"), size: THUMBNAIL_SIZE, max_bytes: MAX_BYTES };

pub(super) fn write(writer: &mut PackageWriter, png: &[u8], document: &[u8]) -> Result<()> {
    Ok(PICTURE.write(writer, png, document, max_design())?)
}
pub(super) fn read<R: Read + Seek>(reader: &mut PackageReader<R>) -> Result<Vec<u8>> {
    let container: Value = serde_json::from_slice(&reader.limited_part("container.json", 4096)?).map_err(invalid)?;
    if container.get("format").and_then(Value::as_str) != Some(VERSION) {
        return Err(invalid("thumbnail container version"));
    }
    // Consumers must also use a bounded PNG decoder; the header is not a decode.
    Ok(PICTURE.read(reader, 16 << 20)?)
}
/// Optional PNG bytes, bounded and keyed to this exact document. Consumers must
/// decode with dimension/allocation limits; invalid pictures are cache misses.
pub fn read_design_thumbnail(bytes: &[u8]) -> Option<Vec<u8>> {
    DesignContainerReader::new(Cursor::new(bytes)).ok()?.thumbnail()
}
/// Read a portable picture without importing or reading the source blobs.
pub fn read_design_thumbnail_file(path: &str) -> Option<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    let source = {
        crate::vfs::check(path).ok()?;
        let metadata = std::fs::metadata(path).ok()?;
        if !metadata.is_file() || metadata.len() > max_design() as u64 {
            return None;
        }
        let source = std::fs::File::open(path).ok()?;
        let metadata = source.metadata().ok()?;
        if !metadata.is_file() || metadata.len() > max_design() as u64 {
            return None;
        }
        source
    };
    #[cfg(target_arch = "wasm32")]
    let source = Cursor::new(crate::vfs::read(path, max_design()).ok()?);
    DesignContainerReader::new(source).ok()?.thumbnail()
}
