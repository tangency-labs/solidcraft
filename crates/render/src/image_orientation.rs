//! What a PNG or JPEG file's header says (#19106): its kind, its pixels as the file stores them, and the
//! EXIF orientation every viewer turns them by. One reader, so a picture's size and its decoded pixels
//! always agree.
//!
//! - No pixels are decoded. The walk reads a JPEG's segments up to its scan; a PNG's size is its IHDR's
//!   and its chunks up to its pixel data are read by the PNG decoder's own `read_info`, so its CRC checks
//!   are the decoder's. Input is hostile, so every read is checked.
//! - The orientation is the EXIF block's: a JPEG's APP1 segment that starts `Exif\0\0` (of two, the last
//!   before its scan, as the decoder keeps it), or a PNG's `eXIf` chunk, which comes before IDAT (PNG 3rd
//!   edition, table 7; with a bad CRC it is skipped, and of two the first counts, as the decoder does).
//!   The `image` crate reads its IFD0. None, a damaged one or a value outside 1 to 8 is 1: the picture
//!   shows as stored, as a browser shows it (CSS Images 3, `image-orientation: from-image`).
//! - Decoded pixels turn in one place, [`turn`]; a file's own tag is read through its decoder in one
//!   place, [`upright`]. Phones and cameras often save a portrait photo as landscape pixels plus an
//!   orientation tag, and the `image` crate decodes the pixels as stored (#19138): everything that
//!   reads a picture file to show it or to measure it goes through [`upright`] or [`decode_upright`].

use std::io::{BufRead, Seek};

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader, ImageResult};

/// A PNG or JPEG file's header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// `png` or `jpeg`.
    pub format: &'static str,
    /// Its width and height in pixels as the file stores them.
    pub stored: [u32; 2],
    /// Its EXIF orientation, 1 to 8.
    pub orientation: u8,
}

/// The header of the PNG or JPEG file `b`; `None` for anything else or a picture with no pixels.
pub fn header(b: &[u8]) -> Option<Header> {
    if b.len() > 24 && b.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes([*b.get(16)?, *b.get(17)?, *b.get(18)?, *b.get(19)?]);
        let h = u32::from_be_bytes([*b.get(20)?, *b.get(21)?, *b.get(22)?, *b.get(23)?]);
        return (w > 0 && h > 0).then(|| Header { format: "png", stored: [w, h], orientation: png_orientation(b) });
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        return jpeg(b);
    }
    None
}

/// `stored` as it shows after `orientation`: its sides swap for the quarter turns, 5 to 8.
pub fn shown(stored: [u32; 2], orientation: u8) -> [u32; 2] {
    use Orientation::{Rotate90, Rotate90FlipH, Rotate270, Rotate270FlipH};
    let [w, h] = stored;
    if matches!(Orientation::from_exif(orientation), Some(Rotate90 | Rotate270 | Rotate90FlipH | Rotate270FlipH)) { [h, w] } else { [w, h] }
}

/// Decoded pixels turned and mirrored as `orientation` says; a value outside 1 to 8 leaves them.
pub fn turn(image: &mut DynamicImage, orientation: u8) {
    if let Some(orientation) = Orientation::from_exif(orientation) {
        image.apply_orientation(orientation);
    }
}

/// The picture `decoder` holds, decoded and turned upright by the EXIF orientation it reads, and
/// whether it was turned: when it was, the file's own bytes still carry the tag, so a copy of them
/// would show the picture on its side to a reader that ignores it.
pub fn upright(mut decoder: impl ImageDecoder) -> ImageResult<(DynamicImage, bool)> {
    let orientation = decoder.orientation()?.to_exif();
    let mut picture = DynamicImage::from_decoder(decoder)?;
    turn(&mut picture, orientation);
    Ok((picture, orientation != 1))
}

/// The picture in `reader`, decoded under the reader's limits and turned upright ([`upright`]).
pub fn decode_upright<R: BufRead + Seek>(reader: ImageReader<R>) -> ImageResult<(DynamicImage, bool)> {
    upright(reader.into_decoder()?)
}

/// The orientation an EXIF block says, 1 when there is none or it says none.
fn orientation(exif: Option<&[u8]>) -> u8 {
    exif.and_then(Orientation::from_exif_chunk).map_or(1, Orientation::to_exif)
}

