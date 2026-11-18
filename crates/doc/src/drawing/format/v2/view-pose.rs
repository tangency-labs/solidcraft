//! A drawing's views save the position of the assembly each draws (`settings.pose`, #13048):
//! `"designed"`, or some of the design's joints at values of their own, each by its id with its values
//! in radians and millimetres, followed down its projected views until one sets its own. An older
//! drawing has none, so each view draws the design as it places its parts. The last release refuses a
//! drawing saved with one, as a newer format.
