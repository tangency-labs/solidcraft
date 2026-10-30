//! Bridge canvas selection into egui's ordinary Tab order.

/// egui's `shift_only` also accepts physical Ctrl+Shift on macOS; dialog traversal must not.
fn tab_direction(modifiers: &egui::Modifiers) -> Option<egui::FocusDirection> {
    if modifiers.ctrl || modifiers.alt || modifiers.command || modifiers.mac_cmd {
        None
    } else {
        Some(if modifiers.shift { egui::FocusDirection::Previous } else { egui::FocusDirection::Next })
    }
}

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
                egui::Event::Key { key: egui::Key::Tab, pressed: true, modifiers, .. } => tab_direction(modifiers),
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
        // Let the dialog's existing Enter-to-OK handler run instead of activating the chip.
        if response.has_focus() && response.ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
            response.surrender_focus();
            return false;
        }
        response.has_focus() || response.clicked()
    }
}

#[derive(Clone, Copy)]
struct Scope {
    id: egui::Id,
    layer: egui::LayerId,
    frame: u64,
    pass: u64,
}

#[derive(Clone, Copy, Default)]
struct Traversal {
    backwards: bool,
    pending: bool,
}

fn active_id() -> egui::Id {
    egui::Id::new("dialog_tab_scope")
}
fn current_id() -> egui::Id {
    egui::Id::new("dialog_tab_current")
}
fn first(id: egui::Id) -> egui::Id {
    id.with("tab_first")
}
fn last(id: egui::Id) -> egui::Id {
    id.with("tab_last")
}
fn traversal(id: egui::Id) -> egui::Id {
    id.with("tab_traversal")
}

/// Run after expression completion, before the canvas or background widgets handle Tab.
pub(crate) fn prepare_active(ctx: &egui::Context) {
    if let Some(scope) = ctx.data(|d| d.get_temp::<Scope>(active_id()))
        && scope.frame.saturating_add(1) >= ctx.cumulative_frame_nr()
    {
        prepare(ctx, scope);
    }
}

fn prepare(ctx: &egui::Context, scope: Scope) {
    // Let a popup finish opening before its own scope takes over.
    if egui::Popup::is_any_open(ctx) && scope.layer.order != egui::Order::Foreground {
        return;
    }
    let direction = ctx.input(|i| {
        i.events.iter().find_map(|event| match event {
            egui::Event::Key { key: egui::Key::Tab, pressed: true, modifiers, .. } => Some(tab_direction(modifiers)),
            _ => None,
        })
    });
    let Some(direction) = direction else { return };
    let Some(direction) = direction else {
        // Cancel egui's automatic Ctrl+Shift+Tab traversal without consuming the shortcut.
        ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
        return;
    };
    let backwards = direction == egui::FocusDirection::Previous;
    ctx.data_mut(|d| d.insert_temp(traversal(scope.id), Traversal { backwards, pending: true }));
    let inside = ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)).is_some_and(|r| r.layer_id == scope.layer);
    ctx.input_mut(|i| {
        i.events.retain(|event| {
            !matches!(event,
                egui::Event::Key { key: egui::Key::Tab, modifiers, .. } if tab_direction(modifiers).is_some()
            )
        })
    });
    ctx.memory_mut(|m| {
        if !inside {
            m.request_focus(if backwards { last(scope.id) } else { first(scope.id) });
        }
        m.move_focus(if backwards { egui::FocusDirection::Previous } else { egui::FocusDirection::Next });
    });
}

/// Invisible boundaries keep egui's native field/action order, skipping disabled controls.
/// They do not allocate space, block canvas picking, or require a second list of widgets.
pub(crate) fn scope<R>(ui: &mut egui::Ui, id: egui::Id, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    if !ui.is_enabled() || !ui.is_visible() {
        return add(ui);
    }
    let ctx = ui.ctx().clone();
    let scope = Scope { id, layer: ui.layer_id(), frame: ctx.cumulative_frame_nr(), pass: ctx.cumulative_pass_nr() };
    let previous = ctx.data(|d| d.get_temp::<Scope>(active_id()));
    let behind = previous.is_some_and(|p| {
        p.pass == scope.pass
            && (p.layer.order > scope.layer.order
                || (p.layer != scope.layer && ctx.memory(|m| m.areas().top_layer_id(scope.layer.order)) == Some(p.layer)))
    });
    if !behind {
        prepare(&ctx, scope);
        ctx.data_mut(|d| d.insert_temp(active_id(), scope));
    }
    let parent = ctx.data(|d| d.get_temp::<egui::Id>(current_id()));
    ctx.data_mut(|d| d.insert_temp(current_id(), id));
    let mut state = ctx.data(|d| d.get_temp::<Traversal>(traversal(id))).unwrap_or_default();
    if state.pending && ctx.memory(|m| m.has_focus(first(id))) {
        ctx.memory_mut(|m| {
            if state.backwards {
                m.request_focus(last(id));
            }
            m.move_focus(if state.backwards { egui::FocusDirection::Previous } else { egui::FocusDirection::Next });
        });
        if !state.backwards {
            state.pending = false;
        }
    }
    ui.interact(egui::Rect::from_min_size(ui.min_rect().min, egui::Vec2::ZERO), first(id), egui::Sense::focusable_noninteractive());
    let result = add(ui);
    if ctx.memory(|m| m.has_focus(last(id))) {
        ctx.memory_mut(|m| m.move_focus(if state.backwards && state.pending { egui::FocusDirection::Previous } else { egui::FocusDirection::None }));
    }
    let end = ui.interact(egui::Rect::from_min_size(ui.min_rect().min, egui::Vec2::ZERO), last(id), egui::Sense::focusable_noninteractive());
    if state.pending && end.has_focus() {
        if state.backwards {
            state.pending = false;
        } else {
            ctx.memory_mut(|m| m.request_focus(first(id)));
        }
        ctx.request_repaint();
    }
    // Kit buttons use explicit borders, so native focus styling alone is not visible.
    // Paint once for the whole scope, including dropdowns and custom dialog actions.
    if let Some(response) = ctx.memory(|m| m.focused()).and_then(|focused| ctx.read_response(focused))
        && response.layer_id == ui.layer_id()
        && response.enabled()
        && response.rect.is_positive()
    {
        let t = crate::theme::Tokens::get();
        ui.painter().rect_stroke(
            response.rect.expand(t.space_2),
            t.control_radius,
            egui::Stroke::new(1.0, crate::theme::TabAccent::current(&ctx).color),
            egui::StrokeKind::Inside,
        );
    }
    ctx.data_mut(|d| {
        d.insert_temp(traversal(id), state);
        d.remove::<egui::Id>(current_id());
        if let Some(parent) = parent {
            d.insert_temp(current_id(), parent);
        }
    });
    result
}

/// A dialog's dropdown is a nested focus loop while its popup is open.
pub(crate) fn popup<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    if ui.ctx().data(|d| d.get_temp::<egui::Id>(current_id())).is_some() { scope(ui, ui.id().with("tab_popup"), add) } else { add(ui) }
}
