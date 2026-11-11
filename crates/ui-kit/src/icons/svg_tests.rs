//! The renderer on a tiny icon set of the tests' own, independent of any host's artwork.
use super::*;
use egui::Color32;

const ROLES: &[&str] = &["ink", "line", "passive-3", "gap", "guide", "active-3"];

const TEST_DATA: IconData = IconData {
    role_names: ROLES,
    role_source_hex: &["#34465b", "#34465b", "#8e9faf", "#ffffff", "#6f879c", "#236ead"],
    palette: [
        &[[0x34, 0x46, 0x5b], [0x34, 0x46, 0x5b], [0x8e, 0x9f, 0xaf], [0xff, 0xff, 0xff], [0x6f, 0x87, 0x9c], [0x23, 0x6e, 0xad]],
        &[[0x1f, 0x20, 0x21], [0xdb, 0xe4, 0xee], [0x54, 0x67, 0x82], [0x1e, 0x25, 0x30], [0x80, 0x93, 0xa4], [0x29, 0x76, 0xb8]],
    ],
    accent_names: &["warm", "cool"],
    accents: &[&[(5, [0xc0, 0x40, 0x10], [0xd0, 0x60, 0x20])], &[(5, [0x10, 0x80, 0x60], [0x20, 0x90, 0x70]), (4, [1, 2, 3], [4, 5, 6])]],
    accent_variants: &[],
    masters: &[
        ("placeholder", "model", "<svg><path d='M8 8L56 8L56 56L8 56Z' fill='#8e9faf'/></svg>"),
        ("bar", "model", "<svg><path d='M8 32L56 32' stroke='#236ead' stroke-width='8'/></svg>"),
        ("broken", "model", "<svg><path d='M 1'/></svg>"),
    ],
    aliases: &[("warm_bar", "bar", "warm"), ("lost", "missing", "")],
    icon_accents: &[("bar", "cool")],
    danger_accent: "warm",
};

static TEST_SET: IconSet = IconSet::new(TEST_DATA);

/// A host's masters over the test set: one replaces `bar`, one is new, one takes the name of the
/// set's `warm_bar` alias; a host alias replaces the set's `lost`.
const HOST: IconExtension = IconExtension {
    masters: &[
        ("bar", "model", "<svg><path d='M8 16L56 16' stroke='#236ead' stroke-width='8'/></svg>"),
        ("dot", "model", "<svg><circle cx='32' cy='32' r='12' fill='#236ead'/></svg>"),
        ("warm_bar", "model", "<svg><circle cx='20' cy='20' r='10' fill='#8e9faf'/></svg>"),
    ],
    aliases: &[("cool_dot", "dot", "cool"), ("lost", "bar", "")],
    icon_accents: &[("dot", "warm")],
};

static EXTENDED: IconSet = IconSet::extended(TEST_DATA, HOST);

fn role(name: &str) -> usize {
    ROLES.iter().position(|r| *r == name).unwrap()
}

#[test]
fn non_blue_dashed_strokes_use_guides_but_preserve_accents_and_knockouts() {
    let svg = "<svg><path d='M8 20L56 20' stroke='#8e9faf' stroke-width='4.25' stroke-dasharray='3 5'/><path d='M8 40L56 40' stroke='#ffffff' stroke-width='6' stroke-dasharray='3 5'/><path d='M8 52L56 52' stroke='#236ead' stroke-width='4.25' stroke-dasharray='3 5'/></svg>";
    let doc = TEST_SET.parse(svg).unwrap();
    let (guide, active) = (role("guide"), role("active-3"));
    let Item::Node(line) = &doc.items[0] else { panic!("expected line") };
    let Item::Node(knockout) = &doc.items[1] else { panic!("expected knockout") };
    let Item::Node(blue) = &doc.items[2] else { panic!("expected blue line") };
    assert_eq!(line.stroke, Paint::Role(guide));
    assert_eq!(knockout.stroke, Paint::Knockout);
    assert_eq!(blue.stroke, Paint::Role(active));
    for dark in [false, true] {
        for accent in std::iter::once(None).chain((0..TEST_SET.data().accent_names.len()).map(Some)) {
            let look = Look { dark, accent, vision: solidcraft_theme::ColorVision::Off, palette: None };
            assert_eq!(TEST_SET.role_rgb(look, guide), TEST_SET.data().palette[usize::from(dark)][guide]);
            let pm = render(&doc, look, 64).unwrap();
            let rgb = |x, y| {
                let p = pm.pixel(x, y).unwrap();
                [p.red(), p.green(), p.blue()]
            };
            assert_eq!(rgb(9, 20), TEST_SET.role_rgb(look, guide));
            assert_eq!(rgb(9, 52), TEST_SET.role_rgb(look, active));
        }
    }
}

#[test]
fn unknown_names_and_broken_masters_draw_the_placeholder() {
    for name in ["no_such_icon", "", "placeholder", "broken", "lost"] {
        let resolved = TEST_SET.resolve(name);
        assert!(resolved.placeholder, "{name}");
        assert_eq!(resolved.master, PLACEHOLDER);
        assert!(TEST_SET.doc_for(resolved.master, 64).is_some(), "{name}");
    }
    assert!(TEST_SET.doc_for("no_such_master", 16).is_some(), "an unknown master draws the placeholder");
    assert_eq!(TEST_SET.master_ids(), ["bar", "placeholder"], "a master that does not parse is left out");
}

