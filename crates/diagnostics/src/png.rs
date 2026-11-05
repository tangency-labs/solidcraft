//! An 8-bit RGB PNG: signature, `IHDR`, one zlib `IDAT`, `IEND` (ISO/IEC 15948).

/// The PNG of a `width` × `height` RGB picture whose rows run top to bottom.
pub fn encode(width: usize, height: usize, rgb: &[u8]) -> Vec<u8> {
    let row = width * 3;
    let mut raw = Vec::with_capacity((row + 1) * height);
    for line in rgb.chunks(row.max(1)).take(height) {
        raw.push(0); // filter type None
        raw.extend_from_slice(line);
    }
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&(width as u32).to_be_bytes());
    header.extend_from_slice(&(height as u32).to_be_bytes());
    // bit depth 8, colour type 2 (RGB), deflate, adaptive filtering, no interlace
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    chunk(&mut out, *b"IHDR", &header);
    chunk(&mut out, *b"IDAT", &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6));
    chunk(&mut out, *b"IEND", &[]);
    out
}

/// Appends one chunk: length, type, data, and the CRC of type and data.
fn chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(&kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(kind.iter().chain(data)).to_be_bytes());
}

/// The CRC-32 PNG uses (polynomial 0xEDB88320, reflected).
fn crc32<'a>(bytes: impl Iterator<Item = &'a u8>) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CRC of `IEND` with no data is the one every PNG ends with.
    #[test]
    fn iend_crc() {
        assert_eq!(crc32(b"IEND".iter()), 0xae42_6082);
    }

    /// A 2 × 1 picture starts with the signature and carries its size in `IHDR`.
    #[test]
    fn header() {
        let png = encode(2, 1, &[255, 0, 0, 0, 0, 255]);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[16..24], &[0, 0, 0, 2, 0, 0, 0, 1]);
    }
}
