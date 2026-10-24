// Modified by sldkit/parasolid-kit; see the crate-root PARTIAL_READERS.md.
// SPDX-License-Identifier: Apache-2.0
//! Convert verified native FIN links to the graph builder's start-vertex convention.
//!
//! Called only after an exact, reviewed native hierarchy gate.
//! The unchanged base FIN layout has forward/backward links in refs[2]/refs[3]
//! and a *forward* (end) vertex in refs[4]. The legacy graph/writer convention
//! uses refs[3] as next and refs[4] as start. See PARTIAL_READERS.md for source evidence.
//! Source offsets, read ranges, markers, and byte-ledger hashes are not changed.

use super::topology::Tables;
use std::collections::HashSet;

/// Select the FINs reached by the verified current faces, including boundary
/// dummies. Saved journals may also contain FINs from superseded loops.
///
/// Call only after native BODY/REGION/SHELL recovery. This follows native
/// forward links before normalization. Missing, shared or cyclic ownership
/// returns `false` without changing the table; it never repairs a broken loop.
pub fn retain_face_fins(tables: &mut Tables) -> bool {
    let Some(mut fins) = face_fins(tables) else {
        return false;
    };
    let mut dummies = Vec::new();
    for id in &fins {
        let fin = &tables.coedges[id];
        let Some(other) = tables.coedges.get(&fin.refs[5]) else {
            return false;
        };
        if other.refs.len() != 9 || other.refs[5] != *id {
            return false;
        }
        if !fins.contains(&other.attr) {
            if other.refs[1..4] != [1, 1, 1] {
                return false;
            }
            dummies.push(other.attr);
        }
    }
    fins.extend(dummies);
    tables.coedges.retain(|id, _| fins.contains(id));
    true
}

fn face_fins(tables: &Tables) -> Option<HashSet<u16>> {
    let mut loops = HashSet::new();
    let mut fins = HashSet::new();
    for face in tables.bridges.values() {
        let mut at = *face.refs.get(2)?;
        if at <= 1 {
            return None;
        }
        while at != 1 {
            if !loops.insert(at) {
                return None;
            }
            let loop_ = tables.loops.get(&at)?;
            if loop_.refs.len() != 4 || loop_.refs[2] != face.attr {
                return None;
            }
            let first = loop_.refs[1];
            let mut id = first;
            loop {
                if !fins.insert(id) {
                    return None;
                }
                let fin = tables.coedges.get(&id)?;
                if fin.refs.len() != 9 || fin.refs[1] != at {
                    return None;
                }
                id = fin.refs[2];
                if id == first {
                    break;
                }
            }
            at = loop_.refs[3];
        }
    }
    (!fins.is_empty()).then_some(fins)
}

/// A recognized native profile must not fall back to the known reversed gauge.
/// Retained source streams and read spans remain available when fins are withheld.
pub fn normalize_or_withhold(tables: &mut Tables) -> usize {
    if normalize(tables) {
        return 0;
    }
    let rejected = tables.coedges.len();
    tables.coedges.clear();
    rejected
}

