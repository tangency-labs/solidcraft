//! A CADIR document's product structure as an import's product tree: each occurrence a node
//! placed in its parent, and each body of the definition an occurrence uses placed where that
//! occurrence puts it, as SolidCraft's STEP reader places an assembly's bodies (one body per
//! placed use, in world coordinates; each node's transform relative to its parent).
//!
//! A part used twice is two bodies, at its two placements: the second a copy of the first
//! ([`RecordBody::copy_of`]), so the design knows them as one part and the record is kept once. A body no occurrence reaches stays where
//! it is, under a root named after the file. An occurrence that mirrors or scales is left out with
//! the reason, as a body whose own placement does is ([`solidcore_cadir`]), and so are the bodies
//! of its part and of everything under it: they do not come in where the file defines them.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::products::{Occurrence, OccurrenceParent, ProductDefinition, PrototypeReference};
use cadmpeg_ir::transform::Transform;
use solidcraft_kernel::{IMPORT_MAX_BODIES, IMPORT_MAX_DEPTH, IMPORT_MAX_VISITS, RecordBody, StepNode};

use super::validation::Origin;

type M4 = [[f64; 4]; 4];

const IDENTITY: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// One body as read, before it is placed: its CADIR body and its record, placed by the body's own
/// placement alone.
pub(super) struct Unplaced {
    pub body: String,
    pub record: RecordBody,
}

/// The bodies and product tree of `ir`'s occurrences, from `read` (each body as read), with what
/// could not be placed in `warnings`. `product` names the root that holds the bodies no
/// occurrence reaches.
pub(super) fn placed(
    ir: &CadIr,
    product: &str,
    read: Vec<Unplaced>,
    layers: &super::HiddenLayers,
    warnings: &mut Vec<String>,
) -> (Vec<RecordBody>, Vec<StepNode>, Vec<Origin>) {
    let mut walk = Walk {
        definitions: ir.model.product_definitions.iter().map(|d| (d.id.as_str(), d)).collect(),
        children: HashMap::new(),
        by_body: HashMap::new(),
        read: &read,
        bodies: Vec::new(),
        origins: Vec::new(),
        first: HashMap::new(),
        layers,
        visits: 0,
        full: false,
        warnings,
    };
    for (i, u) in read.iter().enumerate() {
        walk.by_body.entry(u.body.as_str()).or_default().push(i);
    }
    let mut roots = Vec::new();
    for o in &ir.model.occurrences {
        match &o.parent {
            OccurrenceParent::Root {} => roots.push(o),
            OccurrenceParent::Occurrence { occurrence } => walk.children.entry(occurrence.as_str()).or_default().push(o),
        }
    }
    let order = |a: &&Occurrence, b: &&Occurrence| a.ordinal.cmp(&b.ordinal).then_with(|| a.id.as_str().cmp(b.id.as_str()));
    roots.sort_by(order);
    for list in walk.children.values_mut() {
        list.sort_by(order);
    }
    let reached = reached(&roots, &walk.children, &walk.definitions);
    let mut tree: Vec<StepNode> = Vec::with_capacity(roots.len() + 1);
    for o in roots {
        let mut stack = Vec::new();
        tree.push(walk.node(o, &Transform::identity(), &[], false, 0, &mut stack));
    }
    // Bodies no occurrence reaches stay where they are, under a root named after the file. Those
    // of an occurrence left out (and of everything under it) stay out, as its warning says.
    let loose: Vec<usize> = (0..read.len()).filter(|&i| read.get(i).is_some_and(|u| !reached.contains(u.body.as_str()))).collect();
    if !loose.is_empty() {
        let mut node = StepNode {
            name: product.to_string(),
            transform: IDENTITY,
            bodies: Vec::new(),
            items: Vec::new(),
            left_out: Vec::new(),
            children: Vec::new(),
        };
        for i in loose {
            let Some(u) = read.get(i) else { continue };
            if !walk.room() {
                break;
            }
            node.bodies.push(walk.bodies.len());
            walk.bodies.push(RecordBody { path: vec![product.to_string()], ..u.record.clone() });
            walk.origins.push(Origin { body: u.body.clone(), world: IDENTITY });
        }
        tree.push(node);
    }
    (walk.bodies, tree, walk.origins)
}

