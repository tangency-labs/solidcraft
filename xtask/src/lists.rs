//! `cargo xtask lists [--fix]`: files that many pull requests add to are kept in a canonical,
//! sorted form, so two changes that each add an item meet only when their items are neighbours
//! (CONTRIBUTING.md, "Lists many changes add to"). Checked by `cargo xtask ci` and the Lists
//! workflow; `--fix` rewrites them.
//!
//! - The message catalogs (`assets/locales/*.json`): one message per line, sorted by id, no
//!   duplicate ids, as `serde_json` pretty-prints them.
//! - `tests/KNOWN_FAILING_TESTS`: one test a line with its cause, sorted, each test once
//!   (checked only; `cargo xtask known-failing` reads it).
//! - Growing Rust lists: no written array lengths on more than ten lines of entries.

use std::path::Path;

use serde_json::{Map, Value};

const LOCALES: &str = "assets/locales";

pub fn run(root: &Path, fix: bool) -> Result<(), String> {
    let dir = root.join(LOCALES);
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    let mut stale = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let canonical = canonical(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if canonical == text.replace("\r\n", "\n") {
            continue;
        }
        if fix {
            std::fs::write(&path, &canonical).map_err(|e| format!("{}: {e}", path.display()))?;
            println!("lists: sorted {}", path.display());
        } else {
            stale.push(path.strip_prefix(root).unwrap_or(&path).display().to_string());
        }
    }
    if !stale.is_empty() {
        return Err(format!(
            "not sorted one message per line (or an id appears twice): {}\nrun `cargo xtask lists --fix` and commit the result",
            stale.join(", ")
        ));
    }
    check_arrays(root)?;
    crate::pending::check_locales(root)?;
    crate::pending::check_attribution(root)?;
    crate::known_failing::check_list(root)?;
    println!("lists: growing arrays checked; message catalogs and the known-failing tests sorted; pending locale and attribution files fold cleanly");
    Ok(())
}

/// A catalog as it should be written: its messages sorted by id, one per line. A repeated id
/// keeps its last message, the one the app reads.
pub fn canonical(text: &str) -> Result<String, String> {
    let map: Map<String, Value> = match serde_json::from_str(text).map_err(|e| e.to_string())? {
        Value::Object(map) => map,
        _ => return Err("a catalog must be a JSON object".into()),
    };
    if let Some((id, _)) = map.iter().find(|(_, v)| !v.is_string()) {
        return Err(format!("message `{id}` is not a string"));
    }
    Ok(serde_json::to_string_pretty(&Value::Object(map)).map_err(|e| e.to_string())? + "\n")
}

/// Fixed tables whose dimensions describe a standard or an algorithm, rather than a library.
const FIXED_ARRAYS: &[(&str, &str)] = &[
    ("assets/icons/app/shared/source/app_icon.rs", "FACES"), // Fixed five-face application mark geometry.
    ("crates/doc/src/component_color.rs", "PALETTE"),        // Fixed 24-step hue-wheel algorithm.
    ("crates/doc/src/eval.rs", "ISO_COARSE"),                // ISO metric coarse diameter/pitch pairs.
    ("crates/doc/src/format.rs", "JSON_STEPS"),              // Frozen historical migrations; additions use format/pending.
    ("crates/engine/src/cmd/features.rs", "TABLE"),          // ISO 273 metric bolt clearance-hole dimensions.
    ("crates/engine/src/cmd/parts.rs", "BEARINGS"),          // Standard deep-groove ball bearing dimensions.
    ("crates/engine/src/cmd/parts.rs", "HEX_BOLT"),          // ISO 4017 hex bolt dimensions.
    ("crates/engine/src/cmd/parts.rs", "HEX_NUT"),           // ISO 4032 hex nut dimensions.
    ("crates/engine/src/cmd/parts.rs", "METRIC"),            // Standard metric screw diameters and pitches.
    ("crates/engine/src/cmd/parts.rs", "SHCS"),              // ISO 4762 socket-head cap screw dimensions.
    ("crates/engine/src/cmd/parts.rs", "WASHER"),            // ISO 7089 plain washer dimensions.
    ("crates/ui-egui/src/dialogs_assembly.rs", "TYPES"),     // Joint types; TYPE_LABELS must stay the same length.
    ("crates/ui-egui/src/dialogs_assembly.rs", "TYPE_LABELS"), // Labels for TYPES, index for index.
    ("crates/ui-egui/src/space_mouse_calibration.rs", "STEP_PROMPTS"), // Labels for the twelve fixed calibration motions.
    ("crates/ui-egui/src/space_mouse_calibration.rs", "TRACKBALL_PROMPTS"), // Trackball labels indexed by the same twelve motions.
];

