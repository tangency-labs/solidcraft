//! Standard size tests. Each size's section, built from its dimensions, is held to the area its
//! published table gives; the tables round to three significant figures, so within 0.5 %.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::section;
use super::*;

/// The area of `s`'s built section, mm².
fn built_area(s: &StandardSize) -> f64 {
    section(&s.profile.region().unwrap_or_else(|e| panic!("{}: {e}", s.name))).area
}

#[test]
fn every_size_builds_with_its_published_area() {
    let all = standard_sizes();
    for family in Family::ALL {
        assert!(all.iter().any(|s| s.family == family), "{family:?} has sizes");
    }
    for s in &all {
        let got = built_area(s);
        let off = (got / s.area - 1.0).abs();
        assert!(off <= 0.005, "{}: built {got:.1} mm², the table {:.1} mm² ({:.2} % off)", s.name, s.area, off * 100.0);
    }
    // No two sizes share a name.
    let mut names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
    names.sort_unstable();
    let count = names.len();
    names.dedup();
    assert_eq!(names.len(), count, "names are unique");
}

#[test]
fn the_taper_flange_channels_bend_as_published() {
    // BS 4:1993 (roymech.org, chanA_dimprop): Ix, Iy (cm⁴) and the centroid's distance from the
    // back of the web Cy (cm). With parallel flanges Iy would be 15 % off; with the 5° slope it
    // is within the table's rounding.
    for (name, ix, iy, cy) in [("RSC 203x89", 2492.0, 265.0, 2.65), ("RSC 152x76", 852.0, 114.0, 2.21), ("RSC 432x102", 21373.0, 627.0, 2.31)] {
        let size = find(name).unwrap_or_else(|| panic!("{name}"));
        let s = section(&size.profile.region().unwrap());
        let back = -size.profile.size().0 / 2.0;
        for (what, got, want) in [("Ix", s.ixx / 1e4, ix), ("Iy", s.iyy / 1e4, iy), ("Cy", (s.centroid[0] - back) / 10.0, cy)] {
            assert!((got / want - 1.0).abs() <= 0.005, "{name} {what}: {got:.3}, the table {want}");
        }
    }
}

#[test]
fn sizes_are_found_by_the_names_drawings_use() {
    for (asked, found) in [
        ("RSC 203x89", "RSC 203x89"),
        ("RSC203x89", "RSC 203x89"),
        ("rsc 203 × 89", "RSC 203x89"),
        ("RSC 203x89x30", "RSC 203x89"),
        ("RSC152x76", "RSC 152x76"),
        ("PFC 200x90x30", "PFC 200x90x30"),
        ("PFC 200x90", "PFC 200x90x30"),
        ("RSA50x50x5", "RSA 50x50x5"),
        ("EA 50x50x5", "RSA 50x50x5"),
        ("L 50x50x5.0", "RSA 50x50x5"),
        ("SHS 100x100x5", "SHS 100x100x5"),
        // A square named as an RHS, as some libraries name it.
        ("RHS100x100x5", "SHS 100x100x5"),
        ("RHS 200x100x6.3", "RHS 200x100x6.3"),
        ("CHS42.4x3.2", "CHS 42.4x3.2"),
        ("CHS 48.3x3.2", "CHS 48.3x3.2"),
    ] {
        assert_eq!(find(asked).map(|s| s.name), Some(found.to_string()), "{asked}");
    }
    for asked in ["", "RSC", "203x89", "RSC 203x89x31", "RSC 203", "XYZ 1x1", "RSC 203xx89", "RSC 1e400x89", &"9".repeat(100), "RSC 203x89 y"] {
        assert!(find(asked).is_none(), "{asked:?} is no size");
    }
}

#[test]
fn sizes_carry_their_dimensions() {
    let rsc = find("RSC 203x89").unwrap();
    assert_eq!(
        rsc.profile,
        Profile::Channel {
            height: 203.2,
            width: 88.9,
            web: 8.1,
            flange: 12.9,
            root_radius: Some(13.7),
            toe_radius: Some(3.2),
            flange_slope: Some(RSC_FLANGE_SLOPE)
        }
    );
    // An RHS is named depth by width: the depth is its height.
    let rhs = find("RHS 200x100x6.3").unwrap();
    assert_eq!(rhs.profile, Profile::RectTube { width: 100.0, height: 200.0, wall: 6.3, outer_radius: Some(1.5 * 6.3), inner_radius: Some(6.3) });
    let rsa = find("RSA 50x50x5").unwrap();
    assert_eq!(rsa.profile, Profile::Angle { width: 50.0, height: 50.0, thickness: 5.0, root_radius: Some(7.0), toe_radius: Some(3.5) });
    assert!(!rsa.additional);
    assert!(find("RSA 150x150x18").unwrap().additional);
    assert_eq!(find("CHS 42.4x3.2").unwrap().profile, Profile::RoundTube { diameter: 42.4, wall: 3.2 });
}
