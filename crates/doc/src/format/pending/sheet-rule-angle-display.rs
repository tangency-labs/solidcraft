//! A sheet metal rule can say how the bend angles of its parts read in the DXF bend notes
//! (`angle_display`: `bend`, the default, or `included`, the angle left between a bend's two
//! sides, 180° less the bend angle, as a press brake is set; #4968). It is saved only when it is
//! `included`, in the rule and in the sheet bodies of the design's saved model. Older designs
//! need no step: a rule or body without it reads its bends as bend angles, as before. The last
//! release refuses a design saved with this (the newer format number).
