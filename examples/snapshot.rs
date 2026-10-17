//! Run the simulation natively, print statistics, and write SVG snapshots.
//!
//! cargo run --release -p meander-core --example snapshot -- <out_dir> [steps] [every] [seed]

use meander_core::geom::curvature;
use meander_core::{Params, Sim};
use std::fmt::Write as _;

/// Render the window `[x0, x0 + w] × [0, h]` of the domain.
fn svg(sim: &Sim, x0: f64, w: f64) -> String {
    let h = sim.bounds[1];
    let mut s = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='{x0} 0 {w} {h}' width='{w}' height='{h}'>\
         <rect x='{x0}' width='{w}' height='{h}' fill='#d9d2b6'/>"
    );
    // Landholdings: each a tint seeded by its index, with its boundary
    // with other holdings in dark green and its farmstead as a dot.
    let land = &sim.land;
    let (cols, c) = (land.cols, land.cell);
    let tint = |o: u32| {
        let h = (o as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40;
        format!(
            "rgb({},{},{})",
            150 + (h & 63),
            160 + ((h >> 6) & 63),
            110 + ((h >> 12) & 63)
        )
    };
    for y in 0..land.rows {
        for x in 0..cols {
            let o = land.owner[y * cols + x];
            if o != meander_core::land::NONE {
                let _ = write!(
                    s,
                    "<rect x='{:.1}' y='{:.1}' width='{c:.1}' height='{c:.1}' fill='{}'/>",
                    x as f64 * c,
                    y as f64 * c,
                    tint(o)
                );
            }
        }
    }
    let mut edges = String::new();
    for y in 0..land.rows {
        for x in 0..cols {
            let o = land.owner[y * cols + x];
            let (fx, fy) = (x as f64 * c, y as f64 * c);
            if x + 1 < cols && land.owner[y * cols + x + 1] != o {
                let _ = write!(edges, "M{:.1},{fy:.1}v{c:.1}", fx + c);
            }
            if y + 1 < land.rows && land.owner[(y + 1) * cols + x] != o {
                let _ = write!(edges, "M{fx:.1},{:.1}h{c:.1}", fy + c);
            }
        }
    }
    let _ = write!(
        s,
        "<path d='{edges}' stroke='#3c5a2c' stroke-width='0.8' fill='none'/>"
    );
    for h in land.holdings.iter().flatten() {
        let _ = write!(
            s,
            "<circle cx='{:.1}' cy='{:.1}' r='3' fill='#7a3b2a'/>",
            h.seat[0], h.seat[1]
        );
    }
    let path = |pts: &[[f64; 2]]| {
        let mut d = String::new();
        for (i, p) in pts.iter().enumerate() {
            let _ = write!(
                d,
                "{}{:.2},{:.2} ",
                if i == 0 { "M" } else { "L" },
                p[0],
                p[1]
            );
        }
        d
    };
    for o in &sim.oxbows {
        let age = o.fill;
        let _ = write!(
            s,
            "<path d='{}' fill='none' stroke='#5b8a9a' stroke-opacity='{:.2}' stroke-width='{:.2}' stroke-linejoin='round' stroke-linecap='round'/>",
            path(&o.points),
            1.0 - age,
            o.widths.iter().sum::<f64>() / o.widths.len() as f64
                * sim.params.width
                * (1.0 - 0.7 * age)
        );
    }
    // Tributary streams, red where they end in a lake.
    for t in &sim.tributaries {
        let colour = if t.lake { "#b0402a" } else { "#2f6f8f" };
        let _ = write!(
            s,
            "<path d='{}' fill='none' stroke='{colour}' stroke-width='{:.2}' stroke-linejoin='round'/>",
            path(&t.course),
            0.3 * sim.params.width
        );
    }
    // The channel as round-capped segments at the local width.
    let pts = &sim.centerline;
    for (k, seg) in pts.windows(2).enumerate() {
        let b = 0.5 * (sim.widths[k] + sim.widths[k + 1]);
        let _ = write!(
            s,
            "<path d='M{:.2},{:.2} L{:.2},{:.2}' stroke='#2f6f8f' stroke-width='{:.2}' stroke-linecap='round'/>",
            seg[0][0],
            seg[0][1],
            seg[1][0],
            seg[1][1],
            b * sim.params.width
        );
    }
    s.push_str("</svg>");
    s
}

/// Width quantiles (10%, median, 90%), and the mean width at gentle
/// (radius over 10 widths) and sharp (under 3 widths) bends.
fn width_stats(sim: &Sim) -> [f64; 5] {
    let mut c = Vec::new();
    curvature(&sim.centerline, &mut c);
    let w = sim.params.width;
    let mut ws = sim.widths.clone();
    ws.sort_by(f64::total_cmp);
    let q = |f: f64| ws[((ws.len() - 1) as f64 * f) as usize];
    let mean = |keep: &dyn Fn(f64) -> bool| {
        let (s, n) = c
            .iter()
            .zip(&sim.widths)
            .filter(|(k, _)| keep(k.abs() * w))
            .fold((0.0, 0), |(s, n), (_, b)| (s + b, n + 1));
        s / n.max(1) as f64
    };
    [
        q(0.1),
        q(0.5),
        q(0.9),
        mean(&|k| k < 0.1),
        mean(&|k| k > 1.0 / 3.0),
    ]
}

