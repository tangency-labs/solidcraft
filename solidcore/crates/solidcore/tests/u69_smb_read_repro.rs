#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel};

fn read(on: bool, bytes: Vec<u8>) -> Result<Brep, solidcore::SolidError> {
    let mut k = Kernel::default();
    k.fixes.brep_edit_statuses_pass_on = on;
    let mut b = Brep::new_with_infinite_region()?;
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = |_: EntityRef| ObjectRef { key: ObjectKey(0), type_code: 0 };
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(&mut k, &mut b, bytes, FileType::Ascii, false, &mut env)?;
    Ok(b)
}

#[test]
fn repro() {
    let p = std::env::var("SMB").unwrap_or_else(|_| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/topology.face_p1/plate_2holes.smb").to_string()
    });
    let bytes = std::fs::read(&p).unwrap();
    println!("off: {:?}", read(false, bytes.clone()).map(|_| ()));
    println!("on: {:?}", read(true, bytes).map(|_| ()));
}
