//! Edit View's list of the bodies a drawing view draws (#22107): each component and each body of
//! the view's source with a tick, and the `hide` and `hide_bodies` lists the ticks make.
//!
//! A component every body of which is unticked is left out whole (its path in `hide`); otherwise
//! each unticked body of it is left out on its own (`hide_bodies`). So ticking one body inside a
//! component that was left out whole leaves out its siblings one by one, and the sub-components
//! whose bodies are all unticked whole.

use std::collections::{BTreeMap, BTreeSet};

use solidcraft_engine::doc::drawing::view::{MAX_PATH_IDS, MAX_PATHS};
use solidcraft_engine::doc::drawing_view::HiddenBody;

/// One body of the list: where it is placed, its definition's name, and whether it is ticked.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Body {
    pub path: Vec<u64>,
    pub body: String,
    pub drawn: bool,
}

/// A row of the list, in the order it is shown: a component, then what it holds, indented by depth.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Row {
    Component { path: Vec<u64>, label: String, depth: usize },
    Body { index: usize, depth: usize },
}

/// The lists a view's settings take: `hide` (occurrence paths) and `hide_bodies`.
pub(super) type Lists = (Vec<Vec<u64>>, Vec<HiddenBody>);

#[derive(Debug)]
pub(super) struct Bodies {
    pub bodies: Vec<Body>,
    pub rows: Vec<Row>,
    /// `hide` paths that hold no body of the list (outside the source, or empty): kept as they are.
    kept_hide: Vec<Vec<u64>>,
    /// `hide_bodies` entries that name no body of the list (renamed or deleted since): kept as they
    /// are, and shown under the list.
    pub unmatched: Vec<HiddenBody>,
    initial: Lists,
}

impl Bodies {
    /// The list for `listed` (the bodies the view's source places, in the design's order, as
    /// `drawing.view.inspect` lists them), ticked as `hide` and `hide_bodies` leave them. `name`
    /// gives a component's label from its occurrence path.
    pub fn new(listed: Vec<(Vec<u64>, String)>, hide: &[Vec<u64>], hide_bodies: &[HiddenBody], name: impl Fn(&[u64]) -> String) -> Bodies {
        let hidden_bodies: BTreeSet<(&[u64], &str)> = hide_bodies.iter().map(|b| (b.path.as_slice(), b.body.as_str())).collect();
        let mut bodies: Vec<Body> = listed
            .into_iter()
            .map(|(path, body)| {
                let drawn = !hide.iter().any(|p| !p.is_empty() && path.starts_with(p)) && !hidden_bodies.contains(&(path.as_slice(), body.as_str()));
                Body { path, body, drawn }
            })
            .collect();
        // Group each component's bodies together, components in the order they first appear and
        // the design's own bodies first, as the Browser lists them.
        let mut first: BTreeMap<&[u64], usize> = BTreeMap::new();
        for (at, body) in bodies.iter().enumerate() {
            for k in 1..=body.path.len() {
                first.entry(&body.path[..k]).or_insert(at);
            }
        }
        let keys: Vec<Vec<usize>> =
            bodies.iter().map(|b| (1..=b.path.len()).map(|k| first.get(&b.path[..k]).copied().unwrap_or_default()).collect()).collect();
        let mut order: Vec<usize> = (0..bodies.len()).collect();
        order.sort_by(|a, b| keys[*a].cmp(&keys[*b]));
        bodies = order.iter().filter_map(|at| bodies.get(*at).cloned()).collect();
        let mut rows = Vec::new();
        let mut shown: BTreeSet<Vec<u64>> = BTreeSet::new();
        for (index, body) in bodies.iter().enumerate() {
            for k in 1..=body.path.len() {
                let prefix = body.path[..k].to_vec();
                if shown.insert(prefix.clone()) {
                    rows.push(Row::Component { label: name(&prefix), path: prefix, depth: k - 1 });
                }
            }
            rows.push(Row::Body { index, depth: body.path.len() });
        }
        let kept_hide = hide.iter().filter(|p| !bodies.iter().any(|b| b.path.starts_with(p))).cloned().collect();
        let unmatched = hide_bodies.iter().filter(|h| !bodies.iter().any(|b| b.path == h.path && b.body == h.body)).cloned().collect();
        let mut list = Bodies { bodies, rows, kept_hide, unmatched, initial: (Vec::new(), Vec::new()) };
        list.initial = list.lists();
        list
    }