/// The bodies of the parts every occurrence under `roots` uses (through `children`), whether or
/// not it could be placed: each occurrence once, so a cycle ends.
fn reached<'a>(
    roots: &[&'a Occurrence],
    children: &HashMap<&'a str, Vec<&'a Occurrence>>,
    definitions: &HashMap<&'a str, &'a ProductDefinition>,
) -> HashSet<&'a str> {
    let (mut seen, mut bodies) = (HashSet::new(), HashSet::new());
    let mut stack: Vec<&Occurrence> = roots.to_vec();
    while let Some(o) = stack.pop() {
        if !seen.insert(o.id.as_str()) {
            continue;
        }
        if let PrototypeReference::Local { definition } = &o.prototype
            && let Some(d) = definitions.get(definition.as_str()).copied()
        {
            bodies.extend(d.bodies.iter().map(|b| b.as_str()));
        }
        stack.extend(children.get(o.id.as_str()).into_iter().flatten().copied());
    }
    bodies
}

struct Walk<'a, 'w> {
    definitions: HashMap<&'a str, &'a ProductDefinition>,
    /// Each occurrence's child occurrences, in their order.
    children: HashMap<&'a str, Vec<&'a Occurrence>>,
    /// The read bodies (indices into `read`) of each CADIR body.
    by_body: HashMap<&'a str, Vec<usize>>,
    read: &'a [Unplaced],
    bodies: Vec<RecordBody>,
    /// Where each of `bodies` came from: its neutral-model body and its occurrence's placement.
    origins: Vec<Origin>,
    /// What the file's hidden layers hide.
    layers: &'a super::HiddenLayers,
    /// The read bodies some occurrence placed, each with the body its first placement is: later
    /// placements are copies of that one.
    first: HashMap<usize, usize>,
    visits: usize,
    /// The body limit was reached and said so.
    full: bool,
    warnings: &'w mut Vec<String>,
}

impl Walk<'_, '_> {
    /// Whether another body fits under the import's limit; says so once when not.
    fn room(&mut self) -> bool {
        if self.bodies.len() < IMPORT_MAX_BODIES {
            return true;
        }
        if !self.full {
            self.full = true;
            self.warnings.push(format!("more than {IMPORT_MAX_BODIES} bodies: the rest are left out"));
        }
        false
    }

