//! Running tunnels and their traffic samples, for the UI's flow diagram.

use crate::ssh::Traffic;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const HISTORY: usize = 60;

pub struct Running {
    pub task: tauri::async_runtime::JoinHandle<()>,
    pub status: Arc<Mutex<String>>,
    pub traffic: Arc<Traffic>,
    /// Bytes/s per second: [fwd, back].
    history: VecDeque<[f32; 2]>,
    last: [u64; 2],
    at: Instant,
    done: Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Serialize)]
pub struct Stats {
    live: bool,
    status: String,
    /// Bytes/s per second, oldest first, up to a minute: [out, in].
    history: Vec<[f32; 2]>,
    active: usize,
    total: u64,
    sent: u64,
    received: u64,
    /// Most recent SOCKS destination.
    target: String,
}

impl Running {
    pub fn new(
        task: tauri::async_runtime::JoinHandle<()>,
        status: Arc<Mutex<String>>,
        traffic: Arc<Traffic>,
        done: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self { task, status, traffic, history: VecDeque::new(), last: [0, 0], at: Instant::now(), done }
    }

    pub fn live(&self) -> bool {
        !self.done.load(Relaxed)
    }

    /// Samples throughput (once a second at most) and reports.
    pub fn stats(&mut self) -> Stats {
        let dt = self.at.elapsed().as_secs_f32();
        let now = [self.traffic.fwd.load(Relaxed), self.traffic.back.load(Relaxed)];
        if dt >= 1.0 {
            self.history.push_back([(now[0] - self.last[0]) as f32 / dt, (now[1] - self.last[1]) as f32 / dt]);
            if self.history.len() > HISTORY {
                self.history.pop_front();
            }
            (self.last, self.at) = (now, Instant::now());
        }
        Stats {
            live: self.live(),
            status: self.status.lock().unwrap().clone(),
            history: self.history.iter().copied().collect(),
            active: self.traffic.active.load(Relaxed),
            total: self.traffic.total.load(Relaxed),
            sent: now[0],
            received: now[1],
            target: self.traffic.targets.lock().unwrap().last().cloned().unwrap_or_default(),
        }
    }
}
