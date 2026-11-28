//! An Emboss or Deboss may save `offset_x` and `offset_y` (lengths: a shift of its picked profiles
//! along the sketch's x and y) and `angle` (a turn of them, counter-clockwise in the sketch, about
//! the middle of their bounds), so the profiles can be placed on the face without editing the
//! sketch (#20386). The feature is built from the profiles so placed: the wrap round a cylinder
//! wall, the projection onto a picked face and the plain emboss alike. Each is saved only when
//! given. An older design has none of them, so its features are built where their sketches have
//! the profiles, as before, and nothing is rewritten. The last release warns about the newer
//! format number, opens the design and ignores the fields, so it builds such a feature where its
//! sketch has the profiles, and keeps the fields when it saves.
