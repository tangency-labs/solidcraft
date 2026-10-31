//! What the user had selected in the model, as context for a message ("Attach selection", #2286).
//! The selection is described when the message is sent: faces and edges with the names and
//! numbers the agent's tools take (and a size and place that survive an edit), features by name and
//! kind, bodies by name. The conversation shows the same items as chips; the agent gets them as a
//! block of text after the message.

use std::collections::HashMap;

use serde_json::{Value, json};
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::{Sel, Session};

/// The most items one message carries; the rest are counted, not dropped silently.
pub const MAX_ITEMS: usize = 64;

/// What an item is, for grouping the chips: a face, edge or vertex; a step of the timeline; a body
/// or a placed component; anything else (planes, axes, sketch geometry).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Element,
    Feature,
    Body,
    Other,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Element, Kind::Feature, Kind::Body, Kind::Other];

    /// The message id of the group's heading.
    pub fn heading(self) -> &'static str {
        match self {
            Kind::Element => "ui.agent_chat_ctx_elements",
            Kind::Feature => "ui.agent_chat_ctx_features",
            Kind::Body => "ui.agent_chat_ctx_bodies",
            Kind::Other => "ui.agent_chat_ctx_other",
        }
    }

    fn english(self) -> &'static str {
        match self {
            Kind::Element => "Elements",
            Kind::Feature => "Features",
            Kind::Body => "Bodies",
            Kind::Other => "Other",
        }
    }
}

/// One selected thing: its short name for a chip, and what is known about it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub kind: Kind,
    pub label: String,
    pub detail: String,
    /// What it stands for in the model (one selection item, or the pieces of a rim): to light it up
    /// and to tell later whether it is still there.
    pub sels: Vec<Sel>,
    /// The persistent names of its faces or edges: they survive an edit, so an item whose names are
    /// all gone was changed away or deleted.
    pub names: Vec<String>,
    /// The pill in the message it was picked for (`«n: …»`), 0 for what is not placed in the text
    /// (Attach selection).
    pub group: u32,
}

impl Entry {
    /// Whether both stand for the same thing, whichever pill each is in.
    pub fn same_item(&self, other: &Entry) -> bool {
        self.kind == other.kind && self.label == other.label && self.detail == other.detail && self.sels == other.sels
    }

    fn new(kind: Kind, label: String, detail: String) -> Entry {
        Entry { kind, label, detail, sels: Vec::new(), names: Vec::new(), group: 0 }
    }
}

/// The things a message was sent with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Context {
    pub items: Vec<Entry>,
    /// Selected things past [`MAX_ITEMS`], not described.
    pub omitted: usize,
    /// The pills of the message: their numbers and labels, in reading order.
    pub groups: Vec<(u32, String)>,
}

