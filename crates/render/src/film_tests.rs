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