#[test]
fn aliases_and_defaults_choose_the_accent() {
    let bar = TEST_SET.resolve("bar");
    assert_eq!((bar.master, bar.accent, bar.placeholder), ("bar", TEST_SET.accent_index("cool"), false));
    let warm = TEST_SET.resolve("warm_bar");
    assert_eq!((warm.master, warm.accent), ("bar", TEST_SET.accent_index("warm")));
    assert_eq!(TEST_SET.accent_index(""), None);
    assert_eq!(TEST_SET.accent_index("unknown"), None);
    let look = Look { dark: false, accent: warm.accent, vision: solidcraft_theme::ColorVision::Off, palette: None };
    assert_eq!(TEST_SET.role_colour("active-3", look), Color32::from_rgb(0xc0, 0x40, 0x10));
    assert_eq!(TEST_SET.role_colour("gap", look), Color32::TRANSPARENT);
    assert_eq!(TEST_SET.primary("warm_bar", true, solidcraft_theme::ColorVision::Off), Color32::from_rgb(0xd0, 0x60, 0x20));
}

#[test]
fn a_set_without_artwork_draws_nothing() {
    static EMPTY: IconSet = IconSet::new(IconData::EMPTY);
    let resolved = EMPTY.resolve("extrude");
    assert!(resolved.placeholder);
    assert!(EMPTY.doc_for(resolved.master, 64).is_none());
    assert!(EMPTY.master_ids().is_empty());
}

#[test]
fn hostile_artwork_is_an_error_not_a_panic() {
    for bad in [
        "",
        "<",
        "<svg",
        "<svg><path d=\"M 1\"/></svg>",
        "<svg><path d=\"X\"/></svg>",
        "<svg><g></svg>",
        "<svg><rect/></svg>",
        "<svg><path d=\"M 0 0 L 1 1\" fill=\"#123456\"/></svg>",
        "<svg><path d='M0 0 L 1 1' mask='url(#x)'/></svg>",
    ] {
        let _ = TEST_SET.parse(bad);
    }
    assert!(TEST_SET.parse("<svg><path d=\"M 1\"/></svg>").is_err());
    assert!(TEST_SET.parse("<svg><g></svg>").is_err());
    assert!(TEST_SET.parse("<svg><path d=\"M 0 0 L 1 1\" fill=\"#123456\"/></svg>").is_err(), "a colour outside the set's roles");
}

#[test]
fn an_extended_set_draws_the_hosts_masters_first_in_the_sets_palette() {
    let look = Look { dark: false, accent: None, vision: solidcraft_theme::ColorVision::Off, palette: None };
    let drawn = |set: &'static IconSet, name: &str| render(set.doc_for(set.resolve(name).master, 64).unwrap(), look, 64).unwrap().data().to_vec();
    let parsed = |src: &str| render(&EXTENDED.parse(src).unwrap(), look, 64).unwrap().data().to_vec();
    // The host's `bar` replaces the set's; the set's own masters stay.
    assert_eq!(drawn(&EXTENDED, "bar"), parsed(HOST.masters[0].2));
    assert_ne!(drawn(&EXTENDED, "bar"), drawn(&TEST_SET, "bar"));
    assert_eq!(drawn(&EXTENDED, "placeholder"), drawn(&TEST_SET, "placeholder"));
    assert_eq!(EXTENDED.master_ids(), ["bar", "dot", "placeholder", "warm_bar"]);
    // The host's names come first: its master over the set's alias, its alias over the set's.
    let warm_bar = EXTENDED.resolve("warm_bar");
    assert_eq!((warm_bar.master, warm_bar.accent, warm_bar.placeholder), ("warm_bar", None, false));
    assert!(TEST_SET.resolve("lost").placeholder);
    let lost = EXTENDED.resolve("lost");
    assert_eq!((lost.master, lost.placeholder), ("bar", false));
    // Default accents: the host's for its ids, the set's for the rest; aliases name theirs.
    assert_eq!(EXTENDED.resolve("dot").accent, EXTENDED.accent_index("warm"));
    assert_eq!(EXTENDED.resolve("bar").accent, EXTENDED.accent_index("cool"));
    let cool_dot = EXTENDED.resolve("cool_dot");
    assert_eq!((cool_dot.master, cool_dot.accent), ("dot", EXTENDED.accent_index("cool")));
    // One palette: the set's roles and accents.
    let warm = Look { accent: EXTENDED.accent_index("warm"), ..look };
    assert_eq!(EXTENDED.role_colour("active-3", warm), TEST_SET.role_colour("active-3", warm));
    assert_eq!(EXTENDED.primary("dot", true, solidcraft_theme::ColorVision::Off), Color32::from_rgb(0xd0, 0x60, 0x20));
    // The set it extends is unchanged.
    assert_eq!(TEST_SET.master_ids(), ["bar", "placeholder"]);
    assert_eq!(drawn(&TEST_SET, "warm_bar"), drawn(&TEST_SET, "bar"));
}
