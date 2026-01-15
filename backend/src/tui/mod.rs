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

/// Spawn TUI listener (waits for F2 key OR "tui" command via stdin)
pub fn spawn_tui_listener(tui_state: Arc<TuiState>) {
    tokio::spawn(async move {
        wait_for_hotkey_or_command(tui_state).await;
    });
}

/// Wait for F2 key press OR stdin "tui" command and launch TUI
async fn wait_for_hotkey_or_command(tui_state: Arc<TuiState>) {
    use crossterm::event::{self, Event, KeyCode, KeyModifiers};
    use crossterm::terminal::{enable_raw_mode, disable_raw_mode};
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, BufReader};
    
    // Spawn stdin reader in separate task
    let tui_state_stdin = tui_state.clone();
    tokio::spawn(async move {
        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();
        
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.eq_ignore_ascii_case("tui") {
                        println!("\n🎨 Entering TUI mode...\n");
                        
                        // Activate TUI
                        logger::TUI_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
                        
                        // Run TUI (blocking)
                        if let Err(e) = render::run_tui(tui_state_stdin.clone()).await {
                            eprintln!("❌ TUI Error: {}", e);
                        }
                        
                        // Deactivate TUI when exited
                        logger::TUI_ACTIVE.store(false, std::sync::atomic::Ordering::Relaxed);
                        
                        println!("\n✅ Exited TUI mode. Type 'tui' or press \x1b[1;33mCtrl+T\x1b[0m to re-enter.\n");
                    }
                }
                Err(_) => break,
            }
        }
    });
    
    // Try to enable raw mode for key detection (only works in interactive terminals)
    let raw_mode_enabled = enable_raw_mode().is_ok();
    
    if !raw_mode_enabled {
        // Not an error - just means we're in non-interactive mode (like docker logs)
        println!("💡 Type 'tui' and press Enter to launch TUI, or press \x1b[1;33mCtrl+T\x1b[0m\n");
        
        // Keep the task alive but don't poll for keys
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    }
    
    loop {
        // Non-blocking poll for hotkeys (only works if raw mode succeeded)
        match event::poll(Duration::from_millis(100)) {
            Ok(true) => {
                match event::read() {
                    Ok(Event::Key(key)) => {
                        // Ctrl+T as hotkey (more reliable than F2 over SSH)
                        let is_ctrl_t = key.code == KeyCode::Char('t') && key.modifiers == KeyModifiers::CONTROL;
                        
                        if is_ctrl_t {
                            println!("\n🎨 Entering TUI mode...\n");
                            
                            // Disable raw mode before entering TUI
                            let _ = disable_raw_mode();
                            
                            // Activate TUI
                            logger::TUI_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
                            
                            // Run TUI (blocking)
                            if let Err(e) = render::run_tui(tui_state.clone()).await {
                                eprintln!("❌ TUI Error: {}", e);
                            }
                            
                            // Deactivate TUI when exited
                            logger::TUI_ACTIVE.store(false, std::sync::atomic::Ordering::Relaxed);
                            
                            // Re-enable raw mode
                            let _ = enable_raw_mode();
                            
                            println!("\n✅ Exited TUI mode. Type 'tui' or press \x1b[1;33mCtrl+T\x1b[0m to re-enter.\n");
                        }
                    }
                    Err(_) => {
                        // Error reading keys, fall back to stdin only
                        let _ = disable_raw_mode();
                        println!("💡 Hotkey disabled. Type 'tui' and press Enter to launch TUI\n");
                        
                        loop {
                            tokio::time::sleep(Duration::from_secs(3600)).await;
                        }
                    }
                    _ => {}
                }
            }
            Ok(false) => {
                // No event available
            }
            Err(_) => {
                // Polling error, disable hotkey detection
                let _ = disable_raw_mode();
                println!("💡 Hotkey disabled. Type 'tui' and press Enter to launch TUI\n");
                
                loop {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                }
            }
        }
        
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}