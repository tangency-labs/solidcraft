use super::*;

fn newer() -> Newer {
    Newer { format: 9, reads_up_to: 8, unknown_changes: Vec::new() }
}

/// What typed reading dropped is named and goes back on saving, where it was.
#[test]
fn dropped_fields_are_named_and_put_back() {
    let file = json!({"name": "A", "format": "app/9", "items": [{"id": 1, "size": 2, "colour": "red"}], "extra": {"x": 1}});
    let known = json!({"name": "A", "items": [{"id": 1, "size": 2}]});
    let mut said = NewerFile::new("App", &newer());
    said.compare(&file, &known, "placeholder", &[]);
    assert_eq!(said.not_understood, ["field `extra`", "field `items[1].colour`"]);
    let mut saved = json!({"name": "A", "items": [{"id": 1, "size": 3}]});
    assert!(said.restore_into(&mut saved).is_empty());
    assert_eq!(saved, json!({"name": "A", "items": [{"id": 1, "size": 3, "colour": "red"}], "extra": {"x": 1}}));
    assert_eq!(said.notice(), "Saved by a newer App (format 9, this version reads up to 8). If something's missing, update.");
}

/// A section that can't be read is left out, kept, and goes back while it is unchanged here.
#[test]
fn unreadable_sections_are_left_out_and_kept() {
    let mut top = match json!({"name": "A", "good": 1, "bad": 2}) {
        Value::Object(m) => m,
        _ => unreachable!(),
    };
    let mut said = NewerFile::new("App", &newer());
    leave_out_unreadable(&mut top, &mut said, "design", |key: &str, _: &Value| if key == "bad" { Err("design file: no") } else { Ok(()) });
    assert!(!top.contains_key("bad"));
    assert_eq!(said.not_understood, ["section `bad` (no)"]);
    let read =
        read_around(
            &mut top,
            &mut said,
            &["name"],
            "design",
            |top: &Map<String, Value>| {
                if top.contains_key("good") { Err("design file: clash") } else { Ok(top.len()) }
            },
        );
    assert_eq!(read, Ok(1));
    assert_eq!(said.not_understood, ["section `bad` (no)", "section `good` (clash)"]);
    let mut saved = json!({"name": "A"});
    said.compare(&saved.clone(), &saved.clone(), "placeholder", &[]);
    assert!(said.restore_into(&mut saved).is_empty());
    assert_eq!(saved, json!({"name": "A", "good": 1, "bad": 2}));
}

/// The element a search from the start of `list` finds for `anchor`, as each element looked for
/// its partner before lists were indexed. That is the first with the anchor's id, or the first
/// with its name, or the one at its place while the list keeps its length.
fn first_match(list: &[Value], anchor: &Anchor) -> Option<usize> {
    match anchor {
        Anchor::Id(id) => list.iter().position(|x| x.get("id").and_then(Value::as_u64) == Some(*id)),
        Anchor::Name(name) => list.iter().position(|x| x.get("name").and_then(Value::as_str) == Some(name.as_str())),
        Anchor::Index { at, of } => (list.len() == *of && *at < list.len()).then_some(*at),
    }
}

/// `len` elements from `seed` that repeat ids and names, lack them, have both, or have an id or a
/// name that anchors nothing (a negative or text id, a number for a name), with ids and names
/// below `spread`. Each has a field `x` that the read lists below leave out.
fn mixed_list(seed: &mut u64, len: usize, spread: u64) -> Vec<Value> {
    (0..len)
        .map(|at| {
            *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            let small = (*seed >> 40) % spread;
            match (*seed >> 33) % 10 {
                0 | 1 => json!({"id": small, "x": at}),
                2 | 3 => json!({"name": format!("n{small}"), "x": at}),
                4 | 5 => json!({"id": small, "name": format!("n{}", (small + 1) % spread), "x": at}),
                6 => json!({"id": -1, "name": format!("n{small}"), "x": at}),
                7 => json!({"id": "7", "name": 3, "x": at}),
                8 => json!(at),
                _ => json!({"x": at}),
            }
        })
        .collect()
}

