//! Dependency layering rules (plan/architecture.md §3).
//!
//! Adapted from CADCraft's xtask (MIT OR Apache-2.0), see ATTRIBUTION.md.
//!
//! The rule engine works on a small, metadata-independent model so it can be
//! unit-tested; `from_metadata` builds that model from `cargo metadata`.

use serde_json::Value;

/// Where a workspace crate sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Regular layered crate.
    Layer(u8),
    /// Layer 0, and additionally may depend on no workspace crate at all.
    Standalone,
    /// Test tooling: may depend on anything up to L5 (it sits at L6 for rule
    /// purposes); other crates may use it only as a dev-dependency.
    Testkit,
    /// Binaries and build tooling: exempt from the rules.
    Exempt,
    /// Crates other front ends use too, kept extractable while they live here: they may depend
    /// on other shared crates and on external crates (UI toolkits included) only. solidcore and
    /// solidcore-record are git dependencies, not workspace members, so they count as external.
    /// Any crate may depend on a shared crate; `cargo xtask shared` builds each on its own.
    Shared,
}

impl Class {
    fn layer(self) -> Option<u8> {
        match self {
            Class::Layer(l) => Some(l),
            Class::Standalone => Some(0),
            Class::Testkit => Some(6),
            // Shared crates sit outside the layer order; `check` handles them before it asks.
            Class::Exempt | Class::Shared => None,
        }
    }
}

/// The layering table. Names are package names without the `solidcraft-`
/// prefix.
pub const TABLE: &[(&str, Class)] = &[
    ("geom", Class::Layer(0)),
    // 3D mouse descriptors and reports (no workspace dependencies; the apps read the device)
    ("space-mouse", Class::Layer(0)),
    // the derive macros (proc-macro, no workspace dependencies)
    ("derive", Class::Standalone),
    // the 3Dconnexion driver's client API on macOS (no workspace dependencies)
    ("3dx-client", Class::Standalone),
    // the profiler: timed spans for every layer, the kernel's calls to commands (no workspace dependencies)
    ("profile", Class::Standalone),
    ("sketch", Class::Layer(1)),
    // the B-rep kernel, solidcore (the Rust port of NVIDIA's SMLib, ADR 0002); only the kernel uses it
    ("smlib", Class::Layer(1)),
    // STEP (ISO 10303) exchange structure and geometry, no modelling backend
    ("step", Class::Layer(1)),
    ("kernel", Class::Layer(1)),
    ("render", Class::Layer(1)),
    // multibody dynamics (rigid bodies, joints, forces, time integration; geom only)
    ("dynamics", Class::Layer(1)),
    ("doc", Class::Layer(2)),
    ("io", Class::Layer(3)),
    ("engine", Class::Layer(4)),
    ("ui-egui", Class::Layer(5)),
    ("gpu", Class::Layer(5)),
    ("mcp", Class::Layer(5)),
    ("testkit", Class::Testkit),
    // the sRGB transfer rule and the surface-analysis palette, read by the renderer and the theme (#13291)
    ("color", Class::Shared),
    // the widget handle and rect registry UI components publish into and scenarios/the control channel read (#12936)
    ("ui-registry", Class::Shared),
    // message catalogs' machinery and bidirectional text: lookup, formatting, language choice (#13290)
    ("ui-text", Class::Shared),
    // the MCP server machinery: JSON-RPC over stdio, lifecycle, tool schemas and argument checks; apps register their tools (#12947)
    ("mcp-server", Class::Shared),
    // the token contract UI components draw with: light and dark tokens, colour vision, tab accents, theme state (#13304)
    ("theme", Class::Shared),
    // the control channel: the per-launch token and its files, what a request still running does (#12946)
    ("control", Class::Shared),
    // the window's own control requests over egui: ui.screenshot and ui.resize (#12946)
    ("control-egui", Class::Shared),
    // the viewport renderer: camera, picture description and CPU rasterizer, shading models, hatches (#12943)
    ("viewport", Class::Shared),
    // its GPU work on wgpu: pipelines, offscreen pictures, adapter selection, plan evaluation (#12943)
    ("viewport-gpu", Class::Shared),
    // the viewport in an egui window: egui-wgpu paint callbacks and install (#12943)
    ("viewport-egui", Class::Shared),
    // apps and tooling
    ("solidcraft", Class::Exempt),
    ("cli", Class::Exempt),
    ("web", Class::Exempt),
    ("xtask", Class::Exempt),
];

/// Explicit orderings *within* a layer (earlier may be used by later).
/// The viewport draws display plans the GPU crate evaluates and draws with its pipelines; the MCP
/// server's tests draw screenshots with them. The kernel reads and writes STEP through `step`.
pub const INTRA_LAYER_ORDER: &[&[&str]] = &[&["gpu", "ui-egui"], &["gpu", "mcp"], &["smlib", "kernel"], &["step", "kernel"]];

