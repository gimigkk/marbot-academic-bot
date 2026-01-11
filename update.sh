#!/bin/bash

set -euo pipefail

LOG_FILE="/var/log/marbot-update.log"

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $1" | tee -a "$LOG_FILE"
}

send_whatsapp_message() {
    local message="$1"
    local chat_id="$2"
    local waha_url="$3"
    local api_key="$4"
    
    local temp_payload=$(mktemp)
    
    cat > "$temp_payload" <<-PAYLOAD
{
    "session": "default",
    "chatId": "$chat_id",
    "text": "$message"
}
PAYLOAD
    
    local response=$(curl -s -X POST "$waha_url/api/sendText" \
        -H "X-Api-Key: $api_key" \
        -H "Content-Type: application/json" \
        -d @"$temp_payload" 2>&1 || echo "curl_failed")
    
    log "📱 WhatsApp Response: $response"
    rm -f "$temp_payload"
}

rollback() {
    log "⚠️ Rolling back to previous version..."
    
    if [ ! -d "backend-backup" ]; then
        log "❌ No backup found, cannot rollback!"
        return 1
    fi
    
    docker compose down backend
    
    # Restore backed up backend directory
    rm -rf backend
    mv backend-backup backend
    
    # Build and start
    docker compose build --no-cache backend
    docker compose up -d backend
    
    log "✅ Rolled back successfully"
}

log "🔄 Starting MARBOT update..."

cd /opt/marbot-academic-bot || exit 1

if [ ! -f .env ]; then
    log "❌ Error: .env file not found!"
    exit 1
fi

log "📋 Loading environment variables..."
set -a
source .env
set +a

if [ -z "$DEBUG_GROUP_ID" ] || [ -z "$WAHA_URL" ] || [ -z "$WAHA_API_KEY" ]; then
    log "⚠️ Warning: WhatsApp notification env vars not set properly"
fi

# Get deployment mode from environment (set by GitHub Actions)
DEPLOY_MODE="${DEPLOY_MODE:-auto}"
log "🔧 Deployment mode: $DEPLOY_MODE"

# Get commit info from environment or git
if [ -n "${COMMIT_SHA:-}" ]; then
    NEW_COMMIT="$COMMIT_SHA"
    COMMIT_MSG="${COMMIT_MSG:-Update}"
    COMMIT_AUTHOR="${AUTHOR:-Unknown}"
    ADDITIONS="${ADDITIONS:-0}"
    DELETIONS="${DELETIONS:-0}"
else
    # Fallback to git if running manually
    OLD_COMMIT=$(git rev-parse --short HEAD)
    
    log "📥 Pulling latest code from GitHub..."
    git fetch origin
    
    LOCAL=$(git rev-parse @)
    REMOTE=$(git rev-parse @{u})
    BASE=$(git merge-base @ @{u})
    
    if [ "$LOCAL" = "$REMOTE" ]; then
        log "✅ Already up to date"
        
        UP_TO_DATE_MSG="*[ MARBOT UPDATE ]*
_Already up to date_

