//! Built-in Phase 1 [`SearchSource`] implementations (RFC 0019 §3.1):
//! commands, open design entities (features, bodies, sketches, components, parameters, joints,
//! named views), standard parts library, built-in samples and installed plugins.

use std::collections::BTreeMap;

use serde_json::json;
use solidcraft_doc::FeatureKind;
use solidcraft_doc::joints::JointKind;

use super::{ItemVisibility, SearchContext, SearchField, SearchGroup, SearchItem, SearchRegistry, SearchScope, SearchSource, SearchSurfaces};

/// Maximum number of items emitted by one design source to bound work on huge designs.
const MAX_SOURCE_ITEMS: usize = 10_000;

/// Register every built-in Phase 1 search source in deterministic order.
pub fn register_builtins(registry: &mut SearchRegistry) {
    registry
        .register(Box::new(CommandsSource))
        .register(Box::new(FeaturesSource))
        .register(Box::new(BodiesSource))
        .register(Box::new(SketchesSource))
        .register(Box::new(ComponentsSource))
        .register(Box::new(ParametersSource))
        .register(Box::new(JointsSource))
        .register(Box::new(NamedViewsSource))
        .register(Box::new(PartsSource))
        .register(Box::new(SamplesSource))
        .register(Box::new(InstalledPluginsSource));
}

struct CommandsSource;

impl SearchSource for CommandsSource {
    fn id(&self) -> &'static str {
        "commands"
    }
    fn surfaces(&self) -> SearchSurfaces {
        SearchSurfaces::GLOBAL.union(SearchSurfaces::COMMANDS)
    }
    fn scopes(&self) -> &'static [SearchScope] {
        &[SearchScope::Commands]
    }
    fn group(&self) -> SearchGroup {
        SearchGroup::Commands
    }
    fn search(&self, ctx: &SearchContext<'_>) -> Result<Vec<SearchItem>, String> {
        let mut items = Vec::new();
        for spec in crate::command_specs() {
            let info = spec.info(ctx.session);
            let mut fields = vec![SearchField::title(info.label), SearchField::identifier(info.id)];
            let kw = super::command_keywords(info.id);
            if !kw.is_empty() {
                fields.push(SearchField::keywords(kw));
            }
            if !info.tab.is_empty() {
                fields.push(SearchField::secondary(info.tab));
            }
            if !info.panel.is_empty() {
                fields.push(SearchField::secondary(info.panel));
            }
            let subtitle = if !info.tab.is_empty() && !info.panel.is_empty() {
                Some(format!("{} › {}", info.tab, info.panel))
            } else {
                Some(info.id.to_string())
            };
            items.push(SearchItem {
                id: info.id.to_string(),
                kind: "command".into(),
                group: SearchGroup::Commands,
                title: info.label.to_string(),
                subtitle,
                detail: (!info.params.is_empty()).then(|| truncate_chars(info.params, 160)),
                icon: (!info.icon.is_empty()).then(|| info.icon.to_string()),
                installed: true,
                enabled: info.enabled,
                disabled_reason: info.disabled_reason,
                action: Some(info.id.to_string()),
                metadata: Some(json!({
                    "tab": info.tab,
                    "panel": info.panel,
                    "shortcut": info.shortcut,
                    "planned": info.planned,
                })),
                visibility: ItemVisibility::Public,
                fields,
            });
        }

        // Installed plugin runtime entries are callable commands too (`<plugin>.<entry>`).
        for (package_id, path) in &ctx.session.plugin_state.packages {
            if let Ok(package) = crate::plugin::read_package(path)
                && let Some(runtime) = package.manifest.runtime
            {
                let disabled = ctx.session.disabled_plugins.contains(package_id);
                for entry in runtime.entries {
                    let full_id = format!("{package_id}.{}", entry.name);
                    let mut fields =
                        vec![SearchField::title(&entry.name), SearchField::identifier(&full_id), SearchField::secondary(&package.manifest.name)];
                    if !package.manifest.description.is_empty() {
                        fields.push(SearchField::keywords(&package.manifest.description));
                    }
                    items.push(SearchItem {
                        id: full_id.clone(),
                        kind: "command".into(),
                        group: SearchGroup::Commands,
                        title: entry.name.clone(),
                        subtitle: Some(package.manifest.name.clone()),
                        detail: (!package.manifest.description.is_empty()).then(|| truncate_chars(&package.manifest.description, 160)),
                        icon: Some("play".into()),
                        installed: true,
                        enabled: !disabled,
                        disabled_reason: disabled.then(|| "plugin is disabled".into()),
                        action: Some(full_id),
                        metadata: Some(json!({"plugin": package_id, "entry": entry.name})),
                        visibility: ItemVisibility::Public,
                        fields,
                    });
                }
            }
        }
        Ok(items)
    }
}

