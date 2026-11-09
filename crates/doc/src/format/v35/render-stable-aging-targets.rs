//! Weathering layers save definition/feature/component/occurrence identity, never
//! captured mesh ordinals. Older untargeted layers retain their fields and behavior.
//! Experimental ordinal-targeted layers explicitly require rebinding instead of
//! silently affecting a different occurrence. The last release refuses this newer
//! format, and a pending reader without this descriptor refuses the new intent.
