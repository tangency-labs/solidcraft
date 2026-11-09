// SPDX-License-Identifier: Apache-2.0
//! Saved display transforms from complete local override rows.
//!
//! The exact counted name locates a candidate. Admission requires the following
//! 100+ bytes to close: pinned markers, a rigid matrix, the counted path class,
//! a checked pointer string and an even record-index run. This combined grammar
//! prevents a name inside another payload from becoming a transform row.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ParseError, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::OccurrenceParent;
use cadmpeg_ir::report::loss::LossNote;
use serde::{Deserialize, Serialize};

use crate::container::ContainerScan;
use crate::layout::{scene_override_prefix as prefix, scene_record_index as index_layout};
use crate::loss::F3dLossCode;

const NAME: &str = "OverrideTransformAttribute";
const COUNTED_NAME: &[u8] =
    b"\x1a\0\0\0O\0v\0e\0r\0r\0i\0d\0e\0T\0r\0a\0n\0s\0f\0o\0r\0m\0A\0t\0t\0r\0i\0b\0u\0t\0e\0";
const PATH_CLASS: &str = "PersistentPassiveNodePath";
const MAX_PATH_ELEMENTS: usize = 1024;
const MAX_STRING_UNITS: u32 = 128;
const EPS_SCENE_ORTHONORMAL: f64 = 1e-6;
const EPS_SCENE_DETERMINANT: f64 = 1e-6;
const UNTAGGED_HIGH_WORD: u32 = 0;
const TAGGED_HIGH_WORD: u32 = 0x0100_0000;

pub(crate) type Sources = BTreeMap<String, [u64; 2]>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SceneBinding {
    Bound { occurrence: OccurrenceId },
    Ambiguous {},
    Unbound {},
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[[f64; 4]; 4]", into = "[[f64; 4]; 4]")]
pub(crate) struct SceneTransform([[f64; 4]; 4]);

impl TryFrom<[[f64; 4]; 4]> for SceneTransform {
    type Error = &'static str;

    fn try_from(matrix: [[f64; 4]; 4]) -> Result<Self, Self::Error> {
        if !matrix.iter().flatten().all(|value| value.is_finite())
            || matrix[3] != [0.0, 0.0, 0.0, 1.0]
        {
            return Err("scene transform requires a finite affine matrix");
        }
        for column in 0..3 {
            for other in 0..=column {
                let dot = (0..3)
                    .map(|row| matrix[row][column] * matrix[row][other])
                    .sum::<f64>();
                let expected = if column == other { 1.0 } else { 0.0 };
                if (dot - expected).abs() > EPS_SCENE_ORTHONORMAL {
                    return Err("scene transform rotation must be orthonormal");
                }
            }
        }
        let determinant = matrix[0][0]
            * (matrix[1][1] * matrix[2][2] - matrix[1][2] * matrix[2][1])
            - matrix[0][1] * (matrix[1][0] * matrix[2][2] - matrix[1][2] * matrix[2][0])
            + matrix[0][2] * (matrix[1][0] * matrix[2][1] - matrix[1][1] * matrix[2][0]);
        if (determinant - 1.0).abs() > EPS_SCENE_DETERMINANT {
            return Err("scene transform rotation must have determinant one");
        }
        Ok(Self(matrix))
    }
}

impl From<SceneTransform> for [[f64; 4]; 4] {
    fn from(value: SceneTransform) -> Self {
        value.0
    }
}

fn valid_path_length(count: usize) -> bool {
    count < MAX_PATH_ELEMENTS && count.is_multiple_of(2)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<u64>", into = "Vec<u64>")]
pub(crate) struct ScenePath(Vec<u64>);

impl TryFrom<Vec<u64>> for ScenePath {
    type Error = &'static str;
    fn try_from(path: Vec<u64>) -> Result<Self, Self::Error> {
        if valid_path_length(path.len()) {
            Ok(Self(path))
        } else {
            Err("override path requires a bounded even count")
        }
    }
}
impl From<ScenePath> for Vec<u64> {
    fn from(value: ScenePath) -> Self {
        value.0
    }
}

cadmpeg_core::named_optional_field!(deserialize_tagged_from, u64, "tagged_from");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DisplayPose {
    pub(crate) id: String,
    pub(crate) stream: String,
    pub(crate) byte_offset: u64,
    pub(crate) path: ScenePath,
    /// First element of the tagged path suffix.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tagged_from"
    )]
    pub(crate) tagged_from: Option<u64>,
    pub(crate) transform: SceneTransform,
    pub(crate) binding: SceneBinding,
}

