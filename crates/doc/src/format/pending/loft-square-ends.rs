//! Loft sections save `square` and `weight`: the loft leaves a square end section along its
//! plane's normal, reaching out `weight` times the default before it bends, so faces at an angle
//! or side by side bridge in a smooth bend. Sections picked as body faces are square by default.
//! Older designs have neither and loft as before; no migration. The last release ignores both
//! and lofts straight between the sections (which the kernel refuses at an angle).
//! A section from a curved body face also saves `body` and `face` (the body and a point on the
//! face): the loft runs from that face's rim, and its `sketch` is an empty one on the face's
//! tangent plane. The last release finds no profile in that sketch and refuses the loft.
//! A square end section may also save `flip: true`: it leaves into its face instead of out of
//! it, which picks the way a loft between faces swings round. Older designs have none and loft
//! as before; the last release ignores it and leaves the face outward.