/// The anchor comparing gives `element`, at `at` in a list of `of`.
fn anchor_at(element: &Value, at: usize, of: usize) -> Anchor {
    anchor_of(element).unwrap_or(Anchor::Index { at, of })
}

/// Each element of a list gets the partner a search from the start finds, now that a list is
/// indexed instead of searched for each element. The lists repeat ids and names, miss some, have
/// elements with both, and anchor by place the elements that have neither. A list longer than
/// [`SEARCHED_LEN`] is indexed once the first element looks in it by id or name, and not before,
/// and a shorter one never.
#[test]
fn each_element_gets_the_partner_a_search_from_the_start_finds() {
    let mut seed = 7;
    // Found by id, by name, by place, and not found.
    let mut seen = [0usize; 4];
    for (raw_len, known_len) in [(0, 5), (5, 0), (1, 1), (8, 8), (12, 9), (40, 40), (64, 23), (300, 300)] {
        for _ in 0..20 {
            let raw = mixed_list(&mut seed, raw_len, 8);
            let known = mixed_list(&mut seed, known_len, 6);
            let (mut lists, mut partners, mut by_id_or_name) = (Lists::default(), None, false);
            for (at, element) in raw.iter().enumerate() {
                let anchor = anchor_at(element, at, raw.len());
                let found = lists.find(&mut partners, &known, &anchor);
                assert_eq!(found, first_match(&known, &anchor), "{anchor:?} in {known:?}");
                by_id_or_name |= !matches!(anchor, Anchor::Index { .. });
                let indexed = by_id_or_name && known.len() > SEARCHED_LEN;
                assert_eq!(partners.is_some(), indexed, "{anchor:?} in {}", known.len());
                assert_eq!(lists.indexed, usize::from(indexed));
                let kind = match (&anchor, found) {
                    (_, None) => 3,
                    (Anchor::Id(_), _) => 0,
                    (Anchor::Name(_), _) => 1,
                    (Anchor::Index { .. }, _) => 2,
                };
                seen[kind] += 1;
            }
        }
    }
    assert!(seen.iter().all(|&n| n > 0), "{seen:?}");
}

/// The comparison as it was before lists were indexed, copied from it. Each element searched its
/// list from the start ([`first_match`]), and each path was written out whole. The placeholder
/// and container fields, which these tests don't use, are left out.
fn vanished_before(raw: &Value, known: &Value, path: &str, paths: &mut Vec<String>) -> Option<Kept> {
    match (raw, known) {
        (Value::Object(raw), Value::Object(known)) => {
            let mut kept = BTreeMap::new();
            for (key, value) in raw {
                if path.is_empty() && matches!(key.as_str(), "format" | "format_pending") {
                    continue;
                }
                let inner = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                match known.get(key) {
                    None => {
                        if !looks_default(value) {
                            paths.push(format!("field `{inner}`"));
                        }
                        kept.insert(key.clone(), Kept::Field(value.clone()));
                    }
                    Some(known) => {
                        if let Some(k) = vanished_before(value, known, &inner, paths) {
                            kept.insert(key.clone(), k);
                        }
                    }
                }
            }
            (!kept.is_empty()).then_some(Kept::Fields(kept))
        }
        (Value::Array(raw), Value::Array(known)) => {
            let mut items = Vec::new();
            for (at, element) in raw.iter().enumerate() {
                let anchor = anchor_at(element, at, raw.len());
                let Some(partner) = first_match(known, &anchor).and_then(|place| known.get(place)) else { continue };
                let label = match (element.get("name").and_then(Value::as_str), &anchor) {
                    (Some(name), _) => format!("{path}[{name}]"),
                    (None, Anchor::Id(id)) => format!("{path}[{id}]"),
                    _ => format!("{path}[{at}]"),
                };
                if let Some(k) = vanished_before(element, partner, &label, paths) {
                    items.push((anchor, k));
                }
            }
            (!items.is_empty()).then_some(Kept::Items(items))
        }
        _ => None,
    }
}

