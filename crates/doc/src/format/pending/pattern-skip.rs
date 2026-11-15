//! A Pattern feature can carry `skip`: the numbers of the pattern instances it leaves out (1 is
//! the original, which is never skipped; 2 is the first copy). Older designs have no `skip`, so
//! nothing in them is rewritten and every copy is made as before. The last release warns about
//! the newer format, opens what it can read and makes every copy of such a pattern, the skipped
//! ones included.
