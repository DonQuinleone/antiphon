use std::path::PathBuf;
use std::time::Instant;

use antiphon_store::{SearchIndex, StoreLayout};

const LIST_WINDOW: usize = 500;
const RUNS: usize = 20;

fn main() {
    let root = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: querybench <store-root>");
        std::process::exit(2);
    });
    let layout = StoreLayout::new(PathBuf::from(root));

    let opened = Instant::now();
    let index = SearchIndex::open(&layout).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    report("open", opened.elapsed().as_secs_f64() * 1000.0);

    bench(&index, "list window", "*", Some(LIST_WINDOW));
    bench(&index, "unread scan", "tag:unread", Some(LIST_WINDOW));
    bench(
        &index,
        "scoped body search",
        "path:acct0/** and archive",
        Some(LIST_WINDOW),
    );
}

fn bench(
    index: &SearchIndex,
    label: &str,
    query: &str,
    limit: Option<usize>,
) {
    let mut total_rows = 0;
    let started = Instant::now();
    for _ in 0..RUNS {
        total_rows += index.query(query, limit).unwrap().len();
    }
    let mean_ms =
        started.elapsed().as_secs_f64() * 1000.0 / RUNS as f64;
    report(
        &format!("{label} ({} rows)", total_rows / RUNS),
        mean_ms,
    );
}

fn report(label: &str, ms: f64) {
    println!("{label}: {ms:.1} ms");
}