    /// The node of occurrence `o` under a parent placed by `parent` (`path`: the products above),
    /// its bodies placed in world coordinates, and its children's nodes.
    fn node(&mut self, o: &Occurrence, parent: &Transform, path: &[String], hidden: bool, depth: usize, stack: &mut Vec<String>) -> StepNode {
        let definition = match &o.prototype {
            PrototypeReference::Local { definition } => self.definitions.get(definition.as_str()).copied(),
            _ => None,
        };
        let name = definition.map_or_else(|| o.name.as_deref().unwrap_or_default().trim().to_string(), product_name);
        let mut node =
            StepNode { name: name.clone(), transform: IDENTITY, bodies: Vec::new(), items: Vec::new(), left_out: Vec::new(), children: Vec::new() };
        let label = if name.is_empty() { o.id.as_str().to_string() } else { name.clone() };
        if depth > IMPORT_MAX_DEPTH || stack.iter().any(|s| s == o.id.as_str()) {
            self.warnings.push("assembly nested too deeply or recursive: some components were left out".into());
            return node;
        }
        self.visits += 1;
        if self.visits > IMPORT_MAX_VISITS {
            if self.visits == IMPORT_MAX_VISITS + 1 {
                self.warnings.push(format!("more than {IMPORT_MAX_VISITS} component instances: the rest are left out"));
            }
            return node;
        }
        let local = match local(o) {
            Ok(t) => t,
            Err(why) => {
                self.warnings.push(format!("{label}: not imported: {why}"));
                return node;
            }
        };
        let Ok(world) = parent.compose(local) else {
            self.warnings.push(format!("{label}: not imported: its placement in the assembly is not finite"));
            return node;
        };
        node.transform = column_major(&local);
        // Hidden by the file: the occurrence itself, or a hidden layer it or its part is on.
        let on_hidden_layer = self.layers.occurrences.contains(o.id.as_str())
            || matches!(&o.prototype, PrototypeReference::Local { definition } if self.layers.definitions.contains(definition.as_str()));
        let hidden = hidden || o.visible == Some(false) || on_hidden_layer;
        let mut path = path.to_vec();
        path.push(name.clone());
        match (&o.prototype, definition) {
            (_, Some(d)) => {
                let world = column_major(&world);
                for body in &d.bodies {
                    let indices = self.by_body.get(body.as_str()).cloned().unwrap_or_default();
                    for i in indices {
                        let Some(u) = self.read.get(i) else { continue };
                        if !self.room() {
                            break;
                        }
                        let at = self.bodies.len();
                        node.bodies.push(at);
                        let placement = solidcraft_doc::mat_mul(&world, &u.record.placement.unwrap_or(IDENTITY));
                        // The part's first placement keeps the record; each later one is a copy
                        // of it, so the design knows them as one part placed again, and holds none
                        // of its geometry (no record, faces or triangles), not even for a moment.
                        let part = match self.first.get(&i) {
                            Some(&original) => RecordBody {
                                name: u.record.name.clone(),
                                path: Vec::new(),
                                instance: None,
                                color: u.record.color,
                                paint: u.record.paint.clone(),
                                hidden: u.record.hidden,
                                placement: None,
                                brep: None,
                                face_origin: Vec::new(),
                                mesh: None,
                                copy_of: Some(original),
                                sheet: u.record.sheet,
                            },
                            None => {
                                self.first.insert(i, at);
                                u.record.clone()
                            }
                        };
                        let placed = RecordBody {
                            name: if u.record.name.is_empty() { name.clone() } else { u.record.name.clone() },
                            path: path.clone(),
                            instance: o.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string),
                            hidden: hidden || u.record.hidden,
                            placement: (placement != IDENTITY).then_some(placement),
                            ..part
                        };
                        self.bodies.push(placed);
                        self.origins.push(Origin { body: u.body.clone(), world });
                    }
                }
            }
            (PrototypeReference::External { .. }, None) => self.warnings.push(format!("{label}: not imported: it is a part in another file")),
            (_, None) => self.warnings.push(format!("{label}: not imported: the file does not say which part it is")),
        }
        stack.push(o.id.as_str().to_string());
        let children = self.children.get(o.id.as_str()).cloned().unwrap_or_default();
        for c in children {
            let child = self.node(c, &world, &path, hidden, depth + 1, stack);
            node.children.push(child);
        }
        stack.pop();
        node
    }
}

/// A definition's name: its label, else its name in the file, else its part number.
fn product_name(d: &ProductDefinition) -> String {
    // Trimmed, as SolidCraft's STEP reader trims a product's name ("Guma NR 60ShA černá " is
    // "Guma NR 60ShA černá").
    [&d.label, &d.source_name, &d.part_number].into_iter().flatten().map(|n| n.trim()).find(|n| !n.is_empty()).unwrap_or_default().to_string()
}

/// Occurrence `o`'s placement in its parent, when rigid: one that scales or mirrors is refused.
fn local(o: &Occurrence) -> Result<Transform, &'static str> {
    if o.scale.iter().any(|s| (s.get() - 1.0).abs() > 1e-12) {
        return Err("not supported yet: a component placement that scales");
    }
    let t = o.effective_transform().map_err(|_| "its placement is not finite")?;
    if !t.is_proper_rigid() {
        return Err("not supported yet: a component placement that mirrors or scales");
    }
    Ok(t)
}

/// `t` as SolidCraft's column-major 4 × 4.
pub(super) fn column_major(t: &Transform) -> M4 {
    let r = t.rows();
    let mut m = IDENTITY;
    for (c, col) in m.iter_mut().enumerate() {
        for (row, v) in col.iter_mut().enumerate() {
            *v = r.get(row).and_then(|x| x.get(c)).copied().unwrap_or(0.0);
        }
    }
    m
}