struct FeaturesSource;

impl SearchSource for FeaturesSource {
    fn id(&self) -> &'static str {
        "design.features"
    }
    fn surfaces(&self) -> SearchSurfaces {
        SearchSurfaces::GLOBAL.union(SearchSurfaces::DESIGN).union(SearchSurfaces::BROWSER)
    }
    fn scopes(&self) -> &'static [SearchScope] {
        &[SearchScope::Design, SearchScope::Features]
    }
    fn group(&self) -> SearchGroup {
        SearchGroup::Design
    }
    fn search(&self, ctx: &SearchContext<'_>) -> Result<Vec<SearchItem>, String> {
        let doc = &ctx.session.doc;
        let marker = doc.marker.unwrap_or(doc.features.len());
        let param_rows = doc.param_rows();
        let params: BTreeMap<&str, &solidcraft_doc::ParamRow> = param_rows.iter().take(MAX_SOURCE_ITEMS).map(|p| (p.name.as_str(), p)).collect();
        let mut items = Vec::new();
        for (idx, feature) in doc.features.iter().take(MAX_SOURCE_ITEMS).enumerate() {
            let type_name = feature.kind.type_name();
            let rolled_back = idx >= marker;
            let param_summary = feature.param_names.iter().take(8).find_map(|name| params.get(name.as_str())).map(|p| {
                p.display_value
                    .or(p.value)
                    .filter(|v| v.is_finite())
                    .map_or_else(|| truncate_chars(&p.expression, 64), |v| format!("{v} {}", p.unit).trim().to_string())
            });
            let mut detail_parts = vec![type_name.to_string()];
            if let Some(val) = &param_summary
                && !val.is_empty()
            {
                detail_parts.push(val.clone());
            }
            if feature.suppressed {
                detail_parts.push("suppressed".into());
            }
            if rolled_back {
                detail_parts.push("rolled back".into());
            }
            let fields = vec![SearchField::title(&feature.name), SearchField::identifier(feature.id.to_string()), SearchField::secondary(type_name)];
            items.push(SearchItem {
                id: feature.id.to_string(),
                kind: "feature".into(),
                group: SearchGroup::Design,
                title: feature.name.clone(),
                subtitle: Some(type_name.to_string()),
                detail: Some(detail_parts.join(" · ")),
                icon: Some("timeline".into()),
                installed: true,
                enabled: !feature.suppressed && !rolled_back,
                disabled_reason: if feature.suppressed {
                    Some("feature is suppressed".into())
                } else if rolled_back {
                    Some("feature is after the timeline marker".into())
                } else {
                    None
                },
                action: Some("timeline.edit".into()),
                metadata: Some(json!({
                    "feature_id": feature.id,
                    "type": type_name,
                    "component": feature.component,
                    "suppressed": feature.suppressed,
                    "rolled_back": rolled_back,
                })),
                visibility: ItemVisibility::Public,
                fields,
            });
        }
        Ok(items)
    }
}

struct BodiesSource;

