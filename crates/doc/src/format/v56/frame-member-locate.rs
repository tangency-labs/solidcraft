//! A frame member saves its `locate` point: the point of its profile, measured from the
//! profile's own origin, that sits on the member's line. An older design without it gets no
//! `locate`, so the member keeps using its `anchor` (the profile's bounding-box point) as before.
//! A build before this change opens such a design with a newer-format warning and keeps the field
//! it cannot read.
