# Viewport clipping

The Design viewport computes its clipping planes from the current model and preview
bounds on every projection, using the effective camera (orthographic while a sketch
is open, otherwise the selected projection). Changing projection preserves the orbit
camera's target, orientation and distance.

The bounding sphere around the target, with the existing viewport-scale margin,
continues to determine the far plane and the orthographic depth range. The margin
also admits nearby grids and overlays. In perspective, the near plane must additionally
respect the nearest depth of the scene's axis-aligned bounding box along the camera's
view direction. It is capped at half that depth, with a minimum of 0.001 mm. A box
crossing the eye uses that minimum. Geometry behind the perspective eye remains
outside the view.

Orbit distance alone cannot set a safe near plane. A component can lie close to the
eye in a deep assembly even when the target and the assembly centre are near the
origin. A near-plane floor of 0.1% of orbit distance can then clip the entire component.
Disabling ground shadow does not change these clipping planes.

Empty, non-finite or overflowing bounds use the existing radius-based fallback.
Callers that only supply a radius retain the existing camera API and clipping policy.

Regression coverage uses synthetic panels at opposite ends of a deep scene. Their
vertices must remain inside the depth range through orthographic → perspective →
orthographic toggles, including the viewport's f32 matrix conversion. The viewport
regression checks both ground-shadow settings and unchanged stored camera state.
