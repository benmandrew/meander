pub type P = [f64; 2];

// sqrt of the sum of squares rather than `hypot`, which is a slow software
// routine in WASM.
pub fn dist(a: P, b: P) -> f64 {
    dist2(a, b).sqrt()
}

pub fn dist2(a: P, b: P) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    dx * dx + dy * dy
}

/// Squared distance from `p` to the segment `ab`.
pub fn seg_dist2(p: P, a: P, b: P) -> f64 {
    let (ab, ap) = ([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
    let len2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if len2 > 0.0 { ((ap[0] * ab[0] + ap[1] * ab[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
    dist2(p, lerp(a, b, t))
}

pub fn lerp(a: P, b: P, t: f64) -> P {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

pub fn length(pts: &[P]) -> f64 {
    pts.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// Resample a polyline to uniform arc-length spacing close to `ds`. Both
/// endpoints are kept exactly; the spacing is adjusted so the length divides
/// evenly.
pub fn resample(pts: &[P], ds: f64) -> Vec<P> {
    let mut out = Vec::new();
    resample_into(pts, ds, &mut out);
    out
}

/// [`resample`] into `out`, reusing its allocation.
pub fn resample_into(pts: &[P], ds: f64, out: &mut Vec<P>) {
    resample_core(pts, ds, out, |_, _| {});
}

/// [`resample_into`], also interpolating `vals`, one per point, into
/// `out_vals`.
pub fn resample_with(pts: &[P], vals: &[f64], ds: f64, out: &mut Vec<P>, out_vals: &mut Vec<f64>) {
    out_vals.clear();
    resample_core(pts, ds, out, |seg, t| {
        let v = if seg + 1 < vals.len() { vals[seg] + (vals[seg + 1] - vals[seg]) * t } else { vals[seg] };
        out_vals.push(v);
    });
}

/// Resample, calling `at` with the segment and fraction along it of each
/// output point.
fn resample_core(pts: &[P], ds: f64, out: &mut Vec<P>, mut at: impl FnMut(usize, f64)) {
    out.clear();
    let n = pts.len();
    if n < 2 {
        out.extend_from_slice(pts);
        (0..n).for_each(|i| at(i, 0.0));
        return;
    }
    let total = length(pts);
    let m = ((total / ds).round() as usize).max(1);
    let step = total / m as f64;

    out.reserve(m + 1);
    out.push(pts[0]);
    at(0, 0.0);
    let mut seg = 0;
    let mut seg_start = 0.0;
    let mut seg_len = dist(pts[0], pts[1]);
    for k in 1..m {
        let target = k as f64 * step;
        while seg_start + seg_len < target && seg + 2 < n {
            seg_start += seg_len;
            seg += 1;
            seg_len = dist(pts[seg], pts[seg + 1]);
        }
        let t = if seg_len > 0.0 {
            ((target - seg_start) / seg_len).clamp(0.0, 1.0)
        } else {
            0.0
        };
        out.push(lerp(pts[seg], pts[seg + 1], t));
        at(seg, t);
    }
    out.push(pts[n - 1]);
    at(n - 2, 1.0);
}

/// Signed Menger curvature at each point (positive = left turn). Endpoints
/// get zero.
pub fn curvature(pts: &[P], out: &mut Vec<f64>) {
    let n = pts.len();
    out.clear();
    out.resize(n, 0.0);
    for i in 1..n.saturating_sub(1) {
        let (p0, p1, p2) = (pts[i - 1], pts[i], pts[i + 1]);
        let a = [p1[0] - p0[0], p1[1] - p0[1]];
        let b = [p2[0] - p1[0], p2[1] - p1[1]];
        let cross = a[0] * b[1] - a[1] * b[0];
        let denom = dist(p0, p1) * dist(p1, p2) * dist(p0, p2);
        out[i] = if denom > 0.0 { 2.0 * cross / denom } else { 0.0 };
    }
}

/// Unit right-hand normal at each interior point, from the central
/// difference tangent.
pub fn right_normal(pts: &[P], i: usize) -> P {
    let t = [pts[i + 1][0] - pts[i - 1][0], pts[i + 1][1] - pts[i - 1][1]];
    let len = (t[0] * t[0] + t[1] * t[1]).sqrt();
    if len > 0.0 {
        [t[1] / len, -t[0] / len]
    } else {
        [0.0, 0.0]
    }
}

/// Whether any two non-adjacent segments of the polyline cross. O(n²); for
/// tests and diagnostics only.
pub fn self_intersects(pts: &[P]) -> bool {
    fn orient(a: P, b: P, c: P) -> f64 {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    }
    let n = pts.len();
    for i in 0..n.saturating_sub(1) {
        for j in i + 2..n - 1 {
            let (a, b, c, d) = (pts[i], pts[i + 1], pts[j], pts[j + 1]);
            if orient(a, b, c) * orient(a, b, d) < 0.0 && orient(c, d, a) * orient(c, d, b) < 0.0 {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_keeps_endpoints_and_spacing() {
        let pts = vec![[0.0, 0.0], [3.0, 0.0], [3.0, 4.0]];
        let r = resample(&pts, 0.5);
        assert_eq!(r[0], [0.0, 0.0]);
        assert_eq!(*r.last().unwrap(), [3.0, 4.0]);
        assert_eq!(r.len(), 15);
        // Spacing is uniform except across the corner, where the chord is shorter.
        for w in r.windows(2) {
            assert!(dist(w[0], w[1]) <= 0.5 + 1e-9);
        }
    }

    #[test]
    fn curvature_of_circle() {
        let r = 5.0;
        let pts: Vec<P> = (0..100)
            .map(|k| {
                let t = k as f64 * 0.05;
                [r * t.cos(), r * t.sin()]
            })
            .collect();
        let mut c = Vec::new();
        curvature(&pts, &mut c);
        for &k in &c[1..99] {
            assert!((k - 1.0 / r).abs() < 1e-6, "counter-clockwise turn is positive");
        }
    }
}
