//! A test seam at the runners' file reads: after a reader has built and healed a B-rep,
//! [`after_read`] lets this crate's tests record the edge and vertex tolerances or put
//! other bits in their place. Outside `cfg(test)` it does nothing.
//!
//! The tolerance-substitution replay (`tolsub::replay`, #18230) uses it to show
//! that a golden row moved only through the tolerances a read set: with main's tolerance
//! bits put back on the entities the read changed, the row is main's row bit for bit.
//! The kept-trim cover's raises (tangency-labs/solidcraft#19295) are put back from each
//! read's own record of them (`Brep::trim_cover_raises`) the same way.
//!
//! The seams are the readers the runners share ([`crate::own_reader`]), `corpus.parity`'s
//! `read_breps` and `topology.brep_p10`'s `make_topology_from_data` cases. Each call is one
//! read, numbered from 0 within a case in call order. While a mode is installed
//! ([`active`]), `topology.face_p1`'s cache of recent reads is bypassed.

use solidcore::topology::Brep;

/// Called by every read seam with the B-rep the reader returned, before the runner uses it.
pub(crate) fn after_read(b: &mut Brep) {
    #[cfg(test)]
    state::after_read(b);
    #[cfg(not(test))]
    let _ = b;
}

/// Whether a test has installed a mode: readers that cache a read then read afresh, so
/// the seam sees every read. Always `false` outside `cfg(test)`.
#[cfg(test)]
pub(crate) fn active() -> bool {
    state::active()
}

/// [`active`] outside `cfg(test)`.
#[cfg(not(test))]
pub(crate) fn active() -> bool {
    false
}

#[cfg(test)]
pub(crate) mod replay;

/// Runs `f` with the kept-trim cover's raises (tangency-labs/solidcraft#19295) put back on
/// every read it makes: each raised edge and vertex holds its tolerance before the cover,
/// as on main. For the certificates recorded on the reads before the cover
/// ([`crate::operand_trim_rows`]); the cover's own moves are proved by the replay.
#[cfg(test)]
pub(crate) fn with_trim_cover_put_back<T>(f: impl FnOnce() -> T) -> T {
    // (the mode is removed even when `f` panics, so it cannot reach a later test)
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            if std::thread::panicking() {
                state::set(None);
            }
        }
    }
    state::set(Some(state::Mode::Substitute {
        subs: Vec::new(),
        reads: 0,
        applied: 0,
        already: 0,
        undone: 0,
        failures: Vec::new(),
    }));
    let clear = Clear;
    let out = f();
    drop(clear);
    let Some(state::Mode::Substitute { failures, .. }) = state::take() else {
        panic!("tolsub mode lost");
    };
    assert!(failures.is_empty(), "{failures:?}");
    out
}

#[cfg(test)]
pub(crate) mod state {
    use std::cell::RefCell;

    use solidcore::topology::Brep;

    /// One read's tolerances: `(arena index, bits)` per face, edge and vertex.
    #[derive(Debug, Default, Clone, serde::Serialize)]
    pub(crate) struct Snapshot {
        pub faces: Vec<(u32, u64)>,
        pub edges: Vec<(u32, u64)>,
        pub vertices: Vec<(u32, u64)>,
    }

