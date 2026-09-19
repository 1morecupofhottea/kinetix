//! Live in-flight request view (FR-8.3).
//!
//! A bounded, in-memory registry of requests currently being served (plus a
//! short tail of recently finished ones) so the dashboard can show status,
//! latency, commit state, fallback state, and token counts in real time.
//!
//! This is control-plane observability only: it never blocks the data plane
//! (NFR-2.6), holds no bodies or secrets (FR-13.2), and is bounded so a burst
//! cannot grow memory without limit (NFR-1.3).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use dashmap::DashMap;
use parking_lot::Mutex;
use serde::Serialize;

/// How many recently finished requests to keep visible after completion.
const FINISHED_TAIL: usize = 64;

#[derive(Debug, Clone, Serialize)]
pub struct LiveRequest {
    pub request_id: String,
    pub key_name: Option<String>,
    pub frontend: String,
    pub requested_model: String,
    pub route_name: Option<String>,
    /// `selecting` | `streaming` | `committed` | `done`
    pub phase: String,
    pub commit_state: String,
    pub fallback_hops: u32,
    pub retry_count: u32,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub status: String,
    pub latency_ms: u64,
    pub ttft_ms: Option<i64>,
    pub started_ms_ago: u64,
    pub finished: bool,
}

struct Inner {
    live: DashMap<String, LiveRequest>,
    /// Ordering for eviction (oldest first).
    order: Mutex<VecDeque<String>>,
    max_live: usize,
    dropped: AtomicU64,
}

#[derive(Clone)]
pub struct LiveRequests {
    inner: std::sync::Arc<Inner>,
    started: Instant,
}

impl LiveRequests {
    pub fn new(max_live: usize) -> Self {
        Self {
            inner: std::sync::Arc::new(Inner {
                live: DashMap::new(),
                order: Mutex::new(VecDeque::new()),
                max_live,
                dropped: AtomicU64::new(0),
            }),
            started: Instant::now(),
        }
    }

    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// Register a request as it enters the pipeline.
    pub fn start(
        &self,
        request_id: &str,
        frontend: &str,
        requested_model: &str,
        key_name: Option<String>,
        route_name: Option<String>,
    ) {
        let entry = LiveRequest {
            request_id: request_id.to_string(),
            key_name,
            frontend: frontend.to_string(),
            requested_model: requested_model.to_string(),
            route_name,
            phase: "selecting".into(),
            commit_state: "not_committed".into(),
            fallback_hops: 0,
            retry_count: 0,
            input_tokens: None,
            output_tokens: None,
            status: "in_flight".into(),
            latency_ms: 0,
            ttft_ms: None,
            started_ms_ago: 0,
            finished: false,
        };
        self.inner.live.insert(request_id.to_string(), entry);
        let mut order = self.inner.order.lock();
        order.push_back(request_id.to_string());
        // Bound the map: evict oldest finished/live entries beyond the cap.
        while order.len() > self.inner.max_live {
            if let Some(old) = order.pop_front() {
                if self.inner.live.remove(&old).is_some() {
                    self.inner.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    fn with<R>(&self, request_id: &str, f: impl FnOnce(&mut LiveRequest) -> R) {
        if let Some(mut e) = self.inner.live.get_mut(request_id) {
            f(&mut e);
        }
    }

    /// Mark the request as committed (first client bytes sent).
    pub fn mark_committed(&self, request_id: &str) {
        self.with(request_id, |e| {
            e.phase = "committed".into();
            e.commit_state = "committed".into();
        });
    }

    pub fn mark_streaming(&self, request_id: &str) {
        self.with(request_id, |e| {
            if e.phase == "selecting" {
                e.phase = "streaming".into();
            }
        });
    }

    pub fn set_ttft(&self, request_id: &str, ttft_ms: i64) {
        self.with(request_id, |e| {
            e.ttft_ms = Some(ttft_ms);
        });
    }

    pub fn set_fallback_hops(&self, request_id: &str, hops: u32, retries: u32) {
        self.with(request_id, |e| {
            e.fallback_hops = hops;
            e.retry_count = retries;
        });
    }

    /// Mark a request finished; it stays visible in the tail briefly.
    pub fn finish(
        &self,
        request_id: &str,
        status: &str,
        latency_ms: u64,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    ) {
        self.with(request_id, |e| {
            e.phase = "done".into();
            e.finished = true;
            e.status = status.to_string();
            e.latency_ms = latency_ms;
            e.input_tokens = input_tokens;
            e.output_tokens = output_tokens;
        });
        // Trim the finished tail so long-idle entries do not accumulate.
        let mut order = self.inner.order.lock();
        while order.len() > self.inner.max_live + FINISHED_TAIL {
            if let Some(old) = order.pop_front() {
                self.inner.live.remove(&old);
            }
        }
    }

    /// Snapshot of live + recently finished requests, newest first.
    pub fn snapshot(&self) -> Vec<LiveRequest> {
        let now = self.now_ms();
        let mut out: Vec<LiveRequest> = self
            .inner
            .live
            .iter()
            .map(|e| {
                let mut r = e.clone();
                r.started_ms_ago = now.saturating_sub(0);
                r
            })
            .collect();
        // Newest first: in-flight before finished, then by insertion order.
        let order = self.inner.order.lock();
        let pos: std::collections::HashMap<&String, usize> =
            order.iter().enumerate().map(|(i, k)| (k, i)).collect();
        out.sort_by_key(|r| {
            let p = pos.get(&r.request_id).copied().unwrap_or(usize::MAX);
            (r.finished, std::cmp::Reverse(p))
        });
        out
    }

    pub fn live_count(&self) -> usize {
        self.inner.live.iter().filter(|e| !e.finished).count()
    }

    pub fn dropped(&self) -> u64 {
        self.inner.dropped.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_lifecycle_and_bounds_memory() {
        let lr = LiveRequests::new(4);
        for i in 0..10 {
            lr.start(&format!("r{i}"), "openai", "m", None, None);
        }
        // Bounded: never more than max_live entries retained.
        assert!(lr.snapshot().len() <= 4);
        assert!(lr.dropped() > 0);

        lr.start("x", "openai", "m", Some("key".into()), Some("route".into()));
        lr.mark_committed("x");
        lr.set_ttft("x", 12);
        lr.set_fallback_hops("x", 1, 1);
        lr.finish("x", "success", 99, Some(10), Some(20));
        let snap = lr.snapshot();
        let x = snap.iter().find(|r| r.request_id == "x").unwrap();
        assert_eq!(x.commit_state, "committed");
        assert_eq!(x.status, "success");
        assert_eq!(x.fallback_hops, 1);
        assert_eq!(x.input_tokens, Some(10));
        assert!(x.finished);
        // Finished requests are excluded from the live count.
        assert!(lr.live_count() < snap.len());
    }
}
