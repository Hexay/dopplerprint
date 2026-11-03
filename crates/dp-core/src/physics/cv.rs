//! Closed-form constant-velocity (straight, level, constant-speed) flyover curve.

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CvParams {
    pub f0: f64,
    pub v: f64,
    /// Slant range at CPA (m).
    pub d_cpa: f64,
    /// Arrival time of the CPA sound, in the time base of the caller (audio s or UNIX s).
    pub t_cpa_arrive: f64,
    pub c: f64,
}

/// Received frequency at time `t` for the exact retarded-time constant-velocity model.
pub fn frequency(_p: &CvParams, _t: f64) -> f64 {
    todo!("physics agent: f0*c^2/(c^2-v^2)*[1 - v^2 (t-tc)/sqrt(d^2(c^2-v^2)+c^2 v^2 (t-tc)^2)], tc=emit time")
}