fn test_only(attrs: &[syn::Attribute]) -> bool {
    fn requires_test(meta: &syn::Meta) -> bool {
        match meta {
            syn::Meta::Path(path) => path.is_ident("test"),
            syn::Meta::List(list) => {
                let Ok(items) = list.parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated) else {
                    return false;
                };
                if list.path.is_ident("all") {
                    items.iter().any(requires_test)
                } else {
                    list.path.is_ident("any") && !items.is_empty() && items.iter().all(requires_test)
                }
            }
            _ => false,
        }
    }
    attrs.iter().any(|a| a.path().is_ident("test") || (a.path().is_ident("cfg") && a.parse_args::<syn::Meta>().is_ok_and(|m| requires_test(&m))))
}

#[derive(Default)]
struct ArrayGuard {
    found: Vec<String>,
}

impl ArrayGuard {
    fn array(&mut self, name: &syn::Ident, ty: &syn::Type, expr: &syn::Expr) {
        use syn::spanned::Spanned;
        fn initializer(expr: &syn::Expr) -> Option<&syn::ExprArray> {
            match expr {
                syn::Expr::Array(array) => Some(array),
                syn::Expr::Paren(paren) => initializer(&paren.expr),
                syn::Expr::Group(group) => initializer(&group.expr),
                syn::Expr::Block(block) => match block.block.stmts.last()? {
                    syn::Stmt::Expr(expr, None) => initializer(expr),
                    _ => None,
                },
                _ => None,
            }
        }
        if !matches!(ty, syn::Type::Array(_)) {
            return;
        }
        let count = if let Some(array) = initializer(expr) {
            let mut lines = std::collections::BTreeSet::new();
            for entry in &array.elems {
                lines.extend(entry.span().start().line..=entry.span().end().line);
            }
            lines.len()
        } else {
            // Macro and computed initializers still carry a written array length.
            expr.span().end().line.saturating_sub(expr.span().start().line + 1)
        };
        if count > 10 {
            self.found.push(name.to_string());
        }
    }
}

impl<'ast> syn::visit::Visit<'ast> for ArrayGuard {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let attrs = match item {
            syn::Item::Mod(i) => &i.attrs,
            syn::Item::Fn(i) => &i.attrs,
            syn::Item::Const(i) => &i.attrs,
            syn::Item::Static(i) => &i.attrs,
            syn::Item::Impl(i) => &i.attrs,
            syn::Item::Trait(i) => &i.attrs,
            _ => return syn::visit::visit_item(self, item),
        };
        if !test_only(attrs) {
            syn::visit::visit_item(self, item);
        }
    }

    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        self.array(&item.ident, &item.ty, &item.expr);
        syn::visit::visit_item_const(self, item);
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        self.array(&item.ident, &item.ty, &item.expr);
        syn::visit::visit_item_static(self, item);
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        let attrs = match item {
            syn::ImplItem::Const(i) => &i.attrs,
            syn::ImplItem::Fn(i) => &i.attrs,
            _ => return syn::visit::visit_impl_item(self, item),
        };
        if !test_only(attrs) {
            syn::visit::visit_impl_item(self, item);
        }
    }

    fn visit_impl_item_const(&mut self, item: &'ast syn::ImplItemConst) {
        self.array(&item.ident, &item.ty, &item.expr);
        syn::visit::visit_impl_item_const(self, item);
    }
    fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
        let attrs = match item {
            syn::TraitItem::Const(i) => &i.attrs,
            syn::TraitItem::Fn(i) => &i.attrs,
            _ => return syn::visit::visit_trait_item(self, item),
        };
        if !test_only(attrs) {
            syn::visit::visit_trait_item(self, item);
        }
    }

    fn visit_trait_item_const(&mut self, item: &'ast syn::TraitItemConst) {
        if let Some((_, expr)) = &item.default {
            self.array(&item.ident, &item.ty, expr);
        }
        syn::visit::visit_trait_item_const(self, item);
    }
}

