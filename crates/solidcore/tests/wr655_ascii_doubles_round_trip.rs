//! WR-655: `SmDatabaseIO` writes ASCII doubles as fixed `%.16f`, which keeps fewer than
//! 17 significant digits below 1, so an ASCII write and read moves such values (`0.3`
//! plus 1 ULP reads back as `0.3`). `Kernel::fixes.ascii_doubles_round_trip` (off by
//! default: the C++ writer's bytes) writes each double in the shortest form that reads
//! back as the same double, so an ASCII save and reload keeps every value.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)] // test helpers

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::api_files::{read_brep_from_file, write_brep_to_file};
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::database_io::{
    fixed16, round_trip, DatabaseReader, DatabaseWriter, FileType,
};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{EdgeId, EdgeuseId, FaceId};
use solidcore::{Brep, Kernel, Point3};

/// Doubles `%.16f` does not keep, and some it does.
const VALUES: [f64; 9] = [
    0.300_000_000_000_000_04, // 0.1 + 0.2, the record's example
    1.0 / 3.0,
    2.0 / 3.0e-5,
    1e-300,
    -0.0,
    0.0,
    123_456_789.123,
    -1.0 / 7.0,
    f64::MAX,
];

/// `values` written as one ASCII line and read back with `>>`.
fn write_read(values: &[f64], round_trip_doubles: bool) -> Vec<f64> {
    let mut w =
        DatabaseWriter::open_for_write(FileType::Ascii).with_round_trip_doubles(round_trip_doubles);
    w.reals(values);
    let mut db = DatabaseReader::new();
    db.open_for_read(w.into_bytes(), FileType::Ascii);
    let s = db.in_stream_mut().unwrap();
    values.iter().map(|_| s.extract_double().unwrap()).collect()
}

#[test]
fn the_records_example() {
    let x = 0.1 + 0.2;
    assert_eq!(x, 0.300_000_000_000_000_04);
    assert_eq!(fixed16(x), "0.3000000000000000");
    assert_eq!(round_trip(x), "3.0000000000000004e-1");
    assert_eq!(round_trip(1.0), "1e0");
    assert_eq!(round_trip(-0.0), "-0e0");
    assert_eq!(round_trip(f64::INFINITY), "inf");
    assert_eq!(round_trip(f64::NAN), "nan");
}

#[test]
fn ascii_doubles_read_back_unchanged_only_with_the_fix() {
    let back = write_read(&VALUES, true);
    for (a, b) in VALUES.iter().zip(&back) {
        assert_eq!(a.to_bits(), b.to_bits(), "{a:e} read back as {b:e}");
    }
    // the default is upstream's %.16f: the values below 1 move
    let back = write_read(&VALUES, false);
    assert_eq!(back[0], 0.3);
    assert_eq!(back[3], 0.0);
    assert_ne!(back[7], VALUES[7]); // -1/7 needs 17 digits (1/3 happens to need 16)
                                    // those of 1 and more keep 17 significant digits
    assert_eq!(back[2], VALUES[2]);
    assert_eq!(back[6], VALUES[6]);
}

/// A box with corners at doubles `%.16f` does not keep, written as an ASCII B-rep file.
fn box_file(k: &mut Kernel) -> (Brep, Vec<u8>) {
    let origin = Point3::new(0.1 + 0.2, 1.0 / 3.0, -2.0 / 3.0e-5);
    let b = create_box(k, origin, 1.0 / 7.0, 0.7, 0.07).unwrap();
    let store = AttributeStore::new();
    let records = NotifyRecords::new();
    let key_of = |e| b.owner_identity(e);
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &key_of,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    let file = write_brep_to_file(k, Some(&b), &env, true).unwrap();
    (b, file)
}

/// The vertex points of `b` as bits, sorted.
fn corners(b: &Brep) -> Vec<[u64; 3]> {
    let mut v: Vec<[u64; 3]> = b
        .vertices()
        .iter()
        .map(|(_, v)| {
            [
                v.point.x.to_bits(),
                v.point.y.to_bits(),
                v.point.z.to_bits(),
            ]
        })
        .collect();
    v.sort_unstable();
    v
}

fn read(k: &mut Kernel, file: Vec<u8>) -> Brep {
    let mut attributes = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let fresh = Brep::new();
    let key_of = |e| fresh.owner_identity(e);
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key_of,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_brep_from_file(k, Some(file), true, false, &mut env).unwrap()
}

#[test]
fn an_ascii_b_rep_reloads_exactly_with_the_fix() {
    let mut k = Kernel::default();
    k.fixes.ascii_doubles_round_trip = true;
    let (b, file) = box_file(&mut k);
    let text = String::from_utf8(file.clone()).unwrap();
    assert!(text.contains("e-1"), "exponent notation");
    let back = read(&mut k, file);
    assert_eq!(corners(&back), corners(&b));
}

#[test]
fn by_default_the_file_is_upstreams_and_moves_the_corners() {
    let mut k = Kernel::default();
    let (b, file) = box_file(&mut k);
    let text = String::from_utf8(file.clone()).unwrap();
    assert!(text.contains("0.3000000000000000"), "fixed %.16f");
    let back = read(&mut k, file);
    assert_ne!(corners(&back), corners(&b));
}
