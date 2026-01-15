// src/tui/mod.rs

pub mod state;
pub mod logger;
pub mod render;

pub use state::TuiState;
pub use logger::{JobLogger, generate_job_id, TUI_ACTIVE};

use std::sync::Arc;
use tokio::sync::mpsc;
use std::path::Path;

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

/// Spawn TUI listener - watches for trigger file
pub fn spawn_tui_listener(tui_state: Arc<TuiState>) {
    tokio::spawn(async move {
        let trigger_path = "/tmp/marbot_tui";
        
        loop {
            if Path::new(trigger_path).exists() {
                let _ = std::fs::remove_file(trigger_path);
                
                logger::TUI_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
                
                let _ = render::run_tui(tui_state.clone()).await;
                
                logger::TUI_ACTIVE.store(false, std::sync::atomic::Ordering::Relaxed);
            }
            
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    });
}