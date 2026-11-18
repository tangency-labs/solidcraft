//! Names count characters, not bytes: a timeline folder, browser group, view, body, component or
//! joint name of up to 128 characters is saved, which can be up to 512 bytes of UTF-8 (a hundred
//! `ł` are 200 bytes). What is saved is unchanged, and older designs are read as they were. The
//! last release warns about the newer format number, then refuses the whole design as damaged
//! when a timeline folder name, or the body name of a configuration's face appearance, is over
//! 128 bytes. Other long names open there as saved, and so do saved render targets (their
//! definition takes up to 1024 bytes).
