//! The browser's groups as a tree: how deep a group sits, who holds whom, and pruning what a
//! saved design says about the browser down to what still exists.
use crate::document::{BROWSER_FOLDERS, BrowserEntry, BrowserGroup, Document, MAX_GROUP_NESTING};
use std::collections::{BTreeMap, BTreeSet};

impl Document {
    pub fn browser_group(&self, id: u64) -> Option<&BrowserGroup> {
        self.browser_groups.iter().find(|g| g.id == id)
    }

    /// The level of a group: 1 for a top-level group, 2 for a child of one, and so on. A chain
    /// that loops or runs past the limit counts as one level too deep.
    pub fn group_level(&self, id: u64) -> usize {
        let mut level = 0;
        let mut at = Some(id);
        while let Some(g) = at {
            level += 1;
            if level > MAX_GROUP_NESTING {
                return level;
            }
            at = self.browser_group(g).and_then(|g| g.parent);
        }
        level
    }

    /// Whether `ancestor` is `id` or holds it, at any depth.
    pub fn group_within(&self, id: u64, ancestor: u64) -> bool {
        let mut at = Some(id);
        for _ in 0..=MAX_GROUP_NESTING {
            match at {
                Some(g) if g == ancestor => return true,
                Some(g) => at = self.browser_group(g).and_then(|g| g.parent),
                None => return false,
            }
        }
        false
    }

    /// How many levels the group and the groups under it take: 1 for a group with no child group.
    pub fn group_height(&self, id: u64) -> usize {
        fn height(d: &Document, id: u64, left: usize) -> usize {
            if left == 0 {
                return 1;
            }
            1 + d.browser_groups.iter().filter(|g| g.parent == Some(id)).map(|g| height(d, g.id, left - 1)).max().unwrap_or(0)
        }
        height(self, id, MAX_GROUP_NESTING + 1)
    }

    /// The entries of a holder's list in display order: a group's contents, or the folder's top
    /// level when `holder` is `None`. Groups the list names come where it names them. The child
    /// groups it does not name (in creation order) go just after the last group it names, or
    /// first when it names none, so they sit above the items as they always have. Entries for
    /// groups that are not the holder's children are dropped.
    pub fn ordered_entries(&self, component: u64, folder: &str, holder: Option<u64>) -> Vec<BrowserEntry> {
        let listed: &[BrowserEntry] = match holder {
            Some(g) => self.browser_group(g).map_or(&[], |g| &g.items),
            None => self.browser_order.get(&format!("{component}/{folder}")).map_or(&[], Vec::as_slice),
        };
        let mine: Vec<u64> =
            self.browser_groups.iter().filter(|g| g.component == component && g.folder == folder && g.parent == holder).map(|g| g.id).collect();
        let mut out: Vec<BrowserEntry> = Vec::with_capacity(listed.len() + mine.len());
        for e in listed {
            match e {
                BrowserEntry::Group(id) if !mine.contains(id) || out.contains(e) => {}
                _ => out.push(e.clone()),
            }
        }
        let at = out.iter().rposition(|e| matches!(e, BrowserEntry::Group(_))).map_or(0, |i| i + 1);
        let unlisted: Vec<BrowserEntry> = mine.iter().map(|id| BrowserEntry::Group(*id)).filter(|e| !out.contains(e)).collect();
        out.splice(at..at, unlisted);
        out
    }

    /// The items in a group and in the groups inside it.
    pub fn group_item_keys(&self, id: u64) -> Vec<String> {
        let inside = self.browser_groups.iter().filter(|g| self.group_within(g.id, id));
        inside.flat_map(|g| g.item_keys().map(str::to_string)).collect()
    }

    /// The group holding the item directly, in a component's folder.
    pub fn group_of_item(&self, component: u64, folder: &str, key: &str) -> Option<&BrowserGroup> {
        self.browser_groups.iter().find(|g| g.component == component && g.folder == folder && g.has_item(key))
    }

    /// Only the browser's groups and order changed (a group made, nested, moved into, ordered):
    /// nothing is evaluated and no placement moves. The same whole-Document proof as
    /// [`Document::differs_only_in_render`]; `next_id` may differ, as a new group takes an id.
    pub fn differs_only_in_browser(&self, previous: &Document) -> bool {
        if self.browser_groups == previous.browser_groups && self.browser_order == previous.browser_order {
            return false;
        }
        let mut normalized = previous.clone();
        normalized.browser_groups.clone_from(&self.browser_groups);
        normalized.browser_order.clone_from(&self.browser_order);
        normalized.next_id = self.next_id;
        *self == normalized
    }

