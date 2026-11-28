//! Plate contact: the pair-level decision of Tab and Slot for two plates, the tab plate meeting
//! a plane of the slot plate ([`solidcore::plate_contact::pair_at`]). Ops copies the bodies and
//! hands the plane and the tolerance on as the caller gives them; the kernel reads the faces and
//! decides, and ops does no float maths of its own.

use solidcore::math::Vec2;
use solidcore::plate_contact::{self as pc, FarSide, PairJoint};
pub use solidcore::plate_contact::{Contact, PairSkip, Plane};
use solidcore::prism::profile::{Loop2, Region2};
use solidcore::tab_slot as ts;
pub use solidcore::tab_slot::{stretch_seed, Layout, SlotSkip, StretchSkip, TabSkip};
pub use solidcore::{Point3, Vec3};

use crate::body::Body;
use crate::boolean::{boolean, union_all, BooleanOp};
use crate::contact::{copy_of, poisoned};
use crate::error::status;
use crate::session::{run, run_on, Answer};
use crate::{Error, Result};

/// A pair's answer: the joint, or why there is none.
pub type Pair = core::result::Result<PairJoint, PairSkip>;

impl Answer for Pair {}

/// A pair's answer over several candidate planes: the plane chosen and its joint, or why there
/// is none.
pub type NearPair = core::result::Result<(Plane, PairJoint), PairSkip>;

impl Answer for NearPair {}

/// The plane of a planar face's frame (its origin and in-plane axes, as a body's planar face
/// reads), its normal out of the material ([`Plane::spanned`]).
///
/// # Errors
/// [`crate::Error::Refused`], in the kernel's words, for a non-finite origin or axes that are
/// parallel or not finite.
pub fn face_plane(face: &solidcore_record::Plane) -> Result<Plane> {
    use solidcore_convert::vec3;
    Plane::spanned(vec3(face.origin), vec3(face.x), vec3(face.y)).map_err(status)
}

/// The distinct planes of the planar faces `faces` ([`face_plane`]), in face order
/// ([`pc::distinct_planes`]); a face [`face_plane`] refuses is skipped.
pub fn distinct_face_planes(
    faces: impl IntoIterator<Item = solidcore_record::Plane>,
    tol: f64,
) -> Vec<Plane> {
    pc::distinct_planes(faces.into_iter().filter_map(|f| face_plane(&f).ok()), tol)
}

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
    /// The slot clearance `c` at each end of a tab, along the edge.
    pub clearance: f64,
    /// The slot clearance `c_t` each side of a tab's faces, across the tab plate's thickness.
    pub thickness_clearance: f64,
    /// The least land of S beside a slot.
    pub min_land: f64,
    /// How far a tab reaches past S's far face (negative: short of it, design 6.1).
    pub protrusion: f64,
    /// The slot prism's overshoot `o_S` past each of S's faces (design 7.2).
    pub overshoot: f64,
    /// The seed of the tab width variation, per joint (the caller derives it from its own seed).
    pub seed: u64,
    /// The width variation, a `±` bound on each tab's width (0: every tab is `width` wide).
    pub width_jitter: f64,
    /// The chamfer on the two outer corners of each tab's free end (0: none). The slot is not
    /// chamfered.
    pub tip_chamfer: f64,
}

/// The recipe applied ([`Body::tab_slot_cut`]): T with its tabs joined, S with its slots cut,
/// and the plan they were made from.
#[derive(Debug)]
pub struct TabSlotCut {
    /// The tab plate T with every made tab joined.
    pub tab_plate: Body,
    /// The slot plate S with every made tab's slot cut.
    pub slot_plate: Body,
    /// The strip's run `d` in P's frame.
    pub d: Vec2,
    /// The stretches and tabs, made or skipped.
    pub plan: Vec<StretchPlan>,
}

/// One tab of a stretch: where it starts along the strip, its slot outline in P's frame, and
/// why the slot is not cut (`None`: it is).
#[derive(Clone, Debug, PartialEq)]
pub struct TabPlan {
    /// The tab's start `u_k` along the strip.
    pub start: f64,
    /// The tab's width, the spec's `width` unless varied ([`ts::vary`]).
    pub width: f64,
    /// The slot's laser outline in P's `(x, y)` frame.
    pub slot: Region2,
    /// The first failing check of design 7.4, if any.
    pub skip: Option<SlotSkip>,
    /// The tab's outline in T's `(u, z)` frame (design 7.1), or why it is not made.
    pub tab: core::result::Result<Region2, TabSkip>,
    /// The levels `(w_A, w_B)` of T's faces A and B across the strip, what the tab prism spans.
    pub faces: (f64, f64),
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

impl Answer for FarSide {}

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

