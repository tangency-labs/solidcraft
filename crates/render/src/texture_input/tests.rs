use super::*;

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([64, 128, 192, 255]));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}
#[test]
fn texture_input_preserves_channels_and_explicit_map_interpretation() {
    for role in [MapRole::BaseColor, MapRole::NormalOpenGl, MapRole::Roughness, MapRole::Metallic, MapRole::AmbientOcclusion] {
        let source = TextureSource::new(png(3, 2), role).unwrap();
        let decoded = source.decode(|| false).unwrap();
        assert_eq!((decoded.info().width, decoded.info().height, decoded.info().rgba_bytes), (3, 2, 24));
        assert_eq!(decoded.info().interpretation, if role == MapRole::BaseColor { Interpretation::Srgb } else { Interpretation::Data });
        assert!(decoded.rgba().as_chunks::<4>().0.iter().all(|p| *p == [64, 128, 192, 255]));
    }
}
#[test]
fn texture_input_rejects_dimensions_formats_and_corrupt_pixels() {
    assert!(TextureSource::new(png(MAX_TEXTURE_EDGE + 1, 1), MapRole::BaseColor).is_err());
    assert!(TextureSource::new(Vec::new(), MapRole::BaseColor).is_err());
    assert!(TextureSource::new(b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y 1 +X 2\nabcdefgh".to_vec(), MapRole::BaseColor).is_err());
    let bytes = png(3, 2);
    for n in 0..bytes.len() {
        if let Ok(source) = TextureSource::new(bytes.get(..n).unwrap().to_vec(), MapRole::BaseColor) {
            assert!(source.decode(|| false).is_err(), "truncated PNG accepted at {n}");
        }
    }
}
#[test]
fn texture_input_cancellation_never_publishes_partial_pixels() {
    let source = TextureSource::new(png(3, 2), MapRole::NormalOpenGl).unwrap();
    assert!(source.decode(|| true).is_err());
    let calls = std::cell::Cell::new(0);
    assert!(
        source
            .decode(|| {
                calls.set(calls.get() + 1);
                calls.get() == 2
            })
            .is_err()
    );
    assert!(source.clone().decode(|| false).is_ok());
}
#[test]
fn texture_input_jpeg_is_supported_without_hdr_feature_confusion() {
    let image = image::RgbImage::from_pixel(2, 2, image::Rgb([64, 128, 192]));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, ImageFormat::Jpeg).unwrap();
    let source = TextureSource::new(out.into_inner(), MapRole::BaseColor).unwrap();
    assert_eq!(source.decode(|| false).unwrap().rgba().len(), 16);
}

#[test]
fn texture_input_caps_encoded_storage_and_refuses_precision_loss() {
    let mut bytes = Vec::with_capacity(MAX_TEXTURE_BYTES + 1);
    bytes.extend(png(1, 1));
    assert!(TextureSource::new(bytes, MapRole::Roughness).is_err());
    assert!(TextureSource::new(vec![0; MAX_TEXTURE_BYTES + 1], MapRole::BaseColor).is_err());
    let image = image::DynamicImage::ImageLuma16(image::ImageBuffer::from_pixel(1, 1, image::Luma([32768u16])));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, ImageFormat::Png).unwrap();
    assert!(TextureSource::new(out.into_inner(), MapRole::NormalOpenGl).is_err());
}

