//! Plate contact: the pair-level decision of Tab and Slot for two plates, the tab plate meeting
//! a plane of the slot plate ([`solidcore::plate_contact::pair_at`]). Ops copies the bodies and
//! hands the plane and the tolerance on as the caller gives them; the kernel reads the faces and
//! decides, and ops does no float maths of its own.

use solidcore::plate_contact::{self as pc, PairJoint, PairSkip, Plane};
use solidcore::prism::profile::Region2;

use crate::body::Body;
use crate::contact::{copy_of, poisoned};
use crate::session::{run_on, Answer};
use crate::Result;

/// A pair's answer: the joint, or why there is none.
pub type Pair = core::result::Result<PairJoint, PairSkip>;

impl Answer for Pair {}
impl Answer for Vec<Region2> {}
impl Answer for Option<Region2> {}

impl Body {
    /// This body as the tab plate T against `slot` (S) on `p`, a plane of one of S's outward
    /// planar faces, at `tol` (the caller's: ops invents none): a joint (how T meets `p` and its
    /// plate faces) or why there is none ([`solidcore::plate_contact::pair_at`]). Neither body
    /// changes.
    ///
    /// # Errors
    /// [`crate::Error::Refused`], in the kernel's words, for a `tol` not positive and finite, a
    /// bad plane, or a contact or overlap the kernel does not decide; those of the session.
    pub fn plate_pair(&self, slot: &Body, p: Plane, tol: f64) -> Result<Pair> {
        poisoned(&[self, slot])?;
        run_on("plate_contact::pair", &[("t", self), ("s", slot)], |k| {
            let t = copy_of(k, self)?;
            let s = copy_of(k, slot)?;
            pc::pair_at(k, &t, &s, p, tol)
        })
    }

    /// [`Body::plate_pair`] with the swap [m10] ([`solidcore::plate_contact::with_swap`]): when
    /// this body is skipped for want of plate faces or as lying face to face on `slot`, `slot` is
    /// tried as the tab plate on `q`, a plane of one of this body's outward planar faces, and a
    /// joint there makes the skip [`PairSkip::Swapped`].
    ///
    /// # Errors
    /// Those of [`Body::plate_pair`], for either way round.
    pub fn plate_pair_swapped(&self, slot: &Body, p: Plane, q: Plane, tol: f64) -> Result<Pair> {
        poisoned(&[self, slot])?;
        run_on(
            "plate_contact::pair_swapped",
            &[("t", self), ("s", slot)],
            |k| {
                let t = copy_of(k, self)?;
                let s = copy_of(k, slot)?;
                let forward = pc::pair_at(k, &t, &s, p, tol)?;
                pc::with_swap(|| Ok(forward), || pc::pair_at(k, &s, &t, q, tol))
            },
        )
    }

    /// This body as the slot plate S: its face region in `p` ([`solidcore::plate_contact::slot_face`],
    /// in the `(u, w)` frame [`Body::plate_pair`] reads), what the slot checks of design 7.4
    /// test a slot outline against. Empty when the body has no face in `p`. It does not change.
    ///
    /// # Errors
    /// [`crate::Error::Refused`], in the kernel's words, for a `tol` not positive and finite or a
    /// bad plane; those of the session.
    pub fn slot_face(&self, p: Plane, tol: f64) -> Result<Vec<Region2>> {
        poisoned(&[self])?;
        run_on("plate_contact::slot_face", &[("s", self)], |k| {
            let s = copy_of(k, self)?;
            pc::slot_face(k, &s, p, tol)
        })
    }

