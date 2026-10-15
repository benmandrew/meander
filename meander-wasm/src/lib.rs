//! wasm-bindgen wrapper around `meander-core`.
//!
//! Geometry is exposed as pointers into WASM memory so JS can read it
//! without copying. Call `sync` after stepping, then rebuild every typed
//! array view: memory growth detaches old views.

use meander_core::{Params, Sim};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct River {
    sim: Sim,
    line: Vec<f32>,
    oxbow_points: Vec<f32>,
    /// Start of each oxbow in `oxbow_points`, in points, plus a final end
    /// marker.
    oxbow_offsets: Vec<u32>,
    /// Age of each oxbow as a fraction of its lifetime.
    oxbow_ages: Vec<f32>,
}

#[wasm_bindgen]
impl River {
    #[wasm_bindgen(constructor)]
    pub fn new(width: f64, height: f64, seed: u32, noise: f64) -> River {
        River {
            sim: Sim::new(Params::default(), [width, height], seed as u64, noise),
            line: Vec::new(),
            oxbow_points: Vec::new(),
            oxbow_offsets: Vec::new(),
            oxbow_ages: Vec::new(),
        }
    }

    /// Restart from a noisy straight line, optionally in a new domain.
    pub fn reset(&mut self, width: f64, height: f64, seed: u32, noise: f64) {
        self.sim.bounds = [width, height];
        self.sim.reset(seed as u64, noise);
    }

    pub fn step(&mut self, n: u32) {
        self.sim.step_n(n as usize);
    }

    /// Returns false for an unknown parameter name.
    pub fn set_param(&mut self, name: &str, value: f64) -> bool {
        self.sim.params.set(name, value)
    }

    /// Returns NaN for an unknown parameter name.
    pub fn get_param(&self, name: &str) -> f64 {
        self.sim.params.get(name).unwrap_or(f64::NAN)
    }

    /// Copy the current geometry into the flat f32 buffers.
    pub fn sync(&mut self) {
        let sim = &self.sim;
        self.line.clear();
        self.line.extend(sim.centerline.iter().flat_map(|p| [p[0] as f32, p[1] as f32]));
        self.oxbow_points.clear();
        self.oxbow_offsets.clear();
        self.oxbow_ages.clear();
        let life = sim.params.oxbow_lifetime.max(f64::MIN_POSITIVE);
        for o in &sim.oxbows {
            self.oxbow_offsets.push((self.oxbow_points.len() / 2) as u32);
            self.oxbow_points.extend(o.points.iter().flat_map(|p| [p[0] as f32, p[1] as f32]));
            self.oxbow_ages.push(((sim.time - o.born) / life).clamp(0.0, 1.0) as f32);
        }
        self.oxbow_offsets.push((self.oxbow_points.len() / 2) as u32);
    }

    pub fn line_ptr(&self) -> *const f32 {
        self.line.as_ptr()
    }
    /// Number of f32 values (two per point).
    pub fn line_len(&self) -> usize {
        self.line.len()
    }
    pub fn oxbow_points_ptr(&self) -> *const f32 {
        self.oxbow_points.as_ptr()
    }
    pub fn oxbow_points_len(&self) -> usize {
        self.oxbow_points.len()
    }
    pub fn oxbow_offsets_ptr(&self) -> *const u32 {
        self.oxbow_offsets.as_ptr()
    }
    pub fn oxbow_count(&self) -> usize {
        self.sim.oxbows.len()
    }
    pub fn oxbow_ages_ptr(&self) -> *const f32 {
        self.oxbow_ages.as_ptr()
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
    pub fn point_count(&self) -> usize {
        self.sim.centerline.len()
    }
}