    /// One substitution: on read `read`, the edge (`vertex == false`) or vertex `index`
    /// must hold `now` (this branch's read) and gets `main`; one that holds `main` already
    /// is left as it is.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct Sub {
        pub read: u32,
        pub vertex: bool,
        pub index: u32,
        pub now: u64,
        pub main: u64,
    }

    /// What the seam does.
    pub(crate) enum Mode {
        /// Append each read's snapshot.
        Record(Vec<Snapshot>),
        /// Apply the substitutions of each read, checking the value each replaces.
        Substitute {
            subs: Vec<Sub>,
            reads: u32,
            applied: usize,
            /// Of `applied`, those that held `main` already.
            already: usize,
            /// The edge and vertex tolerances put back from the reads' kept-trim cover
            /// records (`Brep::trim_cover_raises`), before `subs` apply.
            undone: usize,
            failures: Vec<String>,
        },
    }

    thread_local! {
        static MODE: RefCell<Option<Mode>> = const { RefCell::new(None) };
        /// With [`strip_classified_trims`] on: the reads it took the trims off.
        static STRIP: RefCell<Option<u32>> = const { RefCell::new(None) };
    }

    /// The edge uses (arena index) of the `Fillet/sphere_diff_3cylinders.smb` read that
    /// hold a UV trim on this branch and none on main (#20179: the read's point tests now
    /// classify on their face, and the classification makes its missing trims). The
    /// read has [`CLASSIFIED_READ_EDGEUSES`] edge uses.
    pub(crate) const CLASSIFIED_READ_TRIMS: [u32; 5] = [127, 129, 130, 132, 135];
    /// The edge use count of that read.
    pub(crate) const CLASSIFIED_READ_EDGEUSES: usize = 156;
    /// The vertex uses (arena index) of that read whose cached vertex/face gap main's read
    /// holds and this branch's does not: setting the classified trims cleared it.
    pub(crate) const CLASSIFIED_READ_GAPS: [u32; 5] = [126, 128, 130, 132, 134];
    /// The file's zone tolerance: what every edge and vertex of that read holds unless a
    /// healer or the kept-trim cover raised it.
    pub(crate) const CLASSIFIED_READ_ZONE_TOL: f64 = 1e-5;

    /// Runs `f` with main's read of that file put back on every read of it `f` makes (a
    /// read with that many edge uses whose listed uses all hold a trim): the
    /// [`CLASSIFIED_READ_TRIMS`] taken off, the [`CLASSIFIED_READ_GAPS`] computed again,
    /// and where a healer ran, its raises on those trims' edges and their vertices put back
    /// (`put_back_healed_raises`). Returns the value and how many reads it put back.
    pub(crate) fn strip_classified_trims<T>(f: impl FnOnce() -> T) -> (T, u32) {
        struct Clear;
        impl Drop for Clear {
            fn drop(&mut self) {
                STRIP.with(|s| *s.borrow_mut() = None);
            }
        }
        STRIP.with(|s| *s.borrow_mut() = Some(0));
        let clear = Clear;
        let out = f();
        let n = STRIP.with(|s| s.borrow().unwrap_or(0));
        drop(clear);
        (out, n)
    }

    fn strip(b: &mut Brep) {
        STRIP.with(|s| {
            let mut s = s.borrow_mut();
            let Some(n) = s.as_mut() else {
                return;
            };
            let ids: Vec<_> = b
                .edgeuses()
                .iter()
                .filter(|(id, _)| CLASSIFIED_READ_TRIMS.contains(&id.index()))
                .map(|(id, u)| (id, u.uv_trim_curve.is_some()))
                .collect();
            if b.edgeuses().len() != CLASSIFIED_READ_EDGEUSES
                || ids.len() != CLASSIFIED_READ_TRIMS.len()
                || !ids.iter().all(|&(_, t)| t)
            {
                return;
            }
            put_back_healed_raises(b, &ids);
            for (id, _) in ids {
                if let Ok(u) = b.edgeuse_mut(id) {
                    u.uv_trim_curve = None;
                }
            }
            let gaps: Vec<_> = b
                .vertexuses()
                .iter()
                .filter(|(id, vu)| {
                    CLASSIFIED_READ_GAPS.contains(&id.index()) && !vu.vertex_face_gap.is_init()
                })
                .map(|(id, _)| id)
                .collect();
            assert_eq!(
                gaps.len(),
                CLASSIFIED_READ_GAPS.len(),
                "classified read gaps"
            );
            let k = crate::runner_kernel();
            for vu in gaps {
                solidcore::topology::vertexuse::calc_vertex_face_gap(&k, b, vu, false, true)
                    .unwrap();
            }
            *n += 1;
        });
    }

    /// The healer's raises on the classified trims' edges and their end vertices, back at
    /// [`CLASSIFIED_READ_ZONE_TOL`] (where the healer ran, `topology.brep_p10`'s healed
    /// reads: main's read keeps no trim on those uses and its healer raises none of them).
    /// A raise there that the kept-trim cover recorded (`Brep::trim_cover_raises`) stays:
    /// main's cover makes it too, from the edge's other kept trim. Each tolerance put back
    /// must be a raise, and [`CLASSIFIED_READ_ZONE_TOL`] must first cover every gap cached
    /// on its entity (the edge's uses' edge/face and edge/edge gaps, the vertex's uses'
    /// vertex/face and vertex/edge gaps), so the raised tolerance covers them as well.
    fn put_back_healed_raises(b: &mut Brep, ids: &[(solidcore::topology::EdgeuseId, bool)]) {
        let raises = b.trim_cover_raises().clone();
        let mut edges = Vec::new();
        let mut vertices = Vec::new();
        for &(id, _) in ids {
            let u = b.edgeuse(id).unwrap();
            let e = u.edge.unwrap();
            if b.edge(e).unwrap().tolerance == CLASSIFIED_READ_ZONE_TOL
                || raises.edges.contains_key(&e)
            {
                continue;
            }
            edges.push(e);
            for vu in [u.vertexuse, b.edgeuse(u.ccw.unwrap()).unwrap().vertexuse] {
                vertices.push(b.vertexuse(vu.unwrap()).unwrap().vertex.unwrap());
            }
        }
        vertices.sort();
        vertices.dedup();
        // (each gap is held to the tolerance put back, which is under the raised one: the
        // raised tolerance covers it too)
        let put_back = CLASSIFIED_READ_ZONE_TOL;
        for e in edges {
            let tol = b.edge(e).unwrap().tolerance;
            assert!(
                tol > put_back,
                "edge {e:?} tolerance {tol:e} is not a raise"
            );
            for (_, u) in b.edgeuses().iter().filter(|(_, u)| u.edge == Some(e)) {
                assert!(
                    u.max_edge_face_gap.gap_3d <= put_back,
                    "edge {e:?} face gap over {put_back:e}"
                );
                assert!(
                    u.edge_ccw_edge_gap.gap_3d <= put_back,
                    "edge {e:?} edge gap over {put_back:e}"
                );
            }
            b.edge_mut(e).unwrap().tolerance = put_back;
        }
        for v in vertices {
            let tol = b.vertex(v).unwrap().tolerance;
            if tol == put_back || raises.vertices.contains_key(&v) {
                continue;
            }
            assert!(
                tol > put_back,
                "vertex {v:?} tolerance {tol:e} is not a raise"
            );
            for (_, u) in b.vertexuses().iter().filter(|(_, u)| u.vertex == Some(v)) {
                assert!(
                    u.vertex_face_gap.gap_3d <= put_back,
                    "vertex {v:?} face gap over {put_back:e}"
                );
                assert!(
                    u.vertex_edge_gap.gap_3d <= put_back,
                    "vertex {v:?} edge gap over {put_back:e}"
                );
            }
            b.vertex_mut(v).unwrap().tolerance = put_back;
        }
    }

    /// Installs `mode` for the reads that follow on this thread.
    pub(crate) fn set(mode: Option<Mode>) {
        MODE.with(|m| *m.borrow_mut() = mode);
    }

    /// Whether a mode is installed (or the classified trims are taken off).
    pub(crate) fn active() -> bool {
        MODE.with(|m| m.borrow().is_some()) || STRIP.with(|s| s.borrow().is_some())
    }

    /// Removes and returns the installed mode.
    pub(crate) fn take() -> Option<Mode> {
        MODE.with(|m| m.borrow_mut().take())
    }

    pub(super) fn after_read(b: &mut Brep) {
        strip(b);
        MODE.with(|m| match m.borrow_mut().as_mut() {
            None => {}
            Some(Mode::Record(v)) => v.push(snapshot(b)),
            Some(Mode::Substitute {
                subs,
                reads,
                applied,
                already,
                undone,
                failures,
            }) => {
                let read = *reads;
                *reads += 1;
                // (the kept-trim cover's raises, made after the healer, put back first:
                // each entity must still hold the raised value)
                let raises = b.trim_cover_raises().clone();
                for (e, (before, after)) in raises.edges {
                    match b.edge_mut(e) {
                        Ok(r) if r.tolerance.to_bits() == after.to_bits() => {
                            r.tolerance = before;
                            *undone += 1;
                        }
                        _ => failures.push(format!("read {read}: {e:?} lost its cover raise")),
                    }
                }
                for (v, (before, after)) in raises.vertices {
                    match b.vertex_mut(v) {
                        Ok(r) if r.tolerance.to_bits() == after.to_bits() => {
                            r.tolerance = before;
                            *undone += 1;
                        }
                        _ => failures.push(format!("read {read}: {v:?} lost its cover raise")),
                    }
                }
                for s in subs.iter().filter(|s| s.read == read) {
                    let slot = if s.vertex {
                        let id = b
                            .vertices()
                            .iter()
                            .map(|(id, _)| id)
                            .find(|id| id.index() == s.index);
                        id.and_then(|id| b.vertex_mut(id).ok())
                            .map(|v| &mut v.tolerance)
                    } else {
                        let id = b
                            .edges()
                            .iter()
                            .map(|(id, _)| id)
                            .find(|id| id.index() == s.index);
                        id.and_then(|id| b.edge_mut(id).ok())
                            .map(|e| &mut e.tolerance)
                    };
                    match slot {
                        Some(t) if t.to_bits() == s.now => {
                            *t = f64::from_bits(s.main);
                            *applied += 1;
                        }
                        // (a read of a body written after an earlier substitution, as
                        // `corpus.parity`'s round trip reads back, already holds main's)
                        Some(t) if t.to_bits() == s.main => {
                            *applied += 1;
                            *already += 1;
                        }
                        Some(t) => failures.push(format!("{s:?}: holds {:e}", *t)),
                        None => failures.push(format!("{s:?}: no such entity")),
                    }
                }
            }
        });
    }

    /// The face, edge and vertex tolerances of `b`.
    pub(crate) fn snapshot(b: &Brep) -> Snapshot {
        Snapshot {
            faces: b
                .faces()
                .iter()
                .map(|(id, f)| (id.index(), f.tolerance.to_bits()))
                .collect(),
            edges: b
                .edges()
                .iter()
                .map(|(id, e)| (id.index(), e.tolerance.to_bits()))
                .collect(),
            vertices: b
                .vertices()
                .iter()
                .map(|(id, v)| (id.index(), v.tolerance.to_bits()))
                .collect(),
        }
    }
}
