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

/// The registry key of the mandatory values, under either hive.
pub const KEY: &str = r"Software\Policies\SolidCraft";

/// The subkey of [`KEY`] with the recommended values.
pub const RECOMMENDED: &str = "Recommended";

/// 0: no update checks. 1: checks at start and once a day. Locks `check_updates`.
pub const CHECK_FOR_UPDATES: &str = "CheckForUpdates";

/// 0: no anonymous usage statistics, and the person cannot turn them on. 1: allowed, the person
/// chooses. Locks `usage_statistics` when 0.
pub const USAGE_STATISTICS: &str = "UsageStatistics";

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
    /// [`CHECK_FOR_UPDATES`].
    pub check_for_updates: Option<bool>,
    /// [`USAGE_STATISTICS`].
    pub usage_statistics: Option<bool>,
    /// [`ORGANIZATION_NAME`], cleaned: one line, no control or direction characters, at most
    /// [`MAX_NAME`] characters, never empty.
    pub organization: Option<String>,
}

impl Layer {
    /// The values `get` knows, by name. The value is untrusted: anything unexpected is not set.
    pub fn read(get: &dyn Fn(&str) -> Option<Value>) -> Layer {
        let switch = |name: &str| match get(name)? {
            Value::Number(0) => Some(false),
            Value::Number(1) => Some(true),
            other => {
                log::warn!("policy: {name} is {other:?}, expected 0 or 1; ignored");
                None
            }
        };
        let text = |name: &str| match get(name)? {
            Value::Text(text) => clean_name(&text),
            other => {
                log::warn!("policy: {name} is {other:?}, expected text; ignored");
                None
            }
        };
        Layer { check_for_updates: switch(CHECK_FOR_UPDATES), usage_statistics: switch(USAGE_STATISTICS), organization: text(ORGANIZATION_NAME) }
    }

    /// Whether any value is set.
    fn any(&self) -> bool {
        self.check_for_updates.is_some() || self.usage_statistics.is_some() || self.organization.is_some()
    }

    /// `self`, with `under` for the values `self` leaves unset.
    fn or(self, under: Layer) -> Layer {
        Layer {
            check_for_updates: self.check_for_updates.or(under.check_for_updates),
            usage_statistics: self.usage_statistics.or(under.usage_statistics),
            organization: self.organization.or(under.organization),
        }
    }
}

/// What the administrator set. The default is an unmanaged computer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Policy {
    /// Enforced: the person cannot change these.
    pub mandatory: Layer,
    /// Defaults the person may change (not applied yet).
    pub recommended: Layer,
}

impl Policy {
    /// The preferences (by their `ui.prefs` names) a mandatory value replaces.
    pub fn locked(&self) -> Vec<&'static str> {
        let updates = self.mandatory.check_for_updates.map(|_| "check_updates");
        // Allowed (1) leaves the choice to the person; only off locks it.
        let statistics = (self.mandatory.usage_statistics == Some(false)).then_some("usage_statistics");
        updates.into_iter().chain(statistics).collect()
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
        Policy { mandatory: layer(Some(&json)), recommended: layer(json.get(RECOMMENDED)) }
    }

    /// `self`, with `under` for the values `self` leaves unset.
    fn or(self, under: Policy) -> Policy {
        Policy { mandatory: self.mandatory.or(under.mandatory), recommended: self.recommended.or(under.recommended) }
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
    app.policy.mandatory.check_for_updates.unwrap_or(app.preferences.check_updates)
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
    Policy { mandatory: layer(KEY), recommended: layer(&format!(r"{KEY}\{RECOMMENDED}")) }
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

    #[test]
    fn a_computer_without_policy_is_unmanaged() {
        assert_eq!(layer(&[]), Layer::default());
        assert_eq!(Policy::default().locked(), Vec::<&str>::new());
        assert!(!Policy::default().locks("check_updates"));
        assert_eq!(Policy::from_json(""), Policy::default());
    }

    #[test]
    fn check_for_updates_is_zero_or_one_and_anything_else_is_not_set() {
        assert_eq!(layer(&[(CHECK_FOR_UPDATES, Value::Number(0))]).check_for_updates, Some(false));
        assert_eq!(layer(&[(CHECK_FOR_UPDATES, Value::Number(1))]).check_for_updates, Some(true));
        for hostile in [Value::Number(2), Value::Number(u32::MAX), Value::Text("0".into()), Value::Text("x".repeat(10_000))] {
            assert_eq!(layer(&[(CHECK_FOR_UPDATES, hostile)]).check_for_updates, None);
        }
        assert_eq!(layer(&[("SomethingElse", Value::Number(0))]), Layer::default());
    }

    #[test]
    fn a_mandatory_value_locks_its_preference_and_a_recommended_one_does_not() {
        let policy = Policy::from_json(r#"{"CheckForUpdates": 0, "Recommended": {"CheckForUpdates": 1}}"#);
        assert_eq!(policy.mandatory.check_for_updates, Some(false));
        assert_eq!(policy.recommended.check_for_updates, Some(true));
        assert_eq!(policy.locked(), ["check_updates"]);
        let recommended = Policy::from_json(r#"{"Recommended": {"CheckForUpdates": 0}}"#);
        assert_eq!(recommended.recommended.check_for_updates, Some(false));
        assert!(!recommended.locks("check_updates"));
    }

    #[test]
    fn a_hostile_policy_file_is_no_policy() {
        for text in
            ["[]", "null", "{", r#"{"CheckForUpdates": -1}"#, r#"{"CheckForUpdates": 1.5}"#, r#"{"CheckForUpdates": true}"#, r#"{"Recommended": 7}"#]
        {
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
        let unnamed = Policy::from_json(r#"{"CheckForUpdates": 0}"#);
        assert_eq!(unnamed.managed_by().as_deref(), Some("Managed by your organization"));
        let recommended = Policy::from_json(r#"{"Recommended": {"OrganizationName": "ACME", "CheckForUpdates": 0}}"#);
        assert!(!recommended.managed());
        assert_eq!(recommended.organization(), None);
    }

    #[test]
    fn an_invalid_machine_value_leaves_the_users_valid_one() {
        let machine = layer(&[(CHECK_FOR_UPDATES, Value::Number(2))]);
        let user = layer(&[(CHECK_FOR_UPDATES, Value::Number(0))]);
        assert_eq!(machine.or(user).check_for_updates, Some(false));
        let machine = layer(&[(CHECK_FOR_UPDATES, Value::Number(1))]);
        assert_eq!(machine.or(layer(&[(CHECK_FOR_UPDATES, Value::Number(0))])).check_for_updates, Some(true), "a valid machine value wins");
    }

    #[test]
    fn the_registry_wins_over_the_policy_file_value_by_value() {
        let registry = Policy { mandatory: Layer { check_for_updates: Some(false), ..Layer::default() }, recommended: Layer::default() };
        let file = Policy::from_json(r#"{"CheckForUpdates": 1, "Recommended": {"CheckForUpdates": 1}}"#);
        let both = registry.or(file);
        assert_eq!(both.mandatory.check_for_updates, Some(false));
        assert_eq!(both.recommended.check_for_updates, Some(true));
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
