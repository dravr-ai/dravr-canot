#!/usr/bin/env bash
# ABOUTME: Starts a tunnel to expose the local webhook port for Slack/Telegram webhooks
# ABOUTME: Supports Cloudflare Tunnel and ngrok, auto-detects which is installed

set -euo pipefail

PORT=3001
PROVIDER=""

usage() {
    echo "Usage: $0 [--port PORT] [--provider cloudflare|ngrok]"
    echo ""
    echo "Starts a tunnel to expose the local webhook port publicly."
    echo "Slack/Telegram webhook URLs should point to <tunnel-url>/webhook/slack or /webhook/telegram."
    echo ""
    echo "Options:"
    echo "  --port PORT          Local port to tunnel (default: 3001)"
    echo "  --provider PROVIDER  Force a specific provider: cloudflare or ngrok"
    echo "                       Default: auto-detect (prefers cloudflare)"
    exit 1
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --port)
            PORT="$2"
            shift 2
            ;;
        --provider)
            PROVIDER="$2"
            shift 2
            ;;
        --help|-h)
            usage
            ;;
        *)
            echo "Unknown option: $1"
            usage
            ;;
    esac
done

# Auto-detect provider if not specified
if [[ -z "$PROVIDER" ]]; then
    if command -v cloudflared &>/dev/null; then
        PROVIDER="cloudflare"
    elif command -v ngrok &>/dev/null; then
        PROVIDER="ngrok"
    else
        echo "Error: Neither 'cloudflared' nor 'ngrok' found in PATH."
        echo ""
        echo "Install one of:"
        echo "  Cloudflare Tunnel: https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/"
        echo "  ngrok:             https://ngrok.com/download"
        exit 1
    fi
fi

echo "Starting $PROVIDER tunnel on port $PORT..."
echo ""

case "$PROVIDER" in
    cloudflare)
        if ! command -v cloudflared &>/dev/null; then
            echo "Error: 'cloudflared' not found in PATH."
            exit 1
        fi
        echo "The tunnel URL will appear in the output below (look for a *.trycloudflare.com URL)."
        echo "Set your webhook URL to: <tunnel-url>/webhook/slack or /webhook/telegram"
        echo ""
        exec cloudflared tunnel --url "http://localhost:${PORT}"
        ;;
    ngrok)
        if ! command -v ngrok &>/dev/null; then
            echo "Error: 'ngrok' not found in PATH."
            exit 1
        fi
        echo "The tunnel URL will appear in the ngrok dashboard."
        echo "Set your webhook URL to: <tunnel-url>/webhook/slack or /webhook/telegram"
        echo ""
        exec ngrok http "$PORT"
        ;;
    *)
        echo "Error: Unknown provider '$PROVIDER'. Use 'cloudflare' or 'ngrok'."
        exit 1
        ;;
esac
