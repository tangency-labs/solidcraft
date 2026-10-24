//! Work a blend repeats on one body, done once while the blend runs: the body healed, its
//! edges at a tolerance, and its meshes. Bodies are never changed in place, so the same body gives the same
//! answer; the entries go when the outermost blend returns.

use std::cell::RefCell;
use std::sync::Arc;

use solidcraft_geom::Mesh;

use crate::Result;
use crate::body::{Body, EdgeInfo};

/// Bodies remembered at once: a blend works on its input and a few stages of it.
const KEEP: usize = 4;
/// Meshes remembered at once: those bodies, each at a few tolerances.
const KEEP_MESHES: usize = 8;

#[derive(Default)]
struct Memo {
    healed: Vec<(Body, Body)>,
    edges: Vec<(Body, u64, Arc<Vec<EdgeInfo>>)>,
    meshes: Vec<(Body, [u64; 3], Arc<Mesh>)>,
}

thread_local! {
    static MEMO: RefCell<Option<Memo>> = const { RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    /// Bodies healed and edge sets made on this thread, not taken from the memo (tests count them).
    pub(crate) static MADE: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
    /// Meshes made on this thread, not taken from the memo.
    pub(crate) static MESHED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `f`, remembering healed bodies and edges while it runs (a nested call shares the outer memo).
pub(crate) fn scope<T>(f: impl FnOnce() -> T) -> T {
    struct Off;
    impl Drop for Off {
        fn drop(&mut self) {
            MEMO.with(|m| *m.borrow_mut() = None);
        }
    }
    if MEMO.with(|m| m.borrow().is_some()) {
        return f();
    }
    MEMO.with(|m| *m.borrow_mut() = Some(Memo::default()));
    let _off = Off;
    f()
}

/// `make()`, the healed `body`, unless healed already in this scope.
pub(crate) fn healed(body: &Body, make: impl FnOnce() -> Result<Body>) -> Result<Body> {
    let hit = MEMO.with(|m| m.borrow().as_ref().and_then(|m| m.healed.iter().find(|(k, _)| same(k, body)).map(|(_, v)| v.clone())));
    if let Some(b) = hit {
        return Ok(b);
    }
    #[cfg(test)]
    MADE.with(|n| n.set((n.get().0 + 1, n.get().1)));
    let out = make()?;
    MEMO.with(|m| {
        if let Some(m) = m.borrow_mut().as_mut() {
            keep(&mut m.healed, (body.clone(), out.clone()));
        }
    });
    Ok(out)
}

/// `make()`, the edges of `body` at `tol`, unless made already in this scope.
pub(crate) fn edges(body: &Body, tol: f64, make: impl FnOnce() -> Result<Vec<EdgeInfo>>) -> Result<Arc<Vec<EdgeInfo>>> {
    let key = tol.to_bits();
    let hit = MEMO.with(|m| m.borrow().as_ref().and_then(|m| m.edges.iter().find(|(k, t, _)| *t == key && same(k, body)).map(|(_, _, v)| v.clone())));
    if let Some(e) = hit {
        return Ok(e);
    }
    #[cfg(test)]
    MADE.with(|n| n.set((n.get().0, n.get().1 + 1)));
    let out = Arc::new(make()?);
    MEMO.with(|m| {
        if let Some(m) = m.borrow_mut().as_mut() {
            keep(&mut m.edges, (body.clone(), key, out.clone()));
        }
    });
    Ok(out)
}

/// `make()`, the mesh of `body` with these settings, unless made already in this scope.
pub(crate) fn mesh(body: &Body, key: [u64; 3], make: impl FnOnce() -> Result<Mesh>) -> Result<Arc<Mesh>> {
    let hit =
        MEMO.with(|m| m.borrow().as_ref().and_then(|m| m.meshes.iter().find(|(k, t, _)| *t == key && same(k, body)).map(|(_, _, v)| v.clone())));
    if let Some(mesh) = hit {
        return Ok(mesh);
    }
    #[cfg(test)]
    MESHED.with(|n| n.set(n.get() + 1));
    let out = Arc::new(make()?);
    MEMO.with(|m| {
        if let Some(m) = m.borrow_mut().as_mut() {
            if m.meshes.len() >= KEEP_MESHES {
                m.meshes.remove(0);
            }
            m.meshes.push((body.clone(), key, out.clone()));
        }
    });
    Ok(out)
}

fn keep<T>(entries: &mut Vec<T>, entry: T) {
    if entries.len() >= KEEP {
        entries.remove(0);
    }
    entries.push(entry);
}

/// The same body for everything healing and meshing read: its solid, mesh, split lines, face
/// sources, history and size.
fn same(a: &Body, b: &Body) -> bool {
    fn same_arc<T>(x: &Option<Arc<T>>, y: &Option<Arc<T>>) -> bool {
        match (x, y) {
            (Some(x), Some(y)) => Arc::ptr_eq(x, y),
            (None, None) => true,
            _ => false,
        }
    }
    Arc::ptr_eq(&a.solid, &b.solid)
        && same_arc(&a.mesh, &b.mesh)
        && same_arc(&a.splits, &b.splits)
        && same_arc(&a.face_sources, &b.face_sources)
        && same_arc(&a.history, &b.history)
        && a.regularization_protected == b.regularization_protected
        && a.size.to_bits() == b.size.to_bits()
}
