//! History on solidcore. Nothing is tagged yet (the kernel's attribute copying through an
//! operation is not wired): `tag` returns a copy, and `read` reports what an untagged body has,
//! no origin for any face or edge, with each edge's faces, so a history decodes and says that
//! everything is new.
use std::collections::BTreeSet;
use std::sync::Mutex;

use solidcore::topology::edgeuse::p1::face as edgeuse_face;

use super::history::VERSION;
use super::rs::{body, copy_in, state, status};
use super::{ERR_ARG, ERR_EXCEPTION, Handle};

pub fn tag(b: &Handle, operand: i32) -> Result<Handle, i32> {
    if !(0..1 << 30).contains(&operand) {
        return Err(ERR_ARG);
    }
    let mut s = state();
    copy_in(&mut s, &body(b)).map(|(c, domain)| Handle { brep: Mutex::new(c), domain })
}

pub fn read(h: &Handle) -> Result<Vec<i64>, i32> {
    let _kernel = state();
    let b = body(h);
    let int = |n: usize| i64::try_from(n).map_err(|_| ERR_EXCEPTION);
    let faces: Vec<_> = b.face_list().to_vec();
    let mut out = vec![VERSION, int(faces.len())?, int(b.edge_list().len())?];
    // Every face is new to the operation: change GENERATED, no origins.
    for _ in &faces {
        out.extend([4, 0]);
    }
    for &e in b.edge_list() {
        let mut bounded = BTreeSet::new();
        for &eu in &b.edge(e).map_err(|e| status(&e))?.edgeuses {
            if let Some(f) = edgeuse_face(&b, eu).map_err(|e| status(&e))?
                && let Some(at) = faces.iter().position(|x| *x == f)
            {
                bounded.insert(int(at)?);
            }
        }
        out.push(int(bounded.len())?);
        out.extend(bounded);
        out.push(0);
    }
    Ok(out)
}
