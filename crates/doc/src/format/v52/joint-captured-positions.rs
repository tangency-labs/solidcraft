//! Joints save their captured values: `captured`, the values Capture Position
//! (`component.capture_position`) saved and Revert Position (`component.revert_position`) puts
//! the joint back to. An older design has none, so its joints load with their values as before
//! and Revert Position leaves them where they are. The last release warns about the newer format
//! number, opens the design and keeps the `captured` values it can't read.
