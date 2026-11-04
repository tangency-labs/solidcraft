// SPDX-License-Identifier: Apache-2.0
//! Complete synthetic Pipe streams shared by source, native, and projection tests.

use super::{indexed_header, lp_utf16, push_marked_reference};
use crate::native::F3dNative;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use std::io::{Cursor, Write};

pub(crate) const BULK: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
const META: &str = "FusionAssetName[Active]/Design1/MetaStream.dat";

/// Source-layout perturbations that keep the native relation intact or
/// break it on purpose.
#[derive(Clone, Default)]
pub(crate) struct Layout {
    /// Rotate the scope reference table so the scalar lanes do not lead it.
    pub(crate) shift_lanes: bool,
    /// Renumber the dynamic class tags by rotating the type table.
    pub(crate) renumber: bool,
    /// Append an indexed reference that no Pipe relation claims.
    pub(crate) extra_reference: bool,
}

#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) legacy: bool,
    pub(crate) hollow: bool,
    pub(crate) operation: u32,
    pub(crate) body_role: u32,
    pub(crate) values: [f64; 4],
    pub(crate) raw: [u8; 2],
    pub(crate) variant: bool,
    pub(crate) point_version: Option<u32>,
    pub(crate) layout: Layout,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            legacy: false,
            hollow: false,
            operation: 4,
            body_role: 4,
            values: [1.0, 0.0, 0.6, 0.15],
            raw: [1, 1],
            variant: false,
            point_version: None,
            layout: Layout::default(),
        }
    }
}

pub(crate) struct Fixture {
    pub(crate) bulk: Vec<u8>,
    pub(crate) meta: Vec<u8>,
    pub(crate) records: Vec<(u64, u64)>,
    pub(crate) secondary: Vec<(u64, u64)>,
    pub(crate) lane_starts: [usize; 4],
    pub(crate) table: usize,
}

fn header(bytes: &mut Vec<u8>, class: u32, id: u32) {
    indexed_header(bytes, class.to_string().as_bytes().try_into().unwrap(), id);
    bytes.extend_from_slice(&[0; 8]);
}

