//! A drawing's views save which state of the assembly each draws (`settings.exploded`, #13048):
//! `"assembled"`, or one of the design's exploded views by its id, followed down its projected
//! views until one sets its own. An older drawing has none, so each view draws the design as it
//! places its parts. The last release refuses a drawing saved with one, as a newer format.
