// src/tui/mod.rs

pub mod state;
pub mod logger;
pub mod render;

pub use state::TuiState;
pub use logger::{JobLogger, generate_job_id, TUI_ACTIVE};

use std::sync::Arc;
use tokio::sync::mpsc;
use std::io::{self, BufRead};

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

/// Spawn TUI listener - just reads stdin
pub fn spawn_tui_listener(tui_state: Arc<TuiState>) {
    // Spawn blocking stdin reader
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let reader = stdin.lock();
        
        for line in reader.lines() {
            if let Ok(text) = line {
                let trimmed = text.trim();
                
                // Check for "tui" command (case insensitive)
                if trimmed.eq_ignore_ascii_case("tui") || trimmed == "!!!" {
                    println!("\n🎨 Entering TUI mode...\n");
                    
                    // Activate TUI
                    logger::TUI_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
                    
                    // Run TUI in blocking mode
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    if let Err(e) = rt.block_on(render::run_tui(tui_state.clone())) {
                        eprintln!("❌ TUI Error: {}", e);
                    }
                    
                    // Deactivate TUI when exited
                    logger::TUI_ACTIVE.store(false, std::sync::atomic::Ordering::Relaxed);
                    
                    println!("\n✅ Exited TUI mode. Type 'tui' to re-enter.\n");
                }
            }
        }
    });
}