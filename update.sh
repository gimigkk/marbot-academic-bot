#!/bin/bash

set -e  # Exit on error

LOG_FILE="/var/log/marbot-update.log"

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $1" | tee -a "$LOG_FILE"
}

log "🔄 Starting MARBOT update..."

# Navigate to project directory
cd /opt/marbot-academic-bot

# Pull latest code
log "📥 Pulling latest code from GitHub..."
git pull origin main

# Rebuild and restart
log "🔨 Rebuilding containers..."
docker compose down
docker compose up -d --build

# Wait for services to be healthy
log "⏳ Waiting for services to start..."
sleep 15

# Check if services are running
if docker compose ps | grep -q "Up"; then
    log "✅ Update completed successfully!"
    docker compose ps
else
    log "❌ Update failed! Check logs with: docker compose logs"
    exit 1
fi