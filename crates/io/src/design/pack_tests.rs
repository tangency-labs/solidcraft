use std::io::{Cursor, Read};

use super::*;

/// Each entry of `pack`: its name, how it is stored and its bytes.
fn entries(pack: &[u8]) -> Vec<(String, zip::CompressionMethod, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(pack)).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut file = zip.by_index(i).unwrap();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            (file.name().unwrap().into_owned(), file.compression(), bytes)
        })
        .collect()
}

/// Files go in with the bytes given: a design container stored as it is, anything else
/// deflated; the top-level file and the linked designs under `designs/`.
#[test]
fn files_go_in_as_given() {
    let container = b"PK\x03\x04 a design container's bytes".to_vec();
    let json = br#"{"format": "solidcraft/1"}"#.repeat(40);
    let mut pack = PackWriter::default();
    assert_eq!(pack.add(None, "case.sc", &container).unwrap(), "case.sc");
    assert_eq!(pack.add(Some(PACK_DESIGNS), "plate.json", &json).unwrap(), "designs/plate.json");
    let got = entries(&pack.finish().unwrap());
    assert_eq!(
        got,
        [
            ("case.sc".to_string(), zip::CompressionMethod::Stored, container),
            ("designs/plate.json".to_string(), zip::CompressionMethod::Deflated, json)
        ]
    );
}

/// A name another entry has, case aside, is numbered before its extension; the same bytes under
/// two names are two entries.
#[test]
fn clashing_names_are_numbered() {
    let mut pack = PackWriter::default();
    let names: Vec<String> =
        ["plate.sc", "Plate.SC", "plate.sc", "plate", "PLATE"].iter().map(|n| pack.add(Some(PACK_DESIGNS), n, b"x").unwrap()).collect();
    assert_eq!(names, ["designs/plate.sc", "designs/Plate-2.SC", "designs/plate-3.sc", "designs/plate", "designs/PLATE-2"]);
    // The same name at the top level is another entry.
    assert_eq!(pack.add(None, "plate.sc", b"x").unwrap(), "plate.sc");
    assert_eq!(entries(&pack.finish().unwrap()).len(), 6);
}

/// A name is made safe to unpack anywhere: no separators, drive or reserved characters, no
/// control characters, no `.` or space at the end, never empty, and never a device's name.
#[test]
fn names_are_made_safe() {
    for (given, made) in [
        ("../../etc/passwd", ".._.._etc_passwd"),
        (r"C:\parts\plate.sc", "C__parts_plate.sc"),
        ("a<b>c:d\"e|f?g*h.sc", "a_b_c_d_e_f_g_h.sc"),
        ("tab\there\n.sc", "tab_here_.sc"),
        ("plate.sc. . ", "plate.sc"),
        ("", "file"),
        ("...", "file"),
        ("CON", "CON_"),
        ("nul.sc", "nul_.sc"),
        ("com1.part.sc", "com1_.part.sc"),
        ("LPT9", "LPT9_"),
        ("console.sc", "console.sc"),
        ("プレート.sc", "プレート.sc"),
    ] {
        assert_eq!(entry_name(given), made, "{given:?}");
    }
}

/// No more than MAX_PACK_ENTRIES files, and the bytes are counted as they are added: the file
/// that takes the pack past MAX_PACK_BYTES is refused.
#[test]
fn caps_hold() {
    let mut pack = PackWriter::default();
    for i in 0..MAX_PACK_ENTRIES {
        pack.add(Some(PACK_DESIGNS), &format!("d{i}.sc"), b"x").unwrap();
    }
    let e = pack.add(Some(PACK_DESIGNS), "one-more.sc", b"x").unwrap_err().to_string();
    assert!(e.contains(&format!("at most {MAX_PACK_ENTRIES} files")), "{e}");
    assert!(fits_pack(MAX_PACK_BYTES).is_ok());
    let e = fits_pack(MAX_PACK_BYTES + 1).unwrap_err().to_string();
    assert_eq!(e, "the files come to 1025 MiB; a pack holds at most 1024 MiB");
    assert!(is_pack_path("a/b.ZIP") && is_pack_path("b.zip") && !is_pack_path("b.zip.sc") && !is_pack_path("zip"));
}
