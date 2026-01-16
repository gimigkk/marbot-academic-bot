// src/dashboard/handlers.rs

use axum::{
    extract::State,
    response::{Html, IntoResponse},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::tui::state::JobStatus;
use crate::AppState;

#[derive(Serialize, Deserialize)]
pub struct JobResponse {
    id: String,
    chat_id: String,
    sender: String,
    status: String,
    logs: Vec<String>,
    duration_ms: u128,
    current_countdown: Option<CountdownResponse>,
    current_trying: Option<String>,
    last_message_ms: Option<u128>,
    message_body: Option<String>,
    quoted_message: Option<String>,
    tags: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct CountdownResponse {
    attempt: u32,
    remaining: u64,
}

#[derive(Serialize, Deserialize)]
pub struct GeneralLogResponse {
    job_id: String,
    message: String,
}

#[derive(Serialize, Deserialize)]
pub struct DashboardData {
    jobs: Vec<JobResponse>,
    general_log: Vec<GeneralLogResponse>,
    stats: StatsResponse,
}

#[derive(Serialize, Deserialize)]
pub struct StatsResponse {
    active: usize,
    completed: usize,
    failed: usize,
    total: usize,
}

const CSS: &str = include_str!("styles.css");
const JS: &str = include_str!("client.js");

pub async fn serve_dashboard_page() -> impl IntoResponse {
    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width,initial-scale=1" />
<title>MARBOT Dashboard</title>
<style>
{}
</style>
</head>
<body>
<div class="app-container">
    <div class="top-left-controls" id="top-controls">
        <button class="control-btn collapse-btn" id="collapse-sidebar" title="Toggle sidebar">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
                <rect x="3" y="3" width="18" height="18" rx="2"/>
                <line x1="9" y1="3" x2="9" y2="21"/>
            </svg>
        </button>

        <div class="search-container" id="search-container">
            <button class="search-icon-btn" id="search-toggle">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <circle cx="11" cy="11" r="8"/>
                    <path d="m21 21-4.35-4.35"/>
                </svg>
            </button>
            <input 
                type="text" 
                class="search-input" 
                id="search-input" 
                placeholder="Search tasks..."
            />
        </div>
    </div>

    <aside class="sidebar" id="sidebar">
        <div class="resize-handle" id="resize-handle"></div>
        <div class="sidebar-spacer"></div>

        <div class="sidebar-main">
            <div class="view-toggle-container">
                <button class="view-toggle-btn" id="view-toggle-btn">
                    <span class="toggle-label">
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                            <rect x="3" y="3" width="7" height="7" rx="1"/>
                            <rect x="3" y="14" width="7" height="7" rx="1"/>
                            <rect x="14" y="3" width="7" height="7" rx="1"/>
                            <rect x="14" y="14" width="7" height="7" rx="1"/>
                        </svg>
                        <span id="toggle-text">Tasks</span>
                    </span>
                    <span class="toggle-arrow">→</span>
                </button>
            </div>

            <div class="toggle-collapsed">
                <button class="toggle-icon-btn" id="toggle-icon-btn" title="Toggle view">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <rect x="3" y="3" width="7" height="7" rx="1"/>
                        <rect x="3" y="14" width="7" height="7" rx="1"/>
                        <rect x="14" y="3" width="7" height="7" rx="1"/>
                        <rect x="14" y="14" width="7" height="7" rx="1"/>
                    </svg>
                </button>
            </div>

            <div class="sidebar-content" id="sidebar-content">
                <div class="empty-sidebar">No tasks yet</div>
            </div>

            <div class="sidebar-footer">
                <div class="stats-row">
                    <div class="stat-item">
                        <span class="stat-label">Active:</span>
                        <span class="stat-value active" id="stat-active">0</span>
                    </div>
                    <div class="stat-item">
                        <span class="stat-label">Done:</span>
                        <span class="stat-value completed" id="stat-completed">0</span>
                    </div>
                    <div class="stat-item">
                        <span class="stat-label">Failed:</span>
                        <span class="stat-value failed" id="stat-failed">0</span>
                    </div>
                </div>
                <div style="text-align: center; opacity: 0.5; font-size: 10px;">
                    <span id="last-update">-</span>
                </div>
            </div>

            <div class="footer-collapsed">
                <div class="footer-stat-item active">
                    <span class="footer-stat-value" id="stat-active-collapsed">0</span>
                    <span class="footer-stat-label">Active</span>
                </div>
                <div class="footer-stat-item completed">
                    <span class="footer-stat-value" id="stat-completed-collapsed">0</span>
                    <span class="footer-stat-label">Done</span>
                </div>
                <div class="footer-stat-item failed">
                    <span class="footer-stat-value" id="stat-failed-collapsed">0</span>
                    <span class="footer-stat-label">Failed</span>
                </div>
            </div>
        </div>
    </aside>

    <main class="main-content" id="main-content">
        <div class="topbar">
            <div class="topbar-title">
                <span class="app-name">MARBOT</span>
                <span id="topbar-subtitle">Task Logs</span>
            </div>
        </div>

        <div class="terminal-container">
            <div class="terminal" id="terminal-content">
                <div class="empty-state">
                    <div class="empty-state-text">No tasks yet</div>
                    <div class="empty-state-subtext">Tasks will appear when jobs start</div>
                </div>
            </div>
        </div>
    </main>
</div>

<script>
{}
</script>
</body>
</html>
"#,
        CSS, JS
    );

    Html(html)
}

pub async fn get_dashboard_data(State(state): State<AppState>) -> Json<DashboardData> {
    let jobs = state.tui_state.get_jobs().await;
    let general_log = state.tui_state.get_general_log().await;

    let active = jobs
        .iter()
        .filter(|j| matches!(j.status, JobStatus::Active))
        .count();
    let completed = jobs
        .iter()
        .filter(|j| matches!(j.status, JobStatus::Completed))
        .count();
    let failed = jobs
        .iter()
        .filter(|j| matches!(j.status, JobStatus::Failed))
        .count();

    let job_responses: Vec<JobResponse> = jobs
        .iter()
        .map(|job| JobResponse {
            id: job.id.clone(),
            chat_id: job.chat_id.clone(),
            sender: job.sender.clone(),
            status: match job.status {
                JobStatus::Active => "active".to_string(),
                JobStatus::Completed => "completed".to_string(),
                JobStatus::Failed => "failed".to_string(),
            },
            logs: job.logs.clone(),
            duration_ms: job.duration().as_millis(),
            current_countdown: job.current_countdown.as_ref().map(|cd| CountdownResponse {
                attempt: cd.attempt,
                remaining: cd.remaining,
            }),
            current_trying: job.current_trying.clone(),
            last_message_ms: None,
            message_body: job.message_body.clone(),
            quoted_message: job.quoted_message.clone(),
            tags: job.tags.clone(),
        })
        .collect();

    let general_log_responses: Vec<GeneralLogResponse> = general_log
        .iter()
        .map(|log| GeneralLogResponse {
            job_id: log.job_id.clone(),
            message: log.message.clone(),
        })
        .collect();

    Json(DashboardData {
        jobs: job_responses,
        general_log: general_log_responses,
        stats: StatsResponse {
            active,
            completed,
            failed,
            total: jobs.len(),
        },
    })
}