/// A PNG's orientation as its decoder reads it. The PNG decoder's own `read_info` reads the chunks up to
/// the first IDAT by its rules: an `eXIf` with a bad CRC is skipped, and of two the first counts. A file
/// it refuses shows as stored, and its decoder refuses it too. No pixels are decoded.
fn png_orientation(b: &[u8]) -> u8 {
    let Ok(reader) = png::Decoder::new(std::io::Cursor::new(b)).read_info() else { return 1 };
    orientation(reader.info().exif_metadata.as_deref())
}

/// A JPEG's header: the size of its first frame, and the last EXIF block before its scan.
fn jpeg(b: &[u8]) -> Option<Header> {
    let (mut stored, mut exif, mut scanned) = (None, None, false);
    // Walk the markers: the frame's size as `canvas::image_info` always read it, then on to the scan.
    let mut i = 2;
    while i + 9 < b.len() {
        if *b.get(i)? != 0xFF {
            i += 1;
            continue;
        }
        let m = *b.get(i + 1)?;
        if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) || m == 0xFF {
            i += if m == 0xFF { 1 } else { 2 };
            continue;
        }
        let len = usize::from(u16::from_be_bytes([*b.get(i + 2)?, *b.get(i + 3)?]));
        if stored.is_none() && matches!(m, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            let h = u32::from(u16::from_be_bytes([*b.get(i + 5)?, *b.get(i + 6)?]));
            let w = u32::from(u16::from_be_bytes([*b.get(i + 7)?, *b.get(i + 8)?]));
            if w == 0 || h == 0 {
                return None;
            }
            stored = Some([w, h]);
        } else if m == 0xE1
            && !scanned
            && let Some(data) = b.get(i + 4..i + 2 + len).and_then(|data| data.strip_prefix(b"Exif\0\0"))
        {
            // Of two, the last counts, as the decoder keeps it; another APP1 (XMP) is no EXIF block.
            exif = Some(data);
        }
        if matches!(m, 0xDA | 0xD9) {
            // Whatever follows the scan is no header.
            scanned = true;
            if stored.is_some() {
                break;
            }
        }
        i += 2 + len;
    }
    Some(Header { format: "jpeg", stored: stored?, orientation: orientation(exif) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sheet_image::tests::{app1, photo, tiff, with_segment};

    /// A PNG of 32 by 16 pixels, with `exif` as an `eXIf` chunk, before or after its pixels.
    fn png(exif: Option<&[u8]>, after: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, 32, 16);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let chunk = |writer: &mut png::Writer<&mut Vec<u8>>| {
            if let Some(exif) = exif {
                writer.write_chunk(png::chunk::ChunkType(*b"eXIf"), exif).unwrap();
            }
        };
        if !after {
            chunk(&mut writer);
        }
        writer.write_image_data(&[90; 32 * 16 * 3]).unwrap();
        if after {
            chunk(&mut writer);
        }
        writer.finish().unwrap();
        out
    }

    /// Where a [`png`]'s first `eXIf` chunk sits, whole: its length, kind, data and CRC.
    fn exif_chunk(file: &[u8]) -> std::ops::Range<usize> {
        let at = file.windows(4).position(|w| w == b"eXIf").unwrap() - 4;
        let length = u32::from_be_bytes(file[at..at + 4].try_into().unwrap()) as usize;
        at..at + 12 + length
    }

    fn read(b: &[u8]) -> Option<(&'static str, [u32; 2], u8)> {
        header(b).map(|h| (h.format, h.stored, h.orientation))
    }

    /// A JPEG of `width` x `height` pixels, red on its left half and blue on its right half.
    fn test_jpeg(width: u32, height: u32, orientation: Option<u8>) -> Vec<u8> {
        let rgb = image::RgbImage::from_fn(width, height, |x, _| if x < width / 2 { image::Rgb([255, 0, 0]) } else { image::Rgb([0, 0, 255]) });
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95).encode_image(&rgb).expect("a JPEG of a few pixels encodes");
        orientation.map_or(jpeg.clone(), |o| with_segment(&jpeg, 0xE1, &app1(u16::from(o))))
    }

    fn decoded(bytes: &[u8]) -> (DynamicImage, bool) {
        decode_upright(ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().unwrap()).unwrap()
    }

    /// An orientation of 6 stands a 60 x 40 file up as 40 x 60; a file with none is as stored.
    #[test]
    fn an_exif_orientation_turns_the_picture_upright() {
        let (plain, turned) = decoded(&test_jpeg(60, 40, None));
        assert_eq!((plain.width(), plain.height(), turned), (60, 40, false));
        let (upright, turned) = decoded(&test_jpeg(60, 40, Some(6)));
        assert_eq!((upright.width(), upright.height(), turned), (40, 60, true));
        // Orientation 1 is "as stored": nothing turns.
        let (same, turned) = decoded(&test_jpeg(60, 40, Some(1)));
        assert_eq!((same.width(), same.height(), turned), (60, 40, false));
    }

    /// The header's orientation and the decoder's are one: for JPEGs and PNGs with no EXIF block, one,
    /// two that disagree, a damaged one, a PNG eXIf with a broken CRC, and values 0 to 9, the size the header gives is the size
    /// `decode_upright` turns the pixels to, and they turn exactly when the header names a transform.
    #[test]
    fn the_header_and_the_decoder_agree_on_a_files_orientation() {
        let mut files = vec![photo(None), png(None, false)];
        for value in 0..=9u16 {
            let exif = tiff(&[[0x0112, 3, value]]);
            files.push(photo(Some(exif.clone())));
            files.push(png(Some(&exif), false));
            for other in [1, 6] {
                files.push(with_segment(&photo(Some(tiff(&[[0x0112, 3, other]]))), 0xE1, &app1(value)));
            }
        }
        let damaged = [tiff(&[[0x0112, 3, 6]])[..12].to_vec(), b"MM\0\x2a\xff\xff\xff\xf0".to_vec(), tiff(&[[0x0112, 4, 6]])];
        for exif in damaged {
            files.push(photo(Some(exif.clone())));
            files.push(png(Some(&exif), false));
            files.push(with_segment(&photo(Some(tiff(&[[0x0112, 3, 6]]))), 0xE1, &[b"Exif\0\0".as_slice(), &exif].concat()));
        }
        // A PNG's eXIf as its decoder reads it: one whose CRC is broken is skipped, and of two the first counts.
        let six = png(Some(&tiff(&[[0x0112, 3, 6]])), false);
        let mut broken = six.clone();
        broken[exif_chunk(&six).end - 1] ^= 0xFF;
        files.push(broken);
        let eight = png(Some(&tiff(&[[0x0112, 3, 8]])), false);
        let end = exif_chunk(&six).end;
        files.push([&six[..end], &eight[exif_chunk(&eight)], &six[end..]].concat());
        for (case, file) in files.iter().enumerate() {
            let h = header(file).unwrap();
            let (picture, turned) = decoded(file);
            assert_eq!([picture.width(), picture.height()], shown(h.stored, h.orientation), "case {case}: {h:?}");
            let transforms = Orientation::from_exif(h.orientation).is_some_and(|o| o != Orientation::NoTransforms);
            assert_eq!(turned, transforms, "case {case}: {h:?}");
        }
    }

    /// Each orientation 1 to 8 reads from a JPEG's APP1 in either byte order and from a PNG's eXIf, and
    /// the sides swap for 5 to 8 only. A header with no EXIF is 1, a file of neither kind is nothing.
    #[test]
    fn a_files_header_gives_its_exif_orientation() {
        for value in 1..=8u16 {
            let o = value as u8;
            assert_eq!(read(&photo(Some(tiff(&[[0x0112, 3, value]])))), Some(("jpeg", [32, 16], o)), "{value}");
            let mut little = b"II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0".to_vec();
            little.extend([value.to_le_bytes(), [0, 0], [0, 0], [0, 0]].concat());
            assert_eq!(read(&photo(Some(little))), Some(("jpeg", [32, 16], o)), "{value} little-endian");
            assert_eq!(read(&png(Some(&tiff(&[[0x0112, 3, value]])), false)), Some(("png", [32, 16], o)), "{value}");
            assert_eq!(shown([32, 16], o), if value >= 5 { [16, 32] } else { [32, 16] }, "{value}");
        }
        assert_eq!(read(&photo(None)), Some(("jpeg", [32, 16], 1)));
        assert_eq!(read(&png(None, false)), Some(("png", [32, 16], 1)));
        // Headers alone, as the size checks read them.
        let mut png_header = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png_header.extend([640u32.to_be_bytes(), 480u32.to_be_bytes()].concat());
        png_header.extend([8, 6, 0, 0, 0]);
        assert_eq!(read(&png_header), Some(("png", [640, 480], 1)));
        let jpeg_header = [0xFF, 0xD8, 0xFF, 0xC0, 0, 17, 8, 1, 144, 2, 88, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(read(&jpeg_header), Some(("jpeg", [600, 400], 1)));
        assert_eq!(read(&with_segment(&jpeg_header, 0xE1, &app1(6))), Some(("jpeg", [600, 400], 6)));
        for neither in [&b""[..], b"GIF89a", b"\x89PNG", &[0xFF, 0xD8, 0xFF, 0xD9]] {
            assert_eq!(read(neither), None);
        }
        assert_eq!((shown([3, 2], 0), shown([3, 2], 9), shown([3, 2], 255)), ([3, 2], [3, 2], [3, 2]));
    }

    /// A damaged, late or second EXIF block never turns a picture the size doesn't, and no input panics:
    /// a bad value, type or count, a truncated block, an IFD past its end, an eXIf after the pixels, an
    /// APP1 after the scan; of two, the last counts. Mutated files give 1 to 8 or nothing.
    #[test]
    fn damaged_duplicate_or_late_exif_reads_as_stored() {
        let orientation = |b: &[u8]| header(b).map(|h| h.orientation);
        for exif in [
            tiff(&[[0x0112, 3, 0]]),
            tiff(&[[0x0112, 3, 9]]),
            tiff(&[[0x0112, 3, 0xFFFF]]),
            tiff(&[[0x0112, 4, 6]]),
            tiff(&[[0x0110, 3, 6]]),
            tiff(&[[0x0112, 3, 6]])[..12].to_vec(),
            b"MM\0\x2a\xff\xff\xff\xf0".to_vec(),
            b"XX\0\x2a\0\0\0\x08".to_vec(),
            Vec::new(),
        ] {
            assert_eq!(orientation(&photo(Some(exif.clone()))), Some(1), "{exif:?}");
            assert_eq!(orientation(&png(Some(&exif), false)), Some(1), "{exif:?}");
        }
        let mut count = tiff(&[[0x0112, 3, 6]]);
        count[17] = 2;
        assert_eq!(orientation(&photo(Some(count))), Some(1), "two values");
        assert_eq!(orientation(&png(Some(&tiff(&[[0x0112, 3, 6]])), true)), Some(1), "after IDAT");
        let plain = photo(None);
        let late = [&plain[..plain.len() - 2], &[0xFF, 0xE1, 0, 2 + 32], &app1(6)[..], &[0xFF, 0xD9]].concat();
        assert_eq!(orientation(&late), Some(1), "after the scan");
        assert_eq!(orientation(&with_segment(&photo(Some(tiff(&[[0x0112, 3, 1]]))), 0xE1, &app1(6))), Some(1), "the last of two");
        assert_eq!(orientation(&with_segment(&photo(Some(tiff(&[[0x0112, 3, 6]]))), 0xE1, b"http://ns.adobe.com/xap/1.0/\0")), Some(6));
        let mut fill = vec![0xFF; 16 << 20];
        fill[1] = 0xD8;
        assert_eq!(header(&fill), None);

        let mut seed = 0x0001_9106_u64;
        for (case, original) in
            [photo(Some(tiff(&[[0x0112, 3, 6]]))), png(Some(&tiff(&[[0x0112, 3, 6]])), false)].iter().cycle().take(4000).enumerate()
        {
            let mut bytes = original.clone();
            for _ in 0..1 + case % 4 {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                let at = (seed >> 33) as usize % bytes.len().min(160);
                bytes[at] = (seed >> 13) as u8;
            }
            if let Some(h) = header(&bytes) {
                assert!((1..=8).contains(&h.orientation), "{h:?}");
            }
        }
    }
}