    /// What [`Document::browser_pruned`] depends on, cheaply: it can only find something to drop
    /// after a feature, placement, component or group was added or removed. Ids are never
    /// reused, so a removal followed by an addition still moves `next_id`.
    pub fn browser_stamp(&self) -> [u64; 6] {
        let n = |len: usize| u64::try_from(len).unwrap_or(u64::MAX);
        [
            self.next_id,
            n(self.features.len()),
            n(self.occurrences.len()),
            n(self.components.len()),
            n(self.browser_groups.len()),
            n(self.browser_order.len()),
        ]
    }

    /// The groups and order lists with what they name that is gone dropped, or `None` when
    /// nothing is: groups of a missing component or folder, a parent that is missing, elsewhere
    /// or loops, entries for groups and sketches, construction items and placements that no
    /// longer exist, repeats, and order lists of missing components. Body names are left alone:
    /// only an evaluated model knows them.
    pub fn browser_pruned(&self) -> Option<(Vec<BrowserGroup>, BTreeMap<String, Vec<BrowserEntry>>)> {
        if self.browser_groups.is_empty() && self.browser_order.is_empty() {
            return None;
        }
        let components: BTreeSet<u64> = self.components.iter().map(|c| c.id).collect();
        let features: BTreeSet<u64> = self.features.iter().map(|f| f.id).collect();
        let placed_in: BTreeMap<u64, u64> = self.occurrences.iter().map(|o| (o.id, o.parent)).collect();
        let component_exists = |c: u64| c == 0 || components.contains(&c);
        let placed = |component: u64, key: &str| key.parse::<u64>().ok().and_then(|o| placed_in.get(&o)) == Some(&component);
        // Body names cannot be checked here; the other folders hold feature ids.
        let item_exists = |folder: &str, component: u64, key: &str| match folder {
            "bodies" => true,
            "components" => placed(component, key),
            _ => key.parse::<u64>().is_ok_and(|id| features.contains(&id)),
        };
        let mut groups = self.browser_groups.clone();
        let mut order = self.browser_order.clone();
        groups.retain(|g| component_exists(g.component) && BROWSER_FOLDERS.contains(&g.folder.as_str()));
        let known: BTreeMap<u64, (u64, String)> = groups.iter().map(|g| (g.id, (g.component, g.folder.clone()))).collect();
        let parents: BTreeMap<u64, Option<u64>> = groups
            .iter()
            .map(|g| (g.id, g.parent.filter(|p| *p != g.id && known.get(p).is_some_and(|(c, f)| *c == g.component && *f == g.folder))))
            .collect();
        // A loop, or a chain too deep, is cut at the group that makes it.
        let too_deep = |id: u64| std::iter::successors(Some(id), |g| parents.get(g).copied().flatten()).nth(MAX_GROUP_NESTING).is_some();
        let kept: BTreeMap<u64, Option<u64>> = parents.iter().map(|(id, p)| (*id, if too_deep(*id) { None } else { *p })).collect();
        for g in &mut groups {
            g.parent = kept[&g.id];
        }
        let mut taken: BTreeSet<(u64, String, String)> = BTreeSet::new();
        for g in &mut groups {
            let (component, folder, id) = (g.component, g.folder.clone(), g.id);
            let mut seen = BTreeSet::new();
            g.items.retain(|e| match e {
                BrowserEntry::Group(c) => kept.get(c) == Some(&Some(id)) && seen.insert(*c),
                BrowserEntry::Item(k) => item_exists(&folder, component, k) && taken.insert((component, folder.clone(), k.clone())),
            });
        }
        let top = |c: u64, folder: &str, id: u64| kept.get(&id) == Some(&None) && known.get(&id).is_some_and(|(kc, kf)| *kc == c && kf == folder);
        order.retain(|key, list| {
            let Some((c, folder)) = key.split_once('/') else { return false };
            let Some(c) = c.parse::<u64>().ok().filter(|c| component_exists(*c)) else { return false };
            if !BROWSER_FOLDERS.contains(&folder) {
                return false;
            }
            let mut seen = BTreeSet::new();
            list.retain(|e| match e {
                BrowserEntry::Group(id) => top(c, folder, *id) && seen.insert(e.clone()),
                BrowserEntry::Item(k) => item_exists(folder, c, k) && seen.insert(e.clone()),
            });
            !list.is_empty()
        });
        (groups != self.browser_groups || order != self.browser_order).then_some((groups, order))
    }

    /// Put placements of one component in the order the browser shows them: the saved
    /// `components` order, then the rest as they were.
    pub fn sort_placements(&self, parent: u64, ids: &mut [u64]) {
        let Some(order) = self.browser_order.get(&format!("{parent}/components")) else { return };
        let rank: BTreeMap<&str, usize> = order.iter().enumerate().filter_map(|(i, e)| Some((e.as_item()?, i))).collect();
        ids.sort_by_cached_key(|id| rank.get(id.to_string().as_str()).copied().unwrap_or(usize::MAX));
    }
}
