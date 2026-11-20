//! A drawing's views save what they show (`settings.occurrences` and `settings.hide`, #4816): the
//! occurrence paths each draws and leaves out, followed down its projected views until one sets its
//! own. An older drawing has neither, so each view draws every body the design places. The last
//! release refuses a drawing saved with them, as a newer format.
