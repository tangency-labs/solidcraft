// SPDX-License-Identifier: Apache-2.0
//! Low-level native record byte writers for source-less generation.

use cadmpeg_core::decode::u64_from_index;

use cadmpeg_core::CodecError;
use cadmpeg_ir::topology::Body;
use cadmpeg_ir::transform::Transform;

use crate::native::F3dNative;
use crate::writer::generate::index::NativeGenerationIndex;
use crate::writer::primitives::native_bool;

pub(super) fn native_ident(bytes: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    native_text(bytes, 0x0d, value)
}

pub(super) fn native_subident(bytes: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    native_text(bytes, 0x0e, value)
}

pub(super) fn native_curve_base(bytes: &mut Vec<u8>, kind: &str) -> Result<(), CodecError> {
    native_subident(bytes, kind)?;
    native_ident(bytes, "curve")?;
    native_ref(bytes, -1);
    native_i64(bytes, -1);
    native_ref(bytes, -1);
    if kind == "intcurve" {
        bytes.push(native_bool(false));
    }
    Ok(())
}

pub(super) fn native_surface_base(bytes: &mut Vec<u8>, kind: &str) -> Result<(), CodecError> {
    native_subident(bytes, kind)?;
    native_ident(bytes, "surface")?;
    native_ref(bytes, -1);
    native_i64(bytes, -1);
    native_ref(bytes, -1);
    if kind == "spline" {
        bytes.push(native_bool(false));
    }
    Ok(())
}

pub(super) fn native_string(bytes: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    native_text(bytes, 0x07, value)
}

