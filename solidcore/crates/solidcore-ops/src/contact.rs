//! Contact between bodies: whether two bodies meet (share material or touch within a
//! tolerance) and the distance between their boundaries, each decided by the kernel's
//! certified bracket ([`solidcore::query::contact`]). Ops passes the tolerance on as the
//! caller gives it and reads the kernel's verdict; it computes no distance of its own.

use solidcore::geom::certified::Verdict;
use solidcore::kernel::Kernel;
use solidcore::query::contact::{self as c, Contact, Prepared};
use solidcore::topology::brep::p3::brep_copy;
use solidcore::topology::Brep;
use solidcore::Point3;

use crate::body::Body;
use crate::session::{run_on, Answer};
use crate::{Error, Result};

impl Answer for Contact {}
impl Answer for Vec<Contact> {}

/// The bracket of the distance between two bodies' boundaries, at most the width asked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gap {
    /// No point of either boundary is nearer the other than this.
    pub lower: f64,
    /// The distance of the witness points, rounded up.
    pub upper: f64,
    /// A point on each body `upper` apart, when one is known (the first on this body).
    pub witness: Option<[[f64; 3]; 2]>,
}

fn array(p: Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

/// A copy of `body`'s kernel body, the body released at once: the contact test takes its
/// operands mutably (for its classifications' caches) and never edits the caller's, and a
/// body tested against itself is locked only once at a time.
fn copy_of(k: &mut Kernel, body: &Body) -> solidcore::Result<Brep> {
    let b = body.lock();
    brep_copy(k, &b)
}

/// The verdict as an answer: `Yes` meets, `No` does not, `Undecided` refused with the
/// bracket, which names the tolerance that would decide.
fn decided(contact: &Contact, other: usize) -> Result<bool> {
    match contact.verdict {
        Verdict::Yes => Ok(true),
        Verdict::No => Ok(false),
        Verdict::Undecided => Err(Error::ContactUndecided {
            other,
            lower: contact.lower,
            upper: contact.upper,
        }),
    }
}

fn poisoned(bodies: &[&Body]) -> Result<()> {
    if bodies.iter().any(|b| b.is_poisoned()) {
        return Err(Error::Internal(
            "contact: a body was left unfinished by an earlier panic".to_owned(),
        ));
    }
    Ok(())
}

impl Body {
    /// Whether this body and `other` meet at `tol` (the caller's: ops invents none): share
    /// material, or have boundaries within `tol` of each other
    /// ([`solidcore::query::contact::meets`]). Neither body changes; `other` may be this body.
    ///
    /// # Errors
    /// [`Error::ContactUndecided`] when the kernel's bracket does not decide it (`other` 0):
    /// the bracket, never read as either answer; [`Error::Refused`], in the kernel's words, for a
    /// tolerance not finite and positive or a body without faces; those of the session.
    pub fn meets(&self, other: &Body, tol: f64) -> Result<bool> {
        poisoned(&[self, other])?;
        let contact = run_on("contact::meets", &[("a", self), ("b", other)], |k| {
            let mut a = copy_of(k, self)?;
            let mut b = copy_of(k, other)?;
            c::meets(k, &mut a, &mut b, tol)
        })?;
        decided(&contact, 0)
    }

    /// [`Body::meets`] against each of `others`, each with its own tolerance, this body
    /// prepared once (its faces' pieces about the centre of its box,
    /// [`Prepared::about`]).
    ///
    /// # Errors
    /// [`Error::ContactUndecided`] for the first of `others` the kernel's bracket does not
    /// decide, by its index; those of [`Body::meets`].
    pub fn meets_each(&self, others: &[(&Body, f64)]) -> Result<Vec<bool>> {
        let mut bodies: Vec<&Body> = vec![self];
        bodies.extend(others.iter().map(|(b, _)| *b));
        poisoned(&bodies)?;
        let inputs: Vec<(&'static str, &Body)> = bodies.iter().map(|b| ("brep", *b)).collect();
        let contacts = run_on("contact::meets_each", &inputs, |k| {
            let mut a = copy_of(k, self)?;
            let prepared = Prepared::about(k, &a)?;
            let mut contacts = Vec::new();
            for (other, tol) in others {
                let mut b = copy_of(k, other)?;
                contacts.push(c::meets_prepared(k, &mut a, &prepared, &mut b, *tol)?);
            }
            Ok(contacts)
        })?;
        contacts
            .iter()
            .enumerate()
            .map(|(i, contact)| decided(contact, i))
            .collect()
    }

    /// The distance between this body's boundary and `other`'s, bracketed to at most `width`
    /// (relative to its upper end, absolute below 1;
    /// [`solidcore::query::contact::boundary_distance`]).
    ///
    /// # Errors
    /// [`Error::ContactUndecided`] (`other` 0) with the narrowest bracket reached, when the
    /// kernel's work runs out before `width`; [`Error::Refused`], in the kernel's words, for a
    /// width not finite and positive or a body without faces; those of the session.
    pub fn boundary_distance(&self, other: &Body, width: f64) -> Result<Gap> {
        poisoned(&[self, other])?;
        let contact = run_on(
            "contact::boundary_distance",
            &[("a", self), ("b", other)],
            |k| {
                let mut a = copy_of(k, self)?;
                let mut b = copy_of(k, other)?;
                c::boundary_distance(k, &mut a, &mut b, width)
            },
        )?;
        decided(&contact, 0)?;
        Ok(Gap {
            lower: contact.lower,
            upper: contact.upper,
            witness: contact.witness.map(|[p, q]| [array(p), array(q)]),
        })
    }
}
