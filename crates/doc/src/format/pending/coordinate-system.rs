//! Coordinate System features (#14644): `FeatureKind::CoordinateSystem` saves a frame's origin
//! reference and its optional Z and X references or typed directions, and datum references gain
//! a coordinate system source, saved as `o:<ids>/cs:<feature id>/<item>` or `cs:<feature id>/<item>`
//! for one of the design's own (`PlaneRef::Datum`, `GeoRef::Datum`, `AxisTarget::Datum`). Older
//! designs have neither: nothing new, they round-trip as they are. The last release warns about the
//! newer format number, keeps a Coordinate System as a feature it can't read and refuses a datum
//! key it doesn't know, opens what it can read and keeps the rest.
