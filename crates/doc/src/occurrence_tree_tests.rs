use solidcore_record::Vec3;

use super::OccurrenceTree;
use crate::*;

/// `placements` as it walked before the tree: a scan of every occurrence for each component
/// visited. The tree must give exactly this, in this order, for every document.
fn scanning_placements(doc: &Document) -> Vec<(u64, Vec<u64>, Mat)> {
    let mut out = vec![(0, Vec::new(), IDENTITY)];
    let mut stack: Vec<(u64, Vec<u64>, Mat)> = vec![(0, Vec::new(), IDENTITY)];
    while let Some((comp, path, m)) = stack.pop() {
        if out.len() > MAX_PLACEMENTS {
            break;
        }
        if path.len() > MAX_PLACEMENT_DEPTH {
            continue;
        }
        for o in doc.occurrences.iter().filter(|o| o.parent == comp) {
            let mut p = path.clone();
            p.push(o.id);
            let w = mat_mul(&m, &o.transform);
            out.push((o.component, p.clone(), w));
            stack.push((o.component, p, w));
        }
    }
    out
}

/// `inside_component`'s walk as it was before the tree: is `parent` placed anywhere below `component`?
fn scanning_inside(doc: &Document, parent: u64, component: u64) -> bool {
    if doc.component_within(parent, component) {
        return true;
    }
    let mut stack = vec![component];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(c) = stack.pop() {
        if !seen.insert(c) || seen.len() > MAX_PLACEMENTS {
            continue;
        }
        for o in doc.occurrences.iter().filter(|o| o.parent == c) {
            if o.component == parent {
                return true;
            }
            stack.push(o.component);
        }
    }
    false
}

/// Every (parent, component) pair over the ids a document names, and one id it doesn't.
fn same_inside(doc: &Document) -> std::result::Result<(), String> {
    let mut ids: Vec<u64> = doc.occurrences.iter().flat_map(|o| [o.parent, o.component]).chain([0, 999]).collect();
    ids.sort_unstable();
    ids.dedup();
    for &parent in &ids {
        for &component in &ids {
            if doc.inside_component(parent, component) != scanning_inside(doc, parent, component) {
                return Err(format!("inside_component({parent}, {component})"));
            }
        }
    }
    Ok(())
}

/// An occurrence of `component` in `parent`, moved by its id so every path's transform differs.
fn occurrence(id: u64, component: u64, parent: u64) -> Occurrence {
    let t = Vec3::new(id as f64, (id % 7) as f64, 0.5 * (id % 3) as f64);
    Occurrence {
        id,
        component,
        parent,
        name: format!("C{component}:{id}"),
        transform: rigid(t, Vec3::ZERO, Vec3::Z, 0.01 * id as f64),
        grounded: false,
    }
}

fn with(occurrences: Vec<Occurrence>) -> Document {
    let mut doc = Document::new("tree");
    doc.occurrences = occurrences;
    doc
}

/// Component `k` placed once in component `k - 1`, `depth` levels below the root.
fn chain(depth: u64) -> Document {
    with((1..=depth).map(|k| occurrence(k, k, k - 1)).collect())
}

/// The documents whose walks must not move: the walk's boundaries and the hostile shapes a file
/// can hold.
fn shapes() -> Vec<(&'static str, Document)> {
    // 2 under the root, each holding 400 of component 3, each holding 150 of component 4: past
    // MAX_PLACEMENTS, so the walk stops part way.
    let mut wide = vec![occurrence(1, 1, 0), occurrence(2, 1, 0)];
    wide.extend((0..400).map(|i| occurrence(10 + i, 3, 1)));
    wide.extend((0..150).map(|i| occurrence(1000 + i, 4, 3)));
    vec![
        ("empty", with(Vec::new())),
        ("one level", with(vec![occurrence(1, 1, 0), occurrence(2, 2, 0), occurrence(3, 1, 0)])),
        ("depth 63", chain(63)),
        ("depth 64", chain(64)),
        ("depth 65", chain(65)),
        ("depth 66", chain(66)),
        ("a component twice under one parent", with(vec![occurrence(1, 1, 0), occurrence(2, 2, 1), occurrence(3, 2, 1), occurrence(4, 3, 2)])),
        ("an occurrence whose parent is missing", with(vec![occurrence(1, 1, 0), occurrence(2, 2, 99), occurrence(3, 3, 2)])),
        ("a cycle", with(vec![occurrence(1, 1, 0), occurrence(2, 2, 1), occurrence(3, 1, 2)])),
        ("a component inside itself", with(vec![occurrence(1, 1, 0), occurrence(2, 1, 1)])),
        ("duplicated occurrence ids", with(vec![occurrence(1, 1, 0), occurrence(1, 2, 0), occurrence(2, 3, 1), occurrence(2, 3, 2)])),
        ("children listed out of order", with(vec![occurrence(5, 2, 1), occurrence(1, 1, 0), occurrence(4, 3, 2), occurrence(2, 2, 1)])),
        ("past MAX_PLACEMENTS", with(wide)),
    ]
}

#[test]
fn the_walk_is_the_scanning_walk_for_every_shape() {
    for (name, doc) in shapes() {
        let walked = doc.placements();
        assert_eq!(walked, scanning_placements(&doc), "{name}");
        assert_eq!(OccurrenceTree::new(&doc).placements_with(|o| o.transform), walked, "{name}");
        if doc.occurrences.len() < 100 {
            assert_eq!(same_inside(&doc), Ok(()), "{name}");
        }
    }
}

