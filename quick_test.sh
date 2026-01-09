#!/bin/bash

# Load .env file
if [ -f .env ]; then
    export $(cat .env | grep -v '^#' | xargs)
fi

# Use DEBUG_GROUP_ID from .env
CHAT_ID="${DEBUG_GROUP_ID}"

# Test images
IMAGE_PAGI="https://raw.githubusercontent.com/gimigkk/marbot-academic-bot/6b2b72dca7ca954fe5e8eef81649d9fff24515c9/asset/pagi.jpg"
IMAGE_SORE="https://raw.githubusercontent.com/gimigkk/marbot-academic-bot/6b2b72dca7ca954fe5e8eef81649d9fff24515c9/asset/malam.jpg"

echo "🧪 MARBOT IMAGE TEST"
echo "===================="
echo "Target: $CHAT_ID"
echo ""
echo "Choose image to test:"
echo "1) Morning image (pagi.jpg)"
echo "2) Evening image (malam.jpg)"
echo "3) Both images"
read -p "Enter choice (1-3): " choice

send_image() {
    local img_url=$1
    local img_name=$2
    
    CAPTION="🧪 *TEST: $img_name*

Test pengiriman gambar dari terminal
Waktu: $(date '+%d %b %Y, %H:%M:%S WIB')

✅ Image attachment working!"

    PAYLOAD=$(cat <<EOF
{
  "chatId": "$CHAT_ID",
  "file": {
    "url": "$img_url",
    "mimetype": "image/jpeg",
    "filename": "$img_name"
  },
  "caption": "$CAPTION",
  "session": "default"
}
EOF
)

    echo ""
    echo "📤 Sending $img_name..."
    
    HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" \
      -X POST "$WAHA_URL/api/sendImage" \
      -H "Content-Type: application/json" \
      -H "X-Api-Key: $WAHA_API_KEY" \
      -d "$PAYLOAD")

    if [ "$HTTP_CODE" -eq 200 ] || [ "$HTTP_CODE" -eq 201 ]; then
        echo "✅ $img_name sent successfully! (HTTP $HTTP_CODE)"
    else
        echo "❌ Failed to send $img_name (HTTP $HTTP_CODE)"
    fi
}

case $choice in
    1)
        send_image "$IMAGE_PAGI" "pagi.jpg"
        ;;
    2)
        send_image "$IMAGE_SORE" "malam.jpg"
        ;;
    3)
        send_image "$IMAGE_PAGI" "pagi.jpg"
        sleep 2
        send_image "$IMAGE_SORE" "malam.jpg"
        ;;
    *)
        echo "❌ Invalid choice"
        exit 1
        ;;
esac

echo ""
echo "🎉 Test complete!"