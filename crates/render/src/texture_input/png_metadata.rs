//! Allocation-free PNG metadata policy and cooperative source validation.
//! The CPU worker checks every original chunk CRC before opening its row codec.
//! Header admission uses the same ICC/EXIF/order policy without decoding pixels.
use super::{MapRole, Result, invalid};

pub(crate) const BLOCK_BYTES: usize = 4096;
// png0.18.1 uses a128KiB row-ring shift threshold and18KiB boxed
// Huffman tables. This covers ring growth/relocation, bounded rows,
// palette and fixed parser metadata for the admitted texture dimensions;
// variable chunk/text/EXIF allocations are counted separately below.
const FIXED_CODEC_BYTES: u64 = 2 * 1024 * 1024;
const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const BEFORE_DATA: [[u8; 4]; 15] = [
    *b"cHRM", *b"cICP", *b"gAMA", *b"mDCV", *b"cLLI", *b"sBIT", *b"sRGB", *b"bKGD", *b"hIST", *b"tRNS", *b"pHYs", *b"sPLT", *b"PLTE", *b"eXIf",
    *b"iCCP",
];
const fn crc_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut i = 0;
    while i < 256 {
        let mut value = i as u32;
        let mut bit = 0;
        while bit < 8 {
            value = (value >> 1) ^ (0xedb8_8320_u32.wrapping_mul(value & 1));
            bit += 1;
        }
        table[i] = value;
        i += 1;
    }
    table
}
const CRC_TABLE: [u32; 256] = crc_table();
fn be32(bytes: &[u8], at: usize) -> Result<u32> {
    let end = at.checked_add(4).ok_or_else(|| invalid("PNG metadata offset overflow"))?;
    let value: [u8; 4] =
        bytes.get(at..end).ok_or_else(|| invalid("PNG metadata is truncated"))?.try_into().map_err(|_| invalid("PNG metadata word"))?;
    Ok(u32::from_be_bytes(value))
}
fn word(bytes: &[u8], at: usize, length: usize, little: bool) -> Result<u32> {
    let end = at.checked_add(length).ok_or_else(|| invalid("PNG EXIF offset overflow"))?;
    let value = bytes.get(at..end).ok_or_else(|| invalid("PNG EXIF is truncated"))?;
    let mut output = 0_u32;
    for (index, byte) in value.iter().enumerate() {
        let shift = if little { index } else { length - index - 1 };
        output |= u32::from(*byte) << (shift * 8);
    }
    Ok(output)
}
#[derive(Clone, Copy)]
enum Phase {
    Header,
    Bytes { next: usize, crc: u32 },
    ExifHeader,
    ExifEntry { next: usize, remaining: u16, little: bool, orientation_seen: bool },
    Done,
}
/// Fixed metadata only. No byte copy, map decode, recursion or caller-owned lease.
pub(crate) struct PngMetadata {
    phase: Phase,
    position: usize,
    kind: [u8; 4],
    data_start: usize,
    data_end: usize,
    chunk_end: usize,
    data_seen: bool,
    data_ended: bool,
    palette_seen: bool,
    exif_seen: bool,
    time_seen: bool,
    cpu: bool,
    verify_crc: bool,
    srgb: bool,
    color_seen: u8,
    raw_capacity: u64,
    exif_bytes: u64,
    text_payload: u64,
    text_counts: [u64; 3],
}
impl PngMetadata {
    pub(crate) fn new(cpu: bool, verify_crc: bool) -> Self {
        Self {
            phase: Phase::Header,
            position: 8,
            kind: [0; 4],
            data_start: 0,
            data_end: 0,
            chunk_end: 0,
            data_seen: false,
            data_ended: false,
            palette_seen: false,
            exif_seen: false,
            time_seen: false,
            cpu,
            verify_crc,
            srgb: false,
            color_seen: 0,
            raw_capacity: 128,
            exif_bytes: 0,
            text_payload: 0,
            text_counts: [0; 3],
        }
    }
    /// Apply role-specific declarations during the existing bounded header steps.
    pub(crate) fn with_role(mut self, role: MapRole) -> Self {
        self.srgb = role == MapRole::BaseColor;
        self
    }
    fn text_capacities(&self) -> Result<[u64; 3]> {
        let mut capacities = [0; 3];
        for (out, count) in capacities.iter_mut().zip(self.text_counts) {
            *out = if count == 0 { 0 } else { count.checked_next_power_of_two().ok_or_else(|| invalid("PNG text capacity overflow"))?.max(4) };
        }
        Ok(capacities)
    }
    fn text_backing(&self) -> Result<u64> {
        // ISO8859-1 expands to at most2UTF8 bytes per source byte. Up to
        // four owned strings/vectors per text record also get their small
        // growth backing; compressed text stays compressed in these callers.
        self.text_payload
            .checked_mul(2)
            .and_then(|n| n.checked_add(self.text_counts.into_iter().zip([16_u64, 16, 32]).map(|(count, backing)| count * backing).sum::<u64>()))
            .ok_or_else(|| invalid("PNG text storage overflow"))
    }
    fn codec_storage(&mut self, length: usize) -> Result<()> {
        use png::text_metadata::{ITXtChunk, TEXtChunk, ZTXtChunk};
        let sizes = [std::mem::size_of::<TEXtChunk>() as u64, std::mem::size_of::<ZTXtChunk>() as u64, std::mem::size_of::<ITXtChunk>() as u64];
        let prior = self.text_capacities()?;
        let vector_bytes = |counts: [u64; 3]| counts.into_iter().zip(sizes).map(|(n, size)| n * size).sum::<u64>();
        let raw = (length as u64).checked_next_power_of_two().ok_or_else(|| invalid("PNG raw capacity overflow"))?.max(128).max(self.raw_capacity);
        // The chunk buffer grows by doubling from128. Include the old
        // allocation during its final growth before any new parsed value exists.
        let growth = if raw > self.raw_capacity { raw + raw / 2 } else { raw };
        let growing = FIXED_CODEC_BYTES + growth + self.exif_bytes + self.text_backing()? + vector_bytes(prior);
        if growing > super::MAX_CODEC_BYTES {
            return Err(invalid("PNG metadata exceeds the existing 64 MiB codec allowance"));
        }
        self.raw_capacity = raw;
        if self.kind == *b"eXIf" {
            self.exif_bytes = length as u64;
        }
        let text = [*b"tEXt", *b"zTXt", *b"iTXt"].iter().position(|kind| *kind == self.kind);
        if let Some(index) = text {
            self.text_counts[index] += 1;
            self.text_payload += length as u64;
        }
        let current = self.text_capacities()?;
        let old_vectors = prior.into_iter().zip(current).zip(sizes).filter_map(|((old, new), size)| (new > old).then_some(old * size)).sum::<u64>();
        // Latin1 tEXt collect reserves its input character count, then may
        // grow while emitting two UTF8 bytes per character. Keep the old
        // current-record backing too; final String capacity alone is not peak.
        // zTXt/iTXt only use Latin1 for their bounded keyword; their payload
        // is copied with exact capacity or retained compressed, never inflated.
        let string_growth = match text {
            Some(0) => length as u64 + 256,
            Some(_) => 256,
            None => 0,
        };
        let parsed = FIXED_CODEC_BYTES + raw + self.exif_bytes + self.text_backing()? + vector_bytes(current) + old_vectors + string_growth;
        // image's orientation getter owns another EXIF copy after read_info.
        let getters = FIXED_CODEC_BYTES + raw + 2 * self.exif_bytes + self.text_backing()? + vector_bytes(current);
        if parsed.max(getters) > super::MAX_CODEC_BYTES {
            return Err(invalid("PNG metadata exceeds the existing 64 MiB codec allowance"));
        }
        Ok(())
    }
    /// One header, at most4096 CRC bytes, EXIF header, or one IFD entry.
    /// The owner debits and checks cancellation before each call.
    pub(crate) fn step(&mut self, bytes: &[u8]) -> Result<bool> {
        match self.phase {
            Phase::Done => return Ok(true),
            Phase::Header => {
                if bytes.len() > super::MAX_TEXTURE_BYTES {
                    return Err(invalid("texture input must contain at most 16 MiB"));
                }
                if bytes.get(..8) != Some(SIGNATURE.as_slice()) {
                    return Err(invalid("PNG signature missing"));
                }
                let length = be32(bytes, self.position)? as usize;
                let type_start = self.position.checked_add(4).ok_or_else(|| invalid("PNG chunk offset overflow"))?;
                self.data_start = type_start.checked_add(4).ok_or_else(|| invalid("PNG chunk offset overflow"))?;
                self.kind = bytes
                    .get(type_start..self.data_start)
                    .ok_or_else(|| invalid("PNG chunk type missing"))?
                    .try_into()
                    .map_err(|_| invalid("PNG chunk type"))?;
                self.data_end = self.data_start.checked_add(length).ok_or_else(|| invalid("PNG chunk size overflow"))?;
                self.chunk_end = self.data_end.checked_add(4).filter(|end| *end <= bytes.len()).ok_or_else(|| invalid("PNG chunk exceeds source"))?;
                if !self.kind.iter().all(u8::is_ascii_alphabetic) || !self.kind[2].is_ascii_uppercase() {
                    return Err(invalid("PNG chunk type is invalid"));
                }
                if (self.position == 8 && (self.kind != *b"IHDR" || length != 13)) || (self.position != 8 && self.kind == *b"IHDR") {
                    return Err(invalid("PNG requires one first IHDR"));
                }
                if self.srgb {
                    let data = bytes.get(self.data_start..self.data_end).ok_or_else(|| invalid("PNG color metadata missing"))?;
                    validate_srgb_chunk(&self.kind, data, &mut self.color_seen)?;
                }
                // Refuse presence, not just a successfully decoded ICC profile. png can
                // ignore an inflate error after a large temporary allocation.
                if self.kind == *b"iCCP" {
                    return Err(invalid("embedded ICC profiles need explicit color conversion before admission"));
                }
                if self.cpu && [*b"acTL", *b"fcTL", *b"fdAT"].contains(&self.kind) {
                    return Err(invalid("CPU row preparation requires a static PNG"));
                }
                if self.kind[0].is_ascii_uppercase() && ![*b"IHDR", *b"PLTE", *b"IDAT", *b"IEND"].contains(&self.kind) {
                    return Err(invalid("PNG has an unknown critical chunk"));
                }
                if !self.data_seen && self.kind != *b"IDAT" {
                    self.codec_storage(length)?;
                }
                if self.kind == *b"IDAT" {
                    if self.data_ended {
                        return Err(invalid("PNG IDAT chunks must be consecutive"));
                    }
                    self.data_seen = true;
                } else if self.data_seen {
                    self.data_ended = true;
                    if BEFORE_DATA.contains(&self.kind) {
                        return Err(invalid("PNG metadata must precede IDAT"));
                    }
                }
                if self.kind == *b"PLTE" {
                    if self.palette_seen || length == 0 || length > 768 || !length.is_multiple_of(3) {
                        return Err(invalid("PNG palette is invalid"));
                    }
                    self.palette_seen = true;
                }
                if self.kind == *b"eXIf" {
                    if self.exif_seen {
                        return Err(invalid("PNG has duplicate EXIF metadata"));
                    }
                    self.exif_seen = true;
                }
                if self.kind == *b"tIME" {
                    if self.time_seen || length != 7 {
                        return Err(invalid("PNG time metadata is invalid"));
                    }
                    self.time_seen = true;
                }
                if self.kind == *b"IEND" && (length != 0 || self.chunk_end != bytes.len() || !self.data_seen) {
                    return Err(invalid("PNG requires one final IEND after image data"));
                }
                self.phase = Phase::Bytes { next: type_start, crc: u32::MAX };
            }
            Phase::Bytes { next, mut crc } => {
                let end = if self.verify_crc { next.saturating_add(BLOCK_BYTES).min(self.data_end) } else { self.data_end };
                if self.verify_crc {
                    for byte in bytes.get(next..end).ok_or_else(|| invalid("PNG CRC range"))? {
                        crc = (crc >> 8) ^ CRC_TABLE[((crc ^ u32::from(*byte)) & 255) as usize];
                    }
                }
                if end != self.data_end {
                    self.phase = Phase::Bytes { next: end, crc };
                } else {
                    if self.verify_crc && !crc != be32(bytes, self.data_end)? {
                        return Err(invalid("PNG chunk CRC is invalid"));
                    }
                    self.position = self.chunk_end;
                    self.phase = if self.kind == *b"IEND" {
                        Phase::Done
                    } else if self.kind == *b"eXIf" {
                        Phase::ExifHeader
                    } else {
                        Phase::Header
                    };
                }
            }
            Phase::ExifHeader => {
                let exif = bytes.get(self.data_start..self.data_end).ok_or_else(|| invalid("PNG EXIF range"))?;
                let little = match exif.get(..2) {
                    Some(b"II") => true,
                    Some(b"MM") => false,
                    _ => return Err(invalid("PNG EXIF byte order is invalid")),
                };
                if word(exif, 2, 2, little)? != 42 {
                    return Err(invalid("PNG EXIF TIFF header is invalid"));
                }
                let start = word(exif, 4, 4, little)? as usize;
                if start < 8 {
                    return Err(invalid("PNG EXIF IFD offset is invalid"));
                }
                let count = word(exif, start, 2, little)? as u16;
                let next = start.checked_add(2).ok_or_else(|| invalid("PNG EXIF IFD overflow"))?;
                let end = usize::from(count)
                    .checked_mul(12)
                    .and_then(|n| next.checked_add(n))
                    .and_then(|n| n.checked_add(4))
                    .ok_or_else(|| invalid("PNG EXIF IFD overflow"))?;
                if end > exif.len() {
                    return Err(invalid("PNG EXIF IFD is truncated"));
                }
                self.phase = Phase::ExifEntry { next, remaining: count, little, orientation_seen: false };
            }
            Phase::ExifEntry { next, remaining, little, mut orientation_seen } => {
                if remaining == 0 {
                    self.phase = Phase::Header;
                    return Ok(false);
                }
                let exif = bytes.get(self.data_start..self.data_end).ok_or_else(|| invalid("PNG EXIF range"))?;
                if word(exif, next, 2, little)? == 0x112 {
                    if orientation_seen || word(exif, next + 2, 2, little)? != 3 || word(exif, next + 4, 4, little)? != 1 {
                        return Err(invalid("PNG EXIF orientation tag is invalid"));
                    }
                    if word(exif, next + 8, 2, little)? != 1 {
                        return Err(invalid("texture orientation must be normalized before admission"));
                    }
                    orientation_seen = true;
                }
                self.phase = Phase::ExifEntry { next: next + 12, remaining: remaining - 1, little, orientation_seen };
            }
        }
        Ok(matches!(self.phase, Phase::Done))
    }
    #[cfg(test)]
    pub(crate) fn phase_name(&self) -> &'static str {
        match self.phase {
            Phase::Header => "metadata_header",
            Phase::Bytes { .. } => "metadata_bytes",
            Phase::ExifHeader => "metadata_exif_header",
            Phase::ExifEntry { .. } => "metadata_ifd",
            Phase::Done => "metadata_done",
        }
    }
}
/// Shared capture policy: check unsupported metadata before a codec can allocate.
/// Pixel/CRC decode remains a distinct later operation, as before.
pub(crate) fn validate(bytes: &[u8]) -> Result<()> {
    let mut scan = PngMetadata::new(false, false);
    while !scan.step(bytes)? {}
    Ok(())
}

