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
- **Multi-Model Fallback Chain**: Groq Reasoning (120B) → Groq Standard → Groq Vision → Gemini
- **Smart Context Building**: Automatic parallel class detection from sender history
- **Schedule Oracle Integration**: Predicts "before next meeting" deadlines using class schedules
- **Course Alias Support**: Recognizes both full names and common abbreviations
- **Multimodal Support**: Processes both text and images (ignores irrelevant memes)
- **AI-Powered Duplicate Detection**: Pre-filtering + AI verification prevents redundant entries

### 📚 **Academic Management**
- **Assignment Tracking**: Automatically captures course, title, deadline, description, and parallel code
- **Multiple Assignments**: Handles bulk announcements (e.g., "LKP 14, LKP 15, LKP 16 tomorrow")
- **Update Detection**: Recognizes assignment changes and clarifications
- **Clarification Flow**: Interactive system for incomplete assignment data
- **Per-Course Context**: Each course gets independent parallel and deadline analysis

### 👤 **Personal Productivity**
- **Per-User Task Lists**: Track your own completion status
- **Smart Filtering**: View today's tasks, this week's tasks, or all tasks
- **Assignment Details**: Expand any task to see full info + forward original message
- **Progress Tracking**: Mark tasks as done/undone with undo support

### 🔔 **Automated Reminders**
- **Twice Daily**: Morning (07:00) and evening (17:00) GMT+7
- **Smart Prioritization**: Color-coded by urgency (🔴 0-2 days, 🟢 >2 days, ⚪ no deadline)
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
User Message + Sender History + Course List
   ↓
Lightweight AI Analysis (Groq Text Models)
   ↓
Extracts:
  • Global parallel code (if all courses share same)
  • Per-course context hints:
    - Course identification (with alias matching)
    - Individual parallel codes
    - Deadline type classification (explicit/next_meeting/relative/unknown)
  • Schedule oracle integration for "next meeting" deadlines
   ↓
MessageContext object passed to Stage 2
```

**Context Builder Output Example:**
```rust
MessageContext {
  parallel_code: Some("K1"),           // Global if all courses match
  parallel_confidence: 0.95,
  parallel_source: "sender_history",   // Or "explicit" / "unknown"
  deadline_hint: Some("2026-01-08 08:00"),
  deadline_type: "next_meeting",
  course_hints: [
    CourseHint {
      course_name: "KOM120C - Pemrograman",
      parallel_code: Some("K1"),
      deadline_hint: Some("2026-01-08 08:00"),
      deadline_type: "next_meeting"
    }
  ],
  courses_list: "KOM120C - Pemrograman [aka: Pemrog, Programming]\n..."
}
```

#### **Stage 2: Main Extractor** (Comprehensive Analysis)
```
MessageContext + Original Message
   ↓
AI Model Selection (tries in order):
  1. Groq Reasoning (openai/gpt-oss-120b) - complex logic
  2. Groq Vision (if image present) - multimodal
  3. Groq Standard (llama-3.3-70b) - fallback
  4. Gemini - final fallback
   ↓
Classification:
  • NEW: Single assignment
  • UPDATE: Modification to existing
  • MULTIPLE: Bulk assignments
  • UNRECOGNIZED: Not an assignment
   ↓
Extraction:
  • Course (matched against aliases)
  • Title
  • Deadline (uses context hints)
  • Description
  • Parallel code (from context)
   ↓
Duplicate Check (if NEW):
  1. Pre-filter (course, number, type matching)
  2. AI verification (Gemini, high confidence required)
   ↓
Database Storage OR Update
   ↓
Success Notification OR Clarification Request
```

### Tech Stack
- **Framework**: Axum (async web framework)
- **Database**: PostgreSQL + SQLx (compile-time query verification)
- **Async Runtime**: Tokio
- **AI Models**: 
  - **Groq Reasoning** (openai/gpt-oss-120b) - 120B parameter model for complex logic
  - **Groq Vision** (llama-3.2-90b-vision) - multimodal support
  - **Groq Standard** (llama-3.3-70b) - fast text processing
  - **Gemini** (gemini-1.5-flash) - reliable fallback
- **Scheduling**: tokio-cron-scheduler
- **HTTP Client**: reqwest

---

## 🎯 How It Works

### 1. Context Building (Stage 1)
```
Message: "Pemrog LKP 15 sebelum pertemuan selanjutnya"
Sender History: KOM120C K1 (5x), MAT101 K2 (3x)
  ↓