/// Putting kept fields back as it was before lists were indexed, copied from it. `moved` counts the
/// elements that found their partner somewhere else than in the list as it was when this put
/// started on it, because a name was put back into an earlier element.
fn apply_before(kept: &Kept, value: &mut Value, path: &str, lost: &mut Vec<String>, moved: &mut usize) {
    match kept {
        Kept::Field(_) => {}
        Kept::Fields(fields) => {
            let Some(object) = value.as_object_mut() else {
                lost.push(format!("field `{path}`"));
                return;
            };
            for (key, sub) in fields {
                let inner = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                match (sub, object.get_mut(key)) {
                    (Kept::Field(v), None) => {
                        object.insert(key.clone(), v.clone());
                    }
                    (Kept::Field(_), Some(_)) => {}
                    (sub, Some(inner_value)) => apply_before(sub, inner_value, &inner, lost, moved),
                    (Kept::Items(_), None) => {}
                    (_, None) => lost.push(format!("field `{inner}`")),
                }
            }
        }
        Kept::Items(items) => {
            let Some(list) = value.as_array_mut() else {
                lost.push(format!("field `{path}`"));
                return;
            };
            let first = list.clone();
            for (anchor, sub) in items {
                let place = first_match(list, anchor);
                *moved += usize::from(place != first_match(&first, anchor));
                match (place.and_then(|at| list.get_mut(at)), anchor) {
                    (Some(element), _) => apply_before(sub, element, path, lost, moved),
                    (None, Anchor::Index { at, .. }) => lost.push(format!("element {at} of `{path}`")),
                    (None, _) => {}
                }
            }
        }
    }
}

/// The next number from `seed`.
fn next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    *seed >> 33
}

/// `len` elements as [`mixed_list`] makes them, where an object may also hold a list `s` of such
/// elements, `depth` levels down. Some of those lists are empty.
fn nested_list(seed: &mut u64, len: usize, spread: u64, depth: u32) -> Vec<Value> {
    let mut list = mixed_list(seed, len, spread);
    for element in &mut list {
        let roll = next(seed);
        if depth > 0
            && !roll.is_multiple_of(3)
            && let Value::Object(fields) = element
        {
            let inner = nested_list(seed, (roll % 5) as usize, spread, depth - 1);
            fields.insert("s".into(), Value::Array(inner));
        }
    }
    list
}

/// What reading `raw` keeps of it, in every list. No `x`, and every third element with an id
/// without its name, so saving puts the name back.
fn read_of(raw: &[Value]) -> Vec<Value> {
    raw.iter()
        .enumerate()
        .map(|(at, element)| match element {
            Value::Object(fields) => {
                let mut read = fields.clone();
                read.remove("x");
                if at.is_multiple_of(3) && read.get("id").and_then(Value::as_u64).is_some() {
                    read.remove("name");
                }
                if let Some(Value::Array(inner)) = fields.get("s") {
                    read.insert("s".into(), Value::Array(read_of(inner)));
                }
                Value::Object(read)
            }
            other => other.clone(),
        })
        .collect()
}

/// `known` as this build writes it after an edit, from `seed`. Some of its lists lost their last
/// element, so their places no longer tell.
fn written_of(seed: &mut u64, known: &[Value]) -> Vec<Value> {
    let mut list: Vec<Value> = known
        .iter()
        .map(|element| match element {
            Value::Object(fields) => {
                let mut written = fields.clone();
                if let Some(Value::Array(inner)) = fields.get("s") {
                    written.insert("s".into(), Value::Array(written_of(seed, inner)));
                }
                Value::Object(written)
            }
            other => other.clone(),
        })
        .collect();
    if next(seed).is_multiple_of(3) {
        list.pop();
    }
    list
}

