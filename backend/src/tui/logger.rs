// src/tui/logger.rs
use super::state::{LogEntry, JobStatus};
use std::time::Instant;
use tokio::sync::mpsc;

/// Job-specific logger that logs to BOTH stdout and web dashboard channel
#[derive(Clone)]
pub struct JobLogger {
    job_id: String,
    tx: mpsc::UnboundedSender<LogEntry>,
}

impl JobLogger {
    pub fn new(job_id: String, tx: mpsc::UnboundedSender<LogEntry>) -> Self {
        Self { job_id, tx }
    }

    /// Log a regular message (prints to stdout AND sends to dashboard)
    pub fn log(&self, message: &str) {
        // Always print to stdout for SSH/docker logs
        println!("{}", message);

        // Always send to dashboard channel
        let _ = self.tx.send(LogEntry::Message {
            job_id: self.job_id.clone(),
            timestamp: Instant::now(),
            message: message.to_string(),
        });
    }

    /// Log a "TRYING" line that can be overwritten in dashboard
    pub fn log_trying(&self, model: &str, index: usize, total: usize) {
        let message = format!("│ 🔄 TRYING : {} ({}/{})", model, index, total);
        
        // Print to stdout
        println!("{}", message);
        
        // Send to dashboard with special trying update
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

    /// Log countdown with console overwrite using '\r'
    pub fn log_countdown(&self, attempt: u32, remaining: u64) {
        // Console: overwrite current line with '\r'
        use std::io::Write;
        print!("\r│ ⏳ RETRY #{} - Waiting {} seconds...     ", attempt, remaining);
        let _ = std::io::stdout().flush();
        
        // Dashboard: send countdown update for live rendering
        let _ = self.tx.send(LogEntry::CountdownUpdate {
            job_id: self.job_id.clone(),
            attempt,
            remaining,
        });
    }

    /// Clear countdown line
    pub fn log_countdown_clear(&self) {
        // Console: clear the line
        use std::io::Write;
        print!("\r                                                                  \r");
        let _ = std::io::stdout().flush();
        
        // Dashboard: clear countdown state
        let _ = self.tx.send(LogEntry::CountdownClear {
            job_id: self.job_id.clone(),
        });
    }

    /// Update job status
    pub fn set_status(&self, status: JobStatus) {
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