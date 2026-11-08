//! The start page as a component: a rail of entries beside a titled body (action tiles, the
//! recent files and the samples, or a section of the host's tiles or reviews). The host gives
//! every label, icon, thumbnail and what each entry stands for (`A`); [`show`] draws the page and
//! returns the entry chosen this frame, which the host runs. Nothing here knows the app.
//!
//! Icons are names in the icon set ([`crate::icons`]); thumbnails are textures the host made.

use std::borrow::Cow;

use egui::vec2;

use crate::ui_kit::{self, FileView, RecoveryMessage};
use solidcraft_theme::Tokens;

/// Text the host gives: borrowed when it has it, owned when it formatted it this frame.
pub type Label<'a> = Cow<'a, str>;

/// The whole page. `id` names it (`{id}:page`, `{id}:back`, `{id}:recent:N`, `{id}:sample:N`…).
pub struct Page<'a, A> {
    pub id: &'a str,
    /// The narrow layout's menu title, standing for the rail.
    pub name: Label<'a>,
    /// Whether the page takes input (not while a file picker or a load is up).
    pub interactive: bool,
    pub back: Back<'a, A>,
    pub rail: Vec<RailItem<'a, A>>,
    pub title: Label<'a>,
    pub hint: Label<'a>,
    /// An error shown under the hint.
    pub error: Option<Label<'a>>,
    pub body: Body<'a, A>,
}

/// The rail's first row, back to the open document.
pub struct Back<'a, A> {
    /// The label, with `{name}` for the document's name.
    pub template: Label<'a>,
    pub document: Label<'a>,
    pub shortcut: Label<'a>,
    pub chosen: A,
}

pub enum RailItem<'a, A> {
    Entry(RailEntry<'a, A>),
    Separator,
}

pub struct RailEntry<'a, A> {
    pub id: Label<'a>,
    pub label: Label<'a>,
    /// The key hint drawn at the row's end (empty for none).
    pub shortcut: Label<'a>,
    pub selected: bool,
    pub enabled: bool,
    pub chosen: A,
}

pub enum Body<'a, A> {
    Start(Start<'a, A>),
    Tiles(Tiles<'a, A>),
    Reviews(Reviews<'a, A>),
}

/// The start section: an optional recovery banner, the action tiles, the recent files and the
/// samples.
pub struct Start<'a, A> {
    pub banner: Option<Banner<'a, A>>,
    pub actions_title: Label<'a>,
    pub actions: Tiles<'a, A>,
    pub recent_title: Label<'a>,
    /// Shown when there are no recent files.
    pub recent_empty: Label<'a>,
    pub recent: Vec<Recent<'a, A>>,
    pub samples_title: Label<'a>,
    pub samples: Vec<Sample<'a, A>>,
}

/// A grid of action tiles; `grid` keeps its widgets' state apart from other grids.
pub struct Tiles<'a, A> {
    pub grid: &'a str,
    pub tiles: Vec<Tile<'a, A>>,
}

pub struct Tile<'a, A> {
    pub id: Label<'a>,
    pub icon: &'a str,
    pub title: Label<'a>,
    pub hint: Label<'a>,
    /// Drawn as the page's main action.
    pub primary: bool,
    pub chosen: A,
}

pub struct Banner<'a, A> {
    pub title: Label<'a>,
    pub before_filename: Label<'a>,
    pub filename: Label<'a>,
    pub after_filename: Label<'a>,
    pub recover: (Label<'a>, A),
    pub discard: Option<(Label<'a>, A)>,
}

pub struct Recent<'a, A> {
    pub title: Label<'a>,
    pub path: Label<'a>,
    pub when: Label<'a>,
    pub kind: Label<'a>,
    pub thumbnail: Option<(egui::TextureId, egui::Vec2)>,
    /// No picture could be made: a dimmed placeholder rather than a loading one.
    pub thumbnail_failed: bool,
    pub chosen: A,
    /// The row's context menu.
    pub menu: Vec<MenuEntry<'a, A>>,
}

pub struct MenuEntry<'a, A> {
    pub label: Label<'a>,
    pub icon: &'a str,
    pub chosen: A,
}

pub struct Sample<'a, A> {
    pub title: Label<'a>,
    pub description: Label<'a>,
    pub icon: &'a str,
    pub thumbnail: Option<(egui::TextureId, egui::Vec2)>,
    pub chosen: A,
}

/// A list of entries to review (the recoverable designs), each with a keep and a drop action.
pub struct Reviews<'a, A> {
    /// Shown when the list is empty.
    pub empty: Label<'a>,
    pub items: Vec<Review<'a, A>>,
}

