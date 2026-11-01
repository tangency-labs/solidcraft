//! An edge flange can stop short of the start and of the end of its edge: `start_setback` and
//! `end_setback` on a `sheet_flange` feature (lengths, as expressions). Older designs need no
//! transformation: a flange without them runs the whole edge, as before. The last release refuses
//! a design saved with them (the newer format number).
