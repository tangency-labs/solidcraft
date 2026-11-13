//! The gallery's pages, one per component (or per family of calls drawn the same way), grouped as
//! the sidebar lists them. Every name in a page's `items` is a public item of `solidcraft-ui-kit`
//! (`gallery_tests` checks it against the kit's sources).

mod buttons;
mod chrome;
mod feedback;
mod inputs;
mod layout;
mod menus;
mod tables;

use crate::Demo;

/// The sidebar's groups, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Inputs,
    Buttons,
    Tables,
    Menus,
    Layout,
    Feedback,
    Chrome,
}

impl Group {
    pub const ALL: [Group; 7] = [Group::Inputs, Group::Buttons, Group::Tables, Group::Menus, Group::Layout, Group::Feedback, Group::Chrome];

    pub fn title(self) -> &'static str {
        match self {
            Group::Inputs => "Inputs",
            Group::Buttons => "Buttons",
            Group::Tables => "Tables and lists",
            Group::Menus => "Menus",
            Group::Layout => "Layout and sections",
            Group::Feedback => "Feedback",
            Group::Chrome => "App chrome",
        }
    }
}

/// One component's page.
pub struct Page {
    /// Its address (`#id`) and widget id salt.
    pub id: &'static str,
    pub title: &'static str,
    pub group: Group,
    /// The kit's public items the page shows (functions, types, methods).
    pub items: &'static [&'static str],
    /// What it is for, in a sentence or two.
    pub what: &'static str,
    /// A few lines of Rust with the real call signature.
    pub usage: &'static str,
    pub dos: &'static [&'static str],
    pub donts: &'static [&'static str],
    /// The live example, in its states.
    pub demo: fn(&mut egui::Ui, &mut Demo),
}

impl Page {
    /// Whether the sidebar's (lower-case) filter finds this page.
    pub fn matches(&self, filter: &str) -> bool {
        self.title.to_lowercase().contains(filter) || self.id.contains(filter) || self.items.iter().any(|i| i.to_lowercase().contains(filter))
    }
}

/// Every page, in sidebar order within each group.
pub static PAGES: &[Page] = &[
    inputs::TEXT_FIELD,
    inputs::TEXT_AREA,
    inputs::NUMBER_FIELD,
    inputs::COUNT_FIELD,
    inputs::SLIDER,
    inputs::CHECKBOX,
    inputs::SEGMENTED,
    inputs::DROPDOWN,
    inputs::COLOR,
    inputs::SEARCH_FIELD,
    inputs::CHIPS,
    inputs::ROWS,
    buttons::BUTTON,
    buttons::ICON_BUTTONS,
    buttons::TEXT_BUTTONS,
    buttons::FOOTER,
    buttons::KEYBOARD,
    tables::TABLE,
    tables::GRID,
    tables::NAME_LIST,
    tables::TREE,
    tables::LIST_PICKER,
    tables::REPORTS,
    menus::MENU,
    menus::WORKSPACE_MENU,
    menus::RENAME_MENU,
    menus::RADIAL_MENU,
    menus::COMMAND_SEARCH,
    menus::POPUP_LIST,
    layout::SECTION,
    layout::DISCLOSURE,
    layout::TEXT,
    layout::WINDOW,
    layout::DETAILS,
    feedback::STATUS_LINE,
    feedback::PROGRESS,
    feedback::TOAST,
    feedback::CONFIRM,
    feedback::VIEWPORT_STATUS,
    feedback::ATTENTION,
    chrome::APP_BAR,
    chrome::RIBBON,
    chrome::HISTORY_BAR,
    chrome::VIEW_COMPASS,
    chrome::NAVIGATION,
    chrome::WINDOW_CHROME,
    chrome::THEME_PICKER,
];
