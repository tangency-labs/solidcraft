//! The G^n cross-section of a blend (opt-in, not upstream): the section of order n
//! (contact of order n with the faces along the rolling ball's contact lines, the rails)
//! and thumbweight w between the rails.
//!
//! Not in the pinned upstream: the SolidCraft fork's `source/SMLib/src/SmFilletSection.cpp`
//! (`SmGnSectionFSG`, tangency-labs/smlib@11ea2d9 = 0bd3f4d in the history of d01d7b0, which replaces the fork's
//! `SmBlendCurveCrossSectionFSG`). Here the old blend-curve section stays the default; the
//! G^n section is being ported in slices for an opt-in `UpstreamFixes` flag (#2250).
//!
//! The section, in the plane square to the leader (the centre line) at its parameter s:
//! `V(tau)` is the point of that plane at distance `tau r_i` from face i, `F_i(tau)` its
//! foot on face i (`tau = 1` at the rails), and `T_i(u) = sum_{k<=n} c_ik (u - 1)^k` the
//! order-n Taylor polynomial of `F_i` at `tau = 1`. Then
//!
//! ```text
//! S(t) = f(t) T1(1 - p1(t)) + (1 - f(t)) T2(p2(t)),  t in [0, 1],
//! f(t) = (1-t)^(n+1) / ((1-t)^(n+1) + t^(n+1)),
//! p1(t) = t / (1 - w + w t),  p2(t) = t (1 - w) / (1 - w t).
//! ```
//!
//! Over the common denominator `W(t) = ((1-t)^(n+1) + t^(n+1)) (1 - w + w t)^n (1 - w t)^n`
//! (degree 3n + 1, positive Bernstein coefficients) S is a rational Bezier curve whose
//! control points are linear in the jets `c_ik` ([`form`]).
//!
//! | Rust                                  | Fork (`SmFilletSection.cpp`)                     |
//! |---------------------------------------|--------------------------------------------------|
//! | [`interval::Ivl`] and its operators   | `sm_Ivl`, `sm_Dn`, `sm_Up`, `sm_Whole`, `sm_Checked`, `operator+ - * /` |
//! | [`interval::Ivl::sqrt`], [`interval::Ivl::mag`], [`interval::Ivl::mid`], [`interval::Ivl::hull`] | `sm_Sqrt`, `sm_Mag`, `sm_Mid`, `sm_Hull` |
//! | [`series::GnScalar`]                  | the `T` of the templates (`double`, `sm_Ivl`), `sm_Positive`, `sm_Sqrt` |
//! | [`series::Trunc`], [`series::Ser`]    | `sm_Trunc`, `sm_Ser`                              |
//! | [`series::Ser::constant`], [`series::Ser::add`], [`series::Ser::sub`], [`series::Ser::scale`], [`series::Ser::mul`], [`series::Ser::inv_sqrt`] | `sm_SerConst`, `sm_SerAdd`, `sm_SerSub`, `sm_SerScale`, `sm_SerMul`, `sm_SerInvSqrt` |
//! | [`series::V3`], [`series::VSer`] and its methods | `sm_V3`, `sm_V3From`, `sm_VSer`, `sm_VSerConst`, `sm_VSerAdd`, `sm_VSerSub`, `sm_VSerTimes`, `sm_VSerDot`, `sm_VSerCross` |
//! | [`form::bern_mul`], [`form::bern_pow`] | `sm_BernMul`, `sm_BernPow`                       |
//! | [`form::GnSectionForm`], [`form::gn_form`] | `sm_GnSectionForm`, `sm_GnForm`              |
//! | [`form::hermite_points`], [`form::hermite_knots`] | `sm_HermitePoints`, `sm_HermiteKnots` |
//! | [`station::Partials`], [`station::Nurbs`], [`station::Nurbs::of`] | `sm_Partials`, `sm_Nurbs`, `sm_NurbsOf` |
//! | [`station::span`], [`station::ders_basis`], [`station::nurbs_partials`] | `sm_Span`, `sm_DersBasis`, `sm_NurbsPartials` |
//! | [`station::as_b_spline`], [`station::surface_partials`] | `IsKindOf(SmBSplineSurface_TYPE)` and the cast, `sm_SurfacePartials` |
//! | [`station::GnSystem`], [`station::gn_evaluate`], [`station::gn_jacobian`], [`station::solve4`] | `sm_GnSystem`, `sm_GnEvaluate`, `sm_GnJacobian`, `sm_Solve4` |
//! | [`station::GnStation`], [`station::leader_series`], [`station::gn_solve_station`] | `sm_GnStation`, `sm_LeaderSeries`, `sm_GnSolveStation` |
//! | [`section::GN_MAX_ORDER`], [`section::GN_DEFAULT_THUMBWEIGHT`] | `SM_GN_MAX_ORDER`, `SM_GN_DEFAULT_THUMBWEIGHT` |
//! | [`section::GnSectionFsg`], [`section::GnSectionFsg::new`], [`section::GnSectionFsg::is_valid`] | `SmGnSectionFSG`, its constructor, `IsValid` (and `GetOrder`, `GetThumbweight`, `GetCertifiedBound`, `GetCertifiedSpans` as fields) |
//! | [`section::GnSectionFsg::create_surface`], [`section::GnSectionFsg::create_surface_from_curves`], `build` and `certify` (private) | `CreateSurface`, `CreateSurfaceFromCurves`, `Build` (its certificate block in `certify`) |
//! | [`section::GnSectionFsg::contact_order_at`] | `ContactOrderAt` |
//!
//! Deviations:
//! - **Bounds.** `sm_Trunc`'s orders are checked against the series' fixed size
//!   (`SM_GN_MAXS`, `SM_GN_MAXE`) when a [`series::Trunc`] is made; the fork leaves them
//!   to its callers. An empty Bernstein polynomial (which the fork never forms) multiplies
//!   to an empty one instead of reading out of bounds.
//! - **Order-4 contact (fork out-of-bounds write).** `ContactOrderAt` at order 4 truncates
//!   at `(0, 5)`, one eps power past `sm_Ser`'s storage (`SM_GN_MAXE = 4`): the fork writes
//!   `c[0][5]` over `c[1][0]`, unused at sigma order 0. The series here store one more eps
//!   column ([`series::STORE_E`]) that only that truncation uses, so the values are the
//!   ones the fork's layout gives, without the overrun.
//! - **`std::min`/`std::max`.** Kept as `(b < a) ? b : a` / `(a < b) ? b : a`, not
//!   `f64::min`/`f64::max`, so a NaN operand propagates as in the fork.
//! - **Out-params.** `sm_SerInvSqrt` returns its series; `sm_HermitePoints`,
//!   `sm_HermiteKnots`, `sm_DersBasis`, `sm_NurbsPartials`, `sm_SurfacePartials`,
//!   `sm_GnEvaluate`, `sm_GnJacobian`, `sm_Solve4`, `sm_LeaderSeries` and
//!   `sm_GnSolveStation` return theirs.
//! - **Knots.** `sm_NurbsOf` reads `GetKnotsPointers`' full knot vectors; here they are
//!   expanded from [`crate::nurbs::b_spline_surface::BSplineSurface::knots`]' unique knots and
//!   multiplicities (the same vectors).
//! - **Indices.** The fork's nested vectors are read and written through checked signed
//!   indices (`Mat`, `knot`): an index outside reads 0 (a knot NaN) and is not written; the
//!   fork's indices stay inside.
//! - **Generator state.** `CreateSurface`'s solver argument is replaced by its tolerance;
//!   `Build` stores the certified bound and span count as the fork does (`&mut self`). The
//!   fork's `SM_ERR_MESSAGE` report of them is not logged.

pub mod certificate;
pub mod form;
pub mod interval;
pub mod section;
pub mod series;
pub mod station;