    /// [`Body::plate_pair`] over several candidate planes of `slot` (design v2 4.2, 5.3): each is
    /// asked in turn; one joint is taken as it is; of several, the face pick `near` chooses by
    /// [`solidcore::plate_contact::choose_contact`] over each joint's
    /// [`solidcore::plate_contact::contact_distance`], else the pair is skipped
    /// [`PairSkip::SeveralContacts`]. No joint: the first plane's skip. Bodies whose exact boxes
    /// are more than `tol` apart are `no_contact` before any plane is asked. Neither body changes.
    ///
    /// # Errors
    /// Those of [`Body::plate_pair`]; [`crate::Error::Argument`] for no planes.
    pub fn plate_pair_near(
        &self,
        slot: &Body,
        planes: &[Plane],
        near: Option<Point3>,
        tol: f64,
    ) -> Result<NearPair> {
        poisoned(&[self, slot])?;
        if planes.is_empty() {
            return Err(Error::Argument);
        }
        if let Some(gap) = boxes_gap(self, slot, tol)? {
            // Quick reject (design v2 5.2, [m18]): exact boxes more than tol apart never meet on
            // any plane, and a far pair's distance need not be decided to say so.
            let distance = self.boundary_distance(slot, tol).map_or(gap, |g| g.upper);
            return Ok(Err(PairSkip::Contact(Contact::NoContact { distance })));
        }
        run_on(
            "plate_contact::pair_near",
            &[("t", self), ("s", slot)],
            |k| {
                let t = copy_of(k, self)?;
                let s = copy_of(k, slot)?;
                let mut joints = Vec::new();
                let mut first = None;
                for &p in planes {
                    match pc::pair_at(k, &t, &s, p, tol)? {
                        Ok(j) => joints.push((p, j)),
                        Err(why) => {
                            first.get_or_insert(why);
                        }
                    }
                }
                match joints.as_slice() {
                    [] => Ok(Err(first.unwrap_or(PairSkip::SeveralContacts))),
                    [one] => Ok(Ok(*one)),
                    _ => {
                        let Some(q) = near else {
                            return Ok(Err(PairSkip::SeveralContacts));
                        };
                        let mut d = Vec::with_capacity(joints.len());
                        for &(p, _) in &joints {
                            d.push(
                                pc::contact_distance(k, &t, &s, p, q, tol)?
                                    .unwrap_or(f64::INFINITY),
                            );
                        }
                        Ok(match pc::choose_contact(&d, tol) {
                            Some(i) => joints.get(i).copied().ok_or(PairSkip::SeveralContacts),
                            None => Err(PairSkip::SeveralContacts),
                        })
                    }
                }
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

    /// This body as the slot plate S: its far side under one slot `outline` (in `p`'s frame,
    /// [`Body::slot_face`]'s `(u, w)` plane), the thickness `t_S,k` there or why the tab is
    /// skipped ([`solidcore::plate_contact::slot_thickness_under`], design 5.6). It does not
    /// change.
    ///
    /// # Errors
    /// [`crate::Error::Refused`], in the kernel's words, for a `tol` not positive and finite, a
    /// bad plane or loops the placement cannot decide; those of the session.
    pub fn slot_thickness_under(&self, p: Plane, outline: &Loop2, tol: f64) -> Result<FarSide> {
        poisoned(&[self])?;
        run_on("plate_contact::slot_thickness_under", &[("s", self)], |k| {
            let s = copy_of(k, self)?;
            pc::slot_thickness_under(k, &s, p, outline, tol)
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
        match self.plate_pair(slot, p, tol)? {
            Ok(joint) => self.tab_slot_plan_for(slot, p, &joint, spec, tol).map(Ok),
            Err(skip) => Ok(Err(skip)),
        }
    }

    /// [`Body::tab_slot_plan`] for a `joint` already found ([`Body::plate_pair`] of this body
    /// against `slot` on `p`): the run and the stretches with their tabs. Stretch `i` draws its
    /// widths from [`ts::stretch_seed`]`(spec.seed, i)`, so a resize that keeps the stretches
    /// keeps every tab's width offset, while removing or reordering a stretch of this pair
    /// reshuffles the widths of the stretches after it.
    ///
    /// # Errors
    /// Those of [`Body::tab_slot_plan`].
    pub fn tab_slot_plan_for(
        &self,
        slot: &Body,
        p: Plane,
        joint: &PairJoint,
        spec: TabSlotSpec,
        tol: f64,
    ) -> Result<(Vec2, Vec<StretchPlan>)> {
        let d = pc::strip_direction(p, joint.faces.a).map_err(status)?;
        let Some(sigma) = self.strip(slot, p, tol)? else {
            return Ok((d, Vec::new()));
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
        for (index, stretch) in pc::stretches(band, &defects, tol)
            .map_err(status)?
            .into_iter()
            .enumerate()
        {
            let starts = ts::place(stretch, spec.layout, spec.width, spec.end_distance, g_min)
                .map_err(status)?;
            let tabs = match starts {
                Err(skip) => Err(skip),
                Ok(starts) => {
                    let sized = ts::vary(
                        stretch,
                        &starts,
                        spec.width,
                        spec.end_distance,
                        g_min,
                        stretch_seed(spec.seed, index),
                        spec.width_jitter,
                    )
                    .map_err(status)?;
                    let mut tabs = Vec::with_capacity(sized.len());
                    for (start, width) in sized {
                        let outline = ts::slot_outline_along(
                            start,
                            width,
                            across,
                            spec.clearance,
                            spec.thickness_clearance,
                            d,
                        )
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
                        let t_k = match slot.slot_thickness_under(p, &outline.outer, tol)? {
                            FarSide::Thickness(t) => Some(t),
                            far => {
                                if skip.is_none() {
                                    skip = Some(if far == FarSide::Uneven {
                                        SlotSkip::UnevenFarSide
                                    } else {
                                        SlotSkip::NoFarSide
                                    });
                                }
                                t_s
                            }
                        };
                        let tab = ts::tab_outline_for(
                            start,
                            width,
                            t_k,
                            spec.protrusion,
                            level,
                            spec.tip_chamfer,
                        )
                        .map_err(status)?;
                        tabs.push(TabPlan {
                            start,
                            width,
                            slot: outline,
                            skip,
                            tab,
                            faces: across,
                        });
                    }
                    Ok(tabs)
                }
            };
            out.push(StretchPlan { stretch, tabs });
        }
        Ok((d, out))
    }

    /// [`Body::tab_slot_plan`] applied (design 7): every tab whose slot passes 7.4 and whose
    /// outline is made is extruded across T ([`tab_prism`]) and joined to a copy of this body,
    /// and its slot ([`slot_prism`], S's thickness under `p`, `spec.overshoot` past each face)
    /// cut from a copy of `slot`. A skipped slot makes no tab. The tab-plate cuts of 7.3 (root
    /// dogbones, T-slot cut-outs) belong to router dogbones and bolts; a laser joint has none.
    /// Neither body changes.
    ///
    /// # Errors
    /// Those of [`Body::tab_slot_plan`], of the prisms and of the booleans.
    pub fn tab_slot_cut(
        &self,
        slot: &Body,
        p: Plane,
        spec: TabSlotSpec,
        tol: f64,
    ) -> Result<core::result::Result<TabSlotCut, PairSkip>> {
        let (d, plan) = match self.tab_slot_plan(slot, p, spec, tol)? {
            Ok(planned) => planned,
            Err(skip) => return Ok(Err(skip)),
        };
        let mut tabs = vec![self.copy()?];
        let mut slots = Vec::new();
        for t in plan.iter().filter_map(|st| st.tabs.as_ref().ok()).flatten() {
            let (Ok(outline), None) = (&t.tab, t.skip) else {
                continue;
            };
            let FarSide::Thickness(t_k) = slot.slot_thickness_under(p, &t.slot.outer, tol)? else {
                continue;
            };
            tabs.push(tab_prism(p, d, outline, t.faces)?);
            slots.push(slot_prism(p, &t.slot, t_k, spec.overshoot)?);
        }
        let tab_plate = union_all(tabs)?;
        let slot_plate = if slots.is_empty() {
            slot.copy()?
        } else {
            boolean(slot.copy()?, union_all(slots)?, BooleanOp::Subtract)?.ok_or(Error::Empty)?
        };
        Ok(Ok(TabSlotCut {
            tab_plate,
            slot_plate,
            d,
            plan,
        }))
    }
}

/// The tool faces one pair's cuts bring to each plate's boolean (design v2 6.4, counted against
/// `MAX_TOOL_FACES` and `SLOW_TOOL_FACES` per plate): `(T's, S's)`, over the tabs that
/// [`Body::tab_slot_cut`] makes (tab outline made and slot not skipped). A prism of a region has
/// one face per segment of each of its loops and two caps.
#[must_use]
pub fn tool_faces(plan: &[StretchPlan]) -> (usize, usize) {
    let prism = |r: &Region2| {
        r.holes
            .iter()
            .fold(r.outer.segs.len(), |n, h| n + h.segs.len())
            + 2
    };
    plan.iter()
        .filter_map(|st| st.tabs.as_ref().ok())
        .flatten()
        .filter(|t| t.skip.is_none())
        .filter_map(|t| t.tab.as_ref().ok().map(|tab| (prism(tab), prism(&t.slot))))
        .fold((0, 0), |(a, b), (t, s)| (a + t, b + s))
}

/// Tab `k`'s prism ([`solidcore::plate_contact::tab_prism`]): `outline` (a [`TabPlan`]'s tab,
/// in T's `(u, z)` frame on `p`, `u` along `d`) extruded across T from `faces.0` to `faces.1`.
///
/// # Errors
/// [`crate::Error::Refused`], in the kernel's words, for a bad plane, direction or faces; those of
/// the session.
pub fn tab_prism(p: Plane, d: Vec2, outline: &Region2, faces: (f64, f64)) -> Result<Body> {
    run("plate_contact::tab_prism", |k| {
        pc::tab_prism(k, p, d, outline, faces)
    })
    .map(Body::new)
}

/// Slot `k`'s prism ([`solidcore::plate_contact::slot_prism`]): `outline` (a [`TabPlan`]'s
/// slot, in `p`'s frame) extruded along `p`'s normal from `−t_s − over` to `over`.
///
/// # Errors
/// [`crate::Error::Refused`], in the kernel's words, for a bad plane, thickness or overshoot;
/// those of the session.
pub fn slot_prism(p: Plane, outline: &Region2, t_s: f64, over: f64) -> Result<Body> {
    run("plate_contact::slot_prism", |k| {
        pc::slot_prism(k, p, outline, t_s, over)
    })
    .map(Body::new)
}

/// `r` with every y turned (`y -> -y`), loops reversed to keep the outer counter-clockwise: a
/// region in the frame of a plane read in the frame of the plane with the opposite normal
/// (whose x is the same and whose y is turned).
#[allow(
    clippy::float_arithmetic,
    reason = "a reflection: negation only, exact"
)]
fn flip_y(r: &Region2) -> Region2 {
    use solidcore::prism::profile::{Loop2, Seg2};
    let pt = |q: solidcore::math::Point2| solidcore::math::Point2::new(q.x, -q.y);
    let flip = |l: &Loop2| {
        Loop2 {
            segs: l
                .segs
                .iter()
                .map(|g| match *g {
                    Seg2::Line { a, b } => Seg2::Line { a: pt(a), b: pt(b) },
                    Seg2::Arc {
                        center,
                        radius,
                        start,
                        sweep,
                    } => Seg2::Arc {
                        center: pt(center),
                        radius,
                        start: -start,
                        sweep: -sweep,
                    },
                    Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic {
                        p0: pt(p0),
                        p1: pt(p1),
                        p2: pt(p2),
                        p3: pt(p3),
                    },
                    Seg2::Conic { a, apex, b, w } => Seg2::Conic {
                        a: pt(a),
                        apex: pt(apex),
                        b: pt(b),
                        w,
                    },
                    Seg2::Bezier { n, p } => Seg2::Bezier { n, p: p.map(pt) },
                })
                .collect(),
        }
        .reversed()
    };
    Region2 {
        outer: flip(&r.outer),
        holes: r.holes.iter().map(flip).collect(),
    }
}

/// Why a joint of one feature ([`tab_slot_joints`]) is not made.
#[derive(Clone, Debug, PartialEq)]
pub enum JointSkip {
    /// The pair itself has no joint ([`Body::tab_slot_plan`]).
    Pair(PairSkip),
    /// Its slots meet those of the earlier joint at this index on the same slot plate (design
    /// 5.7 `meets_other_joint`).
    MeetsOtherJoint(usize),
    /// One of its plates already takes the earlier joint at this index in another direction
    /// (design 5.7 `other_direction`): a slot plate's is its `n`, a tab plate's its `m`.
    OtherDirection(usize),
}

impl JointSkip {
    /// The skip's reason id of design v2 11.3: the pair's ([`PairSkip::reason_id`]),
    /// `meets_other_plate_joint` or `other_direction`.
    #[must_use]
    pub const fn reason_id(&self) -> &'static str {
        match self {
            Self::Pair(p) => p.reason_id(),
            Self::MeetsOtherJoint(_) => "meets_other_plate_joint",
            Self::OtherDirection(_) => "other_direction",
        }
    }
}

/// One feature's joints in pick order (design 5.7): each `(t, s, p)` names the tab plate and
/// the slot plate by index into `plates` and the plane P of S. Each joint is planned
/// ([`Body::tab_slot_plan`]); a later joint whose slot outlines meet (by
/// [`solidcore::tab_slot::apart`], `spec.min_land`) a slot of an earlier made joint on the same S
/// and P is skipped, naming that joint; before that, a joint asking a plate for another
/// direction (up to sign, [`solidcore::plate_contact::bearing`]) than an earlier made joint
/// gave it is skipped `other_direction`, naming that joint. The answer has one entry per joint, `None` for a joint
/// that is made. No plate changes. Slots on S's planes of one direction (both faces of S) are
/// compared in one frame; tab-plate cuts are not compared (a laser joint has none).
///
/// # Errors
/// [`crate::Error::Argument`] for a plate index out of range; those of
/// [`Body::tab_slot_plan`] and of the kernel's checks.
pub fn tab_slot_joints(
    plates: &[Body],
    joints: &[(usize, usize, Plane)],
    spec: TabSlotSpec,
    tol: f64,
) -> Result<Vec<Option<JointSkip>>> {
    Ok(plan_joints(plates, joints, &vec![spec; joints.len()], tol)?
        .into_iter()
        .map(|(skip, _)| skip)
        .collect())
}

/// The gap between two bodies' exact boxes ([`Body::bounds`]) when it is more than `tol`
/// ([`solidcore::plate_contact::boxes_gap`]), else `None`.
fn boxes_gap(a: &Body, b: &Body, tol: f64) -> Result<Option<f64>> {
    Ok(pc::boxes_gap(a.bounds()?, b.bounds()?, tol))
}

/// [`tab_slot_joints`] with each made joint's plan (empty for a skipped joint), joint `j` laid
/// out by `specs[j]`.
fn plan_joints(
    plates: &[Body],
    joints: &[(usize, usize, Plane)],
    specs: &[TabSlotSpec],
    tol: f64,
) -> Result<Vec<(Option<JointSkip>, Vec<StretchPlan>)>> {
    if specs.len() != joints.len() {
        return Err(Error::Argument);
    }
    // (joint, slot plate, P, slot outline) of every made joint's slots.
    let mut cuts: Vec<(usize, usize, Plane, Region2)> = Vec::new();
    // The direction each plate takes and the joint that gave it.
    let mut dirs: Vec<Option<(Vec3, usize)>> = vec![None; plates.len()];
    let mut out = Vec::with_capacity(joints.len());
    for (j, (&(t, s, p), &spec)) in joints.iter().zip(specs).enumerate() {
        let (Some(tab), Some(slot)) = (plates.get(t), plates.get(s)) else {
            return Err(Error::Argument);
        };
        if let Some(gap) = boxes_gap(tab, slot, tol)? {
            // Quick reject (design v2 5.2, [m18]): exact boxes more than tol apart.
            let distance = tab.boundary_distance(slot, tol).map_or(gap, |g| g.upper);
            let skip = PairSkip::Contact(Contact::NoContact { distance });
            out.push((Some(JointSkip::Pair(skip)), Vec::new()));
            continue;
        }
        let joint = match tab.plate_pair(slot, p, tol)? {
            Ok(joint) => joint,
            Err(skip) => {
                out.push((Some(JointSkip::Pair(skip)), Vec::new()));
                continue;
            }
        };
        let m = joint.faces.b.normal;
        let (_, plan) = tab.tab_slot_plan_for(slot, p, &joint, spec, tol)?;
        let mut other_direction = None;
        for (plate, dir) in [(s, p.normal), (t, m)] {
            if let Some(Some((had, by))) = dirs.get(plate) {
                if !matches!(
                    pc::bearing(*had, dir).map_err(status)?,
                    pc::Bearing::Same | pc::Bearing::Opposite
                ) {
                    other_direction = Some(*by);
                    break;
                }
            }
        }
        if let Some(by) = other_direction {
            out.push((Some(JointSkip::OtherDirection(by)), Vec::new()));
            continue;
        }
        let mine: Vec<Region2> = plan
            .iter()
            .filter_map(|st| st.tabs.as_ref().ok())
            .flatten()
            .filter(|tp| tp.skip.is_none() && tp.tab.is_ok())
            .map(|tp| tp.slot.clone())
            .collect();
        let mut meets = None;
        'earlier: for (other, _, q, cut) in cuts.iter().filter(|c| c.1 == s) {
            // A cut on the other face of S (opposite normal, same frame x) is read in p's frame
            // with its y turned; a cut on a face in another direction is not compared.
            let cut = if *q == p {
                cut.clone()
            } else {
                match pc::bearing(q.normal, p.normal).map_err(status)? {
                    pc::Bearing::Same => cut.clone(),
                    pc::Bearing::Opposite => flip_y(cut),
                    _ => continue,
                }
            };
            for o in &mine {
                if ts::apart(&[cut.clone(), o.clone()], spec.min_land, tol)
                    .map_err(status)?
                    .is_some()
                {
                    meets = Some(*other);
                    break 'earlier;
                }
            }
        }
        if let Some(other) = meets {
            out.push((Some(JointSkip::MeetsOtherJoint(other)), Vec::new()));
            continue;
        }
        // A joint that cuts nothing (every tab refused) holds no direction on its plates.
        let cuts_something = !mine.is_empty();
        cuts.extend(mine.into_iter().map(|o| (j, s, p, o)));
        for (plate, dir) in [(s, p.normal), (t, m)]
            .into_iter()
            .filter(|_| cuts_something)
        {
            if let Some(d @ None) = dirs.get_mut(plate) {
                *d = Some((dir, j));
            }
        }
        out.push((None, plan));
    }
    Ok(out)
}

/// The most tool faces one plate's boolean takes (design v2 6.4): past it a feature is refused
/// `too_many_tool_faces`. A starting value, measured outside the repository.
pub const MAX_TOOL_FACES: usize = 2_000;

/// Past this many tool faces on one plate the feature warns `slow` (design v2 6.4). A starting
/// value, measured outside the repository.
pub const SLOW_TOOL_FACES: usize = 800;

/// [`tab_slot_joints`] and the tool faces each plate's boolean takes over the feature's made
/// joints ([`tool_faces`]: a joint's tab faces go to its tab plate, its slot faces to its slot
/// plate), one count per entry of `plates`.
///
/// # Errors
/// Those of [`tab_slot_joints`].
pub fn tab_slot_tool_faces(
    plates: &[Body],
    joints: &[(usize, usize, Plane)],
    spec: TabSlotSpec,
    tol: f64,
) -> Result<(Vec<Option<JointSkip>>, Vec<usize>)> {
    tab_slot_tool_faces_each(plates, joints, &vec![spec; joints.len()], tol)
}

/// [`tab_slot_tool_faces`] with a spec per joint (`specs[j]` lays out joint `j`): plate pairs of
/// one feature differ in thickness, so in their default tab width and overshoot, and the skips
/// are decided on the tabs each joint will cut.
///
/// # Errors
/// [`crate::Error::Argument`] when `specs` and `joints` differ in length; those of
/// [`tab_slot_joints`].
pub fn tab_slot_tool_faces_each(
    plates: &[Body],
    joints: &[(usize, usize, Plane)],
    specs: &[TabSlotSpec],
    tol: f64,
) -> Result<(Vec<Option<JointSkip>>, Vec<usize>)> {
    let planned = plan_joints(plates, joints, specs, tol)?;
    let mut faces = vec![0_usize; plates.len()];
    for (&(t, s, _), (_, plan)) in joints.iter().zip(&planned) {
        let (on_t, on_s) = tool_faces(plan);
        for (plate, n) in [(t, on_t), (s, on_s)] {
            if let Some(f) = faces.get_mut(plate) {
                *f = f.saturating_add(n);
            }
        }
    }
    Ok((planned.into_iter().map(|(skip, _)| skip).collect(), faces))
}

/// A plate whose boolean takes too many tool faces (design v2 6.4), from [`check_tool_faces`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolFaceCap {
    /// More than the most: the feature is refused.
    TooMany {
        /// The plate's index.
        plate: usize,
        /// Its tool faces.
        faces: usize,
    },
    /// More than the slow count: the feature is made, with a warning.
    Slow {
        /// The plate's index.
        plate: usize,
        /// Its tool faces.
        faces: usize,
    },
}

impl ToolFaceCap {
    /// The reason id of design v2 11: `too_many_tool_faces` or `slow`.
    #[must_use]
    pub const fn reason_id(&self) -> &'static str {
        match self {
            Self::TooMany { .. } => "too_many_tool_faces",
            Self::Slow { .. } => "slow",
        }
    }
}