impl SearchSource for BodiesSource {
    fn id(&self) -> &'static str {
        "design.bodies"
    }
    fn surfaces(&self) -> SearchSurfaces {
        SearchSurfaces::GLOBAL.union(SearchSurfaces::DESIGN).union(SearchSurfaces::BROWSER)
    }
    fn scopes(&self) -> &'static [SearchScope] {
        &[SearchScope::Design, SearchScope::Bodies]
    }
    fn group(&self) -> SearchGroup {
        SearchGroup::Design
    }
    fn search(&self, ctx: &SearchContext<'_>) -> Result<Vec<SearchItem>, String> {
        let s = ctx.session;
        let state = s.model.state();
        let mut items = Vec::new();
        for body in state.bodies.iter().take(MAX_SOURCE_ITEMS) {
            if s.body_removed_later(&body.name) {
                continue;
            }
            let visible = s.body_visible(&body.name);
            let component = s.doc.model_body_component(body);
            let fields = vec![SearchField::title(&body.name), SearchField::identifier(&body.name), SearchField::secondary("body")];
            items.push(SearchItem {
                id: body.name.clone(),
                kind: "body".into(),
                group: SearchGroup::Design,
                title: body.name.clone(),
                subtitle: Some("Body".into()),
                detail: Some(if visible { "Visible".into() } else { "Hidden".into() }),
                icon: Some("body".into()),
                installed: true,
                enabled: true,
                disabled_reason: None,
                action: Some("select.set".into()),
                metadata: Some(json!({
                    "name": body.name,
                    "component": component,
                    "visible": visible,
                })),
                visibility: ItemVisibility::Public,
                fields,
            });
        }
        Ok(items)
    }
}

struct SketchesSource;

impl SearchSource for SketchesSource {
    fn id(&self) -> &'static str {
        "design.sketches"
    }
    fn surfaces(&self) -> SearchSurfaces {
        SearchSurfaces::GLOBAL.union(SearchSurfaces::DESIGN).union(SearchSurfaces::BROWSER)
    }
    fn scopes(&self) -> &'static [SearchScope] {
        &[SearchScope::Design, SearchScope::Sketches]
    }
    fn group(&self) -> SearchGroup {
        SearchGroup::Design
    }
    fn search(&self, ctx: &SearchContext<'_>) -> Result<Vec<SearchItem>, String> {
        let doc = &ctx.session.doc;
        let before_marker = doc.marker.unwrap_or(doc.features.len());
        let mut items = Vec::new();
        for feature in doc.features.iter().take(before_marker).take(MAX_SOURCE_ITEMS) {
            if !matches!(feature.kind, FeatureKind::Sketch { .. }) {
                continue;
            }
            let fields = vec![SearchField::title(&feature.name), SearchField::identifier(feature.id.to_string()), SearchField::secondary("sketch")];
            items.push(SearchItem {
                id: feature.id.to_string(),
                kind: "sketch".into(),
                group: SearchGroup::Design,
                title: feature.name.clone(),
                subtitle: Some("Sketch".into()),
                detail: Some(format!("Sketch #{}", feature.id)),
                icon: Some("sketch".into()),
                installed: true,
                enabled: !feature.suppressed,
                disabled_reason: feature.suppressed.then(|| "sketch is suppressed".into()),
                action: Some("sketch.edit".into()),
                metadata: Some(json!({
                    "sketch_id": feature.id,
                    "component": feature.component,
                    "suppressed": feature.suppressed,
                })),
                visibility: ItemVisibility::Public,
                fields,
            });
        }
        Ok(items)
    }
}

struct ComponentsSource;

