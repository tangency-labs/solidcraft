//! A parts list may save `press_in_hardware: true` (in its `parts_list` record): it then has a row
//! for each kind of press-in hardware in the parts it lists. A drawing saved without it has the flag
//! off, so its parts lists keep exactly their rows. The last release refuses drawings saved with
//! this newer format.
