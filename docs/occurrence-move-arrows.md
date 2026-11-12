# Occurrence Move/Copy arrows

Move/Copy draws and processes occurrence translation and rotation handles in the viewport that owns their pointer events. The numeric fields continue to use the same pending placement state. Capture Position commits the placement; Revert Position discards it.

Holding the snap chord (Ctrl/Cmd by default) while a handle is dragged lands the occurrence's origin on the place under the cursor — a corner, the middle of an edge, the centre of a hole or face, a quarter turn — the same places the Move/Copy triad takes; the handle being dragged decides which of the three distances the place may set. The bodies the occurrence places are kept out of the candidates, since they follow the cursor.

This repairs ordinary occurrence editing as well as pasted linked designs. It does not change joint definitions or introduce free dragging of arbitrary faces. The clipboard pointer-placement contribution is separate (#2433); the arrow input repair is tracked in #3014 and #3015.

The regression exercises an ordinary occurrence, compares an actual viewport arrow drag with numeric input, checks that a second constrained occurrence and its joint remain unchanged, and checks capture, undo, and redo. It does not require clipboard functionality.
