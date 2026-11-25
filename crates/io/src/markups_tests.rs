//! The markup set file: its format, its one check, and writers that meet.

use super::*;

/// A fresh folder of the system's temporary folder for one test.
fn scratch(tag: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("solidcraft-markups-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn markup(id: u64, guid: &str, by: &str) -> Markup {
    Markup {
        id,
        guid: guid.into(),
        status: Status::Open,
        thread: vec![Message { by: by.into(), at: 1, text: "Make this wall 2 mm thicker.".into() }],
        anchor: Anchor::Point { at: [1.0, 2.0, 3.0] },
        view: None,
        images: Vec::new(),
        by: by.into(),
        made: 1,
        changed: 1,
        outcome: None,
    }
}

const GUID_A: &str = "0123456789abcdef0123456789abcdef";
const GUID_B: &str = "fedcba9876543210fedcba9876543210";

/// A set written by format 1, frozen here as text: every later build reads it as it is.
const FORMAT_1: &str = r#"{"format":1,"next_id":3,"markups":[{"id":1,"guid":"0123456789abcdef0123456789abcdef","status":"open","thread":[{"by":"Reviewer","at":1700000000,"text":"Round this edge."},{"by":"agent","at":1700000100,"text":"Which radius?"}],"anchor":{"kind":"face","body":"Body1","maker":2,"face":"F2:top","at":[5.0,10.0,30.0]},"view":{"target":[0.0,0.0,0.0],"yaw":30.0,"pitch":20.0,"distance":200.0,"fov":45.0},"by":"Reviewer","made":1700000000,"changed":1700000100},{"id":2,"guid":"fedcba9876543210fedcba9876543210","status":"resolved","thread":[{"by":"Reviewer","at":1700000000,"text":"Thicker base."},{"by":"agent","at":1700000200,"text":"Base now 6 mm."}],"anchor":{"kind":"edge","body":"Body1","maker":2,"edge":"F2:side:l1|F2:top","at":[5.0,0.0,30.0],"config":"Default"},"by":"Reviewer","made":1700000000,"changed":1700000200,"outcome":{"by":"agent","reason":"Base now 6 mm.","at":1700000200}}]}"#;

#[test]
fn a_format_1_set_reads_as_it_is_and_writes_back_the_same() {
    let set = parse(FORMAT_1).unwrap();
    assert_eq!(set.markups.len(), 2);
    assert_eq!(set.markups[0].note(), "Round this edge.");
    assert!(matches!(&set.markups[0].anchor, Anchor::Face { face, .. } if face == "F2:top"));
    assert_eq!(set.markups[1].status, Status::Resolved);
    assert_eq!(to_text(&set).unwrap(), FORMAT_1);
}

#[test]
fn a_newer_set_is_refused_and_left_as_it_was() {
    let dir = scratch("newer");
    let set = dir.join("part.sc.markups.json").to_string_lossy().to_string();
    let newer = FORMAT_1.replacen("\"format\":1", "\"format\":2", 1);
    std::fs::write(&set, &newer).unwrap();
    let e = read(&set).unwrap_err();
    assert!(e.contains("newer SolidCraft"), "{e}");
    let e = update(&set, |s| {
        s.next_id += 1;
        Ok(())
    })
    .unwrap_err();
    assert!(e.contains("newer SolidCraft"), "{e}");
    assert_eq!(std::fs::read_to_string(&set).unwrap(), newer);
}

#[test]
fn keys_this_format_does_not_know_are_refused_not_dropped() {
    // A write puts the set back whole, so a key it would leave out is refused instead.
    let extra = FORMAT_1.replacen("\"next_id\":3", "\"next_id\":3,\"owner\":\"x\"", 1);
    assert!(parse(&extra).is_err());
    let extra = FORMAT_1.replacen("\"status\":\"open\"", "\"status\":\"open\",\"priority\":1", 1);
    assert!(parse(&extra).is_err());
    assert!(parse("{}").is_err());
    assert!(parse("[]").is_err());
    assert!(parse("not json").is_err());
}

#[test]
fn the_check_refuses_what_a_set_must_not_hold() {
    let ok = Set { format: FORMAT, next_id: 3, markups: vec![markup(1, GUID_A, "R"), markup(2, GUID_B, "R")] };
    assert!(ok.check().is_ok());
    let bad = |f: &dyn Fn(&mut Set)| {
        let mut s = ok.clone();
        f(&mut s);
        s.check().is_err()
    };
    assert!(bad(&|s| s.markups[1].id = 1), "a number twice");
    assert!(bad(&|s| s.markups[1].id = 3), "a number not below next_id");
    assert!(bad(&|s| s.markups[1].guid = GUID_A.to_uppercase()), "a guid twice");
    assert!(bad(&|s| s.markups[0].guid = "../../etc/passwd".into()), "a guid that is not one");
    assert!(bad(&|s| s.markups[0].thread.clear()), "no note");
    assert!(bad(&|s| s.markups[0].status = Status::Resolved), "closed with no outcome");
    assert!(bad(&|s| s.markups[0].anchor = Anchor::Point { at: [f64::NAN, 0.0, 0.0] }), "a point that is not one");
    assert!(bad(&|s| s.markups[0].anchor = Anchor::Occurrence { path: vec![], name: "A".into(), at: None }), "an empty path");
    assert!(
        bad(&|s| s.markups[0].images.push(Image { file: "../x.png".into(), name: "x".into(), by: "R".into(), at: 1, bytes: 1 })),
        "a picture outside the folder"
    );
    assert!(bad(&|s| s.markups[0].by = "a\u{0}b".into()), "a control character in a name");
    assert!(bad(&|s| s.markups[0].thread[0].text = "x".repeat(MAX_NOTE + 1)), "a note too long");
    // Anyone but the maker writes at most MAX_REPLY characters.
    assert!(bad(&|s| s.markups[0].thread.push(Message { by: "agent".into(), at: 2, text: "x".repeat(MAX_REPLY + 1) })));
    assert!(!bad(&|s| s.markups[0].thread.push(Message { by: "R".into(), at: 2, text: "x".repeat(MAX_REPLY + 1) })));
}

#[test]
fn a_reply_too_long_says_the_limit_its_length_and_how_far_over() {
    let e = check_text(&"x".repeat(658), MAX_REPLY).unwrap_err();
    assert!(e.contains("280") && e.contains("658") && e.contains("378 over"), "{e}");
}

#[test]
fn a_set_path_names_its_folder_and_pictures_are_entry_names() {
    assert_eq!(beside("/d/part.sc"), "/d/part.sc.markups.json");
    assert_eq!(folder("/d/part.sc.markups.json").unwrap(), "/d/part.sc.markups");
    assert!(folder("/d/part.json").is_err());
    assert!(folder("/d/part.sc").is_err());
    assert!(is_image_file(&format!("{GUID_A}.png")));
    for bad in ["../a.png", "a.png", &format!("{GUID_A}.gif"), &format!("../{GUID_A}.png"), ""] {
        assert!(!is_image_file(bad), "{bad}");
    }
    assert_eq!(image_kind(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0]), Some("png"));
    assert_eq!(image_kind(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
    assert_eq!(image_kind(b"GIF89a"), None);
}

/// Two writers at once, each adding markups one change at a time: neither loses the other's.
#[test]
fn two_writers_at_once_lose_nothing() {
    let dir = scratch("writers");
    let set = dir.join("part.sc.markups.json").to_string_lossy().to_string();
    const EACH: u64 = 40;
    let writer = |who: &'static str, set: String| {
        std::thread::spawn(move || {
            for i in 0..EACH {
                update(&set, |s| {
                    let id = s.next_id;
                    s.next_id += 1;
                    let guid = format!("{:016x}{:016x}", if who == "a" { 1u64 } else { 2 }, i);
                    s.markups.push(markup(id, &guid, who));
                    Ok(())
                })
                .unwrap();
            }
        })
    };
    let a = writer("a", set.clone());
    let b = writer("b", set.clone());
    a.join().unwrap();
    b.join().unwrap();
    let s = read(&set).unwrap();
    assert_eq!(s.markups.len() as u64, 2 * EACH);
    assert_eq!(s.next_id, 2 * EACH + 1);
    for who in ["a", "b"] {
        assert_eq!(s.markups.iter().filter(|m| m.by == who).count() as u64, EACH);
    }
}

#[test]
fn a_refused_change_leaves_the_file_as_it_was() {
    let dir = scratch("refused");
    let set = dir.join("part.sc.markups.json").to_string_lossy().to_string();
    update(&set, |s| {
        s.markups.push(markup(1, GUID_A, "R"));
        s.next_id = 2;
        Ok(())
    })
    .unwrap();
    let before = std::fs::read(&set).unwrap();
    // The step refuses.
    assert!(update(&set, |_| Err::<(), _>("no".into())).is_err());
    // The step makes a set the check refuses.
    assert!(
        update(&set, |s| {
            s.markups[0].thread.clear();
            Ok(())
        })
        .is_err()
    );
    assert_eq!(std::fs::read(&set).unwrap(), before);
}
