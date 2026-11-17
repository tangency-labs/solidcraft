//! WR-605 on the upstream fixtures: the ring fillets of the `blend.fillet_removal`
//! goldens rows `nurb_fillet_face18` and `defillet_face8` are removed.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::volume;
use solidcore::blend::fillet_removal::delete_fillet_face;
use solidcore::geom::attribute::AttributeStore;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel};

fn fixture(k: &mut Kernel, file: &str) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(file);
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        eprintln!("unavailable upstream test file: {}", path.display());
        return None;
    };
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = solidcore::topology::loopuse::notify_ref;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    Some(b)
}

/// The removal of a face: whether it succeeded, the face counts before and after,
/// whether the body is one closed manifold solid, its volumes before and after and its
/// Level 2 reports after.
struct Removal {
    ok: bool,
    faces: (usize, usize),
    manifold: bool,
    volumes: (f64, f64),
    level2: Vec<String>,
}

/// Removes face `index` of `file`.
fn remove(file: &str, index: usize) -> Option<Removal> {
    let mut k = Kernel::default();
    let mut b = fixture(&mut k, file)?;
    let f = bp1::faces(&b, None).unwrap()[index];
    let faces = b.face_list().len();
    let v0 = volume(&k, &mut b).unwrap();
    b.set_editing_enabled(true);
    let r = delete_fillet_face(&mut k, &mut b, f, None);
    b.set_editing_enabled(false);
    eprintln!("{file} {index}: {r:?}");
    let v1 = volume(&k, &mut b).unwrap();
    let level2 = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    Some(Removal {
        ok: r.is_ok(),
        faces: (faces, b.face_list().len()),
        manifold: brep_is_manifold_solid(&b).unwrap_or(false),
        volumes: (v0, v1),
        level2,
    })
}

/// The ring fillets of the fixtures (`nurb_fillet.smb` face 18, `defillet.smb` face 8),
/// which the four-sided removal refused, are removed: one face fewer, one closed manifold
/// solid with no Level 2 report.
#[test]
fn ring_fillets_of_the_fixtures_are_removed() {
    for (file, index) in [
        ("Solids/nurb_fillet.smb", 18usize),
        ("Defeature/defillet.smb", 8),
    ] {
        let Some(r) = remove(file, index) else {
            return;
        };
        eprintln!(
            "{file} {index}: faces {:?} volumes {:?}",
            r.faces, r.volumes
        );
        assert!(r.ok, "{file} {index}: refused");
        assert_eq!(r.faces.1 + 1, r.faces.0, "{file} {index}: faces");
        assert!(r.manifold, "{file} {index}: not one closed manifold solid");
        assert!(
            r.level2.is_empty(),
            "{file} {index}: Level 2 {:?}",
            r.level2
        );
    }
}