/// Validate the complete visible/dummy fin graph before converting any record.
/// Non-manifold radial rings and incomplete topology remain outside this gate.
pub fn normalize(tables: &mut Tables) -> bool {
    if tables.coedges.is_empty() {
        return false;
    }
    for (&id, fin) in &tables.coedges {
        if fin.refs.len() != 9 || !matches!(fin.marker, Some(b'+' | b'-')) {
            return false;
        }
        let Some(other) = tables.coedges.get(&fin.refs[5]) else {
            return false;
        };
        if other.refs.len() != 9
            || other.refs[5] != id
            || other.refs[6] != fin.refs[6]
            || other.marker == fin.marker
            || !tables.vertex_uses.contains_key(&fin.refs[4])
            || !tables.edge_uses.contains_key(&fin.refs[6])
            || fin.refs[7] > 1
        // Tolerant fin-local curves need their own contract.
        {
            return false;
        }
        if fin.refs[8] > 1 && tables.coedges.get(&fin.refs[8]).and_then(|next| next.refs.get(4)) != Some(&fin.refs[4]) {
            return false;
        }
        if fin.refs[1] <= 1 {
            // A sheet boundary's dummy fin supplies the opposite endpoint.
            // Modern dummy fins may participate in the same-vertex use chain.
            if fin.refs[2] > 1 || fin.refs[3] > 1 {
                return false;
            }
        } else {
            let Some(loop_) = tables.loops.get(&fin.refs[1]) else {
                return false;
            };
            let Some(forward) = tables.coedges.get(&fin.refs[2]) else {
                return false;
            };
            let Some(backward) = tables.coedges.get(&fin.refs[3]) else {
                return false;
            };
            if loop_.refs.get(2).is_none_or(|owner| !tables.bridges.contains_key(owner))
                || forward.refs.get(3) != Some(&id)
                || backward.refs.get(2) != Some(&id)
                || forward.refs.get(1) != Some(&fin.refs[1])
                || backward.refs.get(1) != Some(&fin.refs[1])
                || backward.refs.get(4) != Some(&other.refs[4])
            {
                return false;
            }
        }
    }
    // Compute from the raw table before changing either member of a fin pair.
    let starts = tables.coedges.iter().map(|(&id, fin)| (id, tables.coedges[&fin.refs[5]].refs[4])).collect::<Vec<_>>();
    for (id, start) in starts {
        let Some(fin) = tables.coedges.get_mut(&id) else {
            return false;
        };
        fin.refs.swap(2, 3);
        fin.refs[4] = start;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::partial::topology::Record;

    fn record(attr: u16, refs: Vec<u16>, marker: Option<u8>) -> Record {
        Record { attr, refs, marker, xyz_m: None, xyz_offset: None, owner: None, offset: usize::from(attr) * 100, read_ranges: Vec::new() }
    }

    fn triangle() -> Tables {
        let mut tables = Tables::default();
        tables.bridges.insert(10, record(10, vec![1, 1, 20, 1, 1], Some(b'+')));
        tables.loops.insert(20, record(20, vec![1, 30, 10, 1], None));
        for i in 0..3 {
            let fin = 30 + i;
            let other = 40 + i;
            let end = 50 + i;
            let start = 50 + (i + 2) % 3;
            let edge = 60 + i;
            tables.coedges.insert(fin, record(fin, vec![1, 20, 30 + (i + 1) % 3, 30 + (i + 2) % 3, end, other, edge, 1, 1], Some(b'+')));
            tables.coedges.insert(other, record(other, vec![1, 1, 1, 1, start, fin, edge, 1, 1], Some(b'-')));
            tables.vertex_uses.insert(end, record(end, vec![], None));
            tables.edge_uses.insert(edge, record(edge, vec![fin, 1, 1, 70 + i, 1, 1], None));
        }
        tables
    }

    #[test]
    fn current_faces_select_fins_and_keep_boundary_dummies() {
        let mut tables = triangle();
        // An obsolete journal FIN can reference a removed face and curve.
        tables.coedges.insert(90, record(90, vec![1, 99, 90, 90, 1, 91, 99, 99, 1], Some(b'+')));
        assert!(retain_face_fins(&mut tables));
        assert_eq!(tables.coedges.len(), 6);
        assert!(tables.coedges.contains_key(&40));
        assert!(normalize(&mut tables));
    }

    #[test]
    fn ownership_selection_cannot_hide_broken_current_loops() {
        for (id, field, value) in [(30, 1, 99), (30, 2, 99), (31, 2, 31), (30, 5, 99), (40, 1, 99), (40, 5, 31)] {
            let mut tables = triangle();
            tables.coedges.get_mut(&id).unwrap().refs[field] = value;
            let before = tables.coedges.len();
            assert!(!retain_face_fins(&mut tables), "{id}/{field}");
            assert_eq!(tables.coedges.len(), before);
        }
        for field in [2, 3] {
            let mut tables = triangle();
            tables.loops.get_mut(&20).unwrap().refs[field] = 20;
            assert!(!retain_face_fins(&mut tables));
            assert_eq!(tables.coedges.len(), 6);
        }
    }

    #[test]
    fn converts_forward_links_and_end_vertices_including_dummy_fins() {
        let mut tables = triangle();
        tables.coedges.get_mut(&40).unwrap().refs[8] = 32;
        assert!(normalize(&mut tables));
        let fin = &tables.coedges[&30];
        assert_eq!(fin.refs[2..7], [32, 31, 52, 40, 60]);
        assert_eq!(tables.coedges[&40].refs[4], 50);
        assert_eq!(fin.marker, Some(b'+'));
        assert_eq!(fin.offset, 3000);
    }

    #[test]
    fn broken_links_and_unsupported_curves_never_partially_convert() {
        for (id, field, value) in
            [(30, 2, 99), (30, 3, 31), (30, 4, 99), (30, 5, 30), (40, 6, 61), (40, 2, 30), (40, 8, 31), (30, 7, 99), (31, 1, 99)]
        {
            let mut tables = triangle();
            tables.coedges.get_mut(&id).unwrap().refs[field] = value;
            let before = tables.coedges.iter().map(|(&id, r)| (id, r.refs.clone())).collect::<std::collections::HashMap<_, _>>();
            assert!(!normalize(&mut tables), "{id}/{field}");
            for (id, refs) in before {
                assert_eq!(tables.coedges[&id].refs, refs);
            }
        }
        let mut tables = triangle();
        tables.coedges.get_mut(&40).unwrap().marker = Some(b'+');
        assert!(!normalize(&mut tables));
        assert_eq!(normalize_or_withhold(&mut tables), 6);
        assert!(tables.coedges.is_empty());
        assert_eq!(tables.edge_uses.len(), 3);
    }

    #[test]
    fn opposite_visible_fins_keep_their_senses_and_share_reversed_endpoints() {
        let mut tables = triangle();
        tables.bridges.insert(11, record(11, vec![], Some(b'+')));
        tables.loops.insert(21, record(21, vec![1, 40, 11, 1], None));
        for i in 0..3 {
            let fin = tables.coedges.get_mut(&(40 + i)).unwrap();
            fin.refs[1] = 21;
            fin.refs[2] = 40 + (i + 2) % 3;
            fin.refs[3] = 40 + (i + 1) % 3;
        }
        assert!(normalize(&mut tables));
        assert_eq!(tables.coedges[&30].refs[4], 52);
        assert_eq!(tables.coedges[&40].refs[4], 50);
        assert_eq!(tables.coedges[&30].marker, Some(b'+'));
        assert_eq!(tables.coedges[&40].marker, Some(b'-'));
        assert_eq!(tables.coedges[&40].refs[3], 42);
    }
}