/// Fraction of interior points with bend radius under 1.5 channel widths.
fn tight_fraction(sim: &Sim) -> f64 {
    let mut c = Vec::new();
    curvature(&sim.centerline, &mut c);
    let w = sim.params.width;
    let tight = c.iter().filter(|k| k.abs() * w > 1.0 / 1.5).count();
    tight as f64 / c.len().max(1) as f64
}

/// Mean sinuosity of the reaches that set the channel slope.
fn reach_sinuosity(sim: &mut Sim) -> f64 {
    sim.migration_rate();
    sim.slope.iter().map(|s| 1.0 / s).sum::<f64>() / sim.slope.len().max(1) as f64
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args
        .get(1)
        .expect("usage: snapshot <out_dir> [steps] [every] [seed]");
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
    // Hidden reaches upstream and downstream of the rendered window, in
    // channel widths.
    let up = std::env::var("UP").map_or(0.0, |v| v.parse().unwrap()) * params.width;
    let down = std::env::var("DOWN").map_or(0.0, |v| v.parse().unwrap()) * params.width;
    let view_w = 840.0;
    let mut sim = Sim::new(params, [up + view_w + down, 480.0], seed, 0.5);
    sim.pinches = vec![up, up + view_w];
    // Time each lake took to fill, by its birth time and length.
    let mut lake_lives: Vec<((u64, u64), f64)> = Vec::new();
    let t0 = std::time::Instant::now();
    // Time of each step, to check the work is spread evenly.
    let mut times: Vec<f64> = Vec::with_capacity(steps);
    for k in 0..=steps / every {
        for _ in 0..if k > 0 { every } else { 0 } {
            let t = std::time::Instant::now();
            sim.step();
            times.push(t.elapsed().as_secs_f64() * 1e3);
        }
        let reach = reach_sinuosity(&mut sim);
        println!(
            "step {:6}  t {:8.1}  n {:5}  sinuosity {:5.2}  reach {:4.2}  tight {:4.2}  cutoffs {:4}  oxbows {:3}  holdings {:3} ({:4} reorganised)  width {}",
            sim.steps,
            sim.time,
            sim.centerline.len(),
            sim.sinuosity(),
            reach,
            tight_fraction(&sim),
            sim.cutoffs,
            sim.oxbows.len(),
            sim.land.count(),
            sim.land.reorganisations,
            {
                let [lo, mid, hi, gentle, sharp] = width_stats(&sim);
                format!("{lo:.2}/{mid:.2}/{hi:.2} gentle {gentle:.2} sharp {sharp:.2}")
            }
        );
        let retreat = |side| {
            (0..(sim.bounds[0] / sim.params.width) as usize)
                .map(|x| sim.walls.retreat(side, x as f64))
                .fold(0.0f64, f64::max)
        };
        println!(
            "        supply {:4.2}  jams {:2}  captures {:3}  mills {:2} working of {:2}  scrolls {:3}  wall retreat {:4.2}/{:4.2}  slumps {:3}  tributaries {} ({} into lakes)  tracks {} fords {} (versions {} {})",
            sim.supply(),
            sim.jams.len(),
            sim.captures,
            sim.mills.iter().filter(|m| m.working()).count(),
            sim.mills.len(),
            sim.scrolls.len(),
            retreat(0),
            retreat(1),
            sim.wall_version - 1,
            sim.tributaries.len(),
            sim.tributaries.iter().filter(|t| t.lake).count(),
            sim.tracks.tracks.len(),
            sim.tracks.fords.len(),
            sim.tracks.version,
            sim.tracks.ford_version,
        );
        std::fs::write(
            format!("{out}/step{:06}.svg", sim.steps),
            svg(&sim, up, view_w),
        )
        .unwrap();
        for o in sim.oxbows.iter().filter(|o| !o.is_lake()) {
            let key = (o.born.to_bits(), o.length.to_bits());
            if !lake_lives.iter().any(|l| l.0 == key) {
                lake_lives.push((key, o.filled_at - o.born));
            }
        }
    }
    let mut lives: Vec<f64> = lake_lives.iter().map(|l| l.1).collect();
    lives.sort_by(f64::total_cmp);
    if !lives.is_empty() {
        let q = |f: f64| lives[((lives.len() - 1) as f64 * f) as usize];
        println!(
            "{} lakes filled; time to fill: quartiles {:.0} {:.0} {:.0}",
            lives.len(),
            q(0.25),
            q(0.5),
            q(0.75)
        );
    }
    println!(
        "{:.3} ms/step",
        t0.elapsed().as_secs_f64() * 1e3 / steps as f64
    );
    times.sort_by(f64::total_cmp);
    if !times.is_empty() {
        let q = |f: f64| times[((times.len() - 1) as f64 * f) as usize];
        println!(
            "step time: median {:.3}  99% {:.3}  99.9% {:.3}  max {:.3} ms",
            q(0.5),
            q(0.99),
            q(0.999),
            q(1.0)
        );
    }
}
