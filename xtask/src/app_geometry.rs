//! `cargo xtask app-geometry [--print]`: apps compute no geometry (`solidcore/docs/layers.md` §4,
//! RFC 0003 row 0.1). Vector maths on coordinates, solves and mesh queries belong in the kernel
//! (solidcore); the crates here hold state and call it. The check keeps a ledger of the functions
//! that still compute geometry, [`LIST`], and the ledger only shrinks: a refactor PR deletes its
//! lines and no PR adds one.
//!
//! The heuristic, deliberately plain. The check reads the non-test Rust of the crates in
//! [`CRATES`], following each crate's module tree from its `lib.rs` (a `#[cfg(test)]` or
//! `#[test]` item is left out), and flags every function whose body (closures, nested blocks and
//! expression macros such as `format!` included) uses at least one geometry operation, named by
//! [`geometry_use`]:
//! - vector maths, as a method call: `.dot(v)`, `.cross(v)`, `.normalize()`, `.normalized()`,
//!   `.magnitude()`, `.magnitude2()`, `.distance(p)`, `.distance2(p)`, `.hypot(y)`, `.atan2(x)`,
//!   `.angle(v)`;
//! - solves: `.solve(..)`;
//! - mesh queries: `.raycast(..)`, `.tangent_chain(..)`, the `.triangles` field and
//!   `closest_on_triangle(..)`.
//!
//! It is by name, not by type: a method with one of those names counts whoever defines it. It
//! does not grade `sqrt`, `sin`, `cos`, `length`, `lerp` or plain arithmetic (too common in
//! layout, colour and timing code), so the ledger is a floor of the sites, not a complete count.
//! A function is `<path>:<scope>`, the enclosing inline modules, impl type and function joined
//! by `::`. Nested functions and closures count toward the function they sit in; two functions
//! with one name in one impl type (trait impls) are one site.
//!
//! [`LIST`] holds one site a line, `<path>:<function> <the operations it uses>`, sorted (the
//! kernel rig's known-failing format; the operations are for the reader and are not graded).
//!
//! The check judges a change by what it adds, so drift on main never fails a branch or a merge
//! group. It scans the tree at the base the change started from, [`base`]: `git merge-base HEAD
//! origin/main`, else `HEAD~1` (a pull request's merge commit or a merge-queue group's squash
//! commit, whose first parent is the code it lands on; CI fetches two commits for that), else,
//! with no git history at all, the ledger itself. Then:
//! - a site the tree has and the base does not FAILS, unless the ledger names it (a rename moves
//!   its line);
//! - a site the base already had and the ledger does not name is reported (it came from main);
//! - a listed site that no longer computes geometry or is gone is a warning: its refactor removes
//!   its line.
//!
//! `--print` writes the current sites in the list's format, to regenerate the list or see what a
//! change did.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};

use syn::visit::{self, Visit};

use crate::mesh_decisions::{module_files, relative, test_only, type_name};

pub const LIST: &str = "xtask/app-geometry.txt";

/// The crates that are apps or sit between an app and the kernel: the document, the engine, the
/// UI, file formats, and SolidCraft's own kernel and geometry layers, whose geometry moves into
/// solidcore.
pub const CRATES: &[&str] = &["crates/doc", "crates/engine", "crates/ui-egui", "crates/io", "crates/kernel", "crates/geom"];

/// The geometry operation `call` is, if it is one.
fn geometry_use(call: &Call) -> Option<&'static str> {
    match (call, call.name()) {
        (Call::Method { args: 1, .. }, "dot") => Some("dot"),
        (Call::Method { args: 1, .. }, "cross") => Some("cross"),
        (Call::Method { args: 0, .. }, "normalize") => Some("normalize"),
        (Call::Method { args: 0, .. }, "normalized") => Some("normalized"),
        (Call::Method { args: 0, .. }, "magnitude") => Some("magnitude"),
        (Call::Method { args: 0, .. }, "magnitude2") => Some("magnitude2"),
        (Call::Method { args: 1, .. }, "distance") => Some("distance"),
        (Call::Method { args: 1, .. }, "distance2") => Some("distance2"),
        (Call::Method { args: 1, .. }, "hypot") => Some("hypot"),
        (Call::Method { args: 1, .. }, "atan2") => Some("atan2"),
        (Call::Method { args: 1, .. }, "angle") => Some("angle"),
        (Call::Method { .. }, "solve") => Some("solve"),
        (Call::Method { .. }, "raycast") => Some("raycast"),
        (Call::Method { .. }, "tangent_chain") => Some("tangent_chain"),
        (Call::Function { .. }, "closest_on_triangle") => Some("closest_on_triangle"),
        (Call::Field { .. }, "triangles") => Some("triangles"),
        _ => None,
    }
}

