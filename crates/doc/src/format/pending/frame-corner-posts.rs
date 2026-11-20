//! A design's `frame_corner` feature can list three `members` (#4003): the first two are mitred to
//! each other, and the third (a post) ends flat against them. Older designs need no transformation:
//! their corners list two members, which keep their meaning. The last release refuses a corner of
//! three members (it takes two).
