//! An Unfold may save `"fixed"`, a point on the flat panel that stays where it is when the sheet
//! is laid flat (`sheet.unfold`'s `fixed`). Without it (every older design) the flat stays where
//! it was: on the largest flat panel's plane, or the one an earlier unfold of the sheet fixed, so
//! nothing is rewritten. The last release warns about the newer format number, unfolds such a
//! sheet about its largest flat panel and keeps the field it does not know when saving.
