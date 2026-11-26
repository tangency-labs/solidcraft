use super::*;

#[cfg(feature = "raytrace-cpu")]
mod camera_passes {
    use super::primary::PrimaryFilm;
    use super::*;
    use crate::job::CpuProgressiveOptions;

    fn primary() -> PrimaryFilm {
        let options = CpuProgressiveOptions { samples_per_pixel: 4, seed: 7 };
        let bytes = PrimaryFilm::required_bytes(2, 1, options, crate::job::CpuBudget::Compact).unwrap();
        let mut film = PrimaryFilm::new(2, 1, options, bytes, &CancelToken::default(), crate::job::CpuBudget::Compact).unwrap();
        assert!(!film.ready());
        let mut charged = 0;
        while !film.ready() {
            film.initialize_one(&CancelToken::default(), &mut || {
                charged += 1;
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(charged, 2);
        film
    }

    #[test]
    fn camera_film_keeps_linear_sums_coverage_and_uniform_absolute_passes() {
        let mut film = primary();
        let cancel = CancelToken::default();
        assert!(film.pixel(0).is_err());
        for ordinal in 0..4 {
            film.add(0, ordinal, [2.0, 4.0, 8.0], true, &cancel, &mut || Ok(())).unwrap();
            assert!(film.pixel(0).is_err(), "a partial pass cannot be resolved");
            assert!(film.add(0, ordinal, [0.0; 3], false, &cancel, &mut || Ok(())).is_err(), "duplicate pixel must not consume a sample");
            let covered = ordinal % 2 == 0;
            film.add(1, ordinal, if covered { [6.0; 3] } else { [0.0; 3] }, covered, &cancel, &mut || Ok(())).unwrap();
            assert_eq!(film.samples(), ordinal + 1);
        }
        assert_eq!(film.pixel(0).unwrap(), ([2.0, 4.0, 8.0], 4, 4));
        assert_eq!(film.pixel(1).unwrap(), ([3.0; 3], 2, 4));
        assert!(film.add(0, 4, [1.0; 3], true, &cancel, &mut || Ok(())).is_err());
    }

    #[test]
    fn camera_film_resolves_65536_exact_samples_and_half_coverage() {
        let samples = crate::job::MAX_SAMPLES_PER_PIXEL;
        let options = CpuProgressiveOptions { samples_per_pixel: samples, seed: 7 };
        let budget = crate::job::CpuBudget::Workstation;
        let bytes = PrimaryFilm::required_bytes(1, 1, options, budget).unwrap();
        let cancel = CancelToken::default();
        let mut film = PrimaryFilm::new(1, 1, options, bytes, &cancel, budget).unwrap();
        film.initialize_one(&cancel, &mut || Ok(())).unwrap();
        for ordinal in 0..samples {
            let covered = ordinal % 2 == 0;
            film.add(0, ordinal, if covered { [2.0, 4.0, 8.0] } else { [0.0; 3] }, covered, &cancel, &mut || Ok(())).unwrap();
        }
        assert_eq!(film.pixel(0).unwrap(), ([1.0, 2.0, 4.0], samples / 2, samples));
        assert!(film.add(0, samples, [0.0; 3], false, &cancel, &mut || Ok(())).is_err());
        assert_eq!(film.samples(), samples);
    }

    #[test]
    fn camera_film_cancellation_invalid_radiance_and_work_failure_are_atomic() {
        let mut film = primary();
        let cancel = CancelToken::default();
        let observer = cancel.clone();
        assert_eq!(
            film.add(0, 0, [1.0; 3], true, &cancel, &mut || {
                observer.cancel();
                Ok(())
            })
            .unwrap_err()
            .code,
            ErrorCode::Cancelled
        );
        for (rgb, covered) in [([1.0; 3], false), ([f64::NAN; 3], true), ([-1.0; 3], true)] {
            assert!(film.add(0, 0, rgb, covered, &CancelToken::default(), &mut || Ok(())).is_err());
        }
        assert!(film.add(0, 0, [1.0; 3], true, &CancelToken::default(), &mut || Err(limit("work exhausted"))).is_err());
        film.add(0, 0, [4.0; 3], true, &CancelToken::default(), &mut || Ok(())).unwrap();
        film.add(1, 0, [0.0; 3], false, &CancelToken::default(), &mut || Ok(())).unwrap();
        assert_eq!(film.pixel(0).unwrap(), ([4.0; 3], 1, 1));
        assert_eq!(film.pixel(1).unwrap(), ([0.0; 3], 0, 1));
    }
}

fn film(width: u32, height: u32, limit: u32) -> FilmAccumulator {
    let bytes = FilmAccumulator::required_bytes(width, height, limit).unwrap();
    FilmAccumulator::new(width, height, limit, bytes, &CancelToken::default(), &mut |_| Ok(())).unwrap()
}

fn add(film: &mut FilmAccumulator, start: usize, values: &[[f64; 3]]) {
    film.add_range(start, values, &CancelToken::default(), &mut |_| Ok(())).unwrap();
}

#[test]
fn constant_and_alternating_samples_have_analytic_statistics() {
    let mut film = film(2, 1, 4);
    for value in [0.0, 2.0, 0.0, 2.0] {
        add(&mut film, 0, &[[7.0, 3.0, 1.0], [value; 3]]);
    }
    assert_eq!(film.pixel_statistics(0).unwrap(), PixelStatistics { mean: [7.0, 3.0, 1.0], variance: [0.0; 3], count: 4 });
    let statistics = film.pixel_statistics(1).unwrap();
    assert_eq!(statistics.mean, [1.0; 3]);
    assert_eq!(statistics.variance, [4.0 / 3.0; 3]);
}

#[test]
fn complete_pass_refuses_empty_or_uneven_counts_and_keeps_linear_values() {
    let mut film = film(2, 1, 3);
    assert!(film.complete_pass(&CancelToken::default(), &mut |_| Ok(())).is_err());
    add(&mut film, 0, &[[20.0, 2.0, 0.0]]);
    assert!(film.complete_pass(&CancelToken::default(), &mut |_| Ok(())).is_err());
    add(&mut film, 1, &[[4.0, 8.0, 16.0]]);
    let pass = film.complete_pass(&CancelToken::default(), &mut |_| Ok(())).unwrap();
    assert_eq!(pass.samples_per_pixel(), 1);
    assert_eq!(pass.dimensions(), (2, 1));
    assert_eq!(pass.pixels().collect::<Vec<_>>(), vec![[20.0, 2.0, 0.0], [4.0, 8.0, 16.0]]);
}

#[test]
fn invalid_sample_at_end_leaves_entire_update_unchanged() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        let mut film = film(2, 1, 3);
        add(&mut film, 0, &[[1.0; 3], [2.0; 3]]);
        let before = film.pixels.clone();
        assert_eq!(film.add_range(0, &[[3.0; 3], [bad; 3]], &CancelToken::default(), &mut |_| Ok(())).unwrap_err().code, ErrorCode::InvalidRequest);
        assert_eq!(film.pixels, before);
    }
}

#[test]
fn sample_limit_and_numeric_overflow_refuse_atomically() {
    let mut limited = film(2, 1, 1);
    add(&mut limited, 1, &[[1.0; 3]]);
    let before = limited.pixels.clone();
    assert_eq!(limited.add_range(0, &[[1.0; 3]; 2], &CancelToken::default(), &mut |_| Ok(())).unwrap_err().code, ErrorCode::LimitExceeded);
    assert_eq!(limited.pixels, before);
    let mut extreme = film(2, 1, 3);
    add(&mut extreme, 0, &[[1.0; 3], [f64::MAX; 3]]);
    let before = extreme.pixels.clone();
    assert!(extreme.add_range(0, &[[2.0; 3], [0.0; 3]], &CancelToken::default(), &mut |_| Ok(())).is_err());
    assert_eq!(extreme.pixels, before);
}

#[test]
fn dimensions_bytes_and_actual_capacity_are_checked() {
    for (width, height, samples) in [(0, 1, 1), (1, 0, 1), (u32::MAX, u32::MAX, 1), (1, 1, 0), (1, 1, MAX_SAMPLES + 1)] {
        assert!(FilmAccumulator::required_bytes(width, height, samples).is_err());
    }
    assert!(FilmAccumulator::required_bytes(MAX_DIMENSION, MAX_DIMENSION, 1).is_err());
    assert!(bytes_for_capacity(usize::MAX).is_err());
    let bytes = FilmAccumulator::required_bytes(2, 2, 1).unwrap();
    assert!(FilmAccumulator::new(2, 2, 1, bytes - 1, &CancelToken::default(), &mut |_| Ok(())).is_err());
    let film = film(2, 2, 1);
    assert!(film.allocated_bytes().unwrap() <= bytes);
    assert_eq!(film.allocated_bytes().unwrap(), bytes_for_capacity(film.pixels.capacity()).unwrap());
    assert!(film.allocated_bytes().unwrap() >= (std::mem::size_of::<Pixel>() * (4 + MAX_UPDATE)) as u64);
}

#[test]
fn invalid_ranges_and_oversized_updates_preserve_state() {
    let mut film = film(2, 1, 1);
    for (start, values) in [(0, Vec::new()), (usize::MAX, vec![[1.0; 3]]), (2, vec![[1.0; 3]]), (0, vec![[1.0; 3]; MAX_UPDATE + 1])] {
        assert!(film.add_range(start, &values, &CancelToken::default(), &mut |_| Ok(())).is_err());
        assert!(film.pixels.iter().all(|p| *p == Pixel::default()));
    }
    assert!(film.pixel_statistics(2).is_err());
}

#[test]
fn incremental_partitions_preserve_bitwise_results_in_sample_order() {
    let mut whole = film(4, 1, 3);
    let mut partitioned = film(4, 1, 3);
    for values in [
        [[1.0; 3], [1e12; 3], [0.25; 3], [4.0; 3]],
        [[2.0; 3], [1e12 + 1.0; 3], [0.5; 3], [2.0; 3]],
        [[4.0; 3], [1e12 - 1.0; 3], [0.75; 3], [0.0; 3]],
    ] {
        add(&mut whole, 0, &values);
        add(&mut partitioned, 0, &values[..1]);
        add(&mut partitioned, 1, &values[1..3]);
        add(&mut partitioned, 3, &values[3..]);
    }
    assert_eq!(whole.pixels, partitioned.pixels);
    assert_eq!(whole.pixel_statistics(1).unwrap().variance, [1.0; 3]);
}

#[test]
fn cancellation_and_work_failure_preserve_all_pixels() {
    let mut film = film(2, 1, 3);
    let cancel = CancelToken::default();
    let observer = cancel.clone();
    assert_eq!(
        film.add_range(0, &[[1.0; 3]; 2], &cancel, &mut |_| {
            observer.cancel();
            Ok(())
        })
        .unwrap_err()
        .code,
        ErrorCode::Cancelled
    );
    assert!(film.pixels.iter().all(|p| *p == Pixel::default()));
    let mut charged = 0;
    assert_eq!(
        film.add_range(0, &[[1.0; 3]; 2], &CancelToken::default(), &mut |units| {
            charged += units;
            if charged > 2 { Err(limit("job work exhausted")) } else { Ok(()) }
        })
        .unwrap_err()
        .code,
        ErrorCode::LimitExceeded
    );
    assert_eq!(charged, 4);
    assert!(film.pixels.iter().all(|p| *p == Pixel::default()));
}

#[test]
fn construction_and_complete_pass_share_caller_work_and_cancel() {
    let bytes = FilmAccumulator::required_bytes(2, 1, 1).unwrap();
    let cancel = CancelToken::default();
    cancel.cancel();
    assert!(FilmAccumulator::new(2, 1, 1, bytes, &cancel, &mut |_| panic!("cancelled construction charged work")).is_err());
    let mut charged = 0;
    let mut film = FilmAccumulator::new(2, 1, 1, bytes, &CancelToken::default(), &mut |n| {
        charged += n;
        Ok(())
    })
    .unwrap();
    assert_eq!(charged, 2);
    add(&mut film, 0, &[[1.0; 3]; 2]);
    assert!(film.complete_pass(&CancelToken::default(), &mut |_| Err(limit("job exhausted"))).is_err());
    let pass = film
        .complete_pass(&CancelToken::default(), &mut |n| {
            charged += n;
            Ok(())
        })
        .unwrap();
    assert_eq!(charged, 4);
    assert_eq!(pass.samples_per_pixel(), 1);
}

#[test]
fn sample_counts_reach_4096_and_the_declared_cap_without_truncation() {
    for samples in [4096, MAX_SAMPLES] {
        let mut film = film(1, 1, samples);
        let cancel = CancelToken::default();
        let mut charged = 0u64;
        for _ in 0..samples {
            film.add_range(0, &[[7.0, 3.0, 1.0]], &cancel, &mut |n| {
                charged += n;
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(charged, u64::from(samples) * 2);
        let expected = PixelStatistics { mean: [7.0, 3.0, 1.0], variance: [0.0; 3], count: samples };
        assert_eq!(film.pixel_statistics(0).unwrap(), expected);
        assert_eq!(film.complete_pass(&cancel, &mut |_| Ok(())).unwrap().samples_per_pixel(), samples);
        assert_eq!(film.add_range(0, &[[1.0; 3]], &cancel, &mut |_| Ok(())).unwrap_err().code, ErrorCode::LimitExceeded);
        assert_eq!(film.pixel_statistics(0).unwrap(), expected);
    }
}

#[test]
fn construction_stops_on_exhausted_caller_work() {
    let bytes = FilmAccumulator::required_bytes(4, 1, 1).unwrap();
    let mut charged = 0u64;
    let result = FilmAccumulator::new(4, 1, 1, bytes, &CancelToken::default(), &mut |n| {
        charged += n;
        if charged > 2 { Err(limit("existing job budget exhausted")) } else { Ok(()) }
    });
    assert!(matches!(result, Err(JobError { code: ErrorCode::LimitExceeded, .. })));
    assert_eq!(charged, 3);
}

#[cfg(feature = "raytrace-cpu")]
#[test]
fn primary_film_large_profile_keeps_every_sample_and_cancels_before_mutation() {
    use crate::film::primary::PrimaryFilm;
    use crate::job::{CancelToken, CpuBudget, CpuProgressiveOptions};
    let options = CpuProgressiveOptions { samples_per_pixel: 512, seed: 7 };
    assert!(PrimaryFilm::required_bytes(1, 1, options, CpuBudget::Compact).is_err());
    let bytes = PrimaryFilm::required_bytes(1, 1, options, CpuBudget::Studio).unwrap();
    let cancel = CancelToken::default();
    let mut film = PrimaryFilm::new(1, 1, options, bytes, &cancel, CpuBudget::Studio).unwrap();
    film.initialize_one(&cancel, &mut || Ok(())).unwrap();
    for ordinal in 0..512 {
        film.add(0, ordinal, [1e12, 2.0, 0.0], true, &cancel, &mut || Ok(())).unwrap();
    }
    assert_eq!(film.pixel(0).unwrap(), ([1e12, 2.0, 0.0], 512, 512));
    cancel.cancel();
    assert!(film.add(0, 512, [0.0; 3], true, &cancel, &mut || Ok(())).is_err());
    assert_eq!(film.samples(), 512);
}

mod cpu_adaptive_foundation {
    use super::adaptive::{AdaptiveCameraFilm, SamplingIdentity};
    use super::*;
    use crate::job::{
        CpuBudget, CpuProgressiveOptions,
        adaptive::{AdaptiveSampling, PreviewStage},
    };

    fn options() -> AdaptiveSampling {
        AdaptiveSampling { pilot_samples: 16, min_samples: 16, relative_error: 0.0, absolute_error: 0.1, coverage_error: 0.25 }
    }
    fn film(maximum: u32) -> AdaptiveCameraFilm {
        let production = CpuProgressiveOptions { samples_per_pixel: maximum, seed: 81 };
        let bytes = AdaptiveCameraFilm::required_bytes(2, 1, production, options(), CpuBudget::Compact).unwrap();
        AdaptiveCameraFilm::new(2, 1, production, options(), CpuBudget::Compact, bytes, &CancelToken::default()).unwrap()
    }
    fn source(index: usize, key: SamplingIdentity) -> ([f64; 3], bool) {
        match (key.stage, index) {
            (PreviewStage::Pilot, 0) => ([0.25, 0.5, 0.75], true),
            (PreviewStage::Pilot, _) => {
                if key.ordinal % 2 == 1 {
                    ([1.0, 0.0, 0.0], true)
                } else {
                    ([0.0; 3], false)
                }
            }
            (PreviewStage::Production, 0) => ([3.0, 1.0, 2.0], true),
            (PreviewStage::Production, _) => {
                if key.ordinal % 6 != 5 {
                    ([6.0, 0.0, 0.0], true)
                } else {
                    ([0.0; 3], false)
                }
            }
        }
    }
    fn step(film: &mut AdaptiveCameraFilm, work: &mut u64) {
        let cancel = CancelToken::default();
        let mut charge = || {
            *work += 1;
            Ok(())
        };
        if !film.ready() {
            film.initialize_one(&cancel, &mut charge).unwrap();
        } else if film.resetting() {
            film.reset_one(&cancel, &mut charge).unwrap();
        } else if film.complete_pass().is_ok_and(|pass| pass.metadata().stage == PreviewStage::Pilot && pass.metadata().completed == 16) {
            film.begin_production(&cancel, &mut charge).unwrap();
        } else {
            let index = film.next_pixel();
            if let Some(identity) = film.sampling_identity(index).unwrap() {
                let (rgb, covered) = source(index, identity);
                film.add(index, identity, rgb, covered, &cancel, &mut charge).unwrap();
            } else {
                film.skip_one(index, &cancel, &mut charge).unwrap();
            }
        }
    }
    type Snapshot = (PreviewStage, u32, Vec<([f64; 3], f64, u32, u32)>);
    fn snapshot(film: &AdaptiveCameraFilm) -> Option<Snapshot> {
        let pass = film.complete_pass().ok()?;
        let metadata = pass.metadata();
        let pixels = (0..pass.counts().len())
            .map(|index| {
                let (mean, alpha) = pass.pixel(index).unwrap();
                let count = pass.counts().pixel(index).unwrap();
                (mean, alpha, count.covered(), count.samples())
            })
            .collect();
        Some((metadata.stage, metadata.completed, pixels))
    }
    fn complete(maximum: u32, quantum: usize) -> (Vec<Snapshot>, crate::job::adaptive::SamplingReport, u64) {
        let mut film = film(maximum);
        let mut work = 0;
        let mut snapshots = Vec::new();
        for _ in 0..10_000 {
            for _ in 0..quantum {
                if film.is_final() {
                    break;
                }
                step(&mut film, &mut work);
                if let Some(next) = snapshot(&film)
                    && snapshots.last() != Some(&next)
                {
                    snapshots.push(next);
                }
            }
            if film.is_final() {
                return (snapshots, film.report().unwrap(), work);
            }
        }
        panic!("bounded adaptive foundation did not complete");
    }
    #[test]
    fn cpu_adaptive_film_independent_variance_allocation_discards_pilots_and_normalizes_heterogeneous_production() {
        let mut film = film(48);
        let mut work = 0;
        while film.completed_passes() < 16 {
            step(&mut film, &mut work);
        }
        let pilot = snapshot(&film).unwrap();
        assert_eq!(pilot.2[0], ([0.25, 0.5, 0.75], 1.0, 16, 16));
        assert_eq!(pilot.2[1], ([0.5, 0.0, 0.0], 0.5, 8, 16));
        assert_eq!(film.production_target(0).unwrap(), 16);
        // Eight0/eight1 pilots have M2=4, sample variance4/15. With
        // absolute RGB error0.1, ceil((4/15)/0.01)=27; alpha uses0.25.
        assert_eq!(film.production_target(1).unwrap(), 27);
        while !film.is_final() {
            step(&mut film, &mut work);
        }
        let final_pass = film.complete_pass().unwrap();
        assert_eq!(final_pass.pixel(0).unwrap(), ([3.0, 1.0, 2.0], 1.0), "pilot radiance is discarded rather than stopped into production");
        assert_eq!((final_pass.counts().pixel(0).unwrap().samples(), final_pass.counts().pixel(1).unwrap().samples()), (16, 27));
        let (mean, alpha) = final_pass.pixel(1).unwrap();
        assert_eq!(mean, [138.0 / 27.0, 0.0, 0.0]);
        assert_eq!(alpha, 23.0 / 27.0);
        let report = film.report().unwrap();
        assert_eq!((report.pilot_samples_per_pixel, report.minimum_production_samples, report.maximum_production_samples), (16, 16, 27));
        assert_eq!((report.production_samples, report.primary_path_evaluations, report.pixels_at_cap, report.transmission_strata), (43, 75, 0, 1));
        assert_eq!(work, 133, "init/reset and each sampled/skipped pixel own one exact work unit");
    }
    #[test]
    fn cpu_adaptive_counts_keep_five_of_six_alpha_exact_without_quantizing_raw_radiance() {
        let mut film = film(48);
        let mut work = 0;
        loop {
            if let Some((PreviewStage::Production, 6, _)) = snapshot(&film) {
                break;
            }
            step(&mut film, &mut work);
        }
        let pass = film.complete_pass().unwrap();
        let count = pass.counts().pixel(1).unwrap();
        assert_eq!((count.covered(), count.samples(), count.alpha8().unwrap()), (5, 6, 213));
        let (raw, alpha) = pass.pixel(1).unwrap();
        assert_eq!(raw, [5.0, 0.0, 0.0]);
        assert_eq!(alpha, 5.0 / 6.0);
        assert_eq!(alpha as f32, 5.0f32 / 6.0);
        assert_ne!(alpha as f32, 213.0f32 / 255.0, "PNG byte rounding cannot alter the raw FLOAT32 alpha");
    }
    #[test]
    fn cpu_adaptive_all_complete_passes_and_production_prefixes_are_quantum_and_cap_independent() {
        let (snapshots, report, work) = complete(48, 1);
        assert_eq!((snapshots.clone(), report.clone(), work), complete(48, 16384));
        assert_eq!(snapshots.len(), 64);
        assert_eq!(snapshots.iter().filter(|s| s.0 == PreviewStage::Pilot).count(), 16);
        assert_eq!(snapshots.last().unwrap().1, 48);
        for maximum in 16..=48 {
            let (prefix, _, _) = complete(maximum, 7);
            assert_eq!(&prefix[..16], &snapshots[..16], "the production cap never enters the pilot stream");
            assert_eq!(prefix.last().unwrap(), snapshots.iter().find(|s| s.0 == PreviewStage::Production && s.1 == maximum).unwrap());
        }
    }
    #[test]
    fn cpu_adaptive_sample_identities_separate_pilots_and_preserve_original_production_ordinals() {
        let mut film = film(48);
        let mut work = 0;
        while !film.ready() {
            step(&mut film, &mut work);
        }
        let pilot = film.sampling_identity(0).unwrap().unwrap();
        assert_eq!((pilot.seed, pilot.ordinal, pilot.stage), (81 ^ 0xa511_e9b3, 0, PreviewStage::Pilot));
        while film.complete_pass().is_err() || film.complete_pass().unwrap().metadata().stage == PreviewStage::Pilot {
            step(&mut film, &mut work);
        }
        // A complete first production pass has consumed ordinal0, so the next
        // active pixel receives original seed81 and absolute production ordinal1.
        let production = film.sampling_identity(0).unwrap().unwrap();
        assert_eq!((production.seed, production.ordinal, production.stage), (81, 1, PreviewStage::Production));
        assert_ne!(pilot.seed, production.seed);
        while film.completed_passes() < 32 {
            step(&mut film, &mut work);
        }
        assert!(film.sampling_identity(0).unwrap().is_none(), "a stopped pixel requests bookkeeping rather than another sample");
        assert!(film.complete_pass().unwrap().pixel(2).is_err());
    }
    #[test]
    fn cpu_adaptive_parallel_identity_lookahead_keeps_owner_publication_ordered() {
        let mut candidate = film(48);
        let mut reference = film(48);
        let mut work = 0;
        let mut reference_work = 0;
        let cancel = CancelToken::default();
        for _ in 0..2 {
            assert!(!candidate.ready());
            for index in [0, 1, 2, usize::MAX] {
                assert!(candidate.sampling_identity_in_pass(index).is_err(), "uninitialized pixel {index}");
            }
            step(&mut candidate, &mut work);
            step(&mut reference, &mut reference_work);
        }
        assert!(candidate.ready());
        let pilot = candidate.sampling_identity(0).unwrap().unwrap();
        assert_eq!(candidate.sampling_identity_in_pass(0).unwrap(), Some(pilot));
        assert_eq!(candidate.sampling_identity_in_pass(1).unwrap(), Some(pilot));
        for index in [2, usize::MAX] {
            assert!(candidate.sampling_identity_in_pass(index).is_err());
        }
        assert!(candidate.sampling_identity(1).is_err(), "owner lookup still requires the next pixel");
        let state = |film: &AdaptiveCameraFilm| (film.initialized(), film.ready(), film.next_pixel(), film.completed_passes(), snapshot(film));
        let before = state(&candidate);
        let (rgb, covered) = source(1, pilot);
        let mut debits = 0;
        assert_eq!(
            candidate
                .add(1, pilot, rgb, covered, &cancel, &mut || {
                    debits += 1;
                    Ok(())
                })
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            candidate
                .skip_one(1, &cancel, &mut || {
                    debits += 1;
                    Ok(())
                })
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(debits, 0, "lookahead never authorizes an out-of-order owner transaction");
        assert_eq!(state(&candidate), before);
        step(&mut candidate, &mut work);
        step(&mut reference, &mut reference_work);
        assert_eq!(candidate.next_pixel(), 1);
        assert!(candidate.sampling_identity_in_pass(0).is_err(), "a committed pixel cannot be sampled again in this sweep");
        assert_eq!(candidate.sampling_identity_in_pass(1).unwrap(), Some(pilot));
        assert_eq!(candidate.sampling_identity(1).unwrap(), Some(pilot));
        let before = state(&candidate);
        let (rgb, covered) = source(0, pilot);
        assert_eq!(
            candidate
                .add(0, pilot, rgb, covered, &cancel, &mut || {
                    debits += 1;
                    Ok(())
                })
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(debits, 0);
        assert_eq!(state(&candidate), before);
        step(&mut candidate, &mut work);
        step(&mut reference, &mut reference_work);
        assert_eq!(snapshot(&candidate), snapshot(&reference), "failed owner writes cannot alter the completed film");
        assert_eq!(work, reference_work);
    }

    #[test]
    fn cpu_adaptive_parallel_identity_lookahead_respects_stage_boundaries_and_stopped_pixels() {
        let mut candidate = film(48);
        let mut work = 0;
        let cancel = CancelToken::default();
        while candidate.completed_passes() < 16 {
            step(&mut candidate, &mut work);
        }
        assert_eq!(candidate.next_pixel(), 0);
        for index in [0, 1, 2, usize::MAX] {
            assert!(candidate.sampling_identity_in_pass(index).is_err(), "complete pilots require reset before lookahead");
        }
        candidate
            .begin_production(&cancel, &mut || {
                work += 1;
                Ok(())
            })
            .unwrap();
        assert!(candidate.resetting());
        while candidate.resetting() {
            for index in [0, 1, 2, usize::MAX] {
                assert!(candidate.sampling_identity_in_pass(index).is_err(), "partial reset has no sample identities");
            }
            step(&mut candidate, &mut work);
        }
        let first = candidate.sampling_identity_in_pass(0).unwrap().unwrap();
        assert_eq!((first.seed, first.ordinal, first.stage), (81, 0, PreviewStage::Production));
        assert_eq!(candidate.sampling_identity_in_pass(1).unwrap(), Some(first));
        while candidate.completed_passes() < 32 {
            step(&mut candidate, &mut work);
        }
        assert_eq!(candidate.sampling_identity_in_pass(0).unwrap(), None);
        let future = candidate.sampling_identity_in_pass(1).unwrap().unwrap();
        assert_eq!((future.seed, future.ordinal, future.stage), (81, 16, PreviewStage::Production));
        assert!(candidate.sampling_identity(1).is_err());
        step(&mut candidate, &mut work);
        assert_eq!(candidate.next_pixel(), 1);
        assert!(candidate.sampling_identity_in_pass(0).is_err());
        assert_eq!(candidate.sampling_identity_in_pass(1).unwrap(), Some(future));
        while candidate.completed_passes() < 43 {
            step(&mut candidate, &mut work);
        }
        assert_eq!(candidate.next_pixel(), 0);
        assert_eq!(candidate.sampling_identity_in_pass(0).unwrap(), None);
        assert_eq!(candidate.sampling_identity_in_pass(1).unwrap(), None, "a future stopped pixel consumes no variate");
        let state = |film: &AdaptiveCameraFilm| (film.next_pixel(), film.completed_passes(), snapshot(film));
        let before = state(&candidate);
        let mut debits = 0;
        assert_eq!(
            candidate
                .skip_one(1, &cancel, &mut || {
                    debits += 1;
                    Ok(())
                })
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(debits, 0, "a stopped future pixel still cannot be published ahead of the owner cursor");
        assert_eq!(state(&candidate), before);
        step(&mut candidate, &mut work);
        assert_eq!(candidate.next_pixel(), 1);
        let before = state(&candidate);
        assert!(candidate.sampling_identity_in_pass(0).is_err());
        assert_eq!(candidate.sampling_identity_in_pass(1).unwrap(), None);
        assert_eq!(
            candidate
                .skip_one(0, &cancel, &mut || {
                    debits += 1;
                    Ok(())
                })
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(debits, 0);
        assert_eq!(state(&candidate), before);
        while !candidate.is_final() {
            step(&mut candidate, &mut work);
        }
        for index in [0, 1, 2, usize::MAX] {
            assert!(candidate.sampling_identity_in_pass(index).is_err(), "finished films have no active sweep");
        }
        let final_pass = candidate.complete_pass().unwrap();
        assert_eq!((final_pass.counts().pixel(0).unwrap().samples(), final_pass.counts().pixel(1).unwrap().samples()), (16, 27));
        assert_eq!(work, 133, "lookahead and rejected owner operations consume no additional work");
    }

    #[test]
    fn cpu_adaptive_pilots_plus_production_obey_every_unchanged_profile_and_camera_work_cap() {
        for budget in CpuBudget::ALL {
            let maximum = budget.limits().max_samples_per_pixel;
            let make = |production| CpuProgressiveOptions { samples_per_pixel: production, seed: 0 };
            assert!(AdaptiveCameraFilm::required_bytes(1, 1, make(maximum - 16), options(), budget).is_ok());
            assert!(AdaptiveCameraFilm::required_bytes(1, 1, make(maximum - 15), options(), budget).is_err());
            assert!(AdaptiveCameraFilm::required_bytes(1, 1, make(maximum), options(), budget).is_err());
        }
        let production = CpuProgressiveOptions { samples_per_pixel: 16, seed: 0 };
        assert!(AdaptiveCameraFilm::required_bytes(1024, 512, production, options(), CpuBudget::Compact).is_ok());
        assert!(
            AdaptiveCameraFilm::required_bytes(1024, 513, production, options(), CpuBudget::Compact).is_err(),
            "pilot camera paths are included in the same total sample budget"
        );
        let one = AdaptiveCameraFilm::required_bytes(1, 1, production, options(), CpuBudget::Compact).unwrap();
        let two = AdaptiveCameraFilm::required_bytes(2, 1, production, options(), CpuBudget::Compact).unwrap();
        assert_eq!(two - one, 76, "64-byte moments plus12-byte exact count/allocation plane, with no snapshot plane");
        assert!(AdaptiveCameraFilm::new(1, 1, production, options(), CpuBudget::Compact, one - 1, &CancelToken::default()).is_err());
        assert!(AdaptiveCameraFilm::required_bytes(0, 1, production, options(), CpuBudget::Compact).is_err());
        assert!(
            AdaptiveCameraFilm::required_bytes(1, 1, production, AdaptiveSampling { pilot_samples: 15, ..options() }, CpuBudget::Compact).is_err()
        );
        assert!(
            AdaptiveCameraFilm::required_bytes(1, 1, production, AdaptiveSampling { relative_error: f64::NAN, ..options() }, CpuBudget::Compact)
                .is_err()
        );
    }
    #[test]
    fn cpu_adaptive_add_reset_skip_and_initialization_are_atomic_under_cancel_or_work_failure() {
        for operation in ["initialize", "pilot", "reset", "production", "skip"] {
            for cancelled in [false, true] {
                let mut film = film(48);
                let mut work = 0;
                if operation != "initialize" {
                    while !film.ready() {
                        step(&mut film, &mut work);
                    }
                }
                if matches!(operation, "reset" | "production" | "skip") {
                    while film.completed_passes() < 16 {
                        step(&mut film, &mut work);
                    }
                    film.begin_production(&CancelToken::default(), &mut || Ok(())).unwrap();
                }
                if matches!(operation, "production" | "skip") {
                    while film.resetting() {
                        step(&mut film, &mut work);
                    }
                }
                if operation == "skip" {
                    while film.completed_passes() < 32 {
                        step(&mut film, &mut work);
                    }
                }
                let before = (
                    film.initialized(),
                    film.ready(),
                    film.next_pixel(),
                    film.completed_passes(),
                    snapshot(&film),
                    film.sampling_identity(film.next_pixel()).ok(),
                );
                let cancel = CancelToken::default();
                let token = cancel.clone();
                let mut charge = || {
                    if cancelled {
                        token.cancel();
                        Ok(())
                    } else {
                        Err(JobError::new(ErrorCode::LimitExceeded, "test cumulative work exhausted"))
                    }
                };
                let result = match operation {
                    "initialize" => film.initialize_one(&cancel, &mut charge),
                    "reset" => film.reset_one(&cancel, &mut charge),
                    "skip" => film.skip_one(0, &cancel, &mut charge),
                    _ => {
                        let key = film.sampling_identity(0).unwrap().unwrap();
                        let (rgb, covered) = source(0, key);
                        film.add(0, key, rgb, covered, &cancel, &mut charge)
                    }
                };
                assert_eq!(result.unwrap_err().code, if cancelled { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded }, "{operation}");
                assert_eq!(
                    (
                        film.initialized(),
                        film.ready(),
                        film.next_pixel(),
                        film.completed_passes(),
                        snapshot(&film),
                        film.sampling_identity(film.next_pixel()).ok()
                    ),
                    before,
                    "{operation} may not partially consume its cursor/count/stream"
                );
                assert!(film.report().is_err());
            }
        }
    }
    #[test]
    fn cpu_adaptive_integration_requires_explicit_progressive_intent_and_preserves_strict_input() {
        assert!(serde_json::to_value(crate::job::RaytraceOptions::default()).unwrap().get("adaptive").is_none());
        let invalid: crate::job::RaytraceOptions = serde_json::from_value(serde_json::json!({"adaptive":options()})).unwrap();
        assert!(invalid.validate().is_err(), "adaptive cannot silently introduce a camera film into a fixed saved setup");
        assert!(serde_json::from_value::<crate::job::RaytraceOptions>(serde_json::json!({"adaptive":{"noise_guarantee":true}})).is_err());
        let mut film = film(48);
        let mut work = 0;
        while !film.ready() {
            step(&mut film, &mut work);
        }
        assert!(film.complete_pass().is_err());
        assert!(film.begin_production(&CancelToken::default(), &mut || Ok(())).is_err());
        let key = film.sampling_identity(0).unwrap().unwrap();
        for (rgb, covered) in [([f64::NAN, 0.0, 0.0], true), ([f64::INFINITY, 0.0, 0.0], true), ([1.0, 0.0, 0.0], false)] {
            assert!(film.add(0, key, rgb, covered, &CancelToken::default(), &mut || panic!("invalid input must not debit or mutate work")).is_err());
            assert_eq!(film.sampling_identity(0).unwrap().unwrap(), key);
        }
        assert!(film.add(1, key, [0.0; 3], false, &CancelToken::default(), &mut || Ok(())).is_err());
        assert!(film.skip_one(0, &CancelToken::default(), &mut || Ok(())).is_err());
    }
}
