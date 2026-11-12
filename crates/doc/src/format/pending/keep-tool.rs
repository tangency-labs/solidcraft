//! A feature can save `keep_tool`: the shape it is built from stays a body of its own beside
//! what it did to the bodies it worked on. A box drawn into a solid cuts a pocket and leaves the
//! box standing as well, the way Plasticity's `T` ("Keep tools") does on a command's card.
//! Combine already had its own `keep_tools`, for tools that are bodies in the design, and keeps
//! using it.
//!
//! An older design has no feature with it, since nothing could set it, so it reads back as the
//! design it always was: every feature uses its shape up, which is what they all did before.
//!
//! The last release refuses a design that has it, since the format number it carries is newer
//! than the one that release reads; that is what keeps an older release from quietly dropping
//! the kept body.