/// Comparing and saving keep, name and put back what they did when each element searched its
/// list from the start. The lists hold lists, their elements repeat ids and names (so many share
/// one partner and the lists it holds), saving puts names back that later elements look for, and
/// some lists are one element shorter when saved.
#[test]
fn comparing_and_saving_do_what_searching_each_list_from_the_start_did() {
    let mut seed = 11;
    let (mut kept, mut lost, mut moved) = (0, 0, 0);
    for len in [1, 2, 7, 30, 120] {
        for _ in 0..30 {
            let raw = nested_list(&mut seed, len, 6, 2);
            let known = read_of(&raw);
            let saved = written_of(&mut seed, &known);
            let (file, known, saved) = (json!({ "items": raw }), json!({ "items": known }), json!({ "items": saved }));
            let mut paths = Vec::new();
            let before = vanished_before(&file, &known, "", &mut paths);
            let mut said = NewerFile::new("App", &newer());
            said.compare(&file, &known, "placeholder", &[]);
            assert_eq!(said.kept, before);
            assert_eq!(said.not_understood, paths);
            let (mut expected, mut written, mut not_back) = (saved.clone(), saved, Vec::new());
            if let Some(k) = &before {
                apply_before(k, &mut expected, "", &mut not_back, &mut moved);
            }
            assert_eq!(said.restore_into(&mut written), not_back);
            assert_eq!(written, expected);
            kept += usize::from(before.is_some());
            lost += not_back.len();
        }
    }
    assert!(kept > 0 && lost > 0 && moved > 0, "{kept} {lost} {moved}");
}

/// A list that many elements share as their partner, because they repeat its element's id, is
/// indexed once when comparing and once when saving, not once for each of them. That keeps it as
/// fast as one element looking in it. A name put back into it for one element is then found when
/// the next element that shares it looks in it, as a search from the start found it.
#[test]
fn a_list_many_elements_share_is_indexed_once() {
    // Each element compares its own `s` with the first element's, which is long. Some of theirs
    // are empty, and some look for its last element.
    let mut raw = vec![json!({"id": 0, "s": (0..300).map(|i| json!({"id": i, "x": 1})).collect::<Vec<_>>()})];
    raw.extend((1..200).map(|i| match i % 3 {
        0 => json!({"id": 0, "s": []}),
        1 => json!({"id": 0, "s": [{"id": 299, "x": i}]}),
        _ => json!({"id": 0, "x": i}),
    }));
    let read = read_of(&raw);
    let (file, known) = (json!({ "items": raw }), json!({ "items": read }));
    let mut paths = Vec::new();
    let before = vanished_before(&file, &known, "", &mut paths);
    let mut walk = Compare::new("placeholder", &[]);
    let kept = vanished(&file, &known, &TOP, &mut walk);
    // `items`, and the first element's `s`.
    assert_eq!(walk.lists.indexed, 2);
    assert_eq!(kept, before);
    assert_eq!(walk.named.lines, paths);
    let Some(kept) = kept else { panic!("nothing kept") };
    let (mut written, mut expected, mut not_back) = (known.clone(), known.clone(), Vec::new());
    let mut put = Restore::default();
    apply(&kept, &mut written, &TOP, &mut put);
    assert_eq!(put.lists.indexed, 2);
    apply_before(&kept, &mut expected, "", &mut not_back, &mut 0);
    assert_eq!(written, expected);
    assert_eq!(put.named.lines, not_back);

    // Both elements share the first one's `s`, which is long enough to be indexed. The first puts
    // the name `a` back into its first element, so the second's `x` finds that one and stays out,
    // since it already has an `x`.
    let more: Vec<Value> = (10..10 + SEARCHED_LEN as u64).map(|id| json!({ "id": id })).collect();
    let s = |head: [Value; 2]| Value::Array(head.into_iter().chain(more.iter().cloned()).collect());
    let file = json!({"items": [
        {"id": 1, "s": s([json!({"id": 5, "name": "a", "x": 1}), json!({"name": "a"})])},
        {"id": 1, "s": [{"name": "a", "x": 2}]},
    ]});
    let known = json!({"items": [{"id": 1, "s": s([json!({"id": 5}), json!({"name": "a"})])}, {"id": 1, "s": [{"name": "a"}]}]});
    let mut said = NewerFile::new("App", &newer());
    said.compare(&file, &known, "placeholder", &[]);
    assert_eq!(said.not_understood, ["field `items[1].s[a].name`", "field `items[1].s[a].x`", "field `items[1].s[a].x`"]);
    let mut saved = known.clone();
    assert!(said.restore_into(&mut saved).is_empty());
    let first = s([json!({"id": 5, "name": "a", "x": 1}), json!({"name": "a"})]);
    assert_eq!(saved, json!({"items": [{"id": 1, "s": first}, {"id": 1, "s": [{"name": "a"}]}]}));
}

