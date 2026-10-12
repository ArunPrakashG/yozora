//! Frame pacing for one animation, logged when it settles.

use std::time::{Duration, Instant};

#[derive(Default)]
pub struct FrameStats {
    last: Option<Instant>,
    intervals: Vec<Duration>,
}

impl FrameStats {
    /// Records a frame and returns the time since the previous one (zero for the first).
    pub fn frame(&mut self, now: Instant) -> Duration {
        let dt = self.last.map_or(Duration::ZERO, |last| now - last);
        if self.last.is_some() {
            self.intervals.push(dt);
        }
        self.last = Some(now);
        dt
    }

    /// Logs a summary against the display's refresh interval and resets.
    pub fn finish(&mut self, label: &str, refresh: Option<Duration>) {
        let mut intervals = std::mem::take(&mut self.intervals);
        self.last = None;
        if intervals.is_empty() {
            return;
        }
        intervals.sort();
        let n = intervals.len();
        let total: Duration = intervals.iter().sum();
        let pct = |p: f64| intervals[((n as f64 - 1.0) * p).round() as usize];
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        // A frame counts as late when it arrives more than half a refresh after it was due.
        let late = refresh.map(|r| intervals.iter().filter(|&&d| d > r.mul_f64(1.5)).count());
        tracing::info!(
            target: "yozora::frames",
            "{label}: {n} frames over {:.1} ms, mean {:.2} ms, p50 {:.2} ms, p99 {:.2} ms, max {:.2} ms, \
             refresh {}, late {}",
            ms(total),
            ms(total) / n as f64,
            ms(pct(0.5)),
            ms(pct(0.99)),
            ms(intervals[n - 1]),
            refresh.map_or("unknown".into(), |r| format!("{:.2} ms", ms(r))),
            late.map_or("unknown".into(), |l| l.to_string()),
        );
    }
}
