//! Edge flanges can choose butt, partial overlap or full overlap per corner. The covering and
//! covered edge points and the partial overlap expression are saved on the feature. Older
//! designs need no transformation: without corner overrides they retain their existing joins.
//! A standalone SheetCornerJoin stores the body and two persistent flange tags, its corner type
//! and optional overlap expression, allowing flanges from separate features to be joined later.
