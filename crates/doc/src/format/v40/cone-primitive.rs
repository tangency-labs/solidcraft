//! A design can hold cones (#5641): a `cone` feature with the centre of its base circle (`base`,
//! numbers in mm or length expressions), its `axis` (default +Z), the base `radius`, the
//! `top_radius` (0: a full cone ending in an apex) and the `height` along the axis, and its
//! `operation` (new body, join, cut or intersect) like the other primitives. Older designs need no
//! transformation: they have no such feature. The last release refuses a design saved with this
//! (the newer format number).
