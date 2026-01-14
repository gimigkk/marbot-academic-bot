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
[![Supabase](https://img.shields.io/badge/Supabase-3ECF8E?style=for-the-badge&logo=supabase&logoColor=white)](https://supabase.com/)
[![Docker](https://img.shields.io/badge/docker-%230db7ed.svg?style=for-the-badge&logo=docker&logoColor=white)](https://www.docker.com/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)

**Never miss a deadline again.** An intelligent WhatsApp bot that automatically extracts, organizes, and reminds you about academic assignments using cutting-edge AI.

[Features](#-features) • [Installation](#-installation) • [Commands](#-commands) • [Architecture](#-architecture) • [Deployment](#-deployment)

</div>

---

## ✨ Features

### 🧠 **AI-Powered Intelligence**
- **Two-Stage AI Architecture**: Context Builder → Main Extractor for maximum accuracy
- **Multi-Model Fallback Chain**: 
  - **Primary**: Gemini (gemini-3-flash-preview, gemini-2.5-flash) - Best balance of speed and accuracy
  - **Reasoning**: Groq (openai/gpt-oss-120b, deepseek-r1-distill-qwen-32b) - Complex logic fallback
  - **Vision**: Groq (llama-4-scout-17b, llama-4-maverick-17b) - Multimodal support
  - **Fallback**: Groq Standard (llama-3.3-70b-versatile, llama-3.1-8b-instant)
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
- **Close to Deadline**: H-1 hour from the assignment deadline
- **Smart Prioritization**: Color-coded by urgency (🔴 today, 🟠 tomorrow, 🟡 2 days, 🟢 >2 days, ⚪ no deadline)
- **Humanized Dates**: "Hari ini", "Besok", "H-5" in Indonesian

### 🛡️ **Reliability & Safety**
- **Anti-Spam**: Rate limiting on commands (5 commands / 30 seconds)
- **Whitelist System**: Only processes assignments from authorized academic channels
- **Deduplication**: Message cache prevents duplicate processing
- **Error Recovery**: Graceful fallback through multiple AI models
- **Performance Monitoring**: Real-time latency tracking for AI and database operations
- **Health Checks**: Docker health monitoring for all services

---

## 🚀 Installation

### Prerequisites
```bash
# Required
Rust 1.70+
Docker & Docker Compose
WAHA (WhatsApp HTTP API)

# Services
Supabase Account (free tier available - managed PostgreSQL)

# API Keys
Groq API Key (free tier available)
Gemini API Key (free tier available)
```

### Method 1: Docker Deployment (Recommended)

#### 1. Clone Repository
```bash
git clone https://github.com/gimigkk/marbot-academic-bot.git
cd marbot-academic-bot
```

#### 2. Configure Environment
Create a `.env` file:
```env
# Database (Supabase)
DATABASE_URL=postgresql://postgres:[YOUR-PASSWORD]@[YOUR-PROJECT-REF].supabase.co:5432/postgres

# AI Models
GROQ_API_KEY=gsk_your_groq_api_key
GEMINI_API_KEY=your_gemini_api_key

# WhatsApp (WAHA)
WAHA_URL=http://waha:3000
WAHA_API_KEY=your_waha_api_key

# Channels (comma-separated)
ACADEMIC_CHANNELS=120363xxxxxx@newsletter,120363yyyyyy@g.us
DEBUG_GROUP_ID=120363zzzzzz@g.us

# Dozzle (Log Viewer)
DOZZLE_USERNAME=admin
DOZZLE_PASSWORD=your_secure_password

# Logging
RUST_LOG=info
```

> **Getting Supabase DATABASE_URL:**
> 1. Go to your [Supabase Dashboard](https://app.supabase.com)
> 2. Select your project
> 3. Go to Settings → Database
> 4. Copy the "Connection string" under "Connection pooling" (use Transaction mode)
> 5. Replace `[YOUR-PASSWORD]` with your database password

#### 3. Add Schedule Data
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

#### 4. Configure Database
Run migrations on your Supabase database:
```bash
# Install sqlx-cli if not already installed
cargo install sqlx-cli --no-default-features --features postgres

# Run migrations (ensure DATABASE_URL is set in .env)
sqlx migrate run
```

Alternatively, you can run migrations manually in the Supabase SQL Editor:
1. Go to your Supabase Dashboard → SQL Editor
2. Run the migration files from `migrations/` folder in order

#### 5. Start Services
```bash
docker compose up -d
```

#### 6. Monitor Services
- **Backend logs**: `docker compose logs -f backend`
- **WAHA logs**: `docker compose logs -f waha`
- **Dozzle UI**: http://localhost:8080 (view all logs in browser)
- **Health check**: http://localhost:3000/health

#### 7. Configure WAHA
1. Open WAHA at http://localhost:3001
2. Scan QR code with WhatsApp
3. Webhook is automatically configured to `http://backend:3000/webhook`

### Method 2: Manual Installation

#### 1-3. Same as Docker method (Clone, Environment, Schedule)

#### 4. Database Setup
```bash
# Install sqlx-cli if not already installed
cargo install sqlx-cli --no-default-features --features postgres

# Run migrations (ensure DATABASE_URL points to Supabase)
sqlx migrate run
```

Or use Supabase SQL Editor:
1. Go to Supabase Dashboard → SQL Editor
2. Run migration files from `migrations/` folder

#### 5. Configure Course Aliases
Add courses via Supabase SQL Editor or psql:
```sql
INSERT INTO courses (name, aliases) VALUES 
  ('KOM120C - Pemrograman', ARRAY['Pemrog', 'Programming', 'Prog']),
  ('MAT101 - Kalkulus', ARRAY['Calc', 'Kalkul', 'Calculus']);
```

#### 6. Run Bot
```bash
# Development
cargo run

# Production (optimized)
cargo build --release
./target/release/marbot
```

#### 7. Configure WAHA Webhook
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

### Admin Commands (Debug Channel Only)
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

### Docker Services Architecture
```
┌─────────────────────────────────────────┐
│              marbot_network             │
├─────────────────────────────────────────┤
│                                         │
│  ┌──────────────┐    ┌──────────────┐   │
│  │   Backend    │◄───┤     WAHA     │   │
│  │ (Rust/Axum)  │    │  (WhatsApp)  │   │
│  │  Port 3000   │    │  Port 3001   │   │
│  └──────┬───────┘    └──────────────┘   │
│         │                               │
│         │             ┌──────────────┐  │
│         └────────────►│   Dozzle     │  │
│                       │ (Log Viewer) │  │
│                       │  Port 8080   │  │
│                       └──────────────┘  │
│                                         │
└─────────────────────────────────────────┘
           │
           ▼
   Supabase PostgreSQL
   (Managed Database)
```

### Two-Stage AI Pipeline

#### **Stage 1: Context Builder** (Lightweight & Fast)
```
User Message + Sender History + Course List + Quoted Message
   ↓
Groq Standard Text Models (llama-3.3-70b-versatile)
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

#### **Stage 2: Main Extractor** (Comprehensive Analysis)
```
MessageContext + Original Message + Quoted Context
   ↓
AI Model Selection (tries in order):
  1. Gemini (gemini-3-flash-preview, gemini-2.5-flash) - PRIORITY
  2. Groq Reasoning (openai/gpt-oss-120b, deepseek-r1) - Complex logic
  3. Groq Vision (llama-4-scout-17b) - If image present
  4. Groq Standard (llama-3.3-70b-versatile) - Fast fallback
   ↓
Classification:
  • NEW: Single assignment
  • UPDATE: Modification to existing (uses quoted context)
  • MULTIPLE: Bulk assignments
  • UNRECOGNIZED: Not an assignment
   ↓
Extraction + Duplicate Check + Database Storage
```

### Tech Stack
- **Framework**: Axum (async web framework)
- **Database**: Supabase (Managed PostgreSQL) + SQLx (compile-time query verification)
- **Async Runtime**: Tokio
- **Container Platform**: Docker + Docker Compose
- **AI Models**: 
  - **Gemini** (gemini-3-flash-preview, gemini-2.5-flash) - PRIMARY
  - **Groq Reasoning** (openai/gpt-oss-120b) - 120B parameter reasoning model
  - **Groq Vision** (llama-4-scout-17b) - Multimodal support
  - **Groq Standard** (llama-3.3-70b-versatile) - Fast text processing
- **Scheduling**: tokio-cron-scheduler
- **HTTP Client**: reqwest
- **Log Monitoring**: Dozzle (real-time log viewer)

---

## 🚢 Deployment

### Automated Deployment with GitHub Actions

MARBOT uses GitHub Actions for automated deployment. Every push to main automatically updates your production VPS.

#### Setup GitHub Secrets

Add these secrets to your repository (Settings → Secrets and variables → Actions):

```bash
VPS_HOST=your.vps.ip.address
VPS_PORT=22
VPS_USERNAME=your_username
VPS_SSH_KEY=your_private_ssh_key
WAHA_URL=http://your.vps.ip:3001
WAHA_API_KEY=your_waha_api_key
DEBUG_GROUP_ID=your_whatsapp_group_id
```

#### Initial VPS Setup (One-time)

1. **Prepare your VPS**
   ```bash
   # Install Docker and Docker Compose
   curl -fsSL https://get.docker.com -o get-docker.sh
   sudo sh get-docker.sh
   sudo apt install docker-compose-plugin -y
   
   # Create project directory
   sudo mkdir -p /opt/marbot-academic-bot
   sudo chown $USER:$USER /opt/marbot-academic-bot
   ```

2. **Clone repository**
   ```bash
   cd /opt/marbot-academic-bot
   git clone https://github.com/gimigkk/marbot-academic-bot.git .
   ```

3. **Configure environment**
   ```bash
   # Create .env file with your credentials
   nano .env
   ```
   
   Add your configuration (see [Environment Configuration](#2-configure-environment) section above)

4. **Add schedule data**
   ```bash
   nano schedule.json
   ```
   
   Add your class schedule (see [Schedule Data](#3-add-schedule-data) section above)

5. **Run migrations**
   ```bash
   # Install sqlx-cli
   cargo install sqlx-cli --no-default-features --features postgres
   
   # Run migrations
   sqlx migrate run
   ```

6. **Initial deployment**
   ```bash
   chmod +x update.sh
   ./update.sh
   ```

7. **Configure WAHA**
   - Open http://your-vps-ip:3001
   - Scan QR code with WhatsApp
   - Webhook is automatically configured

#### Deploy to Production

Once initial setup is complete, deploying is as simple as:

```bash
git add .
git commit -m "your changes"
git push origin main
```

**GitHub Actions automatically**:
- ✅ SSHs into your VPS
- ✅ Pulls latest code (handles force pushes)
- ✅ Rebuilds backend Docker container
- ✅ Preserves WAHA session (no QR rescan needed)
- ✅ Runs health checks
- ✅ Sends WhatsApp notification with commit details (+additions/-deletions)

#### Monitor Deployments

```bash
# GitHub Actions dashboard
https://github.com/your-repo/actions

# WhatsApp notification (automatic in debug group)
# Format: ✅ MARBOT UPDATE SUCCESS
#         Commit: "your message"
#         #abc1234 by Author +10 -5

# VPS logs
tail -f /var/log/marbot-update.log

# Docker logs
docker compose logs -f backend

# Web-based logs (Dozzle)
http://your-vps-ip:8080

# Health check
curl http://localhost:3000/health
```

---

### Manual Deployment (Alternative)

If you prefer not to use GitHub Actions, you can deploy manually:

```bash
# SSH into your VPS
ssh user@your-vps-ip

# Navigate to project directory
cd /opt/marbot-academic-bot

# Run update script
./update.sh
```

The `update.sh` script handles everything automatically.

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
Context Output includes schedule time for each parallel
```

### 2. Main Extraction (Stage 2)
```
Context + Message → Gemini (Primary)
  ↓
Classification: NEW_ASSIGNMENT
  ↓
Extraction:
  • Course: KOM120C - Pemrograman ✓
  • Title: LKP 15
  • Deadline: 2026-01-08 08:00 ✓ (from context hint)
  • Parallel: K1 ✓
  • Description: Lab assignment 15
```

### 3. Duplicate Detection Flow
```
New: "LKP 15 - Recursion"
  ↓
Pre-filter: Course match + Parallel overlap + Number match
  ↓
AI Verification (Gemini): High confidence required
  ↓
Result: UPDATE existing or CREATE new
```

---

## 🔧 Configuration

### Model Selection Priority

**Stage 1 (Context Builder):**
- Groq Standard: llama-3.3-70b-versatile, llama-3.1-8b-instant

**Stage 2 (Main Extractor):**
1. **Gemini** (PRIMARY): gemini-3-flash-preview, gemini-2.5-flash
2. **Groq Reasoning**: openai/gpt-oss-120b, deepseek-r1-distill-qwen-32b
3. **Groq Vision**: llama-4-scout-17b (if image present)
4. **Groq Standard**: llama-3.3-70b-versatile (fallback)

**Matching & Deduplication:**
- Gemini only (gemini-3-flash-preview, gemini-2.5-flash)

### Health Monitoring

- **Backend**: `/health` endpoint
- **Docker health checks**: Every 30s
- **Dozzle dashboard**: Real-time log monitoring at port 8080

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
- Test with Docker Compose locally
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

- **Supabase** - Managed PostgreSQL database platform
- **WAHA** - WhatsApp HTTP API
- **Google Gemini** - Primary AI model for fast and reliable extraction
- **Groq** - Lightning-fast inference with reasoning models
- **Docker** - Containerization platform
- **Rust Community** - Amazing ecosystem

---

## 📈 Performance Notes

### AI Model Performance

- **Gemini Flash (PRIMARY)**: Best balance, ~2-3s latency, 95% success rate
- **Groq Reasoning (120B)**: Complex logic, ~2-3s latency, rare fallback
- **Groq Vision (17B)**: Multimodal support, ~3-4s latency (when image present)
- **Groq Standard (70B)**: Fast fallback, ~1-2s latency

### Deployment Performance

- **Zero-downtime updates**: WAHA session persists across backend rebuilds
- **Automatic health checks**: 30s interval with 40s startup grace period
- **Rate limiting**: 5 commands/30s per user prevents spam
- **Message deduplication**: In-memory cache prevents duplicate processing

---

<div align="center">

**Made with ❤️ and 🦀 Rust**

[⬆ Back to Top](#-marbot---academic-assignment-bot)

</div>
