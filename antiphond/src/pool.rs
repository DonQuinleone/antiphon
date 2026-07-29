use std::sync::atomic::{AtomicUsize, Ordering};

/// Runs `task` over every item with at most `limit` running at
/// once, then returns when all have finished. A slow item holds
/// only its own worker: the others keep pulling from the shared
/// cursor, so one grinding account never stalls the rest.
pub(crate) fn run_bounded<T, F>(items: &[T], limit: usize, task: F)
where
    T: Sync,
    F: Fn(&T) + Sync {
    if items.is_empty() {
        return;
    }
    let workers = limit.max(1).min(items.len());
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| drain(items, &next, &task));
        }
    });
}

fn drain<T, F>(items: &[T], next: &AtomicUsize, task: &F)
where
    F: Fn(&T) {
    loop {
        let index = next.fetch_add(1, Ordering::Relaxed);
        let Some(item) = items.get(index) else {
            return;
        };
        task(item);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    #[test]
    fn every_item_runs_exactly_once() {
        let items: Vec<usize> = (0..50).collect();
        let seen: Vec<AtomicUsize> =
            (0..50).map(|_| AtomicUsize::new(0)).collect();
        run_bounded(&items, 4, |item| {
            seen[*item].fetch_add(1, Ordering::SeqCst);
        });
        assert!(seen.iter().all(|count| count.load(Ordering::SeqCst) == 1));
    }

    #[test]
    fn no_more_than_the_bound_run_at_once() {
        let items: Vec<usize> = (0..20).collect();
        let live = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        run_bounded(&items, 3, |_item| {
            let now = live.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(5));
            live.fetch_sub(1, Ordering::SeqCst);
        });
        assert!(peak.load(Ordering::SeqCst) <= 3);
        assert_eq!(live.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_slow_item_does_not_hold_back_the_others() {
        let items: Vec<usize> = (0..6).collect();
        let done = AtomicUsize::new(0);
        run_bounded(&items, 3, |item| {
            if *item == 0 {
                std::thread::sleep(Duration::from_millis(40));
            }
            done.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(done.load(Ordering::SeqCst), items.len());
    }

    #[test]
    fn an_empty_batch_spawns_nothing() {
        let items: Vec<usize> = Vec::new();
        run_bounded(&items, 4, |_item| panic!("must not run"));
    }
}
