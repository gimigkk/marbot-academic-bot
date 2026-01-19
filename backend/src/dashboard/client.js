// dashboard/client.js

(() => {
    const API = '/tui/api/data';
    const POLL_MS = 1000;
    const STORAGE_KEY = 'marbot:lastSelectedJob';
    const SIDEBAR_COLLAPSED_KEY = 'marbot:sidebarCollapsed';
    const VIEW_KEY = 'marbot:currentView';
    const SEARCH_EXPANDED_KEY = 'marbot:searchExpanded';
    const ANALYTICS_OPEN_KEY = 'marbot:analyticsOpen';
    const ANALYTICS_WIDTH_KEY = 'marbot:analyticsWidth';
    const SHOW_BARS_KEY = 'marbot:showBars';

    let currentView = 'tasks';
    let selectedJobId = null;
    let allJobs = [];
    let generalLog = [];
    let clientSideCountdowns = {};
    let jobStartTimes = {};
    let isConnected = true;
    let searchQuery = '';
    let searchExpanded = false;
    let analyticsOpen = false;
    let showBars = true;
    let analyticsChart = null;
    let lastJobCount = 0;
    let lastJobStates = new Map(); // Track job states for change detection

    const jobDetailHtmlCache = {};
    const jobDetailSig = {};
    let generalHtmlCache = { sig: null, html: null };

    const sidebar = document.getElementById('sidebar');
    const topControls = document.getElementById('top-controls');
    const collapseBtn = document.getElementById('collapse-sidebar');
    const resizeHandle = document.getElementById('resize-handle');
    const viewToggleBtn = document.getElementById('view-toggle-btn');
    const toggleIconBtn = document.getElementById('toggle-icon-btn');
    const toggleText = document.getElementById('toggle-text');
    const sidebarContent = document.getElementById('sidebar-content');
    const terminalContent = document.getElementById('terminal-content');
    const topbarSubtitle = document.getElementById('topbar-subtitle');
    const lastUpdateEl = document.getElementById('last-update');
    const searchContainer = document.getElementById('search-container');
    const searchToggle = document.getElementById('search-toggle');
    const searchInput = document.getElementById('search-input');
    const analyticsBtn = document.getElementById('analytics-btn');
    const analyticsPanel = document.getElementById('analytics-panel');
    const analyticsResizeHandle = document.getElementById('analytics-resize-handle');
    const analyticsClose = document.getElementById('analytics-close');
    const showBarsToggle = document.getElementById('show-bars-toggle');
    const analyticsChartCanvas = document.getElementById('analytics-chart');

    let renderedJobIds = new Set();
    let isResizing = false;
    let resizeStartX = 0;
    let resizeStartWidth = 0;
    let isAnalyticsResizing = false;
    let analyticsResizeStartX = 0;
    let analyticsResizeStartWidth = 0;

    // Initialize
    try {
        const collapsed = localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === 'true';
        if (collapsed) {
            sidebar.classList.add('collapsed');
            searchContainer.classList.add('hidden');
            analyticsBtn.classList.add('hidden');
            collapseBtn.classList.add('collapsed');
        }
        const savedWidth = localStorage.getItem('marbot:sidebarWidth');
        if (savedWidth && !collapsed) sidebar.style.width = savedWidth + 'px';
        
        const savedView = localStorage.getItem(VIEW_KEY);
        if (savedView === 'general') currentView = 'general';
        
        const savedSearchExpanded = localStorage.getItem(SEARCH_EXPANDED_KEY) === 'true';
        if (savedSearchExpanded && !collapsed) {
            searchExpanded = true;
            searchContainer.classList.add('expanded');
            updateSearchWidth();
            setTimeout(() => searchInput.focus(), 350);
        }

        const savedAnalyticsOpen = localStorage.getItem(ANALYTICS_OPEN_KEY) === 'true';
        if (savedAnalyticsOpen && !collapsed) {
            analyticsOpen = true;
            const savedAnalyticsWidth = localStorage.getItem(ANALYTICS_WIDTH_KEY);
            if (savedAnalyticsWidth) {
                analyticsPanel.style.width = savedAnalyticsWidth + 'px';
            }
            analyticsPanel.classList.add('open');
            analyticsBtn.classList.add('active');
            // Chart will be initialized on first data fetch
        }

        const savedShowBars = localStorage.getItem(SHOW_BARS_KEY);
        if (savedShowBars !== null) {
            showBars = savedShowBars === 'true';
            showBarsToggle.checked = showBars;
        }
    } catch (e) {}

    // Update search width based on current sidebar width
    function updateSearchWidth() {
        if (!searchExpanded) return;
        const sidebarWidth = sidebar.offsetWidth;
        const maxWidth = sidebarWidth - 112; // Account for collapse (32) + gap (8) + analytics (32) + gap (8) + margins (32)
        searchContainer.style.width = maxWidth + 'px';
    }

    // Search toggle functionality
    searchToggle.addEventListener('click', (e) => {
        e.stopPropagation();
        e.preventDefault();
        
        searchExpanded = !searchExpanded;
        
        if (searchExpanded) {
            searchContainer.classList.add('expanded');
            updateSearchWidth();
            setTimeout(() => {
                searchInput.focus();
            }, 350);
        } else {
            searchContainer.style.width = '32px';
            searchContainer.classList.remove('expanded');
            searchInput.value = '';
            searchQuery = '';
            filterJobs();
        }

        try {
            localStorage.setItem(SEARCH_EXPANDED_KEY, searchExpanded);
        } catch (e) {}
    });

    searchInput.addEventListener('click', (e) => {
        e.stopPropagation();
    });

    document.addEventListener('click', (e) => {
        if (searchExpanded && 
            !searchContainer.contains(e.target) && 
            !e.target.closest('.search-container')) {
            searchExpanded = false;
            searchContainer.style.width = '32px';
            searchContainer.classList.remove('expanded');
            searchInput.value = '';
            searchQuery = '';
            filterJobs();
            try {
                localStorage.setItem(SEARCH_EXPANDED_KEY, false);
            } catch (e) {}
        }
    });

    searchInput.addEventListener('input', (e) => {
        searchQuery = e.target.value.toLowerCase();
        filterJobs();
    });

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

            const searchableText = [
                job.id || '',
                job.sender || '',
                job.chat_id || '',
                job.message_body || '',
                job.quoted_message || '',
                ...(job.tags || []),
                job.status || ''
            ].join(' ').toLowerCase();

            if (searchableText.includes(searchQuery)) {
                item.classList.remove('hidden');
            } else {
                item.classList.add('hidden');
            }
        });
    }

    // Analytics toggle
    analyticsBtn.addEventListener('click', () => {
        analyticsOpen = !analyticsOpen;
        
        if (analyticsOpen) {
            const savedWidth = localStorage.getItem(ANALYTICS_WIDTH_KEY);
            const targetWidth = savedWidth || Math.max(300, Math.floor(document.getElementById('main-content').offsetWidth * 0.5));
            
            analyticsPanel.style.width = targetWidth + 'px';
            analyticsPanel.classList.add('open');
            analyticsBtn.classList.add('active');
            initializeChart();
            // Trigger first update when opening
            setTimeout(() => updateChart(), 100);
        } else {
            analyticsPanel.style.width = '0px';
            analyticsPanel.classList.remove('open');
            analyticsBtn.classList.remove('active');
            destroyChart();
        }

        try {
            localStorage.setItem(ANALYTICS_OPEN_KEY, analyticsOpen);
        } catch (e) {}
    });

    analyticsClose.addEventListener('click', () => {
        analyticsOpen = false;
        analyticsPanel.style.width = '0px';
        analyticsPanel.classList.remove('open');
        analyticsBtn.classList.remove('active');
        destroyChart();
        try {
            localStorage.setItem(ANALYTICS_OPEN_KEY, false);
        } catch (e) {}
    });

    // Analytics resize
    analyticsResizeHandle.addEventListener('mousedown', (e) => {
        isAnalyticsResizing = true;
        analyticsResizeStartX = e.clientX;
        analyticsResizeStartWidth = analyticsPanel.offsetWidth;
        analyticsPanel.classList.add('resizing');
        analyticsResizeHandle.classList.add('active');
        document.body.style.cursor = 'col-resize';
        document.body.style.userSelect = 'none';
        e.preventDefault();
    });

    document.addEventListener('mousemove', (e) => {
        if (isAnalyticsResizing) {
            const delta = e.clientX - analyticsResizeStartX;
            const newWidth = Math.max(250, Math.min(800, analyticsResizeStartWidth + delta));
            analyticsPanel.style.width = newWidth + 'px';
            if (analyticsChart) {
                analyticsChart.resize();
            }
        }
    });

    document.addEventListener('mouseup', () => {
        if (isAnalyticsResizing) {
            isAnalyticsResizing = false;
            analyticsPanel.classList.remove('resizing');
            analyticsResizeHandle.classList.remove('active');
            document.body.style.cursor = '';
            document.body.style.userSelect = '';
            try {
                localStorage.setItem(ANALYTICS_WIDTH_KEY, analyticsPanel.offsetWidth);
            } catch (e) {}
        }
    });

    // Show bars toggle
    showBarsToggle.addEventListener('change', (e) => {
        showBars = e.target.checked;
        try {
            localStorage.setItem(SHOW_BARS_KEY, showBars);
        } catch (e) {}
        updateChart();
    });

    // Collapse button - close analytics when collapsing sidebar
    collapseBtn.addEventListener('click', () => {
        const wasCollapsed = sidebar.classList.contains('collapsed');
        
        if (wasCollapsed) {
            const savedWidth = localStorage.getItem('marbot:sidebarWidth') || '260px';
            sidebar.style.width = savedWidth;
            sidebar.classList.remove('collapsed');
            searchContainer.classList.remove('hidden');
            analyticsBtn.classList.remove('hidden');
            collapseBtn.classList.remove('collapsed');
        } else {
            sidebar.classList.add('collapsed');
            searchContainer.classList.add('hidden');
            analyticsBtn.classList.add('hidden');
            collapseBtn.classList.add('collapsed');
            
            // Close analytics when collapsing sidebar
            if (analyticsOpen) {
                analyticsOpen = false;
                analyticsPanel.style.width = '0px';
                analyticsPanel.classList.remove('open');
                analyticsBtn.classList.remove('active');
                destroyChart();
                try {
                    localStorage.setItem(ANALYTICS_OPEN_KEY, false);
                } catch (e) {}
            }
            
            if (searchExpanded) {
                searchExpanded = false;
                searchContainer.style.width = '32px';
                searchContainer.classList.remove('expanded');
                searchInput.value = '';
                searchQuery = '';
                filterJobs();
                try {
                    localStorage.setItem(SEARCH_EXPANDED_KEY, false);
                } catch (e) {}
            }
        }
        
        try {
            localStorage.setItem(SIDEBAR_COLLAPSED_KEY, !wasCollapsed);
        } catch (e) {}
    });

    // Sidebar resize
    resizeHandle.addEventListener('mousedown', (e) => {
        isResizing = true;
        resizeStartX = e.clientX;
        resizeStartWidth = sidebar.offsetWidth;
        sidebar.classList.add('resizing');
        searchContainer.classList.add('resizing');
        resizeHandle.classList.add('active');
        document.body.style.cursor = 'col-resize';
        document.body.style.userSelect = 'none';
        e.preventDefault();
    });

    document.addEventListener('mousemove', (e) => {
        if (isResizing) {
            const delta = e.clientX - resizeStartX;
            const newWidth = Math.max(200, Math.min(600, resizeStartWidth + delta));
            sidebar.style.width = newWidth + 'px';
            updateSearchWidth();
        }
    });

    document.addEventListener('mouseup', () => {
        if (isResizing) {
            isResizing = false;
            sidebar.classList.remove('resizing');
            searchContainer.classList.remove('resizing');
            resizeHandle.classList.remove('active');
            document.body.style.cursor = '';
            document.body.style.userSelect = '';
            try {
                localStorage.setItem('marbot:sidebarWidth', sidebar.offsetWidth);
            } catch (e) {}
        }
    });

    // Analytics functions
    function categorizeJob(job) {
        const tags = (job.tags || []).map(t => t.toLowerCase().replace(/^#/, ''));
        
        // Bot commands - based on actual BotCommand enum variants from main.rs
        const botCommandTags = [
            'ping',      // BotCommand::Ping
            'tugas',     // BotCommand::Tugas
            'todo',      // BotCommand::Todo
            'today',     // BotCommand::Today
            'week',      // BotCommand::Week
            'help',      // BotCommand::Help
            'undo',      // BotCommand::Undo
            'done',      // BotCommand::Done
            'delete',    // BotCommand::Delete
            'expand',    // BotCommand::Expand
            'setkelas',  // BotCommand::SetKelas
            'mykelas',   // BotCommand::MyKelas
            'error',     // BotCommand::MissingArgument
            'unknown'    // BotCommand::UnknownCommand
        ];
        
        if (tags.some(tag => botCommandTags.includes(tag))) {
            return 'bot';
        }
        
        // AI processing - based on AIClassification enum variants from main.rs
        const aiTags = [
            'ai',                // MessageType::NeedsAI (initial tag)
            'assignment',        // AIClassification::AssignmentInfo
            'update',            // AIClassification::AssignmentUpdate
            'batch',             // AIClassification::MultipleAssignments
            'informal',          // AIClassification::Unrecognized (Informal)
            'academic-related'   // AIClassification::Unrecognized (AcademicRelated)
        ];
        
        if (tags.some(tag => aiTags.includes(tag))) {
            return 'ai';
        }
        
        // Unrecognized - shouldn't happen with proper tagging
        return 'unrecognized';
    }


    function getTimeBuckets(jobs) {
        if (jobs.length === 0) return [];

        const timestamps = jobs.map(j => getJobLatestMs(j)).filter(t => t > 0);
        if (timestamps.length === 0) return [];

        const minTime = Math.min(...timestamps);
        const maxTime = Math.max(...timestamps);
        const spanMs = maxTime - minTime;
        const spanHours = spanMs / (1000 * 60 * 60);

        // If span is less than 24 hours, use 12-hour buckets
        // Otherwise use daily buckets
        const bucketMs = spanHours < 24 ? 12 * 60 * 60 * 1000 : 24 * 60 * 60 * 1000;

        const buckets = [];
        let currentBucket = Math.floor(minTime / bucketMs) * bucketMs;
        
        while (currentBucket <= maxTime) {
            buckets.push(currentBucket);
            currentBucket += bucketMs;
        }

        return buckets;
    }

    function formatBucketLabel(timestamp, bucketMs) {
        const date = new Date(timestamp);
        const is12Hour = bucketMs === 12 * 60 * 60 * 1000;

        if (is12Hour) {
            const hours = date.getHours();
            const period = hours < 12 ? 'AM' : 'PM';
            const displayHour = hours === 0 ? 12 : (hours > 12 ? hours - 12 : hours);
            return `${date.getMonth() + 1}/${date.getDate()} ${displayHour}${period}`;
        } else {
            return `${date.getMonth() + 1}/${date.getDate()}`;
        }
    }

    function processAnalyticsData(jobs) {
        const buckets = getTimeBuckets(jobs);
        if (buckets.length === 0) {
            return {
                labels: [],
                totalJobs: [],
                botJobs: [],
                aiJobs: [],
                unrecognizedJobs: [],
                successJobs: [],
                failedJobs: []
            };
        }

        const bucketMs = buckets.length > 1 ? buckets[1] - buckets[0] : 24 * 60 * 60 * 1000;
        const labels = buckets.map(b => formatBucketLabel(b, bucketMs));
        
        const data = {
            labels,
            totalJobs: new Array(buckets.length).fill(0),
            botJobs: new Array(buckets.length).fill(0),
            aiJobs: new Array(buckets.length).fill(0),
            unrecognizedJobs: new Array(buckets.length).fill(0),
            successJobs: new Array(buckets.length).fill(0),
            failedJobs: new Array(buckets.length).fill(0)
        };

        jobs.forEach(job => {
            const jobTime = getJobLatestMs(job);
            if (jobTime <= 0) return;

            const bucketIndex = Math.floor((jobTime - buckets[0]) / bucketMs);
            if (bucketIndex < 0 || bucketIndex >= buckets.length) return;

            data.totalJobs[bucketIndex]++;

            const category = categorizeJob(job);
            if (category === 'bot') {
                data.botJobs[bucketIndex]++;
            } else if (category === 'ai') {
                data.aiJobs[bucketIndex]++;
            } else {
                data.unrecognizedJobs[bucketIndex]++;
            }

            if (job.status === 'completed') {
                data.successJobs[bucketIndex]++;
            } else if (job.status === 'failed') {
                data.failedJobs[bucketIndex]++;
            }
        });

        return data;
    }

    function initializeChart() {
        if (analyticsChart) {
            updateChart();
            return;
        }

        // Initialize tracking state
        lastJobCount = allJobs.length;
        lastJobStates.clear();
        allJobs.forEach(job => {
            lastJobStates.set(job.id, `${job.status}:${(job.tags || []).join(',')}`);
        });

        const ctx = analyticsChartCanvas.getContext('2d');
        
        analyticsChart = new Chart(ctx, {
            type: 'bar',
            data: {
                labels: [],
                datasets: []
            },
            options: {
                responsive: true,
                maintainAspectRatio: false,
                interaction: {
                    mode: 'index',
                    intersect: false
                },
                plugins: {
                    legend: {
                        labels: {
                            color: '#e0e0e0',
                            font: {
                                family: 'JetBrains Mono, monospace',
                                size: 11
                            },
                            usePointStyle: true,
                            pointStyle: 'circle',
                            boxWidth: 6,
                            boxHeight: 6,
                            padding: 12
                        }
                    },
                    tooltip: {
                        backgroundColor: 'rgba(26, 26, 26, 0.95)',
                        titleColor: '#e0e0e0',
                        bodyColor: '#e0e0e0',
                        borderColor: '#2a2a2a',
                        borderWidth: 1,
                        padding: 12,
                        displayColors: true,
                        titleFont: {
                            family: 'JetBrains Mono, monospace',
                            size: 12
                        },
                        bodyFont: {
                            family: 'JetBrains Mono, monospace',
                            size: 11
                        }
                    }
                },
                scales: {
                    x: {
                        grid: {
                            color: '#2a2a2a',
                            drawBorder: false
                        },
                        ticks: {
                            color: '#999',
                            font: {
                                family: 'JetBrains Mono, monospace',
                                size: 10
                            }
                        }
                    },
                    y: {
                        grace: '15%',  // Add 15% headroom to top
                        grid: {
                            color: '#2a2a2a',
                            drawBorder: false
                        },
                        ticks: {
                            color: '#999',
                            font: {
                                family: 'JetBrains Mono, monospace',
                                size: 10
                            },
                            precision: 0
                        }
                    }
                }
            }
        });

        updateChart();
    }

    function updateChart() {
        if (!analyticsChart) return;

        const data = processAnalyticsData(allJobs);
        
        // Preserve hidden state of existing datasets
        const hiddenStates = {};
        if (analyticsChart.data.datasets) {
            analyticsChart.data.datasets.forEach((dataset, index) => {
                hiddenStates[dataset.label] = analyticsChart.getDatasetMeta(index).hidden;
            });
        }
        
        const datasets = [];

        // Stacked bars (if enabled)
        if (showBars) {
            datasets.push({
                label: 'Success',
                data: data.successJobs,
                backgroundColor: 'rgba(74, 222, 128, 0.6)',
                borderColor: 'rgba(74, 222, 128, 1)',
                borderWidth: 1,
                type: 'bar',
                order: 2
            });

            datasets.push({
                label: 'Failed',
                data: data.failedJobs,
                backgroundColor: 'rgba(248, 113, 113, 0.6)',
                borderColor: 'rgba(248, 113, 113, 1)',
                borderWidth: 1,
                type: 'bar',
                order: 2
            });
        }

        // Line charts
        datasets.push({
            label: 'Total Jobs',
            data: data.totalJobs,
            borderColor: '#e0e0e0',
            backgroundColor: 'rgba(224, 224, 224, 0.1)',
            borderWidth: 1,
            tension: 0.3,
            type: 'line',
            order: 1,
            pointRadius: 3,
            pointHoverRadius: 5
        });

        datasets.push({
            label: 'Bot Commands',
            data: data.botJobs,
            borderColor: '#c66143',
            backgroundColor: 'rgba(198, 97, 67, 0.1)',
            borderWidth: 2,
            tension: 0.3,
            type: 'line',
            order: 1,
            pointRadius: 3,
            pointHoverRadius: 5
        });

        datasets.push({
            label: 'AI Processing',
            data: data.aiJobs,
            borderColor: '#38bdf8',
            backgroundColor: 'rgba(56, 189, 248, 0.1)',
            borderWidth: 2,
            tension: 0.3,
            type: 'line',
            order: 1,
            pointRadius: 3,
            pointHoverRadius: 5
        });

        datasets.push({
            label: 'Unrecognized',
            data: data.unrecognizedJobs,
            borderColor: '#e879f9',
            backgroundColor: 'rgba(232, 121, 249, 0.1)',
            borderWidth: 2,
            tension: 0.3,
            type: 'line',
            order: 1,
            pointRadius: 3,
            pointHoverRadius: 5
        });

        analyticsChart.data.labels = data.labels;
        analyticsChart.data.datasets = datasets;
        
        // Restore hidden state for datasets that existed before
        datasets.forEach((dataset, index) => {
            if (hiddenStates[dataset.label] !== undefined) {
                analyticsChart.getDatasetMeta(index).hidden = hiddenStates[dataset.label];
            }
        });
        
        analyticsChart.update('none');
    }

    function destroyChart() {
        if (analyticsChart) {
            analyticsChart.destroy();
            analyticsChart = null;
            lastJobCount = 0;
            lastJobStates.clear();
        }
    }

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
        return Date.now();
    }

    function formatTimestamp(ms) {
        const date = new Date(ms);
        const now = new Date();
        
        if (date.getDate() === now.getDate() && 
            date.getMonth() === now.getMonth() && 
            date.getFullYear() === now.getFullYear()) {
            return date.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false });
        }
        
        if (date.getFullYear() === now.getFullYear()) {
            const month = date.toLocaleString('en-US', { month: 'short' });
            const day = date.getDate();
            const time = date.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false });
            return `${month} ${day}, ${time}`;
        }
        
        const month = date.toLocaleString('en-US', { month: 'short' });
        const day = date.getDate();
        const year = date.getFullYear();
        const time = date.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false });
        return `${month} ${day} ${year}, ${time}`;
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

        const sorted = [...allJobs].sort((a, b) => {
            return getJobLatestMs(b) - getJobLatestMs(a);
        });

        const displayJobs = sorted.slice(0, 50);
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
                
                const timestamp = formatTimestamp(getJobLatestMs(job));
                
                const cleanTags = [...new Set(job.tags || [])].map(tag => tag.replace(/^#/, ''));
                const tagsHtml = cleanTags.length > 0
                    ? `<div class="job-tags">${cleanTags.map(tag => `<span class="job-tag">${escapeHtml(tag)}</span>`).join('')}</div>`
                    : '';
                
                jobItem.innerHTML = `
                    <div class="job-row">
                        <div class="job-name">
                            <span class="job-status-icon">${getStatusIcon(job.status)}</span>
                            <span class="job-sender">${escapeHtml(job.sender)}</span>
                        </div>
                        <span class="job-duration" data-duration-id="${job.id}">${duration}</span>
                    </div>
                    <div class="job-meta">
                        <span class="job-chat">${escapeHtml(shorten(job.chat_id, 22))}</span>
                        <span class="job-timestamp">${timestamp}</span>
                    </div>
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
                
                const timestampEl = jobItem.querySelector('.job-timestamp');
                if (timestampEl) {
                    const newTimestamp = formatTimestamp(getJobLatestMs(job));
                    if (timestampEl.textContent !== newTimestamp) {
                        timestampEl.textContent = newTimestamp;
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
        const countdownHtml = countdown ? `<div class="countdown-line">│ ⏳ RETRY #${countdown.attempt} - Waiting ${countdown.remaining} seconds...</div>` : '';
        
        const tryingHtml = job.current_trying ? `<div class="countdown-line">${escapeHtml(job.current_trying)}</div>` : '';

        const sig = jobSignature(job);

        if (jobDetailHtmlCache[job.id] && jobDetailSig[job.id] === sig) {
            terminalContent.innerHTML = jobDetailHtmlCache[job.id];
            return;
        }

        const lines = [...job.logs];
        const raw = lines.join('\n');

        try {
            const html = ansiToHtml(raw) + tryingHtml + countdownHtml;
            jobDetailHtmlCache[job.id] = html;
            jobDetailSig[job.id] = sig;
            terminalContent.innerHTML = html;
        } catch (e) {
            const fallback = escapeHtml(raw).replace(/\n/g, '<br>') + tryingHtml + countdownHtml;
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
        const previousJobs = allJobs;
        allJobs = data.jobs;
        generalLog = data.general_log;

        // Detect if we need to update analytics chart
        let analyticsNeedsUpdate = false;
        
        if (analyticsOpen && analyticsChart) {
            // Check if job count changed
            if (allJobs.length !== lastJobCount) {
                analyticsNeedsUpdate = true;
                lastJobCount = allJobs.length;
            }
            
            // Check if any job status or tags changed
            if (!analyticsNeedsUpdate) {
                for (const job of allJobs) {
                    const prevState = lastJobStates.get(job.id);
                    const currentState = `${job.status}:${(job.tags || []).join(',')}`;
                    
                    if (prevState !== currentState) {
                        analyticsNeedsUpdate = true;
                        lastJobStates.set(job.id, currentState);
                    }
                }
            }
            
            // Update state map for new jobs
            if (analyticsNeedsUpdate) {
                allJobs.forEach(job => {
                    lastJobStates.set(job.id, `${job.status}:${(job.tags || []).join(',')}`);
                });
            }
        }

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
                if (newest) selectedJobId = newest.id;
            }
        } catch (e) {}

        allJobs.forEach(job => {
            const sig = jobSignature(job);
            if (jobDetailSig[job.id] && jobDetailSig[job.id] !== sig) {
                delete jobDetailHtmlCache[job.id];
                delete jobDetailSig[job.id];
            }
        });

        // Update analytics chart only if data changed
        if (analyticsNeedsUpdate) {
            updateChart();
        }
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
            
            // Initialize chart on first fetch if analytics is open
            if (analyticsOpen && !analyticsChart) {
                initializeChart();
                updateChart();
            }
            
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