//! Frame pacing for one animation, logged when it settles.

use std::time::{Duration, Instant};

#[derive(Default)]
pub struct FrameStats {
    last: Option<Instant>,
    intervals: Vec<Duration>,
}

/// Frame intervals of one or more animations, in milliseconds.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub frames: usize,
    pub total_ms: f64,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    pub refresh_ms: Option<f64>,
    /// Frames that arrived more than half a refresh after they were due.
    pub late: Option<usize>,
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} frames over {:.1} ms, mean {:.2} ms, p50 {:.2} ms, p99 {:.2} ms, max {:.2} ms, refresh {}, late {}",
            self.frames,
            self.total_ms,
            self.mean_ms,
            self.p50_ms,
            self.p99_ms,
            self.max_ms,
            self.refresh_ms.map_or("unknown".into(), |r| format!("{r:.2} ms")),
            self.late.map_or("unknown".into(), |l| l.to_string()),
        )
    }
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

    /// Ends the current animation without discarding its intervals, so several animations can
    /// be summarized together. The gap until the next one isn't counted.
    pub fn pause(&mut self) {
        self.last = None;
    }

    /// Moves `other`'s intervals into this one, to summarize several animations together.
    pub fn absorb(&mut self, other: &mut FrameStats) {
        self.intervals.append(&mut other.intervals);
        other.last = None;
    }

    pub fn summary(&self, refresh: Option<Duration>) -> Option<Summary> {
        if self.intervals.is_empty() {
            return None;
        }
        let mut intervals = self.intervals.clone();
        intervals.sort();
        let n = intervals.len();
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let total: f64 = intervals.iter().map(|d| ms(*d)).sum();
        let pct = |p: f64| ms(intervals[((n as f64 - 1.0) * p).round() as usize]);
        Some(Summary {
            frames: n,
            total_ms: total,
            mean_ms: total / n as f64,
            p50_ms: pct(0.5),
            p99_ms: pct(0.99),
            max_ms: ms(intervals[n - 1]),
            refresh_ms: refresh.map(ms),
            late: refresh.map(|r| intervals.iter().filter(|&&d| d > r.mul_f64(1.5)).count()),
        })
    }

    /// Logs a summary against the display's refresh interval, resets, and returns it.
    pub fn finish(&mut self, label: &str, refresh: Option<Duration>) -> Option<Summary> {
        let summary = self.summary(refresh);
        self.intervals.clear();
        self.last = None;
        if let Some(s) = &summary {
            tracing::info!(target: "yozora::frames", "{label}: {s}");
        }
        summary
    }
}