fn intra_layer_allowed(from: &str, to: &str) -> bool {
    let (from, to) = (short_name(from), short_name(to));
    INTRA_LAYER_ORDER.iter().any(|chain| match (chain.iter().position(|n| *n == from), chain.iter().position(|n| *n == to)) {
        (Some(f), Some(t)) => t < f,
        _ => false,
    })
}

/// External crates that constitute a UI toolkit / windowing dependency.
/// Entries ending in `*` are prefixes.
pub const UI_CRATES: &[&str] = &["egui", "eframe", "egui-wgpu", "wgpu", "winit", "egui_kittest", "rfd", "bevy*"];

/// First layer allowed to use UI crates.
pub const UI_MIN_LAYER: u8 = 5;

pub fn short_name(pkg: &str) -> &str {
    pkg.strip_prefix("solidcraft-").unwrap_or(pkg)
}

pub fn classify(pkg: &str) -> Option<Class> {
    let s = short_name(pkg);
    TABLE.iter().find(|(n, _)| *n == s).map(|(_, c)| *c)
}

fn is_ui_crate(name: &str) -> bool {
    UI_CRATES.iter().any(|p| match p.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => name == *p,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepKind {
    Normal,
    Dev,
    Build,
}

#[derive(Debug, Clone)]
pub struct Dep {
    pub name: String,
    pub kind: DepKind,
    /// `true` if the dependency is a workspace member.
    pub workspace: bool,
    /// `true` if the package comes from tangency-labs/solidcore (its git URL, or the vendored
    /// `solidcore/` directory), whatever it is called.
    pub from_solidcore: bool,
}

#[derive(Debug, Clone)]
pub struct Crate {
    pub name: String,
    /// The crate's directory relative to the workspace root (`crates/smlib`).
    pub path: String,
    pub deps: Vec<Dep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    Unregistered {
        krate: String,
    },
    Upward {
        krate: String,
        dep: String,
        from: u8,
        to: u8,
        kind: DepKind,
    },
    StandaloneHasWorkspaceDep {
        krate: String,
        dep: String,
    },
    TestkitAsNormalDep {
        krate: String,
    },
    UiBelowL5 {
        krate: String,
        dep: String,
        layer: u8,
    },
    /// A UI crate reached below L5 through a workspace crate on the way (a Shared one: those may
    /// use UI toolkits), which [`Violation::UiBelowL5`] does not see.
    UiReachedBelowL5 {
        krate: String,
        via: String,
        dep: String,
        layer: u8,
    },
    SharedDependsOnApp {
        krate: String,
        dep: String,
    },
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Violation::Unregistered { krate } => {
                write!(f, "{krate}: unknown workspace crate; register it in xtask/src/layers.rs TABLE (see plan/architecture.md §3)")
            }
            Violation::Upward { krate, dep, from, to, kind } => {
                write!(f, "{krate} (L{from}) -> {dep} (L{to}) [{kind:?}]: may only depend on strictly lower layers")
            }
            Violation::StandaloneHasWorkspaceDep { krate, dep } => {
                write!(f, "{krate}: standalone crate must not depend on workspace crate {dep}")
            }
            Violation::TestkitAsNormalDep { krate } => {
                write!(f, "{krate}: solidcraft-testkit may only be a dev-dependency")
            }
            Violation::UiBelowL5 { krate, dep, layer } => {
                write!(f, "{krate} (L{layer}) depends on UI crate `{dep}`; UI toolkits are only allowed in L5+")
            }
            Violation::UiReachedBelowL5 { krate, via, dep, layer } => {
                write!(f, "{krate} (L{layer}) reaches UI crate `{dep}` through {via}; UI toolkits are only allowed in L5+, directly or not")
            }
            Violation::SharedDependsOnApp { krate, dep } => {
                write!(f, "{krate}: shared crate must depend only on other shared crates, solidcore and external crates, not workspace crate {dep}")
            }
        }
    }
}

/// Check all rules. Returns violations sorted for stable output.
pub fn check(crates: &[Crate]) -> Vec<Violation> {
    check_with(crates, classify)
}

