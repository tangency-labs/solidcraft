//! Render jobs may save independent or scrambled-Sobol sampling, additive radiance
//! components, variance-guided and component-specific denoising, extended optical
//! endpoint guides, optical adaptive minimums and progressive photon radii with a
//! bounded pixel probe. An optional glossy-receiver base-roughness threshold
//! transfers smooth-glass area/environment paths to camera continuation. CPU path
//! options also retain first-Fresnel strata and eta-aware Russian roulette. CPU
//! and GPU options may opt into a bounded first bare smooth closed-interface
//! split; absent/false keeps ordinary branch sampling. Absent fields preserve
//! previous sampling, filtering, fixed photon radii and CPU termination. Earlier
//! releases ask for an update and preserve unknown render settings.