impl Fixture {
    pub(crate) fn new(config: Config) -> Self {
        use crate::records::feature::path_features::pipe;
        let tags = if config.layout.renumber {
            [261, 262, 263, 264, 256, 257, 258, 259, 260]
        } else {
            [256, 257, 258, 259, 260, 261, 262, 263, 264]
        };
        let mut refs = vec![100, 103, 106, 109, 200];
        let body = config.operation == 1 || config.operation == 2;
        let mut groups = Vec::new();
        if body {
            groups.push((300_u32, config.body_role, 400_u32));
        }
        groups.push((310, 5, 410));
        for (group, _, member) in &groups {
            refs.extend([*group, *member]);
        }
        if config.point_version.is_some() {
            refs.extend([500, 510]);
        }
        if config.layout.extra_reference {
            refs.push(10);
        }
        if config.layout.shift_lanes {
            refs.rotate_left(4);
        }
        let mut bulk = Vec::new();
        header(&mut bulk, tags[0], 10);
        bulk.extend_from_slice(&[0, u8::from(config.legacy)]);
        if config.legacy {
            bulk.extend_from_slice(&0_u32.to_le_bytes());
        }
        bulk.push(u8::from(config.hollow));
        bulk.extend_from_slice(&config.operation.to_le_bytes());
        bulk.extend_from_slice(&config.raw);
        if config.point_version.is_some() {
            push_marked_reference(&mut bulk, 500);
        }
        bulk.resize(64, 0);
        let table = bulk.len();
        bulk.extend_from_slice(&u32::try_from(refs.len()).unwrap().to_le_bytes());
        for id in &refs {
            push_marked_reference(&mut bulk, *id);
        }
        bulk.extend_from_slice(&u32::MAX.to_le_bytes());
        lp_utf16(&mut bulk, "Pipe");
        let tail = bulk.len();
        bulk.resize(tail + 72, 0);
        bulk[tail..tail + 4].copy_from_slice(&1_u32.to_le_bytes());
        bulk[tail + 30..tail + 34].copy_from_slice(&u32::MAX.to_le_bytes());
        let mut records = vec![(10, 0)];
        let mut secondary = vec![(10, u64_from_index(bulk.len()))];
        header(&mut bulk, tags[1], 10);
        let mut lane_starts = [0; 4];
        for (ordinal, lane_start) in lane_starts.iter_mut().enumerate() {
            let id = 100 + 3 * u32::try_from(ordinal).unwrap();
            let start = bulk.len();
            *lane_start = start;
            records.push((u64::from(id), u64_from_index(start)));
            header(&mut bulk, tags[2], id);
            bulk.extend_from_slice(&[1, 1, 0, 0, 0]);
            push_marked_reference(&mut bulk, 10);
            bulk.extend_from_slice(&u32::try_from(ordinal).unwrap().to_le_bytes());
            bulk.push(0);
            bulk.extend_from_slice(&config.values[ordinal].to_le_bytes());
            push_marked_reference(&mut bulk, id + 1);
            bulk.extend_from_slice(&u32::try_from(ordinal).unwrap().to_le_bytes());
            bulk.extend_from_slice(&[0; 4]);
            push_marked_reference(&mut bulk, 10);
            if config.variant {
                bulk.extend_from_slice(&[1, 1, 0]);
            } else {
                bulk.extend_from_slice(&[0, 0]);
            }
            push_marked_reference(&mut bulk, id + 2);
            bulk.push(0);
            push_marked_reference(&mut bulk, 10);
            assert_eq!(bulk.len() - start, if config.variant { 104 } else { 103 });
            secondary.push((u64::from(id), u64_from_index(bulk.len())));
            header(&mut bulk, tags[1], id);
        }
        records.push((200, u64_from_index(bulk.len())));
        header(&mut bulk, tags[3], 200);
        bulk.extend_from_slice(&0_u32.to_le_bytes());
        bulk.extend_from_slice(&1.0_f64.to_le_bytes());
        secondary.push((200, u64_from_index(bulk.len())));
        header(&mut bulk, tags[1], 200);
        for (id, role, member) in &groups {
            records.push((u64::from(*id), u64_from_index(bulk.len())));
            header(&mut bulk, tags[4], *id);
            bulk.extend_from_slice(&[0, 0]);
            bulk.extend_from_slice(&1_u32.to_le_bytes());
            push_marked_reference(&mut bulk, *member);
            bulk.extend_from_slice(&[0, 0]);
            bulk.extend_from_slice(&0_u32.to_le_bytes());
            bulk.extend_from_slice(&(u64::from(*role) << 32).to_le_bytes());
            bulk.extend_from_slice(&[0; 10]);
            bulk.extend_from_slice(&1_u32.to_le_bytes());
            bulk.extend_from_slice(&0.0_f64.to_le_bytes());
            bulk.extend_from_slice(&1_u32.to_le_bytes());
            push_marked_reference(&mut bulk, id + 2);
            bulk.extend_from_slice(&[0, 0]);
            push_marked_reference(&mut bulk, id + 1);
            bulk.push(0);
            push_marked_reference(&mut bulk, 10);
            secondary.push((u64::from(*id), u64_from_index(bulk.len())));
            header(&mut bulk, tags[1], *id);
            records.push((u64::from(*member), u64_from_index(bulk.len())));
            header(
                &mut bulk,
                if *role == 5 { tags[6] } else { tags[5] },
                *member,
            );
            bulk.extend_from_slice(&[0; 8]);
            secondary.push((u64::from(*member), u64_from_index(bulk.len())));
            header(&mut bulk, tags[1], *member);
        }
        if let Some(version) = config.point_version {
            records.push((500, u64_from_index(bulk.len())));
            header(&mut bulk, tags[8], 500);
            bulk.extend_from_slice(&[0, 0]);
            bulk.extend_from_slice(&0_u32.to_le_bytes());
            bulk.extend_from_slice(&[0; 16]);
            bulk.push(0);
            for coordinate in [0.25_f64, 0.5, 0.75] {
                bulk.extend_from_slice(&coordinate.to_le_bytes());
            }
            bulk.extend_from_slice(&1_u32.to_le_bytes());
            if version == 3 {
                bulk.extend_from_slice(&[0; 24]);
            }
            bulk.extend_from_slice(&1_u32.to_le_bytes());
            push_marked_reference(&mut bulk, 510);
            secondary.push((500, u64_from_index(bulk.len())));
            header(&mut bulk, tags[1], 500);
            records.push((510, u64_from_index(bulk.len())));
            header(&mut bulk, tags[7], 510);
            bulk.extend_from_slice(&[0; 8]);
            secondary.push((510, u64_from_index(bulk.len())));
            header(&mut bulk, tags[1], 510);
        }
        let body_ids = if body { vec![400] } else { Vec::new() };
        let group_ids = groups
            .iter()
            .map(|(id, _, _)| u64::from(*id))
            .collect::<Vec<_>>();
        let point_ids = if config.point_version.is_some() {
            vec![500]
        } else {
            Vec::new()
        };
        let selection_ids = if config.point_version.is_some() {
            vec![510]
        } else {
            Vec::new()
        };
        let mut types = vec![
            (pipe::PIPE_GUID, pipe::PIPE_BASE, 2, "Fusion", &[10_u64][..]),
            (pipe::PIPE_BASE, "", 0, "Fusion", &[][..]),
            (
                pipe::SCALAR_GUID,
                pipe::SCALAR_BASE,
                0,
                "Fusion",
                &[100, 103, 106, 109][..],
            ),
            (
                "9414C87F-8EA3-465D-9F78-FF2FF5E9881F",
                "60F291CB-499B-4AB1-B0E5-340368671724",
                1,
                "Fusion",
                &[200][..],
            ),
            (
                pipe::GROUP_GUID,
                "F8CD0BB3-2CE6-4236-8C1B-6D0638BF215F",
                1,
                "Fusion",
                group_ids.as_slice(),
            ),
            (
                "9716F783-676D-42E0-93F9-EBF273E7C035",
                "6144EA59-3ACA-4172-9F14-A6531E94D69B",
                1,
                "Fusion",
                body_ids.as_slice(),
            ),
            (
                "5A1BF548-241F-46FD-9FB5-E4B05126EB9D",
                "6144EA59-3ACA-4172-9F14-A6531E94D69B",
                1,
                "Fusion",
                &[410][..],
            ),
            (
                "5662F619-2281-4AD9-93DC-1FCFDA1062E7",
                "6144EA59-3ACA-4172-9F14-A6531E94D69B",
                3,
                "Fusion",
                selection_ids.as_slice(),
            ),
            (
                pipe::POINT_GUID,
                pipe::GROUP_GUID,
                config.point_version.unwrap_or(2),
                "Fusion",
                point_ids.as_slice(),
            ),
        ];
        if config.layout.renumber {
            types.rotate_left(4);
        }
        let mut meta = super::streams_test::design_metastream_with_records(&types, &records);
        let at = meta.len() - 20;
        let mut secondary_bytes = Vec::new();
        secondary_bytes.extend_from_slice(&u32::try_from(secondary.len()).unwrap().to_le_bytes());
        for (id, offset) in &secondary {
            secondary_bytes.extend_from_slice(&id.to_le_bytes());
            secondary_bytes.extend_from_slice(&offset.to_le_bytes());
        }
        meta.splice(at..at + 4, secondary_bytes);
        Self {
            bulk,
            meta,
            records,
            secondary,
            lane_starts,
            table,
        }
    }

