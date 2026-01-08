#!/bin/bash

set -e  # Exit on error

LOG_FILE="/var/log/marbot-update.log"

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $1" | tee -a "$LOG_FILE"
}

log "🔄 Starting MARBOT update..."

# Navigate to project directory
cd /opt/marbot-academic-bot

# Check if .env exists
if [ ! -f .env ]; then
    log "❌ Error: .env file not found!"
    exit 1
fi

# Pull latest code
log "📥 Pulling latest code from GitHub..."
git pull origin main

# Load environment variables for the build
set -a
source .env
set +a

# Rebuild and restart
log "🔨 Rebuilding containers..."
docker compose down

# Build with proper args
log "🏗️  Building backend with DATABASE_URL..."
docker compose build --no-cache backend

log "🚀 Starting all services..."
docker compose up -d

# Wait for services to be healthy
log "⏳ Waiting for services to start..."
sleep 20

# Check backend health
log "🏥 Checking backend health..."
for i in {1..10}; do
    if curl -sf http://localhost:3000/health > /dev/null 2>&1; then
        log "✅ Backend is healthy!"
        break
    fi
    if [ $i -eq 10 ]; then
        log "❌ Backend health check failed!"
        docker compose logs backend --tail=50
        exit 1
    fi
    log "⏳ Waiting for backend... (attempt $i/10)"
    sleep 5
done

# Check if all services are running
log "📊 Service status:"
docker compose ps

if docker compose ps | grep -qE "(Exit|unhealthy)"; then
    log "❌ Some services are not healthy!"
    docker compose logs --tail=50
    exit 1
fi

log "✅ Update completed successfully!"
log "🎉 All services are running and healthy!"