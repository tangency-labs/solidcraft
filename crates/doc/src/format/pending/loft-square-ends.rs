//! Loft sections save `square`, `weight` and `flip`: the loft leaves a square end section along
//! its plane's normal (into its face with `flip: true`), reaching out `weight` times the default
//! (0 to 100) before it bends, so faces at an angle, side by side or back to back bridge in a
//! smooth bend. Sections picked as body faces are square by default.
//! A section from a curved body face also saves `body` and `face` (the body and a point on the
//! face): the loft runs from that face's rim, and its `sketch` is an empty one on the face's
//! tangent plane.
//! Older designs have none of these and loft as before; no migration.
//! The last release warns that a design with them comes from a newer format and asks for an
//! update. It does not build these lofts as saved: it ignores `square`, `weight` and `flip`
//! (lofting straight between the sections, which the kernel refuses at an angle) and finds no
//! profile in a curved face's empty sketch, so that loft fails instead of building a wrong shape.
