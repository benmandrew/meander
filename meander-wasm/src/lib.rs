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
    /// Fraction of each oxbow lake filled, in [0, 1].
    oxbow_fills: Vec<f32>,
    /// Time since each oxbow filled and became a scar, or 0 for a lake.
    oxbow_scar_ages: Vec<f32>,
    /// Arc length of each oxbow when it was cut off.
    oxbow_lengths: Vec<f32>,
    /// Arc length along each oxbow as cut off to the start of its piece.
    oxbow_offsets_along: Vec<f32>,
    /// Land height at each oxbow point, in [0, 1], matching `oxbow_line`.
    oxbow_heights: Vec<f32>,
    /// Fraction of the lake depth filled with flood silt at each oxbow
    /// point, in [0, 1], matching `oxbow_line`.
    oxbow_silts: Vec<f32>,
    /// `Sim::oxbow_version` the oxbow polylines, heights, lengths and
    /// offsets were last copied for.
    oxbows_synced: Option<u64>,
    /// All scroll bar polylines, back to back, as flat x, y pairs.
    scroll_line: Vec<f32>,
    /// Start of each scroll bar in `scroll_line`, in points, plus an end
    /// marker.
    scroll_offsets: Vec<u32>,
    /// Age of each scroll bar in simulation time.
    scroll_ages: Vec<f32>,
    /// Each crevasse splay as apex x, y, direction x, y, length and birth
    /// time, then for each ray its direction x, y and the inner and outer
    /// distances from the apex between which sand remains.
    splay_data: Vec<f32>,
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
            oxbow_fills: Vec::new(),
            oxbow_scar_ages: Vec::new(),
            oxbow_lengths: Vec::new(),
            oxbow_offsets_along: Vec::new(),
            oxbow_heights: Vec::new(),
            oxbow_silts: Vec::new(),
            oxbows_synced: None,
            scroll_line: Vec::new(),
            scroll_offsets: Vec::new(),
            scroll_ages: Vec::new(),
            splay_data: Vec::new(),
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

    /// Copy the centreline into a flat buffer.
    pub fn sync_line(&mut self) {
        self.line.clear();
        self.line.extend(self.sim.centerline.iter().flat_map(|p| [p[0] as f32, p[1] as f32]));
    }

    /// Copy the centreline and oxbow polylines into flat buffers, and the
    /// oxbow fills, scar ages, birth lengths, offsets along the original
    /// oxbow, and the land height and silt at each oxbow point. What depends
    /// only on the oxbows' shape is copied only when it changes; see
    /// `oxbow_version`.
    pub fn sync(&mut self) {
        self.sync_line();
        let sim = &self.sim;
        if self.oxbows_synced != Some(sim.oxbow_version) {
            self.oxbows_synced = Some(sim.oxbow_version);
            self.oxbow_line.clear();
            self.oxbow_offsets.clear();
            self.oxbow_lengths.clear();
            self.oxbow_offsets_along.clear();
            self.oxbow_heights.clear();
            for o in &sim.oxbows {
                self.oxbow_offsets.push((self.oxbow_line.len() / 2) as u32);
                self.oxbow_line.extend(o.points.iter().flat_map(|p| [p[0] as f32, p[1] as f32]));
                self.oxbow_heights.extend(o.points.iter().map(|&p| sim.erodibility.height(p) as f32));
                self.oxbow_lengths.push(o.length as f32);
                self.oxbow_offsets_along.push(o.offset as f32);
            }
            self.oxbow_offsets.push((self.oxbow_line.len() / 2) as u32);
        }
        self.oxbow_fills.clear();
        self.oxbow_scar_ages.clear();
        self.oxbow_silts.clear();
        for o in &sim.oxbows {
            self.oxbow_silts.extend(o.points.iter().map(|&p| sim.erodibility.silt_at(p) as f32));
            self.oxbow_fills.push(o.fill as f32);
            self.oxbow_scar_ages.push(if o.is_lake() { 0.0 } else { (sim.time - o.filled_at) as f32 });
        }
    }

    /// Copy the crevasse splays into a flat buffer; see `splay_stride`.
    pub fn sync_splays(&mut self) {
        self.splay_data.clear();
        for s in &self.sim.splays {
            self.splay_data.extend([s.apex[0], s.apex[1], s.dir[0], s.dir[1], s.length, s.born].map(|v| v as f32));
            for (v, [a, b]) in &s.rays {
                self.splay_data.extend([v[0], v[1], *a, *b].map(|x| x as f32));
            }
        }
    }

    pub fn splay_data_ptr(&self) -> *const f32 {
        self.splay_data.as_ptr()
    }
    pub fn splay_count(&self) -> usize {
        self.sim.splays.len()
    }
    /// Floats per splay in the splay buffer.
    pub fn splay_stride(&self) -> usize {
        6 + 4 * meander_core::SPLAY_RAYS
    }
    /// Changes whenever a splay forms, erodes or is dropped.
    pub fn splay_version(&self) -> f64 {
        self.sim.splay_version as f64
    }
    /// Crevasse splays so far, including those since dropped.
    pub fn crevasses(&self) -> u32 {
        self.sim.crevasses as u32
    }

    /// Copy the scroll bars into flat buffers. They change only at floods
    /// and when the channel crosses them; see `scroll_version`.
    pub fn sync_scrolls(&mut self) {
        let sim = &self.sim;
        self.scroll_line.clear();
        self.scroll_offsets.clear();
        self.scroll_ages.clear();
        for s in &sim.scrolls {
            self.scroll_offsets.push((self.scroll_line.len() / 2) as u32);
            self.scroll_line.extend(s.points.iter().flat_map(|p| [p[0] as f32, p[1] as f32]));
            self.scroll_ages.push((sim.time - s.born) as f32);
        }
        self.scroll_offsets.push((self.scroll_line.len() / 2) as u32);
    }

    pub fn scroll_line_ptr(&self) -> *const f32 {
        self.scroll_line.as_ptr()
    }
    pub fn scroll_line_len(&self) -> usize {
        self.scroll_line.len()
    }
    pub fn scroll_offsets_ptr(&self) -> *const u32 {
        self.scroll_offsets.as_ptr()
    }
    pub fn scroll_ages_ptr(&self) -> *const f32 {
        self.scroll_ages.as_ptr()
    }
    pub fn scroll_count(&self) -> usize {
        self.scroll_ages.len()
    }
    /// Changes whenever the scroll bars do.
    pub fn scroll_version(&self) -> f64 {
        self.sim.scroll_version as f64
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
    pub fn oxbow_fills_ptr(&self) -> *const f32 {
        self.oxbow_fills.as_ptr()
    }
    pub fn oxbow_scar_ages_ptr(&self) -> *const f32 {
        self.oxbow_scar_ages.as_ptr()
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
    pub fn oxbow_silts_ptr(&self) -> *const f32 {
        self.oxbow_silts.as_ptr()
    }
    pub fn oxbow_count(&self) -> usize {
        self.sim.oxbows.len()
    }

    /// Overbank deposits on the erodibility grid, row by row, in channel
    /// depths above fresh point-bar sand; `field_cols` × `field_rows` cells
    /// of side `field_cell`. Rebuild the view after every step, like the
    /// others.
    pub fn deposit_ptr(&self) -> *const f32 {
        self.sim.erodibility.deposit.as_ptr()
    }
    pub fn field_cols(&self) -> usize {
        self.sim.erodibility.cols
    }
    pub fn field_rows(&self) -> usize {
        self.sim.erodibility.rows
    }
    pub fn field_cell(&self) -> f64 {
        self.sim.erodibility.cell
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
