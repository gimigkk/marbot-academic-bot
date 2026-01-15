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

const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>MARBOT Dashboard</title>
    <style>
        * {
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }

        body {
            font-family: 'JetBrains Mono', monospace;
            background: #0a0a0a;
            color: #e0e0e0;
            height: 100vh;
            overflow: hidden;
        }

        .container {
            display: grid;
            grid-template-rows: auto 1fr auto;
            height: 100vh;
            padding: 10px;
            gap: 10px;
        }

        .header {
            background: linear-gradient(135deg, #1a1a1a 0%, #2a2a2a 100%);
            padding: 15px 20px;
            border-radius: 8px;
            border: 1px solid #333;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }

        .header h1 {
            font-size: 1.5em;
            font-family: 'JetBrains Mono', monospace;
            background: linear-gradient(90deg, #fff 0%, #c66143 100%);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
            background-clip: text;
            font-weight: bold;
        }

        .stats {
            display: flex;
            gap: 20px;
        }

        .stat {
            display: flex;
            align-items: center;
            gap: 8px;
        }

        .stat-label {
            color: #888;
            font-size: 0.9em;
        }

        .stat-value {
            font-weight: bold;
            font-size: 1.1em;
        }

        .stat-value.active { color: #4ade80; }
        .stat-value.completed { color: #38bdf8; }
        .stat-value.failed { color: #f87171; }

        .main-content {
            display: grid;
            grid-template-columns: 30% 70%;
            gap: 10px;
            overflow: hidden;
        }

        .panel {
            background: #1a1a1a;
            border: 1px solid #333;
            border-radius: 8px;
            display: flex;
            flex-direction: column;
            overflow: hidden;
        }

        .panel-header {
            padding: 12px 15px;
            background: #252525;
            border-bottom: 1px solid #333;
            font-weight: bold;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }

        .panel-content {
            flex: 1;
            overflow-y: auto;
            padding: 10px;
        }

        .tabs {
            display: flex;
            gap: 10px;
            margin-bottom: 10px;
        }

        .tab {
            padding: 8px 16px;
            background: #252525;
            border: 1px solid #333;
            border-radius: 6px;
            cursor: pointer;
            transition: background 0.2s;
            font-size: 0.9em;
            user-select: none;
        }

        .tab:hover {
            background: #2a2a2a;
        }

        .tab.active {
            background: #c66143;
            border-color: #c66143;
            color: white;
        }

        .job-item {
            padding: 10px;
            margin-bottom: 8px;
            background: #252525;
            border-radius: 6px;
            border-left: 3px solid #333;
            cursor: pointer;
            transition: background 0.15s ease, border-color 0.15s ease;
            user-select: none;
        }

        .job-item:hover:not(.grayed) {
            background: #2a2a2a;
        }

        .job-item.selected {
            background: #2a2a2a;
            border-left-color: #c66143;
        }
        
        .job-item.grayed {
            opacity: 0.4;
            cursor: default;
            pointer-events: none;
        }

        .job-item.active { border-left-color: #4ade80; }
        .job-item.completed { border-left-color: #38bdf8; }
        .job-item.failed { border-left-color: #f87171; }

        .job-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 5px;
        }

        .job-status {
            display: flex;
            align-items: center;
            gap: 5px;
            font-size: 0.9em;
        }

        .status-icon.active { color: #4ade80; }
        .status-icon.completed { color: #38bdf8; }
        .status-icon.failed { color: #f87171; }

        .job-chat {
            font-size: 0.9em;
            color: #888;
        }

        .job-duration {
            color: #666;
            font-size: 0.85em;
            min-width: 40px;
            text-align: right;
        }

        /* Terminal styling */
        .terminal-wrapper {
            background: #1a1a1a;
            overflow: hidden;
        }

        .terminal {
            padding: 10px;
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.85em;
            line-height: 1.3;
            color: #e0e0e0;
            white-space: pre-wrap;
            word-break: break-word;
        }
        
        .countdown-line {
            color: #eab308;
            margin-top: 4px;
        }
        
        /* ANSI color classes */
        .ansi-30 { color: #000000; }
        .ansi-31 { color: #f87171; }
        .ansi-32 { color: #4ade80; }
        .ansi-33 { color: #eab308; }
        .ansi-34 { color: #38bdf8; }
        .ansi-35 { color: #e879f9; }
        .ansi-36 { color: #22d3ee; }
        .ansi-37 { color: #ffffff; }
        .ansi-90 { color: #888888; }
        .ansi-1 { font-weight: bold; }

        .empty-state {
            display: flex;
            flex-direction: column;
            align-items: center;
            justify-content: center;
            height: 100%;
            color: #666;
            gap: 10px;
        }

        .footer {
            background: #1a1a1a;
            padding: 10px 15px;
            border: 1px solid #333;
            border-radius: 8px;
            font-size: 0.85em;
            color: #888;
        }

        .spinner {
            animation: spin 1s linear infinite;
            display: inline-block;
        }
        
        .spinner.paused {
            animation-play-state: paused;
        }

        @keyframes spin {
            from { transform: rotate(0deg); }
            to { transform: rotate(360deg); }
        }

        ::-webkit-scrollbar {
            width: 8px;
        }

        ::-webkit-scrollbar-track {
            background: #1a1a1a;
        }

        ::-webkit-scrollbar-thumb {
            background: #333;
            border-radius: 4px;
        }

        ::-webkit-scrollbar-thumb:hover {
            background: #444;
        }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>MARBOT Dashboard</h1>
            <div class="stats">
                <div class="stat">
                    <span class="stat-label">Active:</span>
                    <span class="stat-value active" id="stat-active">0</span>
                </div>
                <div class="stat">
                    <span class="stat-label">Completed:</span>
                    <span class="stat-value completed" id="stat-completed">0</span>
                </div>
                <div class="stat">
                    <span class="stat-label">Failed:</span>
                    <span class="stat-value failed" id="stat-failed">0</span>
                </div>
            </div>
        </div>

        <div class="main-content">
            <div class="panel">
                <div class="panel-header">
                    <span>Navigation</span>
                </div>
                <div class="panel-content">
                    <div class="tabs">
                        <div class="tab active" data-view="tasks">Tasks</div>
                        <div class="tab" data-view="general">General</div>
                    </div>
                    <div id="task-list"></div>
                </div>
            </div>

            <div class="panel">
                <div class="panel-header">
                    <span id="right-panel-title">Task Details</span>
                </div>
                <div class="panel-content" id="detail-content">
                    <div class="empty-state">
                        <div style="font-size: 2em;">📋</div>
                        <div>No tasks yet</div>
                        <div style="font-size: 0.9em;">Tasks will appear here when jobs start</div>
                    </div>
                </div>
            </div>
        </div>

        <div class="footer">
            <span>Auto-refresh: <strong>LIVE</strong></span>
            <span style="margin-left: 20px;">Last updated: <strong id="last-update">-</strong></span>
        </div>
    </div>

    <script>
        let currentView = 'tasks';
        let selectedJobId = null;
        let allJobs = [];
        let generalLog = [];
        let isRendering = false;
        let lastJobStructure = '';
        let clientSideCountdowns = {};
        let jobStartTimes = {};

        const tabs = document.querySelectorAll('.tab');
        tabs.forEach(tab => {
            tab.addEventListener('click', (e) => {
                e.stopPropagation();
                const newView = tab.dataset.view;
                if (newView === currentView) return;
                
                tabs.forEach(t => t.classList.remove('active'));
                tab.classList.add('active');
                currentView = newView;
                
                if (currentView === 'tasks') {
                    if (allJobs.length > 0) {
                        const activeJobs = allJobs.filter(j => j.status === 'active');
                        if (activeJobs.length > 0) {
                            const newestActive = activeJobs.reduce((latest, job) => {
                                return !latest || job.id > latest.id ? job : latest;
                            }, null);
                            selectedJobId = newestActive.id;
                        } else if (!selectedJobId) {
                            const newestJob = allJobs.reduce((latest, job) => {
                                return !latest || job.id > latest.id ? job : latest;
                            }, null);
                            selectedJobId = newestJob.id;
                        }
                    }
                } else if (currentView === 'general') {
                    selectedJobId = null;
                }
                
                renderView();
            });
        });

        document.getElementById('task-list').addEventListener('click', (e) => {
            if (currentView !== 'tasks') return;
            
            const jobItem = e.target.closest('.job-item');
            if (jobItem && !jobItem.classList.contains('grayed')) {
                const jobId = jobItem.dataset.jobId;
                if (jobId && jobId !== selectedJobId) {
                    selectedJobId = jobId;
                    renderView();
                }
            }
        });

        function isAtBottom(element) {
            return element.scrollHeight - element.scrollTop <= element.clientHeight + 10;
        }

        function hasSelection() {
            const selection = window.getSelection();
            return selection && selection.toString().length > 0;
        }

        function ansiToHtml(text) {
            let html = text;
            html = html.replace(/\x1b\[(\d+)m/g, (match, code) => {
                if (code === '0') return '</span>';
                return `<span class="ansi-${code}">`;
            });
            html = html.replace(/\x1b\[38;2;(\d+);(\d+);(\d+)m/g, (match, r, g, b) => {
                return `<span style="color: rgb(${r}, ${g}, ${b})">`;
            });
            return html;
        }

        function getStatusIcon(status) {
            const isPaused = currentView === 'general';
            const spinnerClass = isPaused ? 'spinner paused' : 'spinner';
            
            switch(status) {
                case 'active': return `<span class="${spinnerClass}">⚙</span>`;
                case 'completed': return '✓';
                case 'failed': return '✗';
                default: return '?';
            }
        }

        function getClientDuration(jobId, status) {
            if (!jobStartTimes[jobId]) return '0s';
            
            if (status === 'active') {
                const elapsed = Date.now() - jobStartTimes[jobId];
                return Math.floor(elapsed / 1000) + 's';
            } else {
                return jobStartTimes[jobId].finalDuration || '0s';
            }
        }

        function renderTaskList() {
            const container = document.getElementById('task-list');
            
            if (allJobs.length === 0) {
                container.innerHTML = '<div class="empty-state">No tasks</div>';
                lastJobStructure = '';
                return;
            }

            const sorted = [...allJobs].sort((a, b) => {
                const statusOrder = {active: 0, failed: 1, completed: 2};
                if (statusOrder[a.status] !== statusOrder[b.status]) {
                    return statusOrder[a.status] - statusOrder[b.status];
                }
                return b.id.localeCompare(a.id);
            });

            const isGeneral = currentView === 'general';
            const displayJobs = sorted.slice(0, 20);

            // Create structure signature (no durations)
            const structure = displayJobs.map(j => `${j.id}:${j.status}:${j.id === selectedJobId}`).join('|');
            
            // Only rebuild if structure changed
            if (structure !== lastJobStructure) {
                const html = displayJobs.map(job => {
                    const duration = getClientDuration(job.id, job.status);
                    
                    return `
                    <div class="job-item ${job.status} ${job.id === selectedJobId ? 'selected' : ''} ${isGeneral ? 'grayed' : ''}" 
                         data-job-id="${job.id}">
                        <div class="job-header">
                            <div class="job-status">
                                <span class="status-icon ${job.status}">${getStatusIcon(job.status)}</span>
                                <span>${job.sender}</span>
                            </div>
                            <span class="job-duration" data-duration-id="${job.id}">${duration}</span>
                        </div>
                        <div class="job-chat">${job.chat_id.substring(0, 30)}...</div>
                    </div>
                `;
                }).join('');

                container.innerHTML = html;
                lastJobStructure = structure;
            } else {
                // Just update durations without rebuilding
                displayJobs.forEach(job => {
                    const durationEl = container.querySelector(`[data-duration-id="${job.id}"]`);
                    if (durationEl) {
                        const newDuration = getClientDuration(job.id, job.status);
                        if (durationEl.textContent !== newDuration) {
                            durationEl.textContent = newDuration;
                        }
                    }
                });
            }
        }

        function updateClientSideCountdown(jobId, attempt, remaining) {
            if (!clientSideCountdowns[jobId]) {
                clientSideCountdowns[jobId] = { attempt, remaining, lastUpdate: Date.now() };
            } else {
                const countdown = clientSideCountdowns[jobId];
                countdown.attempt = attempt;
                countdown.remaining = remaining;
                countdown.lastUpdate = Date.now();
            }
        }

        function getClientSideCountdown(jobId) {
            const countdown = clientSideCountdowns[jobId];
            if (!countdown) return null;
            
            const elapsed = Math.floor((Date.now() - countdown.lastUpdate) / 1000);
            const remaining = Math.max(0, countdown.remaining - elapsed);
            
            if (remaining === 0) {
                delete clientSideCountdowns[jobId];
                return null;
            }
            
            return { attempt: countdown.attempt, remaining };
        }

        function renderJobDetail() {
            const job = allJobs.find(j => j.id === selectedJobId);
            const container = document.getElementById('detail-content');
            
            if (!job) {
                container.innerHTML = '<div class="empty-state"><div style="font-size: 2em;">📋</div><div>Select a task to view details</div></div>';
                return;
            }

            if (hasSelection()) {
                return;
            }

            const wasAtBottom = isAtBottom(container);

            let logs = [...job.logs];
            
            if (job.current_trying) {
                logs.push(job.current_trying);
            }

            const rawText = logs.join('\n');
            const htmlText = ansiToHtml(rawText);
            
            // Build countdown HTML separately
            let countdownHtml = '';
            if (job.current_countdown) {
                updateClientSideCountdown(job.id, job.current_countdown.attempt, job.current_countdown.remaining);
            }
            
            const countdown = getClientSideCountdown(job.id);
            if (countdown) {
                countdownHtml = `<div class="countdown-line">│ ⏳ RETRY #${countdown.attempt} - Waiting ${countdown.remaining} seconds...</div>`;
            }
            
            container.innerHTML = `<div class="terminal-wrapper"><div class="terminal">${htmlText}${countdownHtml}</div></div>`;

            if (wasAtBottom) {
                container.scrollTop = container.scrollHeight;
            }
        }

        function renderGeneralLog() {
            const container = document.getElementById('detail-content');
            
            if (generalLog.length === 0) {
                container.innerHTML = '<div class="empty-state"><div style="font-size: 2em;">📋</div><div>No general logs</div></div>';
                return;
            }

            if (hasSelection()) {
                return;
            }

            const wasAtBottom = isAtBottom(container);

            const logs = generalLog.map(log => {
                const jobNum = log.job_id.replace('req_', '');
                return `[#${jobNum}] ${log.message}`;
            });

            const rawText = logs.join('\n');
            const htmlText = ansiToHtml(rawText);
            
            container.innerHTML = `<div class="terminal-wrapper"><div class="terminal">${htmlText}</div></div>`;

            if (wasAtBottom) {
                container.scrollTop = container.scrollHeight;
            }
        }

        function renderView() {
            if (isRendering) return;
            isRendering = true;

            const rightTitle = document.getElementById('right-panel-title');
            
            if (currentView === 'tasks') {
                renderTaskList();
                rightTitle.textContent = 'Task Logs';
                renderJobDetail();
            } else {
                renderTaskList();
                rightTitle.textContent = 'General Logs';
                renderGeneralLog();
            }

            isRendering = false;
        }

        async function fetchData() {
            try {
                const response = await fetch('/tui/api/data');
                const data = await response.json();
                
                const previousJobIds = new Set(allJobs.map(j => j.id));
                
                allJobs = data.jobs;
                generalLog = data.general_log;
                
                // Track start times for new jobs
                allJobs.forEach(job => {
                    if (!jobStartTimes[job.id]) {
                        jobStartTimes[job.id] = Date.now() - job.duration_ms;
                    }
                    
                    // Store final duration for completed jobs
                    if ((job.status === 'completed' || job.status === 'failed') && !jobStartTimes[job.id].finalDuration) {
                        jobStartTimes[job.id] = {
                            start: jobStartTimes[job.id],
                            finalDuration: Math.floor(job.duration_ms / 1000) + 's'
                        };
                    }
                });
                
                document.getElementById('stat-active').textContent = data.stats.active;
                document.getElementById('stat-completed').textContent = data.stats.completed;
                document.getElementById('stat-failed').textContent = data.stats.failed;
                
                if (allJobs.length > 0 && !selectedJobId && currentView === 'tasks') {
                    const newestJob = allJobs.reduce((latest, job) => {
                        return !latest || job.id > latest.id ? job : latest;
                    }, null);
                    
                    if (newestJob && !previousJobIds.has(newestJob.id)) {
                        selectedJobId = newestJob.id;
                    } else if (!selectedJobId) {
                        selectedJobId = newestJob.id;
                    }
                }
                
                renderView();
                
                const now = new Date();
                document.getElementById('last-update').textContent = 
                    now.toLocaleTimeString('en-US', { hour12: false });
                
            } catch (err) {
                console.error('Failed to fetch data:', err);
            }
        }

        // Update countdown and durations every second
        setInterval(() => {
            if (currentView === 'tasks') {
                // Update task list durations
                renderTaskList();
                
                // Update detail countdown if visible
                if (selectedJobId) {
                    const job = allJobs.find(j => j.id === selectedJobId);
                    if (job && (job.current_countdown || clientSideCountdowns[job.id])) {
                        renderJobDetail();
                    }
                }
            }
        }, 1000);

        fetchData();
        setInterval(fetchData, 200);
    </script>
</body>
</html>"#;