
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

## Overview

MARBOT automatically extracts assignment details from WhatsApp group messages using a two-stage AI pipeline, resolves ambiguous deadlines using class schedules, and provides personalized task tracking with automated reminders. Built with Rust for reliability and performance.

**What it does:**
- Extracts assignments from natural language announcements
- Resolves vague deadlines like "submit before next class" using your schedule
- Detects and prevents duplicate entries with AI verification
- Tracks completion status per user
- Sends automated reminders for upcoming deadlines
- Handles bulk announcements and clarifications intelligently

---

## Features

### Intelligent Extraction
Two-stage AI pipeline processes WhatsApp messages to extract structured assignment data. The context builder analyzes sender history and quoted messages, while the main extractor parses course details, titles, descriptions, and deadlines.

### Deadline Resolution
Automatically resolves ambiguous deadlines by matching them against your class schedule. Converts phrases like "next class" or "this Friday" into concrete dates.

### Duplicate Prevention
Pre-filters by course parallel codes, then uses AI verification (95%+ confidence threshold) to prevent duplicate task entries.

### Personal Task Management
Each user maintains their own completion status. Commands provide different views: today's tasks, weekly overview, and full task list with urgency indicators.

### Automated Reminders
Background scheduler sends reminders 3 hours and 1 hour before deadlines to users who haven't marked tasks complete.

---

## Installation

### Prerequisites

