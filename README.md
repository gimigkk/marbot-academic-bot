# Marbot - WhatsApp Academic Info Manager Bot

A WhatsApp-based academic assistant that monitors assignment channels, extracts tasks, and allows students to query their assignments through personal chat commands. No mobile app required—everything happens through WhatsApp messages.

## 📋 Overview

### What It Does
- **Listens** to academic WhatsApp channels/groups
- **Extracts** assignment information automatically
- **Stores** tasks in a central database
- **Responds** to student queries via command-based chat

### Architecture

```
WhatsApp Message → WAHA (Docker) → Webhook → Rust Server → Database
                                                    ↓
WhatsApp Reply ← WAHA API ← Response Formatter ← Business Logic
```

**Components:**
- **WAHA**: WhatsApp HTTP API (Docker, port 3001)
- **Rust Backend**: Axum webhook server (port 3000)
- **PostgreSQL**: Assignment and user data storage

---

## 🚀 Quick Start

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

You should receive: `pong`

---

## 📂 Project Structure

```
whatsapp-academic-bot/
├── backend/
│   ├── Cargo.toml
│   ├── .env.example
│   ├── migrations/
│   └── src/
│       ├── main.rs             # Entry point
│       ├── models.rs           # Database structs
│       ├── db.rs               # Database queries
│       ├── webhook.rs          # Webhook handler
│       ├── waha.rs             # WAHA API client
│       ├── classifier.rs       # Message type detection
│       ├── parser/             # Parse commands & assignments
│       ├── handlers/           # Business logic
│       └── responses.rs        # Format replies
└── waha/
    ├── docker-compose.yml
    └── .waha/                  # Session data (gitignored)
```

---

## 🔄 Message Flow

```
WhatsApp Message
    ↓
webhook.rs (receive)
    ↓
classifier.rs (identify type)
    ↓
parser/ (extract data)
    ↓
handlers/ (business logic)
    ↓
db.rs (database)
    ↓
responses.rs (format)
    ↓
waha.rs (send reply)
```

---

## 💬 Supported Commands

| Command | Description | Example |
|---------|-------------|---------|
| `#help` | Show available commands | `#help` |
| `#recap` | List all pending tasks | `#recap` |
| `#expand N` | Show detailed info for task N | `#expand 3` |
| `#done N` | Mark task N as completed | `#done 5` |
| `#today` | Show tasks due today | `#today` |
| `#week` | Show tasks due this week | `#week` |

**Note:** Commands are case-insensitive

---

## 🧪 Development Workflow

### Local Testing (Without WhatsApp)

```bash
curl -X POST http://localhost:3000/webhook \
  -H "Content-Type: application/json" \
  -d '{
    "event": "message.any",
    "session": "default",
    "payload": {
      "body": "#ping",
      "from": "6281234567890@c.us"
    }
  }'
```

### Local Testing (With WhatsApp)

1. Start WAHA locally
2. Start Rust backend
3. Scan QR code with your phone
4. Send test messages to bot number

### Git Workflow

```bash
git pull origin main
git checkout -b feature/new-command
# Make changes and test
git add .
git commit -m "Add new feature"
git push origin feature/new-command
```

---

## 🔧 Troubleshooting

### WAHA Not Sending Webhooks

```bash
# Check logs
sudo docker logs waha -f

# Test connectivity from Docker
sudo docker exec -it waha curl -X POST http://172.17.0.1:3000/webhook \
  -H "Content-Type: application/json" \
  -d '{"event":"message.any","payload":{"body":"#ping","from":"test"}}'
```

### Rust Not Receiving Messages

```bash
# Check if port is in use
sudo lsof -i :3000

# Verify backend is running
curl http://localhost:3000/health
```

### 401 Unauthorized

Verify API key matches in both WAHA and Rust code:
```rust
.header("X-Api-Key", "devkey123")
```

### Database Connection Errors

```bash
# Test connection
psql $DATABASE_URL

# Check .env file
cat backend/.env
```

---

## 🔑 Configuration

### Environment Variables (.env)

```env
DATABASE_URL=postgresql://user:password@localhost/academic_bot
WAHA_URL=http://localhost:3001
WAHA_API_KEY=devkey123
RUST_LOG=info
```

### Key Concepts

**Why `172.17.0.1`?**  
Docker containers can't reach `localhost`. This is the host machine IP from Docker's bridge network.

**Message Deduplication**  
WAHA may send duplicate events. We track message IDs to process each message once.

---

## 🚀 Production Deployment

### VPS Setup

```bash
# Install dependencies
sudo apt update
sudo apt install -y docker.io postgresql build-essential

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone and build
git clone <repository-url>
cd whatsapp-academic-bot/backend
cargo build --release

# Start services
cd ../waha && docker-compose up -d
cd ../backend && ./target/release/academic-bot
```

### Production Checklist

- [ ] Use strong API keys
- [ ] Enable HTTPS
- [ ] Set up logging and monitoring
- [ ] Configure automatic backups
- [ ] Use dedicated WhatsApp number
- [ ] Set rate limits

---

## 🛑 Stopping & Restarting

**Stop everything:**
```bash
# Stop backend: Ctrl+C
# Stop WAHA
sudo docker stop waha
```

**Restart after reboot:**
```bash
sudo docker start waha
cd backend && cargo run
```

**If session lost:** Repeat QR scan (Step 3)

---

## 📖 Useful Commands

```bash
# Check ports
sudo lsof -i :3000
sudo lsof -i :3001

# Test WAHA API
curl -X POST http://localhost:3001/api/sendText \
  -H "Content-Type: application/json" \
  -H "X-Api-Key: devkey123" \
  -d '{
    "chatId": "6281234567890@c.us",
    "text": "Test message",
    "session": "default"
  }'

# Check database
psql $DATABASE_URL -c "SELECT * FROM assignments;"

# View dependencies
cd backend && cargo tree
```

---

## 🎯 Roadmap

- [x] Basic command handling
- [x] Message deduplication
- [ ] Assignment parsing
- [ ] Task completion tracking
- [ ] Due date notifications
- [ ] Multi-user support
- [ ] Admin commands
- [ ] Export to calendar

---

## ⚠️ Disclaimer

This bot is for educational purposes. Ensure compliance with WhatsApp Terms of Service and your institution's policies. Automated messaging may risk account suspension.

---

**Last Updated**: December 21, 2024  
**Status**: ✅ Core functionality working  
**Version**: 0.1.0
