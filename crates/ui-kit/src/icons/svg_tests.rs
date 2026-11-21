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

/// An id resolves the same the first time and once remembered, and the icon audit still hears
/// of an id asked for again after it was remembered.
#[test]
fn a_remembered_icon_resolves_the_same_and_is_still_audited() {
    for name in ["bar", "warm_bar", "lost", "no_such_icon"] {
        let (first, again) = (TEST_SET.resolve(name), TEST_SET.resolve(name));
        assert_eq!((first.master, first.accent, first.placeholder), (again.master, again.accent, again.placeholder), "{name}");
    }
    audit::start();
    let _ = TEST_SET.resolve("warm_bar");
    let heard: Vec<String> = audit::take().into_iter().map(|r| r.name).collect();
    audit::stop();
    assert_eq!(heard, ["warm_bar"]);
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

static ADDING: IconSet = IconSet::new(TEST_DATA);

#[test]
fn added_masters_draw_in_the_sets_roles_and_never_replace_its_own() {
    let look = Look::themed(false, None, solidcraft_theme::ColorVision::Off);
    // Drawn before it is added: the placeholder; added: its own artwork, in the theme's roles.
    assert!(ADDING.resolve("plugin:a").placeholder);
    let dot = "<svg viewBox='0 0 64 64'><circle cx='32' cy='32' r='20' fill='#236ead'/></svg>";
    ADDING.add("plugin:a", dot).unwrap();
    let added = ADDING.resolve("plugin:a");
    assert_eq!((added.master, added.placeholder), ("plugin:a", false));
    let doc = ADDING.doc_for("plugin:a", 32).unwrap();
    let light = render(doc, look, 32).unwrap();
    let dark = render(doc, Look::themed(true, None, solidcraft_theme::ColorVision::Off), 32).unwrap();
    assert_ne!(light.data(), dark.data(), "the role follows the theme");
    assert_ne!(Some(light.data()), render(ADDING.doc_for(PLACEHOLDER, 32).unwrap(), look, 32).as_ref().map(|p| p.data()));
    // The set's own names, the placeholder and an empty name are not for adding.
    for name in ["bar", "warm_bar", "placeholder", ""] {
        assert!(ADDING.add(name, dot).is_err(), "{name}");
    }
    assert_eq!(ADDING.resolve("bar").accent, ADDING.accent_index("cool"), "bar is still the set's");
    // The first artwork added under a name stays.
    ADDING.add("plugin:a", "<svg><path d='M8 8L56 56' stroke='#34465b'/></svg>").unwrap();
    assert!(std::ptr::eq(ADDING.doc_for("plugin:a", 32).unwrap(), doc));
}

#[test]
fn added_artwork_outside_the_subset_is_refused() {
    let big = format!("<svg>{}</svg>", "<path d='M8 8L9 9' stroke='#34465b'/>".repeat(2000));
    for (name, src) in [
        ("colour", "<svg><circle cx='32' cy='32' r='20' fill='#ff0000'/></svg>".to_owned()),
        ("image", "<svg><image href='https://example.com/a.png' width='64' height='64'/></svg>".to_owned()),
        ("use", "<svg><use href='other.svg#a'/></svg>".to_owned()),
        ("script", "<svg><script>alert(1)</script></svg>".to_owned()),
        ("style", "<svg><style>path{fill:red}</style></svg>".to_owned()),
        ("text", "<svg><text x='1' y='1'>A</text></svg>".to_owned()),
        ("relative", "<svg><path d='m8 8l40 40' stroke='#34465b'/></svg>".to_owned()),
        ("deep", format!("<svg>{}{}</svg>", "<g>".repeat(40), "</g>".repeat(40))),
        ("big", big),
    ] {
        assert!(ADDING.add(&format!("plugin:bad:{name}"), &src).is_err(), "{name}");
        assert!(ADDING.resolve(&format!("plugin:bad:{name}")).placeholder, "{name}");
    }
}

#[test]
fn masks_and_clips_require_defined_local_references_when_present() {
    let defs = "<defs><mask id='m'><path d='M8 8L56 8L56 56Z' fill='white'/></mask><clipPath id='c'><path d='M8 8L56 8L56 56Z'/></clipPath></defs>";
    let svg = |clip: &str, mask: &str| format!("<svg>{defs}<g{clip}><path d='M8 8L56 56'{mask}/></g></svg>");
    for (clip, mask, expected) in [("", "", None), (" clip-path='url(#c)'", " mask='url(#m)'", Some(0))] {
        let doc = TEST_SET.parse(&svg(clip, mask)).unwrap();
        let Item::Group { clip, items, .. } = &doc.items[0] else { panic!("a group") };
        let Item::Node(node) = &items[0] else { panic!("a shape") };
        assert_eq!(*clip, expected);
        assert_eq!(node.mask, expected);
    }
    for value in ["", "none", "#c", "url(other.svg#c)", "url(https://example.com/icon.svg#c)", "url(#missing)", "url(#)", "url(#c"] {
        assert!(TEST_SET.parse(&svg(&format!(" clip-path='{value}'"), "")).is_err(), "clip-path={value}");
        assert!(TEST_SET.parse(&svg("", &format!(" mask='{value}'"))).is_err(), "mask={value}");
    }
}

#[test]
fn closing_tags_must_match_the_open_group_or_definition() {
    let frames = [("g", "g"), ("defs", "defs"), ("mask id='m'", "mask"), ("clipPath id='c'", "clipPath")];
    for (open, matching) in frames {
        for close in ["g", "defs", "mask", "clipPath"] {
            let svg = format!("<svg><{open}><path d='M8 8L56 56'/></{close}></svg>");
            assert_eq!(TEST_SET.parse(&svg).is_ok(), close == matching, "{svg}");
        }
    }
    let nested = "<svg><defs><mask id='m'><path d='M8 8L56 56' fill='white'/></mask></defs><g><g><path d='M8 8L56 56' mask='url(#m)'/></g></g></svg>";
    assert!(TEST_SET.parse(nested).is_ok(), "matched nested groups and definitions stay valid");
    assert!(TEST_SET.parse("<svg></g></svg>").is_err(), "a closing group needs an open frame");
}

#[test]
fn closing_a_path_clears_the_command_before_the_next_token() {
    for path in ["M8 8L56 8L56 56Z0", "M8 8L56 8L56 56Z 1 2", "M8 8L56 8L56 56Z,-1 .5"] {
        assert!(TEST_SET.parse(&format!("<svg><path d='{path}'/></svg>")).is_err(), "{path}");
    }
    for path in ["M8 8L56 8L56 56Z", "M8 8L56 8L56 56ZM16 16L24 24", "M8 8L56 8L56 56ZL8 56"] {
        let doc = TEST_SET.parse(&format!("<svg><path d='{path}'/></svg>")).unwrap();
        assert_eq!(doc.items.len(), 1, "Z at the end or followed by a new command stays valid: {path}");
    }
}

#[test]
fn cyclic_masks_are_rejected_before_rendering_and_acyclic_reuse_still_draws() {
    let shape = "<path d='M8 8L56 8L56 56Z' fill='white' mask='url(#a)'/>";
    let self_ref = format!("<svg><defs><mask id='a'>{shape}</mask></defs></svg>");
    assert!(TEST_SET.parse(&self_ref).unwrap_err().contains("cyclic"));
    // The inner definition refers to its active parent; the parent then refers back to it.
    let cycle = format!("<svg><defs><mask id='a'><mask id='b'>{shape}</mask><path d='M8 8L56 56' mask='url(#b)'/></mask></defs></svg>");
    assert!(TEST_SET.parse(&cycle).unwrap_err().contains("cyclic"));
    let valid = "<svg><defs><mask id='a'><path d='M8 8L56 8L56 56Z' fill='white'/></mask><mask id='b'><path d='M8 8L56 8L56 56Z' fill='white' mask='url(#a)'/></mask></defs><path d='M8 8L56 8L56 56Z' fill='currentColor' mask='url(#b)'/><path d='M8 8L56 8L56 56Z' fill='currentColor' mask='url(#a)'/></svg>";
    let doc = TEST_SET.parse(valid).unwrap();
    let image = render(&doc, Look::themed(false, None, solidcraft_theme::ColorVision::Off), 64).unwrap();
    assert!(image.data().as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0));
}

