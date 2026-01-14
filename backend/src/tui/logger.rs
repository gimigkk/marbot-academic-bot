// src/tui/logger.rs
use super::state::{LogEntry, JobStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tokio::sync::mpsc;

/// Global flag to check if TUI is active
pub static TUI_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Job-specific logger that ONLY sends to channel (never prints to stdout in TUI mode)
#[derive(Clone)]
pub struct JobLogger {
    job_id: String,
    tx: mpsc::UnboundedSender<LogEntry>,
}

impl JobLogger {
    pub fn new(job_id: String, tx: mpsc::UnboundedSender<LogEntry>) -> Self {
        Self { job_id, tx }
    }

    /// Log a regular message
    pub fn log(&self, message: &str) {
        // When TUI is active, avoid printing anything to stdout to keep the alternate screen clean.
        if !TUI_ACTIVE.load(Ordering::Relaxed) {
            println!("{}", message);
        }

        let _ = self.tx.send(LogEntry::Message {
            job_id: self.job_id.clone(),
            timestamp: Instant::now(),
            message: message.to_string(),
        });
    }

    /// Log a "TRYING" line that can be overwritten
    pub fn log_trying(&self, model: &str, index: usize, total: usize) {
        let message = format!("│ 🔄 TRYING : {} ({}/{})", model, index, total);
        
        let _ = self.tx.send(LogEntry::TryingUpdate {
            job_id: self.job_id.clone(),
            message,
        });
    }

    /// Clear the "TRYING" line
    pub fn log_trying_clear(&self) {
        let _ = self.tx.send(LogEntry::TryingClear {
            job_id: self.job_id.clone(),
        });
    }

    /// Log countdown (handles both TUI and normal mode)
    pub fn log_countdown(&self, attempt: u32, remaining: u64) {
        if TUI_ACTIVE.load(Ordering::Relaxed) {
            // TUI mode: send countdown update for live rendering
            let _ = self.tx.send(LogEntry::CountdownUpdate {
                job_id: self.job_id.clone(),
                attempt,
                remaining,
            });
        } else {
            // Non-TUI: overwrite the current console line with '\r'
            use std::io::Write;
            print!("\r│ ⏳ RETRY #{} - Waiting {} seconds...     ", attempt, remaining);
            let _ = std::io::stdout().flush();
        }
    }

    /// Clear countdown line (both TUI and console)
    pub fn log_countdown_clear(&self) {
        if TUI_ACTIVE.load(Ordering::Relaxed) {
            let _ = self.tx.send(LogEntry::CountdownClear {
                job_id: self.job_id.clone(),
            });
        } else {
            use std::io::Write;
            print!("\r                                                                  \r");
            let _ = std::io::stdout().flush();
        }
    }

    /// Update job status
    pub fn set_status(&self, status: JobStatus) {
        // Add 2 blank lines before status change for separation
        if matches!(status, JobStatus::Completed | JobStatus::Failed) {
            let _ = self.tx.send(LogEntry::Message {
                job_id: self.job_id.clone(),
                timestamp: Instant::now(),
                message: String::new(),
            });
            let _ = self.tx.send(LogEntry::Message {
                job_id: self.job_id.clone(),
                timestamp: Instant::now(),
                message: String::new(),
            });
        }

        let _ = self.tx.send(LogEntry::StatusChange {
            job_id: self.job_id.clone(),
            status,
        });
    }

    /// Get job ID
    pub fn job_id(&self) -> &str {
        &self.job_id
    }
}

/// Generate a unique job ID
pub fn generate_job_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);

    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("req_{:03}", id)
}