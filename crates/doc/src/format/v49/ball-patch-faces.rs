//! A face may save `surface` as a `ball_patch` {center, radius}: the corner of three equal rounds
//! as SMLib holds it, a rational patch on that sphere, always with its `nurbs`, the patch itself.
//! An older design has these faces as plain NURBS faces and keeps them so. The last release
//! refuses a design with this change (the newer format number).
