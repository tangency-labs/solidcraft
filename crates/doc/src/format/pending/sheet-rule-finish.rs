//! A sheet metal rule may save how far its stock's thickness may vary
//! (`"thickness_tolerance": "0.08 mm"`), the stock's finish (`"finish": "brushed"`), and which
//! face of the sheet is the finished one (`"finished_face": "back"`), which turns its flat pattern
//! and DXF over so that face is up (#18304). Each is saved only when it differs from the default
//! (no tolerance, no finish, the front face), so older designs open with the defaults, save as
//! they did and lay out their flats as before. The last release opens a design that has them with
//! a warning about the newer format number, and keeps the fields it doesn't read.
