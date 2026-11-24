//! Settings an organization's administrator sets for the people who use this computer (#10260).
//!
//! On Windows the administrator writes values under `Software\Policies\SolidCraft`, with Group
//! Policy or Intune (the templates are in `packaging/windows/policies/`). On macOS they are a
//! managed preference of the app's domain ([`MACOS_DOMAIN`]), from a configuration profile or MDM.
//! On Linux and FreeBSD they are the JSON file [`SYSTEM_FILE`]. A value there is mandatory: it
//! replaces the person's own preference and the control shows as set by the organization. The
//! same value names under `Recommended` are defaults the person may change; they are read here
//! and not applied yet.
//!
//! The machine's value wins over the user's (`HKEY_LOCAL_MACHINE` over `HKEY_CURRENT_USER`, the
//! computer's managed preferences over the user's), value by value. A value of the wrong type or
//! outside its range counts as not set. Nothing is written,
//! nothing is sent anywhere, and a computer without these values behaves as it always has.
//! Policy is read once, when SolidCraft starts.
//!
//! This is not the launcher of `update::managed_by`, which installs and updates a copy.

use crate::SolveApp;
use std::collections::BTreeMap;

/// The registry key of the mandatory values, under either hive.
pub const KEY: &str = r"Software\Policies\SolidCraft";

/// The subkey of [`KEY`] with the recommended values.
pub const RECOMMENDED: &str = "Recommended";

/// 0: no update checks. 1: checks at start and once a day. Locks `check_updates`.
pub const CHECK_FOR_UPDATES: &str = "CheckForUpdates";

/// 0: no anonymous usage statistics, and the person cannot turn them on. 1: allowed, the person
/// chooses. Locks `usage_statistics` when 0.
pub const USAGE_STATISTICS: &str = "UsageStatistics";
/// The highest `AgentTurnLimit`: the most Preferences offers (0 is no limit).
pub const MAX_TURN_LIMIT: u32 = 200;

/// The longest text value read for a setting; anything longer is not a valid choice.
const MAX_CHOICE: usize = 64;

/// What a setting's policy value may be.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// A `REG_DWORD`: 0 or 1.
    Switch,
    /// A `REG_SZ`: one of [`crate::prefs::UNITS`].
    Units,
    /// A `REG_SZ`: `system` or the id of a language SolidCraft has.
    Language,
    /// A `REG_DWORD` from 0 to `max`.
    Number { max: u32 },
    /// A `REG_SZ`: one of the agent permission modes (`ask`, `edits`, `all`).
    Permission,
    /// A `REG_SZ`: the path of a theme file to follow, as `theme_follow` takes it (at most
    /// [`crate::theme_files::MAX_FOLLOW_PATH`] bytes, no control characters, never empty).
    ThemeFile,
}

/// A preference an organization can set: the policy's value name, the preference's `ui.prefs`
/// name, and what the value may be.
pub struct Lock {
    pub policy: &'static str,
    pub pref: &'static str,
    kind: Kind,
}

/// Every preference a policy can set. Keep `packaging/windows/policies/` in step.
pub const LOCKS: &[Lock] = &[
    Lock { policy: "AgentControl", pref: "agent_control", kind: Kind::Switch },
    Lock { policy: "AgentTurnLimit", pref: "agent_chat_turn_limit", kind: Kind::Number { max: MAX_TURN_LIMIT } },
    Lock { policy: "AiFeatures", pref: "ai_features", kind: Kind::Switch },
    Lock { policy: "AiPermissions", pref: "ai_permissions", kind: Kind::Permission },
    Lock { policy: "AiToolbarButton", pref: "ai_toolbar_button", kind: Kind::Switch },
    Lock { policy: CHECK_FOR_UPDATES, pref: "check_updates", kind: Kind::Switch },
    Lock { policy: "ConnectAiApps", pref: "connect_ai_apps", kind: Kind::Switch },
    Lock { policy: "DefaultUnits", pref: "default_units", kind: Kind::Units },
    Lock { policy: "Language", pref: "language", kind: Kind::Language },
    Lock { policy: "ReopenDesigns", pref: "reopen_designs", kind: Kind::Switch },
    Lock { policy: "ShowDiscordButton", pref: "show_discord_button", kind: Kind::Switch },
    Lock { policy: "ShowDonateButton", pref: "show_donate_button", kind: Kind::Switch },
    Lock { policy: THEME_FILE, pref: "theme_follow", kind: Kind::ThemeFile },
];

/// The theme file everyone on this computer follows: its colours replace the chosen theme, and a
/// change to the file shows within a second. Locks `theme_follow` and the theme picker.
pub const THEME_FILE: &str = "ThemeFile";

/// A preference's value, as a policy sets it.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(untagged)]
pub enum Setting {
    Switch(bool),
    Number(u32),
    Text(String),
}

/// The organization's name, shown as "Managed by {name}". Mandatory only; it locks nothing.
pub const ORGANIZATION_NAME: &str = "OrganizationName";

/// The longest organization name shown, in characters; a longer one is cut.
pub const MAX_NAME: usize = 64;

/// The macOS preference domain of the managed values: the app's bundle identifier.
pub const MACOS_DOMAIN: &str = "io.tangencylabs.solidcraft";

/// Where macOS keeps the managed preferences a profile installs: the computer's in this folder,
/// a user's in its subfolder named after the user.
pub const MACOS_MANAGED: &str = "/Library/Managed Preferences";

/// The policy file of Linux and FreeBSD: a JSON object like [`FILE_ENV`]'s. A file that anyone
/// may write is not the administrator's and counts as no policy.
pub const SYSTEM_FILE: &str = "/etc/solidcraft/policy.json";

/// A policy file for tests and screenshots: a JSON object of the same value names, with the
/// recommended ones in a `Recommended` object. A value the registry sets wins, so the variable
/// cannot loosen an administrator's policy.
pub const FILE_ENV: &str = "SOLIDCRAFT_POLICY_FILE";

/// A policy file is a few lines.
#[cfg(not(target_arch = "wasm32"))]
const MAX_FILE: u64 = 64 << 10;

/// A value as the administrator stored it.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(u32),
    Text(String),
}

/// One level of policy: each field is `None` until the administrator sets it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layer {
    /// The valid values of [`LOCKS`], by the preference they set.
    pub settings: BTreeMap<&'static str, Setting>,
    /// [`USAGE_STATISTICS`]: 0 turns them off and locks the preference; 1 leaves the choice.
    pub usage_statistics: Option<bool>,
    /// [`ORGANIZATION_NAME`], cleaned: one line, no control or direction characters, at most
    /// [`MAX_NAME`] characters, never empty.
    pub organization: Option<String>,
}

