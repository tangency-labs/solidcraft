// SPDX-License-Identifier: Apache-2.0
//! F3Z manifest resolution and archive-member dialect classification.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeCheckpoint, DecodeContext};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use serde::Deserialize;

use crate::container::ContainerScan;
use crate::loss::F3dLossCode;

const MANIFEST_ENTRY: &str = "Manifest.json";
const DESIGN_DESCRIPTION_ENTRY: &str = "DesignDescription.json";

#[derive(Deserialize)]
struct ManifestJson {
    root: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesignDescriptionJson {
    design_description: DesignDescription,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesignDescription {
    design_graphs: Vec<DesignGraph>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesignGraph {
    root_ids: Vec<u64>,
    design_objects: Vec<DesignObject>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesignObject {
    id: u64,
    relative_path: String,
    content_type: String,
    references: Vec<DesignObjectReference>,
}

#[derive(Deserialize)]
struct DesignObjectReference {
    #[serde(rename = "type")]
    reference_type: String,
    ids: Vec<u64>,
}

/// Retains member classification and the checkpoints used to reopen each scan.
pub(super) struct ArchiveSession {
    pub(super) members: BTreeMap<String, ClassifiedMember>,
    pub(super) layers: DialectLayers,
    pub(super) losses: Vec<LossNote>,
}

/// Facts read from a member's container scan during classification.
#[derive(Default)]
pub(super) struct MemberMetadata {
    pub(super) references: Vec<String>,
    pub(super) placement_only: bool,
    pub(super) error: Option<String>,
}

pub(super) enum ClassifiedMember {
    Scanned {
        checkpoint: DecodeCheckpoint,
        working_bytes: Option<u64>,
        metadata: Option<Box<MemberMetadata>>,
    },
    Unreadable(String),
}

impl ArchiveSession {
    pub(super) fn member_checkpoint(&self, path: &str) -> Result<&DecodeCheckpoint, CodecError> {
        match self.members.get(path) {
            Some(ClassifiedMember::Scanned { checkpoint, .. }) => Ok(checkpoint),
            Some(ClassifiedMember::Unreadable(message)) => Err(CodecError::malformed(
                format_args!("f3z document member {path} could not be scanned: {message}"),
            )),
            None => Err(CodecError::malformed(format_args!(
                "f3z document member {path} is not present in the archive"
            ))),
        }
    }

    pub(super) fn with_member_scan<T>(
        &self,
        outer: &ContainerScan<'_>,
        path: &str,
        work: impl FnOnce(&ContainerScan<'_>) -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        let arena = DecodeArena::new();
        let root = outer.member_view(&arena, path)?;
        let local = self.member_checkpoint(path)?.resume(&arena);
        local.take_trace();
        let scan = crate::container::scan(&local, root)?;
        work(&scan)
    }
}

/// Resolves the archive manifest to the F3D member that owns the model.
pub(super) fn model_root(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<(String, Option<String>), CodecError> {
    let manifest_bytes = scan.entry_bytes(MANIFEST_ENTRY)?;

    let text = std::str::from_utf8(manifest_bytes).map_err(|error| {
        CodecError::malformed(format_args!("{MANIFEST_ENTRY} is not valid JSON: {error}"))
    })?;
    let manifest: ManifestJson =
        ctx.parse_json(text, "parse F3Z manifest JSON")
            .map_err(|error| {
                let CodecError::Malformed(error) = error else {
                    return error;
                };
                CodecError::malformed(format_args!("{MANIFEST_ENTRY} is not valid JSON: {error}"))
            })?;
    model_root_member(ctx, scan, &manifest.root)
}

/// Reads the member list facts from one nested scan.
/// The work runs on a private context, so its charges stay out of the session.
fn member_metadata(
    checkpoint: &DecodeCheckpoint,
    arena: &DecodeArena,
    member_scan: &ContainerScan<'_>,
) -> MemberMetadata {
    let metadata_ctx = checkpoint.resume(arena);
    metadata_ctx.take_trace();
    let mut metadata = MemberMetadata::default();
    let result = (|| {
        let Some(table) = crate::xref::decode(&metadata_ctx, member_scan)? else {
            return Ok(false);
        };
        metadata.references.extend(
            table
                .references
                .iter()
                .map(|reference| reference.relative_path.as_str().to_owned()),
        );
        let selection = crate::design::decode::body::design_model_selection(member_scan)?;
        crate::xref::is_assembly(
            &metadata_ctx,
            member_scan,
            Some(&table),
            selection.explicitly_empty,
        )
    })();
    match result {
        Ok(placement_only) => metadata.placement_only = placement_only,
        Err(error) => metadata.error = Some(error.to_string()),
    }
    metadata
}

/// Classifies all F3D members and attaches each nested layer to its archive path.
///
/// Each member is inflated and scanned once. With `with_metadata`, the same
/// scan also yields the member references and placement role for the member
/// list; the session keeps those owned facts and drops the scan.
pub(super) fn classify_members<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
    with_metadata: bool,
) -> Result<ArchiveSession, CodecError> {
    let mut members = BTreeMap::new();
    let primary = scan.kind.dialect().clone();
    let mut layers = DialectLayers::of(primary);
    let mut losses = Vec::new();
    for member_path in scan
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .filter(|name| crate::container::is_f3d_name(name))
    {
        super::observer::classifying(member_path, |timing| {
            let checkpoint = ctx.checkpoint();
            let arena = DecodeArena::new();
            let classified = {
                let root = super::observer::timed(&mut timing.nested_scan, || {
                    if scan.has_entry(member_path) {
                        scan.member_view(&arena, member_path)
                    } else {
                        Err(CodecError::malformed(format_args!(
                            "f3z document member {member_path} is not readable"
                        )))
                    }
                });
                root.and_then(|view| {
                    ctx.with_arena(&arena, |local| {
                        let member_scan = super::observer::timed(&mut timing.nested_scan, || {
                            crate::container::scan(local, view)
                        })?;
                        Ok(super::observer::timed(&mut timing.layers, || {
                            crate::dialect::classify_layers(&member_scan)
                        })
                        .map(|(layers, losses)| {
                            let metadata = with_metadata.then(|| {
                                super::observer::timed(&mut timing.metadata, || {
                                    Box::new(member_metadata(&checkpoint, &arena, &member_scan))
                                })
                            });
                            (
                                layers,
                                losses,
                                member_scan.declared_working_bytes(),
                                metadata,
                            )
                        }))
                    })
                })
            };
            let classified = match classified {
                Ok(classified) => classified,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(error) => {
                    let message = format!("{error}");
                    super::push_loss(&mut losses, F3dLossCode::XrefMemberUndecoded, format_args!(
                        "xref {member_path}: member could not be scanned as an F3D document ({message}); its source bytes remain retained"
                    ));
                    members.insert(
                        member_path.to_owned(),
                        ClassifiedMember::Unreadable(message),
                    );
                    return Ok(());
                }
            };
            let (member_layers, mut member_losses, working_bytes, metadata) = classified?;
            if let Some(metadata) = &metadata {
                let facts = u64_from_index(metadata.references.len() + 1);
                ctx.charge_entities(facts, "cache F3Z member metadata")?;
            }
            super::observer::timed(&mut timing.layers, || {
                for loss in &mut member_losses {
                    loss.message = format!("archive member {member_path}: {}", loss.message);
                }
                losses.append(&mut member_losses);
                losses.extend(merge_member_layers(
                    &mut layers,
                    &member_layers,
                    member_path,
                ));
            });

            members.insert(
                member_path.to_owned(),
                ClassifiedMember::Scanned {
                    checkpoint,
                    working_bytes,
                    metadata,
                },
            );
            Ok(())
        })?;
    }
    losses.extend(crate::dialect::dialect_losses(&layers));
    crate::report::header_losses(scan, &mut losses)?;
    Ok(ArchiveSession {
        members,
        layers,
        losses,
    })
}

/// Attaches one archive member's identity and nested layers to its archive path.
pub(super) fn merge_member_layers(
    target: &mut DialectLayers,
    member: &DialectLayers,
    member_path: &str,
) -> Vec<LossNote> {
    let mut losses = Vec::new();
    for matched in member.iter() {
        let instance = match matched.instance() {
            Some(nested) => format!("{member_path}/{nested}"),
            None => member_path.to_owned(),
        };
        let matched = matched
            .clone()
            .with_declared_entry(
                cadmpeg_core::nonblank_const!(crate::dialect::DECLARED_ARCHIVE_MEMBER),
                member_path,
            )
            .with_instance(instance);
        if let Err(cadmpeg_core::dialect::DialectLayerError::Duplicate(rejected)) =
            target.insert(matched)
        {
            let format = rejected.format();
            let collision_instance = rejected.instance().unwrap_or("unidentified");
            super::push_loss(&mut losses, F3dLossCode::DialectLayerCollision, format_args!(
                    "archive member {member_path} produced a duplicate {format} dialect layer at instance {collision_instance}; the later layer was omitted"
                ));
        }
    }
    losses
}

fn model_root_member(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    archive_root: &str,
) -> Result<(String, Option<String>), CodecError> {
    if crate::container::is_f3d_name(archive_root) {
        return Ok((archive_root.to_owned(), None));
    }

    let description_bytes = scan.entry_bytes(DESIGN_DESCRIPTION_ENTRY)?;

    let text = std::str::from_utf8(description_bytes).map_err(|error| {
        CodecError::malformed(format_args!(
            "{DESIGN_DESCRIPTION_ENTRY} is not valid JSON: {error}"
        ))
    })?;
    let description: DesignDescriptionJson = ctx
        .parse_json(text, "match F3Z derived model reference")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            CodecError::malformed(format_args!(
                "{DESIGN_DESCRIPTION_ENTRY} is not valid JSON: {error}"
            ))
        })?;
    let mut candidates = Vec::new();
    for graph in description.design_description.design_graphs {
        let mut root = None;
        for object in &graph.design_objects {
            let mut is_root = false;
            for id in &graph.root_ids {
                if *id == object.id {
                    is_root = true;
                    break;
                }
            }
            if is_root && object.relative_path == archive_root {
                root = Some(object);
                break;
            }
        }
        let Some(root) = root else {
            continue;
        };
        for object in &graph.design_objects {
            if !object.content_type.eq_ignore_ascii_case("f3d")
                || !crate::container::is_f3d_name(&object.relative_path)
                || !scan.has_entry(&object.relative_path)
            {
                continue;
            }
            let mut derived = false;
            for reference in root
                .references
                .iter()
                .filter(|reference| reference.reference_type == "DERIVED")
            {
                for id in &reference.ids {
                    if *id == object.id {
                        derived = true;
                        break;
                    }
                }
                if derived {
                    break;
                }
            }
            if derived {
                candidates.push(object.relative_path.clone());
            }
        }
    }
    candidates.sort_by(Ord::cmp);
    candidates.dedup();
    match candidates.as_slice() {
        [model_root] => Ok((
            model_root.to_owned(),
            Some(archive_root.to_owned()),
        )),
        _ => Err(CodecError::malformed(format_args!(
            "f3z root member {archive_root} is not an f3d document and has {} unambiguous derived f3d model members",
            candidates.len()
        ))),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn manifest_extensions_preserve_depth_refusal() {
        let bytes = crate::test_support::assembly_test::f3z_archive(
            r#"model.f3d","extension":[[[0]]],"spare":"unused"#,
            &[("model.f3d", b"model")],
        );
        crate::test_support::with_decode_context(|scan_ctx| {
            let root = cadmpeg_core::decode::View::over_retained(&bytes);
            let scan = crate::container::scan(scan_ctx, root).unwrap();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_recursion_depth = 1;
            crate::test_support::with_decode_policy(&policy, |ctx| {
                let error = super::model_root(ctx, &scan).unwrap_err();
                let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                    panic!("manifest scan must refuse");
                };
                assert_eq!(
                    limit.dimension,
                    cadmpeg_core::decode::ResourceDimension::RecursionDepth
                );
                assert_eq!(limit.operation, "parse F3Z manifest JSON");
                assert_eq!(Some(limit), ctx.resource_refusal());
            });
        });
    }
}
