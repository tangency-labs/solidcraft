use crate::BinaryReader;

pub(super) struct View;
impl View {
    pub fn u16_be_at(bytes: &[u8], at: usize) -> Option<u16> {
        BinaryReader::with_position(bytes, at).ok()?.u16().ok()
    }
    pub fn u32_be_at(bytes: &[u8], at: usize) -> Option<u32> {
        let mut reader = BinaryReader::with_position(bytes, at).ok()?;
        Some(u32::from_be_bytes(reader.bytes(4).ok()?.try_into().ok()?))
    }
    pub fn f64_be_at(bytes: &[u8], at: usize) -> Option<f64> {
        BinaryReader::with_position(bytes, at).ok()?.f64().ok()
    }
}