#[test]
fn the_walk_stops_at_its_caps_as_before() {
    let deepest = |doc: &Document| doc.placements().iter().map(|(_, path, _)| path.len()).max();
    // A branch is walked to MAX_PLACEMENT_DEPTH levels and one past it (today's 65-id path), no further.
    assert_eq!(deepest(&chain(64)), Some(64));
    assert_eq!(deepest(&chain(66)), Some(MAX_PLACEMENT_DEPTH + 1));
    let (_, wide) = shapes().pop().unwrap();
    let n = wide.placements().len();
    assert!(n > MAX_PLACEMENTS && n <= MAX_PLACEMENTS + 400, "stops once past the cap: {n}");
    // A cycle ends at the depth cap, not in a loop.
    let (_, cycle) = shapes().into_iter().find(|(name, _)| *name == "a cycle").unwrap();
    assert_eq!(deepest(&cycle), Some(MAX_PLACEMENT_DEPTH + 1));
}

/// A component placed twice at each level doubles the paths per level: the walk stops at the cap
/// instead of listing them all. Here 18 levels give 2^18 paths down to the part at the bottom.
#[test]
fn occurrence_paths_stop_at_max_placements() {
    let mut doubling: Vec<Occurrence> = (1..=18).flat_map(|k| [occurrence(2 * k, k, k - 1), occurrence(2 * k + 1, k, k - 1)]).collect();
    doubling.push(occurrence(1000, 99, 18));
    let paths = with(doubling).occurrence_paths(1000);
    assert_eq!(paths.len(), MAX_PLACEMENTS);
    assert!(paths.iter().all(|p| p.len() == 19 && p.last() == Some(&1000)));
}

#[test]
fn children_are_listed_in_document_order() {
    let doc = with(vec![occurrence(5, 2, 1), occurrence(1, 1, 0), occurrence(4, 3, 1), occurrence(2, 2, 1), occurrence(3, 9, 7)]);
    let tree = OccurrenceTree::new(&doc);
    let ids = |c: u64| tree.children(c).iter().map(|o| o.id).collect::<Vec<_>>();
    assert_eq!(ids(1), [5, 4, 2]);
    assert_eq!(ids(0), [1]);
    assert_eq!(ids(7), [3], "an occurrence under a missing parent is still its child");
    assert!(ids(2).is_empty());
}

#[test]
fn a_walk_uses_the_transform_it_is_given() {
    let doc = with(vec![occurrence(1, 1, 0), occurrence(2, 2, 1)]);
    let moved = rigid(Vec3::new(0.0, 0.0, 9.0), Vec3::ZERO, Vec3::Z, 0.0);
    let walked = OccurrenceTree::new(&doc).placements_with(|o| if o.id == 1 { moved } else { o.transform });
    let child = &doc.occurrences[1].transform;
    assert_eq!(walked[1], (1, vec![1], moved));
    assert_eq!(walked[2], (2, vec![1, 2], mat_mul(&moved, child)));
}

/// splitmix64, so each run makes the same documents.
fn next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[test]
fn hostile_documents_walk_as_before() {
    // Few component ids and parents drawn from them, so repeats, missing parents, cycles and
    // duplicated ids all come up.
    let mut state = 4032;
    for round in 0..300 {
        let n = next(&mut state) % 40;
        let components = 1 + next(&mut state) % 12;
        let occurrences = (0..n)
            .map(|i| {
                let id = if next(&mut state).is_multiple_of(9) { i / 2 } else { i + 1 };
                let component = 1 + next(&mut state) % components;
                let parent = next(&mut state) % (components + 2);
                occurrence(id, component, parent)
            })
            .collect();
        let doc = with(occurrences);
        assert_eq!(doc.placements(), scanning_placements(&doc), "round {round}");
        assert_eq!(same_inside(&doc), Ok(()), "round {round}");
    }
}

/// A generated assembly of `top` modules, each holding `parts` parts, each part holding `pins`
/// pins: the shape of a large imported board.
fn assembly(top: u64, parts: u64, pins: u64) -> Document {
    let mut occurrences = Vec::new();
    let mut id = 0;
    let mut add = |component: u64, parent: u64, n: u64| {
        for _ in 0..n {
            id += 1;
            occurrences.push(occurrence(id, component, parent));
        }
    };
    add(1, 0, top);
    for part in 0..parts {
        add(10 + part, 1, 1);
        add(5, 10 + part, pins);
    }
    with(occurrences)
}

#[test]
#[ignore = "manual optimized walk microbenchmark; no timing assertion"]
fn benchmark_placements_walk() {
    // About 10,000 and 97,500 placements, under MAX_PLACEMENTS.
    for (top, parts, pins) in [(10, 100, 9), (39, 250, 9)] {
        let doc = assembly(top, parts, pins);
        let time = |walk: &dyn Fn() -> Vec<(u64, Vec<u64>, Mat)>| {
            let start = std::time::Instant::now();
            let walked = walk();
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            std::hint::black_box(walked);
            elapsed
        };
        let median = |mut ms: Vec<f64>| {
            ms.sort_by(f64::total_cmp);
            ms[ms.len() / 2]
        };
        let walked = doc.placements();
        assert_eq!(walked, scanning_placements(&doc));
        let (scan, tree) = (|| scanning_placements(&doc), || doc.placements());
        // Each order in turn, so neither walk always runs on a cache the other warmed.
        for scanning_first in [true, false] {
            let (mut old, mut new) = (Vec::new(), Vec::new());
            for _ in 0..9 {
                if scanning_first {
                    old.push(time(&scan));
                    new.push(time(&tree));
                } else {
                    new.push(time(&tree));
                    old.push(time(&scan));
                }
            }
            eprintln!(
                "{} occurrences, {} placements, {} first: scanning={:.3}ms tree={:.3}ms (median of 9, tree includes building it)",
                doc.occurrences.len(),
                walked.len(),
                if scanning_first { "scanning" } else { "tree" },
                median(old),
                median(new)
            );
        }
    }
}