/// `check` against any classification (the tests use one with crates TABLE does not have).
fn check_with(crates: &[Crate], classify: impl Fn(&str) -> Option<Class>) -> Vec<Violation> {
    let mut out = Vec::new();
    for c in crates {
        let Some(class) = classify(&c.name) else {
            out.push(Violation::Unregistered { krate: c.name.clone() });
            continue;
        };
        if class == Class::Exempt {
            continue;
        }
        let layer = class.layer().unwrap_or(0);
        for d in &c.deps {
            // Self dev-dependencies (e.g. to enable features in tests) are fine.
            if d.name == c.name {
                continue;
            }
            if d.workspace {
                if class == Class::Standalone {
                    out.push(Violation::StandaloneHasWorkspaceDep { krate: c.name.clone(), dep: d.name.clone() });
                    continue;
                }
                if class == Class::Shared {
                    // Every kind counts, dev included: its tests must build after extraction,
                    // so not even testkit.
                    match classify(&d.name) {
                        // Unregistered deps are reported on their own entry.
                        None | Some(Class::Shared) => {}
                        Some(_) => out.push(Violation::SharedDependsOnApp { krate: c.name.clone(), dep: d.name.clone() }),
                    }
                    continue;
                }
                match classify(&d.name) {
                    // Unregistered deps are reported on their own entry; any crate may use a shared one.
                    None | Some(Class::Shared) => {}
                    Some(Class::Testkit) if d.kind != DepKind::Dev => {
                        out.push(Violation::TestkitAsNormalDep { krate: c.name.clone() });
                    }
                    Some(Class::Testkit) => {}
                    Some(dc) => {
                        let to = dc.layer().unwrap_or(u8::MAX);
                        if to >= layer && !(to == layer && intra_layer_allowed(&c.name, &d.name)) {
                            out.push(Violation::Upward {
                                krate: c.name.clone(),
                                dep: d.name.clone(),
                                from: layer,
                                to: if to == u8::MAX { 7 } else { to },
                                kind: d.kind,
                            });
                        }
                    }
                }
            } else if class != Class::Shared && layer < UI_MIN_LAYER && is_ui_crate(&d.name) {
                out.push(Violation::UiBelowL5 { krate: c.name.clone(), dep: d.name.clone(), layer });
            }
        }
    }
    out.extend(ui_reached_below_l5(crates, &classify));
    out.sort_by_key(|v| v.to_string());
    out.dedup();
    out
}

/// The dependencies `c` builds with (normal and build, not dev), itself left out.
fn built_with(c: &Crate) -> impl Iterator<Item = &Dep> {
    c.deps.iter().filter(move |d| d.kind != DepKind::Dev && d.name != c.name)
}

/// Every UI crate a crate below L5 reaches through the workspace crates it builds with (normal
/// and build dependencies, any depth): egui or wgpu must not come in through a Shared crate either.
fn ui_reached_below_l5(crates: &[Crate], classify: &impl Fn(&str) -> Option<Class>) -> Vec<Violation> {
    let by_name: std::collections::HashMap<&str, &Crate> = crates.iter().map(|c| (c.name.as_str(), c)).collect();
    let mut out = Vec::new();
    for c in crates {
        let Some(Class::Layer(layer)) = classify(&c.name) else { continue };
        if layer >= UI_MIN_LAYER {
            continue;
        }
        let (mut seen, mut todo) = (std::collections::BTreeSet::new(), vec![c.name.as_str()]);
        while let Some(name) = todo.pop() {
            let Some(k) = by_name.get(name) else { continue };
            for d in built_with(k) {
                if d.workspace {
                    if seen.insert(d.name.as_str()) {
                        todo.push(d.name.as_str());
                    }
                } else if name != c.name && is_ui_crate(&d.name) {
                    out.push(Violation::UiReachedBelowL5 { krate: c.name.clone(), via: name.to_owned(), dep: d.name.clone(), layer });
                }
            }
        }
    }
    out
}

