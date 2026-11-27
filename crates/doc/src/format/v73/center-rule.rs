//! The Center assembly rule (#6061): `assembly.constraint.create {type: "center"}` saves its
//! two wall faces as one joint-origin snap, `between_faces` — two picks and the faces'
//! persistent names, resolved to the plane midway between them on every solve — and the rule
//! itself as the joint's `constraint` tag, `center`. Older designs have no such snap and no
//! such tag. The last release warns about the newer format number, opens the design and keeps
//! the unreadable assembly section intact when saving. No step: nothing older is rewritten.