pub(super) fn native_u16_string(bytes: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    let length = u16::try_from(value.len())
        .map_err(|_| CodecError::NotImplemented("F3D native text exceeds u16".into()))?;
    bytes.push(0x08);
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

/// Emit a native string using the minimal length-prefix width: `0x07` (u8) for
/// values up to 255 bytes, `0x08` (u16) up to 65535, otherwise `0x09` with a
/// ref-width (8-byte in generated `BinaryFile8` streams) length prefix.
/// The serializer stores law-formula names this way, so the width follows the
/// text length rather than a fixed tag.
pub(super) fn native_length_prefixed_string(
    bytes: &mut Vec<u8>,
    value: &str,
) -> Result<(), CodecError> {
    if u8::try_from(value.len()).is_ok() {
        native_string(bytes, value)
    } else if let Ok(length) = u16::try_from(value.len()) {
        bytes.push(0x08);
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
        Ok(())
    } else {
        let length = u64_from_index(value.len());
        bytes.push(0x09);
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }
}

fn native_text(bytes: &mut Vec<u8>, tag: u8, value: &str) -> Result<(), CodecError> {
    let length = u8::try_from(value.len())
        .map_err(|_| CodecError::NotImplemented("F3D native text exceeds 255 bytes".into()))?;
    bytes.extend_from_slice(&[tag, length]);
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

pub(super) fn native_ref(bytes: &mut Vec<u8>, value: i64) {
    bytes.push(0x0c);
    bytes.extend_from_slice(&value.to_le_bytes());
}

pub(super) fn native_record_index(base: i64, ordinal: usize) -> Result<i64, CodecError> {
    let ordinal = i64::try_from(ordinal)
        .map_err(|_| CodecError::NotImplemented("F3D record ordinal exceeds i64".into()))?;
    base.checked_add(ordinal)
        .ok_or_else(|| CodecError::NotImplemented("F3D record index exceeds i64".into()))
}

pub(super) fn native_i64(bytes: &mut Vec<u8>, value: i64) {
    bytes.push(0x04);
    bytes.extend_from_slice(&value.to_le_bytes());
}

pub(super) fn native_enum(bytes: &mut Vec<u8>, value: i64) {
    bytes.push(0x15);
    bytes.extend_from_slice(&value.to_le_bytes());
}

pub(super) fn native_f64(bytes: &mut Vec<u8>, value: f64) {
    bytes.push(0x06);
    bytes.extend_from_slice(&value.to_le_bytes());
}

pub(super) fn native_point(bytes: &mut Vec<u8>, point: [f64; 3]) {
    bytes.push(0x13);
    for value in point {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

pub(super) fn native_vector(bytes: &mut Vec<u8>, vector: [f64; 3]) {
    bytes.push(0x14);
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

pub(super) fn native_transform(
    bytes: &mut Vec<u8>,
    topology: &NativeGenerationIndex<'_>,
    body: &Body,
    transform: Transform,
) -> Result<(), CodecError> {
    native_ident(bytes, "transform")?;
    for vector in [
        [
            transform.rows()[0][0],
            transform.rows()[1][0],
            transform.rows()[2][0],
        ],
        [
            transform.rows()[0][1],
            transform.rows()[1][1],
            transform.rows()[2][1],
        ],
        [
            transform.rows()[0][2],
            transform.rows()[1][2],
            transform.rows()[2][2],
        ],
        [
            transform.rows()[0][3] / 600.0,
            transform.rows()[1][3] / 600.0,
            transform.rows()[2][3] / 600.0,
        ],
    ] {
        native_vector(bytes, vector);
    }
    native_f64(bytes, transform.rows()[3][3]);
    let hints = topology.transform_hints.get(body.id.as_str()).map_or_else(
        || derived_transform_hints(transform),
        |hints| [hints.rotation, hints.reflection, hints.shear],
    );
    for hint in hints {
        bytes.push(native_bool(hint));
    }
    Ok(())
}

fn derived_transform_hints(transform: Transform) -> [bool; 3] {
    let linear = [
        [
            transform.rows()[0][0],
            transform.rows()[0][1],
            transform.rows()[0][2],
        ],
        [
            transform.rows()[1][0],
            transform.rows()[1][1],
            transform.rows()[1][2],
        ],
        [
            transform.rows()[2][0],
            transform.rows()[2][1],
            transform.rows()[2][2],
        ],
    ];
    let determinant = linear[0][0] * (linear[1][1] * linear[2][2] - linear[1][2] * linear[2][1])
        - linear[0][1] * (linear[1][0] * linear[2][2] - linear[1][2] * linear[2][0])
        + linear[0][2] * (linear[1][0] * linear[2][1] - linear[1][1] * linear[2][0]);
    let reflection = determinant.is_sign_negative();
    let columns = [
        [linear[0][0], linear[1][0], linear[2][0]],
        [linear[0][1], linear[1][1], linear[2][1]],
        [linear[0][2], linear[1][2], linear[2][2]],
    ];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let scale = columns
        .iter()
        .map(|column| dot(*column, *column))
        .fold(1.0f64, f64::max);
    let shear = dot(columns[0], columns[1]).abs() > f64::EPSILON * scale
        || dot(columns[0], columns[2]).abs() > f64::EPSILON * scale
        || dot(columns[1], columns[2]).abs() > f64::EPSILON * scale;
    let rotation = linear != [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    [rotation, reflection, shear]
}

pub(super) fn native_history_tail(
    bytes: &mut Vec<u8>,
    native: &F3dNative,
) -> Result<(), CodecError> {
    let histories = native.asm_histories.as_slice();
    if histories.is_empty() {
        native_ident(bytes, "delta_state")?;
        return Ok(());
    }
    if histories.len() != 1 {
        return Err(CodecError::NotImplemented(
            "source-less F3D generation supports one ASM history stream".into(),
        ));
    }
    let history = &histories[0];
    if let Some(preamble) = history.preamble {
        if history
            .states()
            .first()
            .is_none_or(|state| state.state_id != preamble.stream_size)
            || preamble.history_entry_count < 0
        {
            return Err(CodecError::malformed(format_args!(
                "F3D history {} requires head state_id == stream_size and nonnegative history_entry_count",
                history.id
            )));
        }
        for name in ["Begin", "of", "ASM", "History"] {
            native_subident(bytes, name)?;
        }
        native_ident(bytes, "Data")?;
        native_ident(bytes, "history_stream")?;
        native_i64(bytes, preamble.stream_size);
        native_i64(bytes, preamble.stream_size);
        native_i64(bytes, 0);
        native_i64(bytes, preamble.history_entry_count);
        for reference in [-1, 0, 1, -1] {
            native_ref(bytes, reference);
        }
        bytes.push(0x11);
    }
    for state in history.states() {
        native_ident(bytes, "delta_state")?;
        native_i64(bytes, state.state_id);
        native_i64(bytes, state.version_flag);
        native_i64(bytes, state.state_flag);
        native_ref(bytes, state.previous_ref.unwrap_or(-1));
        native_ref(bytes, state.next_ref.unwrap_or(-1));
        native_ref(bytes, state.node_index);
        native_ref(bytes, state.partner_ref.unwrap_or(-1));
        native_ref(bytes, state.owner_ref);
        bytes.push(0x0b);
        for board in &state.bulletin_boards {
            native_i64(bytes, 1);
            native_ref(bytes, board.owner_ref);
            native_i64(bytes, board.number);
            for change in &board.changes {
                native_i64(bytes, 1);
                native_ref(bytes, change.old_ref().unwrap_or(-1));
                native_ref(bytes, change.new_ref().unwrap_or(-1));
            }
            native_i64(bytes, 0);
        }
        native_i64(bytes, 0);
        bytes.push(0x11);
        for record in &state.records {
            bytes.extend_from_slice(&record.raw_bytes);
        }
    }
    Ok(())
}
