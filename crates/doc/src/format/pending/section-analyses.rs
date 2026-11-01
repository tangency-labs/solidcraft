//! Section Analyses are saved with the design (`section_analyses`): each one's name, plane
//! (an origin plane, a construction plane by id, a planar face by body and persistent face name,
//! or a fixed plane), distance, flip and hatch, which one is shown, and whether the Analysis
//! folder is hidden. Older designs have none and open as before. The last release refuses a
//! design that has them (it reads the newer format number) instead of dropping them.
