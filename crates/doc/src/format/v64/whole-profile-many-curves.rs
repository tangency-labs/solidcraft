//! A profile pick that names every curve around a profile and its holes (`whole_boundaries`) may
//! now name as many curves as a sketch holds: up to 200 000 around one loop and 400 000 in all,
//! where it stopped at 512 around a loop and 20 000 in all. The outline of a line of script text,
//! or its offset, has more than 512 curves. What is saved is unchanged, and older designs are read
//! as they were. The last release warns about the newer format number, opens the design and keeps
//! a feature whose pick names more curves than it reads as a feature it can't read.
