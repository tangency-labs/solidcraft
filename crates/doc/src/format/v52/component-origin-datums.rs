//! Datum references save an origin datum of an exact placement as `o:<ids>/origin/<item>`
//! (`PlaneRef::Datum` and `GeoRef::Datum`), resolved again on every evaluation so they follow
//! the component. Older designs have no datum references: nothing new, they round-trip as they
//! are. The last release warns about the newer format number, opens what it can read and keeps
//! the rest.