/// A list is indexed only when an element looks in it by its id or name and it is longer than
/// [`SEARCHED_LEN`], when comparing and when saving. Lists of numbers, lists whose elements are
/// told apart by place and short lists, which most of a design's lists are, take no index and no
/// room in the store.
#[test]
fn only_long_lists_looked_in_by_id_or_name_are_indexed() {
    let points: Vec<Value> = (0..50).map(|i| json!({"at": [i, 0, 1], "x": i})).collect();
    let mut long: Vec<Value> = (0..=SEARCHED_LEN).map(|i| json!({"id": i, "size": [1, 2], "x": 1})).collect();
    long.push(json!({"name": "b", "x": 2}));
    let short = vec![json!({"id": 7, "x": 1}), json!({"name": "c", "x": 1})];
    let pairs = vec![json!([0, 0]); 40];
    let file = json!({"points": points, "pairs": pairs, "long": long, "short": short});
    let known = json!({"points": read_of(&points), "pairs": pairs, "long": read_of(&long), "short": read_of(&short)});
    let mut walk = Compare::new("placeholder", &[]);
    let kept = vanished(&file, &known, &TOP, &mut walk);
    // `long` alone.
    assert_eq!((walk.lists.indexed, walk.lists.partners.len()), (1, 1));
    let mut paths = Vec::new();
    assert_eq!(kept, vanished_before(&file, &known, "", &mut paths));
    assert_eq!(walk.named.lines, paths);
    let Some(kept) = kept else { panic!("nothing kept") };
    let mut written = known.clone();
    let mut put = Restore::default();
    apply(&kept, &mut written, &TOP, &mut put);
    assert_eq!((put.lists.indexed, put.lists.partners.len()), (1, 1));
    assert_eq!(written, file);
}

/// Putting the elements left out of a top-level list back as it was before their names were
/// gathered, copied from it. Each element looked through the list for its name.
fn elements_before(elements: &[(String, Value)], value: &mut Value) -> Vec<String> {
    let mut lost = Vec::new();
    let Some(top) = value.as_object_mut() else { return lost };
    for (key, element) in elements {
        let name = element.get("name");
        match top.entry(key.clone()).or_insert_with(|| json!([])) {
            Value::Array(list) if name.is_none() || !list.iter().any(|x| x.get("name") == name) => list.push(element.clone()),
            _ => lost.push(format!("`{key}` element {}, replaced by what was made here", name.and_then(Value::as_str).unwrap_or("unnamed"))),
        }
    }
    lost
}

