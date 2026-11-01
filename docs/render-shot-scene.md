# Scene overview and Shot editor

The Scene overview replaces the old Scene page. Entering Render with no selection,
clearing the selection, or selecting nothing opens Scene. Selecting a saved shot
opens its Shot editor; the other editors retain their existing adapters during the
selection-based Inspector migration.

Scene follows E1: Stages (Look, Light, Frame, Output), Ready to render, Reminders.
Each stage row summarises current state and navigates through the view commands.
Look opens the object/appearance stage, Light selects Environment, Frame selects
the current saved shot, and Output selects Output. Check now and Render shot use
the same request and command adapters as the stage bar. Open Check opens the
existing diagnostics editor because this branch has no Check selection kind.
The check summary distinguishes unchecked, current and stale results. Renderer
setup progress, failures, unavailable hosts and offline reasons remain visible.
All shots shows the saved-shot count and stays faded with the unavailable queue
reason; it never silently starts an individual render.

Shot follows E7: Name; changed-camera banner and Update/Revert/duplicate/remove;
Camera; Lens (GPU); Exposure; Frame. Camera contains projection, FOV, distance in
subject-size units, target XYZ, yaw/pitch/roll, Frame subject and Look at selection.
Camera controls save through render.settings.set and apply through render.camera.set;
navigation alone leaves the camera changed banner until Update shot or Revert.
Remove always uses the shared confirmation and checks for stale document state.

Lens values are saved to the selected shot's GPU job while retaining its other
options. Depth of field needs GPU path tracing and perspective; aperture and focus
distance remain visible when disabled. Raster exposure and orthographic FOV stay
faded with reasons. Changing to orthographic while saved depth of field is enabled
is rejected by the existing settings validation; disable depth of field first.
Frame retains aspect presets and transient composition guides. Size is read-only;
Edit in Output selects Output, the home of image dimensions.

These two editors use the shared workspace header without the old setup form,
page switcher or pinned job footer. Explicit native-studio resource reviews remain
in the library card, keeping Scene limited to orientation and readiness. The
shared section_method component supplies method badges, with a gallery sample.
New copy is in assets/locales/pending/render-shot-scene.json for en, ja, zh, de, ko.

The Shot Target uses the shared number_fields row cells with equal widths. The
Projection row uses the kit's base label column to leave room for both full
projection labels, and the distance label is shortened to Distance. Layout
regression coverage checks every editor widget at 350 and 320 points of content
width, including disabled controls, equal Target cells and untruncated labels.

Validation uses headless egui harnesses and focused render tests. No running-app
visual review or GPU-device verification is claimed by this change.
