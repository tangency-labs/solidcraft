//! Bridge canvas selection into egui's ordinary Tab order.

/// Remember selection widgets without defining a second order for value fields or actions.
#[derive(Clone, Debug, Default)]
pub(crate) struct TabThrough {
    selections: Vec<(usize, egui::Id)>,
}

impl TabThrough {
    /// Start before any widgets are laid out. A canvas pick may have focused its floating
    /// value editor; Tab from there still continues from the active selection row. A popup
    /// must get its chance to consume Tab before this helper runs.
    pub(crate) fn begin(&self, ctx: &egui::Context, active: usize, canvas_value: egui::Id) {
        let direction = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Key { key: egui::Key::Tab, pressed: true, modifiers, .. } if !modifiers.any() => Some(egui::FocusDirection::Next),
                egui::Event::Key { key: egui::Key::Tab, pressed: true, modifiers, .. } if modifiers.shift_only() => {
                    Some(egui::FocusDirection::Previous)
                }
                _ => None,
            })
        });
        let Some(direction) = direction else { return };
        let Some((_, id)) = self.selections.iter().find(|(index, _)| *index == active) else { return };
        ctx.memory_mut(|memory| {
            if memory.focused().is_none_or(|focused| focused == canvas_value) {
                memory.request_focus(*id);
                memory.move_focus(direction);
            }
        });
    }

    /// Keyboard focus and pointer activation must select the same canvas input.
    pub(crate) fn selection(&mut self, index: usize, response: &egui::Response) -> bool {
        if let Some((_, id)) = self.selections.iter_mut().find(|(slot, _)| *slot == index) {
            *id = response.id;
        } else {
            self.selections.push((index, response.id));
        }
        if response.gained_focus() {
            response.scroll_to_me(None);
        }
        response.has_focus() || response.clicked()
    }
}
