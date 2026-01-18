// src/tui/mod.rs

pub mod state;
pub mod logger;

pub use state::TuiState;
pub use logger::{JobLogger, generate_job_id};

use std::sync::Arc;
use tokio::sync::mpsc;

/// Initialize TUI system
pub fn init() -> (Arc<TuiState>, mpsc::UnboundedSender<state::LogEntry>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let tui_state = Arc::new(TuiState::new(rx));
    
    (tui_state, tx)
}

/// Spawn background log collector
pub fn spawn_log_collector(tui_state: Arc<TuiState>) {
    tokio::spawn(async move {
        loop {
            tui_state.process_logs().await;
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    });
}