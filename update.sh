#!/bin/bash

LOG_FILE="/var/log/marbot-update.log"

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $1" | tee -a "$LOG_FILE"
}

# Function to send WhatsApp message
send_whatsapp_message() {
    local message="$1"
    local chat_id="$2"
    local waha_url="$3"
    local api_key="$4"
    
    # Don't exit on curl errors
    set +e
    
    local response=$(curl -s -w "\n%{http_code}" -X POST "${waha_url}/api/sendText" \
        -H "X-Api-Key: ${api_key}" \
        -H "Content-Type: application/json" \
        -d @- << EOF
{
    "session": "default",
    "chatId": "${chat_id}",
    "text": "${message}"
}
EOF
)
    
    local http_code=$(echo "$response" | tail -n1)
    local body=$(echo "$response" | head -n-1)
    
    log "📱 WhatsApp API Status: $http_code"
    log "📱 WhatsApp API Response: $body"
    
    # Re-enable exit on error
    set -e
    
    # Return success regardless of API response (don't fail deployment)
    return 0
}

# Handle errors
handle_error() {
    local exit_code=$1
    local line_number=$2
    
    log "❌ Update failed at line $line_number with exit code $exit_code"
    
    local error_msg="❌ *MARBOT UPDATE FAILED*\\n\\n📅 $(date '+%Y-%m-%d %H:%M:%S')\\n🔴 Exit code: ${exit_code}\\n📍 Failed at line: ${line_number}\\n\\n"
    
    if [ -n "$COMMIT_MSG" ]; then
        # Escape backticks and quotes
        local safe_commit=$(echo "$COMMIT_MSG" | sed 's/`/'"'"'/g' | sed 's/"/\\"/g')
        error_msg+="📝 Attempted commit:\\n\\\`\\\`\\\`${safe_commit}\\\`\\\`\\\`\\n\\n"
    fi
    
    error_msg+="🔍 Check logs: tail -f /var/log/marbot-update.log"
    
    # Load env vars if not already loaded
    if [ -f /opt/marbot-academic-bot/.env ]; then
        set -a
        source /opt/marbot-academic-bot/.env 2>/dev/null || true
        set +a
    fi
    
    if [ -n "$DEBUG_GROUP_ID" ] && [ -n "$WAHA_URL" ] && [ -n "$WAHA_API_KEY" ]; then
        log "📱 Sending failure notification to WhatsApp..."
        send_whatsapp_message "$error_msg" "$DEBUG_GROUP_ID" "$WAHA_URL" "$WAHA_API_KEY" || true
    else
        log "⚠️  Cannot send WhatsApp notification: Missing env vars"
    fi
}

# Set up error trap
set -E
trap 'handle_error $? $LINENO' ERR

log "🔄 Starting MARBOT update..."

# Navigate to project directory
cd /opt/marbot-academic-bot

# Check if .env exists
if [ ! -f .env ]; then
    log "❌ Error: .env file not found!"
    exit 1
fi

# Load environment variables
log "📋 Loading environment variables..."
set -a
source .env
set +a

# Verify required env vars
if [ -z "$DEBUG_GROUP_ID" ] || [ -z "$WAHA_URL" ] || [ -z "$WAHA_API_KEY" ]; then
    log "⚠️  Warning: WhatsApp notification env vars not set"
fi

# Get current commit before update
OLD_COMMIT=$(git rev-parse --short HEAD)

# Discard any local changes
log "🧹 Discarding local changes..."
git reset --hard HEAD
git clean -fd

# Pull latest code
log "📥 Pulling latest code from GitHub..."
git fetch origin

# Check if it's a force push
LOCAL=$(git rev-parse @)
REMOTE=$(git rev-parse @{u})
BASE=$(git merge-base @ @{u})

if [ "$LOCAL" = "$REMOTE" ]; then
    log "✅ Already up to date"
    
    # Send "already up to date" message
    up_to_date_msg="ℹ️ *MARBOT UPDATE*\\n\\n📅 $(date '+%Y-%m-%d %H:%M:%S')\\n✅ Already up to date\\n\\n📌 Current commit: \\\`${OLD_COMMIT}\\\`"
    
    log "📱 Sending 'up to date' notification..."
    send_whatsapp_message "$up_to_date_msg" "$DEBUG_GROUP_ID" "$WAHA_URL" "$WAHA_API_KEY" || true
    
    # Disable error trap before exit
    trap - ERR
    exit 0
elif [ "$LOCAL" = "$BASE" ]; then
    log "📥 Fast-forward update"
    git pull origin main
else
    log "⚠️  Force push detected, resetting to origin/main"
    git reset --hard origin/main
    git clean -fd
fi

# Get new commit info
NEW_COMMIT=$(git rev-parse --short HEAD)
COMMIT_MSG=$(git log -1 --pretty=format:"%s" HEAD)
COMMIT_AUTHOR=$(git log -1 --pretty=format:"%an" HEAD)

log "📝 New commit: ${COMMIT_MSG}"
log "👤 Author: ${COMMIT_AUTHOR}"

# Only rebuild and restart backend
log "🔨 Rebuilding backend container..."
docker compose build --no-cache backend

log "🚀 Restarting backend service only..."
docker compose up -d --no-deps backend

# Ensure waha and dozzle are still running
log "🔍 Ensuring waha and dozzle are running..."
docker compose up -d --no-recreate waha dozzle

# Wait for backend to be healthy
log "⏳ Waiting for backend to start..."
sleep 20

# Check backend health
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
log "🎉 Backend restarted, waha session preserved!"

# Prepare success message
# Escape special characters for JSON
SAFE_COMMIT=$(echo "$COMMIT_MSG" | sed 's/`/'"'"'/g' | sed 's/"/\\"/g')
SAFE_AUTHOR=$(echo "$COMMIT_AUTHOR" | sed 's/"/\\"/g')

success_msg="✅ *MARBOT UPDATE SUCCESS*\\n\\n📅 $(date '+%Y-%m-%d %H:%M:%S')\\n🔄 ${OLD_COMMIT} → ${NEW_COMMIT}\\n\\n📝 Commit message:\\n\\\`\\\`\\\`${SAFE_COMMIT}\\\`\\\`\\\`\\n\\n👤 Author: ${SAFE_AUTHOR}\\n🎉 Backend restarted successfully!"

log "📱 Sending success notification to WhatsApp..."
send_whatsapp_message "$success_msg" "$DEBUG_GROUP_ID" "$WAHA_URL" "$WAHA_API_KEY" || true

log "📱 Notification sent (or attempted)!"

# Disable error trap before clean exit
trap - ERR

exit 0