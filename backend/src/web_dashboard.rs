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
    // read the TUI state (async)
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

const DASHBOARD_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width,initial-scale=1" />
<title>MARBOT Dashboard</title>
<style>
    /* reset/base */
    * { box-sizing: border-box; margin: 0; padding: 0; }
    html, body { height: 100%; }
    body {
        background: #0a0a0a;
        color: #e0e0e0;
        font-family: 'JetBrains Mono', monospace;
        height: 100vh;
    }

    .container {
        display: grid;
        grid-template-rows: auto 1fr auto;
        gap: 10px;
        padding: 10px;
        height: 100vh;
    }

    .header {
        display:flex;
        justify-content:space-between;
        align-items:center;
        padding: 12px 16px;
        background: linear-gradient(135deg,#1a1a1a,#2a2a2a);
        border-radius: 8px;
        border: 1px solid #333;
    }

    .header h1 {
        font-size: 1.4rem;
        background: linear-gradient(90deg,#fff 0%,#c66143 100%);
        -webkit-background-clip: text;
        -webkit-text-fill-color: transparent;
        background-clip: text;
        font-weight: 700;
    }

    .stats { display:flex; gap:16px; align-items:center; }
    .stat { display:flex; gap:8px; align-items:center; }
    .stat-label { color:#888; font-size:0.9rem; }
    .stat-value { font-weight:700; font-size:1.05rem; }
    .stat-value.active { color:#4ade80; }
    .stat-value.completed { color:#38bdf8; }
    .stat-value.failed { color:#f87171; }

    .main-content {
        display:grid;
        grid-template-columns: 30% 70%;
        gap:10px;
        overflow: hidden; /* ensure outer container doesn't leak scrollbars */
    }

    /* Panel base - make sure each panel clips to its radius and creates its own compositing layer */
    .panel {
        background: #1a1a1a;
        border: 1px solid #333;
        border-radius: 8px;
        display:flex;
        flex-direction: column;
        overflow: hidden;               /* clip children & scrollbars */
        background-clip: padding-box;   /* prevent background/border bleed */
        -webkit-background-clip: padding-box;
        position: relative;
        transform: translateZ(0);       /* compositor layer hint (helps overlay-scrollbar clipping on some Linux setups) */
    }

    .panel-header {
        padding: 12px 14px;
        background: #252525;
        border-bottom: 1px solid #333;
        display:flex;
        justify-content:space-between;
        align-items:center;
        font-weight:700;
    }

    .panel .panel-content {
        padding: 12px;
        overflow: hidden; /* inner scroll containers control scrolling */
    }

    .tabs { display:flex; gap:10px; margin-bottom:10px; }
    .tab {
        padding: 8px 14px;
        background:#252525;
        border:1px solid #333;
        border-radius:6px;
        user-select:none;
        cursor:pointer;
        font-size:0.9rem;
    }
    .tab:hover { background:#2a2a2a; }
    .tab.active { background:#c66143; border-color:#c66143; color:white; }

    .job-item {
        padding:10px;
        margin-bottom:8px;
        background:#252525;
        border-radius:6px;
        border-left:3px solid #333;
        cursor:pointer;
        user-select:none;
        transition: background 120ms ease, border-color 120ms ease;
    }
    .job-item:hover:not(.grayed) { background:#2a2a2a; }
    .job-item.selected { background:#2a2a2a; border-left-color:#c66143; }
    .job-item.grayed { opacity:0.45; pointer-events:none; cursor:default; }

    .job-item.active { border-left-color: #4ade80; }
    .job-item.completed { border-left-color: #38bdf8; }
    .job-item.failed { border-left-color: #f87171; }

    .job-header { display:flex; align-items:center; justify-content:space-between; margin-bottom:6px; }
    .job-status { display:flex; align-items:center; gap:8px; font-size:0.95rem; }
    .job-chat { color:#888; font-size:0.9rem; }
    .job-duration { color:#666; min-width:48px; text-align:right; font-size:0.9rem; }

    /* Right panel specific: ensure scrollbar is clipped to rounded corner */
    .main-content .panel:last-child {
        border-radius: 8px;
        overflow: hidden; /* clip scrollbars & children to rounded shape */
    }

    /* The scrolling area inside the right panel */
    .main-content .panel:last-child .panel-content {
        padding: 12px;
        overflow-y: auto;          /* vertical scrolling */
        overflow-x: hidden;        /* prevent horizontal overflow that breaks radii */
        border-radius: 0 8px 8px 0;/* visually round only the right side */
        background: transparent;   /* let outer panel background show */
        -webkit-overflow-scrolling: touch;
        scrollbar-gutter: stable both-edges; /* reserve scrollbar space so overlay scrollbars stay inside */
        height: 100%;
    }

    .terminal-wrapper {
        background: #1a1a1a;
        overflow: hidden;
        border-radius: 6px;
    }

    .terminal {
        padding: 10px;
        font-family: 'JetBrains Mono', monospace;
        font-size: 0.85rem;
        line-height: 1.35;
        white-space: pre-wrap;
        word-break: break-word;
        color: #e0e0e0;
    }

    .countdown-line { color:#eab308; margin-top:6px; }

    .empty-state {
        display:flex;
        flex-direction:column;
        align-items:center;
        justify-content:center;
        height:100%;
        color:#666;
        gap:8px;
    }

    .footer {
        background:#1a1a1a;
        padding:10px 14px;
        border:1px solid #333;
        border-radius:8px;
        font-size:0.9rem;
        color:#888;
        display:flex;
        align-items:center;
    }

    /* ANSI classes */
    .ansi-30{color:#000;}
    .ansi-31{color:#f87171;}
    .ansi-32{color:#4ade80;}
    .ansi-33{color:#eab308;}
    .ansi-34{color:#38bdf8;}
    .ansi-35{color:#e879f9;}
    .ansi-36{color:#22d3ee;}
    .ansi-37{color:#fff;}
    .ansi-90{color:#888;}
    .ansi-1{font-weight:700;}

    /* Scrollbar styling (WebKit) */
    .main-content .panel .panel-content::-webkit-scrollbar { width: 8px; }
    .main-content .panel .panel-content::-webkit-scrollbar-track { background: transparent; border-radius: 8px; }
    .main-content .panel .panel-content::-webkit-scrollbar-thumb { background: #333; border-radius: 6px; }
    .main-content .panel .panel-content::-webkit-scrollbar-thumb:hover { background: #444; }

    /* Firefox */
    .main-content .panel .panel-content { scrollbar-width: thin; scrollbar-color: #333 #1a1a1a; }

    @keyframes spin { from { transform: rotate(0deg); } to { transform: rotate(360deg); } }
    .spinner { display:inline-block; animation: spin 1000ms linear infinite; }
    .spinner.paused { animation-play-state: paused; }

    @media (max-width: 900px) {
        .main-content { grid-template-columns: 1fr; }
    }
</style>
</head>
<body>
<div class="container">
    <div class="header">
        <h1>MARBOT Dashboard</h1>
        <div class="stats">
            <div class="stat"><span class="stat-label">Active:</span><span class="stat-value active" id="stat-active">0</span></div>
            <div class="stat"><span class="stat-label">Completed:</span><span class="stat-value completed" id="stat-completed">0</span></div>
            <div class="stat"><span class="stat-label">Failed:</span><span class="stat-value failed" id="stat-failed">0</span></div>
        </div>
    </div>

    <div class="main-content">
        <div class="panel">
            <div class="panel-header"><span>Navigation</span></div>
            <div class="panel-content">
                <div class="tabs">
                    <div class="tab active" data-view="tasks">Tasks</div>
                    <div class="tab" data-view="general">General</div>
                </div>
                <div id="task-list"></div>
            </div>
        </div>

        <div class="panel">
            <div class="panel-header"><span id="right-panel-title">Task Details</span></div>
            <div class="panel-content" id="detail-content">
                <div class="empty-state">
                    <div style="font-size:2rem;">📋</div>
                    <div>No tasks yet</div>
                    <div style="font-size:0.9rem;">Tasks will appear when jobs start</div>
                </div>
            </div>
        </div>
    </div>

    <div class="footer">
        <span>Auto-refresh: <strong>LIVE</strong></span>
        <span style="margin-left:20px;">Last updated: <strong id="last-update">-</strong></span>
    </div>
</div>

<script>
(() => {
    const API = '/tui/api/data';
    const POLL_MS = 1000;
    let currentView = 'tasks';
    let selectedJobId = null;
    let allJobs = [];
    let generalLog = [];
    let clientSideCountdowns = {};
    let jobStartTimes = {};
    let lastJobStructure = '';

    const tabs = document.querySelectorAll('.tab');
    const taskListEl = document.getElementById('task-list');
    const detailEl = document.getElementById('detail-content');
    const rightTitle = document.getElementById('right-panel-title');

    tabs.forEach(tab => {
        tab.addEventListener('click', () => {
            tabs.forEach(t => t.classList.remove('active'));
            tab.classList.add('active');
            currentView = tab.dataset.view;
            if (currentView === 'general') selectedJobId = null;
            renderView(true);
        });
    });

    taskListEl.addEventListener('click', (e) => {
        if (currentView !== 'tasks') return;
        const jobItem = e.target.closest('.job-item');
        if (!jobItem || jobItem.classList.contains('grayed')) return;
        const id = jobItem.dataset.jobId;
        if (id && id !== selectedJobId) {
            selectedJobId = id;
            renderView(true);
        }
    });

    function ansiToHtml(text) {
        if (!text) return '';
        // escape HTML first
        let escaped = String(text).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
        // apply 24-bit color (38;2;r;g;b)
        escaped = escaped.replace(/\x1b\[38;2;(\d+);(\d+);(\d+)m/g, (m,r,g,b) => `<span style="color: rgb(${r},${g},${b})">`);
        // apply SGR numeric codes (like 0,1,31..)
        escaped = escaped.replace(/\x1b\[(\d+)m/g, (m,code) => code === '0' ? '</span>' : `<span class="ansi-${code}">`);
        // replace newlines with <br>
        return escaped.replace(/\n/g, '<br>');
    }

    function getStatusIcon(status) {
        const isPaused = currentView === 'general';
        const spinnerClass = isPaused ? 'spinner paused' : 'spinner';
        switch (status) {
            case 'active': return `<span class="${spinnerClass}">⚙</span>`;
            case 'completed': return '✓';
            case 'failed': return '✗';
            default: return '?';
        }
    }

    function getClientDuration(jobId, status) {
        const rec = jobStartTimes[jobId];
        if (!rec) return '0s';
        if (status === 'active') {
            const elapsed = Date.now() - (rec.start || rec);
            return Math.floor(elapsed / 1000) + 's';
        } else {
            return rec.finalDuration || '0s';
        }
    }

    function createJobStructureSignature(displayJobs) {
        return displayJobs.map(j => `${j.id}:${j.status}:${j.id===selectedJobId}`).join('|');
    }

    function renderTaskList(force=false) {
        if (!taskListEl) return;
        if (allJobs.length === 0) {
            taskListEl.innerHTML = '<div class="empty-state">No tasks</div>';
            lastJobStructure = '';
            return;
        }

        const sorted = [...allJobs].sort((a,b) => {
            const order = {active: 0, failed: 1, completed: 2};
            if (order[a.status] !== order[b.status]) return order[a.status] - order[b.status];
            return b.id.localeCompare(a.id);
        });

        const displayJobs = sorted.slice(0, 50);
        const signature = createJobStructureSignature(displayJobs);

        if (!force && signature === lastJobStructure) {
            displayJobs.forEach(job => {
                const el = taskListEl.querySelector(`[data-duration-id="${job.id}"]`);
                if (el) {
                    const dur = getClientDuration(job.id, job.status);
                    if (el.textContent !== dur) el.textContent = dur;
                }
            });
            return;
        }

        const html = displayJobs.map(job => {
            const duration = getClientDuration(job.id, job.status);
            const selected = job.id === selectedJobId ? 'selected' : '';
            const grayed = currentView === 'general' ? 'grayed' : '';
            return `
                <div class="job-item ${job.status} ${selected} ${grayed}" data-job-id="${job.id}">
                    <div class="job-header">
                        <div class="job-status">
                            <span class="status-icon ${job.status}">${getStatusIcon(job.status)}</span>
                            <span>${escapeHtml(job.sender)}</span>
                        </div>
                        <span class="job-duration" data-duration-id="${job.id}">${duration}</span>
                    </div>
                    <div class="job-chat">${escapeHtml(shorten(job.chat_id,30))}</div>
                </div>
            `;
        }).join('');
        taskListEl.innerHTML = html;
        lastJobStructure = signature;
    }

    function updateClientSideCountdown(jobId, attempt, remaining) {
        clientSideCountdowns[jobId] = { attempt, remaining, lastUpdate: Date.now() };
    }
    function getClientSideCountdown(jobId) {
        const c = clientSideCountdowns[jobId];
        if (!c) return null;
        const elapsed = Math.floor((Date.now() - c.lastUpdate) / 1000);
        const rem = Math.max(0, c.remaining - elapsed);
        if (rem === 0) { delete clientSideCountdowns[jobId]; return null; }
        return { attempt: c.attempt, remaining: rem };
    }

    function renderJobDetail() {
        const job = allJobs.find(j => j.id === selectedJobId);
        if (!detailEl) return;
        if (!job) {
            detailEl.innerHTML = '<div class="empty-state"><div style="font-size:2rem;">📋</div><div>Select a task to view details</div></div>';
            return;
        }

        const sel = window.getSelection();
        if (sel && sel.toString().length > 0) return;

        if (job.current_countdown) {
            updateClientSideCountdown(job.id, job.current_countdown.attempt, job.current_countdown.remaining);
        }
        const countdown = getClientSideCountdown(job.id);

        const lines = [...job.logs];
        if (job.current_trying) lines.push(job.current_trying);

        const raw = lines.join('\n');
        const html = ansiToHtml(raw);
        const countdownHtml = countdown ? `<div class="countdown-line">│ ⏳ RETRY #${countdown.attempt} - Waiting ${countdown.remaining} seconds...</div>` : '';

        detailEl.innerHTML = `<div class="terminal-wrapper"><div class="terminal">${html}${countdownHtml}</div></div>`;

        const nearBottom = detailEl.scrollHeight - detailEl.scrollTop <= detailEl.clientHeight + 20;
        if (nearBottom) detailEl.scrollTop = detailEl.scrollHeight;
    }

    function renderGeneralLog() {
        if (!detailEl) return;
        if (!generalLog || generalLog.length === 0) {
            detailEl.innerHTML = '<div class="empty-state"><div style="font-size:2rem;">📋</div><div>No general logs</div></div>';
            return;
        }
        const sel = window.getSelection();
        if (sel && sel.toString().length > 0) return;

        const lines = generalLog.map(l => `[#${l.job_id.replace('req_','')}] ${l.message}`);
        const html = ansiToHtml(lines.join('\n'));
        detailEl.innerHTML = `<div class="terminal-wrapper"><div class="terminal">${html}</div></div>`;

        const nearBottom = detailEl.scrollHeight - detailEl.scrollTop <= detailEl.clientHeight + 20;
        if (nearBottom) detailEl.scrollTop = detailEl.scrollHeight;
    }

    function renderView(force=false) {
        renderTaskList(force);
        if (currentView === 'tasks') {
            rightTitle.textContent = 'Task Logs';
            renderJobDetail();
        } else {
            rightTitle.textContent = 'General Logs';
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

        if (allJobs.length > 0 && !selectedJobId && currentView === 'tasks') {
            const newest = allJobs.reduce((latest, job) => !latest || job.id > latest.id ? job : latest, null);
            if (newest && !prevIds.has(newest.id)) selectedJobId = newest.id;
            else if (!selectedJobId) selectedJobId = newest ? newest.id : null;
        }
    }

    async function fetchData() {
        try {
            const res = await fetch(API, { cache: 'no-store' });
            if (!res.ok) throw new Error('Network error');
            const data = await res.json();
            processFetchedData(data);
            renderView();
            const now = new Date();
            document.getElementById('last-update').textContent = now.toLocaleTimeString('en-US', { hour12:false });
        } catch (err) {
            console.error('fetchData error', err);
        }
    }

    setInterval(() => {
        if (currentView === 'tasks') {
            renderTaskList();
            if (selectedJobId) {
                const job = allJobs.find(j => j.id === selectedJobId);
                if (job && (job.current_countdown || clientSideCountdowns[job.id])) {
                    renderJobDetail();
                }
            }
        }
    }, 1000);

    fetchData();
    setInterval(fetchData, POLL_MS);

    /* helpers */
    function escapeHtml(s) {
        if (!s) return '';
        return String(s).replace(/[&<>"']/g, (m) => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[m]));
    }
    function shorten(s, n=30) {
        if (!s) return '';
        return s.length > n ? s.slice(0,n) + '...' : s;
    }
})();
</script>
</body>
</html>
"#;
