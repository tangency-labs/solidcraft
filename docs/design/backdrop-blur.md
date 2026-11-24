# Backdrop blur: one mechanism for every view, window and surface (#19213)

Status: steps 1 to 6 of section 5 are built (section 9 lists where the code differs from this text); section 3.7 is built by its own PR (#10395). Read `docs/blur-behind.md` first; it
describes what main does today and this document changes parts of it.

## 1. What is wrong today

Main already blurs what is *painted before* a panel. `FrameCompositor::render`
(`crates/ui-kit/src/blur_native.rs`) renders the egui primitive stream into one target C, ends the
pass at every `PanelMarker`, runs the Kawase pyramid from the current C, and the marker samples
the result. Stacked cards, menus, dialogs and tooltips therefore already see the UI under them.
That part stays.

What still makes it work only in one place:

1. **The 3D view is a special source.** `blur::viewport` replaces the view's paint callback with a
   `Source`. It draws the viewport background and scene into a separate texture S, C starts as a
   copy of S, and the source callback is skipped in the stream. So the effect needs *exactly one*
   such callback in the frame (`source_count == 1`, `source_seen`). Otherwise it reports
   unavailable (`missing_source`, "multiple viewport sources in one frame").
2. **Views without that callback are switched off.** `SolveApp::ui` passes
   `source_supported = !home.open && !render_workspace::result_active(..)`; `prefs.blur_source`
   ("Unavailable in this workspace view") is the user-visible result. Translucent viewport or
   outside colours are also refused, because S has to be opaque.
3. **Root window only.** `begin`, `decorate`, `BackingSlot::reserve`, `show_combo`,
   `BackingModal` and `Container::background` all test `viewport_id() == ROOT`. A pop-out never
   blurs.
4. **Every marker costs a full-frame pyramid.** The pyramid at each marker covers the whole
   screen whatever the marker's size. Ten cards cost ten pyramids. `Guard` lowers the step count
   when frames run long, which hides the cost instead of removing it.
5. **All or nothing.** More than `MAX_MARKERS` (256) markers, or one refused allocation, turns
   the effect off for the whole frame.
6. **Frames with no panel to blur still pay.** Once a source exists, the whole frame goes through
   C and the present pass, even if no marker was emitted.
7. **No hook on the web** (`blur.rs` wasm stubs; `prefs.blur_web`).

## 2. Goals and non-goals

Goals

- The effect needs nothing from any view. If a surface is translucent and asks for a backdrop, it
  gets a blur of exactly what was painted under it, in any workspace, any window.
- Cost follows what is blurred: nothing for a frame with no marker, area-proportional for a
  frame with markers, and one pyramid for markers that do not overlap.
- A surface opts in with one call; `Container` stays the policy owner for the default surfaces.
- No default changes: radius 0 is free, `blur` stays a theme option, the 0 to 60 px range and
  the capability reasons keep their meaning.

Non-goals

- Web support (needs an egui-wgpu hook the web path does not have; stays "unavailable" with its
  existing reason).
- A new look. For a marker on its own the output must match today's within 1/255.
- Blurring what is painted *after* a panel, or across windows.
- Blurring anything that is translucent only to show what is behind it (section 3.7).

## 3. Design

### 3.1 The frame owns C from its first primitive (delete the source)

`render` clears C and draws the stream in order, including the viewport callback, exactly as the
ordinary pass would. S, `Source`, `blur::viewport`, `background_params`, the `background`
pipeline and shader entry, `source_seen`, `source_count`, `missing_source`,
`State::begin`'s `source_supported` argument, `Inner::geometry` and the `prefs.blur_source`
reason are removed. `viewport.rs` and `render_workspace.rs` call
`painter.add(egui_wgpu::Callback::new_paint_callback(rect, cb))` as they do with blur off.

Why this is enough for the 3D view: `ViewportCallback::prepare` already renders the scene into its
own offscreen picture (the native split scene) and `paint` only draws that picture, so painting it
into a segment pass is the ordinary cost. Its own `frame_key` cache still skips re-rendering a
still scene. The capability rule "native split scene, 1x UI, no depth" is unchanged.

Because there is no source, translucent viewport backgrounds stop being a special failure.

Frames with no marker take the ordinary pass: `render` scans the stream for marker identities first
(already done by `callback_id`) and returns `false` when there is none. That is the zero-cost
path for Home, Render results and every view without translucent panels.

### 3.2 Plan the frame before encoding it (pure, testable)

A new pure function, in its own module `blur_plan.rs`, turns the stream into passes:

```rust
pub struct Extent { pub rect: Rect_px }          // pixel AABB of a job, clip applied
pub struct Member { pub job: usize, pub region: Rect_px }   // marker + padded region
pub struct Run { pub start_job: usize, pub members: Vec<Member> }
pub fn plan(jobs: &[Extent], markers: &[Member], pad: u32) -> Vec<Run>
```

Rules, applied in stream order:

- A **run** is one snapshot of C plus one blur of every member's padded region.
- The snapshot is taken at the first member's position. A later marker `M` *joins* the run only if
  nothing painted since the snapshot (including earlier members' own shadow, fill and content)
  intersects `M`'s padded region. Otherwise `M` starts a new run, and its snapshot includes
  whatever was painted in between. That is what makes "card B over card A" blur A, and "card B
  beside card A" cost one pyramid.
- Members of one run are pairwise disjoint (each earlier member's extent is already "painted
  since the snapshot"), so they can share the pyramid textures at different places.
- Extents come from mesh vertex bounds intersected with the clip rect; a callback's extent is its
  rect intersected with its clip. Computed in one pass over the jobs, only on frames that have a
  marker (measure it; see section 6).

`pad` is the pyramid's reach: `ceil(2 * radius_px) + 2^(steps + 1)`, clamped to the screen. A
test compares region blur with the whole-frame blur inside the marker's rect.

### 3.3 Region-scoped pyramid, shared across a run

For each run, for each pyramid level, one pass: bind the level's pipeline, then for each member
set the scissor to the member's padded region scaled to that level (`ceil` at the outer edge) and
draw the triangle. Pass count per run stays `2 * steps`; drawn pixels are proportional to the
summed member areas. The existing chain textures are reused by every run in the frame, in stream
order, in the same encoder, so wgpu orders the writes. A run's markers draw (sampling the final
level) before the next run's pyramid overwrites it; the stream guarantees that because the next
run starts after the last member of this one.

No feedback loop: the pyramid reads C only when C is not attached to a pass; markers attach C and
sample only the pyramid's last level.

Per-member uniforms stay distinct retained buffers (as `Gpu::markers` does now), plus one small
uniform per member and level for the region, written with `queue.write_buffer` before the run.

### 3.4 One compositor, per-window state

The renderer has one `frame_compositor`, and `begin_viewport(id)` already says which window is about
to paint. Replace `painting_owner: AtomicBool` with a map `ViewportId -> ViewportBlur`
(`gpu`, `serial`, `screen`, `markers`, counters). Each window gets its own C and pyramid sized
to its surface. `BUDGET` is shared across windows; when a new window would exceed it, the
least recently painted window's resources are dropped and it reports unavailable.

`begin(ctx)` is called at the top of each window's UI: today in `SolveApp::ui`; the same call goes
in the pop-out hosts (`crates/ui-egui/src/panel_runtime/app.rs:588`,
`panel_runtime/native.rs`, `agent_chat/panel.rs:846`). The thread-local `CURRENT` plus `Scope`
already nests, so an immediate viewport inside the root's UI restores the root's context on drop.
All `viewport_id() == ROOT` checks (`container.rs`: `BackingSlot::reserve`, `show_combo`,
`BackingModal::show`, `BackingTooltipExt`, `Container::background`, `decorate`) are replaced by
`blur::enabled()` alone, because the thread-local is now set per window.

### 3.5 Degrade per marker, fail per window

- Over the marker cap or the memory budget: that marker's `decorate` returns the ordinary shape
  (it already returns `ordinary` when it cannot mark), the rest keep blurring. The cap becomes a
  budget on summed region area per frame, not a count of 256.
- A refused GPU allocation or a lost device still turns the effect off for that window with
  `Capability::Unavailable(reason)`, and panels keep their plain translucent fill.

### 3.6 Public API

Keep `decorate(painter, frame, content, ordinary) -> Shape` as the low-level entry (the `Container`
hosts, combo popups, modals and tooltips in the kit use it). Add one small constructor for surfaces that have
no `Frame`:

```rust
blur::Backdrop::new(rect).corners(CornerRadius).opacity(f32).shape(&painter) -> Shape
```

It returns `Shape::Noop` when blur is off, so a caller adds it unconditionally. Eligibility policy
(`Container::adaptive`: Panel, Feature, Palette, Chrome; opaque and fully clear fills excluded)
stays in `ui_kit::container` and is documented there as the single place that decides which
default surfaces blur. HUDs, toasts, input shields and the application bar stay out.

### 3.7 Only a themed glass material blurs; see-through never does

Blur belongs to the *material* a theme gives a surface (a translucent fill on a Panel, Feature,
Palette or Chrome surface), not to alpha as such. An element that is translucent because it is
meant to let the model show through, or because it is fading, never blurs:

- the Translucent browser option (`browser_float`) and the History stacked under it (#10395);
- the navigation bar dimmed by dim-when-not-hovered, and any other fade or dim;
- anything else a host makes translucent at run time (a hover fade, a drag ghost).

Today `decorate` decides from alpha alone: it blurs any fill with alpha 1 to 254, and also an
*opaque* fill whose painter opacity is below 1 (`blur_native.rs`, the early return in `decorate`),
and the marker carries that opacity into the shader (`p[3]`). That is how a dimmed bar ends up
blurred. The rule changes to:

- `decorate` returns the ordinary shape whenever `painter.opacity() < 1.0`. The marker no longer
  carries an opacity, and the shader's opacity multiplier is removed (alpha comes from the fill
  alone). A fade-in therefore starts without blur and the blur appears when the element reaches
  full opacity; the fixture for this checks there is no flash.
- `Container` gets `see_through(self) -> Self`. A surface built with it never requests a backdrop,
  whatever its fill alpha. The Translucent browser and anything stacked under it set it, so
  `browser_float` needs no code that reaches into the blur setting. The theme's saved blur radius
  is not touched and unrelated panels keep blurring.
- `Container::adaptive` keeps one meaning ("takes the themed surface treatment"). A second,
  separate check, `Container::blurs`, is `adaptive() && !see_through`.
- A surface whose fill alpha is lowered by a preference (the fill is then no longer the theme's
  glass) counts as see-through. The code that lowers it calls `see_through()` in the same place.

This also settles the open question about #10395: that issue becomes a one-line `see_through()` on
the Browser host and its stack once this lands.

### 3.8 What is deliberately not cached

Today S has a key, so an unchanged scene skips work. The blur result depends on everything painted
before each marker, so it cannot be cached by the viewport's key. egui already skips frames when
nothing changed, so a frame with blur work is a frame where something moved. Section 6 measures a
still model with an animating panel. If that is too slow, add a per-run fingerprint (marker
rects, plus the vertex data and callback revisions of the jobs that intersect the padded regions).
That is a follow-up only if measured to matter.

## 4. Files

| File | Change |
| --- | --- |
| `crates/ui-kit/src/blur_native.rs` | Remove source path; per-window state; use the planner; region passes. |
| `crates/ui-kit/src/blur_plan.rs` (new) | The pure planner and its unit tests. |
| `crates/ui-kit/src/blur.wgsl` | Remove `background`; `down` and `up` unchanged (scissor does the region). |
| `crates/ui-kit/src/blur.rs` | `Backdrop`; remove `viewport`, `source_*`; wasm stubs follow. |
| `crates/ui-kit/src/container.rs` | Drop `ROOT` checks; add `see_through()` and `blurs()` (section 3.7). |
| Browser, History and navigation bar hosts | Call `see_through()` for the translucent and dimmed states. |
| `crates/ui-egui/src/viewport.rs:6774`, `render_workspace.rs:3714` | Plain paint callback. |
| `crates/ui-egui/src/lib.rs` (`ui`, `blur_source`), `prefs.rs` (`blur_availability*`) | Drop the source argument and reason. |
| `crates/ui-egui/src/panel_runtime/*`, `agent_chat/panel.rs` | `begin` per window. |
| `assets/locales/pending/<slug>.json` | Remove `prefs.blur_source` text (check `cargo xtask lists` for how removals are filed). |
| `docs/blur-behind.md` | Rewrite the source, composition and capability sections. |

No change is needed in `vendor/egui`: `render_with_mesh_offset` and the `FrameCompositor`
hooks it needs are already there. No design-format change, no new command, no new preference.

## 5. Order of work (commits on this branch, each builds and passes its tests)

1. `blur_plan.rs` with unit tests (run grouping, overlap with painted-since, padded regions,
   clip handling, disjoint members). No behaviour change.
2. Region-scoped pyramid and per-run sharing, still with today's source. Counters get
   `blur_pixels`. Fixture: region result equals whole-frame result inside the marker.
3. Delete the source: plain viewport callbacks, `begin` without the source argument, marker-free
   frames use the ordinary pass. Home and Render result now blur.
4. Per-window state and `begin` in the pop-out hosts; remove the `ROOT` checks.
5. Per-marker degradation; area budget replaces the count cap.
6. `Backdrop` constructor and a first non-`Container` user; docs rewrite; screenshots.
7. (Can go first, since it needs none of the above.) The see-through rule of section 3.7:
   `decorate` ignores faded painters, `see_through()`, and the Browser, History and navigation bar
   hosts use it.

## 6. Verification

Unit (no GPU): planner cases above.

GPU fixtures (`crates/ui-egui/tests`, the existing rendering fixtures; software adapters are
refused by production policy, so they use `FixturePolicy::Rendering`):

- Panel over an egui-painted gradient with no viewport callback in the frame: blurred, not plain.
- Panel over Home and over a Render result image.
- Card B over card A blurs A; card B beside card A: blur pass count equals one pyramid.
- Pop-out window with a translucent panel.
- Blur off (radius 0), and a frame with no marker: counters stay zero, no extra passes, pixels
  identical to the ordinary pass.
- unorm and sRGB formats; rounded corners; clip.
- No blur, and no marker, for: a surface built with `see_through()`; a painter at opacity below 1
  (the dimmed navigation bar); the Translucent browser with a blurring theme (its backing shows
  the model unblurred and the theme's radius is unchanged); a fade-in, which gains blur only at
  full opacity. A sibling panel on the same theme still blurs.
- The existing blur fixtures (see #17280 for the composition one) still pass, with any
  "resting source" assertion replaced by one that asserts the frame has no source.

Measurements (recorded in the PR, on a real GPU, 1920x1080 and 3840x2160):

- Frame time with 1, 4 and 10 non-overlapping cards, before and after.
- A still model with an animating card.
- Planner CPU time on a busy frame (vertex-bounds pass).

Evidence: screenshots of the same translucent panel over the 3D view, Home, a Render result and
a pop-out, with blur off and on, captured from the app.

## 7. Risks

- **Planner CPU cost.** The vertex-bounds pass touches every vertex of a frame with a marker. If
  it measures above a fraction of a millisecond, switch to clip-rect extents plus bounds only for
  meshes that intersect a marker region.
- **Regions at pyramid edges.** Too small a pad shows seams at a marker's edge. The fixture in step 2
  guards it.
- **Lost per-view quirks.** Some view may rely on the source's background colours to look right
  behind panels. Step 3's screenshots cover every workspace view before merge.
- **Memory.** One C and one pyramid per open window; the shared `BUDGET` bounds it.

## 8. Decisions made, and open

Decided here:

- Pad the region and keep reading outside the marker's rect (today's look), instead of clamping
  the blur to the rect (a visibly different look).
- Stacked glass blurs what is directly under it (a card over a card sees the card below).
- The web stays unavailable with its current reason.
- Translucent for see-through or fading (Translucent browser, dimmed navigation bar) never blurs;
  only a theme's glass material does (section 3.7).

Open for review: none right now.

Related: #9809, #8876, #10395, #11218, #14251 (the UI kit now owns this code).

## 9. Built as (where the code differs from the text above)

- The planner is `plan(jobs, markers, pad, screen)`; a `Member` is `{ job, region }`. Members of a run
  have disjoint marker rectangles, but their padded regions may overlap: both compute the same values
  from the same snapshot, so overlap only costs area. A marker joins only the latest run, because a
  later run's pyramid overwrites the shared textures. `blur_plan::reach` computes the pad from every
  tap of every level instead of the `2 * radius + 2^(steps + 1)` estimate.
- Counters: `blur_passes` is per pass (`2 * steps` per run); `blur_runs` and `blur_pixels` are new;
  `source_passes` and `cache_hits` are gone. There is no cache.
- The blur resources are created inside `render`, not in a callback's `finish_prepare`, because
  nothing prepares them any more.
- The pop-out host in `panel_runtime/native.rs` only opens an invisible placeholder viewport and draws
  nothing, so only `panel_runtime/app.rs` and `agent_chat/panel.rs` call `begin`. The Assistant window
  does not begin a frame when egui draws it inside the main window (`EmbeddedWindow`), where the main
  window's frame is already open.
- Per-window state is `Windows { map: HashMap<viewport id, Inner> }` behind one mutex. A window not
  begun for 600 `begin` calls is forgotten; the 256 MiB budget is shared, and a window that needs room
  releases the least recently painted other window first.
- Step 5: `blur_plan::Budget` (eight screens of region area, 64 runs, 256 markers with a binding) drops
  the later markers of a frame one at a time. The 256-marker count is replaced by a bound of 4096
  emitted markers per window and frame that only protects memory. A marker over that bound returns
  the ordinary shape and no longer turns the effect off.
- Step 6: the first user outside `Container` is the running-operations card (`operations.rs`), whose
  fill is the theme's translucent `overlay`. It asks with `blur::Backdrop`, so it blurs only under a
  blurring theme.
- `prefs.blur_source` is removed from every message catalog directly; pending files only add strings.
