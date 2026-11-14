//! A Scale can save what it scales about as `about`: `{"type": "centre"}` (the centre of its
//! bodies' box, as the features before it leave them) or `{"type": "pick", "at": <reference>}` (a
//! vertex, face, edge, sketch or construction point, found again by name). Either is worked out
//! again each time the design is evaluated, so the Scale follows the features before it when
//! they change. Older designs have no `about` and keep scaling about their fixed `origin`, as
//! before; no rewrite is needed. The last release refuses a design with this change (the newer
//! format number) instead of scaling about the design origin.
