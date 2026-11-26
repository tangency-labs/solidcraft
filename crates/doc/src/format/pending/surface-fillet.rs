//! A `SurfaceFillet` feature (Surface Fillet, #769): `a` and `b`, each `{body, point}` (a surface
//! body of one face and a point on that face; the point also picks the side of the other
//! surface that is kept), `radius` (a length expression), `trim` (`first` or `none`, saved only when not the default `both`: which of the two surfaces are cut back and sewn to the round, else left as they were, the round alone a new body), and `extend` (saved only when true: a
//! surface that stops short of the other is extended to meet it). It cuts the two surfaces where
//! they meet, sews the kept pieces into `a`'s body (`b`'s body is then gone) and rounds the seam.
//! A new feature type: older designs have none, so nothing is rewritten. The last release warns
//! about the newer format number, cannot build the feature (it reports the unknown feature type)
//! and keeps it when saving.
