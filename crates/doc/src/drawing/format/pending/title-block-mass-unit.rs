//! A title block's text cell can save `mass_unit` (`kg`, `g`, `auto` or `both`) to choose how
//! `component.mass` is shown in that cell. A cell saved without `mass_unit` reads as `kg` and saves
//! without it, so older drawings keep their format and bytes with no upgrade step. The last release
//! warns about the newer drawing format, opens what it can read and keeps the rest.
