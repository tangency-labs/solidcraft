//! A picture decoded the way every viewer shows it: turned upright by its EXIF orientation.
//!
//! Phones and cameras often save a portrait photo as landscape pixels plus an orientation tag
//! ("turn it 90° to show it"). The `image` crate decodes the pixels as stored and leaves the tag
//! alone, so a photo decoded with `decode` lies on its side (#19138). Everything that reads a
//! picture file to show it or to measure it goes through [`decode_upright`].

use std::io::{BufRead, Seek};

use image::{DynamicImage, ImageDecoder, ImageReader, ImageResult};

/// The picture in `reader`, decoded under the reader's limits and turned upright by its EXIF
/// orientation, and whether it was turned: when it was, the file's own bytes still carry the tag,
/// so a copy of them would show the picture on its side to a reader that ignores it.
pub fn decode_upright<R: BufRead + Seek>(reader: ImageReader<R>) -> ImageResult<(DynamicImage, bool)> {
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut picture = DynamicImage::from_decoder(decoder)?;
    let turned = orientation != image::metadata::Orientation::NoTransforms;
    picture.apply_orientation(orientation);
    Ok((picture, turned))
}

/// `jpeg` with an EXIF orientation of `orientation` (1 to 8) put right after its start marker:
/// an APP1 segment, "Exif\0\0", then a big-endian TIFF with the one tag 0x0112. For tests.
#[cfg(test)]
pub(crate) fn tagged(jpeg: &[u8], orientation: u8) -> Vec<u8> {
    let tiff = [b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, orientation, 0, 0, 0, 0, 0, 0];
    let length = u16::try_from(2 + 6 + tiff.len()).expect("a short segment");
    let mut with_exif = vec![0xFF, 0xD8, 0xFF, 0xE1];
    with_exif.extend(length.to_be_bytes());
    with_exif.extend(b"Exif\0\0");
    with_exif.extend(tiff);
    with_exif.extend(&jpeg[2..]);
    with_exif
}

/// A JPEG of `width` x `height` pixels, red on its left half and blue on its right half.
#[cfg(test)]
fn test_jpeg(width: u32, height: u32, orientation: Option<u8>) -> Vec<u8> {
    let rgb = image::RgbImage::from_fn(width, height, |x, _| if x < width / 2 { image::Rgb([255, 0, 0]) } else { image::Rgb([0, 0, 255]) });
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95).encode_image(&rgb).expect("a JPEG of a few pixels encodes");
    orientation.map_or(jpeg.clone(), |o| tagged(&jpeg, o))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
