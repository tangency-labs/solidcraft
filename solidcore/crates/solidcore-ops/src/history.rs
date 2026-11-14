//! Where each face and edge of an operation's result came from, relative to its operand, in
//! the records' order: the identity the kernel carries through its edits, read for one call.
//!
//! Before the call every face and edge of the operand is tagged with its record index (an ops
//! attribute, [`ids::OPS_HISTORY`], which the kernel's splits, merges and copies carry as they
//! carry an application's [`crate::provenance`] data). After it, each result face and edge is
//! read for the tags it carries, and each face for how an offset made it
//! ([`ids::OFFSET_ROLE`]: the offset executive's own relation of the face to what it was made
//! from). Both are removed before the call returns. Nothing is measured or sampled: a face is
//! an offset copy because the kernel made it as one, never because of where it lies.
//!
//! Built for the offset family ([`Made::Offset`], [`Made::Local`]) and usable as it is for any
//! call that edits one body in place ([`Made::Plain`]).

use std::collections::{BTreeMap, BTreeSet};

use solidcore::geom::attribute::{ids, Attribute, Behavior};
use solidcore::topology::face;
use solidcore::{Brep, EntityRef};

use crate::body::Body;
use crate::error::status;
use crate::provenance::{self, Carried, Provenance};
use crate::{Order, Result};

/// The operand faces or edges (record indices) an entity came from: the ops tag.
#[derive(Clone, Debug, PartialEq)]
struct Operand(Vec<usize>);

impl Operand {
    fn joined(&self, other: &Self) -> Self {
        let mut v = self.0.clone();
        v.extend(&other.0);
        v.sort_unstable();
        v.dedup();
        Operand(v)
    }
}

impl Provenance for Operand {
    // ops' own id, below the applications' (`ids::USER_1`): it never goes through
    // `provenance::attach`, whose check is for application data.
    const ID: u64 = ids::OPS_HISTORY;
    fn split(&self, child: Option<&Self>) -> Option<Self> {
        Some(child.map_or_else(|| self.clone(), |c| self.joined(c)))
    }
    fn merge(&self, result: Option<&Self>) -> Option<Self> {
        Some(result.map_or_else(|| self.clone(), |r| self.joined(r)))
    }
    fn coincident(&self, onto: Option<&Self>) -> Option<Self> {
        Some(onto.map_or_else(|| self.clone(), |o| self.joined(o)))
    }
}

/// How the call made its faces, which says how the offset roles are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Made<'a> {
    /// A shell, a thickened sheet or a sheet's offset: a face the offset made as a copy of
    /// an operand face is its offset ([`FaceOrigin::OffsetOf`]), one it made along an
    /// operand edge stands in that edge's place ([`FaceOrigin::AlongEdge`]).
    Offset,
    /// A local operation (faces moved, drafted): every face is rebuilt as a copy of its own,
    /// so the roles say nothing; the operand faces `moved` (record indices) are
    /// [`Change::Modified`].
    Local {
        /// The faces the call moves.
        moved: &'a [usize],
    },
    /// Any other edit of one body: tags only.
    Plain,
}

/// Where a result face came from (operand record indices).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FaceOrigin {
    /// It lies on operand face `0` (kept, split from it, merged from it, or moved as it).
    Face(usize),
    /// It is the offset of operand face `0`.
    OffsetOf(usize),
    /// It was made along operand edge `0` (a thickened sheet's wall).
    AlongEdge(usize),
}

/// What became of a result face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// The one face of its operand face.
    Kept,
    /// Its operand face, moved ([`Made::Local`]).
    Modified,
    /// One of several faces of one operand face.
    Split,
    /// One face of several operand faces.
    Merged,
    /// Made by the call: an offset copy, a wall along an edge, a face at a vertex (with no
    /// origin), or a face no operand face carries over (with no origin).
    Generated,
}

/// One result face's history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceHistory {
    /// What became of it.
    pub change: Change,
    /// Where it came from, sorted; empty for a face made from nothing the operand names.
    pub origins: Vec<FaceOrigin>,
}

/// A result's history, in its record's order ([`Body::to_record`] after the call).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct History {
    /// Each result face's.
    pub faces: Vec<FaceHistory>,
    /// Each result edge's operand edges (record indices), sorted; empty for an edge the call
    /// made (where faces it made meet).
    pub edges: Vec<Vec<usize>>,
}

/// `op`, an edit of `body` in place, run with `body`'s faces and edges tagged by their record
/// indices (`order`: the operand's record order), and the result's [`History`]. The tags and
/// the offset roles are removed before this returns, whether `op` succeeds or not.
///
/// # Errors
/// `op`'s error; the kernel's, from the tags (an `order` that names entities the body does not
/// have) or the result's record.
pub fn recorded(
    body: &Body,
    order: &Order,
    made: Made<'_>,
    op: impl FnOnce() -> Result<()>,
) -> Result<History> {
    tag(body, order)?;
    let done = op().and_then(|()| {
        let (_, out) = body.to_record()?;
        read(&body.lock(), &out, made)
    });
    let cleared = clear(&mut body.lock());
    let history = done?;
    cleared?;
    Ok(history)
}