/// The plate with the most tool faces (the first of equals) checked against `max` (refused past
/// it) and `slow` (a warning past it), the caller's ([`MAX_TOOL_FACES`], [`SLOW_TOOL_FACES`]):
/// `None` when no plate passes `slow`.
#[must_use]
pub fn check_tool_faces(per_plate: &[usize], max: usize, slow: usize) -> Option<ToolFaceCap> {
    let (plate, faces) = per_plate.iter().copied().enumerate().fold(
        None,
        |best: Option<(usize, usize)>, (i, n)| match best {
            Some((_, m)) if m >= n => best,
            _ => Some((i, n)),
        },
    )?;
    if faces > max {
        Some(ToolFaceCap::TooMany { plate, faces })
    } else if faces > slow {
        Some(ToolFaceCap::Slow { plate, faces })
    } else {
        None
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
            thickness_clearance: 0.1,
            min_land: 0.5,
            protrusion: 1.0,
            overshoot: 0.5,
            seed: 0,
            width_jitter: 0.0,
            tip_chamfer: 0.0,
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
        // Prisms: the tab 10 x 6 x 7 under the wall (z -1..6 world, y 30..36), the slot
        // 10.2 x 6.2 through S with 0.5 past each face (z -0.5..6.5).
        let t = &tabs[0];
        let tab = tab_prism(p, d, t.tab.as_ref().expect("a tab"), t.faces).unwrap();
        let m = tab.mass_properties(1e-9).unwrap();
        assert!((m.volume - 420.0).abs() < 1e-6, "{m:?}");
        let cx = x(t.start) + 5.0 * d.x;
        let want = [cx, 33.0, 2.5];
        for (got, want) in m.centroid.iter().zip(want) {
            assert!((got - want).abs() < 1e-6, "{m:?} {want:?}");
        }
        let slot = slot_prism(p, &t.slot, 6.0, 0.5).unwrap();
        let m = slot.mass_properties(1e-9).unwrap();
        assert!((m.volume - 10.2 * 6.2 * 7.0).abs() < 1e-6, "{m:?}");
        let want = [cx, 33.0, 3.0];
        for (got, want) in m.centroid.iter().zip(want) {
            assert!((got - want).abs() < 1e-6, "{m:?} {want:?}");
        }
        assert!(slot_prism(p, &t.slot, 0.0, 0.5).is_err());
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

    /// Request 3: a seeded width variation. The same seed plans the same widths, another seed
    /// others; every slot is its tab plus the clearance each side, so both plates carry one
    /// layout; every tab stays inside the end distances of the contact.
    #[test]
    fn a_seeded_width_variation_is_deterministic_and_symmetric() {
        let (s, p) = bottom();
        let spec = TabSlotSpec {
            layout: Layout::Count(3),
            width: 10.0,
            end_distance: 5.0,
            clearance: 0.1,
            thickness_clearance: 0.1,
            min_land: 0.5,
            protrusion: 1.0,
            overshoot: 0.5,
            seed: 42,
            width_jitter: 3.0,
            tip_chamfer: 0.0,
        };
        let wall = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let widths = |spec: TabSlotSpec| -> Vec<f64> {
            let (_, plan) = wall.tab_slot_plan(&s, p, spec, TOL).unwrap().unwrap();
            plan.iter()
                .flat_map(|st| st.tabs.as_ref().unwrap().iter().map(|t| t.width))
                .collect()
        };
        let a = widths(spec);
        assert_eq!(a, widths(spec));
        assert_ne!(a, widths(TabSlotSpec { seed: 43, ..spec }));
        assert!(a.iter().any(|w| (w - 10.0).abs() > 1e-9), "{a:?}");
        let (_, plan) = wall.tab_slot_plan(&s, p, spec, TOL).unwrap().unwrap();
        assert!(plan
            .iter()
            .flat_map(|st| st.tabs.as_ref().unwrap())
            .all(|t| t.skip.is_none()));
        let cut = wall.tab_slot_cut(&s, p, spec, TOL).unwrap().expect("joint");
        let v = |b: &Body| b.mass_properties(1e-9).unwrap().volume;
        let sum: f64 = a.iter().sum();
        assert!((v(&cut.tab_plate) - (43200.0 + sum * 6.0 * 7.0)).abs() < 1e-6);
        let slots: f64 = a.iter().map(|w| (w + 0.2) * 6.2 * 6.0).sum();
        assert!((v(&cut.slot_plate) - (96000.0 - slots)).abs() < 1e-6);
    }

    /// Request 3, on a resize: Wall grown from 120 to 130 long (x 20..140 to 20..150), the same
    /// seed, jitter and three tabs, with room enough that no width is clamped. The tabs move
    /// with the ends, but each keeps its width's offset from the nominal 10.
    #[test]
    fn a_resized_plate_keeps_each_tabs_width_offset() {
        let (s, p) = bottom();
        let spec = TabSlotSpec {
            layout: Layout::Count(3),
            width: 10.0,
            end_distance: 5.0,
            clearance: 0.1,
            thickness_clearance: 0.1,
            min_land: 0.5,
            protrusion: 1.0,
            overshoot: 0.5,
            seed: 42,
            width_jitter: 3.0,
            tip_chamfer: 0.0,
        };
        let tabs = |length: f64| -> Vec<(f64, f64)> {
            let wall = make_box([20.0, 30.0, 6.0], [length, 6.0, 60.0]).unwrap();
            let (_, plan) = wall.tab_slot_plan(&s, p, spec, TOL).unwrap().unwrap();
            plan.iter()
                .flat_map(|st| st.tabs.as_ref().unwrap().iter())
                .map(|t| (t.start, t.width - spec.width))
                .collect()
        };
        let short = tabs(120.0);
        let long = tabs(130.0);
        assert_eq!((short.len(), long.len()), (3, 3), "{short:?} {long:?}");
        assert!(short.iter().any(|t| t.1.abs() > 1e-9), "{short:?}");
        assert!((short[2].0 - long[2].0).abs() > 1.0, "{short:?} {long:?}");
        for (a, b) in short.iter().zip(&long) {
            assert!((a.1 - b.1).abs() < 1e-9, "{short:?} {long:?}");
        }
    }

    /// Request 4: a tip chamfer takes a 2 x 2 triangle off each of a tab's two free corners
    /// (4 mm2, 6 mm deep: 24 mm3 a tab); the slot keeps its full rectangle; too big is refused.
    #[test]
    fn a_tip_chamfer_trims_the_tab_and_not_the_slot() {
        let (s, p) = bottom();
        let spec = TabSlotSpec {
            layout: Layout::Count(2),
            width: 10.0,
            end_distance: 5.0,
            clearance: 0.1,
            thickness_clearance: 0.1,
            min_land: 0.5,
            protrusion: 1.0,
            overshoot: 0.5,
            seed: 1,
            width_jitter: 0.0,
            tip_chamfer: 2.0,
        };
        let wall = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let plain = wall
            .tab_slot_cut(
                &s,
                p,
                TabSlotSpec {
                    tip_chamfer: 0.0,
                    ..spec
                },
                TOL,
            )
            .unwrap()
            .expect("joint");
        let cham = wall.tab_slot_cut(&s, p, spec, TOL).unwrap().expect("joint");
        let v = |b: &Body| b.mass_properties(1e-9).unwrap().volume;
        assert!((v(&plain.tab_plate) - v(&cham.tab_plate) - 2.0 * 24.0).abs() < 1e-6);
        assert!((v(&plain.slot_plate) - v(&cham.slot_plate)).abs() < 1e-9);
        // Half the width (5) or more is refused.
        let big = TabSlotSpec {
            tip_chamfer: 5.0,
            ..spec
        };
        assert!(wall.tab_slot_plan(&s, p, big, TOL).is_err());
    }

    /// 16.2 the recipe applied: Wall on Bottom, count 2: T gains two 10 x 6 x 7 tabs (43200 +
    /// 840), S loses two 10.2 x 6.2 slots through its 6 (96000 - 758.88); the inputs are
    /// untouched. Lifted: `no_contact`, nothing made.
    #[test]
    fn the_tab_slot_recipe_applied() {
        let (s, p) = bottom();
        let spec = TabSlotSpec {
            layout: Layout::Count(2),
            width: 10.0,
            end_distance: 5.0,
            clearance: 0.1,
            thickness_clearance: 0.1,
            min_land: 0.5,
            protrusion: 1.0,
            overshoot: 0.5,
            seed: 0,
            width_jitter: 0.0,
            tip_chamfer: 0.0,
        };
        let wall = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let cut = wall.tab_slot_cut(&s, p, spec, TOL).unwrap().expect("joint");
        let v = |b: &Body| b.mass_properties(1e-9).unwrap().volume;
        assert!(
            (v(&cut.tab_plate) - 44040.0).abs() < 1e-6,
            "{}",
            v(&cut.tab_plate)
        );
        let want = 96000.0 - 2.0 * 10.2 * 6.2 * 6.0;
        assert!(
            (v(&cut.slot_plate) - want).abs() < 1e-6,
            "{}",
            v(&cut.slot_plate)
        );
        assert!((v(&wall) - 43200.0).abs() < 1e-6);
        assert!((v(&s) - 96000.0).abs() < 1e-6);
        assert_eq!(
            cut.plan,
            wall.tab_slot_plan(&s, p, spec, TOL).unwrap().unwrap().1
        );
        let lifted = make_box([20.0, 30.0, 10.0], [120.0, 6.0, 60.0]).unwrap();
        assert!(matches!(
            lifted.tab_slot_cut(&s, p, spec, TOL).unwrap(),
            Err(PairSkip::Contact(Contact::NoContact { .. }))
        ));
    }

    /// The two clearances differ: clearance 0.1 along the edge, 0.4 across the thickness. Wall on
    /// Bottom, count 2, 10 wide tabs in the 6 thick Wall: each slot is 10.2 x 6.8 through S's 6.
    #[test]
    fn the_tab_slot_takes_the_clearance_along_and_the_thickness_clearance_across() {
        let (s, p) = bottom();
        let spec = TabSlotSpec {
            layout: Layout::Count(2),
            width: 10.0,
            end_distance: 5.0,
            clearance: 0.1,
            thickness_clearance: 0.4,
            min_land: 0.5,
            protrusion: 1.0,
            overshoot: 0.5,
            seed: 0,
            width_jitter: 0.0,
            tip_chamfer: 0.0,
        };
        let wall = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let cut = wall.tab_slot_cut(&s, p, spec, TOL).unwrap().expect("joint");
        let v = |b: &Body| b.mass_properties(1e-9).unwrap().volume;
        assert!(
            (v(&cut.tab_plate) - 44040.0).abs() < 1e-6,
            "{}",
            v(&cut.tab_plate)
        );
        let want = 96000.0 - 2.0 * (10.0 + 2.0 * 0.1) * (6.0 + 2.0 * 0.4) * 6.0;
        assert!(
            (v(&cut.slot_plate) - want).abs() < 1e-6,
            "{}",
            v(&cut.slot_plate)
        );
    }

    fn spec2() -> TabSlotSpec {
        TabSlotSpec {
            layout: Layout::Count(2),
            width: 10.0,
            end_distance: 5.0,
            clearance: 0.1,
            thickness_clearance: 0.1,
            min_land: 0.5,
            protrusion: 1.0,
            overshoot: 0.5,
            seed: 0,
            width_jitter: 0.0,
            tip_chamfer: 0.0,
        }
    }

    /// 16.2 overlap found and trimmed [M2]: Wall sunk 4 into Bottom (z 2..66) is an overlap joint,
    /// and its plan (strip, plate faces, tabs, slots) is the plan of the same wall already trimmed
    /// to P (z 6..66). Applied, S loses the same two slots.
    #[test]
    fn an_overlap_is_planned_as_the_trimmed_plate() {
        let (s, p) = bottom();
        let sunk = make_box([20.0, 30.0, 2.0], [120.0, 6.0, 64.0]).unwrap();
        let trimmed = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let joint = sunk.plate_pair(&s, p, TOL).unwrap().expect("joint");
        assert_eq!(joint.contact, Contact::Overlap);
        let want = trimmed.plate_pair(&s, p, TOL).unwrap().expect("joint");
        assert_eq!(joint.faces, want.faces);
        assert!(sunk.strip(&s, p, TOL).unwrap().is_some());
        // The section carries float noise (1e-13) the trimmed plate's face does not: the same
        // plan within 1e-9.
        let (d, plan) = sunk
            .tab_slot_plan(&s, p, spec2(), TOL)
            .unwrap()
            .expect("joint");
        let (want_d, want) = trimmed
            .tab_slot_plan(&s, p, spec2(), TOL)
            .unwrap()
            .expect("joint");
        assert_eq!(d, want_d);
        assert_eq!(plan.len(), want.len(), "{plan:?}");
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        for (got, want) in plan.iter().zip(&want) {
            assert!(near(got.stretch.0, want.stretch.0) && near(got.stretch.1, want.stretch.1));
            let (got, want) = (got.tabs.as_ref().unwrap(), want.tabs.as_ref().unwrap());
            assert_eq!(got.len(), want.len());
            for (g, w) in got.iter().zip(want) {
                assert!(near(g.start, w.start), "{g:?} {w:?}");
                assert_eq!(g.skip, w.skip);
                assert!(g.tab.is_ok() && w.tab.is_ok());
                assert!(near(g.faces.0, w.faces.0) && near(g.faces.1, w.faces.1));
            }
        }
        let cut = sunk
            .tab_slot_cut(&s, p, spec2(), TOL)
            .unwrap()
            .expect("joint");
        let v = cut.slot_plate.mass_properties(1e-9).unwrap().volume;
        assert!((v - (96000.0 - 2.0 * 10.2 * 6.2 * 6.0)).abs() < 1e-6, "{v}");
    }

    /// 16.2 none: a wall beside S (x 200..320) meets P nowhere: `no_contact`, 40 apart; the recipe
    /// makes nothing.
    #[test]
    fn a_wall_beside_s_is_no_joint() {
        let (s, p) = bottom();
        let beside = make_box([200.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        match beside.plate_pair(&s, p, TOL).unwrap() {
            Err(PairSkip::Contact(Contact::NoContact { distance })) => {
                assert!((distance - 40.0).abs() < 1e-2, "{distance}");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            beside.tab_slot_cut(&s, p, spec2(), TOL).unwrap(),
            Err(PairSkip::Contact(Contact::NoContact { .. }))
        ));
    }

    /// 16.2 swapped [m10]: Bottom as the tab plate on the wall's foot (z = 6, n = -z) lies face to
    /// face; the wall as tab plate on Bottom's top is a joint, so the pair is `swapped`.
    #[test]
    fn a_plate_under_a_wall_is_swapped() {
        let (bottom, q) = bottom();
        let wall = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let foot = Plane {
            normal: -Vec3::Z,
            offset: -6.0,
        };
        assert!(matches!(
            bottom.plate_pair(&wall, foot, TOL).unwrap(),
            Err(PairSkip::NotSquare(None))
        ));
        assert_eq!(
            bottom.plate_pair_swapped(&wall, foot, q, TOL).unwrap(),
            Err(PairSkip::Swapped)
        );
        assert!(wall
            .plate_pair_swapped(&bottom, q, foot, TOL)
            .unwrap()
            .is_ok());
    }

    /// 16.2 passes through [m22]: Wall sunk through Bottom (z -2..66) overlaps S and runs out of
    /// its far face (z = 0): `passes_through`, and the recipe makes nothing; flush with the far
    /// face (z 0..66) it is an overlap joint, as is the wall sunk 4.
    #[test]
    fn a_wall_through_s_passes_through() {
        let (s, p) = bottom();
        let through = make_box([20.0, 30.0, -2.0], [120.0, 6.0, 68.0]).unwrap();
        assert_eq!(
            through.plate_pair(&s, p, TOL).unwrap(),
            Err(PairSkip::PassesThrough)
        );
        assert!(matches!(
            through.tab_slot_cut(&s, p, spec2(), TOL).unwrap(),
            Err(PairSkip::PassesThrough)
        ));
        let flush = make_box([20.0, 30.0, 0.0], [120.0, 6.0, 66.0]).unwrap();
        let joint = flush.plate_pair(&s, p, TOL).unwrap().expect("joint");
        assert_eq!(joint.contact, Contact::Overlap);
    }

    /// 16.2 two joints on one S, apart: walls at y 30..36 and 60..66 on Bottom are each a joint;
    /// cut one after the other (the second on the first's S), S loses four slots and each wall
    /// gains two tabs.
    #[test]
    fn two_walls_apart_on_one_s() {
        let (s, p) = bottom();
        let a = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let b = make_box([20.0, 60.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let first = a.tab_slot_cut(&s, p, spec2(), TOL).unwrap().expect("joint");
        let second = b
            .tab_slot_cut(&first.slot_plate, p, spec2(), TOL)
            .unwrap()
            .expect("joint");
        let v = |b: &Body| b.mass_properties(1e-9).unwrap().volume;
        let slot = 10.2 * 6.2 * 6.0;
        assert!((v(&second.slot_plate) - (96000.0 - 4.0 * slot)).abs() < 1e-6);
        assert!((v(&first.tab_plate) - 44040.0).abs() < 1e-6);
        assert!((v(&second.tab_plate) - 44040.0).abs() < 1e-6);
        let tabs = |c: &TabSlotCut| {
            c.plan
                .iter()
                .flat_map(|st| st.tabs.as_ref().unwrap())
                .filter(|t| t.skip.is_none())
                .count()
        };
        assert_eq!((tabs(&first), tabs(&second)), (2, 2));
    }

    /// 16.2 several contacts and the face pick: an L of T standing on P and lying against S's
    /// end face x = 160 meets S in two planes; with no `near` that is `several_contacts`; a
    /// `near` on either contact chooses it; one equally near both (10 mm from each) is `several_contacts`.
    #[test]
    fn several_contacts_and_the_face_pick() {
        let (s, p) = bottom();
        let side = Plane {
            normal: Vec3::X,
            offset: 160.0,
        };
        let t = union_all(vec![
            make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap(),
            make_box([140.0, 30.0, 26.0], [20.0, 6.0, 40.0]).unwrap(),
            make_box([160.0, 30.0, -20.0], [60.0, 6.0, 86.0]).unwrap(),
        ])
        .unwrap();
        let planes = [p, side];
        assert_eq!(
            t.plate_pair_near(&s, &planes, None, TOL).unwrap(),
            Err(PairSkip::SeveralContacts)
        );
        let pick = |q: [f64; 3]| {
            t.plate_pair_near(&s, &planes, Some(Point3::new(q[0], q[1], q[2])), TOL)
                .unwrap()
        };
        assert_eq!(pick([80.0, 33.0, 6.0]).unwrap().0, p);
        assert_eq!(pick([160.0, 33.0, 3.0]).unwrap().0, side);
        assert_eq!(pick([150.0, 33.0, 6.0]), Err(PairSkip::SeveralContacts));
        assert_eq!(t.plate_pair_near(&s, &[], None, TOL), Err(Error::Argument));
    }

    /// 16.2 two joints on one S meeting (design 5.7 `meets_other_joint`): walls apart (y 30..36
    /// and 60..66) are both made; a wall crossing the first (x 27..33, y 20..100) has a slot
    /// meeting the first wall's, so it is skipped naming joint 0. A pair with no joint keeps its
    /// own skip; a bad plate index is refused.
    #[test]
    fn two_walls_meeting_on_one_s() {
        let (s, p) = bottom();
        let a = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let b = make_box([20.0, 60.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let cross = make_box([27.0, 20.0, 6.0], [6.0, 80.0, 60.0]).unwrap();
        let beside = make_box([200.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let plates = [s, a, b, cross, beside];
        let run = |joints: &[(usize, usize, Plane)]| {
            tab_slot_joints(&plates, joints, spec2(), TOL).unwrap()
        };
        assert_eq!(run(&[(1, 0, p), (2, 0, p)]), vec![None, None]);
        assert_eq!(
            run(&[(1, 0, p), (3, 0, p), (2, 0, p)]),
            vec![None, Some(JointSkip::MeetsOtherJoint(0)), None]
        );
        assert_eq!(
            run(&[(3, 0, p), (1, 0, p)]),
            vec![None, Some(JointSkip::MeetsOtherJoint(0))]
        );
        assert!(matches!(
            run(&[(4, 0, p)]).as_slice(),
            [Some(JointSkip::Pair(PairSkip::Contact(
                Contact::NoContact { .. }
            )))]
        ));
        assert_eq!(
            tab_slot_joints(&plates, &[(9, 0, p)], spec2(), TOL),
            Err(Error::Argument)
        );
    }

    /// 16.2 another direction (design 5.7 `other_direction`): Bottom takes the wall's joint along
    /// Z; a post on Bottom's end face x = 160 asks it for X, so it is skipped naming joint 0. The
    /// wall again on P (Z, the same axis) is not skipped for direction.
    #[test]
    fn a_plate_takes_one_direction() {
        let (s, p) = bottom();
        let side = Plane {
            normal: Vec3::X,
            offset: 160.0,
        };
        let wall = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let post = make_box([160.0, 20.0, 1.0], [40.0, 60.0, 4.0]).unwrap();
        let other = make_box([20.0, 60.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        assert!(post.plate_pair(&s, side, TOL).unwrap().is_ok());
        let plates = [s, wall, post, other];
        let run = |joints: &[(usize, usize, Plane)]| {
            tab_slot_joints(&plates, joints, spec2(), TOL).unwrap()
        };
        assert_eq!(
            run(&[(1, 0, p), (2, 0, side), (3, 0, p)]),
            vec![None, Some(JointSkip::OtherDirection(0)), None]
        );
        assert_eq!(
            run(&[(2, 0, side), (1, 0, p)]),
            vec![None, Some(JointSkip::OtherDirection(0))]
        );
    }

    /// A joint that cuts nothing holds no direction: a wall flush with Bottom's edge y = 100 has
    /// every slot `at_edge`, so the post on x = 160 after it still gets its X joint; the wall
    /// that cuts does hold Z against the post.
    #[test]
    fn a_joint_that_cuts_nothing_takes_no_direction() {
        let (s, p) = bottom();
        let side = Plane {
            normal: Vec3::X,
            offset: 160.0,
        };
        let flush = make_box([20.0, 94.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let post = make_box([160.0, 20.0, 1.0], [40.0, 60.0, 4.0]).unwrap();
        let wall = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let plates = [s, flush, post, wall];
        let run = |joints: &[(usize, usize, Plane)]| {
            tab_slot_joints(&plates, joints, spec2(), TOL).unwrap()
        };
        assert_eq!(run(&[(1, 0, p), (2, 0, side)]), vec![None, None]);
        assert_eq!(
            run(&[(3, 0, p), (2, 0, side)]),
            vec![None, Some(JointSkip::OtherDirection(0))]
        );
    }

    /// 16.2 walls on both faces of a shelf (design 5.7 `meets_other_joint`): a wall hanging
    /// under Bottom (P' z = 0, normal -Z) right under the wall on P cuts the same slots, so it
    /// is skipped naming joint 0; one hanging under y 60..66 is apart.
    #[test]
    fn walls_on_both_faces_of_a_shelf() {
        let (s, p) = bottom();
        let under = Plane {
            normal: -Vec3::Z,
            offset: 0.0,
        };
        let wall = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let below = make_box([20.0, 30.0, -60.0], [120.0, 6.0, 60.0]).unwrap();
        let apart = make_box([20.0, 60.0, -60.0], [120.0, 6.0, 60.0]).unwrap();
        assert!(below.plate_pair(&s, under, TOL).unwrap().is_ok());
        let plates = [s, wall, below, apart];
        let run = |joints: &[(usize, usize, Plane)]| {
            tab_slot_joints(&plates, joints, spec2(), TOL).unwrap()
        };
        assert_eq!(
            run(&[(1, 0, p), (2, 0, under)]),
            vec![None, Some(JointSkip::MeetsOtherJoint(0))]
        );
        assert_eq!(
            run(&[(2, 0, under), (1, 0, p)]),
            vec![None, Some(JointSkip::MeetsOtherJoint(0))]
        );
        assert_eq!(run(&[(1, 0, p), (3, 0, under)]), vec![None, None]);
    }

    /// Design v2 11.3's ids for every [`JointSkip`]; a pair skip keeps the pair's id.
    #[test]
    fn joint_skips_name_their_reason_ids() {
        assert_eq!(JointSkip::Pair(PairSkip::Swapped).reason_id(), "swapped");
        assert_eq!(
            JointSkip::Pair(PairSkip::PassesThrough).reason_id(),
            "passes_through"
        );
        assert_eq!(
            JointSkip::MeetsOtherJoint(0).reason_id(),
            "meets_other_plate_joint"
        );
        assert_eq!(JointSkip::OtherDirection(1).reason_id(), "other_direction");
    }

    /// Design v2 6.4: two made laser tabs bring 6 + 6 faces to each plate; a skipped slot, a
    /// tab too short and a stretch with no tabs bring none.
    #[test]
    fn tool_faces_count_made_tabs() {
        use solidcore::tab_slot::{tab_outline, StretchSkip, TabSkip};
        let tab = |start: f64, skip: Option<SlotSkip>, short: bool| {
            let outline = tab_outline(start, 18.0, 12.0, 0.0).unwrap().unwrap();
            TabPlan {
                start,
                width: 18.0,
                slot: outline.clone(),
                skip,
                tab: if short {
                    Err(TabSkip::TooShort { length: -1.0 })
                } else {
                    Ok(outline)
                },
                faces: (0.0, 6.0),
            }
        };
        let plan = [
            StretchPlan {
                stretch: (0.0, 100.0),
                tabs: Ok(vec![
                    tab(10.0, None, false),
                    tab(40.0, Some(SlotSkip::AtEdge), false),
                    tab(60.0, None, true),
                    tab(80.0, None, false),
                ]),
            },
            StretchPlan {
                stretch: (110.0, 115.0),
                tabs: Err(StretchSkip::TooShort {
                    length: 5.0,
                    need: 24.0,
                    most: 0,
                }),
            },
        ];
        assert_eq!(tool_faces(&plan), (12, 12));
        assert_eq!(tool_faces(&[]), (0, 0));
    }

    /// Design v2 6.4 per plate: two walls apart on Bottom, 2 laser tabs each, bring 6 faces a tab
    /// to each wall and 6 a slot to Bottom (24); a skipped joint brings none. The caps: past
    /// `max` is `too_many_tool_faces`, past `slow` is `slow`, at either is not; the plate with the
    /// most faces (the first of equals) is named.
    #[test]
    fn tool_faces_sum_per_plate_and_cap() {
        let (s, p) = bottom();
        let a = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let b = make_box([20.0, 60.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let cross = make_box([27.0, 20.0, 6.0], [6.0, 80.0, 60.0]).unwrap();
        let plates = [s, a, b, cross];
        let (skips, faces) =
            tab_slot_tool_faces(&plates, &[(1, 0, p), (2, 0, p)], spec2(), TOL).unwrap();
        assert_eq!(skips, vec![None, None]);
        assert_eq!(faces, vec![24, 12, 12, 0]);
        let (skips, faces) =
            tab_slot_tool_faces(&plates, &[(1, 0, p), (3, 0, p)], spec2(), TOL).unwrap();
        assert_eq!(skips, vec![None, Some(JointSkip::MeetsOtherJoint(0))]);
        assert_eq!(faces, vec![12, 12, 0, 0]);
        assert_eq!(
            tab_slot_tool_faces(&plates, &[(9, 0, p)], spec2(), TOL),
            Err(Error::Argument)
        );
        assert_eq!(check_tool_faces(&[24, 12, 12], 2_000, 800), None);
        assert_eq!(check_tool_faces(&[], MAX_TOOL_FACES, SLOW_TOOL_FACES), None);
        assert_eq!(
            check_tool_faces(&[800, 2_000], 2_000, 800).map(|c| c.reason_id()),
            Some("slow")
        );
        assert_eq!(
            check_tool_faces(&[801, 900, 900], 2_000, 800),
            Some(ToolFaceCap::Slow {
                plate: 1,
                faces: 900
            })
        );
        let over = check_tool_faces(&[12, 2_150], MAX_TOOL_FACES, SLOW_TOOL_FACES).unwrap();
        assert_eq!(
            over,
            ToolFaceCap::TooMany {
                plate: 1,
                faces: 2_150
            }
        );
        assert_eq!(over.reason_id(), "too_many_tool_faces");
        assert_eq!(
            check_tool_faces(&[24], 20, 10).map(|c| c.reason_id()),
            Some("too_many_tool_faces")
        );
    }

    /// Design 5.6 per tab: S (0,0,0)-(160,100,12) with x > 80 stepped up to z 6, Wall on its
    /// top, count 2: the tab over the 12 is 13 long and its slot cuts 12, the one over the 6 is
    /// 7 long and cuts 6 (T 43200 + 780 + 420, S 144000 - 10.2 x 6.2 x 18). The step at x = 30,
    /// under the first slot: that tab is `uneven_far_side` and only the other is made.
    #[test]
    fn each_tab_reads_the_slot_thickness_under_it() {
        let stepped = |x: f64| {
            let whole = make_box([0.0, 0.0, 0.0], [160.0, 100.0, 12.0]).unwrap();
            let step = make_box([x, 0.0, 0.0], [160.0 - x, 100.0, 6.0]).unwrap();
            boolean(whole, step, BooleanOp::Subtract).unwrap().unwrap()
        };
        let p = Plane {
            normal: Vec3::Z,
            offset: 12.0,
        };
        let wall = make_box([20.0, 30.0, 12.0], [120.0, 6.0, 60.0]).unwrap();
        let v = |b: &Body| b.mass_properties(1e-9).unwrap().volume;
        let s = stepped(80.0);
        let cut = wall
            .tab_slot_cut(&s, p, spec2(), TOL)
            .unwrap()
            .expect("joint");
        let tabs: Vec<_> = cut
            .plan
            .iter()
            .filter_map(|st| st.tabs.as_ref().ok())
            .flatten()
            .collect();
        assert_eq!(tabs.len(), 2, "{tabs:?}");
        assert!(tabs.iter().all(|t| t.skip.is_none()), "{tabs:?}");
        let far = |t: &TabPlan| s.slot_thickness_under(p, &t.slot.outer, TOL).unwrap();
        assert_eq!(far(tabs[0]), FarSide::Thickness(12.0));
        assert_eq!(far(tabs[1]), FarSide::Thickness(6.0));
        let want = 43200.0 + 780.0 + 420.0;
        assert!(
            (v(&cut.tab_plate) - want).abs() < 1e-6,
            "{}",
            v(&cut.tab_plate)
        );
        let want = 144000.0 - 10.2 * 6.2 * 18.0;
        assert!(
            (v(&cut.slot_plate) - want).abs() < 1e-6,
            "{}",
            v(&cut.slot_plate)
        );
        let s = stepped(30.0);
        let cut = wall
            .tab_slot_cut(&s, p, spec2(), TOL)
            .unwrap()
            .expect("joint");
        let skips: Vec<_> = cut
            .plan
            .iter()
            .filter_map(|st| st.tabs.as_ref().ok())
            .flatten()
            .map(|t| t.skip)
            .collect();
        assert_eq!(skips, vec![Some(SlotSkip::UnevenFarSide), None]);
        assert_eq!(SlotSkip::UnevenFarSide.reason_id(), "uneven_far_side");
        assert_eq!(SlotSkip::NoFarSide.reason_id(), "no_far_side");
        assert!(
            (v(&cut.tab_plate) - 43620.0).abs() < 1e-6,
            "{}",
            v(&cut.tab_plate)
        );
        let want = 160.0 * 100.0 * 12.0 - 130.0 * 100.0 * 6.0 - 10.2 * 6.2 * 6.0;
        assert!(
            (v(&cut.slot_plate) - want).abs() < 1e-6,
            "{}",
            v(&cut.slot_plate)
        );
    }

    /// 16.2 quick reject by boxes (design v2 5.2, [m18]): a wall whose box is 40 from S's is
    /// skipped `no_contact` with its distance before any plate reading; a wall standing on S is
    /// still made.
    #[test]
    fn boxes_apart_are_no_contact_at_once() {
        let (s, p) = bottom();
        let wall = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        let beside = make_box([200.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
        assert_eq!(boxes_gap(&wall, &s, TOL).unwrap(), None);
        let gap = boxes_gap(&beside, &s, TOL).unwrap().unwrap();
        assert!((gap - 40.0).abs() < 1e-6, "{gap}");
        let plates = [s, wall, beside];
        match tab_slot_joints(&plates, &[(1, 0, p), (2, 0, p)], spec2(), TOL)
            .unwrap()
            .as_slice()
        {
            [None, Some(JointSkip::Pair(PairSkip::Contact(Contact::NoContact { distance })))] => {
                assert!((distance - 40.0).abs() < 1e-2, "{distance}");
            }
            other => panic!("{other:?}"),
        }
    }
}
