//! `cargo xtask mesh-decisions`: model decisions use exact geometry. What an operation does,
//! which bodies it touches, which face or edge a point or a name refers to, depths and nearness
//! are worked out from the exact B-rep (SMLib: `Body::exact_bounds`, `apart` in
//! `crates/kernel/src/ops.rs`, point classification, projection). A mesh, a body's display
//! tessellation or an imported mesh body, is for display, export, picking previews and measuring
//! mesh bodies.
//!
//! The check reads the non-test Rust of the model crates ([`CRATES`]), following each crate's
//! module tree from its `lib.rs` (a `#[cfg(test)]` module, function or impl is left out), and
//! finds the functions that use a mesh ([`site_uses`] names the patterns). The modules and the
//! functions whose job is display, export, previews or mesh bodies are skipped
//! ([`SKIPPED_MODULES`], [`SKIPPED_FUNCTIONS`], each with its reason).
//! Every other function that uses a mesh is a decision site. The sites there are today are
//! listed in [`LIST`], one a line, `<path>:<function> <what it decides with the mesh>`, sorted
//! (the kernel rig's known-failing format). The list only shrinks: the check fails on a site the
//! list does not name, and on a listed site that no longer uses a mesh (its fix removes its
//! line), as `tests/KNOWN_FAILING_TESTS` and `schema/unconverted.txt` do.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use syn::visit::{self, Visit};

pub const LIST: &str = "xtask/mesh-decisions.txt";

/// The crates whose code decides geometry: the document, the engine and the kernel.
pub const CRATES: &[&str] = &["crates/doc", "crates/engine", "crates/kernel"];

/// The crates where `ModelBody::mesh()` (no argument) is a display mesh. In the kernel, the
/// one argument-less `mesh()` is a T-spline's control net (`geom::tspline::TSpline::mesh`),
/// which is exact geometry, and `ModelBody` does not exist there.
const MODEL_BODY_CRATES: &[&str] = &["crates/doc", "crates/engine"];

/// Modules (a file, or a directory with everything below it) whose job is display, export,
/// previews or mesh bodies: their mesh use is what a mesh is for.
pub const SKIPPED_MODULES: &[(&str, &str)] = &[
    ("crates/doc/src/display_quality.rs", "the viewport's display quality levels"),
    ("crates/doc/src/placed_mesh.rs", "the display meshes and plans of bodies and placed parts"),
    ("crates/engine/src/cmd/file_thumbnail.rs", "a design's thumbnail picture"),
    ("crates/engine/src/cmd/mesh.rs", "mesh bodies: repair, decimate and inspect imported meshes"),
    ("crates/engine/src/cmd/render", "rendered pictures"),
    ("crates/engine/src/cmd/render.rs", "rendered pictures"),
    ("crates/engine/src/cmd/silhouette.rs", "a picture's view-aligned silhouette of the display meshes"),
    ("crates/engine/src/cmd/surface_analysis.rs", "view analyses: zebra, draft and curvature shading of the display mesh"),
    ("crates/engine/src/cmd/view_dxf_meshes.rs", "a drawing's view lines from display meshes"),
    ("crates/engine/src/cmd/view_dxf_scene.rs", "a drawing's hidden-line occlusion over display meshes"),
    ("crates/engine/src/cosmetic_threads.rs", "cosmetic thread hatching drawn on the displayed triangles"),
    ("crates/engine/src/fuzz.rs", "the fuzz harness, which frames what it built"),
    ("crates/engine/src/product_scene.rs", "the scene a rendered picture draws"),
    ("crates/engine/src/render_jobs.rs", "rendered pictures"),
    ("crates/engine/src/slicer_files.rs", "the mesh export a 3D-printing slicer opens"),
    ("crates/engine/src/snap.rs", "the pointer's snap preview over the display mesh"),
    ("crates/engine/src/view.rs", "the viewport's meshes, edges and highlights"),
    ("crates/kernel/src/mesh_convert", "mesh bodies: turning a mesh body into a solid"),
    ("crates/kernel/src/meshbody.rs", "mesh bodies"),
    ("crates/kernel/src/native/preview.rs", "a feature's preview mesh"),
    ("crates/kernel/src/polybool.rs", "mesh bodies: booleans of a body with a closed triangle mesh"),
    ("crates/kernel/src/tess", "the mesher: a body's display tessellation"),
];

