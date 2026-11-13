//! A coil's section can come from a sketch (`sketch`, `profiles?`): the sketch plane is the
//! section plane, and `diameter`/`section_size` are then absent. Older designs need no
//! transformation: a coil without `sketch` is the primitive circular or square section exactly
//! as before. The last release refuses a design saved with this (the newer format number).
