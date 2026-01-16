// src/tui/state.rs
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, RwLock};

// ===== JOB STATUS =====

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JobStatus {
    Active,
    Completed,
    Failed,
}

// ===== LOG ENTRY TYPES =====

#[derive(Debug, Clone)]
pub enum LogEntry {
    Message {
        job_id: String,
        timestamp: Instant,
        message: String,
    },
    TryingUpdate {
        job_id: String,
        message: String,
    },
    TryingClear {
        job_id: String,
    },
    CountdownUpdate {
        job_id: String,
        attempt: u32,
        remaining: u64,
    },
    CountdownClear {
        job_id: String,
    },
    StatusChange {
        job_id: String,
        status: JobStatus,
    },
}

// ===== JOB ENTRY =====

#[derive(Debug, Clone)]
pub struct JobEntry {
    pub id: String,
    pub chat_id: String,
    pub sender: String,
    pub status: JobStatus,
    pub logs: Vec<String>,
    pub started_at: Instant,
    pub completed_at: Option<Instant>,
    pub current_countdown: Option<CountdownState>,
    pub current_trying: Option<String>,
    pub message_body: Option<String>,
    pub quoted_message: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CountdownState {
    pub attempt: u32,
    pub remaining: u64,
}

impl JobEntry {
    pub fn new(
        id: String, 
        chat_id: String, 
        sender: String,
        message_body: Option<String>,
        quoted_message: Option<String>,
        tags: Vec<String>,
    ) -> Self {
        Self {
            id,
            chat_id,
            sender,
            status: JobStatus::Active,
            logs: Vec::new(),
            started_at: Instant::now(),
            completed_at: None,
            current_countdown: None,
            current_trying: None,
            message_body,
            quoted_message,
            tags,
        }
    }

    pub fn duration(&self) -> std::time::Duration {
        match self.completed_at {
            Some(end) => end.duration_since(self.started_at),
            None => Instant::now().duration_since(self.started_at),
        }
    }

    pub fn add_log(&mut self, message: String) {
        if self.logs.len() > 5000 {
            self.logs.drain(0..1000);
        }
        self.logs.push(message);
    }

    pub fn set_trying(&mut self, message: String) {
        self.current_trying = Some(message);
    }

    pub fn clear_trying(&mut self) {
        self.current_trying = None;
    }

    pub fn update_countdown(&mut self, attempt: u32, remaining: u64) {
        self.current_countdown = Some(CountdownState { attempt, remaining });
    }

    pub fn clear_countdown(&mut self) {
        self.current_countdown = None;
    }

    pub fn set_status(&mut self, status: JobStatus) {
        self.status = status;
        if matches!(status, JobStatus::Completed | JobStatus::Failed) {
            self.completed_at = Some(Instant::now());
            self.clear_countdown();
            self.clear_trying();
        }
    }
}

// ===== GENERAL LOG LINE =====

#[derive(Debug, Clone)]
pub struct GeneralLogLine {
    pub job_id: String,
    pub timestamp: Instant,
    pub message: String,
}

// ===== TUI STATE =====

pub struct TuiState {
    jobs: Arc<RwLock<HashMap<String, JobEntry>>>,
    general_log: Arc<RwLock<VecDeque<GeneralLogLine>>>,
    log_rx: Arc<RwLock<mpsc::UnboundedReceiver<LogEntry>>>,
    max_completed: usize,
    max_general_log: usize,
}

impl TuiState {
    pub fn new(log_rx: mpsc::UnboundedReceiver<LogEntry>) -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            general_log: Arc::new(RwLock::new(VecDeque::new())),
            log_rx: Arc::new(RwLock::new(log_rx)),
            max_completed: 20,
            max_general_log: 2000,
        }
    }

    pub async fn process_logs(&self) {
        // Take ownership of receiver while we drain
        let mut rx = self.log_rx.write().await;
        let mut processed = 0usize;
        const MAX_BATCH: usize = 200;

        while processed < MAX_BATCH {
            match rx.try_recv() {
                Ok(entry) => {
                    self.process_single_entry(entry).await;
                    processed += 1;
                }
                Err(_) => break,
            }
        }

        if processed > 0 {
            self.cleanup_old_jobs().await;
            self.cleanup_general_log().await;
        }
    }

    async fn process_single_entry(&self, entry: LogEntry) {
        match entry {
            LogEntry::Message { job_id, timestamp, message } => {
                {
                    let mut jobs = self.jobs.write().await;
                    if let Some(job) = jobs.get_mut(&job_id) {
                        job.add_log(message.clone());
                    }
                }

                let mut general = self.general_log.write().await;
                general.push_back(GeneralLogLine {
                    job_id,
                    timestamp,
                    message,
                });
            }
            LogEntry::TryingUpdate { job_id, message } => {
                let mut jobs = self.jobs.write().await;
                if let Some(job) = jobs.get_mut(&job_id) {
                    job.set_trying(message);
                }
            }
            LogEntry::TryingClear { job_id } => {
                let mut jobs = self.jobs.write().await;
                if let Some(job) = jobs.get_mut(&job_id) {
                    job.clear_trying();
                }
            }
            LogEntry::CountdownUpdate { job_id, attempt, remaining } => {
                let mut jobs = self.jobs.write().await;
                if let Some(job) = jobs.get_mut(&job_id) {
                    job.update_countdown(attempt, remaining);
                }
            }
            LogEntry::CountdownClear { job_id } => {
                let mut jobs = self.jobs.write().await;
                if let Some(job) = jobs.get_mut(&job_id) {
                    job.clear_countdown();
                }
            }
            LogEntry::StatusChange { job_id, status } => {
                let mut jobs = self.jobs.write().await;
                if let Some(job) = jobs.get_mut(&job_id) {
                    job.set_status(status);
                }
            }
        }
    }

    async fn cleanup_old_jobs(&self) {
        let mut jobs = self.jobs.write().await;

        let mut completed: Vec<_> = jobs
            .iter()
            .filter(|(_, job)| matches!(job.status, JobStatus::Completed | JobStatus::Failed))
            .filter_map(|(id, job)| job.completed_at.map(|t| (id.clone(), t)))
            .collect();

        if completed.len() > self.max_completed {
            completed.sort_by_key(|(_, time)| *time);
            let to_remove = completed.len() - self.max_completed;
            for (id, _) in completed.iter().take(to_remove) {
                jobs.remove(id);
            }
        }
    }

    async fn cleanup_general_log(&self) {
        let mut log = self.general_log.write().await;

        if log.len() > self.max_general_log {
            let drain_count = log.len() - self.max_general_log;
            for _ in 0..drain_count {
                log.pop_front();
            }
        }
    }

    pub async fn create_job(
        &self, 
        id: String, 
        chat_id: String, 
        sender: String,
        message_body: Option<String>,
        quoted_message: Option<String>,
        tags: Vec<String>,
    ) {
        let mut jobs = self.jobs.write().await;
        jobs.insert(
            id.clone(), 
            JobEntry::new(id, chat_id, sender, message_body, quoted_message, tags)
        );
    }

    pub async fn get_jobs(&self) -> Vec<JobEntry> {
        let jobs = self.jobs.read().await;
        jobs.values().cloned().collect()
    }

    pub async fn get_general_log(&self) -> Vec<GeneralLogLine> {
        let log = self.general_log.read().await;
        log.iter().cloned().collect()
    }

    pub async fn clear_completed(&self) {
        let mut jobs = self.jobs.write().await;
        jobs.retain(|_, job| matches!(job.status, JobStatus::Active));
    }
}