//! Optional CPU progressive camera-sampling intent in RenderSetup.job.options.
//! Missing/null progressive retains fixed CPU sampling. The saved value contains
//! only bounded samples_per_pixel and a u32 seed; checkpoints, film buffers, PNGs,
//! cancellation, host leases and job identities remain transient runtime state.