Context Builder AI detects:
  • Course: KOM120C - Pemrograman (matches alias "Pemrog")
  • Parallel: K1 (from sender history)
  • Deadline Type: next_meeting
  ↓
Schedule Oracle queries: KOM120C K1 next class = Wednesday 08:00
  ↓
Context Output:
  parallel_code: "K1"
  parallel_source: "sender_history"
  deadline_hint: "2026-01-08 08:00"
  deadline_type: "next_meeting"
```

### 2. Main Extraction (Stage 2)
```
Context + Message → Groq Reasoning (120B)
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

### 3. Multiple Assignment Handling
```
Message: "Pemrog LKP 15 dan Kalkulus Quiz 3 besok jam 10"
  ↓
Context Builder:
  • Course 1: KOM120C - Pemrograman (K1, relative deadline)
  • Course 2: MAT101 - Kalkulus (K2, relative deadline)
  ↓
Main Extractor: MULTIPLE_ASSIGNMENTS
  ↓
Extracts:
  1. Pemrograman K1 - LKP 15 - 2026-01-04 10:00
  2. Kalkulus K2 - Quiz 3 - 2026-01-04 10:00
```

### 4. Duplicate Detection Flow
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

---

## 🔧 Configuration

### Model Selection Priority
Models are tried in order (configurable in `ai_extractor/mod.rs`):

**Stage 1 (Context Builder):**
- Groq Standard Text Models only (llama-3.3-70b, llama-3.1-8b)

**Stage 2 (Main Extractor):**
1. Groq Reasoning (openai/gpt-oss-120b) - Best for complex logic
2. Groq Vision (llama-3.2-90b-vision) - If image attached
3. Groq Standard (llama-3.3-70b, llama-3.1-8b) - Fast fallback
4. Gemini (gemini-1.5-flash) - Final fallback

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

---

## 📊 Database Schema

### Core Tables
- **courses**: Course information with aliases (ARRAY type)
- **assignments**: Assignment details with deadline, description, parallel, sender_id
- **user_completions**: Per-user completion status
- **wa_logs**: Webhook event logs

### Key Features
- UUID primary keys
- JSONB for flexible metadata
- Array columns for message_ids and aliases
- Foreign key constraints
- Sender history tracking for context building

---

## 🔍 Context Builder Deep Dive

### Parallel Code Detection
```rust
Priority Order:
1. Explicit mention in message ("K1", "P2", etc.)
2. Sender history (most frequent parallel for each course)
3. Unknown (null)

Per-Course Independence:
• Each course analyzed separately
• Global parallel only set if ALL courses match
• Prevents incorrect assumptions across subjects
```

### Deadline Type Classification
```rust
"explicit"      → Specific date mentioned
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
- **Groq** - Lightning-fast inference with 120B reasoning models
- **Google Gemini** - Reliable fallback model
- **Rust Community** - Amazing ecosystem

---

## 📈 Performance Notes

### Context Builder Benefits
- **Faster processing**: Lightweight AI call reduces main extraction complexity
- **Better accuracy**: Pre-analyzed context improves deadline prediction
- **Cost efficient**: Separates cheap context building from expensive reasoning
- **Parallel detection**: Historical sender patterns improve class identification

### Model Performance
- **Groq Reasoning (120B)**: Best accuracy, ~2-3s latency
- **Groq Standard (70B)**: Fast fallback, ~1-2s latency
- **Groq Vision (90B)**: Multimodal support, ~3-4s latency
- **Gemini Flash**: Reliable fallback, ~2-3s latency

---

<div align="center">

**Made with ❤️ and 🦀 Rust**

[⬆ Back to Top](#-marbot---academic-assignment-bot)

</div>
