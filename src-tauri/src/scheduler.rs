//! Sync scheduling (docs/06 §8; FR-SYN-4).
//!
//! Triggers per pair: `on_connect` (once per transition to connected,
//! after a 10 s settle delay) and `interval(minutes)` while connected.
//! Enqueued runs are serialized by the global runner in [`crate::sync`].

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::state::AppState;
use crate::sync::{self, RunOptions, Trigger};

const CONNECT_SETTLE: Duration = Duration::from_secs(10);
const TICK: Duration = Duration::from_secs(30);

pub async fn scheduler_loop(state: Arc<AppState>) {
    let mut rx = state.subscribe_connection();
    let mut connected = *rx.borrow();
    let mut connected_since: Option<Instant> = connected.then(Instant::now);
    // Per-pair anchor for the interval trigger; the timer resets after each
    // enqueue and on (re)connect.
    let mut anchors: HashMap<String, Instant> = HashMap::new();

    loop {
        tokio::select! {
            changed = rx.changed() => {
                if changed.is_err() {
                    return; // state dropped; app shutting down
                }
                let now_connected = *rx.borrow();
                if now_connected && !connected {
                    connected_since = Some(Instant::now());
                    anchors.clear();
                    // On-connect trigger with settle delay (docs/06 §8).
                    let state = state.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(CONNECT_SETTLE).await;
                        if !state.is_connected().await {
                            return;
                        }
                        for pair in state.stores.load_sync_pairs() {
                            if pair.enabled && pair.on_connect {
                                sync::enqueue(
                                    &state,
                                    &pair.id,
                                    Trigger::OnConnect,
                                    RunOptions::default(),
                                );
                            }
                        }
                    });
                }
                if !now_connected {
                    connected_since = None;
                }
                connected = now_connected;
            }
            _ = tokio::time::sleep(TICK) => {
                if !connected {
                    continue;
                }
                let base = connected_since.unwrap_or_else(Instant::now);
                for pair in state.stores.load_sync_pairs() {
                    let Some(minutes) = pair.interval_minutes else { continue };
                    if !pair.enabled || minutes == 0 {
                        continue;
                    }
                    let anchor = anchors.get(&pair.id).copied().unwrap_or(base);
                    if anchor.elapsed() >= Duration::from_secs(u64::from(minutes) * 60) {
                        anchors.insert(pair.id.clone(), Instant::now());
                        sync::enqueue(&state, &pair.id, Trigger::Interval, RunOptions::default());
                    }
                }
            }
        }
    }
}
