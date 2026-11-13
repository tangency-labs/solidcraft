//! A construction reference to a body vertex saves the names of the edges that meet there
//! (`edges` beside `at`), so a plane, axis or point on a vertex follows it when an earlier change
//! moves it. Older designs need no transformation: a vertex reference without `edges` is the
//! vertex nearest its point, as before. 0.15.x asks for an update instead of reading `edges`.