#[derive(Debug)]
enum RowError {
    Read(ParseError),
    Grammar(&'static str),
    Decode(CodecError),
}

impl From<ParseError> for RowError {
    fn from(error: ParseError) -> Self {
        Self::Read(error)
    }
}
impl From<CodecError> for RowError {
    fn from(error: CodecError) -> Self {
        Self::Decode(error)
    }
}
impl std::fmt::Display for RowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(error) => write!(f, "{error:?}"),
            Self::Grammar(message) => f.write_str(message),
            Self::Decode(error) => error.fmt(f),
        }
    }
}

fn text<'a>(view: &mut View<'a>) -> Result<crate::bytes::utf16::Utf16View<'a>, RowError> {
    let count = view.req_u32_le()?;
    if count > MAX_STRING_UNITS {
        return Err(RowError::Grammar(
            "scene string exceeds its code-unit bound",
        ));
    }
    let bytes = view
        .counted(u64::from(count), 2)
        .ok_or(RowError::Grammar("UTF-16 count exceeds the stream"))?
        .get();
    let raw = view.req_take(
        bytes
            .checked_mul(2)
            .ok_or(RowError::Grammar("UTF-16 length overflow"))?,
    )?;
    crate::bytes::utf16::Utf16View::new(raw).ok_or(RowError::Grammar("invalid UTF-16 text"))
}

fn row(
    ctx: &DecodeContext<'_>,
    mut view: View<'_>,
    stream: &str,
    offset: u64,
) -> Result<DisplayPose, RowError> {
    let mut fixed = view.req_take_child(prefix::LEN)?;
    if !text(&mut fixed)?.eq_str(NAME)
        || fixed.req_take(prefix::PROLOGUE_VALUE.len())? != prefix::PROLOGUE_VALUE
    {
        return Err(RowError::Grammar("override name or prologue mismatch"));
    }
    let mut matrix = [[0.0; 4]; 4];
    for column in 0..4 {
        for matrix_row in &mut matrix {
            matrix_row[column] = f64::from(fixed.req_f32_le()?);
        }
    }
    SceneTransform::try_from(matrix).map_err(RowError::Grammar)?;
    for matrix_row in &mut matrix[..3] {
        matrix_row[3] *= 10.0;
    }
    let transform = SceneTransform(matrix);
    if fixed.req_take(prefix::PATH_MARKERS_VALUE.len())? != prefix::PATH_MARKERS_VALUE
        || !text(&mut fixed)?.eq_str(PATH_CLASS)
    {
        return Err(RowError::Grammar("override path marker or class mismatch"));
    }
    let pointer = text(&mut view)?;
    let plain = !pointer.is_empty() && pointer.chars().all(|ch| ch.is_ascii_hexdigit());
    let prefixed = pointer.len() > 2
        && pointer.chars().take(2).eq("0x".chars())
        && pointer.chars().skip(2).all(|ch| ch.is_ascii_hexdigit());
    if !plain && !prefixed {
        return Err(RowError::Grammar(
            "override pointer requires hexadecimal text",
        ));
    }
    let count = view.req_u32_le()?;
    let count = view
        .counted(u64::from(count), index_layout::LEN)
        .ok_or(RowError::Grammar("override path exceeds the stream"))?
        .get();
    if !valid_path_length(count) {
        return Err(RowError::Grammar(
            "override path requires a bounded even count",
        ));
    }
    let mut path = ctx.alloc_filled(count, 0_u64, "F3D scene path")?;
    let mut tagged_from = None;
    for (position, index) in path.iter_mut().enumerate() {
        let high = view.req_u32_be()?;
        let middle = view.req_u32_be()?;
        let low = view.req_u32_be()?;
        match high {
            UNTAGGED_HIGH_WORD if tagged_from.is_none() => {}
            TAGGED_HIGH_WORD if tagged_from.is_some() => {}
            TAGGED_HIGH_WORD if position > 0 && position.is_multiple_of(2) => {
                tagged_from = Some(u64_from_index(position));
            }
            _ => {
                return Err(RowError::Grammar(
                    "override element requires a record index in an untagged prefix or tagged pair suffix",
                ));
            }
        }
        *index = (u64::from(middle) << 32) | u64::from(low);
    }
    Ok(DisplayPose {
        id: format!("f3d:{stream}:display-pose#{offset}"),
        stream: stream.to_owned(),
        byte_offset: offset,
        path: ScenePath(path),
        tagged_from,
        transform,
        binding: SceneBinding::Unbound {},
    })
}