*[ \`${OLD_COMMIT}\` ]* 
No changes"
        
        send_whatsapp_message "$UP_TO_DATE_MSG" "$DEBUG_GROUP_ID" "$WAHA_URL" "$WAHA_API_KEY"
        exit 0
    elif [ "$LOCAL" = "$BASE" ]; then
        log "📥 Fast-forward update"
        git pull origin main
    else
        log "⚠️ Force push detected, resetting to origin/main"
        git reset --hard origin/main
        git clean -fd
    fi
    
    NEW_COMMIT=$(git rev-parse --short HEAD)
    COMMIT_MSG=$(git log -1 --pretty=format:"%s" HEAD)
    COMMIT_AUTHOR=$(git log -1 --pretty=format:"%an" HEAD)
    ADDITIONS=$(git show --shortstat | grep -oP '\d+(?= insertion)' || echo "0")
    DELETIONS=$(git show --shortstat | grep -oP '\d+(?= deletion)' || echo "0")
    
    # Auto-detect mode if not set
    if [ "$DEPLOY_MODE" = "auto" ]; then
        if [ -f "backend/marbot-new" ]; then
            DEPLOY_MODE="prebuilt"
        else
            DEPLOY_MODE="vps-build"
        fi
    fi
fi

log "📝 New commit: ${COMMIT_MSG}"
log "👤 Author: ${COMMIT_AUTHOR}"
log "📊 Changes: +${ADDITIONS} -${DELETIONS}"

# Create backup of current backend
if [ -d "backend" ]; then
    log "💾 Backing up current backend directory..."
    rm -rf backend-backup
    cp -r backend backend-backup
fi

# Handle deployment based on mode
if [ "$DEPLOY_MODE" = "prebuilt" ]; then
    # ============================================
    # PREBUILT BINARY MODE
    # ============================================
    log "📦 Using prebuilt binary from GitHub Actions..."
    
    if [ ! -f "backend/marbot-new" ]; then
        log "❌ Error: Prebuilt binary not found at backend/marbot-new!"
        exit 1
    fi
    
    if [ ! -f "backend/Dockerfile.prebuilt" ]; then
        log "❌ Error: backend/Dockerfile.prebuilt not found!"
        exit 1
    fi
    
    # Set executable permission
    chmod +x backend/marbot-new
    
    log "🔨 Building Docker image with prebuilt binary..."
    # Build directly with docker, specifying the correct Dockerfile
    if ! docker build -f backend/Dockerfile.prebuilt -t marbot-academic-bot-backend:latest ./backend; then
        log "❌ Docker build failed!"
        rollback
        
        FAIL_MSG="*[ MARBOT UPDATE FAILED ]*
_${COMMIT_MSG}_

*[ \`#${NEW_COMMIT}\` by ${COMMIT_AUTHOR} ]* +${ADDITIONS} -${DELETIONS}

❌ Docker build failed (prebuilt)
🔄 Rolled back to previous version"
        
        send_whatsapp_message "$FAIL_MSG" "$DEBUG_GROUP_ID" "$WAHA_URL" "$WAHA_API_KEY"
        exit 1
    fi
    
    BUILD_INFO="Built on GitHub Actions ⚡"
    
else
    # ============================================
    # VPS BUILD MODE (FALLBACK)
    # ============================================
    log "⚠️ Building on VPS (GitHub Actions build unavailable)..."
    
    if [ ! -f "backend/Dockerfile" ]; then
        log "❌ Error: backend/Dockerfile not found!"
        exit 1
    fi
    
    if [ ! -f "backend/Cargo.toml" ]; then
        log "❌ Error: backend/Cargo.toml not found! Source code incomplete."
        exit 1
    fi
    
    log "🔨 Building Docker image on VPS (this may take 10-15 minutes)..."
    # Build directly with docker, specifying the correct Dockerfile
    if ! docker build -f backend/Dockerfile -t marbot-academic-bot-backend:latest ./backend; then
        log "❌ VPS build failed!"
        rollback
        
        FAIL_MSG="*[ MARBOT UPDATE FAILED ]*
_${COMMIT_MSG}_

*[ \`#${NEW_COMMIT}\` by ${COMMIT_AUTHOR} ]* +${ADDITIONS} -${DELETIONS}

❌ VPS build failed
🔄 Rolled back to previous version"
        
        send_whatsapp_message "$FAIL_MSG" "$DEBUG_GROUP_ID" "$WAHA_URL" "$WAHA_API_KEY"
        exit 1
    fi
    
    BUILD_INFO="Built on VPS (fallback) 🔄"
fi

log "🚀 Restarting backend service..."
docker compose up -d --no-deps backend

log "🔍 Ensuring waha and dozzle are running..."
docker compose up -d --no-recreate waha dozzle

log "⏳ Waiting for backend to start..."
sleep 20

log "🏥 Checking backend health..."
HEALTH_CHECK_SUCCESS=false
for i in {1..10}; do
    if curl -sf http://localhost:3000/health > /dev/null 2>&1; then
        log "✅ Backend is healthy!"
        HEALTH_CHECK_SUCCESS=true
        break
    fi
    if [ $i -eq 10 ]; then
        log "❌ Backend health check failed!"
        docker compose logs backend --tail=50
        
        rollback
        
        FAIL_MSG="*[ MARBOT UPDATE FAILED ]*
_${COMMIT_MSG}_

*[ \`#${NEW_COMMIT}\` by ${COMMIT_AUTHOR} ]* +${ADDITIONS} -${DELETIONS}

❌ Backend health check failed
🔄 Rolled back to previous version"
        
        send_whatsapp_message "$FAIL_MSG" "$DEBUG_GROUP_ID" "$WAHA_URL" "$WAHA_API_KEY"
        exit 1
    fi
    log "⏳ Waiting for backend... (attempt $i/10)"
    sleep 5
done

log "📊 Service status:"
docker compose ps

if docker compose ps | grep -qE "(Exit|unhealthy)"; then
    log "❌ Some services are not healthy!"
    docker compose logs --tail=50
    
    rollback
    
    FAIL_MSG="*[ MARBOT UPDATE FAILED ]*
_${COMMIT_MSG}_

*[ \`#${NEW_COMMIT}\` by ${COMMIT_AUTHOR} ]* +${ADDITIONS} -${DELETIONS}

❌ Services not healthy
🔄 Rolled back to previous version"
    
    send_whatsapp_message "$FAIL_MSG" "$DEBUG_GROUP_ID" "$WAHA_URL" "$WAHA_API_KEY"
    exit 1
fi

log "✅ Update completed successfully!"

# Remove backup after successful deployment
rm -rf backend-backup

SUCCESS_MSG="*[ MARBOT UPDATE SUCCESS ]*
_${COMMIT_MSG}_

*[ \`#${NEW_COMMIT}\` by ${COMMIT_AUTHOR} ]* +${ADDITIONS} -${DELETIONS}
https://github.com/gimigkk/marbot-academic-bot/commit/${NEW_COMMIT}

${BUILD_INFO}"

log "📱 Sending success notification to WhatsApp..."
send_whatsapp_message "$SUCCESS_MSG" "$DEBUG_GROUP_ID" "$WAHA_URL" "$WAHA_API_KEY"

log "✅ Deployment complete!"

exit 0