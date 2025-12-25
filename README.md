# MARBOT - WhatsApp Academic Bot 🤖

A Rust bot that reads WhatsApp messages from academic channels and automatically saves assignment info to a database using AI.
```
                      ███╗   ███╗ █████╗ ██████╗ ██████╗  ██████╗ ████████╗
                      ████╗ ████║██╔══██╗██╔══██╗██╔══██╗██╔═══██╗╚══██╔══╝
                      ██╔████╔██║███████║██████╔╝██████╔╝██║   ██║   ██║   
                      ██║╚██╔╝██║██╔══██║██╔══██╗██╔══██╗██║   ██║   ██║   
                      ██║ ╚═╝ ██║██║  ██║██║  ██║██████╔╝╚██████╔╝   ██║   
                      ╚═╝     ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝  ╚═════╝    ╚═╝   
                                                                                                                                                                                                                              
                             [🤖 WhatsApp Academic Assistant v1.0]
                                       by Gilang & Arya
```

## What It Does

- Reads messages from WhatsApp channels/groups (via WAHA)
- Uses Gemini AI to understand assignment info (course, deadline, description)
- Saves assignments to PostgreSQL database
- Responds with bot commands like `#tugas` to list assignments
- Prevents duplicate assignments
- Only processes messages from whitelisted channels

## How It Works

```
    [WhatsApp Message]
            │
   WAHA (sends webhook)
            │
 Bot Server (Axum :3000)
            │
         Command?
   ┌─no─────┴─────yes──┐
   │                   │
Whitelist Check        │
   │                   │
Gemini AI           Execute
   │                   │
   ┕──── PostgreSQL ───┘
             │
      [Reply via WAHA]
```
```
marbot-academic-bot/
├── backend/                    # Core Rust backend service
│   ├── migrations/             # SQL database migrations
│   │   ├── *_init_schema.up.sql
│   │   └── *_init_schema.down.sql
│   │
│   ├── src/
│   │   ├── main.rs             # Application entry point & wiring
│   │   │
│   │   ├── classifier.rs       # Message type classification (command, task, ignore, etc.)
│   │   ├── models.rs           # Domain models shared across the application
│   │   ├── scheduler.rs        # Background jobs (reminders, periodic tasks)
│   │   ├── whitelist.rs        # Access control for users / groups
│   │   │
│   │   ├── database/           # Database access layer
│   │   │   ├── pool.rs         # PostgreSQL connection pool setup
│   │   │   ├── crud.rs         # Database queries and mutations
│   │   │   └── mod.rs
│   │   │
│   │   └── parser/             # Message parsing layer
│   │       ├── commands.rs     # Deterministic command parsing (#done, #expand, etc.)
│   │       ├── ai_extractor.rs # AI-assisted extraction from free-form messages
│   │       └── mod.rs
│   │
│   ├── .env                    # Local environment variables (not committed)
│   ├── Cargo.toml              # Rust project manifest
│   ├── Cargo.lock
│   └── README.md               # Backend-specific notes (if any)
│
├── waha/                       # WhatsApp HTTP API (external dependency)
│   ├── .waha/                  # WAHA session data (not committed)
│   └── waha_data/              # Runtime data / volumes
│
├── .gitignore
└── README.md                   # Project documentation (this file)
```

## Quick Start

### Prerequisites
- Docker installed
- Rust toolchain (`rustup`)
- PostgreSQL database
- WhatsApp account for bot

### 1. Set Up Database

```bash
cd backend
cp .env.example .env
# Edit .env with your database credentials
sqlx migrate run
```

### 2. Start WAHA Container

```bash
cd ../waha

sudo docker run -d \
  --name waha \
  -p 3001:3000 \
  -e WAHA_API_KEY=devkey123 \
  -e WHATSAPP_HOOK_URL=http://172.17.0.1:3000/webhook \
  -e WHATSAPP_HOOK_EVENTS=message.any \
  -v "$(pwd)/.waha:/app/.waha" \
  devlikeapro/waha
```

### 3. Create WhatsApp Session & Scan QR

```bash
curl -X POST http://localhost:3001/api/sessions \
  -H "Content-Type: application/json" \
  -H "X-Api-Key: devkey123" \
  -d '{
    "name": "default",
    "config": {
      "webhooks": [
        {
          "url": "http://172.17.0.1:3000/webhook",
          "events": ["message.any"]
        }
      ]
    }
  }'

# Get QR code
curl -X GET http://localhost:3001/api/default/auth/qr \
  -H "X-Api-Key: devkey123"
```

Scan the QR code with your WhatsApp account.

### 4. Start Rust Backend

```bash
cd ../backend
cargo run
```

**Expected output:**
```
🚀 Starting WhatsApp Academic Bot
👂 Listening on 0.0.0.0:3000
```

### 5. Test the Bot

Send from WhatsApp:
```
#ping
```
## Usage

### Bot Commands

Commands can be sent from **any chat** (DM, group, or channel):

| Command | Description | Example |
|---------|-------------|---------|
| `#ping` | Check if bot is alive | `#ping` |
| `#tugas` | List all assignments | `#tugas` |
| `#tugas <id> or #<id>` | Forward original assignment details | `#tugas 1 or just #1` |
| `#done <id>` | Mark assignment complete | `#done 1` |
| `#help` | Show help message | `#help` |

### Natural Language Messages

The bot automatically processes messages from **whitelisted channels only**:

**Creating Assignments:**
```
Tugas Pemrograman Bab 2 K1 deadline 2025-12-31
Dikerjakan individu
```

**Updating Assignments:**
```
Pemrograman Bab 2 deadline diperpanjang jadi 2026-01-05
```

```
LKP 13 GKV dikumpulkan hari ini
```

### AI Classification Logic

The bot uses Gemini AI to classify messages into three types:

1. **AssignmentInfo** (New Assignment)
   - Extracts: course name, title, deadline, description, parallel code
   - Example: "Tugas Matematika Diskrit deadline 2025-12-25"

2. **AssignmentUpdate** (Update Existing)
   - Extracts: reference keywords, changes, new deadline
   - Matches to existing assignment using AI
   - Example: "Matdis deadline diperpanjang jadi 2026-01-10"

3. **Unrecognized**
   - Messages that don't contain academic info
   - Ignored by the bot

## How It Works

1. **Message Arrives**: WAHA sends webhook to bot
2. **Duplicate Check**: Skip if already processed
3. **Whitelist Check**: Only process if from academic channel (or is a command)
4. **AI Classification**: Gemini determines if it's assignment info
5. **Database**: Save or update assignment
6. **Reply**: Confirm via WhatsApp

## Troubleshooting

**Bot not responding?**
- Check WAHA is running
- Verify webhook URL is correct
- Check `WAHA_API_KEY`

**Messages ignored?**
- Make sure chat ID is in `ACADEMIC_CHANNELS`
- Commands work from anywhere, regular messages only from whitelisted channels

**Duplicate assignments?**
- Bot checks for duplicates by title + course
- If issue persists, check database logs
