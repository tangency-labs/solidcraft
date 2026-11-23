//! macOS's secondary click: Control with a primary click (trackpad or one-button mouse) opens
//! the context menu, as a right click does (#20683). winit reports it as a primary button with
//! Control held, so the press waits until it is known to be a click and not a Control+drag,
//! which orbits from empty space (`navigation`).

/// A Control+primary press held until it is a click (sent as a secondary click) or a drag (sent
/// as it came). On only where egui says the platform is macOS (`SolveApp::logic`).
#[derive(Default)]
pub(crate) struct ControlClick {
    pub(crate) enabled: bool,
    press: Option<(egui::Pos2, egui::Modifiers)>,
}

impl ControlClick {
    /// Remap the platform's events of a frame. A held press carries over to the next frame.
    /// Returns the events for the next frame: once a held press turns out to be a drag, it is
    /// sent alone and what followed it waits a frame, so the drag starts as any drag does, on a
    /// frame of its own, and orbits by the whole move.
    pub(crate) fn remap(&mut self, events: &mut Vec<egui::Event>) -> Vec<egui::Event> {
        if !self.enabled || (self.press.is_none() && !events.iter().any(control_press)) {
            return Vec::new();
        }
        let slop = egui::InputOptions::default().max_click_dist;
        let mut out = Vec::with_capacity(events.len() + 2);
        let mut rest = events.drain(..);
        while let Some(event) = rest.next() {
            let Some((at, modifiers)) = self.press else {
                if control_press(&event)
                    && let egui::Event::PointerButton { pos, modifiers, .. } = event
                {
                    self.press = Some((pos, modifiers));
                } else {
                    out.push(event);
                }
                continue;
            };
            match event {
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, .. } => {
                    let modifiers = egui::Modifiers { ctrl: false, ..modifiers };
                    let button = egui::PointerButton::Secondary;
                    out.push(egui::Event::PointerButton { pos: at, button, pressed: true, modifiers });
                    out.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers });
                    self.press = None;
                }
                egui::Event::PointerMoved(pos) if pos.distance(at) <= slop => out.push(event),
                egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. } | egui::Event::PointerGone => {
                    out.push(egui::Event::PointerMoved(at));
                    out.push(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers });
                    self.press = None;
                    let later = std::iter::once(event).chain(rest).collect();
                    *events = out;
                    return later;
                }
                _ => out.push(event),
            }
        }
        drop(rest);
        *events = out;
        Vec::new()
    }
}

fn control_press(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: true, modifiers, .. }
            if modifiers.ctrl && !modifiers.mac_cmd && !modifiers.alt && !modifiers.shift
    )
}

#[cfg(test)]
#[path = "control_click_tests.rs"]
mod tests;
