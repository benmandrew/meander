//! Run the simulation natively, print statistics, and write SVG snapshots.
//!
//! cargo run --release -p meander-core --example snapshot -- <out_dir> [steps] [every] [seed]

use meander_core::{Params, Sim};
use std::fmt::Write as _;

fn svg(sim: &Sim) -> String {
    let [w, h] = sim.bounds;
    let mut s = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 {w} {h}' width='{w}' height='{h}'>\
         <rect width='100%' height='100%' fill='#d9d2b6'/>"
    );
    let path = |pts: &[[f64; 2]]| {
        let mut d = String::new();
        for (i, p) in pts.iter().enumerate() {
            let _ = write!(d, "{}{:.2},{:.2} ", if i == 0 { "M" } else { "L" }, p[0], p[1]);
        }
        d
    };
    for o in &sim.oxbows {
        let age = ((sim.time - o.born) / sim.params.oxbow_lifetime).clamp(0.0, 1.0);
        let _ = write!(
            s,
            "<path d='{}' fill='none' stroke='#5b8a9a' stroke-opacity='{:.2}' stroke-width='{:.2}' stroke-linejoin='round' stroke-linecap='round'/>",
            path(&o.points),
            1.0 - age,
            sim.params.width * (1.0 - 0.7 * age)
        );
    }
    let _ = write!(
        s,
        "<path d='{}' fill='none' stroke='#2f6f8f' stroke-width='{}' stroke-linejoin='round' stroke-linecap='round'/></svg>",
        path(&sim.centerline),
        sim.params.width
    );
    s
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).expect("usage: snapshot <out_dir> [steps] [every] [seed]");
    let steps: usize = args.get(2).map_or(20000, |s| s.parse().unwrap());
    let every: usize = args.get(3).map_or(2000, |s| s.parse().unwrap());
    let seed: u64 = args.get(4).map_or(1, |s| s.parse().unwrap());
    std::fs::create_dir_all(out).unwrap();

    // Parameter overrides from the environment, e.g. ALPHA=0.8, for tuning.
    let mut params = Params::default();
    for name in Params::NAMES {
        if let Ok(v) = std::env::var(name.to_uppercase()) {
            params.set(name, v.parse().unwrap());
        }
    }
    let mut sim = Sim::new(params, [1200.0, 700.0], seed, 0.5);
    let t0 = std::time::Instant::now();
    for k in 0..=steps / every {
        if k > 0 {
            sim.step_n(every);
        }
        println!(
            "step {:6}  t {:8.1}  n {:5}  sinuosity {:5.2}  cutoffs {:4}  oxbows {:3}",
            sim.steps,
            sim.time,
            sim.centerline.len(),
            sim.sinuosity(),
            sim.cutoffs,
            sim.oxbows.len()
        );
        std::fs::write(format!("{out}/step{:06}.svg", sim.steps), svg(&sim)).unwrap();
    }
    println!("{:.2} ms/step", t0.elapsed().as_secs_f64() * 1e3 / steps as f64);
}
