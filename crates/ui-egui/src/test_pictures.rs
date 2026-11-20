//! Pictures for tests that decode a file (#19138).

/// A JPEG of `width` x `height` pixels, red on its left half and blue on its right half, with an
/// EXIF orientation of `orientation` (1 to 8) when one is given: an APP1 segment right after the start
/// marker, "Exif\0\0", then a big-endian TIFF with the one tag 0x0112.
pub(crate) fn jpeg(width: u32, height: u32, orientation: Option<u8>) -> Vec<u8> {
    let rgb = image::RgbImage::from_fn(width, height, |x, _| if x < width / 2 { image::Rgb([255, 0, 0]) } else { image::Rgb([0, 0, 255]) });
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95).encode_image(&rgb).expect("a JPEG of a few pixels encodes");
    let Some(orientation) = orientation else { return jpeg };
    let tiff = [b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, orientation, 0, 0, 0, 0, 0, 0];
    let length = u16::try_from(2 + 6 + tiff.len()).expect("a short segment");
    let mut tagged = vec![0xFF, 0xD8, 0xFF, 0xE1];
    tagged.extend(length.to_be_bytes());
    tagged.extend(b"Exif\0\0");
    tagged.extend(tiff);
    tagged.extend(&jpeg[2..]);
    tagged
}
