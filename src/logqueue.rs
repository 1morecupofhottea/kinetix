//! Asynchronous, bounded usage-log queue (FR-6.4, NFR-1.7).
//!
//! Logging must never block or fail a client request. Writes go through a
//! bounded channel; if the queue is full we drop the row and count it rather
//! than blocking the request path.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::db::{self, Pool, UsageLogRow};

#[derive(Clone)]
pub struct UsageLogQueue {
    tx: mpsc::Sender<UsageLogRow>,
    dropped: Arc<AtomicU64>,
    depth: Arc<AtomicU64>,
}

impl UsageLogQueue {
    pub fn new(pool: Pool, capacity: usize) -> Self {
        let (tx, mut rx) = mpsc::channel::<UsageLogRow>(capacity);
        let dropped = Arc::new(AtomicU64::new(0));
        let depth = Arc::new(AtomicU64::new(0));

        let dropped_task = dropped.clone();
        let depth_task = depth.clone();
        tokio::spawn(async move {
            let mut batch: Vec<UsageLogRow> = Vec::with_capacity(64);
            loop {
                // Drain whatever is available, then flush.
                let first = rx.recv().await;
                let Some(first) = first else { break };
                batch.push(first);
                while batch.len() < 128 {
                    match rx.try_recv() {
                        Ok(row) => batch.push(row),
                        Err(_) => break,
                    }
                }
                for row in batch.drain(..) {
                    depth_task.fetch_sub(1, Ordering::Relaxed);
                    if let Err(e) = db::insert_usage_log(&pool, &row).await {
                        tracing::warn!(error = %e, "failed to write usage log row");
                        dropped_task.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        });

        UsageLogQueue { tx, dropped, depth }
    }

    /// Enqueue a usage row. Never blocks; drops (and counts) when full.
    pub fn enqueue(&self, row: UsageLogRow) {
        match self.tx.try_send(row) {
            Ok(()) => {
                self.depth.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn depth(&self) -> u64 {
        self.depth.load(Ordering::Relaxed)
    }
}