impl Context {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.omitted == 0
    }

    /// Everything counted, listed or not.
    pub fn total(&self) -> usize {
        self.items.len() + self.omitted
    }

    /// Add the items of another message (queued messages go as one prompt), up to the limit.
    pub fn merge(&mut self, other: Context) {
        self.omitted += other.omitted;
        for group in other.groups {
            if !self.groups.iter().any(|(n, _)| *n == group.0) {
                self.groups.push(group);
            }
        }
        for item in other.items {
            if self.items.len() < MAX_ITEMS {
                self.items.push(item);
            } else {
                self.omitted += 1;
            }
        }
    }

    /// These items as the pick the pill `id` stands for.
    pub fn in_group(mut self, id: u32, label: &str) -> Context {
        for item in &mut self.items {
            item.group = id;
        }
        self.groups = vec![(id, label.to_string())];
        self
    }

    /// The same with every pill number raised by `by`: a queued message joining an earlier one in
    /// one prompt must not reuse its numbers.
    pub fn shifted(mut self, by: u32) -> Context {
        for item in &mut self.items {
            if item.group > 0 {
                item.group = item.group.saturating_add(by);
            }
        }
        for group in &mut self.groups {
            group.0 = group.0.saturating_add(by);
        }
        self
    }

    /// The same with the groups renumbered: `order` lists the old number of each group that is
    /// still in the text, in reading order (it becomes 1, 2, ...); the items of any other group
    /// are not placed any more.
    pub fn regrouped(mut self, order: &[u32]) -> Context {
        let number = |old: u32| order.iter().position(|o| *o == old).map(|n| u32::try_from(n + 1).unwrap_or(u32::MAX)).unwrap_or(0);
        let labels: Vec<(u32, String)> = order
            .iter()
            .enumerate()
            .filter_map(|(n, old)| {
                let label = self.groups.iter().find(|(g, _)| g == old)?.1.clone();
                Some((u32::try_from(n + 1).unwrap_or(u32::MAX), label))
            })
            .collect();
        for item in &mut self.items {
            if item.group > 0 {
                item.group = number(item.group);
            }
        }
        self.groups = labels;
        self
    }

    /// The highest pill number.
    pub fn last_group(&self) -> u32 {
        self.groups.iter().map(|(n, _)| *n).max().unwrap_or(0)
    }

    /// The items of one group (0: those not placed in the text), a section per kind.
    fn sections(&self, group: u32, out: &mut String) {
        for kind in Kind::ALL {
            let mut lines = self.items.iter().filter(|i| i.kind == kind && i.group == group).peekable();
            if lines.peek().is_none() {
                continue;
            }
            out.push_str(&format!("\n{}:", kind.english()));
            for item in lines {
                out.push_str(&format!("\n- {}", item.label));
                if !item.detail.is_empty() {
                    out.push_str(&format!(": {}", item.detail));
                }
            }
        }
    }

    /// The block of text the agent gets after the message, `""` when nothing was selected. Picks
    /// made at a place in the text come numbered, as the marks in the text are.
    pub fn prompt_block(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        let placed = !self.groups.is_empty();
        let mut out = format!("[Selected in SolidCraft when this message was sent: {} item(s). ", self.total());
        if placed {
            out.push_str("A mark like «1: …» in the message stands for the numbered group below, at the place where the user picked it. ");
        }
        out.push_str(
            "A face or edge name stays the same when the model is edited; indices and points are as they were then (`model.faces` and `model.edges` list the current ones).]",
        );
        if placed {
            let mut groups = self.groups.clone();
            groups.sort_by_key(|(n, _)| *n);
            for (n, label) in groups {
                // The same mark as in the message (made safe the same way).
                out.push('\n');
                out.push_str(&super::pills::marker(n, &label));
                if self.items.iter().any(|i| i.group == n) {
                    self.sections(n, &mut out);
                } else {
                    out.push_str("\n(none listed: past the limit)");
                }
            }
            if self.items.iter().any(|i| i.group == 0) {
                out.push_str("\nSelected as well, not placed in the message:");
            }
        }
        self.sections(0, &mut out);
        if self.omitted > 0 {
            out.push_str(&format!("\n(and {} more, not listed)", self.omitted));
        }
        out
    }

    /// The short phrase for a pick: the item's own label for one, else what it holds, "3 faces + 1
    /// edge", and the body when they are all of one.
    pub fn group_label(&self) -> String {
        if self.items.len() == 1 {
            return self.items.first().map(|i| i.label.clone()).unwrap_or_default();
        }
        let (mut faces, mut edges, mut vertices, mut features, mut bodies, mut other) = (0, 0, 0, 0, 0, 0);
        let mut body: Option<Option<&str>> = None;
        for item in &self.items {
            let sel = item.sels.first();
            match sel {
                Some(Sel::Face { .. }) => faces += 1,
                Some(Sel::Edge { .. }) => edges += 1,
                Some(Sel::Vertex { .. }) => vertices += 1,
                Some(Sel::Feature { .. }) => features += 1,
                Some(Sel::Body { .. }) => bodies += 1,
                _ => other += 1,
            }
            let of = match sel {
                Some(Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } | Sel::Body { name: body }) => Some(body.as_str()),
                _ => None,
            };
            body = match body {
                None => Some(of),
                Some(prev) if prev == of => Some(prev),
                Some(_) => Some(None),
            };
        }
        let count = |n: usize, one: &str, many: &str| (n > 0).then(|| format!("{n} {}", if n == 1 { one } else { many }));
        let parts: Vec<String> = [
            count(faces, "face", "faces"),
            count(edges, "edge", "edges"),
            count(vertices, "vertex", "vertices"),
            count(features, "feature", "features"),
            count(bodies, "body", "bodies"),
            count(other, "item", "items"),
        ]
        .into_iter()
        .flatten()
        .collect();
        let what = parts.join(" + ");
        match body {
            Some(Some(name)) if faces + edges + vertices > 0 && !name.is_empty() => format!("{name} · {what}"),
            _ => what,
        }
    }
}