    /// This body as the tab plate T against `slot` (S) on `p`: the strip Σ, T's region meeting
    /// S's face in `p` ([`solidcore::plate_contact::strip`], in [`Body::slot_face`]'s `(u, w)`
    /// frame), what the tabs are laid out along. `None` when T meets no area of S's face there.
    /// Neither body changes.
    ///
    /// # Errors
    /// [`crate::Error::Refused`], in the kernel's words, for a `tol` not positive and finite, a
    /// bad plane, or an overlap the kernel does not decide; those of the session.
    pub fn strip(&self, slot: &Body, p: Plane, tol: f64) -> Result<Option<Region2>> {
        poisoned(&[self, slot])?;
        run_on("plate_contact::strip", &[("t", self), ("s", slot)], |k| {
            let t = copy_of(k, self)?;
            let s = copy_of(k, slot)?;
            pc::strip(k, &t, &s, p, tol)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_box;
    use solidcore::plate_contact::Contact;
    use solidcore::tab_slot::{check_slot, slot_outline, SlotSkip};
    use solidcore::Vec3;

    const TOL: f64 = 1.0e-3;

    /// F's Bottom (0,0,0)-(160,100,6) and P its top (z = 6, n = +z).
    fn bottom() -> (Body, Plane) {
        let b = make_box([0.0, 0.0, 0.0], [160.0, 100.0, 6.0]).unwrap();
        (
            b,
            Plane {
                normal: Vec3::Z,
                offset: 6.0,
            },
        )
    }

    /// 16.2 touch: Wall (20,30,6)-(140,36,66) standing on P is a joint, touching at 0, 6 thick;
    /// lifted to z = 10 it is `no_contact`, 4 apart. Neither body changes; tol 0 is refused.
    #[test]
    fn a_wall_on_p_is_a_touch_joint() {
        let (s, p) = bottom();
        let wall = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let joint = wall.plate_pair(&s, p, TOL).unwrap().expect("joint");
        assert_eq!(joint.contact, Contact::Touch { delta: 0.0 });
        assert!((joint.faces.thickness - 6.0).abs() < 1e-9, "{joint:?}");
        let lifted = make_box([20.0, 30.0, 10.0], [120.0, 6.0, 60.0]).unwrap();
        match lifted.plate_pair(&s, p, TOL).unwrap() {
            Err(PairSkip::Contact(Contact::NoContact { distance })) => {
                assert!((distance - 4.0).abs() < 1e-2, "{distance}");
            }
            other => panic!("{other:?}"),
        }
        assert!(wall.plate_pair(&s, p, 0.0).is_err());
    }

    /// The slot face through ops: one hole-free region; a slot on it is cut, one past x = 160 is
    /// `at_edge`.
    #[test]
    fn the_slot_face_through_ops() {
        let (s, p) = bottom();
        let face = s.slot_face(p, TOL).unwrap();
        assert_eq!(face.len(), 1, "{face:?}");
        assert!(face[0].holes.is_empty());
        let inside = slot_outline(70.0, 10.0, (30.0, 36.0), 0.1).unwrap();
        assert_eq!(check_slot(&inside, &face[0], 0.5, TOL).unwrap(), None);
        let past = slot_outline(155.0, 10.0, (30.0, 36.0), 0.1).unwrap();
        assert_eq!(
            check_slot(&past, &face[0], 0.5, TOL).unwrap(),
            Some(SlotSkip::AtEdge)
        );
        assert!(s.slot_face(p, 0.0).is_err());
    }

    /// The strip through ops: Wall on P gives Σ, u [20, 140], w [30, 36]; with the run from the
    /// joint's face a (`strip_direction`) its band is [20, 140], no defects. Lifted: none.
    #[test]
    fn the_strip_through_ops() {
        let (s, p) = bottom();
        let wall = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let joint = wall.plate_pair(&s, p, TOL).unwrap().expect("joint");
        let sigma = wall.strip(&s, p, TOL).unwrap().expect("a strip");
        let d = pc::strip_direction(p, joint.faces.a).unwrap();
        let (band, defects) = pc::strip_defects(&sigma, d, TOL).unwrap();
        let (lo, hi) = if d.x > 0.0 { band } else { (-band.1, -band.0) };
        assert!(
            (lo - 20.0).abs() < 1e-6 && (hi - 140.0).abs() < 1e-6,
            "{band:?} {d:?}"
        );
        assert!(defects.is_empty(), "{defects:?}");
        let lifted = make_box([20.0, 30.0, 10.0], [120.0, 6.0, 60.0]).unwrap();
        assert!(lifted.strip(&s, p, TOL).unwrap().is_none());
        assert!(wall.strip(&s, p, 0.0).is_err());
    }
}