impl Layer {
    /// The values `get` knows, by name. The value is untrusted: anything unexpected is not set.
    pub fn read(get: &dyn Fn(&str) -> Option<Value>) -> Layer {
        let text = |name: &str| match get(name)? {
            Value::Text(text) => clean_name(&text),
            other => {
                log::warn!("policy: {name} is {other:?}, expected text; ignored");
                None
            }
        };
        let settings = LOCKS
            .iter()
            .filter_map(|lock| {
                let value = get(lock.policy)?;
                let setting = parse(lock.kind, &value);
                if setting.is_none() {
                    log::warn!("policy: {} is {value:?}, not a value it takes; ignored", lock.policy);
                }
                Some((lock.pref, setting?))
            })
            .collect();
        let usage_statistics = match get(USAGE_STATISTICS) {
            None => None,
            Some(value) => {
                let on = match parse(Kind::Switch, &value) {
                    Some(Setting::Switch(on)) => Some(on),
                    _ => None,
                };
                if on.is_none() {
                    log::warn!("policy: {USAGE_STATISTICS} is {value:?}, not a value it takes; ignored");
                }
                on
            }
        };
        Layer { settings, usage_statistics, organization: text(ORGANIZATION_NAME) }
    }

    /// [`CHECK_FOR_UPDATES`]: whether update checks run.
    pub fn check_for_updates(&self) -> Option<bool> {
        match self.settings.get("check_updates") {
            Some(Setting::Switch(on)) => Some(*on),
            _ => None,
        }
    }

    /// Whether any value is set.
    fn any(&self) -> bool {
        !self.settings.is_empty() || self.usage_statistics.is_some() || self.organization.is_some()
    }

    /// `self`, with `under` for the values `self` leaves unset.
    fn or(mut self, under: Layer) -> Layer {
        for (pref, setting) in under.settings {
            self.settings.entry(pref).or_insert(setting);
        }
        Layer {
            settings: self.settings,
            usage_statistics: self.usage_statistics.or(under.usage_statistics),
            organization: self.organization.or(under.organization),
        }
    }
}

/// `value` as `kind` takes it; None when it is not one of the values `kind` takes.
fn parse(kind: Kind, value: &Value) -> Option<Setting> {
    match (kind, value) {
        (Kind::Switch, Value::Number(0)) => Some(Setting::Switch(false)),
        (Kind::Switch, Value::Number(1)) => Some(Setting::Switch(true)),
        (Kind::Units, Value::Text(text)) if text.len() <= MAX_CHOICE => {
            crate::prefs::UNITS.iter().find(|unit| unit.eq_ignore_ascii_case(text.trim())).map(|unit| Setting::Text((*unit).into()))
        }
        (Kind::Language, Value::Text(text)) if text.len() <= MAX_CHOICE => {
            let text = text.trim();
            if text.eq_ignore_ascii_case(solidcraft_ui_text::SYSTEM) {
                return Some(Setting::Text(solidcraft_ui_text::SYSTEM.into()));
            }
            crate::i18n::LOCALES.iter().find(|locale| locale.id.eq_ignore_ascii_case(text)).map(|locale| Setting::Text(locale.id.into()))
        }
        (Kind::Number { max }, Value::Number(n)) if *n <= max => Some(Setting::Number(*n)),
        (Kind::Permission, Value::Text(text)) if text.len() <= MAX_CHOICE => {
            crate::agent_chat::agents::Mode::parse(&text.trim().to_ascii_lowercase()).map(|mode| Setting::Text(mode.id().into()))
        }
        (Kind::ThemeFile, Value::Text(text)) => crate::theme_files::follow_path(text).ok().filter(|path| !path.is_empty()).map(Setting::Text),
        _ => None,
    }
}

/// The preference `pref` of `p` (None: not one of [`LOCKS`]).
fn get(p: &crate::prefs::Prefs, pref: &str) -> Option<Setting> {
    Some(match pref {
        "agent_chat_turn_limit" => Setting::Number(p.agent_chat_turn_limit),
        "agent_control" => Setting::Switch(p.agent_control),
        "ai_features" => Setting::Switch(p.ai_features),
        "ai_permissions" => Setting::Text(p.ai_permissions.id().into()),
        "ai_toolbar_button" => Setting::Switch(p.ai_toolbar_button),
        "check_updates" => Setting::Switch(p.check_updates),
        "connect_ai_apps" => Setting::Switch(p.connect_ai_apps),
        "default_units" => Setting::Text(p.default_units.clone()),
        "language" => Setting::Text(p.language.clone()),
        "reopen_designs" => Setting::Switch(p.reopen_designs),
        "show_discord_button" => Setting::Switch(p.show_discord_button),
        "show_donate_button" => Setting::Switch(p.show_donate_button),
        "theme_follow" => Setting::Text(p.theme_follow.clone()),
        _ => return None,
    })
}

/// Set the preference `pref` of `p` to `setting`; a setting of the other kind changes nothing.
fn set(p: &mut crate::prefs::Prefs, pref: &str, setting: &Setting) {
    match (pref, setting) {
        ("agent_chat_turn_limit", Setting::Number(n)) => p.agent_chat_turn_limit = *n,
        ("agent_control", Setting::Switch(on)) => p.agent_control = *on,
        ("ai_features", Setting::Switch(on)) => p.ai_features = *on,
        ("ai_permissions", Setting::Text(id)) => {
            if let Some(mode) = crate::agent_chat::agents::Mode::parse(id) {
                p.ai_permissions = mode;
            }
        }
        ("ai_toolbar_button", Setting::Switch(on)) => p.ai_toolbar_button = *on,
        ("check_updates", Setting::Switch(on)) => p.check_updates = *on,
        ("connect_ai_apps", Setting::Switch(on)) => p.connect_ai_apps = *on,
        ("default_units", Setting::Text(text)) => p.default_units.clone_from(text),
        ("language", Setting::Text(text)) => p.language.clone_from(text),
        ("reopen_designs", Setting::Switch(on)) => p.reopen_designs = *on,
        ("show_discord_button", Setting::Switch(on)) => p.show_discord_button = *on,
        ("show_donate_button", Setting::Switch(on)) => p.show_donate_button = *on,
        ("theme_follow", Setting::Text(path)) => p.theme_follow.clone_from(path),
        _ => {}
    }
}