/// Persistent names of a body's faces and of its edges, kept per body while checking a batch, and
/// only worked out for what is asked (edge names cost more than face names).
#[derive(Default)]
pub struct NameCache {
    faces: HashMap<String, Vec<String>>,
    edges: HashMap<String, Vec<String>>,
}

impl NameCache {
    pub fn new() -> NameCache {
        NameCache::default()
    }

    /// The names of a body's faces (`edges`: of its edges), made by `make` the first time.
    fn names(&mut self, body: &str, edges: bool, make: impl FnOnce() -> Vec<String>) -> &[String] {
        let map = if edges { &mut self.edges } else { &mut self.faces };
        map.entry(body.to_string()).or_insert_with(make)
    }
}

/// Whether what `entry` stands for has left the design: its body or feature is gone, or none of its
/// faces or edges has its persistent name any more (they survive edits, so the item was changed
/// away or deleted). Items that cannot tell, and ones with no names, count as there.
pub fn is_gone(session: &Session, entry: &Entry, names: &mut NameCache) -> bool {
    let Some(sel) = entry.sels.first() else { return false };
    let state = session.world_state();
    match sel {
        Sel::Body { name } => state.body(name).is_none(),
        Sel::Feature { id } => session.doc.feature(*id).is_none(),
        Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => {
            let Some(model_body) = state.body(body) else { return true };
            if entry.names.is_empty() {
                return false;
            }
            let edges = !matches!(sel, Sel::Face { .. });
            let current = names.names(body, edges, || {
                if edges {
                    solidcraft_engine::doc::naming::edge_names(model_body)
                } else {
                    solidcraft_engine::doc::naming::face_names(model_body).to_vec()
                }
            });
            !entry.names.iter().any(|n| current.contains(n))
        }
        _ => false,
    }
}

/// What `entry` stands for in the design as it is now. A face or edge is looked up by its persistent
/// name: its number shifts when the design is edited, and the number it was picked with would light
/// another face. What has gone from the design (see [`is_gone`]) stands for nothing.
pub fn current_sels(session: &Session, entry: &Entry, names: &mut NameCache) -> Vec<Sel> {
    if is_gone(session, entry, names) {
        return Vec::new();
    }
    let Some(first) = entry.sels.first() else { return Vec::new() };
    let (body, is_face) = match first {
        Sel::Face { body, .. } => (body, true),
        Sel::Edge { body, .. } => (body, false),
        _ => return entry.sels.clone(),
    };
    if entry.names.is_empty() {
        return entry.sels.clone();
    }
    let state = session.world_state();
    let Some(model_body) = state.body(body) else { return Vec::new() };
    let current = names.names(body, !is_face, || {
        if is_face { solidcraft_engine::doc::naming::face_names(model_body).to_vec() } else { solidcraft_engine::doc::naming::edge_names(model_body) }
    });
    // The names go with the pieces in order; the point to look at stays the piece's.
    let paired = entry.names.len() == entry.sels.len();
    let mut lit = Vec::new();
    for (at, name) in entry.names.iter().enumerate() {
        let Some(index) = current.iter().position(|n| n == name) else { continue };
        let piece = if paired { entry.sels.get(at) } else { entry.sels.first() };
        match piece {
            Some(Sel::Face { point, .. }) => lit.push(Sel::Face { body: body.clone(), index, point: *point }),
            Some(Sel::Edge { point, .. }) => lit.push(Sel::Edge { body: body.clone(), index, point: *point }),
            _ => {}
        }
    }
    lit
}

