//! Topological ranges and loop selection operate on editable elements, not triangles.
use super::*;
use std::collections::{BTreeMap, VecDeque};

#[derive(Default)]
pub(super) struct State {
    anchors: [Option<Element>; 3],
    pending: Option<(Element, bool)>,
    topology: u64,
}
fn kind(element: Element) -> usize {
    match element {
        Element::Vertex(_) => 0,
        Element::Edge(_) => 1,
        Element::Face(_) | Element::Surface => 2,
    }
}
type Graph = BTreeMap<Element, BTreeSet<Element>>;
fn connect(graph: &mut Graph, a: Element, b: Element) {
    if a != b {
        graph.entry(a).or_default().insert(b);
        graph.entry(b).or_default().insert(a);
    }
}
fn edge(a: usize, b: usize) -> Element {
    Element::Edge([a.min(b), a.max(b)])
}
fn graph(faces: &[Vec<usize>], element: Element) -> Option<Graph> {
    let mut uses: BTreeMap<Element, Vec<usize>> = BTreeMap::new();
    for (id, face) in faces.iter().enumerate() {
        for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
            uses.entry(edge(a, b)).or_default().push(id);
        }
    }
    let mut out = Graph::new();
    match element {
        Element::Surface => {
            out.entry(Element::Surface).or_default();
        }
        Element::Vertex(_) => {
            for e in uses.keys() {
                if let Element::Edge([a, b]) = *e {
                    connect(&mut out, Element::Vertex(a), Element::Vertex(b));
                }
            }
        }
        Element::Edge(_) => {
            let mut incident: BTreeMap<usize, Vec<Element>> = BTreeMap::new();
            for &e in uses.keys() {
                out.entry(e).or_default();
                if let Element::Edge([a, b]) = e {
                    incident.entry(a).or_default().push(e);
                    incident.entry(b).or_default().push(e);
                }
            }
            for neighbors in incident.values() {
                if neighbors.len() > 64 {
                    return None;
                }
                for (i, &a) in neighbors.iter().enumerate() {
                    for &b in neighbors.iter().skip(i + 1) {
                        connect(&mut out, a, b);
                    }
                }
            }
        }
        Element::Face(_) => {
            for (id, _) in faces.iter().enumerate().filter(|(_, f)| !f.is_empty()) {
                out.entry(Element::Face(id)).or_default();
            }
            for ids in uses.values().filter(|ids| ids.len() == 2) {
                if let (Some(&a), Some(&b)) = (ids.first(), ids.get(1)) {
                    connect(&mut out, Element::Face(a), Element::Face(b));
                }
            }
        }
    }
    Some(out)
}
fn distances(graph: &Graph, start: Element) -> BTreeMap<Element, usize> {
    let mut distance = BTreeMap::new();
    if !graph.contains_key(&start) {
        return distance;
    }
    distance.insert(start, 0);
    let mut queue = VecDeque::from([start]);
    while let Some(node) = queue.pop_front() {
        let depth = distance.get(&node).copied().unwrap_or(0) + 1;
        for &next in graph.get(&node).into_iter().flatten() {
            if let std::collections::btree_map::Entry::Vacant(entry) = distance.entry(next) {
                entry.insert(depth);
                queue.push_back(next);
            }
        }
    }
    distance
}
fn range(faces: &[Vec<usize>], first: Element, last: Element) -> Option<BTreeSet<Element>> {
    if kind(first) != kind(last) {
        return None;
    }
    let graph = graph(faces, first)?;
    let a = distances(&graph, first);
    let b = distances(&graph, last);
    let total = *a.get(&last)?;
    Some(a.into_iter().filter_map(|(node, d)| b.get(&node).is_some_and(|r| d + r == total).then_some(node)).collect())
}
fn edge_loop(faces: &[Vec<usize>], seed: Element) -> BTreeSet<Element> {
    let mut uses: BTreeMap<Element, BTreeSet<usize>> = BTreeMap::new();
    let mut incident: BTreeMap<usize, BTreeSet<Element>> = BTreeMap::new();
    for (id, face) in faces.iter().enumerate() {
        for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
            let e = edge(a, b);
            uses.entry(e).or_default().insert(id);
            incident.entry(a).or_default().insert(e);
            incident.entry(b).or_default().insert(e);
        }
    }
    if !uses.contains_key(&seed) {
        return BTreeSet::new();
    }
    let mut selected = BTreeSet::new();
    let mut pending = vec![seed];
    while let Some(e) = pending.pop() {
        if !selected.insert(e) {
            continue;
        }
        if let Element::Edge(vertices) = e {
            for v in vertices {
                let candidates: Vec<_> = incident
                    .get(&v)
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|other| *other != e && uses.get(&e).zip(uses.get(other)).is_some_and(|(a, b)| a.is_disjoint(b)))
                    .collect();
                if candidates.len() == 1
                    && let Some(&next) = candidates.first()
                {
                    pending.push(next);
                }
            }
        }
    }
    selected
}
pub(super) fn pressed(app: &mut SolveApp, hit: Element, shift: bool) {
    if let Some(form) = &app.session.forms_prototype {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        form.logical.faces.hash(&mut hash);
        let key = hash.finish();
        if app.forms_prototype.selection.topology != key {
            app.forms_prototype.selection = State { topology: key, ..Default::default() };
        }
    }
    app.forms_prototype.selection.pending = Some((hit, shift));
    if !shift {
        app.forms_prototype.selection.anchors[kind(hit)] = Some(hit);
    }
}
pub(super) fn sync(app: &mut SolveApp) {
    if let Some(form) = &app.session.forms_prototype {
        let vertices: BTreeSet<_> = form.logical.faces.iter().flatten().copied().collect();
        let edges: BTreeSet<_> = form.surface.boundaries.iter().map(|(e, _)| edge(e[0], e[1])).collect();
        app.forms_prototype.elements.retain(|e| match e {
            Element::Surface => form.profile.is_some(),
            Element::Face(id) => form.logical.faces.get(*id).is_some_and(|f| !f.is_empty()),
            Element::Vertex(id) => vertices.contains(id),
            Element::Edge([a, b]) => edges.contains(&edge(*a, *b)),
        });
        app.forms_prototype.selected = app.forms_prototype.elements.iter().flat_map(|e| e.vertices(&form.logical)).collect();
    }
}
pub(super) fn released(app: &mut SolveApp, toggle: Option<Element>) {
    let before = app.forms_prototype.elements.clone();
    let pending = app.forms_prototype.selection.pending.take();
    let mut extended = false;
    if let Some((hit, true)) = pending {
        let anchor = app.forms_prototype.selection.anchors[kind(hit)];
        if let Some(anchor) = anchor.filter(|a| *a != hit)
            && let Some(form) = &app.session.forms_prototype
            && let Some(elements) = range(&form.logical.faces, anchor, hit)
        {
            app.forms_prototype.elements.extend(elements);
            extended = true;
        } else {
            app.forms_prototype.selection.anchors[kind(hit)] = Some(hit);
        }
    }
    if !extended && let Some(e) = toggle {
        app.forms_prototype.elements.remove(&e);
    }
    if before != app.forms_prototype.elements {
        app.forms_prototype.pivot = None;
    }
    sync(app);
}
pub(super) fn expand(app: &mut SolveApp, connected: bool) {
    let Some(form) = &app.session.forms_prototype else { return };
    let mut selected = app.forms_prototype.elements.clone();
    if connected {
        for category in 0..3 {
            let seeds: Vec<_> = app.forms_prototype.elements.iter().copied().filter(|e| kind(*e) == category).collect();
            let Some(&first) = seeds.first() else { continue };
            let Some(graph) = graph(&form.logical.faces, first) else { continue };
            let mut covered = BTreeSet::new();
            for seed in seeds {
                if !covered.contains(&seed) {
                    covered.extend(distances(&graph, seed).into_keys());
                }
            }
            selected.extend(covered);
        }
    } else {
        let mut covered = BTreeSet::new();
        for &e in &app.forms_prototype.elements {
            if matches!(e, Element::Edge(_)) && !covered.contains(&e) {
                covered.extend(edge_loop(&form.logical.faces, e));
            }
        }
        selected.extend(covered);
    }
    app.forms_prototype.elements = selected;
    app.forms_prototype.pivot = None;
    sync(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranges_include_every_shortest_route_and_loops_stop_at_poles() {
        let faces: Vec<_> = (0..3).flat_map(|j| (0..3).map(move |i| vec![j * 4 + i, j * 4 + i + 1, (j + 1) * 4 + i + 1, (j + 1) * 4 + i])).collect();
        let selected = range(&faces, Element::Vertex(0), Element::Vertex(15)).unwrap();
        assert_eq!(selected.len(), 16);
        let selected = range(&faces, Element::Face(0), Element::Face(8)).unwrap();
        assert_eq!(selected.len(), 9);
        assert_eq!(edge_loop(&faces, edge(1, 5)).len(), 3);
        assert!(range(&faces, Element::Vertex(0), Element::Face(0)).is_none());
        let graph = graph(&faces, edge(0, 1)).unwrap();
        assert_eq!(distances(&graph, edge(0, 1)).len(), 24);
    }
}