    pub(crate) fn archive(&self) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
        super::manifest_test::write_synthetic_manifests(&mut zip, stored);
        zip.start_file(BULK, stored).unwrap();
        zip.write_all(&self.bulk).unwrap();
        zip.start_file(META, stored).unwrap();
        zip.write_all(&self.meta).unwrap();
        zip.finish().unwrap().into_inner()
    }

    pub(crate) fn decode(&self, ctx: &DecodeContext<'_>) -> Result<F3dNative, CodecError> {
        self.with_scan(|scan| {
            let mut native = F3dNative {
                design_types: crate::design::decode::meta::decode_types(scan)?,
                ..Default::default()
            };
            let ids = self
                .records
                .iter()
                .map(|(id, _)| (crate::ids::native_scope(BULK), u32::try_from(*id).unwrap()))
                .collect::<Vec<_>>();
            native.design_record_headers =
                crate::design::decode::sketch::decode_related_record_headers(scan, &ids)?;
            native.design_parameter_owners =
                crate::design::decode::parameters::decode_parameter_owners(
                    scan,
                    &[],
                    &native.design_record_headers,
                )?
                .owners;
            native.design_parameter_scopes =
                crate::design::decode::scopes::parameter_scope::decode_parameter_scopes(
                    ctx,
                    scan,
                    &crate::design::decode::scopes::parameter_scope::ParameterScopeSources {
                        entities: &[],
                        types: &native.design_types,
                        parameters: &[],
                        parameter_owners: &native.design_parameter_owners,
                        component_occurrences: &[],
                        recipes: &[],
                    },
                )?
                .scopes;
            native.design_construction_operand_groups =
                crate::design::decode::operands::decode_construction_operand_groups(
                    scan,
                    &mut native.design_parameter_scopes,
                    &native.design_record_headers,
                )?;
            crate::design::decode::scopes::pipe::bind_constructions(
                scan,
                &crate::design::decode::scopes::pipe::PipeSources {
                    types: &native.design_types,
                    headers: &native.design_record_headers,
                    owners: &native.design_parameter_owners,
                    groups: &native.design_construction_operand_groups,
                },
                &mut native.design_parameter_scopes,
            )?;
            Ok(native)
        })
    }

    pub(crate) fn with_scan<T>(
        &self,
        f: impl FnOnce(&crate::container::ContainerScan<'_>) -> T,
    ) -> T {
        super::zip_test::with_scan(&self.archive(), f)
    }
}