impl SearchSource for ComponentsSource {
    fn id(&self) -> &'static str {
        "design.components"
    }
    fn surfaces(&self) -> SearchSurfaces {
        SearchSurfaces::GLOBAL.union(SearchSurfaces::DESIGN).union(SearchSurfaces::BROWSER)
    }
    fn scopes(&self) -> &'static [SearchScope] {
        &[SearchScope::Design, SearchScope::Components]
    }
    fn group(&self) -> SearchGroup {
        SearchGroup::Design
    }
    fn search(&self, ctx: &SearchContext<'_>) -> Result<Vec<SearchItem>, String> {
        let doc = &ctx.session.doc;
        let mut items = Vec::new();
        for comp in doc.components.iter().take(MAX_SOURCE_ITEMS) {
            let fields = vec![SearchField::title(&comp.name), SearchField::identifier(comp.id.to_string()), SearchField::secondary("component")];
            items.push(SearchItem {
                id: format!("component:{}", comp.id),
                kind: "component".into(),
                group: SearchGroup::Design,
                title: comp.name.clone(),
                subtitle: Some("Component".into()),
                detail: Some(format!("Component #{}", comp.id)),
                icon: Some("component".into()),
                installed: true,
                enabled: true,
                disabled_reason: None,
                action: Some("component.activate".into()),
                metadata: Some(json!({
                    "component_id": comp.id,
                    "parent": comp.parent,
                })),
                visibility: ItemVisibility::Public,
                fields,
            });
        }
        for occ in doc.occurrences.iter().take(MAX_SOURCE_ITEMS) {
            let fields = vec![
                SearchField::title(&occ.name),
                SearchField::identifier(occ.id.to_string()),
                SearchField::secondary("occurrence placement component"),
            ];
            items.push(SearchItem {
                id: format!("occurrence:{}", occ.id),
                kind: "occurrence".into(),
                group: SearchGroup::Design,
                title: occ.name.clone(),
                subtitle: Some("Component placement".into()),
                detail: Some(if occ.grounded { "Grounded".into() } else { format!("Occurrence #{}", occ.id) }),
                icon: Some("component".into()),
                installed: true,
                enabled: true,
                disabled_reason: None,
                action: Some("select.set".into()),
                metadata: Some(json!({
                    "occurrence_id": occ.id,
                    "component_id": occ.component,
                    "parent": occ.parent,
                    "grounded": occ.grounded,
                })),
                visibility: ItemVisibility::Public,
                fields,
            });
        }
        Ok(items)
    }
}

struct ParametersSource;

impl SearchSource for ParametersSource {
    fn id(&self) -> &'static str {
        "design.parameters"
    }
    fn surfaces(&self) -> SearchSurfaces {
        SearchSurfaces::GLOBAL.union(SearchSurfaces::DESIGN)
    }
    fn scopes(&self) -> &'static [SearchScope] {
        &[SearchScope::Design, SearchScope::Parameters]
    }
    fn group(&self) -> SearchGroup {
        SearchGroup::Design
    }
    fn search(&self, ctx: &SearchContext<'_>) -> Result<Vec<SearchItem>, String> {
        let mut items = Vec::new();
        for p in ctx.session.doc.param_rows().into_iter().take(MAX_SOURCE_ITEMS) {
            let formatted = p
                .display_value
                .or(p.value)
                .filter(|v| v.is_finite())
                .map_or_else(|| truncate_chars(&p.expression, 64), |v| format!("{v} {}", p.unit).trim().to_string());
            let mut fields =
                vec![SearchField::title(&p.name), SearchField::identifier(&p.name), SearchField::secondary(format!("parameter {}", p.source))];
            if !p.comment.is_empty() {
                fields.push(SearchField::keywords(&p.comment));
            }
            if let Some(feature) = &p.feature {
                fields.push(SearchField::secondary(feature));
            }
            let detail = if p.comment.is_empty() { formatted.clone() } else { format!("{formatted} · {}", truncate_chars(&p.comment, 96)) };
            items.push(SearchItem {
                id: p.name.clone(),
                kind: "parameter".into(),
                group: SearchGroup::Design,
                title: p.name.clone(),
                subtitle: Some(format!("Parameter ({})", p.source)),
                detail: Some(detail),
                icon: Some("parameters".into()),
                installed: true,
                enabled: p.error.is_none(),
                disabled_reason: p.error.clone(),
                action: Some("parameters.change".into()),
                metadata: Some(json!({
                    "name": p.name,
                    "expression": p.expression,
                    "value": p.value,
                    "display_value": p.display_value,
                    "unit": p.unit,
                    "source": p.source,
                    "comment": p.comment,
                    "favorite": p.favorite,
                })),
                visibility: ItemVisibility::Public,
                fields,
            });
        }
        Ok(items)
    }
}