/// The message as the agent gets it: the text, then what was selected.
pub fn with_context(text: &str, context: &Context) -> String {
    let block = context.prompt_block();
    if block.is_empty() { text.to_string() } else { format!("{text}\n\n{block}") }
}

/// Describe what the session has selected, as what was clicked: see [`clicked`].
pub fn describe_selection(session: &mut Session) -> Context {
    let groups = clicked(session, &session.selection.clone());
    describe_groups(session, &groups)
}

/// How many things were clicked to make a selection: the pieces a click on a rim or on a surface
/// split by a seam selects count once.
pub fn selection_count(session: &Session, selection: &[Sel]) -> usize {
    clicked(session, selection).len()
}

/// A selection as the things the user clicked. A click on a circular rim or a split surface selects
/// all its pieces (so commands get them all); here they are one group, the clicked piece first.
/// Past [`MAX_GROUPING`] items the selection is taken as it is: nobody clicked that many rims.
pub fn clicked(session: &Session, selection: &[Sel]) -> Vec<Vec<Sel>> {
    let mut groups: Vec<Vec<Sel>> = Vec::new();
    let mut taken = vec![false; selection.len()];
    for (i, sel) in selection.iter().enumerate() {
        if taken[i] {
            continue;
        }
        let mut group = vec![sel.clone()];
        if selection.len() <= MAX_GROUPING && matches!(sel, Sel::Edge { .. } | Sel::Face { .. }) {
            for piece in crate::viewport::grouped_selection(session, sel) {
                if let Some(j) = selection.iter().enumerate().position(|(j, x)| j != i && !taken[j] && crate::selection::same_item(x, &piece)) {
                    taken[j] = true;
                    group.push(selection[j].clone());
                }
            }
        }
        groups.push(group);
    }
    groups
}

/// More than this many selected things are not grouped into clicks (each grouping reads the model).
const MAX_GROUPING: usize = 256;

/// Describe a selection, each item on its own: what the picker holds. Face and edge details come
/// from the model's own listings (`model.faces`, `model.edges`), asked once per body.
pub fn describe(session: &mut Session, selection: &[Sel]) -> Context {
    let groups: Vec<Vec<Sel>> = selection.iter().map(|s| vec![s.clone()]).collect();
    describe_groups(session, &groups)
}

