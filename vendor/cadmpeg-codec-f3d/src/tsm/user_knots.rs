// SPDX-License-Identifier: Apache-2.0
//! Regenerate a user-knot extension while retaining its control-cage records.

use std::fmt::Write;

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::subd::{SubdCage, SubdUserKnot};

pub(crate) fn rewrite(
    ctx: &DecodeContext<'_>,
    name: &str,
    bytes: &[u8],
    target: &SubdCage,
) -> Result<Vec<u8>, CodecError> {
    let source = super::parse(ctx, name, bytes)?;
    if source.unknown_record_kinds.contains_key("106ek") {
        return Err(CodecError::NotImplemented(
            "T-spline user-knot edits require a complete positional source run".into(),
        ));
    }
    let desired = target.edges();
    let mut supported = source.surface.cage;
    if supported.edges().len() != desired.len() {
        return Err(CodecError::NotImplemented(
            "T-spline user-knot edits require unchanged edge topology".into(),
        ));
    }
    supported
        .edit_edges(|edges| {
            for (edge, after) in edges.iter_mut().zip(desired) {
                edge.user_knot = after.user_knot;
            }
            Ok(())
        })
        .map_err(|error| super::malformed(name, error))?;
    if supported != *target {
        return Err(CodecError::NotImplemented(
            "T-spline user-knot edits require unchanged cage geometry and topology".into(),
        ));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|error| super::malformed(name, format_args!("payload is not UTF-8: {error}")))?;
    let lines: Vec<_> = text.split_inclusive('\n').collect();
    let last_edge = lines
        .iter()
        .rposition(|line| record_kind(line) == Some("e"))
        .ok_or_else(|| super::malformed(name, "control cage has no edge records"))?;
    let insertion = lines
        .iter()
        .position(|line| record_kind(line) == Some("106ek"))
        .unwrap_or(last_edge + 1);
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let present = desired.iter().any(|edge| edge.user_knot.is_some());
    let mut run = String::new();
    if present {
        let mut live = desired.iter();
        for line in &lines {
            let mut fields = line.split_ascii_whitespace();
            if fields.next() != Some("e") {
                continue;
            }
            let knot = if fields.next().is_some() {
                live.next()
                    .ok_or_else(|| super::malformed(name, "edge-slot map is incomplete"))?
                    .user_knot
            } else {
                None
            };
            let value = match knot {
                Some(SubdUserKnot::Override(value)) => value.get(),
                Some(SubdUserKnot::Inherit) | None => -1.0,
            };
            write!(run, "106ek {value}{newline}").map_err(|error| super::malformed(name, error))?;
        }
        if live.next().is_some() {
            return Err(super::malformed(
                name,
                "edge-slot map has trailing controls",
            ));
        }
    }
    let mut output = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index == insertion && present {
            if !output.is_empty() && !output.ends_with('\n') {
                output.push_str(newline);
            }
            output.push_str(&run);
        }
        if record_kind(line) != Some("106ek") {
            output.push_str(line);
        }
    }
    if insertion == lines.len() && present {
        if !output.is_empty() && !output.ends_with('\n') {
            output.push_str(newline);
        }
        output.push_str(&run);
    }
    Ok(output.into_bytes())
}

fn record_kind(line: &str) -> Option<&str> {
    line.split_ascii_whitespace().next()
}

#[cfg(test)]
mod tests;
