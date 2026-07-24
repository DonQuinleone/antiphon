use std::path::PathBuf;
use std::time::Instant;

use antiphon_store::{Scope, SearchIndex, StoreLayout};

const LIST_WINDOW: usize = 500;
const RUNS: usize = 20;

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().unwrap_or_else(|| usage());
    let budget_ms = match (args.next().as_deref(), args.next()) {
        (None, _) => None,
        (Some("--assert-under"), Some(ms)) => {
            Some(ms.parse::<f64>().unwrap_or_else(|_| usage()))
        }
        _ => usage(),
    };
    let layout = StoreLayout::new(PathBuf::from(root));

    let opened = Instant::now();
    let index = SearchIndex::open(&layout).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    let mut worst: f64 = opened.elapsed().as_secs_f64() * 1000.0;
    report("open", worst);

    let queries = [
        ("list window", "*"),
        ("unread scan", "tag:unread"),
        ("scoped body search", "path:acct0/** and archive"),
    ];
    for (label, query) in queries {
        let mean = bench(&index, label, query, Some(LIST_WINDOW));
        worst = worst.max(mean);
    }
    let scope = Scope::one("acct0");
    let mean = bench_scoped(&index, "scope api window", &scope, "*");
    worst = worst.max(mean);
    let Some(budget_ms) = budget_ms else {
        return;
    };
    if worst > budget_ms {
        eprintln!(
            "budget exceeded: worst {worst:.1} ms > \
             {budget_ms:.1} ms"
        );
        std::process::exit(1);
    }
    println!("within budget: worst {worst:.1} ms");
}

fn usage() -> ! {
    eprintln!("usage: querybench <store-root> [--assert-under <ms>]");
    std::process::exit(2);
}

fn bench(
    index: &SearchIndex,
    label: &str,
    query: &str,
    limit: Option<usize>,
) -> f64 {
    let mut total_rows = 0;
    let started = Instant::now();
    for _ in 0..RUNS {
        total_rows += index.query(query, limit).unwrap().len();
    }
    let mean_ms =
        started.elapsed().as_secs_f64() * 1000.0 / RUNS as f64;
    report(&format!("{label} ({} rows)", total_rows / RUNS), mean_ms);
    mean_ms
}

fn report(label: &str, ms: f64) {
    println!("{label}: {ms:.1} ms");
}

fn bench_scoped(
    index: &SearchIndex,
    label: &str,
    scope: &Scope,
    query: &str,
) -> f64 {
    let mut total_rows = 0;
    let started = Instant::now();
    for _ in 0..RUNS {
        total_rows += index
            .query_scoped(scope, query, Some(LIST_WINDOW))
            .unwrap()
            .len();
    }
    let mean_ms =
        started.elapsed().as_secs_f64() * 1000.0 / RUNS as f64;
    report(&format!("{label} ({} rows)", total_rows / RUNS), mean_ms);
    mean_ms
}
