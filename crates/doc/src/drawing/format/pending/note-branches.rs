//! A drawing's note on a view can have more leaders (`branches`, #13059): each to an edge or a corner
//! of the view's model, saved by name as the note's anchor is, 16 leaders in all. An older note has
//! none, and a note without them saves as before. The last release refuses a drawing saved with one,
//! as a newer format.
