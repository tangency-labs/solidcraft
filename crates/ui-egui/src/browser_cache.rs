//! Stable browser data, indexed once for an evaluated document and borrowed on redraw.
use super::*;
use solidcraft_engine::doc::{Document, ModelState};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Weak};

#[derive(Default)]
struct Component {
    definition: Option<usize>,
    first: Option<Vec<u64>>,
}

#[derive(Default)]
pub(super) struct Node {
    pub component: u64,
    pub occurrence: u64,
    pub name: String,
    pub label: String,
    pub key: String,
    pub tooltip: String,
    pub grounded: bool,
    pub first: bool,
    pub children: Vec<Vec<u64>>,
    pub hover_bodies: Option<Vec<String>>,
    pub folders: [Option<Folder>; 3],
}

/// A row of a folder or group: an item, or a group with its own rows.
pub(super) enum Row {
    Item(Box<Entry>),
    Group(Group),
}

impl Row {
    pub fn count(&self) -> usize {
        match self {
            Row::Item(_) => 1,
            Row::Group(g) => g.count,
        }
    }
}

pub(super) struct Group {
    pub id: u64,
    pub key: String,
    pub label: String,
    /// How many items it holds, groups inside it included.
    pub count: usize,
    pub rows: Vec<Row>,
}

fn collect<'a>(rows: &'a [Row], out: &mut Vec<&'a Entry>) {
    for r in rows {
        match r {
            Row::Item(e) => out.push(e),
            Row::Group(g) => collect(&g.rows, out),
        }
    }
}

impl Group {
    /// Everything in the group, groups inside it included, in display order.
    pub fn entries(&self) -> Vec<&Entry> {
        let mut all = Vec::new();
        collect(&self.rows, &mut all);
        all
    }

    /// Does `test` hold for anything in the group, groups inside it included? Allocates nothing:
    /// a search asks this of a group each frame.
    pub fn any_entry(&self, test: &impl Fn(&Entry) -> bool) -> bool {
        self.rows.iter().any(|r| match r {
            Row::Item(e) => test(e),
            Row::Group(g) => g.any_entry(test),
        })
    }
}

#[derive(Default)]
pub(super) struct Folder {
    pub key: String,
    pub rows: Vec<Row>,
    /// The top-level rows as drawn: the order a drop among them keeps.
    pub top_order: Vec<solidcraft_engine::doc::BrowserEntry>,
    /// How many items the folder holds, groups included.
    pub count: usize,
}

impl Folder {
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        let mut all = Vec::new();
        collect(&self.rows, &mut all);
        all.into_iter()
    }
}

#[derive(Default)]
pub(super) struct Cache {
    doc: Weak<Document>,
    model: Weak<ModelState>,
    sketch: Option<u64>,
    colors: [Color32; 2],
    components: BTreeMap<u64, Component>,
    ids: BTreeMap<u64, usize>,
    scheduled: usize,
    pub root: Node,
    pub root_bodies: Vec<String>,
    pub occurrences: HashMap<Vec<u64>, Node>,
}

impl Cache {
    pub fn prepare(&mut self, app: &SolveApp) {
        let state = app.session.model.state();
        let t = Tokens::get();
        let colors = [t.error, t.sketch_accent];
        if std::ptr::eq(self.doc.as_ptr(), Arc::as_ptr(&app.session.doc))
            && std::ptr::eq(self.model.as_ptr(), Arc::as_ptr(&state))
            && self.sketch == app.session.active_sketch
            && self.colors == colors
        {
            return;
        }
        self.components.clear();
        self.ids.clear();
        self.occurrences.clear();
        for (i, component) in app.session.doc.components.iter().enumerate() {
            self.components.entry(component.id).or_default().definition = Some(i);
        }
        for (i, occurrence) in app.session.doc.occurrences.iter().enumerate() {
            self.ids.insert(occurrence.id, i);
        }
        self.root = Node {
            name: component_name(app, 0),
            key: "c0".into(),
            children: self.children(app, 0, &[], solidcraft_engine::doc::MAX_PLACEMENTS),
            ..Default::default()
        };
        self.scheduled = self.root.children.len();
        self.root_bodies = crate::context_menu::component_bodies(app, 0);
        // Weak snapshot identities catch replacement and reevaluation without retaining a
        // closed design's geometry while the browser is collapsed.
        self.doc = Arc::downgrade(&app.session.doc);
        self.model = Arc::downgrade(&state);
        self.sketch = app.session.active_sketch;
        self.colors = colors;
    }

