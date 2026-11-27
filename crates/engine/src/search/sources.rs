//! Built-in [`SearchSource`] implementations (RFC 0019 §3.1):
//! commands, open design entities (features, bodies, sketches, construction geometry, components,
//! occurrences, parameters, joints, named views), standard parts library, built-in samples and
//! installed plugins.

use std::collections::{BTreeMap, HashMap};

use serde_json::json;
use solidcraft_doc::joints::JointKind;
use solidcraft_doc::{Document, FeatureKind, WorldNames};

use super::{ItemVisibility, SearchContext, SearchField, SearchGroup, SearchItem, SearchRegistry, SearchScope, SearchSource, SearchSurfaces};

/// Maximum number of items emitted by one design source to bound work on huge designs.
const MAX_SOURCE_ITEMS: usize = 10_000;
/// Maximum ancestor depth walked when building component/occurrence paths.
const MAX_PATH_DEPTH: usize = 64;

/// Register every built-in search source in deterministic order.
pub fn register_builtins(registry: &mut SearchRegistry) {
    registry
        .register(Box::new(CommandsSource))
        .register(Box::new(FeaturesSource))
        .register(Box::new(BodiesSource))
        .register(Box::new(SketchesSource))
        .register(Box::new(ConstructionSource))
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
            let kind_label = feature_kind_label(&feature.kind);
            let rolled_back = idx >= marker;
            let comp_path = component_path_label(doc, feature.component);
            let occ_path = first_occurrence_path(doc, feature.component);
            let param_summary = feature.param_names.iter().take(8).find_map(|name| params.get(name.as_str())).map(format_param_value);
            let mut detail_parts = vec![kind_label.to_string()];
            if let Some(val) = &param_summary
                && !val.is_empty()
            {
                detail_parts.push(val.clone());
            }
            if let Some(path) = &comp_path {
                detail_parts.push(path.clone());
            }
            if feature.suppressed {
                detail_parts.push("suppressed".into());
            }
            if rolled_back {
                detail_parts.push("rolled back".into());
            }
            let subtitle = match &comp_path {
                Some(path) => Some(format!("{kind_label} · {path}")),
                None => Some(kind_label.to_string()),
            };
            let mut fields = vec![
                SearchField::title(&feature.name),
                SearchField::identifier(feature.id.to_string()),
                SearchField::secondary(format!("{kind_label} {type_name}")),
            ];
            if let Some(path) = &comp_path {
                fields.push(SearchField::secondary(path));
            }
            items.push(SearchItem {
                id: feature.id.to_string(),
                kind: "feature".into(),
                group: SearchGroup::Design,
                title: feature.name.clone(),
                subtitle,
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
                    "kind_label": kind_label,
                    "component": feature.component,
                    "component_path": comp_path,
                    "occurrence_path": occ_path,
                    "is_sketch": matches!(feature.kind, FeatureKind::Sketch { .. }),
                    "is_construction": feature.kind.is_construction(),
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
        let doc = &s.doc;
        let state = s.model.state();
        let world_names = WorldNames::new(doc, &state);
        let mut comp_placements: HashMap<u64, Vec<Vec<u64>>> = HashMap::new();
        for (owner, path, _) in doc.placements().into_iter().take(MAX_SOURCE_ITEMS) {
            if owner != 0 {
                comp_placements.entry(owner).or_default().push(path);
            }
        }
        let mut items = Vec::new();
        for body in state.bodies.iter().take(MAX_SOURCE_ITEMS) {
            if items.len() >= MAX_SOURCE_ITEMS {
                break;
            }
            if s.body_removed_later(&body.name) {
                continue;
            }
            let component = doc.model_body_component(body);
            let comp_path = component_path_label(doc, component);
            let material = doc.materials.get(&body.name).cloned();
            let (body_kind, body_kind_label, icon) = if body.body.is_mesh() {
                ("mesh", "Mesh", "mesh_body")
            } else if body.is_surface() {
                ("surface", "Surface", "surface")
            } else {
                ("solid", "Solid", "body")
            };

            let placements: &[Vec<u64>] = if component == 0 { &[] } else { comp_placements.get(&component).map(Vec::as_slice).unwrap_or(&[]) };

            if placements.is_empty() {
                let visible = s.body_visible(&body.name);
                let mut fields =
                    vec![SearchField::title(&body.name), SearchField::identifier(&body.name), SearchField::secondary(format!("body {body_kind}"))];
                if let Some(path) = &comp_path {
                    fields.push(SearchField::secondary(path));
                }
                if let Some(mat) = &material {
                    fields.push(SearchField::secondary(mat));
                }
                let subtitle = match &comp_path {
                    Some(path) => Some(format!("Body · {path}")),
                    None => Some("Body".into()),
                };
                let mut detail_parts = vec![body_kind_label.to_string(), if visible { "Visible".into() } else { "Hidden".into() }];
                if let Some(mat) = &material {
                    detail_parts.push(mat.clone());
                }
                items.push(SearchItem {
                    id: body.name.clone(),
                    kind: "body".into(),
                    group: SearchGroup::Design,
                    title: body.name.clone(),
                    subtitle,
                    detail: Some(detail_parts.join(" · ")),
                    icon: Some(icon.into()),
                    installed: true,
                    enabled: true,
                    disabled_reason: None,
                    action: Some("select.set".into()),
                    metadata: Some(json!({
                        "name": body.name,
                        "source_name": body.name,
                        "component": component,
                        "component_path": comp_path,
                        "occurrence_path": Vec::<u64>::new(),
                        "occurrence_label": Option::<String>::None,
                        "visible": visible,
                        "body_kind": body_kind,
                        "material": material,
                    })),
                    visibility: ItemVisibility::Public,
                    fields,
                });
            } else {
                for path in placements {
                    if items.len() >= MAX_SOURCE_ITEMS {
                        break;
                    }
                    let world_name = world_names.name(&body.name, component, path);
                    let visible = s.body_visible(&world_name);
                    let occ_label = occurrence_path_label(doc, path);
                    let location = occ_label.as_deref().or(comp_path.as_deref());
                    let mut fields = vec![
                        SearchField::title(&world_name),
                        SearchField::identifier(&world_name),
                        SearchField::secondary(format!("body {body_kind}")),
                    ];
                    if world_name != body.name {
                        fields.push(SearchField::identifier(&body.name));
                    }
                    if let Some(path_label) = &comp_path {
                        fields.push(SearchField::secondary(path_label));
                    }
                    if let Some(occ) = &occ_label {
                        fields.push(SearchField::secondary(occ));
                    }
                    if let Some(mat) = &material {
                        fields.push(SearchField::secondary(mat));
                    }
                    let subtitle = match location {
                        Some(loc) => Some(format!("Body · {loc}")),
                        None => Some("Body".into()),
                    };
                    let mut detail_parts = vec![body_kind_label.to_string(), if visible { "Visible".into() } else { "Hidden".into() }];
                    if let Some(mat) = &material {
                        detail_parts.push(mat.clone());
                    }
                    items.push(SearchItem {
                        id: world_name.clone(),
                        kind: "body".into(),
                        group: SearchGroup::Design,
                        title: world_name.clone(),
                        subtitle,
                        detail: Some(detail_parts.join(" · ")),
                        icon: Some(icon.into()),
                        installed: true,
                        enabled: true,
                        disabled_reason: None,
                        action: Some("select.set".into()),
                        metadata: Some(json!({
                            "name": world_name,
                            "source_name": body.name,
                            "component": component,
                            "component_path": comp_path,
                            "occurrence_path": path,
                            "occurrence_label": occ_label,
                            "visible": visible,
                            "body_kind": body_kind,
                            "material": material,
                        })),
                        visibility: ItemVisibility::Public,
                        fields,
                    });
                }
            }
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
            let comp_path = component_path_label(doc, feature.component);
            let occ_path = first_occurrence_path(doc, feature.component);
            let mut fields =
                vec![SearchField::title(&feature.name), SearchField::identifier(feature.id.to_string()), SearchField::secondary("sketch")];
            if let Some(path) = &comp_path {
                fields.push(SearchField::secondary(path));
            }
            let subtitle = match &comp_path {
                Some(path) => Some(format!("Sketch · {path}")),
                None => Some("Sketch".into()),
            };
            let mut detail_parts = vec![format!("Sketch #{}", feature.id)];
            if let Some(path) = &comp_path {
                detail_parts.push(path.clone());
            }
            if feature.suppressed {
                detail_parts.push("suppressed".into());
            }
            items.push(SearchItem {
                id: feature.id.to_string(),
                kind: "sketch".into(),
                group: SearchGroup::Design,
                title: feature.name.clone(),
                subtitle,
                detail: Some(detail_parts.join(" · ")),
                icon: Some("sketch".into()),
                installed: true,
                enabled: !feature.suppressed,
                disabled_reason: feature.suppressed.then(|| "sketch is suppressed".into()),
                action: Some("sketch.edit".into()),
                metadata: Some(json!({
                    "sketch_id": feature.id,
                    "feature_id": feature.id,
                    "component": feature.component,
                    "component_path": comp_path,
                    "occurrence_path": occ_path,
                    "suppressed": feature.suppressed,
                })),
                visibility: ItemVisibility::Public,
                fields,
            });
        }
        Ok(items)
    }
}

struct ConstructionSource;

impl SearchSource for ConstructionSource {
    fn id(&self) -> &'static str {
        "design.construction"
    }
    fn surfaces(&self) -> SearchSurfaces {
        SearchSurfaces::GLOBAL.union(SearchSurfaces::DESIGN).union(SearchSurfaces::BROWSER)
    }
    fn scopes(&self) -> &'static [SearchScope] {
        &[SearchScope::Design, SearchScope::Construction]
    }
    fn group(&self) -> SearchGroup {
        SearchGroup::Design
    }
    fn search(&self, ctx: &SearchContext<'_>) -> Result<Vec<SearchItem>, String> {
        let s = ctx.session;
        let doc = &s.doc;
        let before_marker = doc.marker.unwrap_or(doc.features.len());
        let mut items = Vec::new();
        for feature in doc.features.iter().take(before_marker).take(MAX_SOURCE_ITEMS) {
            if !feature.kind.is_construction() {
                continue;
            }
            let (kind_slug, kind_label, icon) = match feature.kind {
                FeatureKind::ConstructionAxis { .. } => ("axis", "Construction Axis", "axis"),
                FeatureKind::ConstructionPoint { .. } => ("point", "Construction Point", "point"),
                _ => ("plane", "Construction Plane", "plane"),
            };
            let visible = !s.visibility.hidden_origin.contains(&feature.name);
            let comp_path = component_path_label(doc, feature.component);
            let mut fields = vec![
                SearchField::title(&feature.name),
                SearchField::identifier(feature.id.to_string()),
                SearchField::secondary(format!("construction {kind_slug} {kind_label}")),
            ];
            if let Some(path) = &comp_path {
                fields.push(SearchField::secondary(path));
            }
            let subtitle = match &comp_path {
                Some(path) => Some(format!("{kind_label} · {path}")),
                None => Some(kind_label.to_string()),
            };
            let mut detail_parts = vec![kind_label.to_string(), if visible { "Visible".into() } else { "Hidden".into() }];
            if feature.suppressed {
                detail_parts.push("suppressed".into());
            }
            items.push(SearchItem {
                id: feature.id.to_string(),
                kind: "construction".into(),
                group: SearchGroup::Design,
                title: feature.name.clone(),
                subtitle,
                detail: Some(detail_parts.join(" · ")),
                icon: Some(icon.into()),
                installed: true,
                enabled: !feature.suppressed,
                disabled_reason: feature.suppressed.then(|| "construction feature is suppressed".into()),
                action: Some("select.set".into()),
                metadata: Some(json!({
                    "feature_id": feature.id,
                    "name": feature.name,
                    "construction_kind": kind_slug,
                    "component": feature.component,
                    "component_path": comp_path,
                    "visible": visible,
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
        let comp_names: HashMap<u64, &str> = doc.components.iter().map(|c| (c.id, c.name.as_str())).collect();
        let mut items = Vec::new();
        for comp in doc.components.iter().take(MAX_SOURCE_ITEMS) {
            let comp_path = component_path_label(doc, comp.id);
            let parent_path = component_path_label(doc, comp.parent);
            let occ_path = first_occurrence_path(doc, comp.id);
            let linked_source = doc.links.get(&comp.id).map(|l| l.path.as_str());
            let std_part = doc.parts.get(&comp.id);
            let part_number = comp.metadata.0.get("part_number").and_then(serde_json::Value::as_str);
            let description = comp.metadata.0.get("description").and_then(serde_json::Value::as_str);

            let mut fields = vec![SearchField::title(&comp.name), SearchField::identifier(comp.id.to_string()), SearchField::secondary("component")];
            if let Some(parent) = &parent_path {
                fields.push(SearchField::secondary(parent));
            }
            if let Some(link) = linked_source {
                fields.push(SearchField::keywords(format!("linked external {link}")));
            }
            if let Some(part) = std_part {
                fields.push(SearchField::keywords(format!("standard part {} {}", part.family, part.size)));
            }
            if let Some(pn) = part_number
                && !pn.is_empty()
            {
                fields.push(SearchField::identifier(pn));
            }
            if let Some(desc) = description
                && !desc.is_empty()
            {
                fields.push(SearchField::keywords(desc));
            }

            let subtitle = match (linked_source, std_part, &parent_path) {
                (Some(_), _, Some(parent)) => Some(format!("Linked component · {parent}")),
                (Some(_), _, None) => Some("Linked component".into()),
                (None, Some(_), Some(parent)) => Some(format!("Standard part · {parent}")),
                (None, Some(_), None) => Some("Standard part".into()),
                (None, None, Some(parent)) => Some(format!("Component · {parent}")),
                (None, None, None) => Some("Component".into()),
            };
            let detail = if let Some(link) = linked_source {
                Some(format!("Linked: {}", truncate_chars(link, 96)))
            } else if let Some(part) = std_part {
                match part.length {
                    Some(len) => Some(format!("{} {}x{}", part.family, part.size, format_length(len))),
                    None => Some(format!("{} {}", part.family, part.size)),
                }
            } else if let Some(pn) = part_number
                && !pn.is_empty()
            {
                Some(format!("Part #{pn}"))
            } else {
                Some(format!("Component #{}", comp.id))
            };
            items.push(SearchItem {
                id: format!("component:{}", comp.id),
                kind: "component".into(),
                group: SearchGroup::Design,
                title: comp.name.clone(),
                subtitle,
                detail,
                icon: Some("component".into()),
                installed: true,
                enabled: true,
                disabled_reason: None,
                action: Some("component.activate".into()),
                metadata: Some(json!({
                    "component_id": comp.id,
                    "parent": comp.parent,
                    "component_path": comp_path,
                    "occurrence_path": occ_path,
                    "linked_source": linked_source,
                    "standard_part": std_part.map(|p| json!({"family": p.family, "size": p.size, "length": p.length})),
                    "part_number": part_number,
                })),
                visibility: ItemVisibility::Public,
                fields,
            });
        }
        for occ in doc.occurrences.iter().take(MAX_SOURCE_ITEMS) {
            let path = doc.occurrence_path(occ.id).unwrap_or_else(|| vec![occ.id]);
            let occ_label = occurrence_path_label(doc, &path);
            let comp_name = comp_names.get(&occ.component).copied();
            let linked_source = doc.links.get(&occ.component).map(|l| l.path.as_str());
            let mut fields = vec![
                SearchField::title(&occ.name),
                SearchField::identifier(occ.id.to_string()),
                SearchField::secondary("occurrence placement component"),
            ];
            if let Some(cname) = comp_name {
                fields.push(SearchField::secondary(cname));
            }
            if let Some(label) = &occ_label {
                fields.push(SearchField::secondary(label));
            }
            if let Some(link) = linked_source {
                fields.push(SearchField::keywords(format!("linked external {link}")));
            }
            let subtitle = match (linked_source, &occ_label) {
                (Some(_), Some(label)) if label != &occ.name => Some(format!("Linked placement · {label}")),
                (Some(_), _) => Some("Linked placement".into()),
                (None, Some(label)) if label != &occ.name => Some(format!("Placement · {label}")),
                _ => Some("Component placement".into()),
            };
            let mut detail_parts = Vec::new();
            if occ.grounded {
                detail_parts.push("Grounded".into());
            }
            if let Some(link) = linked_source {
                detail_parts.push(format!("Linked: {}", truncate_chars(link, 80)));
            } else if let Some(cname) = comp_name {
                detail_parts.push(cname.to_string());
            } else {
                detail_parts.push(format!("Occurrence #{}", occ.id));
            }
            items.push(SearchItem {
                id: format!("occurrence:{}", occ.id),
                kind: "occurrence".into(),
                group: SearchGroup::Design,
                title: occ.name.clone(),
                subtitle,
                detail: Some(detail_parts.join(" · ")),
                icon: Some("component".into()),
                installed: true,
                enabled: true,
                disabled_reason: None,
                action: Some("select.set".into()),
                metadata: Some(json!({
                    "occurrence_id": occ.id,
                    "component_id": occ.component,
                    "component_name": comp_name,
                    "parent": occ.parent,
                    "path": path,
                    "occurrence_label": occ_label,
                    "grounded": occ.grounded,
                    "linked_source": linked_source,
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
            let value_expr = format_param_detail(&p);
            let mut fields = vec![
                SearchField::title(&p.name),
                SearchField::identifier(&p.name),
                SearchField::secondary(if p.source == "user" { "parameter user".to_string() } else { format!("parameter dimension {}", p.source) }),
            ];
            if !p.comment.is_empty() {
                fields.push(SearchField::keywords(&p.comment));
            }
            if let Some(text) = &p.text {
                if !text.is_empty() {
                    fields.push(SearchField::secondary(text));
                }
            } else if expression_is_formula(&p.expression) {
                fields.push(SearchField::secondary(&p.expression));
            }
            if let Some(input) = p.input {
                fields.push(SearchField::secondary(input));
            }
            let subtitle = match (&p.feature, p.input) {
                (Some(feat), Some(inp)) => Some(format!("Dimension ({feat}: {inp})")),
                (Some(feat), None) => Some(format!("Dimension ({feat})")),
                (None, _) if p.source == "sketch" => Some("Dimension (sketch)".into()),
                (None, _) => Some("Parameter (user)".into()),
            };
            let detail = if p.comment.is_empty() { value_expr } else { format!("{value_expr} · {}", truncate_chars(&p.comment, 96)) };
            items.push(SearchItem {
                id: p.name.clone(),
                kind: "parameter".into(),
                group: SearchGroup::Design,
                title: p.name.clone(),
                subtitle,
                detail: Some(detail),
                icon: Some("params".into()),
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
                    "feature": p.feature,
                    "input": p.input,
                    "comment": p.comment,
                    "favorite": p.favorite,
                    "text": p.text,
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
        let s = ctx.session;
        let doc = &s.doc;
        let occ_names: HashMap<u64, &str> = doc.occurrences.iter().map(|o| (o.id, o.name.as_str())).collect();
        let mut items = Vec::new();
        for joint in doc.assembly.joints.iter().take(MAX_SOURCE_ITEMS) {
            let kind_name = joint_kind_label(joint.kind);
            let occ_a = occ_names.get(&joint.a.occurrence).copied();
            let occ_b = occ_names.get(&joint.b.occurrence).copied();
            let visible = !s.visibility.hidden_joints.contains(&joint.id);
            let mut fields = vec![
                SearchField::title(&joint.name),
                SearchField::identifier(joint.id.to_string()),
                SearchField::secondary(format!("joint {kind_name}")),
            ];
            if let Some(a) = occ_a {
                fields.push(SearchField::secondary(a));
            }
            if let Some(b) = occ_b {
                fields.push(SearchField::secondary(b));
            }
            let detail = match (occ_a, occ_b) {
                (Some(a), Some(b)) => format!("{kind_name} · {a} ↔ {b}"),
                (Some(a), None) => format!("{kind_name} · {a}"),
                (None, Some(b)) => format!("{kind_name} · {b}"),
                (None, None) => format!("{kind_name} joint"),
            };
            items.push(SearchItem {
                id: joint.id.to_string(),
                kind: "joint".into(),
                group: SearchGroup::Design,
                title: joint.name.clone(),
                subtitle: Some(format!("Joint ({kind_name})")),
                detail: Some(detail),
                icon: Some("joint".into()),
                installed: true,
                enabled: true,
                disabled_reason: None,
                action: Some("joint.inspect".into()),
                metadata: Some(json!({
                    "joint_id": joint.id,
                    "kind": kind_name,
                    "occurrence_a": occ_a,
                    "occurrence_b": occ_b,
                    "visible": visible,
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
                icon: Some("perspective".into()),
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

fn component_path_label(doc: &Document, comp_id: u64) -> Option<String> {
    if comp_id == 0 {
        return None;
    }
    let mut parts = Vec::new();
    let mut cur = comp_id;
    let mut seen = Vec::new();
    while cur != 0 && parts.len() < MAX_PATH_DEPTH {
        if seen.contains(&cur) {
            break;
        }
        seen.push(cur);
        let Some(comp) = doc.components.iter().find(|c| c.id == cur) else { break };
        parts.push(comp.name.as_str());
        cur = comp.parent;
    }
    if parts.is_empty() {
        return None;
    }
    parts.reverse();
    Some(parts.join(" › "))
}

fn occurrence_path_label(doc: &Document, path: &[u64]) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let mut parts = Vec::with_capacity(path.len().min(MAX_PATH_DEPTH));
    for &id in path.iter().take(MAX_PATH_DEPTH) {
        if let Some(occ) = doc.occurrences.iter().find(|o| o.id == id) {
            parts.push(occ.name.as_str());
        }
    }
    (!parts.is_empty()).then(|| parts.join(" › "))
}

fn first_occurrence_path(doc: &Document, comp_id: u64) -> Vec<u64> {
    if comp_id == 0 {
        return Vec::new();
    }
    doc.occurrence_of(comp_id).and_then(|o| doc.occurrence_path(o.id)).unwrap_or_default()
}

fn feature_kind_label(kind: &FeatureKind) -> &'static str {
    match kind {
        FeatureKind::Sketch { .. } => "Sketch",
        FeatureKind::Extrude { .. } => "Extrude",
        FeatureKind::Revolve { .. } => "Revolve",
        FeatureKind::Fillet { .. } => "Fillet",
        FeatureKind::Chamfer { .. } => "Chamfer",
        FeatureKind::Box { .. } => "Box",
        FeatureKind::Cylinder { .. } => "Cylinder",
        FeatureKind::Sphere { .. } => "Sphere",
        FeatureKind::Torus { .. } => "Torus",
        FeatureKind::Cone { .. } => "Cone",
        FeatureKind::Combine { .. } => "Combine",
        FeatureKind::Pattern { pattern: solidcraft_doc::PatternKind::Rectangular { .. }, .. } => "Rectangular Pattern",
        FeatureKind::Pattern { pattern: solidcraft_doc::PatternKind::Path { .. }, .. } => "Path Pattern",
        FeatureKind::Pattern { .. } => "Circular Pattern",
        FeatureKind::Mirror { .. } => "Mirror",
        FeatureKind::ConstructionPlane { .. } => "Construction Plane",
        FeatureKind::ConstructionAxis { .. } => "Construction Axis",
        FeatureKind::ConstructionPoint { .. } => "Construction Point",
        FeatureKind::Hole { .. } => "Hole",
        FeatureKind::Thread { .. } => "Thread",
        FeatureKind::Loft { .. } => "Loft",
        FeatureKind::Sweep { .. } => "Sweep",
        FeatureKind::Shell { .. } => "Shell",
        FeatureKind::Draft { .. } => "Draft",
        FeatureKind::Split { .. } => "Split Body",
        FeatureKind::SplitFace { .. } => "Split Face",
        _ => {
            let raw = kind.type_name();
            raw.strip_suffix("Feature").unwrap_or(raw)
        }
    }
}

fn format_param_value(p: &solidcraft_doc::ParamRow) -> String {
    if let Some(text) = &p.text {
        return format!("\"{}\"", truncate_chars(text, 64));
    }
    p.display_value
        .or(p.value)
        .filter(|v| v.is_finite())
        .map_or_else(|| truncate_chars(&p.expression, 64), |v| format!("{v} {}", p.unit).trim().to_string())
}

fn format_param_detail(p: &solidcraft_doc::ParamRow) -> String {
    if let Some(text) = &p.text {
        return format!("\"{}\"", truncate_chars(text, 64));
    }
    let val = format_param_value(p);
    let expr = p.expression.trim();
    if !expr.is_empty() && expression_is_formula(expr) && !expr.eq_ignore_ascii_case(&val) {
        format!("{val} = {}", truncate_chars(expr, 64))
    } else {
        val
    }
}

fn expression_is_formula(expr: &str) -> bool {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Check if the expression is a plain numeric literal with an optional unit (`75`, `75 mm`, `75mm`).
    let mut parts = trimmed.split_whitespace();
    let first = parts.next().unwrap_or("");
    let second = parts.next();
    let third = parts.next();
    let is_unit = |u: &str| !u.is_empty() && u.chars().all(|c| c.is_ascii_alphabetic() || c == '°' || c == '"');
    if third.is_none() && first.parse::<f64>().is_ok() && second.is_none_or(is_unit) {
        return false;
    }
    if second.is_none() {
        let num_end = first.find(|c: char| c.is_ascii_alphabetic() || c == '°' || c == '"').unwrap_or(first.len());
        if num_end > 0 && num_end < first.len() && first[..num_end].parse::<f64>().is_ok() && is_unit(&first[num_end..]) {
            return false;
        }
    }
    true
}
