//! What a hardware callout (#20112, [`solidcraft_doc::drawing::hardware_callout`]) says and points at
//! now, for the sheet and for `drawing.note.inspect`: its text read from the hardware record in the
//! drawing's unit and language, and, on a flat pattern view, where its hole is in the flat. Both are
//! none when the hole is lost, so the note keeps its saved text and draws no leader there.

use solidcore_record::Vec2;
use solidcraft_doc::drawing::hardware_callout::{self, HardwareRef};
use solidcraft_doc::drawing::hole_callout::{DEFAULT_DECIMALS, TextStyle};
use solidcraft_doc::drawing::note::Note;
use solidcraft_doc::drawing::{Drawing, Units};
use solidcraft_doc::expr;

use crate::Session;

/// How a callout's numbers and words are written: in the drawing's fixed unit (else the design's), as a
/// hole callout's are, and its language.
pub(crate) fn style(s: &Session, d: &Drawing) -> TextStyle {
    let units = match &d.units {
        Units::Fixed { unit } => unit.clone(),
        _ => expr::length_unit_or_mm(&s.doc.units).to_string(),
    };
    TextStyle { units, decimals: DEFAULT_DECIMALS, language: d.language.clone() }
}

/// What `note` says now when it reads a hardware hole that is still there, in the drawing's fixed unit
/// (else the design's) and its language.
pub(crate) fn text(s: &Session, d: &Drawing, note: &Note) -> Option<String> {
    let hole = note.hardware.as_ref()?;
    let state = s.model.state();
    let read = hardware_callout::read(&state.surface_sheets, hole).ok()?;
    hardware_callout::text(&read, &style(s, d)).ok().map(|lines| lines.join("\n"))
}

/// Where the hole `note` reads is on its flat pattern view's plane, turned as the view turns its flat:
/// none for a note on any other view, or when the flat has no such hole.
pub(crate) fn flat_tip(s: &Session, d: &Drawing, note: &Note, cmd: &str) -> Option<Vec2> {
    let HardwareRef { feature, copy } = note.hardware?;
    let of = d.resolve_view(note.anchor.view()?).ok()?.flat?;
    crate::drawing_view::flat(s, &of, cmd).ok()?.hardware_at(feature, copy)
}

/// The press-in hardware holes of view `view` of `d` that a callout can name, each as its feature, its copy
/// number and the text a callout of it would say now. On a flat pattern view they are its body's, in the order
/// the flat pattern lists them; on a view of the model, every hole whose circular edge is on a flat panel of
/// the folded sheet, where a callout's leader can end. None for a view that can't be read.
pub fn holes(s: &Session, d: &Drawing, view: u64) -> Vec<(u64, u32, String)> {
    let Ok(resolved) = d.resolve_view(view) else { return Vec::new() };
    let state = s.model.state();
    let style = style(s, d);
    let mut seen: Vec<(u64, u32)> = Vec::new();
    match resolved.flat {
        Some(of) => {
            let Ok(reply) = crate::cmd::flat_reply(s, Some(&of.body), "drawing.hardware_callout.add") else { return Vec::new() };
            let all = reply.get("hardware").and_then(|h| h.as_array()).into_iter().flatten();
            for feature in all.filter_map(|h| h.get("feature")?.as_u64()) {
                let copy = u32::try_from(seen.iter().filter(|(f, _)| *f == feature).count()).unwrap_or(u32::MAX);
                seen.push((feature, copy));
            }
        }
        None => {
            // A feature's holes are numbered in the sheet that has them first, as `hardware_callout::read` does.
            let mut done: Vec<u64> = Vec::new();
            for sheet in &state.surface_sheets {
                let mut own: Vec<(u64, u32)> = Vec::new();
                for h in sheet.hardware_holes() {
                    let copy = u32::try_from(own.iter().filter(|(f, _)| *f == h.feature).count()).unwrap_or(u32::MAX);
                    own.push((h.feature, copy));
                }
                let edges = sheet.hardware_edge_points();
                let fresh: Vec<u64> = own.iter().map(|(f, _)| *f).filter(|f| !done.contains(f)).collect();
                seen.extend(own.iter().zip(&edges).filter(|((f, _), e)| e.is_some() && fresh.contains(f)).map(|(h, _)| *h));
                done.extend(fresh);
            }
        }
    }
    seen.into_iter()
        .filter_map(|(feature, copy)| {
            let read = hardware_callout::read(&state.surface_sheets, &HardwareRef { feature, copy }).ok()?;
            Some((feature, copy, hardware_callout::text(&read, &style).ok()?.join(" ")))
        })
        .collect()
}
