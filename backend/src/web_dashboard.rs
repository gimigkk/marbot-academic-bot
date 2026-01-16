// src/web_dashboard.rs

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

pub async fn serve_dashboard_page() -> impl IntoResponse {
    Html(DASHBOARD_HTML)
}

pub async fn get_dashboard_data(
    State(state): State<AppState>,
) -> Json<DashboardData> {
    let jobs = state.tui_state.get_jobs().await;
    let general_log = state.tui_state.get_general_log().await;

    let active = jobs.iter().filter(|j| matches!(j.status, JobStatus::Active)).count();
    let completed = jobs.iter().filter(|j| matches!(j.status, JobStatus::Completed)).count();
    let failed = jobs.iter().filter(|j| matches!(j.status, JobStatus::Failed)).count();

    let job_responses: Vec<JobResponse> = jobs.iter().map(|job| {
        JobResponse {
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
        }
    }).collect();

    let general_log_responses: Vec<GeneralLogResponse> = general_log.iter().map(|log| {
        GeneralLogResponse {
            job_id: log.job_id.clone(),
            message: log.message.clone(),
        }
    }).collect();

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

const DASHBOARD_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width,initial-scale=1" />
<title>MARBOT Dashboard</title>
<style>
    * { box-sizing: border-box; margin: 0; padding: 0; }
    
    :root {
        --sidebar-width: 260px;
        --sidebar-collapsed-width: 64px;
        --bg: #0a0a0a;
        --sidebar-bg: #1a1a1a;
        --border: #2a2a2a;
        --text-primary: #e0e0e0;
        --text-secondary: #999;
        --text-tertiary: #666;
        --hover-bg: #141414;
        --selected-bg: #141414;
        --accent: #c66143;
    }

    html, body { height: 100%; }
    body {
        background: var(--bg);
        color: var(--text-primary);
        font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
        overflow: hidden;
    }

    .app-container {
        display: flex;
        height: 100vh;
        position: relative;
    }

    .collapse-btn {
        position: fixed;
        left: 16px;
        top: 16px;
        width: 32px;
        height: 32px;
        border-radius: 6px;
        border: 1px solid var(--border);
        background: var(--sidebar-bg);
        color: var(--text-secondary);
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: center;
        font-size: 18px;
        transition: all 0.2s ease;
        padding: 0;
        z-index: 100;
        flex-shrink: 0;
    }

    .collapse-btn:hover {
        background: var(--hover-bg);
        color: var(--text-primary);
    }

    .collapse-btn svg {
        width: 18px;
        height: 18px;
        transition: transform 0.2s ease;
    }

    .sidebar.collapsed ~ .collapse-btn svg {
        transform: rotate(180deg);
    }

    .sidebar {
        width: 260px;
        background: var(--sidebar-bg);
        border-right: 1px solid var(--border);
        display: flex;
        flex-direction: column;
        transition: width 0.2s cubic-bezier(0.4, 0, 0.2, 1);
        position: relative;
        z-index: 10;
        flex-shrink: 0;
        overflow: hidden;
    }

    .sidebar.resizing {
        transition: none;
        user-select: none;
    }

    .sidebar.collapsed {
        width: var(--sidebar-collapsed-width) !important;
    }

    .resize-handle {
        position: absolute;
        right: 0;
        top: 0;
        bottom: 0;
        width: 8px;
        cursor: col-resize;
        z-index: 10;
        display: flex;
        align-items: center;
        justify-content: center;
        transition: background 0.2s ease;
    }

    .resize-handle::before {
        content: '';
        position: absolute;
        width: 2px;
        height: 32px;
        background: var(--text-tertiary);
        border-radius: 1px;
        opacity: 0;
        transition: opacity 0.2s ease;
    }

    .resize-handle:hover::before,
    .resize-handle.active::before {
        opacity: 0.5;
    }

    .resize-handle:hover,
    .resize-handle.active {
        background: rgba(198, 97, 67, 0.1);
    }

    .sidebar.collapsed .resize-handle {
        display: none;
    }

    .sidebar-spacer {
        height: 64px;
        flex-shrink: 0;
    }

    .sidebar-main {
        flex: 1;
        display: flex;
        flex-direction: column;
        min-height: 0;
    }

    /* Smooth fade transitions for all content */
    .sidebar-content,
    .sidebar-footer,
    .view-toggle-container,
    .search-container,
    .toggle-collapsed,
    .footer-collapsed {
        transition: opacity 0.15s ease, visibility 0.15s ease;
    }

    /* Expanded state content */
    .sidebar:not(.collapsed) .sidebar-content,
    .sidebar:not(.collapsed) .sidebar-footer,
    .sidebar:not(.collapsed) .view-toggle-container,
    .sidebar:not(.collapsed) .search-container {
        opacity: 1;
        visibility: visible;
    }

    .sidebar:not(.collapsed) .toggle-collapsed,
    .sidebar:not(.collapsed) .footer-collapsed {
        opacity: 0;
        visibility: hidden;
        height: 0;
        padding: 0;
        border: none;
        overflow: hidden;
        flex: 0;
        min-height: 0;
    }

    /* Collapsed state content */
    .sidebar.collapsed .sidebar-content,
    .sidebar.collapsed .sidebar-footer,
    .sidebar.collapsed .view-toggle-container,
    .sidebar.collapsed .search-container {
        opacity: 0;
        visibility: hidden;
        height: 0;
        padding: 0;
        border: none;
        overflow: hidden;
        flex: 0;
        min-height: 0;
    }

    .sidebar.collapsed .toggle-collapsed,
    .sidebar.collapsed .footer-collapsed {
        opacity: 1;
        visibility: visible;
    }

    .sidebar-content {
        flex: 1;
        overflow-y: auto;
        padding: 8px 16px;
        min-height: 0;
    }

    .sidebar-content::-webkit-scrollbar { width: 6px; }
    .sidebar-content::-webkit-scrollbar-track { background: transparent; }
    .sidebar-content::-webkit-scrollbar-thumb { background: #333; border-radius: 3px; }
    .sidebar-content::-webkit-scrollbar-thumb:hover { background: #444; }

    .sidebar-footer {
        padding: 16px;
        border-top: 1px solid var(--border);
        font-size: 12px;
        color: var(--text-secondary);
        flex-shrink: 0;
        margin-top: auto;
    }

    .view-toggle-container {
        padding: 16px;
        flex-shrink: 0;
    }

    .search-container {
        padding: 0 16px 12px 16px;
        flex-shrink: 0;
    }

    .search-input {
        width: 100%;
        padding: 8px 12px;
        background: var(--bg);
        border: 1px solid var(--border);
        border-radius: 6px;
        color: var(--text-primary);
        font-size: 13px;
        outline: none;
        transition: border-color 0.2s ease;
    }

    .search-input:focus {
        border-color: var(--accent);
    }

    .search-input::placeholder {
        color: var(--text-tertiary);
    }

    .stats-row {
        display: flex;
        justify-content: space-between;
        margin-bottom: 12px;
        font-family: 'JetBrains Mono', monospace;
        font-size: 11px;
    }

    .stat-item {
        display: flex;
        align-items: center;
        gap: 6px;
    }

    .stat-label {
        color: var(--text-tertiary);
    }

    .stat-value {
        font-weight: 600;
    }

    .stat-value.active { color: #4ade80; }
    .stat-value.completed { color: #38bdf8; }
    .stat-value.failed { color: #f87171; }

    .view-toggle-btn {
        width: 100%;
        padding: 10px 16px;
        background: var(--sidebar-bg);
        border: 1px solid var(--border);
        border-radius: 8px;
        color: var(--text-primary);
        cursor: pointer;
        transition: all 0.2s ease;
        font-size: 13px;
        font-weight: 500;
        display: flex;
        align-items: center;
        justify-content: space-between;
        position: relative;
        overflow: hidden;
    }

    .view-toggle-btn:hover {
        background: var(--hover-bg);
        border-color: var(--accent);
    }

    .view-toggle-btn::before {
        content: '';
        position: absolute;
        left: 0;
        top: 0;
        bottom: 0;
        width: 3px;
        background: var(--accent);
        transition: opacity 0.2s ease;
        opacity: 0;
    }

    .view-toggle-btn:hover::before {
        opacity: 1;
    }

    .view-toggle-btn.spinning .toggle-label svg,
    .toggle-icon-btn.spinning svg {
        animation: spin 0.3s ease-in-out;
    }

    @keyframes spin {
        from { transform: rotate(0deg); }
        to { transform: rotate(180deg); }
    }

    .toggle-label {
        display: flex;
        align-items: center;
        gap: 8px;
    }

    .toggle-label svg {
        width: 16px;
        height: 16px;
        transition: transform 0.3s ease;
    }

    .view-toggle-btn.general .toggle-label svg {
        transform: rotate(180deg);
    }

    .toggle-arrow {
        font-size: 10px;
        color: var(--text-tertiary);
    }

    .toggle-collapsed {
        padding: 16px;
        display: flex;
        align-items: center;
        justify-content: center;
        flex-shrink: 0;
    }

    .toggle-icon-btn {
        width: 32px;
        height: 32px;
        display: flex;
        align-items: center;
        justify-content: center;
        border-radius: 6px;
        background: var(--sidebar-bg);
        border: 1px solid var(--border);
        color: var(--text-secondary);
        cursor: pointer;
        transition: all 0.2s ease;
    }

    .toggle-icon-btn:hover {
        background: var(--hover-bg);
        color: var(--text-primary);
        border-color: var(--accent);
    }

    .toggle-icon-btn svg {
        width: 18px;
        height: 18px;
    }

    .job-list {
        display: flex;
        flex-direction: column;
        gap: 2px;
        margin: 0;
    }

    .job-item {
        padding: 10px 12px 10px 16px;
        border-radius: 8px;
        cursor: pointer;
        transition: all 0.2s ease;
        position: relative;
    }

    .job-item:hover {
        background: var(--hover-bg);
    }

    .job-item.selected {
        background: var(--selected-bg);
    }

    .job-item.grayed {
        opacity: 0.4;
        pointer-events: none;
    }

    .job-item.hidden {
        display: none;
    }

    .job-item::before {
        content: '';
        position: absolute;
        left: 0;
        top: 8px;
        bottom: 8px;
        width: 3px;
        border-radius: 0 2px 2px 0;
        background: transparent;
        transition: background 0.2s ease;
    }

    .job-item.selected::before { background: var(--accent); }
    .job-item.active::before { background: #4ade80; }
    .job-item.completed::before { background: #38bdf8; }
    .job-item.failed::before { background: #f87171; }

    .job-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: 4px;
    }

    .job-name {
        font-size: 13px;
        font-weight: 500;
        color: var(--text-primary);
        display: flex;
        align-items: center;
        gap: 8px;
    }

    .job-status-icon {
        width: 12px;
        height: 12px;
        flex-shrink: 0;
    }

    .job-status-icon svg {
        width: 100%;
        height: 100%;
    }

    .job-duration {
        font-size: 11px;
        color: var(--text-tertiary);
        font-family: 'JetBrains Mono', monospace;
    }

    .job-chat {
        font-size: 12px;
        color: var(--text-tertiary);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        margin-bottom: 4px;
    }

    .job-tags {
        display: flex;
        flex-wrap: wrap;
        gap: 4px;
        margin-top: 4px;
    }

    .job-tag {
        font-size: 10px;
        padding: 2px 6px;
        border-radius: 4px;
        background: rgba(198, 97, 67, 0.15);
        color: var(--accent);
        font-family: 'JetBrains Mono', monospace;
    }

    .empty-sidebar {
        padding: 24px 0;
        text-align: center;
        color: var(--text-tertiary);
        font-size: 13px;
    }

    .footer-collapsed {
        display: flex;
        flex-direction: column;
        gap: 8px;
        padding: 16px;
        border-top: 1px solid var(--border);
        align-items: center;
        font-family: 'JetBrains Mono', monospace;
        font-size: 11px;
        flex-shrink: 0;
        margin-top: auto;
    }

    .footer-stat-item {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 2px;
    }

    .footer-stat-value {
        font-size: 16px;
        font-weight: 600;
    }

    .footer-stat-item.active .footer-stat-value { color: #4ade80; }
    .footer-stat-item.completed .footer-stat-value { color: #38bdf8; }
    .footer-stat-item.failed .footer-stat-value { color: #f87171; }

    .footer-stat-label {
        font-size: 9px;
        color: var(--text-tertiary);
        text-transform: uppercase;
        letter-spacing: 0.5px;
    }

    .main-content {
        flex: 1;
        display: flex;
        flex-direction: column;
        min-width: 0;
        position: relative;
    }

    .topbar {
        height: 64px;
        border-bottom: 1px solid var(--border);
        display: flex;
        align-items: center;
        padding: 0 20px;
        gap: 12px;
        background: var(--bg);
    }

    .topbar-title {
        font-size: 15px;
        font-weight: 600;
        color: var(--text-primary);
    }

    .topbar-title .app-name {
        color: var(--accent);
        margin-right: 8px;
    }

    .terminal-container {
        flex: 1;
        overflow: hidden;
        display: flex;
        flex-direction: column;
    }

    .terminal {
        font-family: 'JetBrains Mono', monospace;
        font-size: 13px;
        line-height: 1.5;
        white-space: pre-wrap;
        word-break: break-word;
        color: var(--text-primary);
        padding: 16px;
        overflow-y: auto;
        flex: 1;
    }

    .terminal::-webkit-scrollbar { width: 8px; }
    .terminal::-webkit-scrollbar-track { background: transparent; }
    .terminal::-webkit-scrollbar-thumb { background: #333; border-radius: 4px; }
    .terminal::-webkit-scrollbar-thumb:hover { background: #444; }

    .countdown-line { 
        color: #eab308; 
        margin-top: 8px;
        font-weight: 500;
    }

    .empty-state {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        height: 100%;
        color: var(--text-tertiary);
        gap: 8px;
    }

    .empty-state-text { 
        font-size: 14px;
        font-weight: 500;
    }
    
    .empty-state-subtext { 
        font-size: 12px; 
        opacity: 0.7; 
    }

    .ansi-30{color:#000;}.ansi-31{color:#f87171;}.ansi-32{color:#4ade80;}
    .ansi-33{color:#eab308;}.ansi-34{color:#38bdf8;}.ansi-35{color:#e879f9;}
    .ansi-36{color:#22d3ee;}.ansi-37{color:#fff;}.ansi-90{color:#888;}
    .ansi-1{font-weight:700;}

    @media (max-width: 768px) {
        .sidebar {
            position: absolute;
            left: 0;
            top: 0;
            bottom: 0;
            z-index: 100;
        }
        
        .sidebar.collapsed {
            transform: translateX(-100%);
        }

        .collapse-btn {
            left: 8px;
            top: 8px;
        }
    }
</style>
</head>
<body>
<div class="app-container">
    <button class="collapse-btn" id="collapse-sidebar" title="Toggle sidebar">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
            <rect x="3" y="3" width="18" height="18" rx="2"/>
            <line x1="9" y1="3" x2="9" y2="21"/>
        </svg>
    </button>

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

            <div class="search-container">
                <input 
                    type="text" 
                    class="search-input" 
                    id="search-input" 
                    placeholder="Search tasks..."
                />
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
(() => {
    const API = '/tui/api/data';
    const POLL_MS = 1000;
    const STORAGE_KEY = 'marbot:lastSelectedJob';
    const SIDEBAR_COLLAPSED_KEY = 'marbot:sidebarCollapsed';
    const VIEW_KEY = 'marbot:currentView';

    let currentView = 'tasks';
    let selectedJobId = null;
    let allJobs = [];
    let generalLog = [];
    let clientSideCountdowns = {};
    let jobStartTimes = {};
    let isConnected = true;
    let searchQuery = '';

    const jobSortOrder = {};
    let nextSortIndex = 0;

    const jobDetailHtmlCache = {};
    const jobDetailSig = {};
    let generalHtmlCache = { sig: null, html: null };

    const sidebar = document.getElementById('sidebar');
    const collapseBtn = document.getElementById('collapse-sidebar');
    const resizeHandle = document.getElementById('resize-handle');
    const viewToggleBtn = document.getElementById('view-toggle-btn');
    const toggleIconBtn = document.getElementById('toggle-icon-btn');
    const toggleText = document.getElementById('toggle-text');
    const sidebarContent = document.getElementById('sidebar-content');
    const terminalContent = document.getElementById('terminal-content');
    const topbarSubtitle = document.getElementById('topbar-subtitle');
    const lastUpdateEl = document.getElementById('last-update');
    const searchInput = document.getElementById('search-input');

    let renderedJobIds = new Set();
    let isResizing = false;
    let resizeStartX = 0;
    let resizeStartWidth = 0;

    // Initialize
    try {
        const collapsed = localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === 'true';
        if (collapsed) sidebar.classList.add('collapsed');
        const savedWidth = localStorage.getItem('marbot:sidebarWidth');
        if (savedWidth && !collapsed) sidebar.style.width = savedWidth + 'px';
        const savedView = localStorage.getItem(VIEW_KEY);
        if (savedView === 'general') currentView = 'general';
    } catch (e) {}

    // Search input handler
    searchInput.addEventListener('input', (e) => {
        searchQuery = e.target.value.toLowerCase();
        filterJobs();
    });

    // Filter jobs based on search query
    function filterJobs() {
        if (currentView !== 'tasks') return;
        
        const jobList = sidebarContent.querySelector('.job-list');
        if (!jobList) return;

        const items = jobList.querySelectorAll('.job-item');
        
        items.forEach(item => {
            const jobId = item.dataset.jobId;
            const job = allJobs.find(j => j.id === jobId);
            
            if (!job) {
                item.classList.add('hidden');
                return;
            }

            if (!searchQuery) {
                item.classList.remove('hidden');
                return;
            }

            // Search in: tags, message_body, quoted_message, sender, chat_id
            const searchableText = [
                ...(job.tags || []),
                job.message_body || '',
                job.quoted_message || '',
                job.sender || '',
                job.chat_id || ''
            ].join(' ').toLowerCase();

            if (searchableText.includes(searchQuery)) {
                item.classList.remove('hidden');
            } else {
                item.classList.add('hidden');
            }
        });
    }

    // Instant collapse/expand - no delays, handles mid-transition clicks
    collapseBtn.addEventListener('click', () => {
        const wasCollapsed = sidebar.classList.contains('collapsed');
        
        if (wasCollapsed) {
            // Expanding
            const savedWidth = localStorage.getItem('marbot:sidebarWidth') || '260px';
            sidebar.style.width = savedWidth;
            sidebar.classList.remove('collapsed');
        } else {
            // Collapsing
            sidebar.classList.add('collapsed');
        }
        
        try {
            localStorage.setItem(SIDEBAR_COLLAPSED_KEY, !wasCollapsed);
        } catch (e) {}
    });

    // Resize
    resizeHandle.addEventListener('mousedown', (e) => {
        isResizing = true;
        resizeStartX = e.clientX;
        resizeStartWidth = sidebar.offsetWidth;
        sidebar.classList.add('resizing');
        resizeHandle.classList.add('active');
        document.body.style.cursor = 'col-resize';
        document.body.style.userSelect = 'none';
        e.preventDefault();
    });

    document.addEventListener('mousemove', (e) => {
        if (!isResizing) return;
        const delta = e.clientX - resizeStartX;
        const newWidth = Math.max(200, Math.min(600, resizeStartWidth + delta));
        sidebar.style.width = newWidth + 'px';
    });

    document.addEventListener('mouseup', () => {
        if (isResizing) {
            isResizing = false;
            sidebar.classList.remove('resizing');
            resizeHandle.classList.remove('active');
            document.body.style.cursor = '';
            document.body.style.userSelect = '';
            try {
                localStorage.setItem('marbot:sidebarWidth', sidebar.offsetWidth);
            } catch (e) {}
        }
    });

    function getStatusIcon(status) {
        const icons = {
            active: isConnected ? `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <circle cx="12" cy="12" r="10"/>
                <polyline points="12 6 12 12 16 14"/>
            </svg>` : `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <rect x="6" y="4" width="4" height="16"/>
                <rect x="14" y="4" width="4" height="16"/>
            </svg>`,
            completed: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <polyline points="20 6 9 17 4 12"/>
            </svg>`,
            failed: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <line x1="18" y1="6" x2="6" y2="18"/>
                <line x1="6" y1="6" x2="18" y2="18"/>
            </svg>`
        };
        return icons[status] || `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="10"/>
            <line x1="12" y1="8" x2="12" y2="12"/>
            <line x1="12" y1="16" x2="12.01" y2="16"/>
        </svg>`;
    }

    function switchView(view) {
        currentView = view;
        
        const toggleLabelSvg = viewToggleBtn.querySelector('.toggle-label svg');
        
        if (view === 'tasks') {
            viewToggleBtn.classList.remove('general');
            toggleText.textContent = 'Tasks';
            topbarSubtitle.textContent = 'Task Logs';
            toggleLabelSvg.innerHTML = `<rect x="3" y="3" width="7" height="7" rx="1"/>
                <rect x="3" y="14" width="7" height="7" rx="1"/>
                <rect x="14" y="3" width="7" height="7" rx="1"/>
                <rect x="14" y="14" width="7" height="7" rx="1"/>`;
            toggleIconBtn.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <rect x="3" y="3" width="7" height="7" rx="1"/>
                <rect x="3" y="14" width="7" height="7" rx="1"/>
                <rect x="14" y="3" width="7" height="7" rx="1"/>
                <rect x="14" y="14" width="7" height="7" rx="1"/>
            </svg>`;
            try {
                const cached = localStorage.getItem(STORAGE_KEY);
                if (cached) selectedJobId = cached;
            } catch (e) {}
        } else {
            viewToggleBtn.classList.add('general');
            toggleText.textContent = 'General';
            topbarSubtitle.textContent = 'General Logs';
            toggleLabelSvg.innerHTML = `<line x1="3" y1="12" x2="21" y2="12"/>
                <line x1="3" y1="6" x2="21" y2="6"/>
                <line x1="3" y1="18" x2="21" y2="18"/>`;
            toggleIconBtn.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <line x1="3" y1="12" x2="21" y2="12"/>
                <line x1="3" y1="6" x2="21" y2="6"/>
                <line x1="3" y1="18" x2="21" y2="18"/>
            </svg>`;
        }

        try {
            localStorage.setItem(VIEW_KEY, view);
        } catch (e) {}

        renderView(true);
    }

    viewToggleBtn.addEventListener('click', () => {
        viewToggleBtn.classList.add('spinning');
        setTimeout(() => viewToggleBtn.classList.remove('spinning'), 300);
        switchView(currentView === 'tasks' ? 'general' : 'tasks');
    });

    toggleIconBtn.addEventListener('click', () => {
        toggleIconBtn.classList.add('spinning');
        setTimeout(() => toggleIconBtn.classList.remove('spinning'), 300);
        switchView(currentView === 'tasks' ? 'general' : 'tasks');
    });

    switchView(currentView);

    try {
        const stored = localStorage.getItem(STORAGE_KEY);
        if (stored) selectedJobId = stored;
    } catch (e) {}

    sidebarContent.addEventListener('click', (e) => {
        if (currentView !== 'tasks') return;
        const jobItem = e.target.closest('.job-item');
        if (!jobItem || jobItem.classList.contains('grayed')) return;
        const id = jobItem.dataset.jobId;
        if (id && id !== selectedJobId) {
            selectedJobId = id;
            try { localStorage.setItem(STORAGE_KEY, id); } catch (e) {}
            renderView(true);
        }
    });

    function escapeHtml(s) {
        if (!s) return '';
        return String(s).replace(/[&<>"']/g, (m) => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[m]));
    }

    function shorten(s, n=30) {
        if (!s) return '';
        return s.length > n ? s.slice(0, n-3) + '...' : s;
    }

    function ansiToHtml(text) {
        if (!text) return '';
        let escaped = String(text).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
        escaped = escaped.replace(/\x1b\[38;2;(\d+);(\d+);(\d+)m/g, (m,r,g,b) => `<span style="color: rgb(${r},${g},${b})">`);
        escaped = escaped.replace(/\x1b\[(\d+)m/g, (m,code) => code === '0' ? '</span>' : `<span class="ansi-${code}">`);
        return escaped.replace(/\n/g, '<br>');
    }

    function getClientDuration(jobId, status) {
        const rec = jobStartTimes[jobId];
        if (!rec) return '0s';
        if (status === 'active') {
            if (!isConnected) return rec.frozen || Math.floor((rec.frozenMs || 0) / 1000) + 's';
            const elapsed = Date.now() - (rec.start || rec);
            return Math.floor(elapsed / 1000) + 's';
        }
        return rec.finalDuration || '0s';
    }

    function extractTimestampFromLog(line) {
        if (!line) return null;
        let m = line.match(/\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z/);
        if (m) {
            const t = Date.parse(m[0]);
            if (!isNaN(t)) return t;
        }
        return null;
    }

    function getJobLatestMs(job) {
        if (job.last_message_ms) {
            const n = Number(job.last_message_ms);
            if (!isNaN(n) && n > 0) return n;
        }
        const logs = job.logs || [];
        if (logs.length) {
            const last = logs[logs.length - 1];
            const parsed = extractTimestampFromLog(last);
            if (parsed) return parsed;
        }
        return Date.now() - (Number(job.duration_ms) || 0);
    }

    function jobSignature(job) {
        const logsLen = job.logs ? job.logs.length : 0;
        const trying = job.current_trying || '';
        const lastMs = getJobLatestMs(job);
        return `${job.id}:${logsLen}:${trying}:${job.duration_ms}:${lastMs}`;
    }

    function renderSidebar() {
        if (allJobs.length === 0) {
            if (sidebarContent.querySelector('.empty-sidebar')) return;
            sidebarContent.innerHTML = '<div class="empty-sidebar">No tasks</div>';
            renderedJobIds.clear();
            return;
        }

        allJobs.forEach(job => {
            if (jobSortOrder[job.id] === undefined) {
                jobSortOrder[job.id] = nextSortIndex++;
            }
        });

        const sorted = [...allJobs].sort((a, b) => {
            return jobSortOrder[b.id] - jobSortOrder[a.id];
        });

        const displayJobs = sorted.slice(0, 30);
        const currentJobIds = new Set(displayJobs.map(j => j.id));

        let jobList = sidebarContent.querySelector('.job-list');
        if (!jobList) {
            jobList = document.createElement('div');
            jobList.className = 'job-list';
            sidebarContent.innerHTML = '';
            sidebarContent.appendChild(jobList);
            renderedJobIds.clear();
        }

        const existingItems = jobList.querySelectorAll('.job-item');
        existingItems.forEach(item => {
            const id = item.dataset.jobId;
            if (!currentJobIds.has(id)) {
                item.remove();
                renderedJobIds.delete(id);
            }
        });

        displayJobs.forEach((job, index) => {
            let jobItem = jobList.querySelector(`[data-job-id="${job.id}"]`);
            
            if (!jobItem) {
                jobItem = document.createElement('div');
                jobItem.className = 'job-item';
                jobItem.dataset.jobId = job.id;
                
                const duration = getClientDuration(job.id, job.status);
                const grayed = currentView === 'general' ? 'grayed' : '';
                
                jobItem.classList.add(job.status);
                if (grayed) jobItem.classList.add(grayed);
                if (job.id === selectedJobId) jobItem.classList.add('selected');
                
                const tagsHtml = (job.tags && job.tags.length > 0) 
                    ? `<div class="job-tags">${job.tags.map(tag => `<span class="job-tag">${escapeHtml(tag)}</span>`).join('')}</div>`
                    : '';
                
                jobItem.innerHTML = `
                    <div class="job-row">
                        <div class="job-name">
                            <span class="job-status-icon">${getStatusIcon(job.status)}</span>
                            <span class="job-sender">${escapeHtml(job.sender)}</span>
                        </div>
                        <span class="job-duration" data-duration-id="${job.id}">${duration}</span>
                    </div>
                    <div class="job-chat">${escapeHtml(shorten(job.chat_id, 28))}</div>
                    ${tagsHtml}
                `;
                
                if (index === 0) {
                    jobList.insertBefore(jobItem, jobList.firstChild);
                } else {
                    const prevJob = displayJobs[index - 1];
                    const prevItem = jobList.querySelector(`[data-job-id="${prevJob.id}"]`);
                    if (prevItem && prevItem.nextSibling) {
                        jobList.insertBefore(jobItem, prevItem.nextSibling);
                    } else {
                        jobList.appendChild(jobItem);
                    }
                }
                
                renderedJobIds.add(job.id);
            } else {
                const durationEl = jobItem.querySelector('.job-duration');
                if (durationEl && job.status === 'active') {
                    const newDuration = getClientDuration(job.id, job.status);
                    if (durationEl.textContent !== newDuration) {
                        durationEl.textContent = newDuration;
                    }
                }
                
                const statusIcon = jobItem.querySelector('.job-status-icon');
                if (statusIcon) {
                    statusIcon.innerHTML = getStatusIcon(job.status);
                }
                
                jobItem.className = 'job-item ' + job.status;
                if (currentView === 'general') jobItem.classList.add('grayed');
                if (job.id === selectedJobId) jobItem.classList.add('selected');
                
                const currentIndex = Array.from(jobList.children).indexOf(jobItem);
                if (currentIndex !== index) {
                    if (index === 0) {
                        jobList.insertBefore(jobItem, jobList.firstChild);
                    } else {
                        const prevJob = displayJobs[index - 1];
                        const prevItem = jobList.querySelector(`[data-job-id="${prevJob.id}"]`);
                        if (prevItem && prevItem.nextSibling) {
                            jobList.insertBefore(jobItem, prevItem.nextSibling);
                        } else {
                            jobList.appendChild(jobItem);
                        }
                    }
                }
            }
        });

        // Apply search filter after rendering
        filterJobs();
    }

    function updateClientSideCountdown(jobId, attempt, remaining) {
        clientSideCountdowns[jobId] = { attempt, remaining, lastUpdate: Date.now() };
    }

    function getClientSideCountdown(jobId) {
        const c = clientSideCountdowns[jobId];
        if (!c) return null;
        if (!isConnected) return { attempt: c.attempt, remaining: c.remaining };
        const elapsed = Math.floor((Date.now() - c.lastUpdate) / 1000);
        const rem = Math.max(0, c.remaining - elapsed);
        if (rem === 0) { delete clientSideCountdowns[jobId]; return null; }
        return { attempt: c.attempt, remaining: rem };
    }

    function renderJobDetail() {
        const job = allJobs.find(j => j.id === selectedJobId);
        if (!job) {
            terminalContent.innerHTML = `
                <div class="empty-state">
                    <div class="empty-state-text">Select a task to view details</div>
                </div>
            `;
            return;
        }

        const sel = window.getSelection();
        if (sel && sel.toString().length > 0) return;

        if (job.current_countdown) {
            updateClientSideCountdown(job.id, job.current_countdown.attempt, job.current_countdown.remaining);
        }
        const countdown = getClientSideCountdown(job.id);
        const countdownHtml = countdown ? `<div class="countdown-line">RETRY #${countdown.attempt} - Waiting ${countdown.remaining} seconds...</div>` : '';

        const sig = jobSignature(job);

        if (jobDetailHtmlCache[job.id] && jobDetailSig[job.id] === sig) {
            terminalContent.innerHTML = jobDetailHtmlCache[job.id];
            return;
        }

        const lines = [...job.logs];
        if (job.current_trying) lines.push(job.current_trying);
        const raw = lines.join('\n');

        try {
            const html = ansiToHtml(raw) + countdownHtml;
            jobDetailHtmlCache[job.id] = html;
            jobDetailSig[job.id] = sig;
            terminalContent.innerHTML = html;
        } catch (e) {
            const fallback = escapeHtml(raw).replace(/\n/g, '<br>') + countdownHtml;
            terminalContent.innerHTML = fallback;
        }

        const nearBottom = terminalContent.scrollHeight - terminalContent.scrollTop <= terminalContent.clientHeight + 20;
        if (nearBottom) terminalContent.scrollTop = terminalContent.scrollHeight;
    }

    function renderGeneralLog() {
        if (!generalLog || generalLog.length === 0) {
            terminalContent.innerHTML = `
                <div class="empty-state">
                    <div class="empty-state-text">No general logs</div>
                </div>
            `;
            return;
        }

        const genSig = generalLog.length + ':' + (generalLog[generalLog.length-1]?.message || '');
        if (generalHtmlCache.sig === genSig && generalHtmlCache.html) {
            terminalContent.innerHTML = generalHtmlCache.html;
            return;
        }

        const sel = window.getSelection();
        if (sel && sel.toString().length > 0) return;

        const lines = generalLog.map(l => `[#${l.job_id.replace('req_','')}] ${l.message}`);
        const raw = lines.join('\n');

        try {
            const html = ansiToHtml(raw);
            generalHtmlCache = { sig: genSig, html };
            terminalContent.innerHTML = html;
        } catch (e) {
            const fallback = escapeHtml(raw).replace(/\n/g, '<br>');
            terminalContent.innerHTML = fallback;
        }

        const nearBottom = terminalContent.scrollHeight - terminalContent.scrollTop <= terminalContent.clientHeight + 20;
        if (nearBottom) terminalContent.scrollTop = terminalContent.scrollHeight;
    }

    function renderView(force=false) {
        renderSidebar();
        if (currentView === 'tasks') {
            renderJobDetail();
        } else {
            renderGeneralLog();
        }
    }

    function processFetchedData(data) {
        const prevIds = new Set(allJobs.map(j => j.id));
        allJobs = data.jobs;
        generalLog = data.general_log;

        allJobs.forEach(job => {
            if (!jobStartTimes[job.id]) {
                jobStartTimes[job.id] = { start: Date.now() - job.duration_ms };
            }
            if ((job.status === 'completed' || job.status === 'failed') && !jobStartTimes[job.id].finalDuration) {
                jobStartTimes[job.id].finalDuration = Math.floor(job.duration_ms / 1000) + 's';
            }
        });

        document.getElementById('stat-active').textContent = data.stats.active;
        document.getElementById('stat-completed').textContent = data.stats.completed;
        document.getElementById('stat-failed').textContent = data.stats.failed;
        document.getElementById('stat-active-collapsed').textContent = data.stats.active;
        document.getElementById('stat-completed-collapsed').textContent = data.stats.completed;
        document.getElementById('stat-failed-collapsed').textContent = data.stats.failed;

        try {
            const cached = localStorage.getItem(STORAGE_KEY);
            if (cached && allJobs.find(j => j.id === cached)) {
                selectedJobId = cached;
            } else if (allJobs.length > 0 && !selectedJobId && currentView === 'tasks') {
                const newest = allJobs.reduce((latest, job) => {
                    if (!latest) return job;
                    return getJobLatestMs(job) > getJobLatestMs(latest) ? job : latest;
                }, null);
                if (newest && !prevIds.has(newest.id)) selectedJobId = newest.id;
            }
        } catch (e) {}

        allJobs.forEach(job => {
            const sig = jobSignature(job);
            if (jobDetailSig[job.id] && jobDetailSig[job.id] !== sig) {
                delete jobDetailHtmlCache[job.id];
                delete jobDetailSig[job.id];
            }
        });
    }

    async function fetchData() {
        try {
            const res = await fetch(API, { cache: 'no-store' });
            if (!res.ok) throw new Error('Network error');
            const data = await res.json();
            if (!isConnected) {
                isConnected = true;
                console.info('reconnected');
            }
            processFetchedData(data);
            renderView();
            const now = new Date();
            lastUpdateEl.textContent = now.toLocaleTimeString('en-US', { hour12:false });
        } catch (err) {
            if (isConnected) {
                isConnected = false;
                lastUpdateEl.textContent = 'OFFLINE';
            }
            console.error('fetch error', err);
        }
    }

    setInterval(() => {
        if (currentView === 'tasks' && selectedJobId) {
            renderSidebar();
            const job = allJobs.find(j => j.id === selectedJobId);
            if (job && (job.current_countdown || clientSideCountdowns[job.id])) {
                renderJobDetail();
            }
        }
    }, 1000);

    renderView(true);
    fetchData();
    setInterval(fetchData, POLL_MS);
})();
</script>
</body>
</html>
"#;