#[test]
fn acyclic_mask_expansion_and_dependency_depth_have_fixed_budgets() {
    let icon = |levels: usize, copies: usize| {
        let mut svg = String::from("<svg><defs><mask id='m0'><path d='M8 8L56 8L56 56Z' fill='white'/></mask>");
        for level in 1..=levels {
            svg.push_str(&format!("<mask id='m{level}'>"));
            for _ in 0..copies {
                svg.push_str(&format!("<path d='M8 8L56 8L56 56Z' fill='white' mask='url(#m{})'/>", level - 1));
            }
            svg.push_str("</mask>");
        }
        svg.push_str(&format!("</defs><path d='M8 8L56 8L56 56Z' fill='currentColor' mask='url(#m{levels})'/></svg>"));
        svg
    };
    assert!(TEST_SET.parse(&icon(4, 2)).is_ok(), "ordinary repeated shared masks remain supported");
    assert!(TEST_SET.parse(&icon(16, 2)).unwrap_err().contains("budget"), "small DAGs must not expand exponentially at draw time");
    assert!(TEST_SET.parse(&icon(64, 1)).unwrap_err().contains("deeply"), "a flat XML list must not create unbounded renderer recursion");
    let mut repeated = icon(4, 2);
    let node = "<path d='M8 8L56 8L56 56Z' mask='url(#m4)'/>";
    repeated = repeated.replace("</svg>", &format!("{}</svg>", node.repeat(32)));
    assert!(TEST_SET.parse(&repeated).unwrap_err().contains("budget"), "charge each top-level visit, too");
}
