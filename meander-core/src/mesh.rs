//! Triangle-strip channel meshes and the floodplain age map.

use crate::geom::{P, curvature, dist};

/// Floats per mesh vertex: x, y, u (arc length in widths), v (−1 at the left
/// bank, +1 at the right), and curvature times width.
pub const VERTEX_FLOATS: usize = 5;

/// Append a triangle strip for a channel of the given width along `pts`,
/// two vertices per point (left bank, then right bank).
///
/// The inner-bank offset is clamped to 0.9 of the radius of curvature so
/// the inner bank does not fold over itself in tight bends.
pub fn channel_strip(pts: &[P], width: f64, curv: &mut Vec<f64>, out: &mut Vec<f32>) {
    let n = pts.len();
    if n < 2 {
        return;
    }
    curvature(pts, curv);
    // Light smoothing: Menger curvature is noisy at the point spacing, and
    // it drives the shading.
    let raw = curv.clone();
    for i in 1..n - 1 {
        curv[i] = 0.25 * raw[i - 1] + 0.5 * raw[i] + 0.25 * raw[i + 1];
    }
    let half = 0.5 * width;
    let mut u = 0.0;
    for i in 0..n {
        if i > 0 {
            u += dist(pts[i - 1], pts[i]) / width;
        }
        let (a, b) = (pts[i.saturating_sub(1)], pts[(i + 1).min(n - 1)]);
        let t = [b[0] - a[0], b[1] - a[1]];
        let len = t[0].hypot(t[1]).max(f64::MIN_POSITIVE);
        let nr = [t[1] / len, -t[0] / len];
        let c = curv[i];
        let inner = if c != 0.0 { half.min(0.9 / c.abs()) } else { half };
        // A left turn (c > 0) has its centre of curvature on the left.
        let (left, right) = if c > 0.0 { (inner, half) } else { (half, inner) };
        let p = pts[i];
        let cw = (c * width) as f32;
        out.extend_from_slice(&[
            (p[0] - nr[0] * left) as f32,
            (p[1] - nr[1] * left) as f32,
            u as f32,
            -1.0,
            cw,
            (p[0] + nr[0] * right) as f32,
            (p[1] + nr[1] * right) as f32,
            u as f32,
            1.0,
            cw,
        ]);
    }
}

/// Raster of the time each cell was last covered by the channel.
pub struct AgeMap {
    pub cell: f64,
    pub cols: usize,
    pub rows: usize,
    /// Last occupation time per cell, row-major from y = 0; −∞ if never.
    pub last: Vec<f32>,
}

impl AgeMap {
    pub fn new(bounds: P, cell: f64) -> Self {
        let cols = (bounds[0] / cell).ceil().max(1.0) as usize;
        let rows = (bounds[1] / cell).ceil().max(1.0) as usize;
        Self { cell, cols, rows, last: vec![f32::NEG_INFINITY; cols * rows] }
    }

    /// Mark every cell whose centre lies within `radius` of a point.
    pub fn stamp(&mut self, pts: &[P], radius: f64, time: f64) {
        let c = self.cell;
        let r2 = radius * radius;
        let span = |lo: f64, hi: f64, max: usize| {
            let a = ((lo / c - 0.5).ceil().max(0.0)) as usize;
            let b = ((hi / c - 0.5).floor().min(max as f64 - 1.0)).max(-1.0) as isize;
            (a, b)
        };
        for p in pts {
            let (x0, x1) = span(p[0] - radius, p[0] + radius, self.cols);
            let (y0, y1) = span(p[1] - radius, p[1] + radius, self.rows);
            for y in y0 as isize..=y1 {
                let dy = (y as f64 + 0.5) * c - p[1];
                for x in x0 as isize..=x1 {
                    let dx = (x as f64 + 0.5) * c - p[0];
                    if dx * dx + dy * dy <= r2 {
                        self.last[y as usize * self.cols + x as usize] = time as f32;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_on_straight_line() {
        let pts = vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]];
        let mut out = Vec::new();
        channel_strip(&pts, 2.0, &mut Vec::new(), &mut out);
        assert_eq!(out.len(), 3 * 2 * VERTEX_FLOATS);
        // Flow along +x: left bank is +y, right bank is −y.
        assert_eq!(&out[..5], &[0.0, 1.0, 0.0, -1.0, 0.0]);
        assert_eq!(&out[5..10], &[0.0, -1.0, 0.0, 1.0, 0.0]);
        assert_eq!(out[2 * VERTEX_FLOATS * 2 + 2], 1.0, "u is arc length in widths");
    }

    #[test]
    fn stamp_marks_disc() {
        let mut m = AgeMap::new([10.0, 10.0], 1.0);
        m.stamp(&[[5.0, 5.0]], 1.0, 3.0);
        let marked = m.last.iter().filter(|&&t| t == 3.0).count();
        assert_eq!(marked, 4, "the four cells with centres at distance √0.5");
        assert_eq!(m.last[0], f32::NEG_INFINITY);
    }
}