    fn under<'a>(&'a self, path: &'a [u64]) -> impl Iterator<Item = &'a Body> + 'a {
        self.bodies.iter().filter(move |b| b.path.starts_with(path))
    }

    /// Whether every body of component `path` is ticked, and whether any is.
    pub fn component(&self, path: &[u64]) -> (bool, bool) {
        let mut all = true;
        let mut any = false;
        for body in self.under(path) {
            all &= body.drawn;
            any |= body.drawn;
        }
        (all, any)
    }

    /// Ticks or unticks every body of component `path`.
    pub fn set_component(&mut self, path: &[u64], drawn: bool) {
        for body in self.bodies.iter_mut().filter(|b| b.path.starts_with(path)) {
            body.drawn = drawn;
        }
    }

    /// The lists the ticks make, each in the list's order: a component left out whole when every
    /// body under it is unticked (not inside another left out whole), else each unticked body.
    pub fn lists(&self) -> Lists {
        let whole = |path: &[u64]| !path.is_empty() && self.under(path).all(|b| !b.drawn);
        let covered = |path: &[u64]| (1..=path.len()).any(|k| whole(&path[..k]));
        let mut hide: Vec<Vec<u64>> = Vec::new();
        for row in &self.rows {
            if let Row::Component { path, .. } = row
                && whole(path)
                && !(1..path.len()).any(|k| whole(&path[..k]))
            {
                hide.push(path.clone());
            }
        }
        let mut hide_bodies: Vec<HiddenBody> =
            self.bodies.iter().filter(|b| !b.drawn && !covered(&b.path)).map(|b| HiddenBody { path: b.path.clone(), body: b.body.clone() }).collect();
        hide.extend(self.kept_hide.iter().cloned());
        hide_bodies.extend(self.unmatched.iter().cloned());
        (hide, hide_bodies)
    }

    /// What the ticks change from the lists the view had: `hide` and `hide_bodies` each when it
    /// changed.
    pub fn changes(&self) -> (Option<Vec<Vec<u64>>>, Option<Vec<HiddenBody>>) {
        let (hide, hide_bodies) = self.lists();
        let same_paths = |a: &[Vec<u64>], b: &[Vec<u64>]| a.iter().collect::<BTreeSet<_>>() == b.iter().collect::<BTreeSet<_>>();
        let same_bodies = |a: &[HiddenBody], b: &[HiddenBody]| a.iter().collect::<BTreeSet<_>>() == b.iter().collect::<BTreeSet<_>>();
        let hide = (!same_paths(&hide, &self.initial.0)).then_some(hide);
        let hide_bodies = (!same_bodies(&hide_bodies, &self.initial.1)).then_some(hide_bodies);
        (hide, hide_bodies)
    }

    /// The lists leave something out: a component, a body, or an entry the list can't show.
    pub fn leaves_out(&self) -> bool {
        let (hide, hide_bodies) = self.lists();
        !hide.is_empty() || !hide_bodies.is_empty()
    }

    /// Nothing is ticked: the view would draw nothing.
    pub fn none_drawn(&self) -> bool {
        !self.bodies.is_empty() && self.bodies.iter().all(|b| !b.drawn)
    }

    /// The lists are past what a view's settings hold ([`MAX_PATHS`] entries and [`MAX_PATH_IDS`]
    /// ids each).
    pub fn too_many(&self) -> bool {
        let (hide, hide_bodies) = self.lists();
        let ids = |paths: &mut dyn Iterator<Item = usize>| paths.fold(0usize, usize::saturating_add);
        hide.len() > MAX_PATHS
            || hide_bodies.len() > MAX_PATHS
            || ids(&mut hide.iter().map(Vec::len)) > MAX_PATH_IDS
            || ids(&mut hide_bodies.iter().map(|b| b.path.len())) > MAX_PATH_IDS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hidden(path: &[u64], body: &str) -> HiddenBody {
        HiddenBody { path: path.to_vec(), body: body.into() }
    }

    /// A cabinet: its own Top, a Drawer (occurrence 1) with Front and Box, and in it a Runner
    /// (occurrence 2) with Slide L and Slide R.
    fn cabinet(hide: &[Vec<u64>], hide_bodies: &[HiddenBody]) -> Bodies {
        let listed = vec![
            (vec![1], "Front".to_owned()),
            (vec![1, 2], "Slide L".to_owned()),
            (Vec::new(), "Top".to_owned()),
            (vec![1, 2], "Slide R".to_owned()),
            (vec![1], "Box".to_owned()),
        ];
        Bodies::new(listed, hide, hide_bodies, |path| format!("C{path:?}"))
    }

    fn tick(list: &mut Bodies, body: &str, drawn: bool) {
        if let Some(b) = list.bodies.iter_mut().find(|b| b.body == body) {
            b.drawn = drawn;
        }
    }

    #[test]
    fn rows_group_each_component_after_the_designs_own_bodies() {
        let list = cabinet(&[], &[]);
        let names: Vec<String> = list
            .rows
            .iter()
            .map(|row| match row {
                Row::Component { label, depth, .. } => format!("{depth}:{label}"),
                Row::Body { index, depth } => format!("{depth}:{}", list.bodies[*index].body),
            })
            .collect();
        assert_eq!(names, ["0:Top", "0:C[1]", "1:Front", "1:Box", "1:C[1, 2]", "2:Slide L", "2:Slide R"]);
        assert_eq!(list.changes(), (None, None), "nothing changed, nothing to send");
    }

    #[test]
    fn unticking_every_body_of_a_component_leaves_it_out_whole() {
        let mut list = cabinet(&[], &[]);
        tick(&mut list, "Slide L", false);
        assert_eq!(list.lists(), (Vec::<Vec<u64>>::new(), vec![hidden(&[1, 2], "Slide L")]));
        tick(&mut list, "Slide R", false);
        assert_eq!(list.lists(), (vec![vec![1, 2]], Vec::new()));
        list.set_component(&[1], false);
        assert_eq!(list.lists(), (vec![vec![1]], Vec::new()), "the outer component covers the inner one");
        tick(&mut list, "Top", false);
        assert!(list.none_drawn());
    }

    #[test]
    fn ticking_one_body_inside_a_component_left_out_whole_leaves_out_its_siblings() {
        let mut list = cabinet(&[vec![1]], &[]);
        assert!(list.bodies.iter().filter(|b| b.path.starts_with(&[1])).all(|b| !b.drawn));
        assert_eq!(list.component(&[1]), (false, false));
        tick(&mut list, "Slide L", true);
        assert_eq!(list.component(&[1]), (false, true), "the drawer is mixed");
        let (hide, hide_bodies) = list.lists();
        assert_eq!(hide, Vec::<Vec<u64>>::new());
        assert_eq!(hide_bodies, vec![hidden(&[1], "Front"), hidden(&[1], "Box"), hidden(&[1, 2], "Slide R")]);
        assert_eq!(list.changes(), (Some(Vec::new()), Some(hide_bodies)));
    }

    #[test]
    fn a_nested_component_stays_whole_when_a_sibling_body_is_ticked_back() {
        let mut list = cabinet(&[vec![1]], &[]);
        tick(&mut list, "Front", true);
        assert_eq!(list.lists(), (vec![vec![1, 2]], vec![hidden(&[1], "Box")]));
    }

    #[test]
    fn entries_outside_the_list_are_kept() {
        let gone = hidden(&[1], "Old door");
        let mut list = cabinet(&[vec![9]], std::slice::from_ref(&gone));
        assert_eq!(list.unmatched, vec![gone.clone()]);
        assert_eq!(list.changes(), (None, None));
        tick(&mut list, "Top", false);
        assert_eq!(list.lists(), (vec![vec![9]], vec![hidden(&[], "Top"), gone]));
    }

    #[test]
    fn too_many_bodies_one_by_one_is_past_the_limit() {
        let listed: Vec<(Vec<u64>, String)> = (0..=MAX_PATHS + 1).map(|i| (vec![1], format!("Board {i}"))).collect();
        let mut list = Bodies::new(listed, &[], &[], |_| "Carcass".into());
        for body in list.bodies.iter_mut().skip(1) {
            body.drawn = false;
        }
        assert!(list.too_many(), "{} bodies one by one", MAX_PATHS + 1);
        list.set_component(&[1], false);
        assert!(!list.too_many(), "the whole component is one path");
    }
}
