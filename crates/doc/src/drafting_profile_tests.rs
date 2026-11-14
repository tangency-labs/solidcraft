use serde_json::{Value, json};

use crate::DocError;
use crate::display_units::{DecimalMarker, DimensionValue, MAX_DECIMALS, NumberFormat, format_dimension, format_number};
use crate::drafting_profile::DraftingProfile;
use crate::expr::Kind;
use crate::tolerance::{Indicator, Repeat, Role, Tolerance, TolerancePresentation};

fn profile(id: &str) -> DraftingProfile {
    DraftingProfile::library().into_iter().find(|p| p.id == id).unwrap()
}

/// 40 ±0.05, `count` times, as a diameter when counted.
fn dimension(profile: &DraftingProfile, count: u32) -> String {
    let value = DimensionValue {
        nominal: 40.0,
        kind: Kind::Length,
        tolerance: Tolerance::Symmetric { deviation: 0.05 },
        presentation: TolerancePresentation::Deviations,
        role: Role::Normal,
        indicator: (count > 1).then_some(Indicator::Diameter),
        count,
        not_to_scale: false,
    };
    format_dimension(&value, &profile.style(Kind::Length, "mm").unwrap()).unwrap().plain()
}

fn refused(p: &DraftingProfile, fragment: &str) {
    match p.validate() {
        Err(DocError::Invalid(message)) => assert!(message.contains(fragment), "{message:?} does not name {fragment:?}"),
        other => panic!("{p:?}: expected a refusal naming {fragment:?}, got {other:?}"),
    }
}

/// The built-ins: the default first (ISO's rules with a decimal point), ISO 129-1 with its
/// decimal comma, and ASME with a point, no trailing zeros and `4X`; each valid, ids unique. A
/// length takes 2 decimals, as a new dimension and a hole callout do, so a sheet reads alike.
#[test]
fn the_built_ins_write_their_numbers_their_own_way() {
    let library = DraftingProfile::library();
    let ids: Vec<&str> = library.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["iso_decimal_point", "iso_129_1", "asme_y14_5"]);
    assert_eq!(library[0], DraftingProfile::default());
    for p in &library {
        p.validate().unwrap();
        assert_eq!(p.version, 1);
    }
    assert_eq!(profile("iso_129_1").number.marker, DecimalMarker::Comma);
    assert_eq!(dimension(&DraftingProfile::default(), 1), "40.00 ±0.05");
    assert_eq!(dimension(&DraftingProfile::default(), 4), "4× Ø40.00 ±0.05");
    assert_eq!(dimension(&profile("iso_129_1"), 1), "40,00 ±0,05");
    for p in &library {
        assert_eq!((p.length_decimals, p.angle_decimals, p.tolerance_decimals), (2, 2, 2), "{}", p.id);
    }
    assert_eq!(dimension(&profile("asme_y14_5"), 1), "40 ±0.05");
    assert_eq!(dimension(&profile("asme_y14_5"), 4), "4X Ø40 ±0.05");
}

/// A length takes the length decimals and an angle the angle decimals; a number with no unit is
/// no dimension.
#[test]
fn a_dimension_takes_its_kinds_decimals() {
    let p = DraftingProfile { length_decimals: 3, angle_decimals: 0, tolerance_decimals: 4, ..DraftingProfile::default() };
    let length = p.style(Kind::Length, "in").unwrap();
    assert_eq!((length.unit.as_str(), length.decimals, length.tolerance_decimals, length.repeat), ("in", 3, 4, Repeat::Times));
    assert_eq!(p.style(Kind::Angle, "deg").unwrap().decimals, 0);
    assert!(p.style(Kind::Unitless, "").is_err());
}

/// A profile is saved whole and read back the same; a field this version doesn't know is refused,
/// not dropped.
#[test]
fn a_profile_is_saved_whole() {
    for p in DraftingProfile::library() {
        let saved = serde_json::to_value(&p).unwrap();
        assert_eq!(serde_json::from_value::<DraftingProfile>(saved.clone()).unwrap(), p);
        let mut newer = saved;
        newer["leader_style"] = json!("curved");
        assert!(serde_json::from_value::<DraftingProfile>(newer).is_err(), "{}", p.id);
    }
    let saved = serde_json::to_value(profile("asme_y14_5")).unwrap();
    assert_eq!(saved["repeat"], "x");
}

