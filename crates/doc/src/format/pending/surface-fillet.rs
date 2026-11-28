//! A `SurfaceFillet` feature (Surface Fillet, #769): `a` and `b`, each `{body, point}` (a surface
//! body of one face and a point on that face; the point also picks the side of the other
//! surface that is kept), `radius` (a length expression), `trim` (`first` or `none`, saved only when not the default `both`: which of the two surfaces are cut back and sewn to the round, else left as they were, the round alone a new body), and `extend` (saved only when true: a
//! surface that stops short of the other is extended to meet it). It cuts the two surfaces where
//! they meet, sews the kept pieces into `a`'s body and rounds the seam. With `both`, `b`'s body is
//! then gone; with `first` or `none` it remains as it was.
//! A new feature type: older designs have none, so nothing is rewritten. The last release warns
//! about the newer format number, cannot build the feature (it reports the unknown feature type)
//! and keeps it when saving.
//!
//! A `SurfaceChamfer` feature (Surface Chamfer, #769) is the same with `distance` (a length
//! expression) in place of `radius`, and optional `distance2` or `angle` (the second side of the
//! bevel), `flip` (saved only when true: the other surface is the first). A new feature type, with
//! the same effect on the last release.
