# CPU Interactive core

First slice of #8936 under #6201. `job::interactive` prepares bounded CPU
progressive requests and coordinates their lifetime. **It is not yet connected
to the workspace UI.** Entering Render does not start a job. No default,
document format, final export, panel or gizmo is changed by this slice.

`cpu_request` clones an immutable final request. It preserves the camera,
background, shadows, exposure, lighting, resources and transport. Only preview
dimensions and camera/reflection sampling change: maximum 1024 pixels along the
longest edge, rounded to the nearest pixel without enlargement, 16 SPP, seed 0,
one primary and one selected reflection attempt per camera sample. The original
final request stays untouched. Integer aspect rounding is at most half a pixel
(or one pixel for an extreme aspect). Transparent output, another integrator or
unsupported progressive transport returns an error instead of dropping effects.
The existing profile and host still enforce geometry, traversal and memory limits.

## Adapter contract

- Allocate a distinct transient job session for each document's Interactive
  controller. Never recreate a controller with the same session while callbacks
  from its previous instance can arrive. Do not register its jobs in the engine's
  final/export registry. Even its 16-SPP completed image is provisional content.
- Increment `VisualRevision.visual_revision` whenever camera, draft, material,
  visibility, lighting, HDR/resource, method or output intent changes. Include
  changes that leave the document revision unchanged. Capture this counter and
  `scene_generation` atomically with the full request and scene. This core does
  not infer that identity from a setup name or a partial hash.
- Explicit activation calls `enable`; edits call `edit`. Supply monotonic elapsed
  `Duration` values. `capture` issues one ticket after 200 ms without a newer edit.
  Do not evaluate geometry or read files every frame just to compare identity.
- Capture through the existing authoritative engine boundary, prepare the CPU
  request, then admit/submit through the installed CPU host using
  `CaptureTicket::job_id()`. Pass the resulting handle to `attach`
  with the original ticket. Delayed jobs are cancelled, including a wrong serial
  or generation in our dedicated preview session. A foreign session is rejected
  without cancelling its work, even if its handle arrived through a misrouted
  callback. Final jobs must keep their distinct engine session.
  Report a failed capture through `capture_failed`; failure waits for an edit or
  explicit resume, rather than retrying every frame.
- Read only `current()` to publish checkpoint pixels. The existing host provides
  immutable previews, sample counts, sequence and final target bytes. Their job
  and revision must remain attached to the displayed image. A copied handle or
  retained texture is not permission to display it as current after an edit.
- Pause may retain an immutable checkpoint in the adapter, explicitly labelled
  paused and stale after edits. It cancels work. Resume uses the latest revision.
  Leave Render or change documents with `disable`; Drop also cancels owned work.
- Before a manual final starts, call `set_final_active(true)`. This cancels only
  Interactive work and prevents new capture tickets. Call it with false after the
  final finishes; an enabled, unpaused controller then groups the latest edits
  again. No method here cancels a final job or exports an image.

The UI adapter, registered activation/pause commands, current-request identity
capture, retained paused texture and visual acceptance remain a following slice
coordinated with #8000/#7375. This is not a claim that those integration gates or
the installed application's shadow defect are fixed.
