//! A bill of materials view can be numbered (`assembly.bom.renumber`, `assembly.bom.item.set`):
//! its rows' item numbers are saved in `assembly.item_numbers`, one entry for each view numbered
//! (its assembly, view and list options) with each row's number and lock under the row's key.
//! Older designs need no transformation: a design without it has no view numbered, and every
//! number is provisional as before. The last release asks for an update instead of dropping it on
//! save.
