//! A sketch curve can represent a closed periodic control-point spline (`"type": "spline"`,
//! `"periodic": true`, `"control": true`, and `"degree"` from 3 to 9), with `degree + 1` or more
//! control points. Older designs need no transformation: they have no such curves. The last
//! release refuses a design that has one and asks for an update.