/// What the administrator set. The default is an unmanaged computer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Policy {
    /// Enforced: the person cannot change these.
    pub mandatory: Layer,
    /// Defaults the person may change (not applied yet).
    pub recommended: Layer,
    /// The person's own value of each preference a mandatory value replaces, kept for when the
    /// policy goes: it is what is saved.
    pub own: BTreeMap<&'static str, Setting>,
}

impl Policy {
    /// The preferences (by their `ui.prefs` names) a mandatory value replaces.
    pub fn locked(&self) -> Vec<&'static str> {
        // UsageStatistics allowed (1) leaves the choice to the person; only off locks it.
        let statistics = (self.mandatory.usage_statistics == Some(false)).then_some("usage_statistics");
        let mut locked: Vec<&'static str> = self.mandatory.settings.keys().copied().chain(statistics).collect();
        locked.sort_unstable();
        locked
    }

    /// The administrator set a mandatory value: SolidCraft says it is managed.
    pub fn managed(&self) -> bool {
        self.mandatory.any()
    }

    /// The organization's name, when the administrator gave one.
    pub fn organization(&self) -> Option<&str> {
        self.mandatory.organization.as_deref()
    }

    /// "Managed by {name}", or "Managed by your organization" without a name; None when unmanaged.
    pub fn managed_by(&self) -> Option<String> {
        self.managed().then(|| match self.organization() {
            Some(name) => solidcraft_ui_text::format("policy.managed_by", &[("name", name)]),
            None => solidcraft_ui_text::tr("policy.managed").to_string(),
        })
    }

    /// A mandatory value replaces the preference `name`.
    pub fn locks(&self, name: &str) -> bool {
        self.locked().contains(&name)
    }

    /// The policy in a JSON object (see [`FILE_ENV`]); anything else is no policy.
    pub fn from_json(text: &str) -> Policy {
        let json = serde_json::from_str::<serde_json::Value>(text).unwrap_or_default();
        let layer = |object: Option<&serde_json::Value>| {
            Layer::read(&|name| match object?.get(name)? {
                serde_json::Value::Number(n) => n.as_u64().and_then(|n| u32::try_from(n).ok()).map(Value::Number),
                serde_json::Value::String(s) => Some(Value::Text(s.clone())),
                _ => None,
            })
        };
        Policy { mandatory: layer(Some(&json)), recommended: layer(json.get(RECOMMENDED)), own: BTreeMap::new() }
    }

    /// `self`, with `under` for the values `self` leaves unset.
    fn or(self, under: Policy) -> Policy {
        Policy { mandatory: self.mandatory.or(under.mandatory), recommended: self.recommended.or(under.recommended), own: self.own }
    }
}

/// `text` as one short line for the app bar: control and direction characters become spaces,
/// spaces collapse, and it is cut to [`MAX_NAME`] characters. None when nothing is left.
fn clean_name(text: &str) -> Option<String> {
    let invisible = |c: char| c.is_control() || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}');
    let spaced: String = text.chars().take(4 * MAX_NAME).map(|c| if invisible(c) { ' ' } else { c }).collect();
    let name: String = spaced.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_NAME).collect();
    let name = name.trim_end().to_string();
    (!name.is_empty()).then_some(name)
}

/// Update checks run at start and once a day: the administrator's answer, else the person's.
pub fn check_updates(app: &SolveApp) -> bool {
    app.policy.mandatory.check_for_updates().unwrap_or(app.preferences.check_updates)
}

/// Give each preference a mandatory value sets that value. A preference that differs is the
/// person's own choice (from the saved file, a reload or a shortcut such as the Donate pill's
/// Hide): it is kept in [`Policy::own`] for when the policy goes. Cheap; runs every frame.
pub fn enforce(app: &mut SolveApp) {
    enforce_keeping(app, false);
}

/// [`enforce`] after the person's preferences were loaded: every loaded value of a preference the
/// policy sets is the person's choice, the policy's own value included, so it replaces the kept one.
pub fn enforce_loaded(app: &mut SolveApp) {
    let prefs: Vec<_> = app.policy.mandatory.settings.keys().copied().collect();
    for pref in prefs {
        if let Some(loaded) = get(&app.preferences, pref) {
            app.policy.own.insert(pref, loaded);
        }
    }
    enforce_keeping(app, true);
}

/// [`enforce`] after Restore defaults: the person's kept value stays, the default is not a choice.
pub fn enforce_after_reset(app: &mut SolveApp) {
    enforce_keeping(app, true);
}

fn enforce_keeping(app: &mut SolveApp, keep_own: bool) {
    if app.policy.mandatory.settings.is_empty() {
        return;
    }
    // The organization's theme file wins over the `--theme-file` launch flag too.
    if app.policy.locks("theme_follow") {
        app.prefs_window.follow.launch = None;
    }
    let mut units = false;
    for (pref, setting) in app.policy.mandatory.settings.clone() {
        let Some(current) = get(&app.preferences, pref) else { continue };
        if current == setting {
            continue;
        }
        if !keep_own {
            app.policy.own.insert(pref, current);
        }
        set(&mut app.preferences, pref, &setting);
        units |= pref == "default_units";
    }
    if units {
        crate::prefs::default_units_changed(app);
    }
}

/// The saved preferences hold the person's own values, not the policy's (`saved` is the object
/// [`SolveApp::prefs`] writes): removing the policy brings the person's choices back.
pub fn keep_own(app: &SolveApp, saved: &mut serde_json::Value) {
    if let Some(prefs) = saved.get_mut("preferences").and_then(serde_json::Value::as_object_mut) {
        own_values(app, prefs);
    }
}

/// Put the person's own value of each preference the policy sets into `prefs` (the stored form,
/// as `ui.prefs` reports it).
pub fn own_values(app: &SolveApp, prefs: &mut serde_json::Map<String, serde_json::Value>) {
    for (pref, own) in &app.policy.own {
        if let Ok(value) = serde_json::to_value(own) {
            prefs.insert((*pref).into(), value);
        }
    }
}

/// The values in force of the preferences a mandatory value sets, by `ui.prefs` name.
pub fn in_force(app: &SolveApp) -> serde_json::Map<String, serde_json::Value> {
    app.policy.mandatory.settings.iter().filter_map(|(pref, setting)| Some(((*pref).to_string(), serde_json::to_value(setting).ok()?))).collect()
}

/// `value` for the preference `pref`, which the policy sets, is what `ui.prefs` reports for it:
/// the person's stored choice (the value in force when the two agree). Sending it back is no change.
pub fn unchanged(app: &SolveApp, pref: &str, value: &serde_json::Value) -> bool {
    let stored = app.policy.own.get(pref).cloned().or_else(|| get(&app.preferences, pref));
    stored.and_then(|stored| serde_json::to_value(stored).ok()).as_ref() == Some(value)
}

