# 🤖 MARBOT - Academic Assignment Bot

<div align="center">

```
███╗   ███╗ █████╗ ██████╗ ██████╗  ██████╗ ████████╗
████╗ ████║██╔══██╗██╔══██╗██╔══██╗██╔═══██╗╚══██╔══╝
██╔████╔██║███████║██████╔╝██████╔╝██║   ██║   ██║   
██║╚██╔╝██║██╔══██║██╔══██╗██╔══██╗██║   ██║   ██║   
██║ ╚═╝ ██║██║  ██║██║  ██║██████╔╝╚██████╔╝   ██║   
╚═╝     ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝  ╚═════╝    ╚═╝   
                                                     
         WhatsApp Academic Assistant v1.0          
```

[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![PostgreSQL](https://img.shields.io/badge/postgres-%23316192.svg?style=for-the-badge&logo=postgresql&logoColor=white)](https://www.postgresql.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)

**Never miss a deadline again.** An intelligent WhatsApp bot that automatically extracts, organizes, and reminds you about academic assignments using cutting-edge AI.

[Features](#-features) • [Installation](#-installation) • [Commands](#-commands) • [Architecture](#-architecture) • [Contributing](#-contributing)

</div>

---

## ✨ Features

### 🧠 **AI-Powered Intelligence**
- **Multi-Model Architecture**: Groq reasoning models (120B GPT-OSS) → Vision models → Gemini fallback
- **Smart Extraction**: Automatically detects assignments from natural language messages
- **Multimodal Support**: Processes both text and images (even when images are memes!)
- **Context-Aware**: Uses schedule data to predict deadlines intelligently
- **Duplicate Detection**: AI-powered duplicate checking prevents redundant entries

### 📚 **Academic Management**
- **Assignment Tracking**: Automatically captures course, title, deadline, description, and parallel code
- **Multiple Assignments**: Handles bulk announcements (e.g., "LKP 14, LKP 15, LKP 16 tomorrow")
- **Update Detection**: Recognizes assignment changes and clarifications
- **Clarification Flow**: Interactive system for incomplete assignment data
- **Schedule Oracle**: Predicts "before next meeting" deadlines using class schedules

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

### 5. Run Bot
```bash
# Development
cargo run

# Production (optimized)
cargo build --release
./target/release/marbot
```

### 6. Configure WAHA Webhook
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
WhatsApp Message → WAHA → Webhook → Marbot → AI Processing → Database
                                      ↓
                                  Scheduler → Reminders → WhatsApp
```

### AI Pipeline
```
1. Context Builder (Schedule Oracle + Parallel Detection)
   ↓
2. Classification (NEW/UPDATE/MULTIPLE/UNRECOGNIZED)
   ↓
3. Extraction (Course, Title, Deadline, Description, Parallel)
   ↓
4. Duplicate Check (Pre-filter + AI Verification)
   ↓
5. Database Storage
   ↓
6. Clarification (if incomplete) OR Success Notification
```

### Tech Stack
- **Framework**: Axum (async web framework)
- **Database**: PostgreSQL + SQLx (compile-time query verification)
- **Async Runtime**: Tokio
- **AI Models**: 
  - Groq (reasoning models for complex logic)
  - Gemini (fallback + matching logic)
- **Scheduling**: tokio-cron-scheduler
- **HTTP Client**: reqwest

---

## 🎯 How It Works

### 1. Message Classification
```rust
// Classifier determines message type
"#tugas" → Command
"Ada tugas LKP 15 besok" → NeedsAI (assignment info)
"halo" → Unrecognized (ignored if not from academic channel)
```

### 2. AI Extraction
```
Message: "Pemrog LKP 15 dan Kalkulus Quiz 3 besok jam 10"
  ↓
AI detects: MULTIPLE_ASSIGNMENTS
  ↓
Extracts:
  1. Pemrograman - LKP 15 - 2026-01-04 10:00 - K1
  2. Kalkulus - Quiz 3 - 2026-01-04 10:00 - null
```

### 3. Context Enhancement
```
Message: "LKP 15 sebelum pertemuan selanjutnya"
  ↓
Schedule Oracle checks: Pemrograman K1 next class is Wednesday 08:00
  ↓
AI uses hint: deadline = 2026-01-08 08:00
```

### 4. Duplicate Detection
```
New: "LKP 15 - Recursion"
Existing in DB: "LKP 15 - Programming Lab 15"
  ↓
Pre-filter: Same course ✓, same number ✓, same type ✓
  ↓
AI verification: "High confidence duplicate" → UPDATE existing
```

---

## 🔧 Configuration

### Whitelist System
Only messages from whitelisted channels are processed (except commands):
```env
ACADEMIC_CHANNELS=120363xxxxx@newsletter,120363yyyyy@g.us
```

### Rate Limiting
Default: 5 commands per 30 seconds per user (configurable in `main.rs`)

### AI Model Selection
Models are tried in order (edit `ai_extractor/mod.rs`):
1. Groq Reasoning (openai/gpt-oss-120b)
2. Groq Vision (if image attached)
3. Groq Standard (llama-3.3-70b)
4. Gemini (fallback)

---

## 📊 Database Schema

### Core Tables
- **courses**: Course information with aliases
- **assignments**: Assignment details with deadline, description, parallel
- **user_completions**: Per-user completion status
- **wa_logs**: Webhook event logs

### Key Features
- UUID primary keys
- JSONB for flexible metadata
- Array columns for message_ids
- Foreign key constraints

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
- **Groq** - Lightning-fast inference
- **Google Gemini** - Reliable fallback model
- **Rust Community** - Amazing ecosystem

---

<div align="center">

**Made with ❤️ and 🦀 Rust**

[⬆ Back to Top](#-marbot---academic-assignment-bot)

</div>