/// The elements left out of a top-level list (parameters this build can't read) go back on saving
/// as they did when each looked through the list for its name. One whose name the list has, from
/// this build or from an element put back before it, stays out and is named. The names are text,
/// numbers (0.0 and -0.0 are one name, 3 and 3.0 two), other values or missing, and one key holds
/// no list.
#[test]
fn left_out_elements_go_back_as_a_search_of_the_list_did() {
    let names = [
        json!("p0"),
        json!("p1"),
        json!("p2"),
        json!(3),
        json!(3.0),
        json!(0.0),
        json!(-0.0),
        json!(0),
        json!(null),
        json!({"a": 1}),
        json!([1, "p1"]),
    ];
    let mut seed = 5;
    let (mut back, mut clashed) = (0, 0);
    for _ in 0..60 {
        let mut said = NewerFile::new("App", &newer());
        for _ in 0..next(&mut seed) % 40 {
            let key = ["params", "other", "scalar", "fresh"][(next(&mut seed) % 4) as usize];
            let roll = next(&mut seed);
            let element = match (roll % 13) as usize {
                11 => json!({"x": roll}),
                12 => json!(roll),
                n => {
                    let mut element = json!({"x": roll});
                    element["name"] = names[n].clone();
                    element
                }
            };
            said.keep_element(key, element, format!("element of `{key}`"));
        }
        let written = json!({"name": "A", "params": [{"name": "p1"}, {"name": 3}, {"x": 1}, 7, {"name": 0.0}], "other": [], "scalar": 2});
        let (mut saved, mut expected) = (written.clone(), written);
        let not_back = elements_before(&said.elements, &mut expected);
        assert_eq!(said.restore_into(&mut saved), not_back);
        assert_eq!(saved, expected);
        back += said.elements.len() - not_back.len();
        clashed += not_back.iter().filter(|line| !line.starts_with("`scalar`")).count();
    }
    assert!(back > 0 && clashed > 0, "{back} {clashed}");
}

/// Each path is named whole, however long the names and keys in it, while the lines that name
/// fields fit in [`NAMED_BYTES`] together. The rest are counted in one more line, when comparing
/// and when saving.
#[test]
fn paths_are_named_whole_until_the_lines_fill_16_mib_then_counted() {
    // 1 MiB, as long as the longest face name a design reads back.
    let (name, key) = ("A".repeat(1 << 20), "K".repeat(1 << 20));
    let mut component = Map::new();
    component.insert("id".into(), json!(1));
    component.insert("name".into(), json!(name));
    for i in 0..20 {
        component.insert(format!("f{i:02}"), json!(1));
    }
    let file = json!({"a": {"b": 1}, "components": [component], "parts": [{"name": "p", "c": 1}]});
    let known = json!({"a": {}, "components": [{"id": 1, "name": name}], "parts": [{"name": "p"}]});
    let mut said = NewerFile::new("App", &newer());
    said.compare(&file, &known, "placeholder", &[]);
    let mut expected = vec!["field `a.b`".to_string()];
    expected.extend((0..15).map(|i| format!("field `components[{name}].f{i:02}`")));
    assert!(expected.iter().map(String::len).sum::<usize>() <= NAMED_BYTES);
    assert!(expected.iter().map(String::len).sum::<usize>() + expected[1].len() > NAMED_BYTES);
    // Five fields of the component and the part's.
    expected.push("6 more fields".into());
    assert_eq!(said.not_understood, expected);
    let mut saved = known.clone();
    assert!(said.restore_into(&mut saved).is_empty());
    assert_eq!(saved, file);

    // Elements told by their place, below a long key, can't go back into a list one shorter.
    let (mut top, mut read) = (Map::new(), Map::new());
    top.insert(key.clone(), Value::Array(vec![json!({"x": 1}); 20]));
    read.insert(key.clone(), Value::Array(vec![json!({}); 20]));
    let (file, known) = (Value::Object(top), Value::Object(read));
    let mut said = NewerFile::new("App", &newer());
    said.compare(&file, &known, "placeholder", &[]);
    let mut expected: Vec<String> = (0..15).map(|i| format!("field `{key}[{i}].x`")).collect();
    expected.push("5 more fields".into());
    assert_eq!(said.not_understood, expected);
    let mut saved = known.clone();
    if let Some(Value::Array(list)) = saved.get_mut(&key) {
        list.pop();
    }
    let mut expected: Vec<String> = (0..15).map(|i| format!("element {i} of `{key}`")).collect();
    expected.push("5 more fields or elements".into());
    assert_eq!(said.restore_into(&mut saved), expected);
}
