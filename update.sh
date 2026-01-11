#!/bin/bash

set -euo pipefail  # Exit on error, undefined variables, and pipe failures

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

# Rollback function
rollback() {
    log "⚠️ Rolling back to previous version..."
    if [ -f "marbot-backup" ]; then
        docker compose down backend
        
        # Determine which Dockerfile to use for rollback
        if [ -f "Dockerfile.backup-mode" ]; then
            ROLLBACK_MODE=$(cat Dockerfile.backup-mode)
            log "Using $ROLLBACK_MODE for rollback"
            if [ "$ROLLBACK_MODE" = "prebuilt" ]; then
                # Restore prebuilt binary
                if [ -f "marbot-binary" ]; then
                    rm -f marbot-binary
                fi
                cp marbot-backup marbot-binary
                docker compose build --no-cache -f Dockerfile.prebuilt backend
            else
                # Use original Dockerfile
                docker compose build --no-cache backend
            fi
        else
            # Fallback: try to restore with prebuilt if binary exists
            if [ -f "marbot-binary" ]; then
                rm -f marbot-binary
            fi
            cp marbot-backup marbot-binary
            if [ -f "Dockerfile.prebuilt" ]; then
                docker compose build --no-cache -f Dockerfile.prebuilt backend
            else
                docker compose build --no-cache backend
            fi
        fi
        
        docker compose up -d backend
        log "✅ Rolled back successfully"
    else
        log "❌ No backup found, cannot rollback!"
    fi
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
    log "⚠️ Warning: WhatsApp notification env vars not set properly"
fi

# Determine build mode
BUILD_MODE="${BUILD_MODE:-auto}"
log "🔧 Build mode: $BUILD_MODE"

# Get commit info from environment or git
if [ -n "${COMMIT_SHA:-}" ]; then
    NEW_COMMIT="$COMMIT_SHA"
    COMMIT_MSG="${COMMIT_MSG:-Update}"
    COMMIT_AUTHOR="${AUTHOR:-Unknown}"
    ADDITIONS="${ADDITIONS:-0}"
    DELETIONS="${DELETIONS:-0}"
else
    # Fallback to git if env vars not set
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
fi

log "📝 New commit: ${COMMIT_MSG}"
log "👤 Author: ${COMMIT_AUTHOR}"
log "📊 Changes: +${ADDITIONS} -${DELETIONS}"

# Decide whether to use prebuilt binary or build on VPS
if [ "$BUILD_MODE" = "prebuilt" ] && [ -f marbot-new ]; then
    log "✅ Using prebuilt binary from GitHub Actions"
    USE_PREBUILT=true
elif [ "$BUILD_MODE" = "vps-build" ]; then
    log "⚠️ Building on VPS (GitHub Actions build unavailable)"
    USE_PREBUILT=false
else
    # Auto mode: prefer prebuilt if available
    if [ -f marbot-new ]; then
        log "✅ Prebuilt binary found, using it"
        USE_PREBUILT=true
    else
        log "⚠️ No prebuilt binary, will build on VPS"
        USE_PREBUILT=false
    fi
fi

if [ "$USE_PREBUILT" = true ]; then
    # ============================================
    # PREBUILT BINARY MODE
    # ============================================
    log "📦 Using prebuilt binary from GitHub Actions..."
    
    # Backup current binary if it exists
    if [ -f marbot-binary ]; then
        log "💾 Backing up current binary..."
        cp marbot-binary marbot-backup
        echo "prebuilt" > Dockerfile.backup-mode
    fi
    
    # Copy prebuilt binary into Docker build context
    cp marbot-new marbot-binary
    chmod +x marbot-binary
    
    # Use Dockerfile.prebuilt for prebuilt binary
    if [ ! -f Dockerfile.prebuilt ]; then
        log "❌ Error: Dockerfile.prebuilt not found!"
        exit 1
    fi
    
    # Build Docker image with prebuilt binary
    log "🔨 Building Docker image with prebuilt binary..."
    if ! docker compose build --no-cache -f Dockerfile.prebuilt backend; then
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
    
    # Clean up temporary binary
    rm -f marbot-new
    
else
    # ============================================
    # VPS BUILD MODE (FALLBACK)
    # ============================================
    log "⚠️ GitHub Actions build failed - building on VPS..."
    
    # Backup mode indicator
    echo "vps-build" > Dockerfile.backup-mode
    
    # Make sure Dockerfile (original) exists
    if [ ! -f Dockerfile ]; then
        log "❌ Error: Dockerfile not found!"
        exit 1
    fi
    
    # Build using original Dockerfile
    log "🔨 Building Docker image on VPS (this may take 10-15 minutes)..."
    if ! docker compose build --no-cache backend; then
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
fi

log "🚀 Restarting backend service..."
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

# Check if all services are running
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
rm -f marbot-backup
rm -f Dockerfile.backup-mode

# Prepare success message
if [ "$USE_PREBUILT" = true ]; then
    BUILD_INFO="Built on GitHub Actions ⚡"
else
    BUILD_INFO="Built on VPS (fallback) 🔄"
fi

SUCCESS_MSG="*[ MARBOT UPDATE SUCCESS ]*
_${COMMIT_MSG}_

*[ \`#${NEW_COMMIT}\` by ${COMMIT_AUTHOR} ]* +${ADDITIONS} -${DELETIONS}
https://github.com/gimigkk/marbot-academic-bot/commit/${NEW_COMMIT}

${BUILD_INFO}"

log "📱 Sending success notification to WhatsApp..."
send_whatsapp_message "$SUCCESS_MSG" "$DEBUG_GROUP_ID" "$WAHA_URL" "$WAHA_API_KEY"

log "✅ Deployment complete!"

exit 0