pub struct Review<'a, A> {
    pub title: Label<'a>,
    pub detail: Label<'a>,
    pub recover: (Label<'a>, A),
    pub discard: (Label<'a>, A),
}

/// Draw the page; the entry chosen this frame, if any.
pub fn show<A>(ui: &mut egui::Ui, page: Page<'_, A>) -> Option<A> {
    let t = Tokens::get();
    let Page { id, name, interactive, back, rail, title, hint, error, body } = page;
    let mut chosen = None;
    let mut rail = Some((back, rail));
    let mut body = Some(body);
    FileView::page(ui, |ui| {
        ui.add_enabled_ui(interactive, |ui| {
            let full = ui.available_rect_before_wrap();
            solidcraft_ui_registry::publish_rect(&format!("{id}:page"), full);
            let narrow = full.width() < t.file_nav_width + FileView::tile_min_width() + t.file_content_inset_x * 2.0;
            let navigation_id = format!("{id}_navigation");
            if narrow {
                FileView::navigation(ui, |ui| {
                    ui_kit::dropdown_menu(ui, navigation_id.as_str(), &name, |ui| {
                        ui.set_min_width(t.file_nav_width);
                        FileView::navigation(ui, |ui| {
                            if let Some((back, rail)) = rail.take() {
                                navigation(ui, id, back, rail, &mut chosen);
                            }
                            if chosen.is_some() {
                                ui.close();
                            }
                        });
                    });
                });
                if let Some(body) = body.take() {
                    content(ui, id, (title.as_ref(), hint.as_ref(), error.as_deref()), body, true, &mut chosen);
                }
            } else {
                let rail_rect = egui::Rect::from_min_size(full.min, vec2(t.file_nav_width, full.height()));
                FileView::rail(ui, rail_rect, |ui| {
                    egui::ScrollArea::vertical().id_salt(navigation_id.as_str()).auto_shrink([false, false]).show(ui, |ui| {
                        if let Some((back, rail)) = rail.take() {
                            navigation(ui, id, back, rail, &mut chosen);
                        }
                    });
                });
                let body_rect = egui::Rect::from_min_max(egui::pos2(rail_rect.right(), full.top()), full.max);
                ui.scope_builder(egui::UiBuilder::new().max_rect(body_rect), |ui| {
                    if let Some(body) = body.take() {
                        content(ui, id, (title.as_ref(), hint.as_ref(), error.as_deref()), body, false, &mut chosen);
                    }
                });
            }
        });
    });
    chosen
}

fn navigation<A>(ui: &mut egui::Ui, id: &str, back: Back<'_, A>, rail: Vec<RailItem<'_, A>>, chosen: &mut Option<A>) {
    let back_id = format!("{id}:back");
    let response = FileView::back_to(ui, &back_id, &back.template, &back.document, &back.shortcut);
    publish(&back_id, &response);
    if response.clicked() {
        *chosen = Some(back.chosen);
    }
    for item in rail {
        match item {
            RailItem::Separator => FileView::separator(ui),
            RailItem::Entry(entry) => {
                let response = FileView::nav_row(ui, &entry.id, &entry.label, &entry.shortcut, entry.selected, entry.enabled);
                publish(&entry.id, &response);
                if response.clicked() {
                    *chosen = Some(entry.chosen);
                }
            }
        }
    }
}

fn publish(id: &str, response: &egui::Response) {
    solidcraft_ui_registry::publish_handle(id, response.rect.center());
    solidcraft_ui_registry::publish_rect(id, response.rect);
}

fn content<A>(ui: &mut egui::Ui, id: &str, heading: (&str, &str, Option<&str>), body: Body<'_, A>, narrow: bool, chosen: &mut Option<A>) {
    let t = Tokens::get();
    let (title, hint, error) = heading;
    let mut body = Some(body);
    FileView::content(ui, narrow, |ui| {
        egui::ScrollArea::vertical().id_salt(format!("{id}_content")).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            FileView::title(ui, title);
            ui.add_space(t.space_6);
            FileView::subtitle(ui, hint);
            ui.add_space(t.file_section_gap);
            if let Some(message) = error {
                ui_kit::text(ui, message, ui_kit::Text::Error(&format!("{id}_status")));
            }
            match body.take() {
                Some(Body::Start(start)) => start_content(ui, id, start, chosen),
                Some(Body::Tiles(tiles)) => tile_grid(ui, tiles, chosen),
                Some(Body::Reviews(reviews)) => review_list(ui, id, reviews, chosen),
                None => {}
            }
        });
    });
}

