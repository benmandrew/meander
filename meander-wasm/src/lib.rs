//! wasm-bindgen wrapper around `meander-core`.
//!
//! Geometry is exposed as pointers into WASM memory so JS can read it
//! without copying. Call `sync` after stepping, then rebuild every typed
//! array view: memory growth detaches old views.

use meander_core::mesh::{VERTEX_FLOATS, channel_strip};
use meander_core::{Params, Sim};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct River {
    sim: Sim,
    /// Channel triangle strip; see `mesh::VERTEX_FLOATS` for the layout.
    channel: Vec<f32>,
    /// All oxbow strips, back to back.
    oxbows: Vec<f32>,
    /// Start of each oxbow strip in `oxbows`, in vertices, plus a final end
    /// marker.
    oxbow_offsets: Vec<u32>,
    /// Age of each oxbow as a fraction of its lifetime.
    oxbow_ages: Vec<f32>,
    /// Floodplain age per cell, scaled to 0..=254 over the age span; 255
    /// means never occupied.
    ages: Vec<u8>,
    /// Erodibility multiplier per cell, scaled so 255 is double the base.
    erod: Vec<u8>,
    curv: Vec<f64>,
    /// Centreline as flat x, y pairs.
    line: Vec<f32>,
    /// All oxbow polylines, back to back, as flat x, y pairs.
    oxbow_line: Vec<f32>,
    /// Start of each oxbow in `oxbow_line`, in points, plus an end marker.
    oxbow_line_offsets: Vec<u32>,
}

#[wasm_bindgen]
impl River {
    #[wasm_bindgen(constructor)]
    pub fn new(width: f64, height: f64, seed: u32, noise: f64) -> River {
        River {
            sim: Sim::new(Params::default(), [width, height], seed as u64, noise),
            channel: Vec::new(),
            oxbows: Vec::new(),
            oxbow_offsets: Vec::new(),
            oxbow_ages: Vec::new(),
            ages: Vec::new(),
            erod: Vec::new(),
            curv: Vec::new(),
            line: Vec::new(),
            oxbow_line: Vec::new(),
            oxbow_line_offsets: Vec::new(),
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

    /// Rebuild the meshes and the age texture. Oxbows narrow to
    /// `1 - oxbow_shrink` of the channel width over their lifetime; the age
    /// texture saturates at `age_span` time units.
    pub fn sync(&mut self, oxbow_shrink: f64, age_span: f64) {
        let sim = &self.sim;
        let w = sim.params.width;
        self.channel.clear();
        channel_strip(&sim.centerline, w, &mut self.curv, &mut self.channel);

        self.oxbows.clear();
        self.oxbow_offsets.clear();
        self.oxbow_ages.clear();
        let life = sim.params.oxbow_lifetime.max(f64::MIN_POSITIVE);
        for o in &sim.oxbows {
            let age = ((sim.time - o.born) / life).clamp(0.0, 1.0);
            self.oxbow_offsets.push((self.oxbows.len() / VERTEX_FLOATS) as u32);
            channel_strip(&o.points, w * (1.0 - oxbow_shrink * age), &mut self.curv, &mut self.oxbows);
            self.oxbow_ages.push(age as f32);
        }
        self.oxbow_offsets.push((self.oxbows.len() / VERTEX_FLOATS) as u32);

        let now = sim.time as f32;
        let scale = 254.0 / age_span.max(f64::MIN_POSITIVE) as f32;
        self.ages.clear();
        self.ages.extend(sim.age_map.last.iter().map(|&t| {
            if t == f32::NEG_INFINITY { 255 } else { ((now - t) * scale).min(254.0) as u8 }
        }));

        let e = &sim.erodibility;
        let (hetero, clay) = (sim.params.heterogeneity, sim.params.clay_factor);
        self.erod.clear();
        for y in 0..e.rows {
            for x in 0..e.cols {
                let p = [(x as f64 + 0.5) * e.cell, (y as f64 + 0.5) * e.cell];
                self.erod.push((e.at(p, hetero, clay) * 127.5).min(255.0) as u8);
            }
        }
    }

    /// Copy the centreline and oxbow polylines into flat buffers, and the
    /// oxbow ages as fractions of their lifetime.
    pub fn sync_lines(&mut self) {
        let sim = &self.sim;
        self.line.clear();
        self.line.extend(sim.centerline.iter().flat_map(|p| [p[0] as f32, p[1] as f32]));
        self.oxbow_line.clear();
        self.oxbow_line_offsets.clear();
        self.oxbow_ages.clear();
        let life = sim.params.oxbow_lifetime.max(f64::MIN_POSITIVE);
        for o in &sim.oxbows {
            self.oxbow_line_offsets.push((self.oxbow_line.len() / 2) as u32);
            self.oxbow_line.extend(o.points.iter().flat_map(|p| [p[0] as f32, p[1] as f32]));
            self.oxbow_ages.push(((sim.time - o.born) / life).clamp(0.0, 1.0) as f32);
        }
        self.oxbow_line_offsets.push((self.oxbow_line.len() / 2) as u32);
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
    pub fn oxbow_line_offsets_ptr(&self) -> *const u32 {
        self.oxbow_line_offsets.as_ptr()
    }

    pub fn channel_ptr(&self) -> *const f32 {
        self.channel.as_ptr()
    }
    /// Number of vertices in the channel strip.
    pub fn channel_vertices(&self) -> usize {
        self.channel.len() / VERTEX_FLOATS
    }
    pub fn oxbows_ptr(&self) -> *const f32 {
        self.oxbows.as_ptr()
    }
    pub fn oxbow_vertices(&self) -> usize {
        self.oxbows.len() / VERTEX_FLOATS
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
    pub fn ages_ptr(&self) -> *const u8 {
        self.ages.as_ptr()
    }
    pub fn age_cols(&self) -> usize {
        self.sim.age_map.cols
    }
    pub fn age_rows(&self) -> usize {
        self.sim.age_map.rows
    }
    pub fn erod_ptr(&self) -> *const u8 {
        self.erod.as_ptr()
    }
    pub fn erod_cols(&self) -> usize {
        self.sim.erodibility.cols
    }
    pub fn erod_rows(&self) -> usize {
        self.sim.erodibility.rows
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
