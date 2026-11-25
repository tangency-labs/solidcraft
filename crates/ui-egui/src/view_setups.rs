//! App-wide templates describe the arrangement of live views without recording design data.

use serde::Serialize;
use serde_json::{Value, json};
use solidcraft_views::{EMPTY, MAX_POPOUTS, MAX_VIEWS, Tiles, WindowArrangement, WindowLevel, WindowOptions, WindowPlacement};

use crate::{SolveApp, documents, view_windows};

const MAX_SETUPS: usize = 32;

#[derive(Clone, Debug, PartialEq, Serialize)]
struct Setup {
    windows: Vec<SetupWindow>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct SetupWindow {
    views: usize,
    level: SetupLevel,
    options: WindowOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    panel_layout: Option<Value>,
    placement: Option<Placement>,
    tiles: Option<Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SetupLevel {
    Full,
    ViewsOnly,
    Clean,
}

impl From<SetupLevel> for WindowLevel {
    fn from(level: SetupLevel) -> Self {
        match level {
            SetupLevel::Full => Self::Full,
            SetupLevel::ViewsOnly => Self::ViewsOnly,
            SetupLevel::Clean => Self::Clean,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Placement {
    pub(crate) display: Option<usize>,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
    pub(crate) maximised: bool,
    pub(crate) fullscreen: bool,
}

impl Placement {
    fn capture(app: &SolveApp, placement: &WindowPlacement) -> Self {
        Self {
            display: placement.display.as_deref().and_then(|display| view_windows::display_index(app, display)),
            x: placement.x,
            y: placement.y,
            w: placement.w,
            h: placement.h,
            maximised: placement.maximised,
            fullscreen: placement.fullscreen,
        }
    }

    fn valid(&self) -> bool {
        [self.x, self.y, self.w, self.h].into_iter().all(|value| value.is_finite() && value.abs() <= 100_000.0) && self.w > 0.0 && self.h > 0.0
    }

    fn same_position(&self, other: &Self) -> bool {
        self.display == other.display
            && self.maximised == other.maximised
            && self.fullscreen == other.fullscreen
            && [(self.x, other.x), (self.y, other.y), (self.w, other.w), (self.h, other.h)].into_iter().all(|(a, b)| (a - b).abs() <= 2.0)
    }
}

fn map_tiles(tiles: &Tiles, leaf: &impl Fn(u64) -> u64) -> Tiles {
    match tiles {
        Tiles::Leaf(id) => Tiles::Leaf(if *id == EMPTY { EMPTY } else { leaf(*id) }),
        Tiles::Split { axis, ratio, first, second } => {
            Tiles::Split { axis: *axis, ratio: *ratio, first: Box::new(map_tiles(first, leaf)), second: Box::new(map_tiles(second, leaf)) }
        }
    }
}

impl Setup {
    fn capture(app: &SolveApp) -> Self {
        let windows = std::iter::once(0)
            .chain(app.docs.views.windows().iter().map(|window| window.id))
            .filter_map(|id| {
                let views = app.docs.views.views_of(id)?;
                let tiles = views
                    .tiles()
                    .filter(|_| views.is_tiled())
                    .map(|tiles| map_tiles(tiles, &|view| views.index_of(view).map_or(EMPTY, |index| index as u64 + 1)).to_json());
                Some(SetupWindow {
                    views: views.len(),
                    level: view_windows::setup_level(app, id),
                    options: app.docs.views.window(id).map_or_else(WindowOptions::default, |window| window.options),
                    panel_layout: app.docs.views.window(id).and_then(|window| window.panel_layout.clone()),
                    placement: view_windows::setup_placement(app, id).map(|placement| Placement::capture(app, placement)),
                    tiles,
                })
            })
            .collect();
        Self { windows }
    }

    fn parse(value: &Value) -> Option<Self> {
        let stored = value.get("windows")?.as_array()?;
        if stored.is_empty() || stored.len() > MAX_POPOUTS + 1 {
            return None;
        }
        let mut total = 0;
        let mut windows = Vec::new();
        for window in stored {
            let views = usize::try_from(window.get("views")?.as_u64()?).ok()?;
            if !(1..=MAX_VIEWS).contains(&views) {
                return None;
            }
            total += views;
            let level = match window.get("level")?.as_str()? {
                "full" => SetupLevel::Full,
                "clean" => SetupLevel::Clean,
                "views_only" => SetupLevel::ViewsOnly,
                _ => return None,
            };
            let placement = match window.get("placement").filter(|value| !value.is_null()) {
                None => None,
                Some(value) => {
                    let number = |key| value.get(key)?.as_f64().map(|number| number as f32);
                    let placement = Placement {
                        display: value.get("display").and_then(Value::as_u64).and_then(|index| usize::try_from(index).ok()),
                        x: number("x")?,
                        y: number("y")?,
                        w: number("w")?,
                        h: number("h")?,
                        maximised: value.get("maximised")?.as_bool()?,
                        fullscreen: value.get("fullscreen")?.as_bool()?,
                    };
                    if !placement.valid() {
                        return None;
                    }
                    Some(placement)
                }
            };
            let tiles = match window.get("tiles").filter(|value| !value.is_null()) {
                None => None,
                Some(value) => {
                    let tiles = Tiles::from_json(value)?;
                    if tiles.views().into_iter().any(|slot| slot > views as u64) {
                        return None;
                    }
                    Some(tiles.to_json())
                }
            };
            let options = window.get("options").cloned().and_then(|value| serde_json::from_value(value).ok()).unwrap_or_default();
            let panel_layout = window.get("panel_layout").and_then(solidcraft_views::ViewSet::read_panel_layout);
            windows.push(SetupWindow { views, level, options, panel_layout, placement, tiles });
        }
        (total <= MAX_VIEWS).then_some(Self { windows })
    }

    fn matches(&self, actual: &Self) -> bool {
        self.windows.len() == actual.windows.len()
            && self.windows.iter().zip(&actual.windows).all(|(saved, actual)| {
                saved.views == actual.views
                    && saved.level == actual.level
                    && saved.options == actual.options
                    && saved.panel_layout == actual.panel_layout
                    && saved.tiles == actual.tiles
                    && saved.placement.as_ref().is_none_or(|placement| actual.placement.as_ref().is_none_or(|actual| placement.same_position(actual)))
            })
    }
}

pub(crate) fn current_snapshot(app: &SolveApp) -> Option<Value> {
    serde_json::to_value(Setup::capture(app)).ok()
}

fn valid_name(name: &str) -> Option<&str> {
    let name = name.trim();
    (!name.is_empty() && name.chars().count() <= solidcraft_views::MAX_NAME && !name.chars().any(char::is_control)).then_some(name)
}

pub(crate) fn names(app: &SolveApp) -> Vec<String> {
    app.preferences
        .view_window_setups
        .iter()
        .take(MAX_SETUPS)
        .filter(|(name, value)| valid_name(name) == Some(name) && Setup::parse(value).is_some())
        .map(|(name, _)| name.clone())
        .collect()
}

pub(crate) fn matching_name(app: &SolveApp) -> Option<String> {
    if app.preferences.view_window_setups.is_empty() {
        return None;
    }
    let current = if app.view_windows.is_child() { app.view_windows.setup_snapshot().and_then(Setup::parse)? } else { Setup::capture(app) };
    app.preferences.view_window_setups.iter().take(MAX_SETUPS).find_map(|(name, value)| {
        (valid_name(name) == Some(name) && Setup::parse(value).is_some_and(|saved| saved.matches(&current))).then(|| name.clone())
    })
}

fn save(app: &mut SolveApp, name: &str) -> Result<(), String> {
    if app.preferences.view_window_setups.contains_key(name) {
        return Err("layout_name_exists".into());
    }
    if app.preferences.view_window_setups.len() >= MAX_SETUPS {
        return Err("too_many_layouts".into());
    }
    let snapshot = current_snapshot(app).ok_or("invalid_layout_setup")?;
    app.preferences.view_window_setups.insert(name.to_owned(), snapshot);
    Ok(())
}

fn apply(app: &mut SolveApp, name: &str) -> Result<(), String> {
    let setup = app.preferences.view_window_setups.get(name).and_then(Setup::parse).ok_or("no_such_layout")?;
    view_windows::remember_strip_order(app);
    let ids: Vec<_> = view_windows::strip_views(app).into_iter().map(|(_, view)| view.id).collect();
    let mut remaining = ids.into_iter();
    let window_limit = if view_windows::supports_pop_out(app) { MAX_POPOUTS + 1 } else { 1 };
    let mut groups: Vec<_> = setup
        .windows
        .into_iter()
        .take(window_limit)
        .map_while(|template| {
            let views: Vec<_> = remaining.by_ref().take(template.views).collect();
            if views.is_empty() {
                return None;
            }
            let tiles = template
                .tiles
                .as_ref()
                .and_then(Tiles::from_json)
                .map(|tiles| map_tiles(&tiles, &|slot| views.get(slot.saturating_sub(1) as usize).copied().unwrap_or(EMPTY)));
            let active = tiles.as_ref().and_then(|tiles| {
                tiles.views().into_iter().find(|id| {
                    app.docs
                        .views
                        .window_of(*id)
                        .and_then(|owner| app.docs.views.views_of(owner))
                        .and_then(|views| views.get(*id))
                        .is_some_and(|view| view.kind.can_tile())
                })
            });
            Some(WindowArrangement {
                views,
                active,
                tiles,
                level: template.level.into(),
                options: template.options,
                panel_layout: template.panel_layout,
                placement: template.placement.map(|placement| view_windows::resolve_setup_placement(app, placement)),
            })
        })
        .collect();
    let Some(main) = groups.first_mut() else { return Err("invalid_layout_setup".into()) };
    main.views.extend(remaining);
    let presentation: Vec<_> = groups.iter().map(|group| (group.level, group.placement.clone())).collect();
    if !documents::change_views(app, |views| views.arrange_windows(groups)) {
        return Err("invalid_layout_setup".into());
    }
    view_windows::apply_setup_presentation(app, presentation);
    Ok(())
}

pub(crate) enum DeferredSetup {
    Save(String),
    Apply(String),
}

impl DeferredSetup {
    pub(crate) fn run(self, app: &mut SolveApp) -> Result<(), String> {
        match self {
            Self::Save(name) => save(app, &name),
            Self::Apply(name) => apply(app, &name),
        }
    }
}

pub(crate) fn command(app: &mut SolveApp, id: &str, request: Value) -> Result<Value, String> {
    if id == "tab.layout.list" {
        return Ok(json!({"layouts": names(app)}));
    }
    let name = request.get("name").and_then(Value::as_str).and_then(valid_name).ok_or("invalid_layout_name")?;
    match id {
        "tab.layout.save" | "tab.layout.apply" => {
            let action = if id == "tab.layout.save" { DeferredSetup::Save(name.into()) } else { DeferredSetup::Apply(name.into()) };
            if app.view_windows.is_child() {
                view_windows::defer_setup(app, action);
            } else {
                action.run(app)?;
            }
        }
        "tab.layout.rename" => {
            let new = request.get("new_name").and_then(Value::as_str).and_then(valid_name).ok_or("invalid_layout_name")?;
            if app.preferences.view_window_setups.contains_key(new) {
                return Err("layout_name_exists".into());
            }
            let setup = app.preferences.view_window_setups.remove(name).ok_or("no_such_layout")?;
            app.preferences.view_window_setups.insert(new.to_owned(), setup);
        }
        "tab.layout.remove" => {
            app.preferences.view_window_setups.remove(name).ok_or("no_such_layout")?;
        }
        _ => return Err("no_such_command".into()),
    }
    Ok(request)
}
