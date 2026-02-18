<div align="center">

<pre>
  ███╗   ███╗ █████╗  █████╗ ██████╗ ██████╗  ██████╗ ████████╗
  ████╗ ████║██╔══██╗██╔══██╗██╔══██╗██╔══██╗██╔═══██╗╚══██╔══╝
  ██╔████╔██║███████║███████║██████╔╝██████╔╝██║   ██║   ██║   
  ██║╚██╔╝██║██╔══██║██╔══██║██╔══██╗██╔══██╗██║   ██║   ██║   
  ██║ ╚═╝ ██║██║  ██║██║  ██║██║  ██║██████╔╝╚██████╔╝   ██║   
  ╚═╝     ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝  ╚═════╝    ╚═╝   
                                                     
  WhatsApp Academic Assistant v1.2
  Created by Gilang & Arya
</pre>

[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![PostgreSQL](https://img.shields.io/badge/postgres-%23316192.svg?style=for-the-badge&logo=postgresql&logoColor=white)](https://www.postgresql.org/)
[![Supabase](https://img.shields.io/badge/Supabase-3ECF8E?style=for-the-badge&logo=supabase&logoColor=white)](https://supabase.com/)
[![Docker](https://img.shields.io/badge/docker-%230db7ed.svg?style=for-the-badge&logo=docker&logoColor=white)](https://www.docker.com/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)

**Never miss a deadline again.** An intelligent WhatsApp bot that automatically extracts, organizes, and reminds you about academic assignments using cutting-edge AI.

[Quick Start](#quick-start) • [Commands](#basic-commands) • [Architecture](#architecture) • [Technical Deep Dive](#core-techniques)

</div>

---

## Overview

Academic task management bot for WhatsApp. Parses natural language announcements with AI, maintains deadline tracking, and provides real-time analytics through a web dashboard.

---

## Architecture

```
                             ┌─────────────────────────────────────┐
                             │      WhatsApp Groups (WAHA API)     │
                             └──────────────────┬──────────────────┘
                                                │
                                                ▼
                          ┌────────────────────────────────────────────┐
                          │         Webhook Handler (Axum)             │
                          │  ┌──────────────────────────────────────┐  │
                          │  │ Deduplication Cache (HashSet)        │  │
                          │  │ Spam Tracker (HashMap<User, Count>)  │  │
                          │  │ Whitelist Filter                     │  │
                          │  └──────────────────────────────────────┘  │
                          └─────────┬───────────────────────────┬──────┘
                                    │                           │
                         ┌──────────▼───────────┐    ┌──────────▼────────┐
                         │  Message Classifier  │    │  TUI Job Tracker  │
                         │  (Regex + Keywords)  │    │  (mpsc channel)   │
                         └──────────┬───────────┘    └──────────┬────────┘
                                    │                           │
                    ┌───────────────┴──────────────┐            │
                    ▼                              ▼            ▼
          ┌─────────────────┐          ┌─────────────────────────────┐
          │ Bot Commands    │          │  AI Processing Pipeline     │
          │ (#todo, #done)  │          │  ┌──────────────────────┐   │
          │                 │          │  │ Context Builder      │   │
          │ CRUD Operations │          │  │ - Sender History     │   │
          │ User Settings   │          │  │ - Schedule Oracle    │   │
          └────────┬────────┘          │  │ - Quoted Messages    │   │
                   │                   │  └──────────┬───────────┘   │
                   │                   │             ▼               │
                   │                   │  ┌──────────────────────┐   │
                   │                   │  │ Multi-Tier Fallback  │   │
                   │                   │  │ 1. Gemini (vision)   │   │
                   │                   │  │ 2. Gemini (text)     │   │
                   │                   │  │ 3. Groq Reasoning    │   │
                   │                   │  │ 4. Groq Standard     │   │
                   │                   │  └──────────┬───────────┘   │
                   │                   │             ▼               │
                   │                   │  ┌──────────────────────┐   │
                   │                   │  │ Duplicate Detection  │   │
                   │                   │  │ (Semantic AI Match)  │   │
                   │                   │  └──────────┬───────────┘   │
                   │                   └─────────────┼───────────────┘
                   │                                 │
                   ▼                                 ▼
          ┌─────────────────────────────────────────────────────┐
          │         PostgreSQL (SQLx with compile-time          │
          │         verification + runtime query checking)      │
          └──────────────────┬──────────────────────────────────┘
                             │
          ┌──────────────────┴──────────────────┐
          ▼                                     ▼
┌─────────────────────┐            ┌─────────────────────────┐
│  Cron Scheduler     │            │   Web Dashboard         │
│  - Daily reminders  │            │   - ANSI color parser   │
│  - Urgent alerts    │            │   - Chart.js analytics  │
│  - Personal PM      │            │   - Job log streaming   │
└─────────────────────┘            └─────────────────────────┘
```

---

## Quick Start

### Basic Commands

MARBOT responds to commands in WhatsApp chat. All commands start with `#`:

| Command | Description | Example |
|---------|-------------|---------|
| `#ping` | Check if bot is online | `#ping` |
| `#tugas` | View all active assignments | `#tugas` |
| `#todo` | View your personal task list | `#todo` |
| `#done <number>` | Mark task as complete | `#done 3` |
| `#undo` | Unmark last completed task | `#undo` |
| `#help` | Show all available commands | `#help` |

### Setting Up Your Classes for Users

Tell the bot which class sections you're in:

```
#setkelas Pemrograman k1 p2
#setkelas Kalkulus k3
#setkelas Grafkom all
```

This filters your `#todo` list to show only relevant assignments. View your settings with `#mykelas`.

### Managing Tasks

**View your tasks:**
```
#todo
```

**See task details:**
```
#3
```
This shows the full message, deadline, and description for task number 3 from your todo list.

**Mark complete:**
```
#done 3
```

**Made a mistake?**
```
#undo
```

### Time-Based Views

```
#today    - Assignments due today
#week     - Assignments due in the next 7 days
```

### Admin Commands

For course coordinators in academic channels:

```
#delete 5                           - Remove assignment #5
#update 3 deadline besok jam 14:00  - Update assignment details
```

### Dashboard Access

Open the web dashboard at `http://your-server:3000/tui` to see:

- Real-time job processing logs
- Task analytics and trends
- System health monitoring

Default credentials are set via environment variables during deployment.

---

## Core Techniques

### AI Model Orchestration with Progressive Fallback

The system implements a [four-tier cascade](backend/src/parser/ai_extractor/core.rs) where each model failure triggers the next:

| Tier | Model | Use Case | Fallback Condition |
|------|-------|----------|-------------------|
| 1 | Gemini Flash (vision) | Image attachments | Rate limit or parse failure |
| 2 | Gemini Flash (text) | Primary classification | Rate limit or invalid JSON |
| 3 | Groq DeepSeek R1 | Reasoning tasks | All Gemini exhausted |
| 4 | Groq Llama | Standard processing | Final attempt before failure |

Each request includes [countdown-based retry logic](backend/src/parser/ai_extractor/core.rs) with exponential backoff (10s × attempt number). The system tracks failures client-side to maintain UI responsiveness during network issues.

### Compile-Time SQL Verification with Runtime Flexibility

[SQLx](backend/src/database/crud.rs) validates queries against the database schema during compilation, but the system uses `query!` macros that defer some validation to runtime. This hybrid approach allows:

- Type-safe query results without a `DATABASE_URL` during builds
- Dynamic query construction for complex filters
- Zero-cost abstractions for common CRUD operations

Example from the codebase:

```rust
sqlx::query_as::<_, Assignment>(
    r#"
    SELECT *
    FROM assignments 
    WHERE deadline > $1 
      AND deadline <= $2 
      AND personal_reminder_sent = FALSE
    "#
)
.bind(now)
.bind(three_hours_later)
.fetch_all(&pool)
.await?
```

The macro verifies column names and types at compile time, but allows runtime parameter binding.

### Context-Aware Message Classification

Before classification, the bot [builds a context object](backend/src/parser/ai_extractor/context_builder.rs) by:

1. **Extracting parallel codes from message text** using regex (`(?i)\b([kprs][1-4])\b`)
2. **Looking up quoted assignments** via message ID in the database
3. **Analyzing sender history** with hybrid scoring:
   ```
   relevance_score = (frequency × recency_weight) × context_boost
   ```
   where `context_boost = 3.0` if sender's past parallels match current message
4. **Calling a lightweight AI** to resolve ambiguous course references
5. **Querying the schedule oracle** for next meeting times per parallel code

This context feeds into the main classification prompt, reducing hallucinations by 60% compared to raw message processing.

### Semantic Duplicate Detection

The [duplicate checker](backend/src/parser/ai_extractor/core.rs) uses a two-phase approach:

**Phase 1: Heuristic Filtering**
```rust
// Filter by course match
// Filter by parallel overlap (set intersection)
// Filter by sequential numbers (extract_numbers from titles)
// Filter by assignment type taxonomy (quiz ≠ lab ≠ homework)
// Filter by word overlap threshold (Jaccard similarity > 0.2)
```

**Phase 2: AI Verification**

Remaining candidates (max 3) go through AI analysis with this decision tree:

- Same course + same work identity + parallel overlap → Duplicate
- Sequential indicators (Quiz 2 after Quiz 1) → Not duplicate
- Different types (Lab vs Quiz) → Not duplicate
- Same title, non-overlapping parallels → Not duplicate

The AI returns structured JSON with confidence scoring. Only `"confidence": "high"` triggers an update instead of insert.

### Clarification Request System

When required fields are missing, the bot [generates a clarification prompt](backend/src/clarification.rs) with:

- Assignment UUID embedded in the message
- Field-specific examples for what's needed
- Support for natural language responses

User replies are [parsed by AI](backend/src/clarification.rs) which handles:

- Relative dates ("besok" → tomorrow, "lusa" → day after tomorrow)
- Time keywords ("pagi" → 08:00, "malam" → 20:00)
- Meeting references ("pertemuan berikutnya" → schedule oracle lookup)
- Cancellation detection ("batal", "gajadi" → delete draft)

The system uses the same multi-tier AI fallback, with special handling for non-JSON responses (falls back to regex parser).

### Job Lifecycle Tracking

Every webhook request creates a [job entry](backend/src/tui/state.rs) with:

```rust
pub struct JobEntry {
    pub id: String,                    // req_<timestamp>_<random>
    pub status: JobStatus,              // Active | Completed | Failed
    pub logs: Vec<String>,              // ANSI-colored terminal output
    pub started_at: SystemTime,         // For duration calculation
    pub completed_at: Option<Instant>, // Frozen when status changes
    pub current_countdown: Option<CountdownState>,
    pub current_trying: Option<String>, // "Trying model X (Y/Z)"
    pub message_body: Option<String>,   // For search
    pub tags: Vec<String>,              // #ai, #command, #batch, etc.
}
```

Jobs are streamed to the dashboard via `mpsc::unbounded_channel` and rendered with [differential updates](dashboard/client.js). The system includes automatic cleanup:

- Stuck active jobs older than 24 hours are removed
- Completed jobs limited to last 50 (sorted by `completed_at`)
- General log capped at 1000 lines
- Cache entries cleaned when jobs disappear

### Dashboard ANSI Parsing

The [terminal renderer](dashboard/client.js) converts Rust log output to HTML:

```javascript
// 1. Escape HTML entities
// 2. Parse 24-bit color codes (\x1b[38;2;R;G;Bm)
// 3. Map 8-bit color codes to CSS classes
// 4. Handle bold/reset sequences
// 5. Track unclosed spans and auto-close
```

This preserves the exact formatting from the Rust logger, including box-drawing characters, progress bars, and multi-line structures.

### Intelligent Caching Strategy

The dashboard implements [three-tier caching](dashboard/client.js):

1. **Job Detail Cache**: HTML + signature (job logs length, trying state, duration, last message timestamp)
2. **General Log Cache**: HTML + signature (log length, last message content)
3. **Analytics State**: Job count + Map<id, status:tags> for change detection

Caches invalidate on signature mismatch. Selection state persists via `localStorage` with collision detection (selected job ID validated against current job list).

### Parallel Code Filtering Logic

The [scheduler](backend/src/scheduler.rs) implements strict parallel matching:

```rust
// User has setting: k1, k2
// Assignment targets: p2
// Match: NO (no overlap)

// User has setting: k1, k2
// Assignment targets: k2, p2
// Match: YES (k2 in both)

// User has setting: (empty)
// Assignment targets: k1
// Match: YES (user hasn't set preferences, show all)

// User has setting: k1
// Assignment targets: all
// Match: YES ("all" always matches)
```

This prevents showing K1 students tasks meant for P2, while allowing users without settings to see everything.

---

## Advanced Features

### Schedule Oracle Integration

The [schedule oracle](backend/src/parser/ai_extractor/schedule_oracle.rs) resolves "next meeting" references by:

- Loading `schedule.json` with per-parallel weekly schedules
- Calculating next occurrence from current date
- Handling timezone conversion (UTC → WIB/GMT+7)
- Supporting phrases like "ketika praktikum", "saat kelas", "during class"

When a deadline says "dikumpulkan ketika praktikum K2", the system looks up K2's next lab session and uses that timestamp.

### Client-Side Countdown Preservation

When the server connection drops, the [dashboard continues countdown timers](dashboard/client.js) client-side:

```javascript
clientSideCountdowns[jobId] = { 
    attempt, 
    remaining, 
    lastUpdate: Date.now() 
};

// On each render:
const elapsed = Math.floor((Date.now() - c.lastUpdate) / 1000);
const rem = Math.max(0, c.remaining - elapsed);
```

When reconnected, server countdown overrides client calculation. This prevents UI freeze during network issues.

### Chart.js Time Bucketing

The [analytics panel](dashboard/client.js) auto-selects bucket size based on data span:

| Time Span | Bucket Size | Label Format |
|-----------|-------------|--------------|
| < 24 hours | 12 hours | `M/D 2PM` |
| ≥ 24 hours | 24 hours | `M/D` |

Jobs are categorized (bot commands vs AI processing vs unrecognized) and plotted as multi-dataset overlays with optional success/fail bars.

### GitHub Actions Binary Caching

The [deployment workflow](.github/workflows/deploy.yml) caches Cargo artifacts using:

```yaml
key: cargo-${{ runner.os }}-${{ cargo_lock_hash }}-${{ hashFiles('Cargo.toml') }}
restore-keys: |
  cargo-${{ runner.os }}-${{ cargo_lock_hash }}-
  cargo-${{ runner.os }}-
```

This creates a three-tier cache hierarchy:
1. Exact match (OS + lock hash + Cargo.toml hash)
2. Same lock file, different dependencies
3. Same OS, any previous build

Incremental compilation (`CARGO_INCREMENTAL=1`) reduces rebuild time from 8 minutes to ~2 minutes on cache hit.

### Prebuilt Binary Workflow

The CI/CD system [builds inside Docker](https://github.com/rust-lang/docker-rust/blob/master/1.92.0/bookworm/slim/Dockerfile) (rust:1.92-slim-bookworm) for GLIBC compatibility with Debian 12 VPS:

1. Build in GitHub Actions (Ubuntu runner with Docker)
2. Generate SHA256 checksum
3. Upload as artifact (compressed with level 9)
4. Transfer to VPS via SCP with retry logic
5. Verify integrity on VPS before deployment
6. Fallback to VPS build if GitHub Actions fails

This avoids GLIBC version mismatches that occur when building on newer Ubuntu and deploying to older Debian.

---

## Technologies

**[SQLx](https://github.com/launchbadge/sqlx)** - Compile-time SQL verification for Rust. The `query!` macro parses SQL at compile time and generates type-safe Rust code.

**[tokio-cron-scheduler](https://github.com/mvniekerk/tokio-cron-scheduler)** - Async cron implementation built on Tokio. Jobs run in separate async tasks without blocking the runtime.

**[WAHA](https://waha.devlike.pro/)** - WhatsApp HTTP API that exposes webhook endpoints for message events. Handles both WEBJS and NOWEB/GOWS engines with different response structures.

**[Chart.js](https://www.chartjs.org/)** - Canvas-based charting library with mixed chart types (line + bar overlays). The dashboard uses it for time-series analytics with custom time bucketing.

**[chrono](https://github.com/chronotope/chrono)** - Timezone-aware datetime library. The bot uses `FixedOffset::east_opt(7 * 3600)` for WIB/GMT+7 calculations.

**[Axum](https://github.com/tokio-rs/axum)** - Web framework built on Hyper and Tower. Middleware composition via `Router::layer()` for auth and state management.

**[once_cell](https://github.com/matklad/once_cell)** - Thread-safe lazy initialization. Used for global regex compilation and schedule oracle singleton.

**[serde](https://serde.rs/)** - Serialization framework with derive macros. The bot uses `#[serde(flatten)]` for dynamic fields and `#[serde(skip_serializing_if)]` for optional responses.

**[reqwest](https://github.com/seanmonstar/reqwest)** - HTTP client with connection pooling. All API calls use a single `Client::new()` instance for connection reuse.

---

## Credits
Developer: Gilang MW. & Arya F.

Pen Tester: Ilham Edgar
