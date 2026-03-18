# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |

## Reporting a Vulnerability

If you discover a security vulnerability in dravr-canot, please report it responsibly:

1. **Do not** open a public GitHub issue
2. Email **security@dravr.ai** with a description of the vulnerability
3. Include steps to reproduce, if possible
4. You will receive an acknowledgment within 48 hours

We will work with you to understand the issue and coordinate a fix before any public disclosure.

## Security Model

dravr-canot handles webhook ingress from external chat platforms. The security boundary is:

- **Webhook signature verification** — every inbound webhook is cryptographically verified before processing (HMAC-SHA256 for WhatsApp/Messenger/Slack, Ed25519 for Discord, secret token for Telegram)
- **Constant-time comparison** — all signature checks use constant-time comparison to prevent timing attacks
- **Replay protection** — configurable timestamp validation policy to reject stale webhooks
- **No secrets in core** — the library stores no API keys or tokens at rest; credentials are passed per-request via `ChannelConfig`
- **Bearer auth** — `dravr-canot-server` supports optional `DRAVR_API_KEY` for request authentication
- **Input validation** — all webhook payloads are validated at system boundaries before processing