enum Call {
    Method { name: String, args: usize },
    Function { name: String },
    Field { name: String },
}

impl Call {
    fn name(&self) -> &str {
        match self {
            Call::Method { name, .. } | Call::Function { name } | Call::Field { name } => name,
        }
    }
}

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let found = scan(root, false)?;
    if args.contains(&"--print") {
        for (site, uses) in &found {
            println!("{site} {}", uses.join(","));
        }
        return Ok(());
    }
    let text = std::fs::read_to_string(root.join(LIST)).map_err(|e| format!("{LIST}: {e}"))?;
    let base = match base(root) {
        Ok((rev, sites)) => {
            println!("app-geometry: judging what this tree adds to {rev}");
            Some(sites)
        }
        Err(e) => {
            println!("app-geometry: no git base ({e}); judging against {LIST}");
            None
        }
    };
    println!("{}", grade(&text, &found, base.as_ref()).map_err(|e| format!("{LIST}: {e}"))?);
    Ok(())
}

/// The base this tree started from, and its sites: the first of `git merge-base HEAD
/// origin/main` and `HEAD~1` that git has, its crates read with `git archive`.
fn base(root: &Path) -> Result<(String, BTreeSet<String>), String> {
    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new("git").arg("-C").arg(root).args(args).stderr(Stdio::null()).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned()).filter(|s| !s.is_empty())
    };
    let (rev, how) = git(&["merge-base", "HEAD", "origin/main"])
        .map(|r| (r, "merge base with origin/main"))
        .or_else(|| git(&["rev-parse", "--verify", "-q", "HEAD~1^{commit}"]).map(|r| (r, "HEAD~1")))
        .ok_or("neither `git merge-base HEAD origin/main` nor HEAD~1 is available")?;
    let dir = std::env::temp_dir().join(format!("xtask-app-geometry-{}", std::process::id()));
    let sites = sites_at(root, &rev, &dir);
    let _ = std::fs::remove_dir_all(&dir);
    Ok((format!("{} ({how})", &rev[..rev.len().min(10)]), sites?))
}

/// The sites of revision `rev`, its Rust extracted into `dir`.
fn sites_at(root: &Path, rev: &str, dir: &Path) -> Result<BTreeSet<String>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut archive = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["archive", "--format=tar", rev, "--"])
        // The crates' Rust, and the assets' Rust their `#[path]` modules include.
        .args(CRATES.iter().chain(&["assets"]).map(|c| format!(":(glob){c}/**/*.rs")))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("git archive: {e}"))?;
    let stdout = archive.stdout.take().ok_or("git archive: no output")?;
    let tar = Command::new("tar").arg("-x").arg("-C").arg(dir).stdin(stdout).status().map_err(|e| format!("tar: {e}"))?;
    let archived = archive.wait().map_err(|e| format!("git archive: {e}"))?;
    if !archived.success() || !tar.success() {
        return Err(format!("cannot read the crates at {rev}"));
    }
    Ok(scan(dir, true)?.into_keys().collect())
}

/// Every site under `root`: `<path>:<function>` with the operations it uses, sorted. With
/// `missing_ok` (an older revision), a crate that did not exist yet is skipped.
fn scan(root: &Path, missing_ok: bool) -> Result<BTreeMap<String, Vec<&'static str>>, String> {
    let mut found = BTreeMap::new();
    for krate in CRATES {
        let lib = root.join(krate).join("src/lib.rs");
        if missing_ok && !lib.exists() {
            continue;
        }
        let mut files = Vec::new();
        module_files(&lib, true, &mut files)?;
        files.sort();
        files.dedup();
        for path in files {
            let rel = relative(root, &path);
            let source = std::fs::read_to_string(&path).map_err(|e| format!("{rel}: {e}"))?;
            for (function, uses) in sites(&source).map_err(|e| format!("{rel}: {e}"))? {
                found.insert(format!("{rel}:{function}"), uses);
            }
        }
    }
    Ok(found)
}