struct JointsSource;

impl SearchSource for JointsSource {
    fn id(&self) -> &'static str {
        "design.joints"
    }
    fn surfaces(&self) -> SearchSurfaces {
        SearchSurfaces::GLOBAL.union(SearchSurfaces::DESIGN).union(SearchSurfaces::BROWSER)
    }
    fn scopes(&self) -> &'static [SearchScope] {
        &[SearchScope::Design, SearchScope::Joints]
    }
    fn group(&self) -> SearchGroup {
        SearchGroup::Design
    }
    fn search(&self, ctx: &SearchContext<'_>) -> Result<Vec<SearchItem>, String> {
        let mut items = Vec::new();
        for joint in ctx.session.doc.assembly.joints.iter().take(MAX_SOURCE_ITEMS) {
            let kind_name = joint_kind_label(joint.kind);
            let fields = vec![
                SearchField::title(&joint.name),
                SearchField::identifier(joint.id.to_string()),
                SearchField::secondary(format!("joint {kind_name}")),
            ];
            items.push(SearchItem {
                id: joint.id.to_string(),
                kind: "joint".into(),
                group: SearchGroup::Design,
                title: joint.name.clone(),
                subtitle: Some(format!("Joint ({kind_name})")),
                detail: Some(format!("{kind_name} joint")),
                icon: Some("joint".into()),
                installed: true,
                enabled: true,
                disabled_reason: None,
                action: Some("joint.inspect".into()),
                metadata: Some(json!({
                    "joint_id": joint.id,
                    "kind": kind_name,
                })),
                visibility: ItemVisibility::Public,
                fields,
            });
        }
        Ok(items)
    }
}

fn joint_kind_label(kind: JointKind) -> &'static str {
    match kind {
        JointKind::Rigid => "rigid",
        JointKind::Revolute => "revolute",
        JointKind::Slider => "slider",
        JointKind::Cylindrical => "cylindrical",
        JointKind::PinSlot => "pin_slot",
        JointKind::Planar => "planar",
        JointKind::Ball => "ball",
        JointKind::Parallel => "parallel",
        JointKind::Perpendicular => "perpendicular",
        JointKind::Angle => "angle",
        JointKind::PointOnPlane => "point_on_plane",
        JointKind::PointOnLine => "point_on_line",
        JointKind::LineOnPlane => "line_on_plane",
    }
}

struct NamedViewsSource;

impl SearchSource for NamedViewsSource {
    fn id(&self) -> &'static str {
        "design.named_views"
    }
    fn surfaces(&self) -> SearchSurfaces {
        SearchSurfaces::GLOBAL.union(SearchSurfaces::DESIGN)
    }
    fn scopes(&self) -> &'static [SearchScope] {
        &[SearchScope::Design, SearchScope::NamedViews]
    }
    fn group(&self) -> SearchGroup {
        SearchGroup::Design
    }
    fn search(&self, ctx: &SearchContext<'_>) -> Result<Vec<SearchItem>, String> {
        let mut items = Vec::new();
        for view in ctx.session.doc.named_views.iter().take(MAX_SOURCE_ITEMS) {
            let fields = vec![SearchField::title(&view.name), SearchField::identifier(&view.name), SearchField::secondary("named view camera")];
            items.push(SearchItem {
                id: view.name.clone(),
                kind: "named_view".into(),
                group: SearchGroup::Design,
                title: view.name.clone(),
                subtitle: Some("Named view".into()),
                detail: Some("Saved camera view".into()),
                icon: Some("camera".into()),
                installed: true,
                enabled: true,
                disabled_reason: None,
                action: Some("view.apply".into()),
                metadata: Some(json!({
                    "name": view.name,
                    "distance": view.distance,
                })),
                visibility: ItemVisibility::Public,
                fields,
            });
        }
        Ok(items)
    }
}

