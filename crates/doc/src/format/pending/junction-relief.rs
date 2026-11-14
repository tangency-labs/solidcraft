//! An edge flange's segment saves `relief` (`{shape, width?, depth?}`) when `sheet.flange` was
//! asked to relieve the junction the segment ends at: the shape (`straight`, `rectangular`,
//! `obround`, `tear` or `notch`), and its width and depth where the segment named its own.
//! Absent, no relief is cut and the junction builds exactly as designs before this change were
//! built, so an older design has no key and needs no rewrite. The last release refuses a design
//! with this change (the newer format number).