/// The functions of `source` that compute geometry, with the operations each uses (outside test
/// code), sorted.
pub fn sites(source: &str) -> Result<BTreeMap<String, Vec<&'static str>>, String> {
    let ast = syn::parse_file(source).map_err(|e| format!("cannot parse Rust source: {e}"))?;
    let mut finder = Finder { scope: Vec::new(), fns: 0, found: BTreeMap::new() };
    finder.visit_file(&ast);
    for uses in finder.found.values_mut() {
        uses.sort_unstable();
    }
    Ok(finder.found)
}

struct Finder {
    /// The enclosing inline modules, impl type and function, outermost first.
    scope: Vec<String>,
    /// How many functions enclose the code being visited: a use belongs to the outermost one.
    fns: usize,
    found: BTreeMap<String, Vec<&'static str>>,
}

impl Finder {
    fn note(&mut self, call: Call) {
        let Some(what) = geometry_use(&call) else { return };
        // Outside any function (a const or static initialiser) the use belongs to that item.
        let site = if self.scope.is_empty() { "<module>".to_owned() } else { self.scope.join("::") };
        let uses = self.found.entry(site).or_default();
        if !uses.contains(&what) {
            uses.push(what);
        }
    }

    fn scoped(&mut self, name: String, f: impl FnOnce(&mut Self)) {
        self.scope.push(name);
        f(self);
        self.scope.pop();
    }

    /// A function: inside another function it joins that one (a nested `fn` is part of its
    /// parent's site), otherwise it is a site of its own.
    fn function(&mut self, name: String, f: impl FnOnce(&mut Self)) {
        if self.fns > 0 {
            f(self);
        } else {
            self.fns += 1;
            self.scoped(name, f);
            self.fns -= 1;
        }
    }

    /// A macro's arguments, when they are expressions (`format!`, `vec!`, `assert!`).
    fn macro_args(&mut self, mac: &syn::Macro) {
        let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
        if let Ok(args) = mac.parse_body_with(parser) {
            for e in &args {
                self.visit_expr(e);
            }
        } else if let Ok(block) = mac.parse_body_with(syn::Block::parse_within) {
            for s in &block {
                self.visit_stmt(s);
            }
        }
    }
}

impl<'ast> Visit<'ast> for Finder {
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if !test_only(&m.attrs) && m.content.is_some() {
            self.scoped(m.ident.to_string(), |f| visit::visit_item_mod(f, m));
        }
    }

    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        if !test_only(&i.attrs) {
            self.function(i.sig.ident.to_string(), |f| visit::visit_item_fn(f, i));
        }
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        if !test_only(&i.attrs) {
            self.scoped(type_name(&i.self_ty), |f| visit::visit_item_impl(f, i));
        }
    }

    fn visit_impl_item_fn(&mut self, i: &'ast syn::ImplItemFn) {
        if !test_only(&i.attrs) {
            self.function(i.sig.ident.to_string(), |f| visit::visit_impl_item_fn(f, i));
        }
    }

    fn visit_item_trait(&mut self, i: &'ast syn::ItemTrait) {
        if !test_only(&i.attrs) {
            self.scoped(i.ident.to_string(), |f| visit::visit_item_trait(f, i));
        }
    }

    fn visit_trait_item_fn(&mut self, i: &'ast syn::TraitItemFn) {
        if !test_only(&i.attrs) {
            self.function(i.sig.ident.to_string(), |f| visit::visit_trait_item_fn(f, i));
        }
    }

    fn visit_item_const(&mut self, i: &'ast syn::ItemConst) {
        if !test_only(&i.attrs) {
            self.scoped(i.ident.to_string(), |f| visit::visit_item_const(f, i));
        }
    }

    fn visit_item_static(&mut self, i: &'ast syn::ItemStatic) {
        if !test_only(&i.attrs) {
            self.scoped(i.ident.to_string(), |f| visit::visit_item_static(f, i));
        }
    }

    fn visit_stmt(&mut self, s: &'ast syn::Stmt) {
        if let syn::Stmt::Local(l) = s
            && test_only(&l.attrs)
        {
            return;
        }
        visit::visit_stmt(self, s);
    }

    fn visit_expr_method_call(&mut self, e: &'ast syn::ExprMethodCall) {
        self.note(Call::Method { name: e.method.to_string(), args: e.args.len() });
        visit::visit_expr_method_call(self, e);
    }

    fn visit_expr_call(&mut self, e: &'ast syn::ExprCall) {
        if let syn::Expr::Path(p) = e.func.as_ref()
            && let Some(last) = p.path.segments.last()
        {
            self.note(Call::Function { name: last.ident.to_string() });
        }
        visit::visit_expr_call(self, e);
    }

    fn visit_expr_field(&mut self, e: &'ast syn::ExprField) {
        if let syn::Member::Named(name) = &e.member {
            self.note(Call::Field { name: name.to_string() });
        }
        visit::visit_expr_field(self, e);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.macro_args(mac);
        visit::visit_macro(self, mac);
    }
}