fn metadata_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    // Independent bitwise IEEE CRC; production uses its fixed lookup table.
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend(kind);
    out.extend(data);
    let mut crc = u32::MAX;
    for byte in kind.iter().chain(data) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 0 { crc >> 1 } else { (crc >> 1) ^ 0xedb88320 };
        }
    }
    out.extend((!crc).to_be_bytes());
    out
}
fn normalized_exif(entries: u16, orientation: u16) -> Vec<u8> {
    let mut data = b"II".to_vec();
    data.extend(42_u16.to_le_bytes());
    data.extend(8_u32.to_le_bytes());
    data.extend(entries.to_le_bytes());
    for i in 0..entries {
        data.extend((if i == 0 { 0x112_u16 } else { 0xc000 + i }).to_le_bytes());
        data.extend(3_u16.to_le_bytes());
        data.extend(1_u32.to_le_bytes());
        data.extend((if i == 0 { orientation } else { i }).to_le_bytes());
        data.extend(0_u16.to_le_bytes());
    }
    data.extend(0_u32.to_le_bytes());
    data
}
fn insert_metadata(mut image: Vec<u8>, kind: &[u8; 4], data: &[u8], before_data: bool) -> Vec<u8> {
    let at = if before_data { 33 } else { image.len() - 12 };
    image.splice(at..at, metadata_chunk(kind, data));
    image
}
#[test]
fn png_metadata_policy_refuses_icc_and_rotated_or_misordered_exif_before_any_codec() {
    let plain = png(2, 1);
    let expected = TextureSource::new(plain.clone(), MapRole::BaseColor).unwrap().decode(|| false).unwrap();
    let normalized = insert_metadata(plain.clone(), b"eXIf", &normalized_exif(8, 1), true);
    let decoded = TextureSource::new(normalized.clone(), MapRole::BaseColor).unwrap().decode(|| false).unwrap();
    assert_eq!(decoded.rgba(), expected.rgba());
    assert_eq!(normalized.as_slice(), TextureSource::new(normalized.clone(), MapRole::BaseColor).unwrap().bytes.as_ref().as_slice());
    for (kind, data, before, reason) in [
        (*b"eXIf", normalized_exif(1, 6), true, "orientation"),
        (*b"eXIf", normalized_exif(1, 6), false, "precede IDAT"),
        (*b"eXIf", normalized_exif(1, 1), false, "precede IDAT"),
        (*b"iCCP", b"profile\0\0broken".to_vec(), true, "ICC"),
        (*b"iCCP", b"profile\0\0broken".to_vec(), false, "ICC"),
    ] {
        let bytes = insert_metadata(plain.clone(), &kind, &data, before);
        assert!(png_metadata::validate(&bytes).unwrap_err().to_string().contains(reason));
        assert!(TextureSource::new(bytes, MapRole::BaseColor).err().unwrap().to_string().contains(reason));
    }
}
#[test]
fn png_metadata_policy_rejects_a_small_icc_container_before_its_large_partial_inflate() {
    use std::io::Write;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    for _ in 0..10_240 {
        encoder.write_all(&[0; 4096]).unwrap();
    }
    let mut compressed = encoder.finish().unwrap();
    compressed.truncate(compressed.len() - 6);
    let mut icc = b"profile\0\0".to_vec();
    icc.extend(compressed);
    assert!(icc.len() < 128 * 1024, "the hostile source remains far below the existing16MiB cap");
    let bytes = insert_metadata(png(1, 1), b"iCCP", &icc, true);
    // The pure policy never constructs a decoder; missing/truncated compressed
    // contents cannot turn unsupported ICC into a None profile after inflation.
    assert!(png_metadata::validate(&bytes).unwrap_err().to_string().contains("ICC"));
    assert!(TextureSource::new(bytes, MapRole::Roughness).err().unwrap().to_string().contains("ICC"));
}
#[test]
fn png_metadata_cooperative_checks_cover_crc_ifd_tail_order_and_static_policy() {
    let source = insert_metadata(png(2, 1), b"eXIf", &normalized_exif(32, 1), true);
    let mut scanner = png_metadata::PngMetadata::new(true, true);
    let mut entries = 0;
    let mut done = false;
    for _ in 0..1000 {
        if scanner.phase_name() == "metadata_ifd" {
            entries += 1;
        }
        if scanner.step(&source).unwrap() {
            done = true;
            break;
        }
    }
    assert!(done);
    assert_eq!(entries, 33, "32 exact IFD entries plus their bounded final transition");
    let mut corrupt = source.clone();
    let idat = corrupt.windows(4).position(|kind| kind == b"IDAT").unwrap();
    corrupt[idat + 4] ^= 0xff;
    let mut scanner = png_metadata::PngMetadata::new(true, true);
    assert!((0..1000).find_map(|_| scanner.step(&corrupt).err()).unwrap().to_string().contains("CRC"));
    for kind in [*b"IHDR", *b"PLTE", *b"ABCD", *b"acTL", *b"fcTL", *b"fdAT"] {
        let hostile = insert_metadata(png(1, 1), &kind, &[0; 13], false);
        let mut scanner = png_metadata::PngMetadata::new(true, true);
        assert!((0..1000).find_map(|_| scanner.step(&hostile).err()).is_some(), "tail {kind:?} cannot escape validation without Reader::finish");
    }
    let separated = insert_metadata(png(1, 1), b"vpAg", &[1], false);
    let separated = insert_metadata(separated, b"IDAT", &[0], false);
    let mut scanner = png_metadata::PngMetadata::new(true, true);
    assert!((0..1000).find_map(|_| scanner.step(&separated).err()).unwrap().to_string().contains("consecutive"));
}