/// Describe groups of pieces that were clicked as one thing, each once.
pub fn describe_groups(session: &mut Session, groups: &[Vec<Sel>]) -> Context {
    let mut context = Context::default();
    let mut listings: HashMap<(&'static str, String), Value> = HashMap::new();
    for group in groups {
        if context.items.len() >= MAX_ITEMS {
            context.omitted += 1;
        } else if let Some(first) = group.first() {
            let mut item = entry(session, first, &mut listings);
            if group.len() > 1 {
                join_pieces(session, group, &mut item, &mut listings);
            }
            item.sels = group.clone();
            context.items.push(item);
        }
    }
    context
}

/// An item clicked as several pieces (the halves of a rim, the patches of a split surface): one
/// entry with the whole size, and what the tools take for every piece in its detail.
fn join_pieces(session: &mut Session, group: &[Sel], item: &mut Entry, listings: &mut HashMap<(&'static str, String), Value>) {
    let mut sizes = Vec::new();
    let mut indices = Vec::new();
    let mut names = Vec::new();
    let mut unit = "";
    let mut body = String::new();
    for sel in group {
        let found = match sel {
            Sel::Edge { body: b, index, point } => {
                unit = "mm";
                body = b.clone();
                indices.push(*index);
                listed_edge(session, listings, b, *index, *point)
            }
            Sel::Face { body: b, index, .. } => {
                unit = "mm²";
                body = b.clone();
                indices.push(*index);
                listed(session, listings, "model.faces", "faces", b, *index)
            }
            _ => None,
        };
        if let Some(f) = found {
            if let Some(n) = f["name"].as_str().filter(|n| !n.is_empty()) {
                names.push(n.to_string());
            }
            if let Some(v) = f[if unit == "mm" { "length" } else { "area" }].as_f64() {
                sizes.push(v);
            }
        }
    }
    let pieces = group.len();
    let total: f64 = sizes.iter().sum();
    if matches!(group.first(), Some(Sel::Edge { .. })) && sizes.len() == pieces {
        item.label = format!("Edge {} mm ({pieces} pieces) · {body}", num(total));
    } else {
        item.label = format!("{} ({pieces} pieces)", item.label);
    }
    let whole = if sizes.len() == pieces { format!(", total {} {unit}", num(total)) } else { String::new() };
    let said = if names.is_empty() { String::new() } else { format!(", names {}", names.join(" + ")) };
    item.detail = format!("clicked as one: {pieces} pieces, indices {indices:?}{said}{whole}; first piece: {}", item.detail);
    item.names = names;
}

fn entry(session: &mut Session, sel: &Sel, listings: &mut HashMap<(&'static str, String), Value>) -> Entry {
    match sel {
        Sel::Face { body, index, point } => {
            let face = listed(session, listings, "model.faces", "faces", body, *index);
            let mut parts = Vec::new();
            // A name a person can read: the shape and its size, then the body. What the tools take
            // (the persistent name, the index, the pick point) is in the detail.
            let mut shape = format!("Face #{index}");
            if let Some(f) = &face {
                let radius = f["cylinder"]["radius"].as_f64();
                let planar = f["plane_normal"].is_array();
                let area = f["area"].as_f64().map(|a| format!(" {} mm²", num(a))).unwrap_or_default();
                shape = match (radius, planar) {
                    (Some(r), _) => format!("Round face r{}", num(r)),
                    (None, true) => format!("Flat face{area}"),
                    _ => format!("Curved face{area}"),
                };
                if let Some(name) = f["name"].as_str() {
                    parts.push(format!("name {name}"));
                }
                parts.push(match (radius, planar) {
                    (Some(r), _) => format!("cylindrical, radius {} mm", num(r)),
                    (None, true) => format!("planar, normal {}", point3(&f["plane_normal"])),
                    _ => "curved".to_string(),
                });
                if let Some(a) = f["area"].as_f64() {
                    parts.push(format!("area {} mm²", num(a)));
                }
                parts.push(format!("centroid {}", point3(&f["centroid"])));
                parts.push(format!("index {index}"));
            }
            parts.push(format!("pick point {}", point3(&json!(point))));
            let mut entry = Entry::new(Kind::Element, format!("{shape} · {body}"), parts.join(", "));
            entry.names = face.as_ref().and_then(|f| f["name"].as_str()).filter(|n| !n.is_empty()).map(str::to_string).into_iter().collect();
            entry
        }
        Sel::Edge { body, index, point } => {
            let edge = listed_edge(session, listings, body, *index, *point);
            let mut parts = Vec::new();
            let mut shape = format!("Edge #{index}");
            if let Some(e) = &edge {
                if let Some(name) = e["name"].as_str() {
                    parts.push(format!("name {name}"));
                }
                if let Some(l) = e["length"].as_f64() {
                    shape = format!("Edge {} mm", num(l));
                    parts.push(format!("length {} mm", num(l)));
                }
                parts.push(format!("from {} to {}", point3(&e["start"]), point3(&e["end"])));
                parts.push(format!("index {index}"));
            }
            parts.push(format!("pick point {}", point3(&json!(point))));
            let mut entry = Entry::new(Kind::Element, format!("{shape} · {body}"), parts.join(", "));
            entry.names = edge.as_ref().and_then(|e| e["name"].as_str()).filter(|n| !n.is_empty()).map(str::to_string).into_iter().collect();
            entry
        }
        Sel::Vertex { body, point } => Entry::new(Kind::Element, format!("Vertex of {body}"), format!("at {}", point3(&json!(point)))),
        Sel::Body { name } => Entry::new(Kind::Body, name.clone(), String::new()),
        Sel::Occurrence { path } => Entry::new(
            Kind::Body,
            format!("Component occurrence {}", path.iter().map(u64::to_string).collect::<Vec<_>>().join("/")),
            format!("occurrence path {path:?}"),
        ),
        Sel::Feature { id } => {
            let feature = session.doc.find_feature(&id.to_string());
            let name = feature.map(|f| f.name.clone()).unwrap_or_else(|| format!("#{id}"));
            let kind = feature.and_then(|f| serde_json::to_value(f).ok()).and_then(|v| v["type"].as_str().map(str::to_string));
            let mut detail = format!("feature id {id}");
            if let Some(kind) = kind {
                detail = format!("{kind}, {detail}");
            }
            if feature.is_some_and(|f| f.suppressed) {
                detail.push_str(", suppressed");
            }
            Entry::new(Kind::Feature, name, detail)
        }
        other => {
            // Planes, axes and sketch geometry: named by their kind and whatever identifies them.
            let value = serde_json::to_value(other).unwrap_or(Value::Null);
            let kind = value["type"].as_str().unwrap_or("item").replace('_', " ");
            let id = ["name", "id", "index", "sketch"].iter().find_map(|k| match &value[k] {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            });
            let label = match id {
                Some(id) => format!("{kind} {id}"),
                None => kind,
            };
            let label: String = label.chars().take(60).collect();
            Entry::new(Kind::Other, capitalised(&label), String::new())
        }
    }
}

/// The listing entry for a face or edge (`index`), from a command run once per body.
fn listed(
    session: &mut Session,
    listings: &mut HashMap<(&'static str, String), Value>,
    command: &'static str,
    list: &str,
    body: &str,
    index: usize,
) -> Option<Value> {
    let listing =
        listings.entry((command, body.to_string())).or_insert_with(|| session.execute(command, &json!({"body": body})).unwrap_or(Value::Null));
    let index = u64::try_from(index).ok()?;
    listing[list].as_array()?.iter().find(|item| item["index"].as_u64() == Some(index)).cloned()
}

/// The listing entry of the edge picked at `point`: the one with that index, unless its middle is
/// somewhere else (the selection and the listing number edges alike, but a stale index must not
/// describe another edge), then the edge whose middle is nearest the point.
fn listed_edge(session: &mut Session, listings: &mut HashMap<(&'static str, String), Value>, body: &str, index: usize, point: Vec3) -> Option<Value> {
    let by_index = listed(session, listings, "model.edges", "edges", body, index);
    let near = |e: &Value| -> Option<f64> {
        let mid = e["mid"].as_array()?;
        let p = |i: usize| mid.get(i).and_then(Value::as_f64);
        let dist = ((p(0)? - point.x).powi(2) + (p(1)? - point.y).powi(2) + (p(2)? - point.z).powi(2)).sqrt();
        dist.is_finite().then_some(dist)
    };
    let tolerance = |e: &Value| (0.02 * e["length"].as_f64().unwrap_or(0.0)).max(0.05);
    if let Some(e) = &by_index
        && near(e).is_some_and(|d| d <= tolerance(e))
    {
        return by_index;
    }
    let all = listings.get(&("model.edges", body.to_string()))?["edges"].as_array()?.clone();
    all.into_iter().filter_map(|e| near(&e).map(|d| (d, e))).filter(|(d, e)| *d <= tolerance(e)).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, e)| e)
}

/// A length or area to two decimals, without a trailing `.00`.
fn num(v: f64) -> String {
    if !v.is_finite() {
        return "?".into();
    }
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// `(x, y, z)` from a JSON array of three numbers, `?` when it is not one.
fn point3(v: &Value) -> String {
    match v.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect::<Vec<_>>()) {
        Some(c) if c.len() == 3 => format!("({}, {}, {})", num(c[0]), num(c[1]), num(c[2])),
        _ => "?".into(),
    }
}

fn capitalised(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}