/// A built-in that changed after a drawing took its copy is reported with both versions, and
/// the copy keeps its own rules; a copy as the library has it, or a house profile, reports
/// nothing.
#[test]
fn a_changed_built_in_is_reported_and_never_applied() {
    let copy = profile("iso_129_1");
    assert_eq!(copy.differs_from_library(), None);
    let mut older = copy.clone();
    older.tolerance_decimals = 3;
    assert_eq!(older.differs_from_library(), Some((1, 1)));
    assert_eq!(older.style(Kind::Length, "mm").unwrap().tolerance_decimals, 3, "the copy keeps its own rules");
    let house = DraftingProfile { id: "workshop".into(), name: "Workshop".into(), edition: String::new(), ..copy };
    house.validate().unwrap();
    assert_eq!(house.differs_from_library(), None);
}

/// Every field's limits.
#[test]
fn a_profile_out_of_its_limits_is_refused() {
    let ok = DraftingProfile::default();
    for id in ["", "A", "1st", "_x", "iso-129", "iso 129", "é", &"a".repeat(65)] {
        refused(&DraftingProfile { id: id.into(), ..ok.clone() }, "id");
    }
    DraftingProfile { id: "a".repeat(64), ..ok.clone() }.validate().unwrap();
    refused(&DraftingProfile { version: 0, ..ok.clone() }, "version");
    for name in ["", "  ", "tab\there", &"n".repeat(129)] {
        refused(&DraftingProfile { name: name.into(), ..ok.clone() }, "name");
    }
    DraftingProfile { name: "ü".repeat(128), edition: "e".repeat(128), ..ok.clone() }.validate().unwrap();
    refused(&DraftingProfile { edition: "e".repeat(129), ..ok.clone() }, "edition");
    refused(&DraftingProfile { edition: "line\nbreak".into(), ..ok.clone() }, "edition");
    DraftingProfile { length_decimals: 8, angle_decimals: 8, tolerance_decimals: 8, ..ok.clone() }.validate().unwrap();
    refused(&DraftingProfile { length_decimals: 9, ..ok.clone() }, "8 decimals");
    refused(&DraftingProfile { angle_decimals: 9, ..ok.clone() }, "8 decimals");
    refused(&DraftingProfile { tolerance_decimals: u8::MAX, ..ok.clone() }, "8 decimals");
}

/// A profile gives as many decimals as the formatter takes, and no more: one cap for both
/// ([`MAX_DECIMALS`]), so no profile names a number of decimals that fails when a value is written.
#[test]
fn a_profile_gives_as_many_decimals_as_the_formatter_takes() {
    let most = DraftingProfile { length_decimals: MAX_DECIMALS, tolerance_decimals: MAX_DECIMALS, ..DraftingProfile::default() };
    most.validate().unwrap();
    assert_eq!(dimension(&most, 1), "40.00000000 ±0.05000000");
    refused(&DraftingProfile { length_decimals: MAX_DECIMALS + 1, ..DraftingProfile::default() }, "8 decimals");
    assert!(format_number(40.0, MAX_DECIMALS + 1, &NumberFormat::default()).is_err(), "the formatter's cap is the profile's");
}

fn random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// Profiles as a file might hold them (each field replaced by something hostile in turn, or
/// several together): reading and checking never panics, and every profile that reads and
/// passes the check is saved and read back the same.
#[test]
fn hostile_profiles_never_panic() {
    let mut seed = 0x4896_d09d_1a2b_3c4d;
    let hostile = [
        json!(null),
        json!(-1),
        json!(255),
        json!(256),
        json!(1e300),
        json!(f64::MIN_POSITIVE),
        json!(""),
        json!("\u{0}"),
        json!("x".repeat(70)),
        json!([]),
        json!({}),
        json!({"marker": "semicolon"}),
        json!(true),
        json!(u64::MAX),
    ];
    let fields = ["id", "version", "name", "edition", "number", "length_decimals", "angle_decimals", "tolerance_decimals", "repeat"];
    let base = serde_json::to_value(DraftingProfile::default()).unwrap();
    let (mut read, mut accepted) = (0, 0);
    for _ in 0..20_000 {
        let mut v = base.clone();
        for _ in 0..1 + random(&mut seed) % 3 {
            let field = fields[(random(&mut seed) % fields.len() as u64) as usize];
            let value = if random(&mut seed).is_multiple_of(4) {
                Value::from(random(&mut seed) % 12)
            } else {
                hostile[(random(&mut seed) % hostile.len() as u64) as usize].clone()
            };
            if random(&mut seed).is_multiple_of(10) {
                v.as_object_mut().unwrap().remove(field);
            } else {
                v[field] = value;
            }
        }
        let Ok(p) = serde_json::from_value::<DraftingProfile>(v) else { continue };
        read += 1;
        if p.validate().is_ok() {
            accepted += 1;
            assert_eq!(serde_json::from_value::<DraftingProfile>(serde_json::to_value(&p).unwrap()).unwrap(), p);
            let _ = p.style(Kind::Length, "mm");
        }
    }
    assert!(read > 1_000 && accepted > 500, "read {read}, accepted {accepted}");
}
