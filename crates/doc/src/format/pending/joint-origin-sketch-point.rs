//! A point joint origin (`Snap::Point`, `"type": "point"`) can save `on`, the sketch point it
//! sits on: `{"sketch": <sketch feature id>, "point": "<point id>"}` (#21815). The origin is then
//! that point as the sketch places it now, so it follows the sketch when the design changes.
//! Older designs need no transformation: a point origin without `on` stays at its `point`, as
//! before. The last release warns about the newer format number and keeps the origin at its
//! saved `point`, so the joint no longer follows the sketch there.