/// `add`, disabled and followed by "Set by your organization" while a mandatory value sets the
/// preference (`locked`).
pub fn row<R>(ui: &mut egui::Ui, locked: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    if !locked {
        return add(ui);
    }
    ui.horizontal_wrapped(|ui| {
        let inner = ui.add_enabled_ui(false, add).inner;
        crate::ui_kit::settings_inline_note(ui, solidcraft_ui_text::tr("policy.set_by_organization"));
        inner
    })
    .inner
}

// ---- user interface ---------------------------------------------------------------------------

/// Room the badge's full label leaves for the document tabs (about one tab and its +); with less,
/// the badge says just "Managed".
const TAB_ROOM: f32 = 140.0;

/// The application bar's "Managed by {name}" badge at the right of `span`, while a policy is set:
/// it opens Preferences › General. Its rect (None: not shown).
pub fn badge(app: &mut SolveApp, ui: &mut egui::Ui, span: egui::Rect, labeled: bool) -> Option<egui::Rect> {
    use crate::ui_kit::BackingTooltipExt as _;
    let full = app.policy.managed_by()?;
    let labels = [full.as_str(), solidcraft_ui_text::tr("policy.managed_short")];
    let (rect, response) = crate::toolbar::offer_pill(ui, span, labeled, &labels, TAB_ROOM, "briefcase", "titlebar:managed", true)?;
    let tip = format!("{full}\n{}", solidcraft_ui_text::tr("policy.managed_tip"));
    if response.backing_on_hover_text(tip).clicked() {
        crate::prefs::open_at(app, 0);
    }
    Some(rect)
}

/// A briefcase and "Managed by {name}" on one line, in the note colour, while a policy is set
/// (Preferences and About). `handle` names it for scenarios.
pub fn note(app: &SolveApp, ui: &mut egui::Ui, handle: &str) {
    let Some(text) = app.policy.managed_by() else { return };
    let t = solidcraft_theme::Tokens::get();
    let line = ui.horizontal(|ui| {
        let (icon, _) = ui.allocate_exact_size(egui::vec2(t.space_16, t.space_16), egui::Sense::hover());
        crate::icons::paint_ink(ui.painter(), icon, "briefcase", t.text_dim);
        ui.label(egui::RichText::new(text).size(t.font_label).color(t.text_dim));
    });
    crate::scenario::publish_target(handle, line.response.rect);
}

/// Anonymous usage statistics are allowed: the organization did not turn them off. The person's
/// choice still decides.
pub fn usage_statistics_allowed(app: &SolveApp) -> bool {
    app.policy.mandatory.usage_statistics != Some(false)
}

/// This computer's policy. The host reads it once at start; tests and the web app keep the default.
#[cfg(not(target_arch = "wasm32"))]
pub fn load() -> Policy {
    let file = std::env::var_os(FILE_ENV).and_then(|path| json_file(std::path::Path::new(&path)));
    system().or(file.unwrap_or_default())
}

/// The policy in the JSON file at `path`, at most [`MAX_FILE`] of it; None when it can't be read.
#[cfg(not(target_arch = "wasm32"))]
fn json_file(path: &std::path::Path) -> Option<Policy> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path).ok()?.take(MAX_FILE).read_to_string(&mut text).ok()?;
    Some(Policy::from_json(&text))
}

#[cfg(target_arch = "wasm32")]
pub fn load() -> Policy {
    Policy::default()
}

#[cfg(windows)]
fn system() -> Policy {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let layer = |path: &str| {
        // Each hive's values are checked on their own, so an invalid machine value leaves the
        // user's; among valid values the machine's wins.
        [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER]
            .into_iter()
            .filter_map(|hive| RegKey::predef(hive).open_subkey(path).ok())
            .map(|key| {
                Layer::read(&|name| {
                    key.get_value::<u32, _>(name).ok().map(Value::Number).or_else(|| key.get_value::<String, _>(name).ok().map(Value::Text))
                })
            })
            .fold(Layer::default(), Layer::or)
    };
    Policy { mandatory: layer(KEY), recommended: layer(&format!(r"{KEY}\{RECOMMENDED}")), own: BTreeMap::new() }
}

/// The managed preferences of [`MACOS_DOMAIN`]: the computer's, then the user's for the values
/// it leaves unset. A profile writes them as a property list; `plutil` (part of macOS) turns one
/// into JSON, so the app reads them without a parser of its own.
#[cfg(target_os = "macos")]
fn system() -> Policy {
    let computer = std::path::PathBuf::from(MACOS_MANAGED).join(format!("{MACOS_DOMAIN}.plist"));
    // The login name from the account database, not $USER: the environment is the person's to
    // change and must not pick another user's (looser) policy.
    let user = nix::unistd::User::from_uid(nix::unistd::getuid())
        .ok()
        .flatten()
        .map(|user| std::path::PathBuf::from(MACOS_MANAGED).join(user.name).join(format!("{MACOS_DOMAIN}.plist")));
    [Some(computer), user].into_iter().flatten().filter_map(|path| managed_plist(&path)).fold(Policy::default(), Policy::or)
}

/// One managed preferences file as a policy; None when there is none or it can't be read.
#[cfg(target_os = "macos")]
fn managed_plist(path: &std::path::Path) -> Option<Policy> {
    if !path.is_file() {
        return None;
    }
    let out = std::process::Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-", "--"])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.len() as u64 > MAX_FILE {
        log::warn!("policy: {} is not a readable property list; ignored", path.display());
        return None;
    }
    Some(Policy::from_json(&String::from_utf8_lossy(&out.stdout)))
}

