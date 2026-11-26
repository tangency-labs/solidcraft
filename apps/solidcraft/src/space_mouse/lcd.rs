//! The SpaceMouse Enterprise's display through smed (its API 2): what keys 1–12 do, as one
//! environment of labels and icons, shown while SolidCraft's window has the focus. Display only:
//! the keys still run their jobs from smed's `key` events, so the `command` callbacks smed also
//! sends are ignored. smed draws the page; SolidCraft sends names, labels and SVG icons.

use smed_client::{App, ClientMessage, Command, Environment};
use solidcraft_ui_egui::space_mouse::ButtonJob;
use std::collections::BTreeMap;

/// The one environment's id. Its name is the Buttons section's, translated.
const ENVIRONMENT: &str = "buttons";
/// SolidCraft's icon in the display's header, by the id it is sent under.
const APP_ICON: &str = "solidcraft";
const APP_SVG: &str = include_str!("../../../../assets/branding/logo/solidcraft-mark.svg");

/// What the display shows: plain data, compared every frame, with icons by name only (their SVG is
/// looked up when the page is sent).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Page {
    /// The environment's name, in the header next to "SolidCraft".
    pub name: String,
    /// The keys 1–12 that have a job, in order.
    pub tiles: Vec<Tile>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    /// The key printed 1–12 (not its button number: key 1 is 3Dconnexion's button 13).
    pub key: u8,
    /// The job's command id for smed, the same for the same job on two keys.
    pub command: String,
    pub label: String,
    /// An icon of SolidCraft's set.
    pub icon: Option<&'static str>,
}

/// The page for `device`'s jobs on a short press of keys 1–12.
pub fn page(app: &solidcraft_ui_egui::SolveApp, device: &str) -> Page {
    page_of(
        &solidcraft_ui_egui::space_mouse::button_jobs(app, device),
        |job| solidcraft_ui_egui::prefs::button_job_label(app, false, Some(job)),
        solidcraft_ui_text::tr("ui.3d_mouse_buttons").to_string(),
    )
}

/// The page for `jobs` (by button number), each named by `label`.
fn page_of(jobs: &BTreeMap<u8, ButtonJob>, label: impl Fn(&ButtonJob) -> String, name: String) -> Page {
    let tiles = (1..=12)
        .filter_map(|key: u8| {
            let job = jobs.get(&smed_client::key_id(&key.to_string())?)?;
            Some(Tile { key, command: command_id(job), label: label(job), icon: job_icon(job) })
        })
        .collect();
    Page { name, tiles }
}

/// The messages that put `page` on the display: icons first (smed needs them before the commands
/// that use them), then the catalogue, the environment and its choice. smed forgets all of it when
/// the connection ends, so a new connection gets the whole page again.
pub fn messages(page: &Page) -> Vec<ClientMessage> {
    let mut messages = vec![ClientMessage::SetIcon { id: APP_ICON.into(), svg: Some(APP_SVG.into()), png: None }];
    let mut sent: Vec<&str> = Vec::new();
    let mut commands: Vec<Command> = Vec::new();
    let mut environment = Environment::new(ENVIRONMENT, page.name.clone());
    for tile in &page.tiles {
        let mut command = Command::new(tile.command.clone(), tile.label.clone());
        if let Some(name) = tile.icon
            && let Some(svg) = icon_svg(name)
        {
            if !sent.contains(&name) {
                sent.push(name);
                messages.push(ClientMessage::SetIcon { id: name.into(), svg: Some(svg.into()), png: None });
            }
            command = command.with_icon(name);
        }
        if !commands.iter().any(|known| known.id == command.id) {
            commands.push(command);
        }
        environment = environment.with_key(tile.key.to_string(), tile.command.clone());
    }
    messages.push(ClientMessage::SetCommands { app: App::new("SolidCraft").with_icon(APP_ICON), commands });
    messages.push(ClientMessage::SetEnvironments { environments: vec![environment] });
    messages.push(ClientMessage::SetEnvironment { id: ENVIRONMENT.into() });
    messages
}

/// A job's command id for smed: one id per distinct job.
fn command_id(job: &ButtonJob) -> String {
    match job {
        ButtonJob::Settings => "settings".into(),
        ButtonJob::View(view) => format!("view.{view}"),
        ButtonJob::Key(key) => format!("key.{key:?}").to_lowercase(),
        ButtonJob::Command(id) => id.clone(),
    }
}

/// The icon a job shows: a command's own, the 3D mouse settings', or for Fit and Home the
/// toolbar's. Other views and keys show their label only.
fn job_icon(job: &ButtonJob) -> Option<&'static str> {
    match job {
        ButtonJob::Command(id) => solidcraft_engine::find_command(id).map(|command| command.icon).filter(|icon| !icon.is_empty()),
        ButtonJob::Settings => Some("prefs_mouse3d"),
        ButtonJob::View(view) if view == "fit" => Some("fit"),
        ButtonJob::View(view) if view == "home" => Some("home"),
        ButtonJob::View(_) | ButtonJob::Key(_) => None,
    }
}

