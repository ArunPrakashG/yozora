//! A damped spring advanced by real frame time.
//!
//! Springs keep their velocity when retargeted, so an animation can be interrupted mid-flight
//! without a visible jump. Once settled they report so, and callers stop requesting frames.

use std::time::Duration;

/// Stiffness and damping for unit mass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringConfig {
    pub stiffness: f32,
    pub damping: f32,
}

impl SpringConfig {
    /// Popovers: about 3.9% overshoot, at rest by about 490 ms.
    pub const POPOVER: Self = Self { stiffness: 380.0, damping: 28.0 };
    /// Workspace indicator, switches, highlights.
    pub const INDICATOR: Self = Self { stiffness: 520.0, damping: 34.0 };
}

#[derive(Clone, Debug)]
pub struct Spring {
    config: SpringConfig,
    value: f32,
    velocity: f32,
    target: f32,
}

/// Largest integration step. Frame gaps are split into steps this size so the result does not
/// depend on the refresh rate.
const MAX_STEP: f32 = 1.0 / 1000.0;
/// Distance and speed below which the spring snaps to its target and stops.
const REST_EPSILON: f32 = 0.001;

impl Spring {
    pub fn new(value: f32, config: SpringConfig) -> Self {
        Self { config, value, velocity: 0.0, target: value }
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub fn is_settled(&self) -> bool {
        self.value == self.target && self.velocity == 0.0
    }

    /// Advances by `dt` and returns whether the spring is now at rest.
    pub fn step(&mut self, dt: Duration) -> bool {
        if self.is_settled() {
            return true;
        }
        // Cap long gaps (a stalled frame, a suspended session) so the spring never explodes.
        let mut remaining = dt.as_secs_f32().min(0.1);
        while remaining > 0.0 {
            let h = remaining.min(MAX_STEP);
            let force = -self.config.stiffness * (self.value - self.target) - self.config.damping * self.velocity;
            self.velocity += force * h;
            self.value += self.velocity * h;
            remaining -= h;
        }
        if (self.value - self.target).abs() < REST_EPSILON && self.velocity.abs() < REST_EPSILON {
            self.value = self.target;
            self.velocity = 0.0;
        }
        self.is_settled()
    }
}

/// Where a spring released at rest from 0 toward 1 is after `t` seconds. Negative `t` is 0.
///
/// The closed form, for animations that are a pure function of elapsed time, like staggered
/// rows that each start a little later.
pub fn step_response(config: SpringConfig, t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    let w0 = config.stiffness.sqrt();
    let zeta = config.damping / (2.0 * w0);
    if zeta < 1.0 {
        let wd = w0 * (1.0 - zeta * zeta).sqrt();
        1.0 - (-zeta * w0 * t).exp() * ((wd * t).cos() + zeta * w0 / wd * (wd * t).sin())
    } else if zeta == 1.0 {
        1.0 - (-w0 * t).exp() * (1.0 + w0 * t)
    } else {
        let s = (zeta * zeta - 1.0).sqrt();
        let (r1, r2) = (-w0 * (zeta - s), -w0 * (zeta + s));
        1.0 - (r2 * (r1 * t).exp() - r1 * (r2 * t).exp()) / (r2 - r1)
    }
}

/// How long until `step_response` stays within 0.1% of 1.
pub fn settle_time(config: SpringConfig) -> std::time::Duration {
    let mut t = 2.0f32;
    while t > 0.0 && (1.0 - step_response(config, t)).abs() < 0.001 {
        t -= 0.002;
    }
    std::time::Duration::from_secs_f32(t + 0.002)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(config: SpringConfig) -> (f32, Duration) {
        let mut spring = Spring::new(0.0, config);
        spring.set_target(1.0);
        let frame = Duration::from_micros(6_944);
        let mut peak = 0.0f32;
        let mut elapsed = Duration::ZERO;
        while !spring.step(frame) {
            peak = peak.max(spring.value());
            elapsed += frame;
            assert!(elapsed < Duration::from_secs(2), "spring never settled");
        }
        (peak, elapsed)
    }

    #[test]
    fn popover_spring_overshoots_slightly_and_settles() {
        let (peak, elapsed) = run(SpringConfig::POPOVER);
        assert!((1.03..1.045).contains(&peak), "peak {peak}");
        assert!(elapsed < Duration::from_millis(700), "settled after {elapsed:?}");
    }

    #[test]
    fn motion_is_independent_of_refresh_rate() {
        let sample = |frame_us: u64| {
            let mut spring = Spring::new(0.0, SpringConfig::POPOVER);
            spring.set_target(1.0);
            let frame = Duration::from_micros(frame_us);
            let mut t = Duration::ZERO;
            while t < Duration::from_millis(120) {
                spring.step(frame);
                t += frame;
            }
            spring.value()
        };
        // Both intervals divide 120 ms exactly: roughly 167 Hz and 67 Hz.
        let fast = sample(6_000);
        let slow = sample(15_000);
        assert!((fast - slow).abs() < 0.01, "{fast} vs {slow}");
    }

    #[test]
    fn retargeting_keeps_velocity() {
        let mut spring = Spring::new(0.0, SpringConfig::POPOVER);
        spring.set_target(1.0);
        spring.step(Duration::from_millis(50));
        let before = spring.value();
        spring.set_target(0.0);
        spring.step(Duration::from_millis(5));
        assert!(spring.value() > before, "momentum should carry it forward briefly");
    }

    #[test]
    fn closed_form_matches_stepping() {
        let mut spring = Spring::new(0.0, SpringConfig::POPOVER);
        spring.set_target(1.0);
        for _ in 0..150 {
            spring.step(std::time::Duration::from_millis(1));
        }
        let closed = step_response(SpringConfig::POPOVER, 0.150);
        assert!((spring.value() - closed).abs() < 0.01, "{} vs {closed}", spring.value());
        let settle = settle_time(SpringConfig::POPOVER).as_millis();
        assert!((350..700).contains(&settle), "settles in {settle} ms");
    }
}