/// Role-aware encoded declaration check, separate from the common data/CRC policy.
/// Untagged bytes are explicitly interpreted as sRGB by the caller's base-color role.
pub(crate) fn validate_srgb_declarations(bytes: &[u8]) -> Result<()> {
    let mut offset = 8usize;
    let mut seen = 0u8;
    while offset < bytes.len() {
        let length = be32(bytes, offset)? as usize;
        let kind_start = offset.checked_add(4).ok_or_else(|| invalid("PNG color metadata offset overflow"))?;
        let start = kind_start.checked_add(4).ok_or_else(|| invalid("PNG color metadata offset overflow"))?;
        let end = start.checked_add(length).filter(|end| *end <= bytes.len()).ok_or_else(|| invalid("PNG color metadata exceeds source"))?;
        let kind = bytes.get(kind_start..start).ok_or_else(|| invalid("PNG color metadata kind missing"))?;
        let data = bytes.get(start..end).ok_or_else(|| invalid("PNG color metadata missing"))?;
        validate_srgb_chunk(kind, data, &mut seen)?;
        offset = end.checked_add(4).filter(|end| *end <= bytes.len()).ok_or_else(|| invalid("PNG color metadata trailer missing"))?;
    }
    Ok(())
}

fn validate_srgb_chunk(kind: &[u8], data: &[u8], seen: &mut u8) -> Result<()> {
    let (bit, valid) = match kind {
        b"gAMA" => (1, data.len() == 4 && be32(data, 0)? == 45_455),
        b"cHRM" => {
            let standard = [31_270, 32_900, 64_000, 33_000, 30_000, 60_000, 15_000, 6_000];
            let valid = data.len() == 32 && standard.into_iter().enumerate().all(|(i, value)| be32(data, i * 4).is_ok_and(|actual| actual == value));
            (2, valid)
        }
        b"sRGB" => (4, data.len() == 1 && data.first().is_some_and(|intent| *intent <= 3)),
        // H.273: BT.709 primaries, IEC61966-2-1 transfer, RGB identity matrix,
        // full range. Other transfer/primaries require an explicit conversion.
        b"cICP" => (8, data == [1, 13, 0, 1]),
        b"mDCV" | b"cLLI" => (16, false),
        _ => (0, true),
    };
    if !valid || (bit != 0 && *seen & bit != 0) {
        return Err(invalid(
            "base-color PNG requires normalized sRGB gAMA/cHRM/cICP declarations without conflicting, duplicate or HDR metadata; normalize the image before import",
        ));
    }
    *seen |= bit;
    Ok(())
}