/// Functions, in modules that also decide geometry, whose job is the display mesh itself, a
/// preview, or a mesh body.
pub const SKIPPED_FUNCTIONS: &[(&str, &str)] = &[
    ("crates/doc/src/eval.rs:ImportDigests::of", "mesh-body imports: retained content identities, not tessellated-solid decisions"),
    ("crates/doc/src/eval.rs:MeshDigest::matches", "mesh-body imports: compares retained source-array identities"),
    ("crates/doc/src/eval.rs:ModelBody::display_plan", "the display plan"),
    ("crates/doc/src/eval.rs:ModelBody::mesh", "the display mesh"),
    ("crates/doc/src/eval.rs:ModelBody::mesh_in_place", "the display mesh where the body stands"),
    ("crates/doc/src/eval.rs:ModelBody::plan_mesh", "the display plan's mesh"),
    ("crates/doc/src/eval.rs:ModelBody::plan_mesh_at", "the display plan's mesh"),
    ("crates/doc/src/eval.rs:ModelBody::plan_mesh_in_place_at", "the display plan's mesh where the body stands"),
    ("crates/doc/src/eval.rs:ModelBody::plan_mesh_soon", "the display plan's mesh, made ahead"),
    ("crates/doc/src/eval.rs:ModelBody::refined_display", "the display plan at a finer quality"),
    ("crates/doc/src/eval.rs:ModelState::warm_display", "display meshes made ahead"),
    ("crates/doc/src/eval.rs:ModelState::warm_display_at", "display meshes made ahead"),
    ("crates/doc/src/eval.rs:ModelState::warm_meshes", "display meshes made ahead"),
    ("crates/doc/src/eval.rs:feature_json", "fingerprints feature inputs; triangles are only embedded mesh-body source data"),
    ("crates/doc/src/eval.rs:mesh_bodies", "mesh bodies: a mesh import's triangles"),
    ("crates/doc/src/eval.rs:mesh_content_digest", "mesh-body imports: hashes source arrays without geometric classification"),
    ("crates/engine/src/cmd/file.rs:saved_preview", "the preview picture saved in a design"),
    ("crates/kernel/src/body.rs:Body::classify_point", "mesh bodies: its mesh-body branch (a B-rep is classified by SMLib)"),
    ("crates/kernel/src/body.rs:Body::mesh_untimed", "the mesher: a body's tessellation"),
    ("crates/kernel/src/body.rs:Body::retained_bytes", "the evaluation cache's memory estimate"),
    ("crates/kernel/src/body.rs:Body::unmeshed_faces", "the mesher's report of faces it left out"),
];