/// Build the model from `cargo metadata --format-version 1 --no-deps`.
pub fn from_metadata(meta: &Value, root: Option<&std::path::Path>) -> Result<Vec<Crate>, String> {
    let pkgs = meta["packages"].as_array().ok_or("metadata: no packages array")?;
    let members: Vec<&str> = pkgs.iter().filter_map(|p| p["name"].as_str()).collect();
    let mut out = Vec::new();
    for p in pkgs {
        let name = p["name"].as_str().ok_or("package without name")?.to_owned();
        let manifest = p["manifest_path"].as_str().unwrap_or_default();
        let path = match (root, std::path::Path::new(manifest).parent()) {
            (Some(r), Some(dir)) => dir.strip_prefix(r).map(|d| d.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| name.clone()),
            _ => name.clone(),
        };
        let mut deps = Vec::new();
        for d in p["dependencies"].as_array().into_iter().flatten() {
            let dname = d["name"].as_str().unwrap_or_default().to_owned();
            let kind = match d["kind"].as_str() {
                Some("dev") => DepKind::Dev,
                Some("build") => DepKind::Build,
                _ => DepKind::Normal,
            };
            let workspace = members.contains(&dname.as_str()) || d["path"].is_string();
            let from_git = d["source"].as_str().is_some_and(|u| u.contains("tangency-labs/solidcore"));
            let vendored = match (root, d["path"].as_str()) {
                (Some(r), Some(p)) => std::path::Path::new(p).strip_prefix(r).is_ok_and(|rel| rel.starts_with("solidcore")),
                _ => false,
            };
            deps.push(Dep { name: dname, kind, workspace, from_solidcore: from_git || vendored });
        }
        out.push(Crate { name, path, deps });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// The allowlist of today's direct uses of solidcore's inner crates.
pub const SEAM_LIST: &str = "xtask/solidcore-seam.txt";

/// The solidcore crates apps may use, one a line (solidcore docs/layers.md §1). After vendoring
/// solidcore owns the list at `solidcore/public-crates.txt`; the rule reads that first and falls
/// back to the xtask copy, so cutover changes no code.
pub const SEAM_PUBLIC: &[&str] = &["solidcore/public-crates.txt", "xtask/solidcore-public.txt"];

/// The crate names in a public-crates file (`#` comments, blank lines skipped).
fn parse_public(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split_whitespace().next().unwrap_or(l).to_owned())
        .collect()
}

/// The public-crates text under `root`: the first of [`SEAM_PUBLIC`] that exists.
pub fn read_public(root: &std::path::Path) -> Result<String, String> {
    for p in SEAM_PUBLIC {
        if let Ok(t) = std::fs::read_to_string(root.join(p)) {
            return Ok(t);
        }
    }
    Err(format!("none of {SEAM_PUBLIC:?} exists"))
}

/// Package names that are solidcore's, whichever way they come in. Anything else from
/// tangency-labs/solidcore is caught by its source (`Dep::from_solidcore`).
fn is_solidcore_name(name: &str) -> bool {
    name == "solidcore" || name.starts_with("solidcore-") || name == "ssi" || name.starts_with("step-") || name == "nurbs-grid-mesh"
}

/// The seam rule: SolidCraft crosses into solidcore only through the public crates ([`SEAM_PUBLIC`]). Every other
/// solidcore crate, in any dependency kind, must be named in `allow` (lines
/// `crate-path -> solidcore-crate  reason`; `#` comments). The list only shrinks: a listed use
/// that is gone is an error too. Returns the messages, sorted.
pub fn seam_check(crates: &[Crate], allow: &str, public: &str) -> Vec<String> {
    let public = parse_public(public);
    let mut errors = Vec::new();
    let mut listed: Vec<(String, String)> = Vec::new();
    for (n, line) in allow.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.splitn(2, " -> ");
        let (Some(from), Some(rest)) = (it.next(), it.next()) else {
            errors.push(format!("{SEAM_LIST}:{}: expected `crate-path -> solidcore-crate  reason`", n + 1));
            continue;
        };
        let mut parts = rest.split_whitespace();
        let (Some(to), Some(_reason)) = (parts.next(), parts.next()) else {
            errors.push(format!(
                "{SEAM_LIST}:{}: `{line}` needs a one-line reason after the crate name, naming the solidcore issue or plan step that removes it",
                n + 1
            ));
            continue;
        };
        listed.push((from.trim().to_owned(), to.to_owned()));
    }
    let mut used: Vec<(String, String)> = Vec::new();
    for c in crates {
        // After vendoring, solidcore's own crates are workspace members; their rules are its own.
        if is_solidcore_name(&c.name) {
            continue;
        }
        for d in &c.deps {
            if !(is_solidcore_name(&d.name) || d.from_solidcore) || public.contains(&d.name) {
                continue;
            }
            let key = (c.path.clone(), d.name.clone());
            if used.contains(&key) {
                continue;
            }
            used.push(key.clone());
            if !listed.contains(&key) {
                errors.push(format!(
                    "{} ({}) uses solidcore crate `{}` [{:?}]: call solidcore-ops or solidcore-tess instead (solidcore docs/layers.md; `Order` comes via solidcore_ops::Order, not solidcore-convert)",
                    c.path, c.name, d.name, d.kind
                ));
            }
        }
    }
    for (from, to) in &listed {
        if !used.contains(&(from.clone(), to.clone())) {
            errors.push(format!("{SEAM_LIST}: `{from} -> {to}` is no longer used; remove it from the list"));
        }
    }
    errors.sort();
    errors.dedup();
    errors
}

/// One rule, one message: solidcore must build from its own tree alone.
const STANDALONE_RULE: &str = "solidcore must build on its own; see solidcore/docs/layers.md";

/// Collapse `.` and `..` in a relative path without touching the disk. `None` when it climbs above its start.
fn normalize_rel(path: &str) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for part in path.replace('\\', "/").split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            p => out.push(p.to_owned()),
        }
    }
    Some(out)
}

/// The dependency entries of a manifest: (table description, key, spec) for every dependency table (workspace,
/// target-specific, dev, build and the old spellings) and for `[patch]`/`[replace]`.
fn manifest_dep_entries(doc: &toml::Table) -> Vec<(String, String, toml::Value)> {
    const KINDS: [&str; 5] = ["dependencies", "dev-dependencies", "build-dependencies", "dev_dependencies", "build_dependencies"];
    let mut out = Vec::new();
    let mut add = |what: String, t: Option<&toml::Value>| {
        for (k, v) in t.and_then(toml::Value::as_table).into_iter().flatten() {
            out.push((what.clone(), k.clone(), v.clone()));
        }
    };
    for kind in KINDS {
        add(format!("[{kind}]"), doc.get(kind));
        add(format!("[workspace.{kind}]"), doc.get("workspace").and_then(|w| w.get(kind)));
    }
    for (cfg, t) in doc.get("target").and_then(toml::Value::as_table).into_iter().flatten() {
        for kind in KINDS {
            add(format!("[target.{cfg}.{kind}]"), t.get(kind));
        }
    }
    for (source, t) in doc.get("patch").and_then(toml::Value::as_table).into_iter().flatten() {
        add(format!("[patch.{source}]"), Some(t));
    }
    add("[replace]".into(), doc.get("replace"));
    out
}

