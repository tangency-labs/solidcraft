//! Linked designs (Link Parameters): the rule for bringing a linked design's parameters into a
//! design, changing them and taking them out, as pure changes to the document. The engine reads
//! the files; this decides what their values may change, so link, update, relink and break share
//! one rule and its tests need no files.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::params::Owner;
use crate::{DocError, Document, LinkedDesign, LinkedParam, MAX_LINKED_DESIGNS, MAX_PARAMS, ParamUser, Result};

/// What giving a linked design new parameters changes.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct LinkedChange {
    /// Parameters whose value changed.
    pub changed: Vec<ValueChange>,
    /// Parameters the design has now and didn't have.
    pub added: Vec<String>,
    /// Parameters the design no longer has.
    pub removed: Vec<String>,
}

impl LinkedChange {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.added.is_empty() && self.removed.is_empty()
    }
}

/// One parameter's value before and after, as their text (`8.2 mm` → `8.5 mm`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ValueChange {
    pub name: String,
    pub from: String,
    pub to: String,
}

impl Document {
    /// What a design file holds that linking would have refused, as it can come from a merge or
    /// a hand edit: a name this design owns and a linked design has too, a name two linked
    /// designs have, or one design linked twice. The design still opens and evaluates (its own
    /// parameter wins, then the first linked design); this says so, one line each.
    pub fn linked_clashes(&self) -> Vec<String> {
        let file = |d: &LinkedDesign| d.path.rsplit(['/', '\\']).next().unwrap_or(&d.path).to_string();
        let mut out = Vec::new();
        for (i, design) in self.linked_designs.iter().enumerate() {
            if let Some(first) = self.linked_designs.iter().take(i).find(|d| d.path == design.path) {
                out.push(format!("`{}` is linked twice: its first link is used", file(first)));
                continue;
            }
            for p in &design.params {
                match self.owner(&p.name) {
                    Some(Owner::Linked { design: used, .. }) if !std::ptr::eq(used, design) => out.push(format!(
                        "`{}` comes from both `{}` and `{}`: the value from `{}` is used",
                        p.name,
                        file(used),
                        file(design),
                        file(used)
                    )),
                    Some(Owner::User(_) | Owner::Model(_) | Owner::Input { .. }) => {
                        out.push(format!("`{}` is both this design's parameter and one from `{}`: this design's own is used", p.name, file(design)))
                    }
                    Some(Owner::Linked { .. }) | None => {}
                }
            }
        }
        out
    }

    /// What giving linked design `index` (`None`: a new one) the parameters `params` changes, or
    /// why it can't: two parameters with one name, a name this design already uses for something
    /// else, a parameter that would go while something here still uses it, or too many.
    /// Values are compared in their text form, so float noise is never a change.
    pub fn linked_change(&self, index: Option<usize>, params: &[LinkedParam]) -> Result<LinkedChange> {
        let current = match index {
            Some(i) => Some(self.linked_designs.get(i).ok_or_else(|| DocError::Unknown("linked design".into()))?),
            None if self.linked_designs.len() >= MAX_LINKED_DESIGNS => {
                return Err(DocError::Invalid(format!("a design can link parameters from at most {MAX_LINKED_DESIGNS} designs")));
            }
            None => None,
        };
        let old: &[LinkedParam] = current.map_or(&[], |d| d.params.as_slice());
        let mut names = BTreeSet::new();
        if let Some(p) = params.iter().find(|p| !names.insert(p.name.as_str())) {
            return Err(DocError::Invalid(format!("the linked design has two parameters named `{}`", p.name)));
        }
        let others = self.linked_params().count().saturating_sub(old.len());
        if others.saturating_add(params.len()) > MAX_PARAMS {
            return Err(DocError::Invalid("too many linked parameters".into()));
        }
        let mut clash = Vec::new();
        for p in params {
            match self.owner(&p.name) {
                None => {}
                Some(Owner::Linked { design, .. }) if current.is_some_and(|c| std::ptr::eq(c, design)) => {}
                Some(Owner::User(_) | Owner::Model(_) | Owner::Input { .. } | Owner::Linked { .. }) => clash.push(p.name.as_str()),
            }
        }
        if !clash.is_empty() {
            return Err(DocError::Invalid(format!(
                "this design already uses the names {}; rename them here or in the linked design first",
                clash.join(", ")
            )));
        }
        let removed: Vec<String> = old.iter().filter(|o| !names.contains(o.name.as_str())).map(|o| o.name.clone()).collect();
        for name in &removed {
            let users = self.param_users(name);
            if !users.is_empty() {
                let list: Vec<String> = users.iter().map(ParamUser::describe).collect();
                return Err(DocError::Invalid(format!(
                    "`{name}` is no longer in the linked design, but {} use it (a renamed parameter counts as removed)",
                    list.join(", ")
                )));
            }
        }
        let before = |n: &str| old.iter().find(|o| o.name == n);
        let changed = params
            .iter()
            .filter_map(|p| {
                before(&p.name).filter(|o| o.expr != p.expr).map(|o| ValueChange { name: p.name.clone(), from: o.expr.clone(), to: p.expr.clone() })
            })
            .collect();
        let added = params.iter().filter(|p| before(&p.name).is_none()).map(|p| p.name.clone()).collect();
        Ok(LinkedChange { changed, added, removed })
    }

    /// Give linked design `index` (`None`: add `design` as a new one) its new file, scope and
    /// parameters, after the checks of [`Document::linked_change`]. Favourites of parameters
    /// that went are dropped.
    pub fn set_linked(&mut self, index: Option<usize>, design: LinkedDesign) -> Result<LinkedChange> {
        let change = self.linked_change(index, &design.params)?;
        for name in &change.removed {
            self.favorites.remove(name);
        }
        match index.and_then(|i| self.linked_designs.get_mut(i)) {
            Some(slot) => *slot = design,
            None => self.linked_designs.push(design),
        }
        Ok(change)
    }