/// What a function does with a mesh, the patterns the check matches:
/// - `.mesh(t)` (one argument): `kernel::Body::mesh`, a body's tessellation;
/// - `.mesh()` (none), in [`MODEL_BODY_CRATES`]: `ModelBody::mesh`, its display mesh;
/// - `.mesh_plan(t)`, `.mesh_of(plan)`: the tessellation plan and the mesh it evaluates to;
/// - `.plan_mesh()`, `.plan_mesh_at(q)`, `.display_plan()`, `.display_plan_at(q)`: the display
///   plan and its mesh;
/// - `mesh_box(..)`, `body_box(..)`, `reference_mesh(..)`: a box or mesh from a tessellation;
/// - `.triangles`: reading a mesh's triangles (nearest point, ray casts, inside tests);
/// - `closest_on_triangle(..)`: the nearest point of a triangle.
fn site_uses(call: &Call, model_body: bool) -> Option<&'static str> {
    match (call, call.name()) {
        (Call::Method { args: 1, .. }, "mesh") => Some(".mesh(t)"),
        (Call::Method { args: 0, .. }, "mesh") if model_body => Some(".mesh()"),
        (Call::Method { .. }, "mesh_plan") => Some(".mesh_plan()"),
        (Call::Method { .. }, "mesh_of") => Some(".mesh_of()"),
        (Call::Method { .. }, "plan_mesh" | "plan_mesh_at") => Some(".plan_mesh()"),
        (Call::Method { .. }, "display_plan" | "display_plan_at") => Some(".display_plan()"),
        (Call::Function { .. }, "mesh_box") => Some("mesh_box()"),
        (Call::Function { .. }, "body_box") => Some("body_box()"),
        (Call::Function { .. }, "reference_mesh") => Some("reference_mesh()"),
        (Call::Function { .. }, "closest_on_triangle") => Some("closest_on_triangle()"),
        (Call::Field { .. }, "triangles") => Some(".triangles"),
        _ => None,
    }
}

/// A use in the source that [`site_uses`] grades.
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

pub fn run(root: &Path) -> Result<(), String> {
    println!("{}", check(root, SKIPPED_MODULES, SKIPPED_FUNCTIONS)?);
    Ok(())
}

/// The sites under `root`, these modules and functions skipped, graded against [`LIST`].
fn check(root: &Path, modules: &[(&str, &str)], functions: &[(&str, &str)]) -> Result<String, String> {
    let found = scan_skipping(root, modules, functions)?;
    let text = std::fs::read_to_string(root.join(LIST)).map_err(|e| format!("{LIST}: {e}"))?;
    grade(&text, &found).map_err(|e| format!("{LIST}: {e}"))
}

/// Every decision site under `root`, these modules and functions skipped: `<path>:<function>`
/// with the patterns it uses. A skipped module that does not exist, or a skipped function that
/// does not use a mesh, is an error: the skips stay true.
fn scan_skipping(root: &Path, modules: &[(&str, &str)], functions: &[(&str, &str)]) -> Result<BTreeMap<String, Vec<&'static str>>, String> {
    for (skip, _) in modules {
        if !root.join(skip).exists() {
            return Err(format!("{skip} is skipped but does not exist: take it off SKIPPED_MODULES in xtask/src/mesh_decisions.rs"));
        }
    }
    let mut found = BTreeMap::new();
    for krate in CRATES {
        let lib = root.join(krate).join("src/lib.rs");
        let model_body = MODEL_BODY_CRATES.contains(krate);
        let mut files = Vec::new();
        module_files(&lib, true, &mut files)?;
        files.sort();
        files.dedup();
        for path in files {
            let rel = relative(root, &path);
            if modules.iter().any(|(skip, _)| rel == *skip || rel.starts_with(&format!("{skip}/"))) {
                continue;
            }
            let source = std::fs::read_to_string(&path).map_err(|e| format!("{rel}: {e}"))?;
            for (function, uses) in sites(&source, model_body).map_err(|e| format!("{rel}: {e}"))? {
                found.insert(format!("{rel}:{function}"), uses);
            }
        }
    }
    for (skip, _) in functions {
        if found.remove(*skip).is_none() {
            return Err(format!("{skip} is skipped but uses no mesh: take it off SKIPPED_FUNCTIONS in xtask/src/mesh_decisions.rs"));
        }
    }
    Ok(found)
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// The source files of the module at `file` and of every non-test module below it, into `out`.
/// `owns_dir` is whether `file` is a `lib.rs` or `mod.rs` (its submodules sit beside it) rather
/// than `name.rs` (its submodules sit in `name/`).
fn module_files(file: &Path, owns_dir: bool, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let source = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let ast = syn::parse_file(&source).map_err(|e| format!("{}: cannot parse Rust source: {e}", file.display()))?;
    out.push(file.to_path_buf());
    let here = file.parent().unwrap_or(Path::new("."));
    let dir = if owns_dir { here.to_path_buf() } else { here.join(file.file_stem().unwrap_or_default()) };
    declared_modules(&ast.items, here, &dir, out)
}

