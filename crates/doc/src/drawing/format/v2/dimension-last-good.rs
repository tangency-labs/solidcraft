//! A drawing's dimensions save their last good value (`last_good`, W19d, #4896) when they have one:
//! what the dimension last read, so one that can't be measured any more still says what it was
//! after the design is reopened. An older drawing has none, and each dimension has one again from
//! its next read. The last release refuses a drawing saved with them, as a newer format.
