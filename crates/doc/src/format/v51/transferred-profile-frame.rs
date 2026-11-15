//! A new-body extrusion or revolve whose outputs transfer into its source sketch's component
//! reads the profile through the recorded transfer frame. Capturing that component's position
//! therefore moves the body once, as the pending preview does. Existing designs use their
//! saved transfer transforms without rewriting fields; independent cross-component references
//! keep their live placement. The last release refuses the newer design-format number rather
//! than reopening the design with the former double-placement behavior.
