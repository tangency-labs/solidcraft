use super::*;

#[cfg(feature = "native")]
#[test]
fn box_end_to_end() {
    let b = Brep::make_box([1.0, 2.0, 3.0], [10.0, 20.0, 30.0]).unwrap();
    assert!((b.volume(1e-7).unwrap() - 6000.0).abs() < 1e-6);
    assert_eq!(b.counts().unwrap(), [6, 12, 8]);
    let c = b.try_clone().unwrap();
    drop(b);
    assert!((c.volume(1e-7).unwrap() - 6000.0).abs() < 1e-6);
    assert_eq!(current_call(), None, "no call in flight between calls");
}

#[cfg(feature = "native")]
#[test]
fn bad_arguments_are_errors() {
    assert_eq!(Brep::make_box([0.0; 3], [1.0, -1.0, 1.0]).unwrap_err(), Error::Argument);
    assert_eq!(Brep::make_box([f64::NAN, 0.0, 0.0], [1.0; 3]).unwrap_err(), Error::Argument);
    let b = Brep::make_box([0.0; 3], [1.0; 3]).unwrap();
    assert_eq!(b.volume(f64::INFINITY).unwrap_err(), Error::Argument);
}

#[cfg(feature = "native")]
#[test]
fn native_files_round_trip_and_failures_dump_their_inputs() {
    let dir = std::env::temp_dir().join(format!("solidcraft-smlib-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let b = Brep::make_box([0.0; 3], [2.0, 3.0, 4.0]).unwrap();
    let path = dir.join("box.smb");
    b.write(&path, false).unwrap();
    let back = Brep::read(&path, false).unwrap();
    assert!((back.volume(1e-7).unwrap() - 24.0).abs() < 1e-9);

    let dumps = dir.join("dumps");
    std::fs::create_dir_all(&dumps).unwrap();
    set_dump_dir(Some(&dumps)).unwrap();
    assert!(b.write(&dir.join("missing/x.smb"), false).is_err());
    set_dump_dir(None).unwrap();
    let names: Vec<String> =
        std::fs::read_dir(&dumps).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().any(|n| n.ends_with("-smlib_brep_write.txt")), "{names:?}");
    let smb = names.iter().find(|n| n.ends_with("-smlib_brep_write-brep.smb")).unwrap();
    let replayed = Brep::read(&dumps.join(smb), false).unwrap();
    assert!((replayed.volume(1e-7).unwrap() - 24.0).abs() < 1e-9);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(feature = "native")]
#[test]
fn calls_from_many_threads_are_serialised() {
    let handles: Vec<_> = (0..8)
        .map(|i| {
            std::thread::spawn(move || {
                let s = 1.0 + i as f64;
                let b = Brep::make_box([0.0; 3], [s; 3]).unwrap();
                (b.volume(1e-7).unwrap() - s * s * s).abs() < 1e-9 * s * s * s
            })
        })
        .collect();
    assert!(handles.into_iter().all(|h| h.join().unwrap()));
}

#[cfg(not(feature = "native"))]
#[test]
fn without_native_every_call_says_so() {
    assert_eq!(init(), Err(Error::NotBuilt));
    assert_eq!(Brep::make_box([0.0; 3], [1.0; 3]).unwrap_err(), Error::NotBuilt);
    assert_eq!(current_call(), None);
}