struct PartsSource;

impl SearchSource for PartsSource {
    fn id(&self) -> &'static str {
        "parts"
    }
    fn surfaces(&self) -> SearchSurfaces {
        SearchSurfaces::GLOBAL.union(SearchSurfaces::PARTS)
    }
    fn scopes(&self) -> &'static [SearchScope] {
        &[SearchScope::Parts]
    }
    fn group(&self) -> SearchGroup {
        SearchGroup::Parts
    }
    fn search(&self, ctx: &SearchContext<'_>) -> Result<Vec<SearchItem>, String> {
        let families = crate::cmd::parts::catalog_families();
        let sized_tokens = parse_size_length_tokens(&ctx.query.text);
        let mut items = Vec::with_capacity(families.len());

        for fam in families {
            let sizes_joined = fam.sizes.join(" ");
            let title = format!("{} ({})", fam.name, fam.standard);
            let fields = vec![
                SearchField::title(&title),
                SearchField::identifier(fam.id),
                SearchField::keywords(format!("{} {} {} {}", fam.standard, fam.system, fam.aliases, sizes_joined)),
            ];
            items.push(SearchItem {
                id: fam.id.to_string(),
                kind: "standard_part".into(),
                group: SearchGroup::Parts,
                title,
                subtitle: Some(format!("{} · {}", fam.standard, fam.system)),
                detail: Some(format!("{} sizes", fam.sizes.len())),
                icon: Some("fastener".into()),
                installed: true,
                enabled: true,
                disabled_reason: None,
                action: Some("parts.insert".into()),
                metadata: Some(json!({
                    "family": fam.id,
                    "standard": fam.standard,
                    "system": fam.system,
                    "sizes": fam.sizes,
                })),
                visibility: ItemVisibility::Public,
                fields,
            });

            // When the query includes an explicit `<size>x<length>` (e.g. `M5x12 SHCS`), emit a
            // pre-parameterized hit for every family that actually carries that size.
            if fam.has_length {
                for (raw_tok, size_part, length_mm) in &sized_tokens {
                    if let Some(canonical_size) = fam.sizes.iter().find(|s| s.eq_ignore_ascii_case(size_part)) {
                        let sized_title = format!("{} {}x{}", fam.name, canonical_size, format_length(*length_mm));
                        let sized_id = format!("{}:{}x{}", fam.id, canonical_size, format_length(*length_mm));
                        items.push(SearchItem {
                            id: sized_id.clone(),
                            kind: "standard_part".into(),
                            group: SearchGroup::Parts,
                            title: sized_title.clone(),
                            subtitle: Some(format!("{} · {}", fam.standard, fam.system)),
                            detail: Some(format!("Insert {} ({}, {} mm)", fam.name, canonical_size, format_length(*length_mm))),
                            icon: Some("fastener".into()),
                            installed: true,
                            enabled: true,
                            disabled_reason: None,
                            action: Some("parts.insert".into()),
                            metadata: Some(json!({
                                "family": fam.id,
                                "standard": fam.standard,
                                "size": canonical_size,
                                "length": length_mm,
                            })),
                            visibility: ItemVisibility::Public,
                            fields: vec![
                                SearchField::title(&sized_title),
                                SearchField::identifier(sized_id),
                                SearchField::keywords(format!("{} {} {} {}", raw_tok, fam.standard, fam.aliases, canonical_size)),
                            ],
                        });
                    }
                }
            }
        }
        Ok(items)
    }
}

/// Extract `<size>x<length>` tokens like `M5x12` or `M6×20` from a query string.
fn parse_size_length_tokens(query: &str) -> Vec<(String, String, f64)> {
    let mut out = Vec::new();
    for raw in query.split_whitespace().take(super::MAX_QUERY_TOKENS) {
        let normalized = raw.replace('×', "x");
        if let Some((size, len_str)) = normalized.split_once(['x', 'X'])
            && !size.is_empty()
            && !len_str.is_empty()
            && let Ok(length) = len_str.parse::<f64>()
            && length.is_finite()
            && length > 0.0
            && length <= 10_000.0
        {
            out.push((raw.to_string(), size.to_string(), length));
        }
    }
    out
}

