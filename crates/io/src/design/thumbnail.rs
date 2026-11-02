//! Portable, disposable PNG pictures. Reading never resolves source blobs.
use super::*;
pub const THUMBNAIL_SIZE: [usize; 2] = [240, 150];
const MAX_BYTES: usize = 256 << 10;
const FORMAT: &str = "solidcraft-thumbnail/1";

fn valid_header(bytes: &[u8]) -> bool {
    bytes.len() <= MAX_BYTES
        && bytes.get(..8) == Some(b"\x89PNG\r\n\x1a\n")
        && bytes.get(8..16) == Some(b"\0\0\0\rIHDR")
        && bytes.get(16..20) == Some((THUMBNAIL_SIZE[0] as u32).to_be_bytes().as_slice())
        && bytes.get(20..24) == Some((THUMBNAIL_SIZE[1] as u32).to_be_bytes().as_slice())
}
pub(super) fn write(writer: &mut PackageWriter, png: &[u8], document: &[u8]) -> Result<()> {
    if !valid_header(png) {
        return Ok(());
    }
    let key = serde_json::to_vec(&json!({"format": FORMAT, "app": env!("CARGO_PKG_VERSION"), "document": digest(document), "payload": digest(png)}))
        .map_err(invalid)?;
    if !writer.room(2, key.len().saturating_add(png.len()), max_design()) {
        return Ok(());
    }
    writer.part("cache/thumbnail.json", &key, false)?;
    writer.part("cache/thumbnail.png", png, false)
}
pub(super) fn read<R: Read + Seek>(reader: &mut PackageReader<R>) -> Result<Vec<u8>> {
    let container: Value = serde_json::from_slice(&reader.limited_part("container.json", 4096)?).map_err(invalid)?;
    if container.get("format").and_then(Value::as_str) != Some(VERSION) {
        return Err(invalid("thumbnail container version"));
    }
    let key: Value = serde_json::from_slice(&reader.limited_part("cache/thumbnail.json", 4096)?).map_err(invalid)?;
    let document = reader.document(16 << 20)?;
    if key.get("format").and_then(Value::as_str) != Some(FORMAT)
        || key.get("app").and_then(Value::as_str) != Some(env!("CARGO_PKG_VERSION"))
        || key.get("document").and_then(Value::as_str) != Some(digest(&document).as_str())
    {
        return Err(invalid("thumbnail key"));
    }
    let png = reader.limited_part("cache/thumbnail.png", MAX_BYTES)?;
    if !valid_header(&png) || key.get("payload").and_then(Value::as_str) != Some(digest(&png).as_str()) {
        return Err(invalid("thumbnail payload"));
    }
    // Consumers must also use a bounded PNG decoder; the header is not a decode.
    Ok(png)
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
