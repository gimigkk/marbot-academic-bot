// src/tui/mod.rs

pub mod state;
pub mod logger;
pub mod render;

pub use state::TuiState;
pub use logger::{JobLogger, generate_job_id, TUI_ACTIVE};

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

/// Spawn TUI listener (waits for F2 key)
pub fn spawn_tui_listener(tui_state: Arc<TuiState>) {
    tokio::spawn(async move {
        wait_for_hotkey(tui_state).await;
    });
}

/// Wait for F2 key press and launch TUI
async fn wait_for_hotkey(tui_state: Arc<TuiState>) {
    use crossterm::event::{self, Event, KeyCode, KeyModifiers};
    use std::time::Duration;
    
    //println!("\n💡 Press \x1b[1;33mF2\x1b[0m anytime to enter TUI mode\n");
    
    loop {
        // Non-blocking poll (don't block the async runtime)
        if event::poll(Duration::from_millis(100)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                // F2 key pressed
                if key.code == KeyCode::F(2) && key.modifiers == KeyModifiers::NONE {
                    println!("\n🎨 Entering TUI mode...\n");
                    
                    // Activate TUI
                    logger::TUI_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
                    
                    // Run TUI (blocking)
                    if let Err(e) = render::run_tui(tui_state.clone()).await {
                        eprintln!("❌ TUI Error: {}", e);
                    }
                    
                    // Deactivate TUI when exited
                    logger::TUI_ACTIVE.store(false, std::sync::atomic::Ordering::Relaxed);
                    
                    println!("\n✅ Exited TUI mode. Press \x1b[1;33mF2\x1b[0m to re-enter.\n");
                }
            }
        }
        
        // Small sleep to prevent busy-waiting
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}