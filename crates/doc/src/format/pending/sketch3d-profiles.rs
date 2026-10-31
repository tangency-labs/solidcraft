//! A surface patch can patch planar loops of 3D sketch curves: `profiles_3d` holds indices into
//! the sketch's planar 3D loops (`sketch.inspect`'s `profiles_3d`), each patched in the plane it
//! lies in (#741). Older designs need no transformation: without the field the patch takes its
//! profiles from the sketch's 2D profile list exactly as before. The last release refuses a
//! design saved with this (the newer format number).