/// The standalone rule: every manifest under `solidcore/` (`manifests`: repo-relative path, text) builds from
/// solidcore's own tree. No dependency, `[workspace.dependencies]` entry or `[patch]` may point with `path` outside
/// `solidcore/`, and none may be a SolidCraft crate (`solidcraft-*`, also behind `package = ...`).
pub fn standalone_check(manifests: &[(String, String)]) -> Vec<String> {
    let mut errors = Vec::new();
    for (file, text) in manifests {
        let doc = match text.parse::<toml::Table>() {
            Ok(d) => d,
            Err(e) => {
                errors.push(format!("{file}: cannot parse the manifest ({e}); {STANDALONE_RULE}"));
                continue;
            }
        };
        let dir: Vec<String> = file.rsplit_once('/').map(|(d, _)| d).and_then(normalize_rel).unwrap_or_default();
        for (what, key, spec) in manifest_dep_entries(&doc) {
            let real = spec.get("package").and_then(toml::Value::as_str).unwrap_or(&key);
            if real.starts_with("solidcraft-") || key.starts_with("solidcraft-") {
                errors.push(format!("{file}: {what} dependency `{key}` is a SolidCraft crate; {STANDALONE_RULE}"));
            }
            if let Some(path) = spec.get("path").and_then(toml::Value::as_str) {
                let joined = normalize_rel(&format!("{}/{path}", dir.join("/")));
                if !joined.is_some_and(|p| p.first().map(String::as_str) == Some("solidcore")) {
                    errors.push(format!("{file}: {what} dependency `{key}` has path `{path}`, which leaves solidcore/; {STANDALONE_RULE}"));
                }
            }
        }
    }
    errors.sort();
    errors.dedup();
    errors
}

/// Every `Cargo.toml` under `root/solidcore`, as (repo-relative path, text). Skips build output and the
/// upstream checkout.
pub fn read_solidcore_manifests(root: &std::path::Path) -> Result<Vec<(String, String)>, String> {
    fn walk(root: &std::path::Path, rel: &str, out: &mut Vec<(String, String)>) -> Result<(), String> {
        let dir = root.join(rel);
        let Ok(rd) = std::fs::read_dir(&dir) else { return Ok(()) };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let child = format!("{rel}/{name}");
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if !matches!(name.as_str(), "target" | "upstream" | ".git" | "node_modules") {
                    walk(root, &child, out)?;
                }
            } else if name == "Cargo.toml" {
                let text = std::fs::read_to_string(e.path()).map_err(|e| format!("{child}: {e}"))?;
                out.push((child, text));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, "solidcore", &mut out)?;
    Ok(out)
}

