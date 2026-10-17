//! Counters shared by offline CLI and future live UI. Offline rates measure decoder
//! processing, never VNC reception or sender scheduling.
use serde::Serialize;
use std::time::Instant;

#[derive(Debug, Default, Serialize)]
pub struct Counters {
    pub images: u64,
    pub valid_frames: u64,
    pub unique_blocks: u32,
    pub total_blocks: u32,
    pub duplicates: u64,
    pub crc_failures: u64,
    pub locator_failures: u64,
    pub palette_failures: u64,
    pub invalid_frames: u64,
    pub foreign_frames: u64,
    pub waiting_metadata: u64,
    pub unique_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct MetricsSnapshot<'a> {
    pub counters: &'a Counters,
    pub elapsed_seconds: f64,
    pub unique_blocks_per_second: f64,
    pub goodput_bytes_per_second: f64,
}

pub struct Metrics {
    pub counters: Counters,
    started: Instant,
}
impl Default for Metrics {
    fn default() -> Self {
        Self {
            counters: Counters::default(),
            started: Instant::now(),
        }
    }
}
impl Metrics {
    pub fn snapshot(&self) -> MetricsSnapshot<'_> {
        let elapsed = self.started.elapsed().as_secs_f64().max(1e-9);
        MetricsSnapshot {
            counters: &self.counters,
            elapsed_seconds: elapsed,
            unique_blocks_per_second: self.counters.unique_blocks as f64 / elapsed,
            goodput_bytes_per_second: self.counters.unique_bytes as f64 / elapsed,
        }
    }
}