    fn children(&self, app: &SolveApp, component: u64, path: &[u64], remaining: usize) -> Vec<Vec<u64>> {
        let Some(children) = app.tree.model.as_ref().and_then(|model| model.children.get(&component)) else { return Vec::new() };
        if path.len() >= 64 {
            return Vec::new();
        }
        children
            .iter()
            .filter(|id| !path.contains(id))
            .take(remaining)
            .map(|id| {
                let mut child = Vec::with_capacity(path.len() + 1);
                child.extend_from_slice(path);
                child.push(*id);
                child
            })
            .collect()
    }

    pub fn take_occurrence(&mut self, app: &mut SolveApp, path: &[u64]) -> Option<(Vec<u64>, Node)> {
        if let Some(cached) = self.occurrences.remove_entry(path) {
            return Some(cached);
        }
        if path.is_empty() || path.len() > 64 {
            return None;
        }
        let doc = self.doc.upgrade()?;
        let component = model_placement(app, path)?.component;
        let occurrence = doc.occurrences.get(*self.ids.get(path.last()?)?)?;
        let index = self.components.get_mut(&component)?;
        let definition = doc.components.get(index.definition?)?;
        let first = index.first.get_or_insert_with(|| doc.first_path(component)) == path;
        let mut tooltip = format!("Component: {}\nPlacement: ", definition.name);
        for (i, id) in path.iter().enumerate() {
            if i != 0 {
                tooltip.push_str(" / ");
            }
            tooltip.push_str(&doc.occurrences.get(*self.ids.get(id)?)?.name);
        }
        tooltip.push_str("\nGeometry edits apply to every instance of this component.");
        let children = self.children(app, component, path, solidcraft_engine::doc::MAX_PLACEMENTS.saturating_sub(self.scheduled));
        self.scheduled += children.len();
        Some((
            path.to_vec(),
            Node {
                component,
                occurrence: occurrence.id,
                name: occurrence.name.clone(),
                label: crate::dialogs_part_size::browser_label(app, component, &occurrence.name),
                key: occurrence_key(path),
                tooltip,
                grounded: occurrence.grounded,
                first,
                children,
                hover_bodies: None,
                folders: Default::default(),
            },
        ))
    }

    pub fn folder(&self, app: &SolveApp, component: u64, path: Option<&[u64]>, folder: &str) -> Folder {
        let Some(model) = &app.tree.model else { return Folder::default() };
        let Some(state) = self.model.upgrade() else { return Folder::default() };
        let Some(doc) = self.doc.upgrade() else { return Folder::default() };
        let prefix = path.map(occurrence_key).unwrap_or_else(|| format!("c{component}"));
        let indices = match folder {
            "bodies" => &model.bodies,
            "sketches" => &model.sketches,
            _ => &model.planes,
        };
        // A folder that holds only groups (a New Group made before anything is in it) is still
        // drawn, or the group can't be seen or filled (#10526).
        let grouped = doc.browser_groups.iter().any(|g| g.component == component && g.folder == folder);
        let none = Vec::new();
        let indices = match indices.get(&component) {
            Some(indices) => indices,
            None if grouped => &none,
            None => return Folder::default(),
        };
        let mut all = Vec::with_capacity(indices.len());
        for i in indices {
            let entry = match folder {
                "bodies" => state.bodies.get(*i).filter(|b| !app.session.body_removed_later(&b.name)).map(|b| {
                    let world = path.map_or_else(|| b.name.clone(), |p| doc.world_body_name(&state, &b.name, component, p));
                    Entry::body(app, b, world)
                }),
                "sketches" => doc.features.get(*i).map(|f| Entry::sketch(app, f)),
                _ => doc.features.get(*i).map(|f| Entry::plane(app, f)),
            };
            if let Some(mut entry) = entry {
                if path.is_some() && folder != "bodies" {
                    entry.row_key = format!("{prefix}/{}", entry.row_key);
                    if let Some(key) = &mut entry.patterns {
                        *key = format!("{prefix}/{key}");
                    }
                }
                all.push(entry);
            }
        }
        if all.is_empty() && !grouped {
            return Folder::default();
        }
        let rows = arrange(&doc, component, folder, &prefix, all);
        let top_order = rows
            .iter()
            .map(|r| match r {
                Row::Item(e) => solidcraft_engine::doc::BrowserEntry::item(&e.key),
                Row::Group(g) => solidcraft_engine::doc::BrowserEntry::Group(g.id),
            })
            .collect();
        let count = rows.iter().map(Row::count).sum();
        Folder { key: format!("{prefix}/{folder}"), rows, top_order, count }
    }
}
