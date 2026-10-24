//! Decoder for the exact retained native-master profile, not arbitrary external
//! EXR import. Dimensions, header, offsets, row sizes and every sample are checked.
use super::{CancelToken, ErrorCode, JobError, Result};

fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}

pub(super) struct Decoder {
    width: usize,
    height: usize,
    row: usize,
    table: usize,
    first: usize,
    pixels: Vec<[f32; 4]>,
}
impl Decoder {
    pub(super) fn new(bytes: &[u8], width: u32, height: u32, cancel: &CancelToken) -> Result<Self> {
        cancel.check()?;
        let limit = super::linear_exr::encoded_limit(width, height)?;
        let expected = super::linear_exr::header(width, height)?;
        if bytes.len() as u64 > limit || !bytes.starts_with(&expected) {
            return Err(invalid("master is not the retained native FLOAT32 linear-sRGB/D65 profile"));
        }
        let first = expected.len() + height as usize * 8;
        let length = first + height as usize * (width as usize * 16 + 8);
        if bytes.len() != length {
            return Err(invalid("native master has missing or trailing scanline bytes"));
        }
        let count = width as usize * height as usize;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(count).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "master decoding allocation failed"))?;
        if pixels.capacity() != count {
            return Err(invalid("master decoding exceeded admitted capacity"));
        }
        Ok(Self { width: width as usize, height: height as usize, row: 0, table: expected.len(), first, pixels })
    }
    pub(super) fn step(&mut self, bytes: &[u8], cancel: &CancelToken) -> Result<bool> {
        cancel.check()?;
        for _ in 0..4 {
            if self.row == self.height {
                break;
            }
            let row_bytes = self.width * 16;
            let offset = self.first + self.row * (row_bytes + 8);
            let table = self.table + self.row * 8;
            let stored = bytes.get(table..table + 8).and_then(|s| s.try_into().ok()).map(u64::from_le_bytes);
            let y = word(bytes, offset)?;
            let length = word(bytes, offset + 4)?;
            if stored != Some(offset as u64) || y != self.row as u32 || length as usize != row_bytes {
                return Err(invalid("native master scanline offset, index or length differs"));
            }
            for x in 0..self.width {
                if x % 4096 == 0 {
                    cancel.check()?;
                }
                let mut pixel = [0.0; 4];
                for (plane, channel) in [3, 2, 1, 0].into_iter().enumerate() {
                    pixel[channel] = f32::from_bits(word(bytes, offset + 8 + plane * self.width * 4 + x * 4)?);
                }
                if pixel.iter().any(|value| !value.is_finite() || *value < 0.0) || pixel[3] > 1.0 {
                    return Err(invalid("native master has invalid radiance or coverage"));
                }
                self.pixels.push(pixel);
            }
            self.row += 1;
        }
        cancel.check()?;
        Ok(self.row == self.height)
    }
    pub(super) fn rows(&self) -> u64 {
        self.row as u64
    }
    pub(super) fn finish(self) -> Result<Vec<[f32; 4]>> {
        if self.row != self.height {
            return Err(invalid("native master decoding is incomplete"));
        }
        Ok(self.pixels)
    }
}
fn word(bytes: &[u8], offset: usize) -> Result<u32> {
    bytes.get(offset..offset + 4).and_then(|s| s.try_into().ok()).map(u32::from_le_bytes).ok_or_else(|| invalid("truncated native master word"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> (Vec<u8>, Vec<[f32; 4]>) {
        let pixels = vec![[8.0, 0.1, 2.0, 0.5], [0.0, -0.0, 0.000001, 0.0]];
        let mut task = super::super::linear_exr::ExrTask::new(2, 1, pixels.clone(), CancelToken::default()).unwrap();
        assert!(task.step(1).unwrap());
        (task.finish().unwrap().0, pixels)
    }
    #[test]
    fn exact_native_profile_roundtrips_bits_and_rejects_wrong_layout_or_partial_decode() {
        let (bytes, pixels) = source();
        let cancel = CancelToken::default();
        let mut decoder = Decoder::new(&bytes, 2, 1, &cancel).unwrap();
        assert!(decoder.step(&bytes, &cancel).unwrap());
        assert_eq!(
            decoder.finish().unwrap().iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>(),
            pixels.iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>()
        );
        assert!(Decoder::new(&bytes, 1, 2, &cancel).is_err());
        assert!(Decoder::new(&bytes[..bytes.len() - 1], 2, 1, &cancel).is_err());
        assert!(Decoder::new(&bytes, 2, 1, &cancel).unwrap().finish().is_err());
        let mut corrupt = bytes.clone();
        let table = super::super::linear_exr::header(2, 1).unwrap().len();
        corrupt[table] ^= 1;
        assert!(Decoder::new(&corrupt, 2, 1, &cancel).unwrap().step(&corrupt, &cancel).is_err());
        let mut corrupt = bytes.clone();
        let last = corrupt.len() - 4;
        corrupt[last..].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(Decoder::new(&corrupt, 2, 1, &cancel).unwrap().step(&corrupt, &cancel).is_err());
        let mut decoder = Decoder::new(&bytes, 2, 1, &cancel).unwrap();
        cancel.cancel();
        assert!(decoder.step(&bytes, &cancel).is_err());
        assert_eq!(decoder.rows(), 0);
    }
}
