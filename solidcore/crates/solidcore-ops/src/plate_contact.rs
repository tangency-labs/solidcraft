//! Plate contact: the pair-level decision of Tab and Slot for two plates, the tab plate meeting
//! a plane of the slot plate ([`solidcore::plate_contact::pair_at`]). Ops copies the bodies and
//! hands the plane and the tolerance on as the caller gives them; the kernel reads the faces and
//! decides, and ops does no float maths of its own.

use solidcore::math::Vec2;
use solidcore::plate_contact::{self as pc, Contact, PairJoint, PairSkip, Plane};
use solidcore::prism::profile::Region2;
use solidcore::tab_slot::{self as ts, Layout, SlotSkip, StretchSkip, TabSkip};

use crate::body::Body;
use crate::contact::{copy_of, poisoned};
use crate::error::status;
use crate::session::{run_on, Answer};
use crate::Result;

/// A pair's answer: the joint, or why there is none.
pub type Pair = core::result::Result<PairJoint, PairSkip>;

impl Answer for Pair {}

/// What the Tab and Slot recipe is asked for (design 6, 7): the layout, the tab width `W`, the
/// end distance `e`, the slot clearance `c` and the least land `MIN_LAND`, all the caller's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabSlotSpec {
    /// How the tabs of each stretch are laid out.
    pub layout: Layout,
    /// The tab width `W`.
    pub width: f64,
    /// The end distance `e`.
    pub end_distance: f64,
    /// The slot clearance `c`.
    pub clearance: f64,
    /// The least land of S beside a slot.
    pub min_land: f64,
    /// How far a tab reaches past S's far face (negative: short of it, design 6.1).
    pub protrusion: f64,
}

/// One tab of a stretch: where it starts along the strip, its slot outline in P's frame, and
/// why the slot is not cut (`None`: it is).
#[derive(Clone, Debug, PartialEq)]
pub struct TabPlan {
    /// The tab's start `u_k` along the strip.
    pub start: f64,
    /// The slot's laser outline in P's `(x, y)` frame.
    pub slot: Region2,
    /// The first failing check of design 7.4, if any.
    pub skip: Option<SlotSkip>,
    /// The tab's outline in T's `(u, z)` frame (design 7.1), or why it is not made.
    pub tab: core::result::Result<Region2, TabSkip>,
}

/// One stretch of the strip and its tabs, or why it gets none.
#[derive(Clone, Debug, PartialEq)]
pub struct StretchPlan {
    /// The stretch `[a, b]` along the strip.
    pub stretch: (f64, f64),
    /// Its tabs, or why there are none.
    pub tabs: core::result::Result<Vec<TabPlan>, StretchSkip>,
}

