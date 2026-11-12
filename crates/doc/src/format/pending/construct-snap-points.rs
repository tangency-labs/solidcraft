//! A construction point, axis or plane can be built from a snap point (#11228): a reference of
//! type `snap` saves the edge, face or sketch curve it is on (`of`), which point it is (`kind`:
//! `centre` or `middle`) and where it was picked (`at`), so it follows that edge
//! or face when the model changes. An older design has no such reference and opens as before.
//! The last release refuses a design that has one (it asks for an update) instead of failing to
//! read the reference.
