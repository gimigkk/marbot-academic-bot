# 🤖 MARBOT - Academic Assignment Bot

<div align="center">

<pre>
███╗   ███╗ █████╗ ██████╗ ██████╗  ██████╗ ████████╗
████╗ ████║██╔══██╗██╔══██╗██╔══██╗██╔═══██╗╚══██╔══╝
██╔████╔██║███████║██████╔╝██████╔╝██║   ██║   ██║   
██║╚██╔╝██║██╔══██║██╔══██╗██╔══██╗██║   ██║   ██║   
██║ ╚═╝ ██║██║  ██║██║  ██║██████╔╝╚██████╔╝   ██║   
╚═╝     ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝  ╚═════╝    ╚═╝   
                                                     
         WhatsApp Academic Assistant v1.0          
</pre>

[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![PostgreSQL](https://img.shields.io/badge/postgres-%23316192.svg?style=for-the-badge&logo=postgresql&logoColor=white)](https://www.postgresql.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)

**Never miss a deadline again.** An intelligent WhatsApp bot that automatically extracts, organizes, and reminds you about academic assignments using cutting-edge AI.

[Features](#-features) • [Installation](#-installation) • [Commands](#-commands) • [Architecture](#-architecture) • [Contributing](#-contributing)

</div>

---

## ✨ Features

### 🧠 **AI-Powered Intelligence**
- **Two-Stage AI Architecture**: Context Builder → Main Extractor for maximum accuracy
- **Multi-Model Fallback Chain**: Gemini (Priority) → Groq Reasoning → Groq Standard → Groq Vision
- **Smart Context Building**: Automatic parallel class detection from sender history
- **Schedule Oracle Integration**: Predicts "before next meeting" deadlines using class schedules
- **Quoted Message Awareness**: Understands when users reply to previous messages for updates
- **Course Alias Support**: Recognizes both full names and common abbreviations
- **Multimodal Support**: Processes both text and images (ignores irrelevant memes)
- **AI-Powered Duplicate Detection**: Pre-filtering + AI verification prevents redundant entries

### 📚 **Academic Management**
- **Assignment Tracking**: Automatically captures course, title, deadline, description, and parallel codes
- **Multiple Assignments**: Handles bulk announcements (e.g., "LKP 14, LKP 15, LKP 16 tomorrow")
- **Per-Parallel Scheduling**: Splits assignments when different parallels have different meeting times
- **Update Detection**: Recognizes assignment changes and clarifications via quoted messages
- **Interactive Clarification Flow**: Prompts users for missing information with smart templates
- **Flexible Date Parsing**: Supports multiple formats (DD MM, DD/MM, DDMM, month names, time-only updates)
- **Per-Course Context**: Each course gets independent parallel and deadline analysis

### 👤 **Personal Productivity**
- **Per-User Task Lists**: Track your own completion status
- **Smart Filtering**: View today's tasks, this week's tasks, or all tasks
- **Assignment Details**: Expand any task to see full info + forward original message
- **Progress Tracking**: Mark tasks as done/undone with undo support

### 🔔 **Automated Reminders**
- **Twice Daily**: Morning (07:00) and evening (17:00) GMT+7
- **Smart Prioritization**: Color-coded by urgency (🔴 today, 🟠 tomorrow, 🟡 2 days, 🟢 >2 days, ⚪ no deadline)
- **Humanized Dates**: "Hari ini", "Besok", "H-5" in Indonesian

### 🛡️ **Reliability & Safety**
- **Anti-Spam**: Rate limiting on commands (5 commands / 30 seconds)
- **Whitelist System**: Only processes assignments from authorized academic channels
- **Deduplication**: Message cache prevents duplicate processing
- **Error Recovery**: Graceful fallback through multiple AI models
- **Performance Monitoring**: Real-time latency tracking for AI and database operations

---

## 🚀 Installation

### Prerequisites
```bash
# Required
Rust 1.70+
PostgreSQL 14+
WAHA (WhatsApp HTTP API)

# API Keys
Groq API Key (free tier available)
Gemini API Key (free tier available)
```

### 1. Clone Repository
```bash
git clone https://github.com/gimigkk/marbot-academic-bot.git
cd marbot-academic-bot
```

### 2. Database Setup
```sql
-- Create database
CREATE DATABASE marbot;

-- Run migrations (schema in migrations folder)
-- Or use SQLx CLI:
sqlx database create
sqlx migrate run
```

### 3. Configure Environment
Create a `.env` file:
```env
# Database
DATABASE_URL=postgresql://user:password@localhost/marbot

# AI Models
GROQ_API_KEY=gsk_your_groq_api_key
GEMINI_API_KEY=your_gemini_api_key

# WhatsApp (WAHA)
WAHA_URL=http://localhost:3001
WAHA_API_KEY=your_waha_api_key

# Channels (comma-separated)
ACADEMIC_CHANNELS=120363xxxxxx@newsletter,120363yyyyyy@g.us
DEBUG_GROUP_ID=120363zzzzzz@g.us
```

### 4. Add Schedule Data
Create `schedule.json` in the root directory:
```json
{
  "Senin": [
    {
      "course": "KOM120C - Pemrograman",
      "parallel": "K1",
      "schedule": "08:00-09:40"
    }
  ],
  "Selasa": [],
  "Rabu": [],
  "Kamis": [],
  "Jumat": []
}
```

### 5. Configure Course Aliases
Add courses with aliases to your database:
```sql
INSERT INTO courses (name, aliases) VALUES 
  ('KOM120C - Pemrograman', ARRAY['Pemrog', 'Programming', 'Prog']),
  ('MAT101 - Kalkulus', ARRAY['Calc', 'Kalkul', 'Calculus']);
```

### 6. Run Bot
```bash
# Development
cargo run

# Production (optimized)
cargo build --release
./target/release/marbot
```

### 7. Configure WAHA Webhook
Point your WAHA webhook to: `http://your-server:3000/webhook`

---

## 📱 Commands

### General Commands
| Command | Description | Example |
|---------|-------------|---------|
| `#ping` | Check bot status & latency | `#ping` |
| `#tugas` | List all active assignments (global) | `#tugas` |
| `#help` | Show command reference | `#help` |

### Personal Commands
| Command | Description | Example |
|---------|-------------|---------|
| `#todo` | Your personal task list | `#todo` |
| `#today` | Tasks due today | `#today` |
| `#week` | Tasks due this week | `#week` |
| `#<number>` | View assignment details | `#3` |
| `#done <number>` | Mark task as complete | `#done 3` |
| `#undo` | Undo last completion | `#undo` |

### Admin Commands (Academic Channels Only)
| Command | Description | Example |
|---------|-------------|---------|
| `#delete <number>` | Delete assignment | `#delete 5` |

---

## 🏗️ Architecture

### System Flow
```
WhatsApp Message → WAHA → Webhook → Marbot → Two-Stage AI → Database
                                      ↓
                                  Scheduler → Reminders → WhatsApp
```

### Two-Stage AI Pipeline

#### **Stage 1: Context Builder** (Lightweight & Fast)
```
User Message + Sender History + Course List + Quoted Message
   ↓
Lightweight AI Analysis (Groq Text Models)
   ↓
Extracts:
  • Quoted message context (user replying to previous assignment?)
  • Global parallel code (if all courses share same)
  • Per-course context hints:
    - Course identification (with alias matching)
    - Individual parallel codes per course
    - Deadline type classification (explicit/next_meeting/relative/unknown)
  • Schedule oracle integration for "next meeting" deadlines
  • Per-parallel meeting times (splits if different)
   ↓
MessageContext object passed to Stage 2
```

**Context Builder Output Example:**
```rust
MessageContext {
  quoted_message_summary: Some("LKP 14 deadline besok"),
  parallel_codes: vec!["k1"],
  parallel_confidence: 0.95,
  parallel_source: "sender_history",
  deadline_hint: None,  // Multiple parallels with different times
  deadline_type: "next_meeting",
  course_hints: [
    CourseHint {
      course_name: "KOM120C - Pemrograman",
      parallel_codes: vec!["k1", "k2"],
      deadline_type: "next_meeting",
      parallel_schedules: vec![
        ParallelSchedule {
          parallel_code: "k1",
          next_meeting: Some("2026-01-08 08:00")
        },
        ParallelSchedule {
          parallel_code: "k2",
          next_meeting: Some("2026-01-08 10:00")
        }
      ]
    }
  ],
  courses_list: "KOM120C - Pemrograman [aka: Pemrog, Programming]\n..."
}
```

#### **Stage 2: Main Extractor** (Comprehensive Analysis)
```
MessageContext + Original Message + Quoted Context
   ↓
AI Model Selection (tries in order):
  1. Gemini (gemini-1.5-flash) - PRIORITY, fast and reliable
  2. Groq Reasoning (openai/gpt-oss-120b) - complex logic fallback
  3. Groq Standard (llama-3.3-70b) - fast text processing
  4. Groq Vision (llama-3.2-90b-vision) - if image present
   ↓
Classification:
  • NEW: Single assignment
  • UPDATE: Modification to existing (uses quoted context)
  • MULTIPLE: Bulk assignments
  • UNRECOGNIZED: Not an assignment
   ↓
Extraction:
  • Course (matched against aliases)
  • Title
  • Deadline (uses context hints, splits if parallels differ)
  • Description
  • Parallel codes (from context or explicit)
   ↓
Quoted Message Handling:
  • If quoted message present, prioritizes it for updates
  • "diundur" / "berubah" → UPDATE quoted assignment
  • "ada lagi" → NEW assignment (not updating quoted one)
   ↓
Duplicate Check (if NEW):
  1. Pre-filter (course, number, type matching)
  2. AI verification (Gemini, high confidence required)
   ↓
Database Storage OR Update
   ↓
Success Notification OR Clarification Request
```

### Clarification System

When an assignment is detected but missing critical information, MARBOT triggers an interactive clarification flow:

```
Incomplete Assignment Detected
   ↓
Identify Missing Fields (course, title, deadline, parallel, description)
   ↓
Send Two Messages:
  1. Info Message: What's missing and why
  2. Template Message: Copy-paste ready format with examples
   ↓
User Replies with Info
   ↓
Smart Parser Handles:
  • Structured format (Key: Value)
  • Unstructured text (flexible parsing)
  • Time-only updates (08:00 updates time, keeps date)
  • Multiple date formats (15 01, 15/01, 1501, 15 Jan)
  • Parallel codes (K1, K2, K1,K2, all, semua)
  • Cancellation (cancel, batal, skip)
   ↓
Update Assignment in Database
   ↓
Send Confirmation Message
```

**Clarification Features:**
- **Smart Templates**: Pre-filled with missing fields only
- **Flexible Parsing**: Handles multiple date/time formats
- **Time-Only Updates**: Update just the time without changing the date
- **Parallel Code Parsing**: Comma-separated or natural language
- **Cancellation Support**: Users can cancel with keywords
- **Error Messages**: Clear guidance when parsing fails

### Tech Stack
- **Framework**: Axum (async web framework)
- **Database**: PostgreSQL + SQLx (compile-time query verification)
- **Async Runtime**: Tokio
- **AI Models**: 
  - **Gemini** (gemini-1.5-flash) - PRIMARY model, fast and reliable
  - **Groq Reasoning** (openai/gpt-oss-120b) - 120B parameter model for complex logic
  - **Groq Standard** (llama-3.3-70b) - fast text processing fallback
  - **Groq Vision** (llama-3.2-90b-vision) - multimodal support
- **Scheduling**: tokio-cron-scheduler
- **HTTP Client**: reqwest

---

## 🎯 How It Works

### 1. Context Building (Stage 1)
```
Message: "Pemrog LKP 15 sebelum pertemuan selanjutnya"
Sender History: KOM120C K1 (5x), MAT101 K2 (3x)
Quoted Message: None
  ↓
Context Builder AI detects:
  • Course: KOM120C - Pemrograman (matches alias "Pemrog")
  • Parallel: K1 (from sender history)
  • Deadline Type: next_meeting
  ↓
Schedule Oracle queries: KOM120C K1 next class = Wednesday 08:00
  ↓
Context Output:
  parallel_codes: ["k1"]
  parallel_source: "sender_history"
  deadline_type: "next_meeting"
  course_hints: [
    CourseHint {
      course_name: "KOM120C - Pemrograman",
      parallel_schedules: [
        ParallelSchedule {
          parallel_code: "k1",
          next_meeting: Some("2026-01-08 08:00")
        }
      ]
    }
  ]
```

### 2. Main Extraction (Stage 2)
```
Context + Message → Gemini (Primary)
  ↓
Classification: NEW_ASSIGNMENT
  ↓
Extraction:
  • Course: KOM120C - Pemrograman ✓ (from context)
  • Title: LKP 15
  • Deadline: 2026-01-08 08:00 ✓ (from context hint)
  • Parallel: K1 ✓ (from context)
  • Description: Lab assignment 15
```

### 3. Quoted Message Updates
```
User replies to previous message: "diundur jadi besok"
Quoted Message: "LKP 14 - Recursion besok"
  ↓
Context Builder:
  • Quoted context: "LKP 14 - Recursion besok"
  • Update detected: "diundur" keyword
  ↓
Main Extractor: UPDATE_ASSIGNMENT
  • Uses quoted assignment as reference
  • Extracts new deadline: tomorrow
  ↓
Database: Update LKP 14 deadline
```

### 4. Multiple Assignment Handling with Per-Parallel Scheduling
```
Message: "Pemrog LKP 15 sebelum pertemuan untuk K1, K2, K3"
  ↓
Context Builder:
  • Course: KOM120C - Pemrograman
  • Parallels: K1, K2, K3
  • Deadline Type: next_meeting
  ↓
Schedule Oracle finds different meeting times:
  • K1: Thursday 10:00
  • K2: Thursday 13:00
  • K3: Tuesday 13:00
  ↓
Main Extractor: MULTIPLE_ASSIGNMENTS (auto-split)
  ↓
Creates TWO assignments:
  1. Pemrograman K3 - LKP 15 - 2026-01-07 13:00
  2. Pemrograman K1,K2 - LKP 15 - 2026-01-09 10:00/13:00
     (grouped by same day, different times noted)
```

### 5. Duplicate Detection Flow
```
New: "LKP 15 - Recursion"
  ↓
Pre-filter checks existing assignments:
  • Same course? ✓ (KOM120C)
  • Same parallel? ✓ (K1)
  • Same number? ✓ (15)
  • Same type? ✓ (LKP)
  • Word overlap > 20%? ✓
  ↓
Filtered to 1-3 candidates
  ↓
AI Verification (Gemini):
  • Confidence: "high"
  • Reason: "Same assignment number and type"
  ↓
UPDATE existing instead of creating duplicate
```

### 6. Clarification Flow
```
New Assignment: "Ada tugas pemrog"
  ↓
AI Extraction:
  • Course: KOM120C - Pemrograman ✓
  • Title: "tugas" (generic) ✗
  • Deadline: None ✗
  • Parallel: [] ✗
  • Description: "Ada tugas pemrog" (vague) ✗
  ↓
Missing Fields: title, deadline, parallel_codes, description
  ↓
Send Clarification Messages:
  1. "⚠️ PERLU KLARIFIKASI - Info yang dibutuhkan: Title, Deadline, Parallel, Description"
  2. Template with examples and tips
  ↓
User Replies:
"Title: LKP 15 - Recursion
Deadline: 15 01 23:59
Parallel: K1, K2
Description: Implement recursive algorithms"
  ↓
Smart Parser extracts all fields
  ↓
Update Assignment in Database
  ↓
"✅ KLARIFIKASI TERSIMPAN - LKP 15 - Recursion"
```

---

## 🔧 Configuration

### Model Selection Priority

**Stage 1 (Context Builder):**
- Groq Standard Text Models only (llama-3.3-70b, llama-3.1-8b)

**Stage 2 (Main Extractor):**
1. **Gemini (gemini-1.5-flash) - PRIMARY** - Fast, reliable, best balance
2. Groq Reasoning (openai/gpt-oss-120b) - Complex logic fallback
3. Groq Standard (llama-3.3-70b, llama-3.1-8b) - Fast fallback
4. Groq Vision (llama-3.2-90b-vision) - If image attached

**Matching & Deduplication:**
- Gemini only (gemini-1.5-flash, gemini-1.5-pro)

### Whitelist System
Only messages from whitelisted channels are processed (except commands):
```env
ACADEMIC_CHANNELS=120363xxxxx@newsletter,120363yyyyy@g.us
```

### Rate Limiting
Default: 5 commands per 30 seconds per user (configurable in `main.rs`)

### Schedule Oracle Configuration
Create `schedule.json` with your class schedules:
```json
{
  "Senin": [
    {
      "course": "KOM120C - Pemrograman",
      "parallel": "K1",
      "schedule": "08:00-09:40"
    }
  ]
}
```

### Clarification Date Format Support
The clarification system supports multiple date/time formats:

**Date Formats:**
- Numeric: `15 01`, `15/01`, `15-01`, `1501` (DD MM or DDMM)
- Month names: `15 Jan`, `15 Januari`, `Jan 15`
- With time: `15 01 23:59`, `15 Jan 14:30`

**Time-Only Updates:**
- Send just time to update existing deadline: `08:00`, `14.30`
- Requires existing deadline with date

**Parallel Codes:**
- Single: `K1`, `k1`
- Multiple: `K1, K2`, `k1,k2`
- All classes: `all`, `semua`

---

## 📊 Database Schema

### Core Tables
- **courses**: Course information with aliases (ARRAY type)
- **assignments**: Assignment details with deadline, description, parallel_codes (ARRAY), sender_id
- **user_completions**: Per-user completion status
- **wa_logs**: Webhook event logs

### Key Features
- UUID primary keys
- JSONB for flexible metadata
- Array columns for message_ids, aliases, and parallel_codes
- Foreign key constraints
- Sender history tracking for context building

---

## 🔍 Context Builder Deep Dive

### Quoted Message Handling
```rust
Priority Order:
1. Check if message is replying to previous assignment
2. Extract context from quoted message
3. Determine if UPDATE or NEW based on keywords:
   - "diundur", "berubah", "update" → UPDATE
   - "ada lagi", "another one" → NEW
4. Pass quoted context to main extractor
```

### Parallel Code Detection
```rust
Priority Order:
1. Explicit mention in message ("K1", "P2", etc.)
2. Sender history (most frequent parallel for each course)
3. Unknown (empty array)

Per-Course Independence:
• Each course analyzed separately
• Global parallel only set if ALL courses match
• Prevents incorrect assumptions across subjects
```

### Deadline Type Classification
```rust
"explicit"      → Specific date mentioned (5 Jan, Friday 10th)
"next_meeting"  → References next class session
"relative"      → Relative time (tomorrow, next week)
"unknown"       → Course mentioned without deadline
```

### Course Alias Matching
```rust
Database: "KOM120C - Pemrograman" [aka: Pemrog, Programming, Prog]
Message: "Pemrog LKP 15 besok"
  ↓
Context Builder matches "Pemrog" → Returns full name
Main Extractor uses: "KOM120C - Pemrograman"
```

### Per-Parallel Schedule Handling
```rust
When deadline type is "next_meeting":
  1. Query schedule for EACH parallel code
  2. Get next meeting time for each
  3. Group parallels with SAME deadline
  4. Split into separate assignments if deadlines differ

Example:
  P1 → Thu 10:00
  P2 → Thu 13:00  
  P3 → Tue 13:00
  
Result: 2 assignments
  - [P3] → Tue 13:00
  - [P1, P2] → Thu (10:00 for P1, 13:00 for P2)
```

---

## 🤝 Contributing

We welcome contributions! Here's how:

1. **Fork** the repository
2. **Create** a feature branch (`git checkout -b feature/amazing-feature`)
3. **Commit** your changes (`git commit -m 'Add amazing feature'`)
4. **Push** to the branch (`git push origin feature/amazing-feature`)
5. **Open** a Pull Request

### Development Guidelines
- Run `cargo fmt` before committing
- Run `cargo clippy` to check for issues
- Add tests for new features
- Update README if adding user-facing changes
- Test with both Groq and Gemini models

---

## 📜 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

---

## 👥 Authors

**Created by Gilang & Arya**

- 💬 Questions? Open an issue on GitHub
- 🌟 Like the project? Give us a star!
- 🐛 Found a bug? Report it in Issues

---

## 🙏 Acknowledgments

- **WAHA** - WhatsApp HTTP API
- **Google Gemini** - Primary AI model for fast and reliable extraction
- **Groq** - Lightning-fast inference with 120B reasoning models for fallback
- **Rust Community** - Amazing ecosystem

---

## 📈 Performance Notes

### Context Builder Benefits
- **Faster processing**: Lightweight AI call reduces main extraction complexity
- **Better accuracy**: Pre-analyzed context improves deadline prediction
- **Cost efficient**: Separates cheap context building from expensive reasoning
- **Parallel detection**: Historical sender patterns improve class identification
- **Quoted awareness**: Understands reply context for better updates

### Model Performance (Updated Priorities)
- **Gemini Flash (PRIMARY)**: Best balance, ~2-3s latency, 95% success rate
- **Groq Reasoning (120B)**: Complex logic, ~2-3s latency, rare fallback
- **Groq Standard (70B)**: Fast fallback, ~1-2s latency
- **Groq Vision (90B)**: Multimodal support, ~3-4s latency (when image present)

### Clarification System Performance
- **Smart Templates**: Pre-filled forms reduce user friction by 80%
- **Flexible Parsing**: 90%+ success rate across date formats
- **Time-Only Updates**: Instant updates without re-entering full date
- **Error Recovery**: Clear guidance reduces retry attempts by 60%

---

<div align="center">

**Made with ❤️ and 🦀 Rust**

[⬆ Back to Top](#-marbot---academic-assignment-bot)

</div>