| Component | Link | Notes |
|-----------|------|-------|
| Docker & Docker Compose | [Install](https://docs.docker.com/get-docker/) | Required |
| Supabase Account | [Sign Up](https://supabase.com) | Free tier available |
| Groq API Key | [Console](https://console.groq.com) | 30 requests/min free |
| Gemini API Key | [Get Key](https://aistudio.google.com/app/apikey) | 15 requests/min free |
| WAHA Instance | [Docs](https://waha.devlike.pro/) | WhatsApp API gateway |

### Setup

```bash
# Clone repository
git clone https://github.com/gimigkk/marbot-academic-bot.git
cd marbot-academic-bot

# Configure environment
cp .env.example .env
# Edit .env with your credentials

# Set up class schedule
cp schedule.example.json schedule.json
# Edit schedule.json with your class times

# Run migrations
cargo install sqlx-cli --no-default-features --features postgres
sqlx migrate run

# Start services
docker compose up -d

# Configure WAHA
# Open http://localhost:3001 and scan QR code
```

### Access Points

- Backend API: `http://localhost:3000`
- TUI Dashboard: `http://localhost:3000/tui`
- Log Viewer: `http://localhost:8080`

---

## Configuration

### Environment Variables

```env
# Database (Supabase)
DATABASE_URL=postgresql://postgres:[PASSWORD]@[PROJECT].supabase.co:5432/postgres

# AI Models
GROQ_API_KEY=gsk_your_groq_api_key
GEMINI_API_KEY=your_gemini_api_key

# WhatsApp (WAHA)
WAHA_URL=http://waha:3000
WAHA_API_KEY=your_waha_api_key

# Channels (comma-separated group IDs)
ACADEMIC_CHANNELS=120363xxxxxx@g.us
DEBUG_GROUP_ID=120363zzzzzz@g.us

# Monitoring
DOZZLE_USERNAME=admin
DOZZLE_PASSWORD=secure_password
RUST_LOG=info
```

### Class Schedule

Configure `schedule.json` for deadline prediction:

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

---

## Commands

### Personal Management
| Command | Description |
|---------|-------------|
| `#todo` | View your task list with urgency indicators |
| `#today` | View tasks due today |
| `#week` | View tasks due this week |
| `#done <num>` | Mark task as complete |
| `#undo` | Undo last completion |

### Course Settings
| Command | Description |
|---------|-------------|
| `#setkelas Pemrograman k1 p2` | Set user's class parallels |
| `#mykelas` | View user's current settings |

### Information
| Command | Description |
|---------|-------------|
| `#<number>` | View assignment details + original message |
| `#tugas` | View all assignments |
| `#ping` | Check system health |

### Admin (Debug Group Only)
| Command | Description |
|---------|-------------|
| `#delete <num>` | Delete assignment |
| `#update <num>` | Re-parse with AI |

---

## Architecture

### AI Pipeline

```
WhatsApp Message
    ↓
Context Builder (Gemini/Groq)
├─ Sender history analysis
├─ Quoted message resolution
└─ Class schedule matching
    ↓
Main Extractor (Gemini/Groq)
├─ Course & parallel extraction
├─ Title & description parsing
└─ Deadline resolution
    ↓
Duplicate Detection (Gemini)
├─ Pre-filtering by parallel codes
└─ AI verification (95%+ confidence)
    ↓
PostgreSQL Storage
    ↓
Automated Reminders
```

### Technology Stack

| Component | Technology |
|-----------|-----------|
| Backend | Rust 1.92 (Axum) |
| Database | Supabase (PostgreSQL 16) |
| WhatsApp | WAHA (HTTP API) |
| AI Primary | Google Gemini |
| AI Fallback | Groq (DeepSeek, Llama) |
| Container | Docker Compose |
| Monitoring | Dozzle + TUI Dashboard |
| CI/CD | GitHub Actions |

### Project Structure

```
backend/
├── src/
│   ├── main.rs              # Entry point
│   ├── lib.rs               # Library interface
│   ├── models.rs            # Data structures
│   ├── classifier.rs        # Message classification
│   ├── clarification.rs     # Handles clarification messages
│   ├── scheduler.rs         # Reminder system
│   ├── whitelist.rs         # User access control
│   ├── database/
│   │   ├── mod.rs
│   │   ├── pool.rs          # Connection pooling
│   │   └── crud.rs          # Database operations
│   ├── parser/
│   │   ├── commands.rs      # Command handling
│   │   └── ai_extractor/    # AI extraction logic
│   ├── dashboard/           # Web dashboard
│   │   ├── handlers.rs      # HTTP handlers
│   │   ├── auth.rs          # Authentication
│   │   ├── client.js        # Frontend JavaScript
│   │   └── styles.css       # Dashboard styles
│   └── tui/                 # Terminal UI
│       ├── state.rs         # UI state management
│       └── logger.rs        # Logging interface
├── migrations/              # Database migrations
└── tests/                   # Unit tests

tests/
└── integration_tests.rs     # Integration tests

waha/
└── waha_data/               # WAHA persistent data

docker-compose.yml
update.sh                    # Deployment script
schedule.json                # Class schedule configuration
.github/workflows/deploy.yml
```

---

## Deployment

### Automated Deployment with GitHub Actions

Configure secrets in your repository settings:

```
VPS_HOST=your.vps.ip
VPS_PORT=22
VPS_USERNAME=user
VPS_SSH_KEY=private_key
WAHA_URL=http://your.vps.ip:3001
WAHA_API_KEY=api_key
```

Every push to `main` triggers:
1. Binary build (Debian Bookworm compatible)
2. Transfer to VPS with retry logic
3. Automated deployment with health checks
4. WhatsApp notification of deployment status

### Manual Deployment

```bash
ssh user@your-vps-ip
cd /opt/marbot-academic-bot
./update.sh
```

---

## Development

### Local Development

```bash
# Install dependencies
cargo build

# Watch mode
cargo watch -x run

# Run tests
cargo test

# Format code
cargo fmt

# Lint
cargo clippy
```

### Performance

- **AI Latency:** 2-3s (Gemini), 1-2s (Groq)
- **Success Rate:** ~95% with automatic retry
- **Rate Limits:** Anti-spam protection (5 commands/30s per user)
- **Optimizations:** Connection pooling, message deduplication, incremental builds

---

## Contributing

Contributions welcome. Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/improvement`)
3. Run `cargo fmt` and `cargo clippy`
4. Commit changes with clear messages
5. Open a pull request

---

## License

MIT License - see [LICENSE](LICENSE) file for details.

---

## Authors

Created by **Gilang & Arya**

Questions or issues? [Open an issue](https://github.com/gimigkk/marbot-academic-bot/issues)

---

<div align="center">

Built with Rust

</div>
