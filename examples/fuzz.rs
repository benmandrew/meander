//! Seed fuzzer: run the river as the page does, for random seeds and
//! window widths, check `Sim::check` after every step, and print a replay
//! link for each failure.
//!
//! cargo run --release -p meander-core --example fuzz -- [runs] [years] [base]
//!
//! `runs` defaults to 32 and `years`, after the page's warm-up of 1,250
//! years, to 2,500. `base` seeds the draws of seeds and widths, so a sweep
//! repeats exactly; it defaults to the clock and is printed. Exits with 1
//! if any run fails.

use meander_core::rng::Rng;
use meander_core::{Params, Sim};
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

// The page's settings, from `web/ui.js`.
const YEARS_PER_UNIT: f64 = 2.5;
const WARMUP: f64 = 500.0;
const NOISE: f64 = 0.5;
const VIEW_H: f64 = 480.0;
// Window aspect ratios, from a phone held upright to an ultrawide screen.
const ASPECT: [f64; 2] = [0.45, 3.5];

thread_local! {
    // The message of the last panic on this thread, with its location.
    static PANIC: RefCell<String> = const { RefCell::new(String::new()) };
}

struct Case {
    seed: u32,
    view_w: f64,
}

/// Run `case` for `steps` steps. On failure, returns the step and why.
fn run(case: &Case, steps: u64) -> Result<(), (u64, String)> {
    let mut sim = Sim::new(Params::default(), [VIEW_H, VIEW_H], 0, NOISE);
    sim.reset_view(case.view_w, VIEW_H, case.seed as u64, NOISE);
    let mut at = 0;
    let result = catch_unwind(AssertUnwindSafe(|| {
        while sim.steps < steps {
            at = sim.steps + 1;
            sim.step();
            sim.check().map_err(|m| (sim.steps, m))?;
        }
        Ok(())
    }));
    result.unwrap_or_else(|_| Err((at, PANIC.with(|p| p.borrow().clone()))))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |i: usize| {
        args.get(i)
            .map(|v| v.parse::<u64>().expect("arguments are integers"))
    };
    let runs = arg(1).unwrap_or(32) as usize;
    let years = arg(2).unwrap_or(2500) as f64;
    let base = arg(3).unwrap_or_else(|| {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
        now.map_or(1, |d| d.as_secs())
    });
    let dt = Params::default().dt;
    let warmup = (WARMUP / dt).round() as u64;
    let steps = warmup + (years / YEARS_PER_UNIT / dt).round() as u64;

    let mut rng = Rng::new(base);
    let cases: Vec<Case> = (0..runs)
        .map(|_| {
            let seed = (rng.next_u64() >> 32) as u32;
            let aspect = ASPECT[0] + (ASPECT[1] - ASPECT[0]) * rng.uniform();
            let view_w = (VIEW_H * aspect * 100.0).round() / 100.0;
            Case { seed, view_w }
        })
        .collect();
    println!("fuzz: {runs} runs of {years} years after the warm-up, base {base}");

    std::panic::set_hook(Box::new(|info| {
        PANIC.with(|p| *p.borrow_mut() = info.to_string())
    }));
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                while let Some(case) = cases.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let Err((step, why)) = run(case, steps) else {
                        continue;
                    };
                    let year = (step as f64 - warmup as f64) * dt * YEARS_PER_UNIT;
                    let (seed, view) = (case.seed, case.view_w);
                    println!(
                        "FAIL seed {seed} view {view}: {why}\n  at step {step}, year {year:.0}: \
                         http://localhost:8000/?seed={seed}&tick={step}&view={view}"
                    );
                    failures.lock().unwrap().push(seed);
                }
            });
        }
    });
    let failed = failures.into_inner().unwrap().len();
    println!("fuzz: {failed} of {runs} runs failed");
    if failed > 0 {
        std::process::exit(1);
    }
}
