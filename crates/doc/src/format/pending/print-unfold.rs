//! A design made by Unfold for Printing saves what it unfolds (`print_unfold`): the body lying
//! on its base face, a point on that face, a point on each edge set as a bend, how far the work
//! has come, the feature that holds its surface model, and where the body was copied from (the
//! design, the body and what it measured there). An older design has none and is no unfolded
//! print. The last release opens a design that has it with a warning and keeps what it can't
//! read: its bodies open as the imported bodies they are, and saving keeps the field.
//! Laid flat it also saves a point on each bend the pattern is split at (`splits`) and the print
//! bed it is checked against (`bed`: a name, a width and a depth).
//! With its wall and joints chosen it saves them (`joints`: the wall thickness, what bends and
//! cuts get, and the edges with a joint of their own) and the feature that makes the printable
//! body (`solid`).