fn tile_grid<A>(ui: &mut egui::Ui, tiles: Tiles<'_, A>, chosen: &mut Option<A>) {
    let Tiles { grid, tiles } = tiles;
    let mut tiles: Vec<Option<Tile<'_, A>>> = tiles.into_iter().map(Some).collect();
    FileView::grid(ui, grid, FileView::tile_min_width(), tiles.len(), |ui, index, width| {
        let Some(tile) = tiles.get_mut(index).and_then(Option::take) else { return };
        let response = FileView::action_tile(ui, &tile.id, width, tile.icon, &tile.title, &tile.hint, tile.primary);
        publish(&tile.id, &response);
        if response.clicked() {
            *chosen = Some(tile.chosen);
        }
    });
}

fn start_content<A>(ui: &mut egui::Ui, id: &str, start: Start<'_, A>, chosen: &mut Option<A>) {
    let t = Tokens::get();
    let Start { banner, actions_title, actions, recent_title, recent_empty, recent, samples_title, samples } = start;
    if let Some(banner) = banner {
        let banner_id = format!("{id}:recovery");
        let message = RecoveryMessage {
            title: &banner.title,
            before_filename: &banner.before_filename,
            filename: &banner.filename,
            after_filename: &banner.after_filename,
        };
        let (recover_label, recover_chosen) = banner.recover;
        let (discard_label, discard_chosen) = banner.discard.map_or((None, None), |(label, chosen)| (Some(label), Some(chosen)));
        let (recover, discard) = FileView::recovery_banner(ui, &banner_id, message, &recover_label, discard_label.as_deref());
        publish(&format!("{banner_id}:recover"), &recover);
        if recover.clicked() {
            *chosen = Some(recover_chosen);
        }
        if let (Some(discard), Some(discard_chosen)) = (discard, discard_chosen) {
            publish(&format!("{banner_id}:discard"), &discard);
            if discard.clicked() {
                *chosen = Some(discard_chosen);
            }
        }
        ui.add_space(t.file_section_gap);
    }
    FileView::section(ui, &actions_title, t.space_12, |ui| tile_grid(ui, actions, chosen));
    ui.add_space(t.file_section_gap);
    FileView::section(ui, &recent_title, t.space_10, |ui| {
        if recent.is_empty() {
            FileView::detail(ui, &recent_empty);
            return;
        }
        FileView::recent_list(ui, |ui| {
            for (index, row) in recent.into_iter().enumerate() {
                let row_id = format!("{id}:recent:{index}");
                let response = FileView::recent_row(
                    ui,
                    &row_id,
                    ui_kit::RecentRow {
                        title: &row.title,
                        path: &row.path,
                        when: &row.when,
                        workspace: &row.kind,
                        thumbnail: row.thumbnail,
                        thumbnail_failed: row.thumbnail_failed,
                    },
                );
                publish(&row_id, &response);
                let mut menu = row.menu.into_iter().map(Some).collect::<Vec<_>>();
                ui_kit::Menu::context(&response, |ui| {
                    for entry in &mut menu {
                        let picked =
                            entry.as_ref().is_some_and(|entry| ui_kit::MenuItem::new(&entry.label, &entry.label, entry.icon).show(ui).clicked());
                        if picked && let Some(entry) = entry.take() {
                            *chosen = Some(entry.chosen);
                            ui.close();
                        }
                    }
                });
                if response.clicked() {
                    *chosen = Some(row.chosen);
                }
            }
        });
    });
    ui.add_space(t.file_section_gap);
    FileView::section(ui, &samples_title, t.space_10, |ui| {
        let (shown, picks): (Vec<_>, Vec<_>) =
            samples.into_iter().map(|sample| ((sample.title, sample.description, sample.icon, sample.thumbnail), sample.chosen)).unzip();
        let cards: Vec<_> = shown
            .iter()
            .map(|(title, description, icon, thumbnail)| ui_kit::SampleCard { title, description, icon, thumbnail: *thumbnail })
            .collect();
        let sample_id = format!("{id}:sample");
        let responses = FileView::sample_grid(ui, &sample_id, &cards);
        for ((index, response), pick) in responses.into_iter().enumerate().zip(picks) {
            publish(&format!("{sample_id}:{index}"), &response);
            if response.clicked() {
                *chosen = Some(pick);
            }
        }
    });
}