/// The crate of a site (`crates/<name>/...`).
fn crate_of(site: &str) -> &str {
    site.strip_prefix("crates/").and_then(|s| s.split('/').next()).unwrap_or("?")
}

/// The per-crate counts of `sites`, in [`CRATES`] order: `doc 12, engine 3, ...`.
fn counts<'a>(sites: impl Iterator<Item = &'a str>) -> String {
    let mut n: BTreeMap<&str, usize> = BTreeMap::new();
    for s in sites {
        *n.entry(crate_of(s)).or_default() += 1;
    }
    CRATES.iter().map(|c| c.trim_start_matches("crates/")).map(|c| format!("{c} {}", n.get(c).unwrap_or(&0))).collect::<Vec<_>>().join(", ")
}

/// The gate. A site in `found` that `base` lacks and `list` does not name fails: this change
/// added it. A site `base` already had that `list` does not name is reported, and a listed site
/// that is not found is a warning; neither fails. Without a base, the list is the base.
fn grade(list: &str, found: &BTreeMap<String, Vec<&'static str>>, base: Option<&BTreeSet<String>>) -> Result<String, String> {
    let listed = crate::kernel_cases::known_failing(list)?;
    let named = |site: &str| listed.iter().any(|(s, _)| *s == site);
    let (mut new, mut from_main, mut stale) = (Vec::new(), Vec::new(), Vec::new());
    for (site, uses) in found {
        if named(site) {
            continue;
        }
        if base.is_some_and(|b| b.contains(site)) {
            from_main.push(format!(
                "UNLISTED: {site} ({}) was already at the base; `cargo xtask app-geometry --print` regenerates the list",
                uses.join(", ")
            ));
        } else {
            new.push(format!(
                "NEW: {site} computes geometry ({}): call the kernel (solidcore ops) for it instead; apps hold state and compute no geometry",
                uses.join(", ")
            ));
        }
    }
    for (site, _) in &listed {
        if !found.contains_key(*site) {
            stale.push(format!("warning: STALE: {site} no longer computes geometry (or is gone): remove its line"));
        }
    }
    let by_crate = counts(listed.iter().map(|(s, _)| *s));
    let notes: Vec<String> = from_main.into_iter().chain(stale).collect();
    let notes = if notes.is_empty() { String::new() } else { format!("\n  {}", notes.join("\n  ")) };
    if new.is_empty() {
        Ok(format!("app-geometry: {} sites compute geometry ({by_crate}); no new ones{notes}", listed.len()))
    } else {
        Err(format!("{} new site(s) (listed: {by_crate}):\n  {}{notes}", new.len(), new.join("\n  ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_is_found_per_function_outside_tests() {
        let s = sites(
            r#"
            fn tilt(n: V, u: V) -> f64 { n.dot(u).acos() }
            struct Rig;
            impl Rig {
                fn frame(&self, a: V, b: V) -> V { let f = |x: V| x.cross(b); f(a).normalized() }
                fn nested(&self) { fn inner(a: V) -> f64 { a.magnitude() } let _ = inner; }
                fn plain(&self, a: f64) -> f64 { a.sqrt() + a.sin() }
                fn arity(&self, a: V) -> f64 { a.dot() }
            }
            fn in_macro(a: V, b: V) -> String { format!("{}", a.distance(b)) }
            fn mesh(m: &Mesh) -> usize { m.triangles.len() }
            #[cfg(test)] mod tests { fn fixture(a: V, b: V) { a.dot(b); } }
            #[test] fn t(a: V, b: V) { a.dot(b); }
            mod inner { fn nearest(p: V) -> V { closest_on_triangle(p, p, p, p) } }
            "#,
        )
        .unwrap();
        let keys: Vec<&str> = s.keys().map(String::as_str).collect();
        assert_eq!(keys, ["Rig::frame", "Rig::nested", "in_macro", "inner::nearest", "mesh", "tilt"]);
        assert_eq!(s["Rig::frame"], ["cross", "normalized"]);
        assert_eq!(s["Rig::nested"], ["magnitude"], "a nested fn counts toward its parent");
        assert!(sites("invalid Rust {{").is_err(), "unparseable source fails closed");
    }

    const LISTED: &str = "# comment\ncrates/doc/src/a.rs:tilt dot\ncrates/ui-egui/src/b.rs:near distance\n";

    fn found(names: &[&str]) -> BTreeMap<String, Vec<&'static str>> {
        names.iter().map(|n| (n.to_string(), vec!["dot"])).collect()
    }

    fn base(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    const BOTH: [&str; 2] = ["crates/doc/src/a.rs:tilt", "crates/ui-egui/src/b.rs:near"];

    #[test]
    fn listed_sites_pass_and_counts_are_per_crate() {
        let ok = grade(LISTED, &found(&BOTH), Some(&base(&BOTH))).unwrap();
        assert!(ok.contains("doc 1, engine 0, ui-egui 1, io 0, kernel 0, geom 0"), "{ok}");
    }

    #[test]
    fn a_site_the_branch_adds_fails() {
        let now = found(&["crates/doc/src/a.rs:tilt", "crates/ui-egui/src/b.rs:near", "crates/doc/src/a.rs:pick"]);
        let e = grade(LISTED, &now, Some(&base(&BOTH))).unwrap_err();
        assert!(e.contains("NEW: crates/doc/src/a.rs:pick"), "{e}");
        let e = grade(LISTED, &now, None).unwrap_err();
        assert!(e.contains("NEW: crates/doc/src/a.rs:pick"), "without a base the list is the base: {e}");
    }

    #[test]
    fn a_site_already_at_the_base_only_reports() {
        let now = found(&["crates/doc/src/a.rs:tilt", "crates/ui-egui/src/b.rs:near", "crates/io/src/c.rs:merged"]);
        let ok = grade(LISTED, &now, Some(&base(&["crates/io/src/c.rs:merged"]))).unwrap();
        assert!(ok.contains("UNLISTED: crates/io/src/c.rs:merged"), "{ok}");
    }

    #[test]
    fn a_stale_listed_site_warns_but_passes() {
        let ok = grade(LISTED, &found(&["crates/doc/src/a.rs:tilt"]), Some(&base(&BOTH))).unwrap();
        assert!(ok.contains("warning: STALE: crates/ui-egui/src/b.rs:near"), "{ok}");
        assert!(grade(LISTED, &found(&["crates/doc/src/a.rs:tilt"]), None).is_ok());
    }

    #[test]
    fn a_renamed_site_passes_once_its_line_moves() {
        let listed = "crates/doc/src/a.rs:tilt_to dot\n";
        assert!(grade(listed, &found(&["crates/doc/src/a.rs:tilt_to"]), Some(&base(&["crates/doc/src/a.rs:tilt"]))).is_ok());
    }

    #[test]
    fn the_list_is_sorted_with_a_reason_each() {
        assert!(grade("b.rs:near x\na.rs:tilt x\n", &found(&["a.rs:tilt", "b.rs:near"]), None).is_err());
        assert!(grade("a.rs:tilt\n", &found(&["a.rs:tilt"]), None).is_err());
    }

    /// Judged against the base this tree started from (in a merge-queue group, the code it lands
    /// on), so only this change's own additions can fail it, never drift on main.
    #[test]
    fn the_workspace_has_no_new_app_geometry() {
        run(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap(), &[]).unwrap();
    }
}