fn array_violations(source: &str) -> Result<Vec<String>, String> {
    use syn::visit::Visit;
    let ast = syn::parse_file(source).map_err(|e| e.to_string())?;
    if test_only(&ast.attrs) {
        return Ok(Vec::new());
    }
    let mut guard = ArrayGuard::default();
    guard.visit_file(&ast);
    Ok(guard.found)
}

fn check_arrays(root: &Path) -> Result<(), String> {
    fn scan(root: &Path, dir: &Path, failures: &mut Vec<String>) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if kind.is_dir() {
                if !matches!(name.as_ref(), "target" | ".git" | "tests" | "benches") {
                    scan(root, &path, failures)?;
                }
            } else if kind.is_file() && path.extension().is_some_and(|e| e == "rs") && name != "tests.rs" && !name.ends_with("_tests.rs") {
                let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                let source = std::fs::read_to_string(&path).map_err(|e| format!("{relative}: {e}"))?;
                for name in array_violations(&source).map_err(|e| format!("{relative}: {e}"))? {
                    if !FIXED_ARRAYS.contains(&(relative.as_str(), name.as_str())) {
                        failures.push(format!("{relative}: {name}"));
                    }
                }
            }
        }
        Ok(())
    }
    let mut failures = Vec::new();
    for dir in ["apps", "assets", "crates", "tools", "xtask"] {
        scan(root, &root.join(dir), &mut failures)?;
    }
    failures.sort();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("use slices without written lengths for growing lists (fixed tables need a reason in FIXED_ARRAYS):\n{}", failures.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::{array_violations, canonical};

    #[test]
    fn growing_arrays_reject_lengths_and_allow_slices_short_tables_and_test_fixtures() {
        let entries = "    0,\n".repeat(11);
        let array = format!("const GROWING: [u8; 11] = [\n{entries}];");
        assert_eq!(array_violations(&array).unwrap(), ["GROWING"]);
        assert_eq!(array_violations(&array.replace("const", "static")).unwrap(), ["GROWING"]);
        assert_eq!(array_violations(&format!("const GENERATED: [u8; 11] = entries![\n{entries}];")).unwrap(), ["GENERATED"]);
        assert_eq!(array_violations(&format!("impl Catalog {{ {array} }}")).unwrap(), ["GROWING"]);
        assert!(array_violations(&array.replace("[u8; 11] = [", "&[u8] = &[ ")).unwrap().is_empty());
        assert!(array_violations(&format!("#[cfg(test)] mod tests {{ {array} }}")).unwrap().is_empty());
        assert!(array_violations(&format!("#![cfg(test)]\n{array}")).unwrap().is_empty());
        assert!(array_violations(&format!("#[cfg(all(test, unix))] mod tests {{ {array} }}")).unwrap().is_empty());
        assert_eq!(array_violations(&format!("#[cfg(not(test))] mod production {{ {array} }}")).unwrap(), ["GROWING"]);
        assert_eq!(array_violations(&format!("trait Catalog {{ {array} }}")).unwrap(), ["GROWING"]);
        assert!(array_violations(&format!("#[test] fn fixture() {{ {array} }}")).unwrap().is_empty());
        assert!(array_violations("const SHORT: [u8; 100] = [0; 100];").unwrap().is_empty());
        assert!(array_violations(&format!("const TEN: [u8; 10] = [\n{}];", "0,\n".repeat(10))).unwrap().is_empty());
        assert!(array_violations(&format!("const TEXT: &str = {array:?};")).unwrap().is_empty());
    }

    #[test]
    fn catalogs_are_sorted_one_per_line_and_a_repeated_id_keeps_its_last_message() {
        let text = "{\n  \"b\": \"B\",\n  \"a\": \"A\",\n  \"b\": \"B2\"\n}";
        assert_eq!(canonical(text).unwrap(), "{\n  \"a\": \"A\",\n  \"b\": \"B2\"\n}\n");
        assert!(canonical("[]").is_err());
        assert!(canonical("{\"a\": 1}").is_err());
    }
}
