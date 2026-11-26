#### WR-63. `SmCone::CreateCanonical` overloads misuse their arguments
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `SmCone.cpp:341`-`348` `SmCone::CreateCanonical(Origin, BotRadius, SemiAngleDeg, pOptHeight, bSwapUV, bInsideOut)` (def. `:308`); `SmCone.cpp:392`-`400` `SmCone::CreateCanonical(Origin, BotRadius, TopRadius, Height, ...)` (def. `:375`).
- **What happens:** With a height, the first overload passes `FALSE` as the bounded constructor's `bInsideOut` and the caller's `bInsideOut` as `bMakeNurbGenCurve`, so an inside-out request is dropped. For `Height <= 0` the second overload builds the infinite cone with `TopRadius` as the semi-angle in degrees.
- **Trigger:** `CreateCanonical(O, 1.0, 30.0, &h, cone, FALSE, TRUE)` gives `GetInsideOut() == FALSE`; `CreateCanonical(O, 1.0, 30.0, 0.0, cone)` gives a 30 degree cone.
- **How found:** read during porting; compared by fixtures `canonical_semi_with_height_inside_is_mngc`, `canonical_height_zero_top_is_semi` (`surface.cone_p1`).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Reproduces both.
