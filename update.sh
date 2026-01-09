#!/bin/bash

LOG_FILE="/var/log/marbot-update.log"

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $1" | tee -a "$LOG_FILE"
}

# Function to send WhatsApp message - simplified to avoid trap issues
send_whatsapp_message() {
    local message="$1"
    local chat_id="$2"
    local waha_url="$3"
    local api_key="$4"
    
    # Create temp file for payload
    local temp_payload=$(mktemp)
    
    cat > "$temp_payload" <<-PAYLOAD
{
    "session": "default",
    "chatId": "$chat_id",
    "text": "$message"
}
PAYLOAD
    
    # Make the API call without failing on error
    local response=$(curl -s -X POST "$waha_url/api/sendText" \
        -H "X-Api-Key: $api_key" \
        -H "Content-Type: application/json" \
        -d @"$temp_payload" 2>&1 || echo "curl_failed")
    
    log "📱 WhatsApp Response: $response"
    
    # Clean up temp file
    rm -f "$temp_payload"
}

log "🔄 Starting MARBOT update..."

# Navigate to project directory
cd /opt/marbot-academic-bot || exit 1

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
    log "⚠️  Warning: WhatsApp notification env vars not set properly"
    log "   DEBUG_GROUP_ID: ${DEBUG_GROUP_ID:-not set}"
    log "   WAHA_URL: ${WAHA_URL:-not set}"
    log "   WAHA_API_KEY: ${WAHA_API_KEY:+set}"
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
    UP_TO_DATE_MSG="ℹ️ *MARBOT UPDATE*

📅 $(date '+%Y-%m-%d %H:%M:%S')
✅ Already up to date

📌 Current commit: \`${OLD_COMMIT}\`"
    
    log "📱 Sending 'up to date' notification..."
    send_whatsapp_message "$UP_TO_DATE_MSG" "$DEBUG_GROUP_ID" "$WAHA_URL" "$WAHA_API_KEY"
    
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
        
        # Send failure message
        FAIL_MSG="❌ *MARBOT UPDATE FAILED*

📅 $(date '+%Y-%m-%d %H:%M:%S')
🔴 Backend health check failed

📝 Commit: ${COMMIT_MSG}
👤 Author: ${COMMIT_AUTHOR}

🔍 Check logs: tail -f /var/log/marbot-update.log"
        
        send_whatsapp_message "$FAIL_MSG" "$DEBUG_GROUP_ID" "$WAHA_URL" "$WAHA_API_KEY"
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
    
    # Send failure message
    FAIL_MSG="❌ *MARBOT UPDATE FAILED*

📅 $(date '+%Y-%m-%d %H:%M:%S')
🔴 Services not healthy

📝 Commit: ${COMMIT_MSG}
👤 Author: ${COMMIT_AUTHOR}

🔍 Check logs: tail -f /var/log/marbot-update.log"
    
    send_whatsapp_message "$FAIL_MSG" "$DEBUG_GROUP_ID" "$WAHA_URL" "$WAHA_API_KEY"
    exit 1
fi

log "✅ Update completed successfully!"
log "🎉 Backend restarted, waha session preserved!"

# Prepare and send success message
SUCCESS_MSG="✅ *MARBOT UPDATE SUCCESS*

📅 $(date '+%Y-%m-%d %H:%M:%S')
🔄 ${OLD_COMMIT} → ${NEW_COMMIT}

📝 Commit message:
\`\`\`
${COMMIT_MSG}
\`\`\`

👤 Author: ${COMMIT_AUTHOR}
🎉 Backend restarted successfully!"

log "📱 Sending success notification to WhatsApp..."
send_whatsapp_message "$SUCCESS_MSG" "$DEBUG_GROUP_ID" "$WAHA_URL" "$WAHA_API_KEY"

log "✅ Deployment complete with notification sent!"

exit 0