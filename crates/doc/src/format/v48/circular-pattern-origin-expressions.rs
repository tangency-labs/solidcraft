//! A circular pattern's axis `origin` can hold length expressions (strings) as well as numbers,
//! so a pattern about a parameter-driven point follows the parameter (#10852). An origin of
//! plain numbers is saved as before, and older designs need no transformation. The last release
//! refuses this newer format and asks for an update instead of failing to read the origin.