fn read(
    ctx: &DecodeContext<'_>,
    view: View<'_>,
    stream: &str,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<DisplayPose>, CodecError> {
    let mut rows = Vec::new();
    let mut refused = 0_u64;
    let mut first = None;
    for relative in memchr::memmem::find_iter(view.window(), COUNTED_NAME) {
        let offset = view
            .start()
            .checked_add(relative)
            .ok_or_else(|| CodecError::malformed("scene offset overflow"))?;
        let candidate = view
            .child(offset, view.end())
            .ok_or_else(|| CodecError::malformed("scene candidate exceeds its stream"))?;
        match row(ctx, candidate, stream, u64_from_index(relative)) {
            Ok(record) => {
                ctx.charge_entities(1, "F3D display pose")?;
                rows.push(record);
            }
            Err(RowError::Decode(error)) => return Err(error),
            Err(error) => {
                refused += 1;
                first.get_or_insert_with(|| format!("at byte {relative}: {error}"));
            }
        }
    }
    if refused > 0 {
        losses.push(F3dLossCode::SceneOverrideRefused.note(format!(
            "{stream}: {refused} display override candidate(s) refused; first: {}",
            first.unwrap_or_default()
        )));
    }
    Ok(rows)
}

pub(crate) fn collect(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    ir: &mut cadmpeg_ir::CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<bool, CodecError> {
    if ctx.container_only() {
        return Ok(false);
    }
    let mut rows = Vec::new();
    if let Some(folder) = scan.design_asset_folder() {
        let stream = format!("{folder}/OGS.BlobFolder/OGS/DefaultScene/world");
        if let Some(view) = scan.entry_view(&stream) {
            rows = read(ctx, view, &stream, losses)?;
        }
    }
    let present = !rows.is_empty();
    ir.native
        .namespace_mut("f3d")
        .set_arena(ctx, "display_poses", &rows)?;
    Ok(present)
}

pub(crate) fn sources(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    ir: &cadmpeg_ir::CadIr,
) -> Result<Sources, CodecError> {
    let Some(native) = ir.native.namespace("f3d") else {
        return Ok(Sources::new());
    };
    let spaces = native.arena_as_for_decode(ctx, "design_component_naming_spaces")?;
    let components = crate::design::decode::components::decode_local_components(scan, &spaces)?;
    let mut owners = BTreeMap::<&str, BTreeMap<u64, Option<u64>>>::new();
    for (stream, components) in &components {
        let index = owners.entry(stream).or_default();
        for (record, definition) in &components.roots {
            index
                .entry(definition.root_entity)
                .and_modify(|value| *value = None)
                .or_insert(Some(*record));
        }
    }
    let owner = |stream: &str, entity| owners.get(stream)?.get(&entity).copied().flatten();
    let mut out = Sources::new();
    for carrier in native.arena_iter_as_for_decode::<crate::records::feature::assembly_features::DesignComponentOccurrence>(ctx, "design_component_occurrences") {
        let carrier = carrier?;
        if let Some(definition) = crate::ids::native_stream(&carrier.id).and_then(|stream| owner(stream, carrier.owner_entity)) {
            out.insert(carrier.id, [definition, u64::from(carrier.record_index)]);
        }
    }
    for reference in native
        .arena_iter_as_for_decode::<crate::records::xref::XrefReference>(ctx, "xref_references")
    {
        let reference = reference?;
        if let Some(source) = &reference.placement_source {
            if let Some(definition) = source
                .owner
                .as_ref()
                .and_then(|entity| owner(&source.stream, entity.target))
            {
                out.insert(reference.id, [definition, source.carrier_record_index]);
            }
        }
    }
    Ok(out)
}

pub(crate) fn bind(
    ctx: &DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
    sources: &Sources,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let Some(native) = ir.native.namespace("f3d") else {
        return Ok(());
    };
    let mut rows: Vec<DisplayPose> = native.arena_as_for_decode(ctx, "display_poses")?;
    if rows.is_empty() {
        return Ok(());
    }
    let occurrences = ir
        .model
        .occurrences
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let mut paths = BTreeMap::<Vec<u64>, Option<OccurrenceId>>::new();
    for occurrence in &ir.model.occurrences {
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = Some(occurrence);
        let mut valid = true;
        while let Some(record) = current {
            if !seen.insert(record.id.as_str()) {
                valid = false;
                break;
            }
            if let Some(reference) = &record.native_ref {
                if let Some(pair) = sources.get(reference) {
                    chain.push(*pair);
                } else {
                    valid = false;
                    break;
                }
            }
            current = match &record.parent {
                OccurrenceParent::Root {} => None,
                OccurrenceParent::Occurrence { occurrence: parent } => {
                    let parent = occurrences.get(parent.as_str()).copied();
                    if parent.is_none() {
                        valid = false;
                    }
                    parent
                }
            };
        }
        if valid && !chain.is_empty() {
            let path = chain.into_iter().rev().flatten().collect::<Vec<_>>();
            paths
                .entry(path)
                .and_modify(|id| *id = None)
                .or_insert_with(|| Some(occurrence.id.clone()));
        }
    }
    let mut unbound = 0;
    let mut ambiguous = 0;
    for record in &mut rows {
        if record.tagged_from.is_some() {
            unbound += 1;
            record.binding = SceneBinding::Unbound {};
            continue;
        }
        record.binding = match paths.get(&record.path.0) {
            Some(Some(occurrence)) => SceneBinding::Bound {
                occurrence: occurrence.clone(),
            },
            Some(None) => {
                ambiguous += 1;
                SceneBinding::Ambiguous {}
            }
            None => {
                unbound += 1;
                SceneBinding::Unbound {}
            }
        };
    }
    if unbound > 0 {
        losses.push(F3dLossCode::SceneOverrideUnbound.note(format!("{unbound} display override row(s) have no neutral occurrence at their alternating record-index path")));
    }
    if ambiguous > 0 {
        losses.push(F3dLossCode::SceneOverrideAmbiguous.note(format!("{ambiguous} display override row(s) share their alternating record-index path with multiple neutral occurrences")));
    }
    ir.native
        .namespace_mut("f3d")
        .set_arena(ctx, "display_poses", &rows)?;
    Ok(())
}

pub(crate) fn validate(
    rows: &[DisplayPose],
    model: &cadmpeg_ir::document::Model,
) -> Vec<cadmpeg_ir::report::check::Finding> {
    let occurrences = model
        .occurrences
        .iter()
        .map(|record| &record.id)
        .collect::<BTreeSet<_>>();
    rows.iter()
        .filter_map(|row| {
            let tagged_valid = row.tagged_from.is_none_or(|first| {
                first > 0 && first.is_multiple_of(2) && first < u64_from_index(row.path.0.len())
            });
            let message = if !tagged_valid {
                "Fusion displayed pose has an invalid tagged path suffix"
            } else if let SceneBinding::Bound { occurrence } = &row.binding {
                if row.tagged_from.is_some() {
                    "Fusion displayed pose binds a tagged path"
                } else if occurrences.contains(occurrence) {
                    return None;
                } else {
                    "Fusion displayed pose binds a missing occurrence"
                }
            } else {
                return None;
            };
            Some(cadmpeg_ir::report::check::Finding {
                related: Vec::new(),
                check: cadmpeg_ir::report::check::Check::NativeLinks,
                severity: cadmpeg_ir::report::Severity::Error,
                message: message.to_owned(),
                entity: Some(row.id.clone()),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
