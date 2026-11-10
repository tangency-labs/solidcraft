//! Glass assignments can save optional `model` roughness, Abbe dispersion and a
//! coherent dielectric film (IOR and thickness in nanometres). Omission retains
//! the preceding smooth, nondispersive interface and incoherent sheet response.
//! CPU and GPU jobs can save optional `caustics` photon count, gather radius in
//! millimetres, light-path depth and seed. Omission creates no photon map and
//! retains preceding sampling. Older releases ask for an update rather than
//! discard these authored material and light-transport choices. No migration of
//! existing designs, geometry, engineering material or saved camera is required.
