//! wasm-bindgen wrapper around `meander-core`.
//!
//! The simulated river is longer than the visible window: hidden reaches
//! upstream and downstream let meanders develop before they come into view
//! and keep the pinned outlet out of sight.
//!
//! Geometry is exposed as pointers into WASM memory so JS can read it
//! without copying. Call `sync` after stepping, then rebuild every typed
//! array view: memory growth detaches old views.

use meander_core::{Params, Sim};
use wasm_bindgen::prelude::*;

/// Hidden reach lengths, in channel widths.
const UPSTREAM: f64 = 60.0;
const DOWNSTREAM: f64 = 20.0;

#[wasm_bindgen]
pub struct River {
    sim: Sim,
    /// Left edge of the visible window in world units.
    view_x: f64,
    /// Centreline as flat x, y pairs.
    line: Vec<f32>,
    /// All oxbow polylines, back to back, as flat x, y pairs.
    oxbow_line: Vec<f32>,
    /// Start of each oxbow in `oxbow_line`, in points, plus an end marker.
    oxbow_offsets: Vec<u32>,
    /// Age of each oxbow in simulation time.
    oxbow_ages: Vec<f32>,
    /// Arc length of each oxbow when it was cut off.
    oxbow_lengths: Vec<f32>,
    /// Arc length along each oxbow as cut off to the start of its piece.
    oxbow_offsets_along: Vec<f32>,
    /// Land height at each oxbow point, in [0, 1], matching `oxbow_line`.
    oxbow_heights: Vec<f32>,
}

#[wasm_bindgen]
impl River {
    /// Parameters take their defaults; call `reset` after setting them.
    #[wasm_bindgen(constructor)]
    pub fn new() -> River {
        River {
            sim: Sim::new(Params::default(), [1.0, 1.0], 0, 0.0),
            view_x: 0.0,
            line: Vec::new(),
            oxbow_line: Vec::new(),
            oxbow_offsets: Vec::new(),
            oxbow_ages: Vec::new(),
            oxbow_lengths: Vec::new(),
            oxbow_offsets_along: Vec::new(),
            oxbow_heights: Vec::new(),
        }
    }

    /// Restart from a noisy straight line, for a visible window of the given
    /// size, and run `warmup` time units so the river opens developed.
    pub fn reset(&mut self, view_w: f64, view_h: f64, seed: u32, noise: f64, warmup: f64) {
        let w = self.sim.params.width;
        self.view_x = UPSTREAM * w;
        self.sim.bounds = [(UPSTREAM + DOWNSTREAM) * w + view_w, view_h];
        // Pinch the valley at the window edges so no bend straddles them.
        self.sim.pinches = vec![self.view_x, self.view_x + view_w];
        self.sim.reset(seed as u64, noise);
        self.advance(warmup);
    }

    /// Run whole steps covering `time` units of simulated time; returns the
    /// number of steps taken.
    pub fn advance(&mut self, time: f64) -> u32 {
        let n = (time / self.sim.params.dt).round().max(0.0) as u32;
        self.sim.step_n(n as usize);
        n
    }

    /// Returns false for an unknown parameter name.
    pub fn set_param(&mut self, name: &str, value: f64) -> bool {
        self.sim.params.set(name, value)
    }

    /// Returns NaN for an unknown parameter name.
    pub fn get_param(&self, name: &str) -> f64 {
        self.sim.params.get(name).unwrap_or(f64::NAN)
    }

    /// Copy the centreline and oxbow polylines into flat buffers, and the
    /// oxbow ages, birth lengths, offsets along the original oxbow and the
    /// land height at each oxbow point.
    pub fn sync(&mut self) {
        let sim = &self.sim;
        self.line.clear();
        self.line.extend(sim.centerline.iter().flat_map(|p| [p[0] as f32, p[1] as f32]));
        self.oxbow_line.clear();
        self.oxbow_offsets.clear();
        self.oxbow_ages.clear();
        self.oxbow_lengths.clear();
        self.oxbow_offsets_along.clear();
        self.oxbow_heights.clear();
        for o in &sim.oxbows {
            self.oxbow_offsets.push((self.oxbow_line.len() / 2) as u32);
            self.oxbow_line.extend(o.points.iter().flat_map(|p| [p[0] as f32, p[1] as f32]));
            self.oxbow_heights.extend(o.points.iter().map(|&p| sim.erodibility.height(p) as f32));
            self.oxbow_ages.push((sim.time - o.born) as f32);
            self.oxbow_lengths.push(o.length as f32);
            self.oxbow_offsets_along.push(o.offset as f32);
        }
        self.oxbow_offsets.push((self.oxbow_line.len() / 2) as u32);
    }

    pub fn line_ptr(&self) -> *const f32 {
        self.line.as_ptr()
    }
    /// Number of f32 values in the centreline (two per point).
    pub fn line_len(&self) -> usize {
        self.line.len()
    }
    pub fn oxbow_line_ptr(&self) -> *const f32 {
        self.oxbow_line.as_ptr()
    }
    pub fn oxbow_line_len(&self) -> usize {
        self.oxbow_line.len()
    }
    pub fn oxbow_offsets_ptr(&self) -> *const u32 {
        self.oxbow_offsets.as_ptr()
    }
    pub fn oxbow_ages_ptr(&self) -> *const f32 {
        self.oxbow_ages.as_ptr()
    }
    pub fn oxbow_lengths_ptr(&self) -> *const f32 {
        self.oxbow_lengths.as_ptr()
    }
    pub fn oxbow_offsets_along_ptr(&self) -> *const f32 {
        self.oxbow_offsets_along.as_ptr()
    }
    pub fn oxbow_heights_ptr(&self) -> *const f32 {
        self.oxbow_heights.as_ptr()
    }
    pub fn oxbow_count(&self) -> usize {
        self.sim.oxbows.len()
    }

    /// Erodibility noise at a point, in [0, 1]; low values erode slowly.
    pub fn soil(&self, x: f64, y: f64) -> f64 {
        self.sim.erodibility.noise_at([x, y])
    }

    pub fn view_x(&self) -> f64 {
        self.view_x
    }
    pub fn time(&self) -> f64 {
        self.sim.time
    }
    pub fn sinuosity(&self) -> f64 {
        self.sim.sinuosity()
    }
    pub fn cutoffs(&self) -> u32 {
        self.sim.cutoffs as u32
    }
    pub fn floods(&self) -> u32 {
        self.sim.floods as u32
    }
    /// Cutoffs made by floods, included in `cutoffs`.
    pub fn flood_cutoffs(&self) -> u32 {
        self.sim.flood_cutoffs as u32
    }
    pub fn point_count(&self) -> usize {
        self.sim.centerline.len()
    }
}

impl Default for River {
    fn default() -> Self {
        Self::new()
    }
}
