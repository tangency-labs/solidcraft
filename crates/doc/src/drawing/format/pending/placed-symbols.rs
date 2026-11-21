//! Drawings save where their own symbols are placed (`placed_symbols`, #13063): each on a sheet,
//! a view or a view's model as a note is, with its place, scale, angle and its prompts' answers.
//! An older drawing has none. The last release warns about the newer format number, opens what it
//! can read and keeps the rest: it sets the placements aside, with the symbol definitions, and
//! writes them back when it saves. Deleting there a sheet or a view that a placement sits on makes
//! a drawing this release refuses as damaged (#19678).
