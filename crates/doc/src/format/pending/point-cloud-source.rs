//! A point cloud remembers the file it was read from (#8818): `source`, its path, which
//! `pointcloud.reload` reads again. Older designs need no transformation: their clouds have no
//! source and cannot be reloaded, only inserted again. The last release refuses a design saved
//! with this (the newer format number) instead of opening its clouds without their source.
