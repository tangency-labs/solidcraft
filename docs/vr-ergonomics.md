# VR and AR ergonomics

Guidance for any SolidCraft client that puts a design into a headset or onto a tablet:
the desktop OpenXR view (#7275, #18712), a standalone viewer, or an AR client. The rules
are written so that a person or an AI can implement them directly and check the result.

## How to use this document

- Every rule has an ID (`C1`, `T3`, `S2`, ...) and a **pass condition** that can be tested
  against a running build, a measurement or a code review.
- Rule IDs are stable. Cite them in code comments, review comments and test names.
- A rule is either **required** or **recommended**. Required rules are release blockers
  for a VR or AR client; recommended rules are judgement calls with a stated default.
- Each rule ends with the source it comes from. Where no source exists the rule is marked
  *(project rule)* and is a design decision, not a finding.
- Thresholds are stated as numbers. If a number is unknown for a given headset, measure it
  rather than guessing.

Status: draft for review. Rules marked *(project rule)* are design decisions, not findings
from the literature.

## The baseline: ten heuristics

Nielsen's ten heuristics are the general rulebook, and the 2026 systematic review
confirms that VR usability work still combines them with VR-specific additions rather
than replacing them (Nielsen, 1994; Othman et al., 2026).

| Heuristic | What it means in a headset | Phase 1 |
|---|---|---|
| Visibility of system status | Say what is happening: connecting, loading, tracking lost, scale mode. Silence in VR reads as a freeze. | required |
| Match with the real world | True scale, upright, floor where the floor is. | required |
| User control and freedom | An obvious way out at all times, and an undo. | required |
| Consistency and standards | Same model, materials, colours and names as the desktop. | required |
| Error prevention | Make the wrong thing hard to do, not easy to undo. | required |
| Recognition over recall | Show the current tool, unit, scale and selection. | required |
| Flexibility and efficiency | Short path for a quick look, no forced ceremony. | recommended |
| Aesthetic and minimalist design | Fewer panels in space, not more. | recommended |
| Help users recover from errors | Name the problem and the fix in the headset, not in a log. | required |
| Help and documentation | One line where the user is looking, not a manual. | recommended |

Source: Nielsen (1994); Othman et al. (2026).

## C - Comfort

Discomfort is physical and ends the session. Nothing else in this document matters if the
user has to take the headset off. These rules are the release blockers.

| ID | Rule | Pass condition |
|---|---|---|
| **C1** | Never display a frame rendered from stale tracking. | If the pose used for rendering is older than the frame's predicted display time, the frame is skipped and the previous one is re-submitted, or the pose is re-fetched. A frame is never shown with a tracking-validity flag unset. |
| **C2** | No camera motion the user did not cause. | Default mode has zero translational acceleration, zero roll and zero forced rotation. Locomotion is off unless enabled. |
| **C3** | If locomotion exists, prefer teleport. | Teleport transition completes within one frame, or uses a vignette of at least 60 % screen coverage during the transition. Continuous locomotion, if offered, is capped at 1.4 m/s. |
| **C4** | Keep the horizon stable. | Camera roll is 0 unless the user physically rolls their head; no artificial pitch or yaw offset is applied to the view. |
| **C5** | Put reading content in central vision. | All text and all precision detail sit within 30° eccentricity of the gaze direction. Peripheral content is for awareness only. |
| **C6** | The exit is reachable at all times. | The runtime system menu and the in-app exit both work during loading, during connection and mid-frame. Response within one frame. |
| **C7** | Do not move the user on entry. | Spawn at the headset's own floor origin, in the user's own seated or standing posture. No teleport, no recentring, no forced height. |

Sources: C1-C7 from Souchet et al. (2023). How long a session lasts is the user's
decision and is not a rule here.

## T - Text and legibility

| ID | Rule | Pass condition |
|---|---|---|
| **T1** | Text subtends at least 1.0° of visual angle for body copy, 1.5° for anything read while moving the head. | Glyph cap height `h >= 2 * d * tan(0.5°)` at viewing distance `d`. At 0.5 m that is 8.7 mm; at 1 m, 17.5 mm; at 2 m, 34.9 mm. |
| **T2** | Contrast ratio at least 4.5:1 for body text and 3:1 for text above 1.5°. | Measured on the rendered frame, not on the source colours. |
| **T3** | Text is pinned to the world or to a panel, never to the head, except transient status. | Head-locked text exists only for status that disappears within 2 s. Panels that follow the head use damping of at least 100 ms. |
| **T4** | Numbers that matter get a fixed, world-anchored place. | Dimensions, units and the scale factor are shown in a panel that does not move when the head moves. |
| **T5** | No hairline strokes, no serif below 1.5°. | Minimum stroke width 2 px at the rendered angular size; sans-serif below 1.5°. |

Sources: T1 *(project rule; 1.0° is a working convention, not a measured threshold)*; T2
*(project rule, aligns with WCAG 2.2 AA)*; T3-T5 from Mehmedova et al. (2025).

## I - Interaction and reach

| ID | Rule | Pass condition |
|---|---|---|
| **I1** | Controls sit in a comfortable volume close to the body. | Controls within 0.3-0.6 m of the torso, in a cone of about 45° below eye level. |
| **I2** | No sustained mid-air reach. | No interaction requires holding the arm extended for more than 5 s; a hands-down rest pose is always available. |
| **I3** | Direct grab for near objects, ray pointing for far ones. | Grab is used at or below 0.75 m; ray beyond that. Neither technique is forced onto the other's range. |
| **I4** | Two hands beat one for precision. | Scale and rotation use a two-handed grab where precision matters, not a one-handed slider. |
| **I5** | Every input is acknowledged within 100 ms. | Visual, audio or haptic confirmation within 100 ms of the input event. Silence after an action is treated as failure. |
| **I6** | Hand tracking is for gross gestures only. | Hand tracking is not used where a tolerance below 5 mm matters; controllers are required there. |
| **I7** | A physical keyboard beats a virtual one. | Where text entry is needed, the physical keyboard is the default path; the virtual keyboard is the fallback. |

Sources: I1-I5 from Bowman et al. (2001) and Mehmedova et al. (2025); I6 from Mehmedova et
al. (2025); I7 from Bowman et al. (2001). Voice is a complementary input in a headset
rather than a rule; its confirmation behaviour follows heuristic 5 above.

## S - Scale

Judging size is one of the reasons to put a design in a headset, so scale has to be
checkable rather than asserted.

| ID | Rule | Pass condition |
|---|---|---|
| **S1** | 1:1 is the default and the current scale is always visible. | A scale indicator reads `1:1` (or the active factor) at all times, without a hover or a menu. |
| **S2** | Reference objects are on request, not permanently on screen. | A life-size hand (open and fist) and a ruler are hidden by default and appear from one visible control. |
| **S3** | Reference objects live in the stage space at 1:1. | Hand and ruler share the design's transform. They are never scaled independently of the model. |
| **S4** | The ruler reads the design file's unit. | Millimetres for a metric file, inches for an imperial one, overridable by the user, and the override persists per file. |
| **S5** | Floor and eye height are correct. | A 1 m test cube placed on the stage floor measures 1 m against the ruler, seated and standing. |
| **S6** | Use passthrough anchoring where available. | Where the runtime supports passthrough, the model can be placed on a detected real surface. |
| **S7** | Offer percentile hands rather than one invented size. | Hand geometry selectable as 5th, 50th or 95th percentile. |

Sources: S1-S6 *(project rule, from the usability discussion in #13469)*; S7 from the TU
Delft DINED anthropometric database.

Hand dimensions come from TU Delft's DINED database (`dined.io.tudelft.nl`), which
publishes anthropometric data by percentile and is licensed CC BY 4.0 (TU Delft, n.d.).
The percentile values themselves sit behind the 1D Database tool rather than on the public
landing page, so they are read from the tool when the hand model is built, not quoted
here.

## L - Cognitive load

| ID | Rule | Pass condition |
|---|---|---|
| **L1** | No invisible modes. | The user can always say what the app is doing and what the next action will do, from what is on screen. |
| **L2** | Keep the panel count small. | At most three floating panels at once. More than three is a search problem, not a layout. |
| **L3** | Menus stay shallow and small. | Menu depth at most two levels; at most seven items per level. |

Sources: L1-L2 from Mehmedova et al. (2025); L3 from Bowman et al. (2001). What a user
does in a session, and for how long, is theirs to decide and is not a rule here.

## Open questions

**How much of the desktop interface belongs in the headset.** The instinct is to say the
in-headset UI should be designed from the headset task and not translated from the desktop
menus. That conflicts with Nielsen's consistency and standards: the same operation should be in
the same place with the same name wherever the user meets it. Both cannot be followed to
the letter.

The likely resolution is that *naming and behaviour* stay consistent while *presentation*
is rebuilt for the headset - the same command, reached a different way. That needs to be
worked out against a real in-headset task before it becomes a rule, so it is recorded here
rather than asserted above.

## Checklist for reviewing a VR feature

Each line maps to a rule above.

- [ ] Exit works during loading, mid-frame and mid-connection. (`C6`)
- [ ] No frame is ever shown with stale tracking. (`C1`)
- [ ] Nothing moves the camera that the user did not cause. (`C2`, `C3`, `C4`)
- [ ] Text meets 1.0° and 4.5:1 at its real distance. (`T1`, `T2`)
- [ ] Scale is visible, 1:1 by default, and checkable with a reference object. (`S1`, `S2`)
- [ ] The ruler shows the file's unit and the override is discoverable. (`S4`)
- [ ] Floor and eye height verified with a 1 m cube, seated and standing. (`S5`)
- [ ] No interaction needs an arm held out for more than 5 s. (`I2`)
- [ ] Every input is acknowledged within 100 ms. (`I5`)
- [ ] The current mode, tool and selection are visible without recall. (`L1`)
- [ ] A failed action names the problem in the headset. (heuristic 9)
- [ ] Nothing destructive is possible without confirmation. (heuristic 5)
- [ ] The model looks the same as on the desktop. (heuristic 4)

## Measurements for further testing

Not rules, but worth testing before an official release:

- Simulator Sickness Questionnaire before and after a session, 16 items, scores recorded
  and compared between builds (Kennedy et al., 1993).
- Session length and time-to-first-task, to see where the interface costs the user time.


## What this does not cover

Haptics, eye tracking and foveated rendering, shared and multi-user sessions, and
accessibility beyond text size and reach. Each needs its own reading before it is
designed.

## References

All sources are freely accessible at the linked addresses. None sits entirely behind a
paywall.

Bowman, D. A., Kruijff, E., LaViola, J. J., Jr., & Poupyrev, I. (2001). An introduction to
    3D user interface design. *Presence: Teleoperators and Virtual Environments, 10*(1),
    96-108. Free author copy: https://eprints.cs.vt.edu/archive/00000548/01/3dui_presence.pdf

Kennedy, R. S., Lane, N. E., Berbaum, K. S., & Lilienthal, M. G. (1993). Simulator
    Sickness Questionnaire: An enhanced method for quantifying simulator sickness.
    *The International Journal of Aviation Psychology, 3*(3), 203-220. Free full text:
    https://cgvr.cs.uni-bremen.de/teaching/vr_literatur/Simulator%20Sickness%20Questionnaire%2C%20An%20Enhanced%20Method%20for%20Quantifying%20Simulator%20Sickness%20-%20Kennedy%2C%20Lane%2C%20Berbaum%2C%20Lilienthal%2C%201993.pdf

Mehmedova, E., Berrezueta-Guzman, S., & Wagner, S. (2025). *Virtual reality user interface
    design: Best practices and implementation* [Preprint]. arXiv.
    https://doi.org/10.48550/arXiv.2508.09358

Nielsen, J. (1994). Ten usability heuristics. In J. Nielsen & R. L. Mack (Eds.), *Usability
    inspection methods* (pp. 25-62). John Wiley & Sons. Free overview of the heuristics:
    https://www.nngroup.com/articles/ten-usability-heuristics/

Othman, M. K., Mat, R., & Zulkiply, N. (2026). A systematic review and taxonomy of VR
    usability heuristics: Trends, challenges, and directions. *ACM Computing Surveys,
    58*(14). Open access at ACM. https://doi.org/10.1145/3832771

Souchet, A. D., Lourdeaux, D., Burkhardt, J.-M., & Hancock, P. A. (2023). Design
    guidelines for limiting and eliminating virtual reality-induced symptoms and effects
    at work: A comprehensive, factor-oriented review. *Frontiers in Psychology, 14*,
    1161932. Open access. https://doi.org/10.3389/fpsyg.2023.1161932

TU Delft. (n.d.). *DINED anthropometric database*. https://dined.io.tudelft.nl