pub fn describe(class: Option<Class>) -> String {
    match class {
        Some(Class::Layer(l)) => format!("L{l}"),
        Some(Class::Standalone) => "L0 standalone".into(),
        Some(Class::Testkit) => "testkit".into(),
        Some(Class::Exempt) => "exempt".into(),
        Some(Class::Shared) => "shared".into(),
        None => "UNREGISTERED".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str, deps: &[(&str, DepKind, bool)]) -> Crate {
        Crate {
            name: name.into(),
            path: format!("crates/{}", short_name(name)),
            deps: deps.iter().map(|(n, k, w)| Dep { name: (*n).into(), kind: *k, workspace: *w, from_solidcore: false }).collect(),
        }
    }
    use DepKind::*;

    #[test]
    fn clean_downward_graph_passes() {
        let g = [
            c("solidcraft-geom", &[("kurbo", Normal, false)]),
            c("solidcraft-doc", &[("solidcraft-geom", Normal, true)]),
            c("solidcraft-engine", &[("solidcraft-doc", Normal, true), ("solidcraft-testkit", Dev, true)]),
            c("solidcraft-ui-egui", &[("solidcraft-engine", Normal, true), ("egui", Normal, false)]),
            c("solidcraft-cli", &[("solidcraft-ui-egui", Normal, true)]),
        ];
        assert!(check(&g).is_empty(), "{:?}", check(&g));
    }

    #[test]
    fn upward_dependency_flagged() {
        let v = check(&[c("solidcraft-doc", &[("solidcraft-engine", Normal, true)])]);
        assert!(matches!(v[..], [Violation::Upward { from: 2, to: 4, .. }]));
    }

    #[test]
    fn sideways_dependency_flagged() {
        let v = check(&[c("solidcraft-kernel", &[("solidcraft-sketch", Normal, true)])]);
        assert!(matches!(v[..], [Violation::Upward { from: 1, to: 1, .. }]));
    }

    #[test]
    fn self_dev_dependency_ignored() {
        assert!(check(&[c("solidcraft-doc", &[("solidcraft-doc", Dev, true)])]).is_empty());
    }

    #[test]
    fn upward_dev_dependency_flagged() {
        let v = check(&[c("solidcraft-geom", &[("solidcraft-doc", Dev, true)])]);
        assert!(matches!(v[..], [Violation::Upward { kind: Dev, .. }]));
    }

    /// egui or wgpu that a Shared crate brings in is caught below L5 too, at any depth, naming the
    /// crate it comes through; dev-dependencies and L5 crates are not.
    #[test]
    fn ui_crates_reached_through_a_shared_crate_below_l5_flagged() {
        let classes = |pkg: &str| if short_name(pkg).starts_with("view") { Some(Class::Shared) } else { classify(pkg) };
        let g = [
            c("solidcraft-engine", &[("solidcraft-doc", Normal, true), ("solidcraft-viewtest", Dev, true)]),
            c("solidcraft-doc", &[("solidcraft-viewbase", Normal, true)]),
            c("solidcraft-viewbase", &[("solidcraft-viewgpu", Normal, true), ("serde", Normal, false)]),
            c("solidcraft-viewgpu", &[("wgpu", Normal, false), ("egui", Normal, false)]),
            c("solidcraft-viewtest", &[("egui-wgpu", Normal, false)]),
            c("solidcraft-ui-egui", &[("solidcraft-viewgpu", Normal, true)]),
        ];
        let v = check_with(&g, classes);
        let reached: Vec<(&str, &str, &str)> = v
            .iter()
            .filter_map(|v| match v {
                Violation::UiReachedBelowL5 { krate, via, dep, .. } => Some((krate.as_str(), via.as_str(), dep.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(
            reached,
            [
                ("solidcraft-doc", "solidcraft-viewgpu", "egui"),
                ("solidcraft-doc", "solidcraft-viewgpu", "wgpu"),
                ("solidcraft-engine", "solidcraft-viewgpu", "egui"),
                ("solidcraft-engine", "solidcraft-viewgpu", "wgpu"),
            ]
        );
        assert!(v[0].to_string().contains("reaches UI crate `egui` through solidcraft-viewgpu"), "{}", v[0]);
        let clean = [c("solidcraft-engine", &[("solidcraft-viewbase", Normal, true)]), c("solidcraft-viewbase", &[("serde", Normal, false)])];
        assert!(check_with(&clean, classes).is_empty());
    }

    #[test]
    fn ui_crates_below_l6_flagged() {
        for dep in ["egui", "eframe", "winit", "egui_kittest", "rfd", "bevy_ecs", "bevy"] {
            let v = check(&[c("solidcraft-engine", &[(dep, Normal, false)])]);
            assert!(matches!(v[..], [Violation::UiBelowL5 { layer: 4, .. }]), "{dep}");
        }
        assert!(check(&[c("solidcraft-engine", &[("egui_extras_not", Normal, false)])]).is_empty());
        assert!(check(&[c("solidcraft-ui-egui", &[("winit", Normal, false)])]).is_empty());
    }

    #[test]
    fn unregistered_crate_is_error() {
        let v = check(&[c("solidcraft-mystery", &[])]);
        assert!(matches!(&v[..], [Violation::Unregistered { krate }] if krate == "solidcraft-mystery"));
        assert!(v[0].to_string().contains("register"));
    }

    #[test]
    fn testkit_only_as_dev_dependency() {
        let v = check(&[c("solidcraft-doc", &[("solidcraft-testkit", Normal, true)])]);
        assert!(matches!(v[..], [Violation::TestkitAsNormalDep { .. }]));
        assert!(check(&[c("solidcraft-doc", &[("solidcraft-testkit", Dev, true)])]).is_empty());
        assert!(check(&[c("solidcraft-testkit", &[("solidcraft-engine", Normal, true)])]).is_empty());
    }

    #[test]
    fn apps_and_xtask_exempt() {
        for app in ["solidcraft", "solidcraft-cli", "xtask"] {
            assert!(check(&[c(app, &[("egui", Normal, false), ("solidcraft-ui-egui", Normal, true)])]).is_empty());
        }
    }

    #[test]
    fn shared_crate_must_not_depend_on_layered_crates() {
        let v = check(&[c("solidcraft-ui-registry", &[("solidcraft-engine", Normal, true)])]);
        assert!(matches!(&v[..], [Violation::SharedDependsOnApp { krate, dep }] if krate == "solidcraft-ui-registry" && dep == "solidcraft-engine"));
        assert!(v[0].to_string().contains("shared crate must depend only on"));
        let v = check(&[c("solidcraft-ui-registry", &[("solidcraft-geom", Build, true)])]);
        assert!(matches!(v[..], [Violation::SharedDependsOnApp { .. }]));
    }

    #[test]
    fn any_crate_may_use_a_shared_crate() {
        for user in ["solidcraft-geom", "solidcraft-engine", "solidcraft-ui-egui", "solidcraft-testkit"] {
            assert!(check(&[c(user, &[("solidcraft-ui-registry", Normal, true)])]).is_empty(), "{user}");
        }
    }

    #[test]
    fn shared_crate_may_use_shared_and_external_crates() {
        let classes = |pkg: &str| if short_name(pkg) == "ui-text" { Some(Class::Shared) } else { classify(pkg) };
        for kind in [Normal, Dev, Build] {
            let g = [c("solidcraft-ui-registry", &[("solidcraft-ui-text", kind, true), ("serde", Normal, false), ("solidcore", Normal, false)])];
            assert!(check_with(&g, classes).is_empty(), "{kind:?}: {:?}", check_with(&g, classes));
        }
    }

    #[test]
    fn shared_crate_may_use_ui_crates() {
        for dep in ["egui", "eframe", "winit", "wgpu"] {
            assert!(check(&[c("solidcraft-ui-registry", &[(dep, Normal, false)])]).is_empty(), "{dep}");
        }
    }

    #[test]
    fn shared_crate_must_not_use_testkit_even_in_tests() {
        let v = check(&[c("solidcraft-ui-registry", &[("solidcraft-testkit", Dev, true)])]);
        assert!(matches!(v[..], [Violation::SharedDependsOnApp { .. }]));
    }

    const PUB: &str = "# public\nsolidcore-ops\nsolidcore-tess\nsolidcore-record\n";

    fn seam_crate(dep: &str, kind: DepKind, from_solidcore: bool) -> Crate {
        Crate {
            name: "solidcraft-smlib".into(),
            path: "crates/smlib".into(),
            deps: vec![Dep { name: dep.into(), kind, workspace: false, from_solidcore }],
        }
    }

    #[test]
    fn seam_direct_kernel_use_fails() {
        for kind in [Normal, Dev, Build] {
            let e = seam_check(&[seam_crate("solidcore", kind, true)], "", PUB);
            assert_eq!(e.len(), 1, "{kind:?}");
            assert!(e[0].contains("crates/smlib") && e[0].contains("`solidcore`"), "{}", e[0]);
            assert!(e[0].contains("call solidcore-ops or solidcore-tess instead (solidcore docs/layers.md") && e[0].contains("solidcore_ops::Order"));
        }
        for dep in ["solidcore-convert", "solidcore-ssi", "ssi", "step-import", "step-reader", "nurbs-grid-mesh"] {
            assert_eq!(seam_check(&[seam_crate(dep, Normal, false)], "", PUB).len(), 1, "{dep}");
        }
        // a solidcore crate with a name this list does not know, found by where it comes from
        assert_eq!(seam_check(&[seam_crate("brand-new-kernel-crate", Normal, true)], "", PUB).len(), 1);
    }

    #[test]
    fn seam_public_crates_pass() {
        for dep in ["solidcore-ops", "solidcore-tess", "solidcore-record"] {
            assert!(seam_check(&[seam_crate(dep, Normal, true)], "", PUB).is_empty(), "{dep}");
        }
    }

    #[test]
    fn seam_allowlisted_use_passes() {
        let list = "# today\ncrates/smlib -> solidcore  the kernel wrapper, until the switch\n";
        assert!(seam_check(&[seam_crate("solidcore", Normal, true)], list, PUB).is_empty());
        // listing one crate does not cover another solidcore crate
        assert_eq!(seam_check(&[seam_crate("solidcore-convert", Normal, true)], list, PUB).len(), 2);
    }

    #[test]
    fn seam_removed_use_must_leave_the_list() {
        let list = "crates/smlib -> solidcore  the kernel wrapper\n";
        let e = seam_check(&[seam_crate("solidcore-ops", Normal, true)], list, PUB);
        assert_eq!(e.len(), 1);
        assert!(e[0].contains("remove it from the list"), "{}", e[0]);
    }

    #[test]
    fn seam_public_list_is_data() {
        // a crate added to the public list passes; one left off it fails
        let c = [seam_crate("solidcore-newthing", Normal, true)];
        assert_eq!(seam_check(&c, "", PUB).len(), 1);
        assert!(seam_check(&c, "", "solidcore-newthing  # ops gained it\n").is_empty());
    }

    #[test]
    fn seam_public_list_prefers_solidcore_copy() {
        let dir = std::env::temp_dir().join(format!("seam-public-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("solidcore")).unwrap();
        std::fs::create_dir_all(dir.join("xtask")).unwrap();
        std::fs::write(dir.join("xtask/solidcore-public.txt"), "old\n").unwrap();
        assert_eq!(read_public(&dir).unwrap(), "old\n");
        std::fs::write(dir.join("solidcore/public-crates.txt"), "new\n").unwrap();
        assert_eq!(read_public(&dir).unwrap(), "new\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn seam_list_lines_need_a_reason() {
        let e = seam_check(&[seam_crate("solidcore", Normal, true)], "crates/smlib -> solidcore\n", PUB);
        assert!(e.iter().any(|m| m.contains("needs a one-line reason")), "{e:?}");
    }

    #[test]
    fn seam_matches_by_package_name_not_source() {
        let meta: Value = serde_json::from_str(
            r#"{"packages":[{"name":"solidcraft-smlib","manifest_path":"/w/crates/smlib/Cargo.toml","dependencies":[
                {"name":"solidcore","kind":null,"path":"/w/solidcore/crates/solidcore"},
                {"name":"solidcore-ops","kind":null,"source":"git+https://github.com/tangency-labs/solidcore?rev=abc"}]}]}"#,
        )
        .unwrap();
        let g = from_metadata(&meta, Some(std::path::Path::new("/w"))).unwrap();
        assert_eq!(g[0].path, "crates/smlib");
        let e = seam_check(&g, "", PUB);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("`solidcore`"));
    }

    #[test]
    fn metadata_parsing() {
        let meta: Value = serde_json::from_str(
            r#"{"packages":[
                {"name":"solidcraft-doc","dependencies":[
                    {"name":"solidcraft-geom","kind":null,"path":"/x/crates/geom"},
                    {"name":"serde","kind":null},
                    {"name":"proptest","kind":"dev"}]},
                {"name":"solidcraft-geom","dependencies":[]}
            ]}"#,
        )
        .unwrap();
        let g = from_metadata(&meta, None).unwrap();
        assert_eq!(g.len(), 2);
        let doc = g.iter().find(|c| c.name == "solidcraft-doc").unwrap();
        assert!(doc.deps[0].workspace && !doc.deps[1].workspace);
        assert_eq!(doc.deps[2].kind, Dev);
        assert!(check(&g).is_empty());
    }

    fn sc(name: &str, body: &str) -> Vec<String> {
        standalone_check(&[(name.into(), body.into())])
    }

    #[test]
    fn standalone_clean_manifest_passes() {
        let m = "[package]\nname = \"solidcore-ops\"\n[dependencies]\nthiserror = \"2\"\nsolidcore = { path = \"../solidcore\" }\n[dev-dependencies]\ngoldens = { path = \"../goldens\" }\n";
        assert!(sc("solidcore/crates/solidcore-ops/Cargo.toml", m).is_empty());
        let root = "[workspace]\nmembers = [\"crates/a\"]\n[workspace.dependencies]\nlocal = { path = \"crates/a\" }\n";
        assert!(sc("solidcore/Cargo.toml", root).is_empty());
    }

    #[test]
    fn standalone_path_outside_solidcore_fails() {
        let m = "[dependencies]\ngeom = { path = \"../../../crates/geom\" }\n";
        let e = sc("solidcore/crates/ops/Cargo.toml", m);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("solidcore/crates/ops/Cargo.toml") && e[0].contains("`geom`") && e[0].contains("solidcore must build on its own"));
        // From the workspace root, one step up is already outside.
        assert_eq!(sc("solidcore/Cargo.toml", "[workspace.dependencies]\nx = { path = \"../crates/x\" }\n").len(), 1);
    }

    #[test]
    fn standalone_solidcraft_crate_fails() {
        let e = sc("solidcore/crates/ops/Cargo.toml", "[dependencies]\nsolidcraft-geom = \"0.1\"\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("solidcraft-geom"));
        let renamed = "[dependencies]\ng = { package = \"solidcraft-geom\", version = \"0.1\" }\n";
        assert_eq!(sc("solidcore/crates/ops/Cargo.toml", renamed).len(), 1);
    }

    #[test]
    fn standalone_dev_build_target_and_patch_leaks_fail() {
        let dev = "[dev-dependencies]\ngeom = { path = \"../../../crates/geom\" }\n";
        assert_eq!(sc("solidcore/crates/ops/Cargo.toml", dev).len(), 1);
        let build = "[build-dependencies]\nsolidcraft-testkit = { path = \"../../../crates/testkit\" }\n";
        assert!(!sc("solidcore/crates/ops/Cargo.toml", build).is_empty());
        let target = "[target.'cfg(unix)'.dependencies]\ngeom = { path = \"../../../crates/geom\" }\n";
        assert_eq!(sc("solidcore/crates/ops/Cargo.toml", target).len(), 1);
        let patch = "[patch.crates-io]\nfoo = { path = \"../crates/foo\" }\n";
        assert_eq!(sc("solidcore/Cargo.toml", patch).len(), 1);
    }
}