    /// Break linked design `index`: its parameters become this design's own user parameters
    /// with the values last read, and the file is forgotten. Returns their names.
    pub fn break_linked(&mut self, index: usize) -> Result<Vec<String>> {
        let design = self.linked_designs.get(index).cloned().ok_or_else(|| DocError::Unknown("linked design".into()))?;
        if self.params.len().saturating_add(design.params.len()) > MAX_PARAMS {
            return Err(DocError::Invalid("too many parameters".into()));
        }
        if index < self.linked_designs.len() {
            self.linked_designs.remove(index);
        }
        let names = design.params.iter().map(|p| p.name.clone()).collect();
        self.params.extend(design.params.into_iter().map(|p| crate::Param {
            name: p.name,
            expr: p.expr,
            unit: p.unit,
            comment: p.comment,
            model: false,
        }));
        Ok(names)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::ParamScope;

    fn p(name: &str, expr: &str) -> LinkedParam {
        LinkedParam { name: name.into(), expr: expr.into(), unit: "mm".into(), comment: String::new() }
    }

    fn design(params: Vec<LinkedParam>) -> LinkedDesign {
        LinkedDesign { path: "/std/standard.solidcraft".into(), scope: ParamScope::All, params }
    }

    #[test]
    fn an_update_reports_what_changed_and_keeps_float_noise_out() {
        let mut d = Document::default();
        let made = d.set_linked(None, design(vec![p("hole", "8.2 mm"), p("len", "9.5 mm")])).unwrap();
        assert_eq!(made.added, ["hole", "len"]);
        d.set_favorite("len", true).unwrap();
        let change = d.set_linked(Some(0), design(vec![p("hole", "8.5 mm"), p("depth", "10.9 mm")])).unwrap();
        assert_eq!(change.changed, [ValueChange { name: "hole".into(), from: "8.2 mm".into(), to: "8.5 mm".into() }]);
        assert_eq!((change.added, change.removed), (vec!["depth".to_string()], vec!["len".to_string()]));
        assert!(!d.favorites.contains("len"), "the favourite of a parameter that went is dropped");
        assert!(d.linked_change(Some(0), &d.linked_designs[0].params.clone()).unwrap().is_empty());
    }

    #[test]
    fn a_clash_or_a_parameter_still_in_use_refuses_and_changes_nothing() {
        let mut d = Document::default();
        d.add_user_param("wall", "2 mm", None, None).unwrap();
        let e = d.set_linked(None, design(vec![p("wall", "3 mm"), p("hole", "8 mm")])).unwrap_err().to_string();
        assert!(e.contains("wall") && !e.contains("hole"), "{e}");
        d.set_linked(None, design(vec![p("hole", "8 mm")])).unwrap();
        d.add_user_param("twice", "hole * 2", None, None).unwrap();
        let before = d.clone();
        let e = d.set_linked(Some(0), design(vec![p("bore", "8 mm")])).unwrap_err().to_string();
        assert!(e.contains("hole") && e.contains("twice") && e.contains("renamed"), "{e}");
        let e = d.set_linked(Some(0), design(vec![p("hole", "8 mm"), p("hole", "9 mm")])).unwrap_err().to_string();
        assert!(e.contains("two parameters"), "{e}");
        // A second linked design may not bring a name the first one has.
        let e = d.set_linked(None, design(vec![p("hole", "8 mm")])).unwrap_err().to_string();
        assert!(e.contains("hole"), "{e}");
        assert_eq!(d, before);
        assert!(d.set_linked(Some(7), design(vec![])).is_err());
    }

    /// A merged or hand-made file can hold what linking refuses. It still evaluates, by the same
    /// rule edits follow (this design's own parameter, then the first linked design), and says so.
    #[test]
    fn a_file_holding_a_clash_evaluates_by_the_edit_rule_and_says_so() {
        let mut d = Document::default();
        d.add_user_param("hole", "5 mm", None, None).unwrap();
        let mut other = design(vec![p("bore", "9 mm")]);
        other.path = "/std/other.solidcraft".into();
        d.linked_designs = vec![design(vec![p("hole", "8 mm"), p("bore", "7 mm")]), other, design(vec![p("len", "1 mm")])];
        let (vals, errs) = d.param_values();
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!((vals["hole"].v, vals["bore"].v), (5.0, 7.0));
        assert!(matches!(d.owner("hole"), Some(Owner::User(_))));
        assert_eq!(d.param_rows().iter().filter(|r| r.name == "hole" || r.name == "bore").count(), 2);
        let clashes = d.linked_clashes();
        assert_eq!(clashes.len(), 3, "{clashes:?}");
        assert!(clashes[0].contains("`hole` is both this design's parameter"), "{clashes:?}");
        assert!(clashes[1].contains("`bore` comes from both `standard.solidcraft` and `other.solidcraft`"), "{clashes:?}");
        assert!(clashes[2].contains("linked twice"), "{clashes:?}");
        assert!(Document::default().linked_clashes().is_empty());
    }

    #[test]
    fn break_keeps_the_values_as_user_parameters() {
        let mut d = Document::default();
        d.set_linked(None, design(vec![p("hole", "8.2 mm")])).unwrap();
        assert_eq!(d.break_linked(0).unwrap(), ["hole"]);
        assert!(d.linked_designs.is_empty());
        assert!(matches!(d.owner("hole"), Some(Owner::User(p)) if p.expr == "8.2 mm"));
        assert!(d.break_linked(0).is_err());
    }
}
