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

fn declaration_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut result = (data.len() as u32).to_be_bytes().to_vec();
    result.extend(kind);
    result.extend(data);
    let mut crc = u32::MAX;
    for byte in kind.iter().chain(data) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320_u32.wrapping_mul(crc & 1));
        }
    }
    result.extend((!crc).to_be_bytes());
    result
}
fn declared_image(declarations: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut bytes = png(2, 2);
    let chunks: Vec<_> = declarations.iter().flat_map(|(kind, data)| declaration_chunk(kind, data)).collect();
    bytes.splice(33..33, chunks);
    bytes
}
#[test]
fn legacy_gamma_admission_preserves_encoded_pixels_and_gpu_map_interpretation() {
    let bytes = declared_image(&[(b"gAMA", 100_000_u32.to_be_bytes().to_vec())]);
    let legacy = TextureSource::new(bytes.clone(), MapRole::BaseColor).unwrap();
    assert_eq!(legacy.decode(|| false).unwrap().rgba(), [64_u8, 128, 192, 255].repeat(4));
    let map = crate::job::textures::TextureMap::new(bytes.clone(), MapRole::BaseColor).unwrap();
    let plain = crate::job::textures::TextureMap::new(png(2, 2), MapRole::BaseColor).unwrap();
    let cancel = crate::job::CancelToken::default();
    assert_eq!(map.decode(&cancel).unwrap(), plain.decode(&cancel).unwrap());
    assert!(TextureSource::new_portable(bytes, MapRole::BaseColor).err().unwrap().to_string().contains("normalized sRGB"));
}
#[test]
fn normalized_srgb_declarations_are_accepted_without_reencoding_samples() {
    let chromaticities = [31_270_u32, 32_900, 64_000, 33_000, 30_000, 60_000, 15_000, 6_000].into_iter().flat_map(u32::to_be_bytes).collect();
    let declarations = [(b"gAMA", 45_455_u32.to_be_bytes().to_vec()), (b"cHRM", chromaticities), (b"sRGB", vec![0]), (b"cICP", vec![1, 13, 0, 1])];
    for declaration in &declarations {
        let bytes = declared_image(std::slice::from_ref(declaration));
        let source = TextureSource::new_portable(bytes, MapRole::BaseColor).unwrap();
        source.validate_static_source().unwrap();
        assert!(source.decode(|| false).unwrap().rgba().as_chunks::<4>().0.iter().all(|pixel| *pixel == [64, 128, 192, 255]));
    }
    let source = TextureSource::new_portable(declared_image(&declarations), MapRole::BaseColor).unwrap();
    assert_eq!(source.decode(|| false).unwrap().rgba().len(), 16);
}
#[test]
fn conflicting_png_color_declarations_refuse_color_but_preserve_explicit_linear_data() {
    let chromaticities = [31_270_u32, 32_900, 64_000, 33_000, 30_000, 50_000, 15_000, 6_000].into_iter().flat_map(u32::to_be_bytes).collect();
    for declaration in [(b"gAMA", 100_000_u32.to_be_bytes().to_vec()), (b"cHRM", chromaticities), (b"cICP", vec![1, 16, 0, 1])] {
        let bytes = declared_image(std::slice::from_ref(&declaration));
        assert!(TextureSource::new_portable(bytes.clone(), MapRole::BaseColor).err().unwrap().to_string().contains("normalized sRGB"));
        for role in [MapRole::NormalOpenGl, MapRole::Roughness, MapRole::Metallic, MapRole::AmbientOcclusion] {
            let source = TextureSource::new_portable(bytes.clone(), role).unwrap();
            assert_eq!(source.decode(|| false).unwrap().rgba().get(..4), Some([64, 128, 192, 255].as_slice()));
        }
    }
    let duplicate = declared_image(&[(b"gAMA", 45_455_u32.to_be_bytes().to_vec()), (b"gAMA", 45_455_u32.to_be_bytes().to_vec())]);
    assert!(TextureSource::new_portable(duplicate.clone(), MapRole::BaseColor).is_err());
    let mut metadata = png_metadata::PngMetadata::new(true, true).with_role(MapRole::BaseColor);
    let error = (0..100).find_map(|_| metadata.step(&duplicate).err()).unwrap();
    assert!(error.to_string().contains("normalized sRGB"), "cooperative color checks retain duplicate-declaration state between steps");
    for kind in [b"mDCV", b"cLLI"] {
        assert!(validate_color_declarations(&declared_image(&[(kind, vec![0; 24])]), MapRole::BaseColor).is_err());
    }
}