/// The out-of-line modules `items` declare. `here` is the directory of the file they are in
/// (`#[path]` resolves against it); `dir` is where their files sit without one.
fn declared_modules(items: &[syn::Item], here: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for item in items {
        let syn::Item::Mod(m) = item else { continue };
        if test_only(&m.attrs) {
            continue;
        }
        let name = m.ident.to_string();
        let path = path_attribute(&m.attrs);
        match &m.content {
            Some((_, inner)) => {
                let base = match &path {
                    Some(p) => here.join(p),
                    None => dir.join(&name),
                };
                declared_modules(inner, &base, &base, out)?;
            }
            None => match path {
                Some(p) => {
                    let file = here.join(p);
                    let owns = file.file_name().is_some_and(|f| f == "mod.rs");
                    module_files(&file, owns, out)?;
                }
                None => {
                    let flat = dir.join(format!("{name}.rs"));
                    if flat.is_file() {
                        module_files(&flat, false, out)?;
                    } else {
                        module_files(&dir.join(&name).join("mod.rs"), true, out)?;
                    }
                }
            },
        }
    }
    Ok(())
}

fn path_attribute(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find(|a| a.path().is_ident("path")).and_then(|a| match &a.meta {
        syn::Meta::NameValue(nv) => match &nv.value {
            syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => Some(s.value()),
            _ => None,
        },
        _ => None,
    })
}

/// Whether `attrs` keep an item to test builds: `#[test]`, `#[cfg(test)]` or
/// `#[cfg(all(.., test, ..))]`.
fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if a.path().is_ident("test") {
            return true;
        }
        if !a.path().is_ident("cfg") {
            return false;
        }
        let Ok(meta) = a.parse_args::<syn::Meta>() else { return false };
        match meta {
            syn::Meta::Path(p) => p.is_ident("test"),
            syn::Meta::List(l) if l.path.is_ident("all") => l
                .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
                .is_ok_and(|items| items.iter().any(|m| m.path().is_ident("test"))),
            _ => false,
        }
    })
}

/// The functions of `source` that use a mesh, with the patterns each uses (outside test code).
pub fn sites(source: &str, model_body: bool) -> Result<BTreeMap<String, Vec<&'static str>>, String> {
    let ast = syn::parse_file(source).map_err(|e| format!("cannot parse Rust source: {e}"))?;
    let mut finder = Finder { model_body, scope: Vec::new(), found: BTreeMap::new() };
    finder.visit_file(&ast);
    Ok(finder.found)
}

struct Finder {
    model_body: bool,
    /// The enclosing inline modules, impl type and function, outermost first.
    scope: Vec<String>,
    found: BTreeMap<String, Vec<&'static str>>,
}