#[test]
fn png_text_metadata_preflight_bills_real_vector_growth_under_the_existing_codec_cap() {
    let mut image = png(1, 1);
    let marker = image.windows(4).position(|kind| kind == b"IDAT").unwrap() - 4;
    let tail = image.split_off(marker);
    let entry = metadata_chunk(b"tEXt", b"k\0");
    let record_size = std::mem::size_of::<png::text_metadata::TEXtChunk>() as u64;
    let count = (MAX_CODEC_BYTES / (4 * record_size)).next_power_of_two() as usize;
    let mut within = Vec::with_capacity(image.len() + count * entry.len() + tail.len());
    within.extend(&image);
    for _ in 0..count {
        within.extend(&entry);
    }
    within.extend(&tail);
    assert!(within.len() < MAX_TEXTURE_BYTES);
    // This is a pure allocation-free policy check, not a large codec allocation.
    png_metadata::validate(&within).unwrap();
    let mut excessive = Vec::with_capacity(image.len() + (count + 1) * entry.len() + tail.len());
    excessive.extend(&image);
    for _ in 0..count + 1 {
        excessive.extend(&entry);
    }
    excessive.extend(&tail);
    assert!(excessive.len() < MAX_TEXTURE_BYTES);
    assert!(
        png_metadata::validate(&excessive).unwrap_err().to_string().contains("codec allowance"),
        "the next actual Vec<TEXtChunk> growth would overlap old/new arrays above64MiB"
    );
    assert!(TextureSource::new(excessive, MapRole::BaseColor).err().unwrap().to_string().contains("codec allowance"));
}

#[test]
#[cfg(target_pointer_width = "64")]
fn png_text_metadata_bills_latin1_old_and_new_string_backing_before_a_codec_can_run() {
    const MIB: usize = 1024 * 1024;
    let base = png(1, 1);
    let marker = base.windows(4).position(|kind| kind == b"IDAT").unwrap() - 4;
    let prefix = &base[..marker];
    let tail = &base[marker..];
    let mut latin = Vec::with_capacity(12 * MIB + 2);
    latin.extend(b"k\0");
    latin.resize(12 * MIB + 2, 0xff);
    let large = metadata_chunk(b"tEXt", &latin);
    let mut alone = Vec::with_capacity(prefix.len() + large.len() + tail.len());
    alone.extend(prefix);
    alone.extend(&large);
    alone.extend(tail);
    png_metadata::validate(&alone).unwrap(); // No arbitrary12MiB text-size prohibition.
    let count = 262_143;
    let tiny = metadata_chunk(b"tEXt", b"k\0");
    let mut hostile = Vec::with_capacity(prefix.len() + count * tiny.len() + large.len() + tail.len());
    hostile.extend(prefix);
    for _ in 0..count {
        hostile.extend(&tiny);
    }
    hostile.extend(&large);
    hostile.extend(tail);
    assert!(hostile.len() < MAX_TEXTURE_BYTES && hostile.capacity() <= MAX_TEXTURE_BYTES);
    assert_eq!(std::mem::size_of::<png::text_metadata::TEXtChunk>(), 48);
    // Independent lower bound, deliberately ignoring all small-string, fixed
    // decoder and parser storage: raw16MiB + existing text-vector12MiB +
    // old12/new24MiB Latin1 expansion already reaches the whole64MiB cap.
    // The positive keyword/fixed allocations make that an actual excess.
    let lower_bound = 16 * MIB + (count + 1) * 48 + 12 * MIB + 24 * MIB;
    assert_eq!(lower_bound as u64, MAX_CODEC_BYTES);
    assert!(png_metadata::validate(&hostile).unwrap_err().to_string().contains("codec allowance"));
    assert!(TextureSource::new(hostile, MapRole::BaseColor).err().unwrap().to_string().contains("codec allowance"));
}
