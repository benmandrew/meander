//! Howard–Knutson weights from the Ikeda–Parker–Sawai (1981) bend theory.
//!
//! IPS model the excess near-bank velocity `u_b` with a linearised
//! shallow-water equation along the channel. In the form given by Sun et al.
//! (1996), with half-width `B`, depth `H`, mean velocity `U`, friction
//! coefficient `C_f`, Froude number `F`, scour factor `A` and centreline
//! curvature `C`:
//!
//! ```text
//! du_b/ds + λ u_b = −B U dC/ds + B C_f U (F² + A + 1) C / H,    λ = 2 C_f / H
//! ```
//!
//! For `du/ds + λu = a C + b dC/ds`, integrating the `dC/ds` term by parts
//! gives `u = b C(s) + (a − bλ) ∫₀^∞ e^{−λξ} C(s − ξ) dξ`. Writing the
//! integral as `⟨C⟩/λ`, with `⟨C⟩` the exponentially weighted upstream mean,
//! and measuring curvature in inverse channel widths (`w = 2B`):
//!
//! ```text
//! u_b = (U/2) · [ −Cw + (F² + A + 3)/2 · ⟨Cw⟩ ]
//! ```
//!
//! This is the Howard–Knutson rate with `Ω = −1`, `Γ = (F² + A + 3)/2`, and
//! upstream decay `α = λw = 2 C_f (w/H)` per channel width. The factor `U/2`
//! and the IPS erosion coefficient fold into the simulation's erodibility,
//! since simulated time has no physical unit.

/// Howard–Knutson `(Ω, Γ, α)` for friction coefficient `friction`, width to
/// depth ratio `aspect`, Froude number `froude` and scour factor `scour`.
pub fn coefficients(friction: f64, aspect: f64, froude: f64, scour: f64) -> (f64, f64, f64) {
    let omega = -1.0;
    let gamma = 0.5 * (froude * froude + scour + 3.0);
    let alpha = 2.0 * friction * aspect;
    (omega, gamma, alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Solve `du/ds + λu = aC + b dC/ds` by forward Euler for a sine
    /// curvature and compare with the closed form at the end of the run.
    #[test]
    fn closed_form_matches_the_ode() {
        let (cf, aspect, froude, scour) = (0.008, 25.0, 0.4, 2.89);
        let (omega, gamma, alpha) = coefficients(cf, aspect, froude, scour);
        // Units: lengths in channel widths, B = 1/2, H = 1/aspect, U = 1.
        let (b_half, h) = (0.5, 1.0 / aspect);
        let lambda = 2.0 * cf / h;
        assert!((lambda - alpha).abs() < 1e-12);
        let a = b_half * cf * (froude * froude + scour + 1.0) / h;
        let b = -b_half;
        let k = std::f64::consts::TAU / 12.0;
        let curv = |s: f64| 0.3 * (k * s).sin();
        let dcurv = |s: f64| 0.3 * k * (k * s).cos();

        let ds = 1e-3;
        let (mut u, mut s) = (0.0, 0.0);
        // Long enough for the start-up transient, e^{−λs}, to vanish.
        while s < 40.0 / lambda {
            u += ds * (a * curv(s) + b * dcurv(s) - lambda * u);
            s += ds;
        }
        // ⟨C⟩ = λ ∫ e^{−λξ} C(s − ξ) dξ, by quadrature.
        let (mut mean, mut xi) = (0.0, 0.0);
        while xi < 40.0 / lambda {
            mean += lambda * (-lambda * xi).exp() * curv(s - xi) * ds;
            xi += ds;
        }
        // u_b = (U/2)(Ω Cw + Γ ⟨Cw⟩), with w = 1 and U = 1.
        let closed = 0.5 * (omega * curv(s) + gamma * mean);
        assert!((u - closed).abs() < 1e-3 * closed.abs().max(0.1), "ode {u}, closed form {closed}");
    }
}