/// The recipe's answer for a pair: the strip's run `d` in P's frame and its stretches, or why
/// the pair has no joint.
pub type TabSlotPlan = core::result::Result<(Vec2, Vec<StretchPlan>), PairSkip>;
impl Answer for Vec<Region2> {}

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

    /// This body as the slot plate S: its thickness under `p`
    /// ([`solidcore::plate_contact::slot_thickness`]), `None` when it has no face below `p`. It
    /// does not change.
    ///
    /// # Errors
    /// [`crate::Error::Refused`], in the kernel's words, for a `tol` not positive and finite or a
    /// bad plane; those of the session.
    pub fn slot_thickness(&self, p: Plane, tol: f64) -> Result<Option<f64>> {
        poisoned(&[self])?;
        run_on("plate_contact::slot_thickness", &[("s", self)], |k| {
            let s = copy_of(k, self)?;
            pc::slot_thickness(k, &s, p, tol)
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

    /// The Tab and Slot recipe for this body as T against `slot` (S) on `p` (design 5 to 7.4):
    /// the pair ([`Body::plate_pair`]), the strip Σ ([`Body::strip`]) and its run
    /// ([`solidcore::plate_contact::strip_direction`] from the joint's face a), its band and
    /// defects, the stretches, the tabs of each ([`solidcore::tab_slot::place`] with the laser
    /// `g_min`), and each slot outline ([`solidcore::tab_slot::slot_outline_along`], across Σ's
    /// width) checked against S's face in `p` ([`Body::slot_face`]): cut when it passes on any
    /// region, else the first region's skip (`at_edge` when S has no face there). Every number
    /// is the kernel's; neither body changes.
    ///
    /// # Errors
    /// Those of the reads it makes and of the kernel's checks of `spec` and `tol`.
    pub fn tab_slot_plan(
        &self,
        slot: &Body,
        p: Plane,
        spec: TabSlotSpec,
        tol: f64,
    ) -> Result<TabSlotPlan> {
        let joint = match self.plate_pair(slot, p, tol)? {
            Ok(joint) => joint,
            Err(skip) => return Ok(Err(skip)),
        };
        let d = pc::strip_direction(p, joint.faces.a).map_err(status)?;
        let Some(sigma) = self.strip(slot, p, tol)? else {
            return Ok(Ok((d, Vec::new())));
        };
        let (band, defects) = pc::strip_defects(&sigma, d, tol).map_err(status)?;
        let across = pc::strip_across(&sigma, d, tol).map_err(status)?;
        let face = slot.slot_face(p, tol)?;
        let t_s = slot.slot_thickness(p, tol)?;
        let level = match joint.contact {
            Contact::Touch { delta } => delta,
            _ => 0.0,
        };
        let g_min = ts::laser_g_min(spec.clearance, spec.min_land).map_err(status)?;
        let mut out = Vec::new();
        for stretch in pc::stretches(band, &defects, tol).map_err(status)? {
            let starts = ts::place(stretch, spec.layout, spec.width, spec.end_distance, g_min)
                .map_err(status)?;
            let tabs = match starts {
                Err(skip) => Err(skip),
                Ok(starts) => {
                    let mut tabs = Vec::with_capacity(starts.len());
                    for start in starts {
                        let outline =
                            ts::slot_outline_along(start, spec.width, across, spec.clearance, d)
                                .map_err(status)?;
                        let mut skip = Some(SlotSkip::AtEdge);
                        for (i, region) in face.iter().enumerate() {
                            let s = ts::check_slot(&outline, region, spec.min_land, tol)
                                .map_err(status)?;
                            if s.is_none() || i == 0 {
                                skip = s;
                            }
                            if s.is_none() {
                                break;
                            }
                        }
                        let tab =
                            ts::tab_outline_for(start, spec.width, t_s, spec.protrusion, level)
                                .map_err(status)?;
                        tabs.push(TabPlan {
                            start,
                            slot: outline,
                            skip,
                            tab,
                        });
                    }
                    Ok(tabs)
                }
            };
            out.push(StretchPlan { stretch, tabs });
        }
        Ok(Ok((d, out)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_box;
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

    /// 16.2 the recipe through ops: Wall (20..140) on Bottom, count 2, W 10, e 5, c 0.1: one
    /// stretch [20, 140], tabs at 25 and 125 (along d; mirrored when d runs -x), both slots cut;
    /// a wall flush with S's edge at y = 100 gets its slots `at_edge`; lifted: `no_contact`.
    #[test]
    fn the_tab_slot_recipe_through_ops() {
        let (s, p) = bottom();
        let spec = TabSlotSpec {
            layout: Layout::Count(2),
            width: 10.0,
            end_distance: 5.0,
            clearance: 0.1,
            min_land: 0.5,
            protrusion: 1.0,
        };
        let wall = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let (d, plan) = wall
            .tab_slot_plan(&s, p, spec, TOL)
            .unwrap()
            .expect("joint");
        assert_eq!(plan.len(), 1, "{plan:?}");
        let tabs = plan[0].tabs.as_ref().expect("tabs");
        assert_eq!(tabs.len(), 2, "{tabs:?}");
        let x = |u: f64| u * d.x;
        let mut xs: Vec<f64> = tabs.iter().map(|t| x(t.start)).collect();
        xs.sort_by(f64::total_cmp);
        let want = if d.x > 0.0 {
            [25.0, 125.0]
        } else {
            [-135.0, -35.0]
        };
        assert!(
            (xs[0] - want[0]).abs() < 1e-6 && (xs[1] - want[1]).abs() < 1e-6,
            "{xs:?} {d:?}"
        );
        assert!(tabs.iter().all(|t| t.skip.is_none()), "{tabs:?}");
        // Tabs: h = t_S + protrusion = 7, from z -7 to the touch level 0, W wide along u.
        assert_eq!(s.slot_thickness(p, TOL).unwrap(), Some(6.0));
        for t in tabs {
            let tab = t.tab.as_ref().expect("a tab");
            let want = ts::tab_outline(t.start, 10.0, 7.0, 0.0).unwrap().unwrap();
            assert_eq!(tab, &want, "{t:?}");
        }
        let short = TabSlotSpec {
            protrusion: -7.0,
            ..spec
        };
        let (_, plan) = wall
            .tab_slot_plan(&s, p, short, TOL)
            .unwrap()
            .expect("joint");
        let tabs = plan[0].tabs.as_ref().expect("tabs");
        assert!(
            tabs.iter()
                .all(|t| matches!(t.tab, Err(TabSkip::TooShort { .. }))),
            "{tabs:?}"
        );
        let past = make_box([20.0, 94.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let (_, plan) = past
            .tab_slot_plan(&s, p, spec, TOL)
            .unwrap()
            .expect("joint");
        let skips: Vec<_> = plan
            .iter()
            .flat_map(|st| st.tabs.as_ref().expect("tabs").iter().map(|t| t.skip))
            .collect();
        assert!(
            !skips.is_empty() && skips.iter().all(|k| *k == Some(SlotSkip::AtEdge)),
            "{plan:?}"
        );
        let lifted = make_box([20.0, 30.0, 10.0], [120.0, 6.0, 60.0]).unwrap();
        assert!(matches!(
            lifted.tab_slot_plan(&s, p, spec, TOL).unwrap(),
            Err(PairSkip::Contact(Contact::NoContact { .. }))
        ));
    }
}