/// The SVG master of icon `name`, the small variant when there is one (the display draws icons at
/// about 24 px); none for a name the set only has a placeholder for.
fn icon_svg(name: &str) -> Option<&'static str> {
    let resolved = solidcraft_look::icons::SET.resolve(name);
    if resolved.placeholder {
        return None;
    }
    let mut found = None;
    for (master, family, svg) in solidcraft_look::icons::DATA.masters {
        if *master != resolved.master {
            continue;
        }
        if *family == "small" {
            return Some(svg);
        }
        found = Some(*svg);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_ui_egui::space_mouse::ButtonKey;

    fn label(job: &ButtonJob) -> String {
        format!("{job:?}")
    }

    /// Jobs by the key printed on the device ("1", "menu"), as button numbers.
    fn jobs(list: &[(&str, ButtonJob)]) -> BTreeMap<u8, ButtonJob> {
        list.iter().map(|(key, job)| (smed_client::key_id(key).unwrap(), job.clone())).collect()
    }

    #[test]
    fn a_page_has_the_keys_1_to_12_with_a_job_in_order() {
        let page = page_of(
            &jobs(&[
                ("menu", ButtonJob::View("top".into())),
                ("2", ButtonJob::Key(ButtonKey::Esc)),
                ("1", ButtonJob::Settings),
                ("12", ButtonJob::View("fit".into())),
                ("11", ButtonJob::View("home".into())),
                ("10", ButtonJob::View("front".into())),
                ("t", ButtonJob::View("bottom".into())),
            ]),
            label,
            "Buttons".into(),
        );
        let keys: Vec<u8> = page.tiles.iter().map(|tile| tile.key).collect();
        assert_eq!(keys, [1, 2, 10, 11, 12], "the Menu and T keys have no tile, 3 to 9 have no job");
        assert_eq!(page.name, "Buttons");
        assert_eq!(page.tiles[1].command, "key.esc");
        assert_eq!(page.tiles[1].label, "Key(Esc)");
        assert_eq!(page.tiles[1].icon, None, "keys show their label only");
        assert_eq!(page.tiles[0].icon, Some("prefs_mouse3d"));
        let icons: Vec<Option<&str>> = page.tiles.iter().map(|tile| tile.icon).collect();
        assert_eq!(icons, [Some("prefs_mouse3d"), None, None, Some("home"), Some("fit")], "views other than Fit and Home have none");
    }

    #[test]
    fn a_command_shows_its_own_icon() {
        let command = solidcraft_engine::find_command("sketch.line").expect("the line command");
        let page = page_of(&jobs(&[("5", ButtonJob::Command(command.id.into()))]), label, String::new());
        assert_eq!(page.tiles[0].command, "sketch.line");
        assert_eq!(page.tiles[0].icon, Some(command.icon).filter(|icon| !icon.is_empty()));
    }

    #[test]
    fn the_messages_send_each_icon_and_command_once_before_the_environment() {
        let page = page_of(
            &jobs(&[("1", ButtonJob::Settings), ("2", ButtonJob::Settings), ("3", ButtonJob::View("front".into()))]),
            label,
            "Buttons".into(),
        );
        let lines: Vec<String> = messages(&page).iter().map(smed_client::to_line).collect();
        let types: Vec<&str> = lines.iter().map(|line| line.split('"').nth(3).unwrap_or_default()).collect();
        assert_eq!(types, ["set_icon", "set_icon", "set_commands", "set_environments", "set_environment"]);
        assert!(lines[0].contains(r#""id":"solidcraft""#) && lines[0].contains("<svg"), "{}", lines[0]);
        assert!(lines[1].contains(r#""id":"prefs_mouse3d""#) && lines[1].contains("<svg"), "{}", lines[1]);
        assert_eq!(
            lines[2].trim_end(),
            r#"{"type":"set_commands","app":{"name":"SolidCraft","icon":"solidcraft"},"commands":[{"id":"settings","label":"Settings","icon":"prefs_mouse3d"},{"id":"view.front","label":"View(\"front\")"}]}"#
        );
        assert_eq!(
            lines[3].trim_end(),
            r#"{"type":"set_environments","environments":[{"id":"buttons","name":"Buttons","keys":{"1":"settings","2":"settings","3":"view.front"}}]}"#
        );
        assert_eq!(lines[4].trim_end(), r#"{"type":"set_environment","id":"buttons"}"#);
    }

    #[test]
    fn every_icon_a_tile_names_has_an_svg_without_text_or_pictures() {
        for name in ["prefs_mouse3d", "fit", "home"] {
            let svg = icon_svg(name).unwrap_or_else(|| panic!("no SVG for {name}"));
            assert!(svg.contains("<svg") && !svg.contains("<text") && !svg.contains("<image"), "{name}");
        }
        assert!(APP_SVG.contains("<svg") && !APP_SVG.contains("<text") && !APP_SVG.contains("<image"));
        assert_eq!(icon_svg("no such icon"), None);
        // The display draws icons small: the small master where the set has one, also when a larger
        // one comes after it.
        let masters = solidcraft_look::icons::DATA.masters;
        let (name, small) = masters
            .iter()
            .enumerate()
            .find_map(|(at, (master, family, svg))| {
                let later = masters.get(at + 1..)?.iter().any(|(other, family, _)| other == master && *family != "small");
                (*family == "small" && later).then_some((*master, *svg))
            })
            .expect("an icon with a small master and a larger one after it");
        assert_eq!(icon_svg(name), Some(small), "{name}: the small master");
    }
}
