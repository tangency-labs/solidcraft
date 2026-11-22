#### DOC-5. `SmApiCreateTaperExtrude` documents 90-180 degrees as an outer draft, but the code takes the side from the angle's sign
- **Severity:** Docs
- **Area:**
- **Location:** `source/SM_API/inc/SmApiBrep.h:318` (`dDraftAngleDeg`: "0-90 inner, 90-180 outer") and `source/SMLib/src/SmPrimitiveCreation.cpp:3609` (the same in the parameter comment of `SmPrimitiveCreation::CreateTaperExtrude`); behaviour at `:3728` and `:3731`. The method comment at `:3564-3565` agrees with the code ("typically 30 degrees for an inner taper, and -30 degrees for an outer taper").
- **What happens:** Only the sign picks the side (`dDraftAngleDeg < 0` gives `SM_OD_RIGHT_HAND_SIDE`, anything else `SM_OD_LEFT_HAND_SIDE`), and the in-plane offset is `|dHeight * tan(|dDraftAngleDeg|)|`. An angle between 90 and 180 degrees therefore drafts to the same side as a positive acute angle, by `|tan(180° - angle)|`: 135 gives the taper of 45, not an outer one. An outer taper needs a negative angle. Neither comment says that the curves must form one chain either: `SM_ASSERT(nChain == 1)` at `:3678` is the only check, and it compiles to nothing without `SM_DEBUG_CODE` (`SmMessages.h:714`), so a profile with an inner loop is not refused.
- **Trigger:** `SmApiCreateTaperExtrude` with `dDraftAngleDeg` 135 against 45 (same side, same offset), or with a profile of two closed loops.
- **How found:** read during porting, following tangency-labs/solidcore#413 (reported from SolidCraft's move onto SMLib).
- **Confirmed:** no.
- **Severity:** docs.
- **Rust port:** Faithful to the code: `PrimitiveCreation::create_taper_extrude` (`primitive/primitive_creation/p2/taper.rs`) takes the side from the sign, makes the offset positive and does not count the chains. Its doc comment, and that of the API wrapper `sweep::api_brep::create_taper_extrude` (which stops at `SolidError::Unported` until the B-rep constructor is ported), state that convention.
