//! A frame member saves its `up` axis when it is not world +Z: the world axis its profile's y
//! points along as far as each line allows, and the axis its vertical members run along. An
//! older design without it gets world +Z, so its members keep the axes they had. A build before
//! this change opens such a design with a newer-format warning, builds its members with Z up,
//! and keeps the field it cannot read when it saves.