/// [`SYSTEM_FILE`], unless anyone may write it.
#[cfg(all(unix, not(target_os = "macos")))]
fn system() -> Policy {
    system_file(std::path::Path::new(SYSTEM_FILE))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn system_file(path: &std::path::Path) -> Policy {
    use std::os::unix::fs::PermissionsExt as _;
    let Ok(meta) = std::fs::metadata(path) else { return Policy::default() };
    if meta.permissions().mode() & 0o002 != 0 {
        log::warn!("policy: {} may be written by anyone; ignored", path.display());
        return Policy::default();
    }
    json_file(path).unwrap_or_default()
}

#[cfg(not(any(windows, unix, target_arch = "wasm32")))]
fn system() -> Policy {
    Policy::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(values: &[(&str, Value)]) -> Layer {
        Layer::read(&|name| values.iter().find(|(n, _)| *n == name).map(|(_, v)| v.clone()))
    }

    fn switch(on: bool) -> Option<Setting> {
        Some(Setting::Switch(on))
    }

    fn text(text: &str) -> Option<Setting> {
        Some(Setting::Text(text.into()))
    }

    /// The value `policy` gives `pref` in a layer with only that value.
    fn one(policy: &str, value: Value) -> Option<Setting> {
        let pref = LOCKS.iter().find(|lock| lock.policy == policy).map(|lock| lock.pref)?;
        layer(&[(policy, value)]).settings.get(pref).cloned()
    }

    #[test]
    fn a_computer_without_policy_is_unmanaged() {
        assert_eq!(layer(&[]), Layer::default());
        assert_eq!(Policy::default().locked(), Vec::<&str>::new());
        for lock in LOCKS {
            assert!(!Policy::default().locks(lock.pref));
        }
        assert_eq!(Policy::from_json(""), Policy::default());
    }

    #[test]
    fn every_lock_names_a_preference_the_app_stores_and_can_set() {
        let mut p = crate::prefs::Prefs::default();
        let stored = serde_json::to_value(&p).unwrap_or_default();
        for lock in LOCKS {
            assert!(stored.get(lock.pref).is_some(), "{} is not a stored preference", lock.pref);
            let value = get(&p, lock.pref).unwrap_or_else(|| panic!("{} cannot be read", lock.pref));
            set(&mut p, lock.pref, &value);
            assert_eq!(get(&p, lock.pref), Some(value));
        }
        let names: Vec<_> = LOCKS.iter().map(|lock| lock.policy).collect();
        assert!(names.windows(2).all(|w| w[0] < w[1]), "LOCKS stay sorted by policy name");
    }

    #[test]
    fn each_switch_is_zero_or_one_and_anything_else_is_not_set() {
        for policy in [
            "AgentControl",
            "AiFeatures",
            "AiToolbarButton",
            CHECK_FOR_UPDATES,
            "ConnectAiApps",
            "ReopenDesigns",
            "ShowDiscordButton",
            "ShowDonateButton",
        ] {
            assert_eq!(one(policy, Value::Number(0)), switch(false), "{policy}");
            assert_eq!(one(policy, Value::Number(1)), switch(true), "{policy}");
            for hostile in [Value::Number(2), Value::Number(u32::MAX), Value::Text("0".into()), Value::Text("x".repeat(10_000))] {
                assert_eq!(one(policy, hostile), None, "{policy}");
            }
        }
        assert_eq!(layer(&[("SomethingElse", Value::Number(0))]), Layer::default());
    }

    #[test]
    fn default_units_take_one_of_the_apps_units_and_anything_else_is_not_set() {
        for unit in crate::prefs::UNITS {
            assert_eq!(one("DefaultUnits", Value::Text((*unit).into())), text(unit));
        }
        assert_eq!(one("DefaultUnits", Value::Text(" IN ".into())), text("in"), "case and spaces do not matter");
        for hostile in
            [Value::Text("inch".into()), Value::Text(String::new()), Value::Text("mm\0".into()), Value::Text("m".repeat(100_000)), Value::Number(1)]
        {
            assert_eq!(one("DefaultUnits", hostile), None);
        }
    }

    #[test]
    fn language_takes_system_or_a_language_the_app_has_and_anything_else_is_not_set() {
        assert_eq!(one("Language", Value::Text("System".into())), text(solidcraft_ui_text::SYSTEM));
        for locale in crate::i18n::LOCALES {
            assert_eq!(one("Language", Value::Text(locale.id.to_ascii_uppercase())), text(locale.id), "{}", locale.id);
        }
        for hostile in [
            Value::Text("xx".into()),
            Value::Text("de\u{202E}".into()),
            Value::Text(String::new()),
            Value::Text("d".repeat(100_000)),
            Value::Number(0),
        ] {
            assert_eq!(one("Language", hostile), None);
        }
    }

    #[test]
    fn the_turn_limit_is_zero_to_two_hundred_and_permissions_a_mode_and_anything_else_is_not_set() {
        for n in [0, 1, 24, MAX_TURN_LIMIT] {
            assert_eq!(one("AgentTurnLimit", Value::Number(n)), Some(Setting::Number(n)));
        }
        for hostile in [Value::Number(MAX_TURN_LIMIT + 1), Value::Number(u32::MAX), Value::Text("24".into())] {
            assert_eq!(one("AgentTurnLimit", hostile), None);
        }
        for (text, mode) in [("ask", "ask"), ("EDITS", "edits"), (" All ", "all")] {
            assert_eq!(one("AiPermissions", Value::Text(text.into())), self::text(mode), "{text}");
        }
        for hostile in [Value::Text("everything".into()), Value::Text(String::new()), Value::Text("a".repeat(100_000)), Value::Number(0)] {
            assert_eq!(one("AiPermissions", hostile), None);
        }
    }

    #[test]
    fn the_organizations_permissions_hold_for_every_agent_and_its_always_allowed_answers_wait() {
        use crate::agent_chat::agents::{Mode, always_allowed_in_force, mode_in_force};
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.preferences.ai_permissions = Mode::Edits;
        app.preferences.agent_permissions.insert("codex".into(), Mode::All);
        app.preferences.agent_always_allow.insert("claude".into(), vec!["file.save".into()]);
        assert_eq!(mode_in_force(&app, "codex"), Mode::All, "without a policy an agent's own mode holds");
        assert!(always_allowed_in_force(&app, "claude", "file.save"));
        app.policy = Policy::from_json(r#"{"AiPermissions": "ask"}"#);
        enforce(&mut app);
        for agent in ["codex", "claude", "other"] {
            assert_eq!(mode_in_force(&app, agent), Mode::Ask, "{agent}");
        }
        assert!(!always_allowed_in_force(&app, "claude", "file.save"), "an Always allowed answer doesn't apply");
        // Kept for when the policy goes.
        assert_eq!(app.preferences.agent_permissions.get("codex"), Some(&Mode::All));
        assert_eq!(app.preferences.agent_always_allow["claude"], ["file.save"]);
        assert_eq!(saved(&app)["ai_permissions"], serde_json::json!("edits"));
        assert_eq!(app.preferences.agent_chat_turn_limit, crate::agent_chat::gate::DEFAULT_TURN_LIMIT);
        app.policy = Policy::from_json(r#"{"AgentTurnLimit": 12, "AiFeatures": 0, "AgentControl": 0, "ConnectAiApps": 0, "AiToolbarButton": 0}"#);
        enforce(&mut app);
        let p = &app.preferences;
        assert_eq!(
            (p.agent_chat_turn_limit, p.ai_features, p.agent_control, p.connect_ai_apps, p.ai_toolbar_button),
            (12, false, false, false, false)
        );
        assert_eq!(saved(&app)["agent_chat_turn_limit"], serde_json::json!(0), "the person's own limit is saved");
    }

    #[test]
    fn a_mandatory_value_locks_its_preference_and_a_recommended_one_does_not() {
        let policy = Policy::from_json(r#"{"CheckForUpdates": 0, "ShowDonateButton": 0, "Recommended": {"CheckForUpdates": 1}}"#);
        assert_eq!(policy.mandatory.check_for_updates(), Some(false));
        assert_eq!(policy.recommended.check_for_updates(), Some(true));
        assert_eq!(policy.locked(), ["check_updates", "show_donate_button"]);
        let recommended = Policy::from_json(r#"{"Recommended": {"CheckForUpdates": 0, "Language": "de"}}"#);
        assert_eq!(recommended.recommended.check_for_updates(), Some(false));
        assert!(!recommended.locks("check_updates"));
        assert!(!recommended.locks("language"));
    }

    #[test]
    fn a_hostile_policy_file_is_no_policy() {
        for text in [
            "[]",
            "null",
            "{",
            r#"{"CheckForUpdates": -1}"#,
            r#"{"CheckForUpdates": 1.5}"#,
            r#"{"CheckForUpdates": true}"#,
            r#"{"Recommended": 7}"#,
            r#"{"DefaultUnits": ["mm"]}"#,
            r#"{"Language": {"id": "de"}}"#,
        ] {
            assert_eq!(Policy::from_json(text), Policy::default(), "{text}");
        }
    }

    #[test]
    fn the_organization_name_is_one_clean_short_line_and_anything_else_is_not_set() {
        let name = |v: Value| layer(&[(ORGANIZATION_NAME, v)]).organization;
        assert_eq!(name(Value::Text("  ACME   Corp ".into())).as_deref(), Some("ACME Corp"));
        assert_eq!(name(Value::Text("Acme\r\nCorp\t\u{202E}Ltd\u{200B}".into())).as_deref(), Some("Acme Corp Ltd"));
        assert_eq!(name(Value::Text("x".repeat(100_000))).map(|n| n.chars().count()), Some(MAX_NAME));
        assert_eq!(name(Value::Text("é".repeat(70))).map(|n| n.chars().count()), Some(MAX_NAME), "cut by characters, not bytes");
        for nothing in [Value::Text(String::new()), Value::Text(" \n\u{200B} ".into()), Value::Number(1)] {
            assert_eq!(name(nothing), None);
        }
    }

    #[test]
    fn the_theme_file_is_one_line_path_and_anything_else_is_not_set() {
        assert_eq!(one(THEME_FILE, Value::Text(r"  C:\ProgramData\ACME\theme.json ".into())), text(r"C:\ProgramData\ACME\theme.json"));
        assert_eq!(one(THEME_FILE, Value::Text("/etc/solidcraft/acme-theme.json".into())), text("/etc/solidcraft/acme-theme.json"));
        for hostile in [
            Value::Text(String::new()),
            Value::Text("  ".into()),
            Value::Text("a\nb".into()),
            Value::Text("a\u{0}b".into()),
            Value::Text("x".repeat(5000)),
            Value::Number(1),
        ] {
            assert_eq!(one(THEME_FILE, hostile), None);
        }
    }

    #[test]
    fn any_mandatory_value_makes_the_computer_managed_and_a_recommended_one_does_not() {
        crate::i18n::install();
        let _language = solidcraft_ui_text::scope("en");
        assert!(!Policy::default().managed());
        assert_eq!(Policy::default().managed_by(), None);
        let named = Policy::from_json(r#"{"OrganizationName": "ACME"}"#);
        assert!(named.managed());
        assert_eq!(named.organization(), Some("ACME"));
        assert_eq!(named.managed_by().as_deref(), Some("Managed by ACME"));
        assert!(named.locked().is_empty(), "a name locks no setting");
        let unnamed = Policy::from_json(r#"{"ReopenDesigns": 0}"#);
        assert_eq!(unnamed.managed_by().as_deref(), Some("Managed by your organization"));
        let recommended = Policy::from_json(r#"{"Recommended": {"OrganizationName": "ACME", "CheckForUpdates": 0}}"#);
        assert!(!recommended.managed());
        assert_eq!(recommended.organization(), None);
    }

    #[test]
    fn an_invalid_machine_value_leaves_the_users_valid_one() {
        let machine = layer(&[(CHECK_FOR_UPDATES, Value::Number(2)), ("DefaultUnits", Value::Text("parsec".into()))]);
        let user = layer(&[(CHECK_FOR_UPDATES, Value::Number(0)), ("DefaultUnits", Value::Text("cm".into()))]);
        let both = machine.or(user);
        assert_eq!(both.check_for_updates(), Some(false));
        assert_eq!(both.settings.get("default_units").cloned(), text("cm"));
        let machine = layer(&[(CHECK_FOR_UPDATES, Value::Number(1))]);
        assert_eq!(machine.or(layer(&[(CHECK_FOR_UPDATES, Value::Number(0))])).check_for_updates(), Some(true), "a valid machine value wins");
    }

    #[test]
    fn the_registry_wins_over_the_policy_file_value_by_value() {
        let registry = Policy::from_json(r#"{"CheckForUpdates": 0}"#);
        let file = Policy::from_json(r#"{"CheckForUpdates": 1, "Language": "de", "Recommended": {"CheckForUpdates": 1}}"#);
        let both = registry.or(file);
        assert_eq!(both.mandatory.check_for_updates(), Some(false));
        assert_eq!(both.mandatory.settings.get("language").cloned(), text("de"), "the file still sets what the registry leaves");
        assert_eq!(both.recommended.check_for_updates(), Some(true));
    }

    /// The preferences file `app` would save, as JSON.
    fn saved(app: &SolveApp) -> serde_json::Value {
        serde_json::from_str::<serde_json::Value>(&app.prefs()).unwrap_or_default()["preferences"].clone()
    }

    #[test]
    fn a_policy_sets_its_settings_keeps_each_persons_choice_and_refuses_changes() {
        use egui::accesskit::{Role, Toggled};
        use serde_json::json;
        crate::i18n::install();
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let p = &mut h.app.preferences;
        (p.language, p.default_units, p.reopen_designs, p.show_donate_button, p.show_discord_button) = ("de".into(), "mm".into(), true, true, false);
        h.app.policy =
            Policy::from_json(r#"{"Language": "en", "DefaultUnits": "IN", "ReopenDesigns": 0, "ShowDonateButton": 0, "ShowDiscordButton": 1}"#);
        h.frame();
        // In force: the organization's values, whatever the person chose.
        let p = &h.app.preferences;
        assert_eq!(
            (p.language.as_str(), p.default_units.as_str(), p.reopen_designs, p.show_donate_button, p.show_discord_button),
            ("en", "in", false, false, true)
        );
        assert_eq!(crate::links::title_bar(&h.app).map(|l| l.id).collect::<Vec<_>>(), ["discord"], "the policy hides Donate and shows Discord");
        // Saved: the person's own choices, back when the policy goes.
        let own = json!({"language": "de", "default_units": "mm", "reopen_designs": true, "show_donate_button": true, "show_discord_button": false});
        for (pref, value) in own.as_object().into_iter().flatten() {
            assert_eq!(&saved(&h.app)[pref], value, "{pref} is saved as the person chose it");
        }
        let mut after = crate::scenario::Harness::new();
        after.app.load_prefs(&h.app.prefs());
        assert_eq!(after.app.preferences.default_units, "mm", "without the policy the person's choice is back");
        assert_eq!(after.app.preferences.language, "de");
        // Loaded preferences are the person's choice, even one equal to the policy's value.
        let file = |units: &str, app: &SolveApp| {
            let mut file = serde_json::from_str::<serde_json::Value>(&app.prefs()).unwrap_or_default();
            file["preferences"]["default_units"] = json!(units);
            file.to_string()
        };
        let reload = file("in", &h.app);
        h.app.load_prefs(&reload);
        assert_eq!(saved(&h.app)["default_units"], json!("in"), "the reloaded choice replaces the kept one");
        assert_eq!(saved(&h.app)["language"], json!("de"));
        let reload = file("cm", &h.app);
        h.app.load_prefs(&reload);
        assert_eq!(h.app.preferences.default_units, "in", "the policy's value stays in force");
        assert_eq!(saved(&h.app)["default_units"], json!("cm"));
        // A load without preferences (another part of the file) keeps the person's choice.
        h.app.load_prefs(r#"{"dark":false}"#);
        assert_eq!(saved(&h.app)["default_units"], json!("cm"), "a load without preferences changes no kept choice");
        assert_eq!(h.app.preferences.default_units, "in");
        let reload = file("mm", &h.app);
        h.app.load_prefs(&reload);
        // The control channel reports the stored choices, the values in force, and refuses a change.
        let reply = h.call("ui.prefs", json!({"open": "General"}));
        assert_eq!(reply["result"]["default_units"], json!("mm"), "{reply}");
        assert_eq!(reply["result"]["policy"]["settings"]["default_units"], json!("in"), "{reply}");
        assert_eq!(
            reply["result"]["policy"]["locked"],
            json!(["default_units", "language", "reopen_designs", "show_discord_button", "show_donate_button"])
        );
        // Another value, the value in force included, is refused: only the stored choice goes back.
        for (pref, value) in
            [("default_units", json!("cm")), ("default_units", json!("in")), ("language", json!("fr")), ("show_donate_button", json!(false))]
        {
            let refused = h.call("ui.prefs", json!({ pref: value }));
            assert_eq!(refused["ok"], json!(false), "{pref}: {refused}");
            assert!(refused["error"].as_str().unwrap_or_default().contains("set by your organization's policy"), "{refused}");
        }
        assert_eq!(h.call("ui.prefs", json!({"default_units": "mm"}))["ok"], json!(true), "sending back what was read is no change");
        assert_eq!(h.app.preferences.default_units, "in");
        // Restore defaults on General keeps the organization's values and the person's choices.
        assert_eq!(h.call("ui.prefs", json!({"button": "defaults"}))["ok"], json!(true));
        assert_eq!((h.app.preferences.language.as_str(), h.app.preferences.show_donate_button), ("en", false));
        assert_eq!(saved(&h.app)["language"], json!("de"));
        assert_eq!(saved(&h.app)["show_donate_button"], json!(true));
        // Something else that changes a set preference (here the Discord pill's ×) is undone, and is
        // the person's new choice.
        h.app.preferences.show_discord_button = false;
        h.frame();
        assert!(h.app.preferences.show_discord_button);
        // Preferences shows each row greyed out with who set it.
        let _language = solidcraft_ui_text::scope("en");
        crate::prefs::reveal_preference(&mut h, "prefs:show_donate_button");
        let tree = h.output.accesskit_update.as_ref().expect("an accessibility tree");
        for (label, on) in [("Show Donate button", false), ("Show Discord button", true)] {
            let boxes: Vec<_> = tree.nodes.iter().filter(|(_, n)| n.role() == Role::CheckBox && n.label() == Some(label)).collect();
            assert_eq!(boxes.len(), 1, "{label}");
            assert!(boxes[0].1.is_disabled(), "{label} is locked");
            assert_eq!(boxes[0].1.toggled(), Some(if on { Toggled::True } else { Toggled::False }), "{label} shows the organization's value");
        }
        assert!(h.widget("Set by your organization").is_some(), "the rows say who set them");
    }

    #[test]
    fn a_theme_file_policy_is_followed_and_cannot_be_stopped_or_changed() {
        use egui::Color32;
        use serde_json::json;
        use solidcraft_theme::Tokens;
        crate::i18n::install();
        let dir = std::env::temp_dir().join(format!("solidcraft-policy-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("acme-theme.json");
        let path = file.to_string_lossy().to_string();
        let write = |panel: &str| {
            let text = format!(r##"{{"format":"solidcraft-theme/1","name":"ACME","base":"light","colors":{{"panel":"{panel}"}}}}"##);
            std::fs::write(&file, text).unwrap();
        };
        write("#203040");
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        // A file followed with --theme-file gives way to the organization's.
        let other = dir.join("own.json").to_string_lossy().to_string();
        std::fs::write(&other, r##"{"format":"solidcraft-theme/1","name":"Own","base":"dark","colors":{"panel":"#010203"}}"##).unwrap();
        crate::prefs::follow_theme_file_for_run(&mut h.app, &other).unwrap();
        h.app.policy = Policy::from_json(&json!({ "ThemeFile": path }).to_string());
        h.frames(2);
        assert!(h.app.prefs_window.follow.launch.is_none(), "the launch flag no longer applies");
        assert_eq!(h.app.preferences.theme_follow, path);
        assert_eq!(Tokens::get().panel, Color32::from_rgb(0x20, 0x30, 0x40), "the organization's colours are on");
        assert_eq!(saved(&h.app)["theme_follow"], json!(""), "the person's own choice is kept");

        // The control channel refuses to stop following or to follow another file.
        for value in [json!(""), json!(other)] {
            let refused = h.call("ui.prefs", json!({ "theme_follow": value }));
            assert_eq!(refused["ok"], json!(false), "{refused}");
            assert!(refused["error"].as_str().unwrap_or_default().contains("set by your organization's policy"), "{refused}");
        }
        let read = h.call("ui.prefs", json!({}));
        assert_eq!(h.call("ui.prefs", json!({ "theme_follow": read["result"]["theme_follow"] }))["ok"], json!(true), "sending back what was read is no change");
        // Picking a theme records the person's choice and keeps following the file.
        let picked = h.call("ui.prefs", json!({"theme": "dark"}));
        assert_eq!(picked["result"]["theme_followed"].as_str(), Some(path.as_str()), "{picked}");
        h.frames(2);
        assert_eq!(Tokens::get().panel, Color32::from_rgb(0x20, 0x30, 0x40));

        // The organization replaces the file: shown without a restart.
        write("#506070");
        h.frames(70);
        assert_eq!(Tokens::get().panel, Color32::from_rgb(0x50, 0x60, 0x70));
        // A missing file keeps the last good theme and says why.
        std::fs::remove_file(&file).unwrap();
        h.frames(70);
        assert_eq!(Tokens::get().panel, Color32::from_rgb(0x50, 0x60, 0x70));
        let out = h.call("ui.prefs", json!({}));
        assert!(out["result"]["theme_follow_error"].is_string(), "{out}");
        assert!(out["result"]["policy"]["locked"].as_array().is_some_and(|l| l.contains(&json!("theme_follow"))), "{out}");

        // Preferences shows the row greyed out with who set it.
        let _language = solidcraft_ui_text::scope("en");
        h.call("ui.prefs", json!({"open": "Appearance"}));
        h.frames(4);
        crate::prefs::reveal_preference(&mut h, "prefs:theme_follow_toggle");
        assert!(h.widget("Set by your organization").is_some(), "the row says who set it");
        let tree = h.output.accesskit_update.as_ref().expect("an accessibility tree");
        let boxes: Vec<_> = tree
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == egui::accesskit::Role::CheckBox && n.label() == Some("Follow a theme file"))
            .collect();
        assert_eq!(boxes.len(), 1);
        assert!(boxes[0].1.is_disabled(), "the follow row is locked");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_a_policy_nothing_is_enforced_or_kept() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.default_units = "ft".into();
        h.app.preferences.show_donate_button = false;
        h.frame();
        assert_eq!(h.app.preferences.default_units, "ft");
        assert!(h.app.policy.own.is_empty());
        assert_eq!(saved(&h.app)["show_donate_button"], serde_json::json!(false));
        assert!(h.call("ui.prefs", serde_json::json!({"default_units": "cm"}))["ok"].as_bool().unwrap_or_default());
    }

    #[test]
    fn the_templates_offer_every_policy_the_app_reads() {
        let admx = include_str!("../../../packaging/windows/policies/solidcraft.admx");
        let adml = include_str!("../../../packaging/windows/policies/en-US/solidcraft.adml");
        for name in LOCKS.iter().map(|lock| lock.policy).chain([ORGANIZATION_NAME, USAGE_STATISTICS]) {
            assert!(admx.contains(&format!(r#"valueName="{name}""#)), "{name} is missing from solidcraft.admx");
            assert!(adml.contains(&format!(r#"<string id="{name}">"#)), "{name} is missing from solidcraft.adml");
        }
        for unit in crate::prefs::UNITS {
            assert!(admx.contains(&format!("<string>{unit}</string>")), "DefaultUnits offers no {unit}");
        }
    }

    #[test]
    fn usage_statistics_off_locks_the_preference_and_allowed_leaves_the_choice() {
        assert_eq!(layer(&[(USAGE_STATISTICS, Value::Number(0))]).usage_statistics, Some(false));
        assert_eq!(layer(&[(USAGE_STATISTICS, Value::Number(1))]).usage_statistics, Some(true));
        for hostile in [Value::Number(2), Value::Text("0".into())] {
            assert_eq!(layer(&[(USAGE_STATISTICS, hostile)]).usage_statistics, None);
        }
        let off = Policy::from_json(r#"{"UsageStatistics": 0}"#);
        assert!(off.locks("usage_statistics"));
        assert!(off.managed());
        let allowed = Policy::from_json(r#"{"UsageStatistics": 1}"#);
        assert!(!allowed.locks("usage_statistics"), "allowed leaves the person's choice");
        assert!(allowed.managed());
        let both = Policy::from_json(r#"{"UsageStatistics": 0, "CheckForUpdates": 1}"#);
        assert_eq!(both.locked(), ["check_updates", "usage_statistics"]);
        assert!(!Policy::from_json(r#"{"Recommended": {"UsageStatistics": 0}}"#).locks("usage_statistics"));
    }

    #[test]
    fn a_policy_file_anyone_may_write_is_no_policy() {
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let dir = std::env::temp_dir().join(format!("solidcraft-policy-file-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("policy.json");
            std::fs::write(&path, r#"{"UsageStatistics": 0}"#).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(system_file(&path).locks("usage_statistics"));
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
            assert_eq!(system_file(&path), Policy::default());
            assert_eq!(system_file(&dir.join("missing.json")), Policy::default());
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn the_shipped_templates_name_the_values_the_app_reads() {
        let linux = Policy::from_json(include_str!("../../../packaging/linux/policies/policy.json"));
        assert_eq!(linux.locked(), ["check_updates", "usage_statistics"]);
        assert_eq!(linux.organization(), Some("Example Corp"));
        let admx = include_str!("../../../packaging/windows/policies/solidcraft.admx");
        let mobileconfig = include_str!("../../../packaging/macos/policies/solidcraft.mobileconfig");
        for name in [CHECK_FOR_UPDATES, USAGE_STATISTICS, ORGANIZATION_NAME] {
            assert!(admx.contains(&format!("valueName=\"{name}\"")), "{name} in the ADMX");
            assert!(mobileconfig.contains(&format!("<key>{name}</key>")), "{name} in the profile");
        }
        assert!(mobileconfig.contains(&format!("<string>{MACOS_DOMAIN}</string>")));
    }
}
