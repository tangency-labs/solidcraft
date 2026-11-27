//! A design file to the bitmap the handler returns, on every platform: both picture formats, the
//! sizes Explorer asks for, and the files that must fail (hostile ones included).

mod support;

use std::io::Cursor;

use support::{DOCUMENT, V1, V2};

fn size(bytes: &[u8], cx: u32) -> Option<(u32, u32)> {
    solidcraft_explorer::thumbnail(Cursor::new(bytes), cx).map(|b| (b.width, b.height))
}

#[test]
fn both_picture_formats_give_a_thumbnail_of_the_size_asked_for() {
    let v2 = support::design(V2, 512, 384);
    assert_eq!(size(&v2, 1024), Some((512, 384)));
    assert_eq!(size(&v2, 256), Some((256, 192)));
    assert_eq!(size(&v2, 96), Some((96, 72)));
    assert_eq!(size(&v2, 32), Some((32, 24)));
    let v1 = support::design(V1, 240, 150);
    assert_eq!(size(&v1, 256), Some((240, 150)));
    assert_eq!(size(&v1, 96), Some((96, 60)));
    assert_eq!(size(&v1, 0), None);
}

#[test]
fn a_thumbnail_is_bgra_rows_from_the_top() {
    let bitmap = solidcraft_explorer::thumbnail(Cursor::new(support::design(V2, 512, 384)), 512).unwrap();
    assert_eq!(bitmap.bgra.len(), 512 * 384 * 4);
    assert_eq!(bitmap.bgra[0..4], [0, 255, 0, 255], "the top-left pixel is green");
    let last_row = 383 * 512 * 4;
    assert_eq!(bitmap.bgra[last_row..last_row + 4], [0, 0, 255, 255], "the bottom-left pixel is red");
    assert_eq!(bitmap.bgra[511 * 4 + 3], 0, "the right half is transparent");
}

#[test]
fn designs_without_a_usable_picture_give_none() {
    assert_eq!(size(&support::keyed_to(V2, 512, 384, b"{}"), 256), None, "stale key");
    assert_eq!(size(&support::design(V1, 512, 384), 256), None, "format 1 must be 240 x 150");
    assert_eq!(size(&support::design("solidcraft-thumbnail/9", 512, 384), 256), None, "unknown format");
    assert_eq!(size(&support::container(&[("container.json", support::CONTAINER), ("document.json", DOCUMENT)]), 256), None, "no picture");
    assert_eq!(size(DOCUMENT, 256), None, "plain JSON");
    assert_eq!(size(b"", 256), None, "empty");
}

#[test]
fn hostile_files_give_none_without_a_panic() {
    let good = support::design(V2, 512, 384);
    for n in 0..good.len() {
        assert_eq!(std::panic::catch_unwind(|| size(&good[..n], 256)).unwrap(), None, "cut at {n}");
    }
    assert_eq!(size(&support::bomb(), 256), None, "decompression bomb");
    assert_eq!(size(&support::many_entries(&good), 256), None, "65,535 entries claimed");
}

#[test]
fn transparent_pixels_come_back_as_zeros() {
    // The test picture's right half is transparent blue.
    for (tag, w, h, cx) in [(V2, 512, 384, 512), (V2, 512, 384, 256), (V1, 240, 150, 256), (V1, 240, 150, 96)] {
        let bitmap = solidcraft_explorer::thumbnail(Cursor::new(support::design(tag, w, h)), cx).unwrap();
        let row = bitmap.width as usize * 4;
        assert_eq!(bitmap.bgra[row - 4..row], [0, 0, 0, 0], "{tag} at {cx}: the top-right pixel");
        assert_eq!(bitmap.bgra[bitmap.bgra.len() - 4..], [0, 0, 0, 0], "{tag} at {cx}: the bottom-right pixel");
    }
}

#[test]
fn every_size_explorer_asks_for_fits_without_enlarging() {
    let v2 = support::design(V2, 512, 384);
    let v1 = support::design(V1, 240, 150);
    for (cx, want_v2, want_v1) in [
        (1, (1, 1), (1, 1)),
        (16, (16, 12), (16, 10)),
        (32, (32, 24), (32, 20)),
        (96, (96, 72), (96, 60)),
        (256, (256, 192), (240, 150)),
        (384, (384, 288), (240, 150)),
        (512, (512, 384), (240, 150)),
        (1024, (512, 384), (240, 150)),
        (u32::MAX, (512, 384), (240, 150)),
    ] {
        assert_eq!(size(&v2, cx), Some(want_v2), "format 2 at {cx}");
        assert_eq!(size(&v1, cx), Some(want_v1), "format 1 at {cx}");
    }
}

#[test]
fn every_truncation_of_a_format_1_design_gives_none() {
    let good = support::design(V1, 240, 150);
    for n in 0..good.len() {
        assert_eq!(std::panic::catch_unwind(|| size(&good[..n], 96)).unwrap(), None, "cut at {n}");
    }
}
