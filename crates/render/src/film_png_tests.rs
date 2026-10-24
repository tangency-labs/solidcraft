use super::*;
use crate::film::FilmAccumulator;

fn film(width: u32, height: u32, rgb: [f64; 3]) -> FilmAccumulator {
    let bytes = FilmAccumulator::required_bytes(width, height, 1).unwrap();
    let cancel = CancelToken::default();
    let mut film = FilmAccumulator::new(width, height, 1, bytes, &cancel, &mut |_| Ok(())).unwrap();
    let count = width as usize * height as usize;
    for start in (0..count).step_by(256) {
        let values = vec![rgb; (count - start).min(256)];
        film.add_range(start, &values, &cancel, &mut |_| Ok(())).unwrap();
    }
    film
}

fn finish(mut task: FilmPngTask<'_>, units: usize) -> CompletedPng {
    for _ in 0..100_000 {
        if let ResolvePoll::Complete(png) = task.step(units, &mut |_| Ok(())).unwrap() {
            return png;
        }
    }
    panic!("bounded film fixture did not complete");
}

#[test]
fn decoded_png_has_one_analytic_exposure_reinhard_srgb_transform() {
    for (ev, expected) in [(0.0, [188, 0, 0, 255]), (1.0, [213, 0, 0, 255])] {
        let film = film(1, 1, [1.0, 0.0, 0.0]);
        let pass = film.complete_pass(&CancelToken::default(), &mut |_| Ok(())).unwrap();
        let host = CpuHost::cooperative();
        let png = finish(FilmPngTask::new(pass, &host, ev, CancelToken::default()).unwrap(), 1);
        let decoded = image::load_from_memory(png.bytes()).unwrap().to_rgba8();
        assert_eq!(decoded.get_pixel(0, 0).0, expected);
        let reader = png::Decoder::new(std::io::Cursor::new(png.bytes())).read_info().unwrap();
        assert_eq!(reader.info().srgb, Some(png::SrgbRenderingIntent::Perceptual));
        assert_eq!(png.samples_per_pixel(), 1);
    }
}

#[test]
fn incremental_partitions_produce_identical_png_and_retain_same_host_lease() {
    let film = film(3, 5, [0.5, 2.0, 4.0]);
    let host = CpuHost::cooperative();
    let first = finish(
        FilmPngTask::new(film.complete_pass(&CancelToken::default(), &mut |_| Ok(())).unwrap(), &host, 0.0, CancelToken::default()).unwrap(),
        1,
    );
    let first_lease = host.admitted_bytes();
    assert!(first_lease >= first.bytes.capacity() as u64);
    let second = finish(
        FilmPngTask::new(film.complete_pass(&CancelToken::default(), &mut |_| Ok(())).unwrap(), &host, 0.0, CancelToken::default()).unwrap(),
        256,
    );
    assert_eq!(first.bytes(), second.bytes());
    assert_eq!(second.dimensions(), (3, 5));
    assert!(host.admitted_bytes() > first_lease);
    drop(second);
    assert_eq!(host.admitted_bytes(), first_lease);
    drop(first);
    assert_eq!(host.admitted_bytes(), 0);
}

#[test]
fn cancellation_at_resolution_encoding_and_finish_never_publishes() {
    for phase in 0..3 {
        let film = film(2, 2, [1.0; 3]);
        let cancel = CancelToken::default();
        let host = CpuHost::cooperative();
        let mut task = FilmPngTask::new(film.complete_pass(&cancel, &mut |_| Ok(())).unwrap(), &host, 0.0, cancel.clone()).unwrap();
        for _ in 0..100 {
            let reached = match phase {
                0 => task.pixel == 0,
                1 => task.writer.is_some() && task.row == 0,
                _ => task.row == 2,
            };
            if reached {
                break;
            }
            assert!(matches!(task.step(1, &mut |_| Ok(())).unwrap(), ResolvePoll::Pending(_)));
        }
        cancel.cancel();
        assert_eq!(task.step(1, &mut |_| Ok(())).err().unwrap().code, ErrorCode::Cancelled);
        assert!(task.terminal);
        assert_eq!(host.admitted_bytes(), 0);
    }
}

#[test]
fn exhausted_job_work_and_extreme_exposure_fail_without_png_or_leaked_lease() {
    for rgb in [[1.0; 3], [f64::MAX; 3]] {
        let film = film(1, 1, rgb);
        let host = CpuHost::cooperative();
        let mut task =
            FilmPngTask::new(film.complete_pass(&CancelToken::default(), &mut |_| Ok(())).unwrap(), &host, 16.0, CancelToken::default()).unwrap();
        let result = if rgb[0] == 1.0 { task.step(1, &mut |_| Err(limit("same job exhausted"))) } else { task.step(1, &mut |_| Ok(())) };
        assert!(result.is_err());
        assert_eq!(host.admitted_bytes(), 0);
    }
}

#[test]
fn snapshot_admission_covers_actual_capacities_and_dimension_limits() {
    assert!(layout(0, 1).is_err());
    assert!(layout(u32::MAX, 1).is_err());
    assert!(layout(4096, 4096).is_err());
    let film = film(2, 2, [1.0; 3]);
    let pass = film.complete_pass(&CancelToken::default(), &mut |_| Ok(())).unwrap();
    let host = CpuHost::cooperative();
    let needed = FilmPngTask::required_bytes(&pass).unwrap();
    let task = FilmPngTask::new(pass, &host, 0.0, CancelToken::default()).unwrap();
    assert_eq!(host.admitted_bytes(), needed);
    let (rgba, encoded, _) = layout(2, 2).unwrap();
    assert_eq!(task.rgba.capacity(), rgba);
    assert_eq!(task.encoded.capacity(), encoded);
    drop(task);
    assert_eq!(host.admitted_bytes(), 0);
    assert!(
        FilmPngTask::new(film.complete_pass(&CancelToken::default(), &mut |_| Ok(())).unwrap(), &host, f64::NAN, CancelToken::default()).is_err()
    );
    assert_eq!(host.admitted_bytes(), 0);
}