fn review_list<A>(ui: &mut egui::Ui, id: &str, reviews: Reviews<'_, A>, chosen: &mut Option<A>) {
    if reviews.items.is_empty() {
        ui_kit::text(ui, &reviews.empty, ui_kit::Text::Inline);
    }
    for (index, review) in reviews.items.into_iter().enumerate() {
        let review_id = format!("{id}:recovery:{index}");
        let (recover_label, recover_chosen) = review.recover;
        let (discard_label, discard_chosen) = review.discard;
        let (recover, discard) = FileView::recovery_review(ui, &review_id, &review.title, &review.detail, &recover_label, &discard_label);
        publish(&format!("{review_id}:recover"), &recover);
        if recover.clicked() {
            *chosen = Some(recover_chosen);
        }
        if let Some(discard) = discard {
            publish(&format!("{review_id}:discard"), &discard);
            if discard.clicked() {
                *chosen = Some(discard_chosen);
            }
        }
        ui.add_space(Tokens::get().space_12);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    enum Picked {
        Back,
        Rail(&'static str),
        Tile(&'static str),
        Recent(usize),
        Forget(usize),
        Sample(usize),
    }

    /// A start page of plain data: no app, no design, no engine.
    fn page(recent: usize) -> Page<'static, Picked> {
        let tile = |id: &'static str, primary| Tile {
            id: id.into(),
            icon: "new",
            title: id.into(),
            hint: "a hint".into(),
            primary,
            chosen: Picked::Tile(id),
        };
        let entry = |id: &'static str, selected| RailEntry {
            id: id.into(),
            label: id.into(),
            shortcut: "".into(),
            selected,
            enabled: true,
            chosen: Picked::Rail(id),
        };
        Page {
            id: "start",
            name: "Start page".into(),
            interactive: true,
            back: Back { template: "Back to {name}".into(), document: "Plate".into(), shortcut: "Esc".into(), chosen: Picked::Back },
            rail: vec![RailItem::Entry(entry("start:home", true)), RailItem::Separator, RailItem::Entry(entry("start:about", false))],
            title: "Start".into(),
            hint: "Pick something".into(),
            error: None,
            body: Body::Start(Start {
                banner: None,
                actions_title: "New".into(),
                actions: Tiles { grid: "start_new", tiles: vec![tile("start:first", true), tile("start:second", false)] },
                recent_title: "Recent".into(),
                recent_empty: "Nothing yet".into(),
                recent: (0..recent)
                    .map(|i| Recent {
                        title: format!("part {i}").into(),
                        path: "~/part".into(),
                        when: "Just now".into(),
                        kind: "Surface".into(),
                        thumbnail: None,
                        thumbnail_failed: false,
                        chosen: Picked::Recent(i),
                        menu: vec![MenuEntry { label: "Remove from list".into(), icon: "delete", chosen: Picked::Forget(i) }],
                    })
                    .collect(),
                samples_title: "Samples".into(),
                samples: (0..3)
                    .map(|i| Sample {
                        title: format!("sample {i}").into(),
                        description: "a sample".into(),
                        icon: "box",
                        thumbnail: None,
                        chosen: Picked::Sample(i),
                    })
                    .collect(),
            }),
        }
    }

    /// Draws the page three frames (hover, press, release) at what `name` published, and returns
    /// what the page chose.
    fn click(size: egui::Vec2, name: &str, recent: usize) -> Vec<Picked> {
        let ctx = crate::fonts::test_context();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let input = |events| egui::RawInput { screen_rect: Some(screen), focused: true, events, ..Default::default() };
        let mut chosen = Vec::new();
        solidcraft_ui_registry::clear_handles();
        crate::test_support::frame(&ctx, input(Vec::new()), |ui| chosen.extend(show(ui, page(recent))));
        let at = solidcraft_ui_registry::handle_at(name).unwrap_or_else(|| panic!("{name} was not drawn"));
        for pressed in [true, false] {
            solidcraft_ui_registry::clear_handles();
            crate::test_support::frame(&ctx, input(crate::test_support::pointer(at, pressed)), |ui| chosen.extend(show(ui, page(recent))));
        }
        chosen
    }

    #[test]
    fn start_page_from_plain_data_returns_the_tile_clicked() {
        let wide = egui::vec2(1400.0, 900.0);
        assert_eq!(click(wide, "start:second", 0), [Picked::Tile("start:second")]);
        assert_eq!(click(wide, "start:first", 0), [Picked::Tile("start:first")]);
    }

    #[test]
    fn start_page_returns_the_rail_entry_recent_and_sample_clicked() {
        let wide = egui::vec2(1400.0, 900.0);
        assert_eq!(click(wide, "start:back", 2), [Picked::Back]);
        assert_eq!(click(wide, "start:about", 2), [Picked::Rail("start:about")]);
        assert_eq!(click(wide, "start:recent:1", 2), [Picked::Recent(1)]);
        assert_eq!(click(wide, "start:sample:2", 2), [Picked::Sample(2)]);
    }

    #[test]
    fn start_page_lays_out_inside_a_narrow_window_and_keeps_its_tiles() {
        assert_eq!(click(egui::vec2(420.0, 800.0), "start:first", 0), [Picked::Tile("start:first")]);
        let page = solidcraft_ui_registry::rect_of("start:page").unwrap();
        assert!(page.contains_rect(solidcraft_ui_registry::rect_of("start:second").unwrap()));
        assert!(solidcraft_ui_registry::handle_at("start:back").is_none(), "the rail folds into the page's menu");
    }
}