fn format_length(length: f64) -> String {
    if length.fract().abs() < 1e-9 { format!("{}", length as i64) } else { format!("{length}") }
}

struct SamplesSource;

impl SearchSource for SamplesSource {
    fn id(&self) -> &'static str {
        "samples"
    }
    fn surfaces(&self) -> SearchSurfaces {
        SearchSurfaces::GLOBAL
    }
    fn scopes(&self) -> &'static [SearchScope] {
        &[SearchScope::Samples]
    }
    fn group(&self) -> SearchGroup {
        SearchGroup::Help
    }
    fn search(&self, _ctx: &SearchContext<'_>) -> Result<Vec<SearchItem>, String> {
        let mut items = Vec::new();
        for sample in crate::sample::samples() {
            let slug = sample.name.to_ascii_lowercase().replace(' ', "_");
            items.push(SearchItem {
                id: slug.clone(),
                kind: "sample".into(),
                group: SearchGroup::Help,
                title: sample.name.to_string(),
                subtitle: Some("Sample design".into()),
                detail: Some(sample.about.to_string()),
                icon: Some("file".into()),
                installed: true,
                enabled: true,
                disabled_reason: None,
                action: None,
                metadata: Some(json!({"name": sample.name, "about": sample.about})),
                visibility: ItemVisibility::Public,
                fields: vec![
                    SearchField::title(sample.name),
                    SearchField::identifier(slug),
                    SearchField::keywords(sample.about),
                    SearchField::secondary("sample example"),
                ],
            });
        }
        Ok(items)
    }
}

struct InstalledPluginsSource;

impl SearchSource for InstalledPluginsSource {
    fn id(&self) -> &'static str {
        "plugins.installed"
    }
    fn surfaces(&self) -> SearchSurfaces {
        SearchSurfaces::GLOBAL.union(SearchSurfaces::PARTS)
    }
    fn scopes(&self) -> &'static [SearchScope] {
        &[SearchScope::Plugins]
    }
    fn group(&self) -> SearchGroup {
        SearchGroup::InstalledPlugins
    }
    fn search(&self, ctx: &SearchContext<'_>) -> Result<Vec<SearchItem>, String> {
        let mut items = Vec::new();
        for (package_id, path) in &ctx.session.plugin_state.packages {
            let package = crate::plugin::read_package(path).map_err(|e| format!("failed to read plugin `{package_id}`: {e}"))?;
            let manifest = package.manifest;
            let disabled = ctx.session.disabled_plugins.contains(package_id);
            let mut fields = vec![SearchField::title(&manifest.name), SearchField::identifier(&manifest.id)];
            if !manifest.description.is_empty() {
                fields.push(SearchField::keywords(&manifest.description));
            }
            if !manifest.capabilities.is_empty() {
                fields.push(SearchField::secondary(manifest.capabilities.join(" ")));
            }
            items.push(SearchItem {
                id: manifest.id.clone(),
                kind: "plugin".into(),
                group: SearchGroup::InstalledPlugins,
                title: manifest.name.clone(),
                subtitle: Some(format!("Plugin v{}", manifest.version)),
                detail: (!manifest.description.is_empty()).then(|| truncate_chars(&manifest.description, 160)),
                icon: Some("plugin".into()),
                installed: true,
                enabled: !disabled,
                disabled_reason: disabled.then(|| "plugin is disabled".into()),
                action: None,
                metadata: Some(json!({
                    "id": manifest.id,
                    "version": manifest.version,
                    "capabilities": manifest.capabilities,
                })),
                visibility: ItemVisibility::Public,
                fields,
            });
        }
        Ok(items)
    }
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut iter = text.chars();
    let taken: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() { format!("{taken}…") } else { taken }
}
