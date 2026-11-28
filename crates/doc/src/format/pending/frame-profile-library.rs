//! A design saves its profile library (`frame_profiles`): named frame profiles, each a built-in
//! profile or a drawn section, with its placement points and stock data. A frame member saves
//! `profile: {"source": "library_profile", "id": ...}` when it is made from one, and `point` when it
//! sits on its line by a named placement point. An older design has neither and builds as before.
//! The last release opens a design with this change with a warning about the newer format and keeps
//! what it can't read.
