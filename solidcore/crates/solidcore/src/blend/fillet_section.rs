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
//! | [`form::hermite_points_sided`], [`form::hermite_knots`] | `sm_HermitePoints`, `sm_HermiteKnots` |
//! | [`station::Partials`], [`station::Nurbs`], [`station::Nurbs::of`] | `sm_Partials`, `sm_Nurbs`, `sm_NurbsOf` |
//! | [`station::span`], [`station::ders_basis`], [`station::nurbs_partials`] | `sm_Span`, `sm_DersBasis`, `sm_NurbsPartials` |
//! | [`station::as_b_spline`], [`station::surface_partials`] | `IsKindOf(SmBSplineSurface_TYPE)` and the cast, `sm_SurfacePartials` |
//! | [`station::GnSystem`], [`station::gn_evaluate`], [`station::gn_jacobian`], [`station::solve4`] | `sm_GnSystem`, `sm_GnEvaluate`, `sm_GnJacobian`, `sm_Solve4` |
//! | [`station::GnStation`], [`station::leader_series`], [`station::gn_solve_station`], [`station::gn_solve_station_counted`] | `sm_GnStation`, `sm_LeaderSeries`, `sm_GnSolveStation` (its `pR` and `lSteps`, 014f66f) |
//! | [`definition::T2`], [`definition::t2_pow`], [`definition::Tv`], [`definition::bern_taylor`], [`definition::StationMemo`], [`definition::evaluate_definition`] | `sm_T2`, `sm_T2Pow`, `sm_TV`, `sm_BernTaylor`, `sm_StationMemo`, `SmProceduralBlend::EvaluateDefinition` (014f66f, c6d9b9a, 0136260) |
//! | [`section::GN_MAX_ORDER`], [`section::GN_DEFAULT_THUMBWEIGHT`], [`section::GN_THUMBWEIGHT_EQUAL_AREA`] | `SM_GN_MAX_ORDER`, `SM_GN_DEFAULT_THUMBWEIGHT`, `SM_GN_THUMBWEIGHT_EQUAL_AREA` (6148773 = 1947a45) |
//! | [`section::GnSectionFsg::equal_area_thumbweight`], [`section::GnSectionFsg::thumbweight_at`], `gn_equal_area_w`, `gn_equal_area_w_solve`, `gn_station_thumbweight` (private) | `EqualAreaThumbweight` (6148773 = 1947a45; b9d6a72, 7102bc5), `ThumbweightAt`, `sm_GnEqualAreaW` (memoized, b8ec238), `sm_GnEqualAreaWSolve` (b8ec238), `sm_GnStationThumbweight` (7102bc5) |
//! | [`section::ThumbweightMemo`], [`section::OperationScope`] | `t_sWMemo` and `SM_GN_W_MEMO`, `SmGnSectionFSG::OperationScope` and `t_lOperationScopes` (b8ec238, smlib#109) |
//! | [`tay::Tay`], [`tay::Ring`], [`tay::TayScalar`], [`tay::tay_sqrt`], [`tay::tay_sin_cos`], [`tay::pow_n`], [`tay::wide`], [`tay::acos_clamped`] | `sm_Tay`, `sm_TayOf`, its operators, the templates' `Z` and `S`, `sm_TaySqrt`, `sm_TaySinCos`, `sm_PowN`, `sm_Wide`, `sm_Acos`, `sm_SinCos` (7102bc5) |
//! | [`tay::gn_area_integrand`], [`tay::gn_area_sum`], [`tay::gn_wedge_cos`], [`tay::gn_half_angle`], [`tay::gn_target`], [`tay::gn_invert`], [`tay::GnAreaCells`] | `sm_GnAreaIntegrand`, `sm_GnAreaSum` (in place of `sm_GnSectionAreaIntegral`), `sm_GnWedgeCos`, `sm_GnHalfAngle`, `sm_GnTarget`, `sm_GnInvert`, `sm_GnAreaCells` (7102bc5; `At`, `sOff`, `sAt` f9b133a) |
//! | [`form::binom`], [`form::GnSectionFormT`], [`form::gn_form_t`], [`form::gn_form_normalized`] | `sm_Binom`, `sm_GnSectionFormT`, `sm_GnFormT`, `sm_GnFormNormalized` (7102bc5) |
//! | [`certificate::GnVarying`], [`certificate::GnRowTerms`], [`certificate::centred`], [`certificate::gn_row_terms_over`], [`certificate::PieceSeries`], [`certificate::gn_fourth_over_series`] | `sm_GnVarying`, `sm_GnRowTerms`, `sm_Centred`, `sm_GnRowTermsOver`, `sm_GnFourthOver`'s `pF`, `pV`, `pFm`, `pVm`, `pdMid` (7102bc5) |
//! | [`section::GnSectionFsg`], [`section::GnSectionFsg::new`], [`section::GnSectionFsg::is_valid`] | `SmGnSectionFSG`, its constructor, `IsValid` (and `GetOrder`, `GetThumbweight`, `GetCertifiedBound`, `GetCertifiedSpans` as fields) |
//! | [`section::GnSectionFsg::create_surface`], [`section::GnSectionFsg::create_surface_from_curves`], `build` and `certify` (private) | `CreateSurface`, `CreateSurfaceFromCurves`, `Build` (its certificate block in `certify`) |
//! | [`section::GnSectionFsg::contact_order_at`] | `ContactOrderAt` |
//! | [`section::ArcSection`], [`section::GnSectionFsg::circle`], [`section::GnSectionFsg::build_with_radius`] | `SmArcSection`, `SmGnSectionFSG(const SmArcSection &)` (3aae1c4), `Build`'s `dRadius` and its order 1 (cc08236) |
//! | [`certificate::arc_rows`], [`certificate::arc_row_terms_over`] | `sm_ArcRows`, `sm_ArcRowTermsOver` (cc08236) |
//! | [`definition::T2Of`], [`definition::TvOf`], [`definition::T2Scalar`] | `sm_T2Of`, `sm_TVT`, `sm_PositiveFinite`, `sm_PowOf` (c5b6bb7: `sm_T2` and `sm_TV` over `double` or `sm_Ivl`) |
//! | [`certificate::SpanAt`], [`certificate::gn_enclose_over`], [`certificate::face_partials_over`] | `sm_GnSpanAt`, `sm_GnEncloseOver`, `sm_FacePartialsOver` (c5b6bb7) |
//! | [`definition_box`] | `sm_GnDefPiece`, `sm_GnDefFace`, `sm_GnDefs`, `sm_DefinitionEnclosable`, `sm_GnDefFaceOf`, `sm_DefPieceAt`, `sm_BernTaylorOver`, `sm_DefinitionPartialsNaive`, `sm_DefinitionPartialsOverBox` (c5b6bb7) |
//! | [`station::feet_on_knots`] | `sm_FeetOnKnots` (c5b6bb7) |
//!
//! Deviations:
//! - **The equal area thumbweight.** Resolved at every station (the fork's 7102bc5), so
//!   a generator's thumbweight is never written: `ContactOrderAt` takes `&self`, and
//!   the solvers' copies of one generator need nothing shared. The
//!   station's `dW` and `dWds` start NaN, as the fork's do.
//! - **The area cells.** `sm_GnAreaCells`' `sHave` and `sI` are one `Option` per cell.
//!   Its sums off the grid and at points count into the kernel's
//!   [`crate::surface::procedural_blend::BlendCounts`], so `Over` and `At` take the kernel.
//! - **The thumbweight memo (b8ec238).** The fork's `thread_local` memo and scope count
//!   are the kernel's (`blend_w_memo`, `blend_operation_scopes`), so the solves that read
//!   them take the kernel; the scope count is a shared atomic, so a fillet session holds
//!   its scope while it takes `&mut Kernel`. The memo keeps what the solve gives, so every
//!   result is the same with or without it.
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
//! - **Cancellation (#2136, #2371).** `Build` polls the kernel's cancel token
//!   ([`crate::kernel::Kernel::check_cancelled`]) once per station (before the
//!   certificate, and again where each station is solved for the span ending there),
//!   once per row or station of the skin's net, and its certificate block once per span, per knot-line secant step and per
//!   halved span; `sm_GnCertifySpan` once per level, piece's end station and piece's bound.
//!   A fired token returns `Cancelled`; one that never fires changes nothing.

pub mod certificate;
pub mod continuation;
pub mod definition;
pub mod definition_box;
pub mod form;
pub mod interval;
pub mod offset_box;
pub mod recertify;
pub mod section;
pub mod series;
pub mod smooth_interior;
pub mod smooth_interior_plan;
pub mod station;
pub mod tay;