impl Finder {
    fn note(&mut self, call: Call) {
        let Some(what) = site_uses(&call, self.model_body) else { return };
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

    /// A macro's arguments, when they are expressions (`format!`, `vec!`, `assert!`): syn leaves
    /// macro bodies unparsed, and a mesh call inside one is a use all the same.
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

fn type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(p) => p.path.segments.last().map_or_else(|| "<type>".to_owned(), |s| s.ident.to_string()),
        syn::Type::Reference(r) => type_name(&r.elem),
        _ => "<type>".to_owned(),
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
            self.scoped(i.sig.ident.to_string(), |f| visit::visit_item_fn(f, i));
        }
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        if !test_only(&i.attrs) {
            self.scoped(type_name(&i.self_ty), |f| visit::visit_item_impl(f, i));
        }
    }

    fn visit_impl_item_fn(&mut self, i: &'ast syn::ImplItemFn) {
        if !test_only(&i.attrs) {
            self.scoped(i.sig.ident.to_string(), |f| visit::visit_impl_item_fn(f, i));
        }
    }

    fn visit_item_trait(&mut self, i: &'ast syn::ItemTrait) {
        if !test_only(&i.attrs) {
            self.scoped(i.ident.to_string(), |f| visit::visit_item_trait(f, i));
        }
    }

    fn visit_trait_item_fn(&mut self, i: &'ast syn::TraitItemFn) {
        if !test_only(&i.attrs) {
            self.scoped(i.sig.ident.to_string(), |f| visit::visit_trait_item_fn(f, i));
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

/// The gate: every site in `found` is listed in `list`, and every listed site is still found.
fn grade(list: &str, found: &BTreeMap<String, Vec<&'static str>>) -> Result<String, String> {
    let listed = crate::kernel_cases::known_failing(list)?;
    let mut problems = Vec::new();
    for (site, uses) in found {
        if !listed.iter().any(|(s, _)| s == site) {
            problems.push(format!(
                "NEW: {site} decides with a mesh ({}): use exact geometry (`Body::exact_bounds`, `kernel::apart`, the B-rep's \
                 faces and edges); a mesh is for display, export, previews and mesh bodies",
                uses.join(", ")
            ));
        }
    }
    for (site, _) in &listed {
        if !found.contains_key(*site) {
            problems.push(format!("FIXED: {site} no longer uses a mesh: remove its line"));
        }
    }
    if problems.is_empty() {
        Ok(format!("mesh-decisions: {} known decision sites use a mesh; no new ones", listed.len()))
    } else {
        Err(format!("{} problem(s):\n  {}", problems.len(), problems.join("\n  ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(source: &str) -> BTreeMap<String, Vec<&'static str>> {
        sites(source, true).unwrap()
    }

    #[test]
    fn mesh_uses_are_found_per_function_outside_tests() {
        let s = found(
            r#"
            fn decides(b: &Body) -> bool { b.mesh(&tol).is_ok() }
            fn display_mesh(mb: &ModelBody) -> usize { mb.mesh().triangles.len() }
            impl Sheet { fn near(&self) -> bool { boxes_apart(&mesh_box(self.mb), &body_box(&self.tool)) } }
            fn in_macro(mb: &ModelBody) -> String { format!("{}", mb.plan_mesh().is_some()) }
            fn exact(b: &Body) -> bool { b.exact_bounds().is_ok() }
            #[cfg(test)] mod tests { fn fixture(b: &Body) { b.mesh(&tol); } }
            #[test] fn t(b: &Body) { b.mesh(&tol); }
            mod inner { fn nearest(p: Vec3, t: [Vec3; 3]) -> Vec3 { closest_on_triangle(p, t[0], t[1], t[2]) } }
            "#,
        );
        let keys: Vec<&str> = s.keys().map(String::as_str).collect();
        assert_eq!(keys, ["Sheet::near", "decides", "display_mesh", "in_macro", "inner::nearest"]);
        assert_eq!(s["display_mesh"], [".triangles", ".mesh()"]);
        assert_eq!(s["Sheet::near"], ["mesh_box()", "body_box()"]);
        // In the kernel the argument-less `mesh()` is a T-spline's control net.
        assert!(sites("fn regular(ts: &TSpline) -> bool { ts.mesh().is_regular() }", false).unwrap().is_empty());
        assert!(sites("invalid Rust {{", true).is_err(), "unparseable source fails closed");
    }

    const LISTED: &str = "# comment\na.rs:decides why\nb.rs:near why\n";

    fn sites_of(names: &[&str]) -> BTreeMap<String, Vec<&'static str>> {
        names.iter().map(|n| (n.to_string(), vec![".mesh(t)"])).collect()
    }

    #[test]
    fn listed_sites_pass() {
        assert!(grade(LISTED, &sites_of(&["a.rs:decides", "b.rs:near"])).is_ok());
    }

    #[test]
    fn a_new_site_fails() {
        let e = grade(LISTED, &sites_of(&["a.rs:decides", "b.rs:near", "c.rs:picks"])).unwrap_err();
        assert!(e.contains("NEW: c.rs:picks"), "{e}");
    }

    #[test]
    fn a_fixed_site_still_listed_fails() {
        let e = grade(LISTED, &sites_of(&["a.rs:decides"])).unwrap_err();
        assert!(e.contains("FIXED: b.rs:near"), "{e}");
    }

    #[test]
    fn the_list_is_sorted_with_a_reason_each() {
        assert!(grade("b.rs:near why\na.rs:decides why\n", &sites_of(&["a.rs:decides", "b.rs:near"])).is_err());
        assert!(grade("a.rs:decides\n", &sites_of(&["a.rs:decides"])).is_err());
    }

    #[test]
    fn the_module_tree_leaves_out_test_modules_and_follows_paths() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("solidcraft-mesh-decisions-{}-{unique}", std::process::id()));
        let src = root.join("crates/doc/src");
        std::fs::create_dir_all(src.join("eval")).unwrap();
        std::fs::create_dir_all(root.join("crates/engine/src")).unwrap();
        std::fs::create_dir_all(root.join("crates/kernel/src/tess")).unwrap();
        std::fs::write(src.join("lib.rs"), "mod eval;\n#[cfg(test)]\nmod tests;\n").unwrap();
        std::fs::write(src.join("eval.rs"), "#[path = \"more.rs\"]\nmod more;\nmod nested;\n").unwrap();
        std::fs::write(src.join("more.rs"), "fn skip(mb: &ModelBody) -> bool { mb.mesh().triangles.is_empty() }").unwrap();
        std::fs::write(src.join("eval/nested.rs"), "fn hit(b: &Body) { b.mesh(&t); }").unwrap();
        std::fs::write(src.join("tests.rs"), "fn fixture(b: &Body) { b.mesh(&t); }").unwrap();
        std::fs::write(root.join("crates/engine/src/lib.rs"), "").unwrap();
        std::fs::write(root.join("crates/kernel/src/lib.rs"), "mod tess;").unwrap();
        std::fs::write(root.join("crates/kernel/src/tess/mod.rs"), "fn mesher(b: &Body) { b.mesh_plan(&t); }").unwrap();
        let found = scan_skipping(&root, &[("crates/kernel/src/tess", "the mesher")], &[]).unwrap();
        assert!(scan_skipping(&root, &[("crates/doc/src/gone.rs", "gone")], &[]).is_err(), "a skipped module that is gone fails");
        assert!(scan_skipping(&root, &[], &[("crates/doc/src/more.rs:other", "x")]).is_err(), "a skipped function without a mesh fails");
        let keys: Vec<&str> = found.keys().map(String::as_str).collect();
        assert_eq!(keys, ["crates/doc/src/eval/nested.rs:hit", "crates/doc/src/more.rs:skip"]);
        std::fs::create_dir_all(root.join("xtask")).unwrap();
        std::fs::write(root.join(LIST), "crates/doc/src/eval/nested.rs:hit picks a face\ncrates/doc/src/more.rs:skip skips a body\n").unwrap();
        let skips: &[(&str, &str)] = &[("crates/kernel/src/tess", "the mesher")];
        check(&root, skips, &[]).unwrap();
        std::fs::write(src.join("eval/nested.rs"), "fn hit(b: &Body) { b.exact_bounds(); }").unwrap();
        assert!(check(&root, skips, &[]).unwrap_err().contains("FIXED: crates/doc/src/eval/nested.rs:hit"));
        std::fs::write(src.join("eval/nested.rs"), "fn hit(b: &Body) { b.mesh(&t); }\nfn picks(b: &Body) { b.mesh(&t); }").unwrap();
        assert!(check(&root, skips, &[]).unwrap_err().contains("NEW: crates/doc/src/eval/nested.rs:picks"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_workspace_has_no_new_mesh_decisions() {
        run(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    }
}