/// Every face and edge of `order` tagged with its index, after every ops tag the body carried
/// is gone.
fn tag(body: &Body, order: &Order) -> Result<()> {
    let mut b = body.lock();
    clear(&mut b)?;
    let faces = order.faces.iter().map(|&f| EntityRef::Face(f)).enumerate();
    let edges = order.edges.iter().map(|&e| EntityRef::Edge(e)).enumerate();
    for (i, e) in faces.chain(edges) {
        b.add_attribute(
            e,
            Attribute::application(ids::OPS_HISTORY, Behavior::Copy, Carried(Operand(vec![i]))),
        )
        .map_err(status)?;
    }
    Ok(())
}

/// Every ops tag and offset role on `b`'s faces and edges removed.
fn clear(b: &mut Brep) -> Result<()> {
    let entities: Vec<EntityRef> = b
        .face_list()
        .iter()
        .map(|&f| EntityRef::Face(f))
        .chain(b.edge_list().iter().map(|&e| EntityRef::Edge(e)))
        .collect();
    for e in entities {
        let marks: Vec<_> = b
            .entity_attributes(e)
            .into_iter()
            .filter(|(_, a)| matches!(a.attribute_id(), ids::OPS_HISTORY | ids::OFFSET_ROLE))
            .map(|(h, _)| h)
            .collect();
        for h in marks {
            b.remove_attribute(e, h).map_err(status)?;
        }
    }
    Ok(())
}

/// The operand indices `e` carries.
fn tags(b: &Brep, e: EntityRef) -> Vec<usize> {
    provenance::read::<Operand>(b, e).map_or_else(Vec::new, |o| o.0)
}

/// The offset role face `e` carries, if any.
fn role(b: &Brep, e: EntityRef) -> Option<i64> {
    b.entity_attributes(e)
        .into_iter()
        .find(|(_, a)| a.attribute_id() == ids::OFFSET_ROLE)
        .and_then(|(_, a)| a.long_value())
}

/// The result's history, read off its tags and roles (`out`: the result's record order).
fn read(b: &Brep, out: &Order, made: Made<'_>) -> Result<History> {
    let roles = matches!(made, Made::Offset);
    // each face's tags and role; how many result faces without a role carry each tag
    let mut faces: Vec<(Vec<usize>, Option<i64>)> = Vec::new();
    let mut on: BTreeMap<usize, usize> = BTreeMap::new();
    for &f in &out.faces {
        let e = EntityRef::Face(f);
        let role = if roles { role(b, e) } else { None };
        let t = tags(b, e);
        if role.is_none() {
            for &i in &t {
                *on.entry(i).or_default() += 1;
            }
        }
        faces.push((t, role));
    }
    let mut history = Vec::new();
    for (&f, (t, role)) in out.faces.iter().zip(&faces) {
        history.push(match *role {
            Some(ids::OFFSET_ROLE_COPY) => FaceHistory {
                change: Change::Generated,
                origins: t.iter().map(|&i| FaceOrigin::OffsetOf(i)).collect(),
            },
            Some(ids::OFFSET_ROLE_EDGE) => FaceHistory {
                change: Change::Generated,
                origins: along(b, f)?,
            },
            Some(_) => FaceHistory {
                change: Change::Generated,
                origins: Vec::new(),
            },
            None => FaceHistory {
                change: change(t, &on, made),
                origins: t.iter().map(|&i| FaceOrigin::Face(i)).collect(),
            },
        });
    }
    let edges = out
        .edges
        .iter()
        .map(|&e| tags(b, EntityRef::Edge(e)))
        .collect();
    Ok(History {
        faces: history,
        edges,
    })
}

/// What became of a face that lies on its operand faces `t`.
fn change(t: &[usize], on: &BTreeMap<usize, usize>, made: Made<'_>) -> Change {
    match t {
        [] => Change::Generated,
        [i] if on.get(i).copied().unwrap_or(0) > 1 => Change::Split,
        [i] => match made {
            Made::Local { moved } if moved.contains(i) => Change::Modified,
            _ => Change::Kept,
        },
        _ => Change::Merged,
    }
}

/// The operand edge a face made along an edge stands on: the one operand edge its edges carry
/// (a thickened sheet's wall: that edge and its copy), or none where they carry none or
/// several.
fn along(b: &Brep, f: solidcore::topology::FaceId) -> Result<Vec<FaceOrigin>> {
    let mut found: BTreeSet<usize> = BTreeSet::new();
    for e in face::edges(b, f, None, false, None).map_err(status)? {
        found.extend(tags(b, EntityRef::Edge(e)));
    }
    Ok(match found.iter().copied().collect::<Vec<_>>()[..] {
        [e] => vec![FaceOrigin::AlongEdge(e)],
        _ => Vec::new(),
    })
}
