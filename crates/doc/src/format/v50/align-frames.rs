//! An Align feature (`FeatureKind::Align`) saves optional `from_frame` and `to_frame` (a joint
//! origin on the model, `{"type": "origin", "origin": {...}}`, or a sketch point or the centre of a
//! sketch circle, arc or ellipse, `{"type": "sketch_point", "sketch": id, "point": "<id>"}` /
//! `{"type": "sketch_curve", "sketch": id, "curve": "<id>"}`), found again from the geometry on
//! every evaluation so the bodies follow them, and an optional `angle` expression (a turn about the
//! target's direction). Older designs have none of them and evaluate as before. The last release
//! refuses a design with this change by its newer format number.
