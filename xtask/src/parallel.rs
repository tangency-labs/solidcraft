//! Independent work spread over the machine's cores. The quick checks of `cargo xtask ci` read and
//! parse thousands of files and start several processes; one at a time that costs seconds a
//! step, most of all on Windows, where each file and process costs more.

use std::sync::atomic::{AtomicUsize, Ordering};

/// `f` of every item, on up to as many threads as the machine runs at once, in the items' order.
/// A thread that panics makes it an error, so no item is ever dropped unseen.
pub fn map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Result<Vec<R>, String> {
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(items.len()).max(1);
    let next = AtomicUsize::new(0);
    let finished = std::thread::scope(|scope| {
        let threads: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(i) else { break };
                        done.push((i, f(item)));
                    }
                    done
                })
            })
            .collect();
        threads.into_iter().map(|thread| thread.join()).collect::<Vec<_>>()
    });
    let mut all = Vec::with_capacity(items.len());
    for done in finished {
        all.extend(done.map_err(|_| "a worker thread panicked".to_string())?);
    }
    all.sort_by_key(|(i, _)| *i);
    Ok(all.into_iter().map(|(_, r)| r).collect())
}

#[cfg(test)]
mod tests {
    use super::map;

    #[test]
    fn every_item_once_in_order() {
        let items: Vec<usize> = (0..1000).collect();
        assert_eq!(map(&items, |i| i * 2).unwrap(), items.iter().map(|i| i * 2).collect::<Vec<_>>());
        assert!(map(&[] as &[u8], |b| *b).unwrap().is_empty());
    }

    #[test]
    fn a_panicking_item_is_an_error_not_a_gap() {
        let result = std::panic::catch_unwind(|| map(&[1, 2, 3], |i| if *i == 2 { panic!("item two") } else { *i }));
        assert!(matches!(result, Ok(Err(_))), "{result:?}");
    }
}
