#!/usr/bin/env bash
# Runs the M0 gate against the dev plane. Provider keys are read from the private env file by
# NAME only and never printed. The stale AWS_* entries in that file are deliberately NOT used;
# AWS auth comes from the aex-admin profile.
set -euo pipefail
ENVFILE="${AEX_KEYS_FILE:-$HOME/workspace/aex_workspace/aex-backup/.env.dev}"
pick() { grep "^$1=" "$ENVFILE" | head -1 | cut -d= -f2- | tr -d '
'; }
# 2026-08-18: the direct Anthropic/DeepSeek/OpenRouter keys on this machine are revoked
# (both providers answer 401). The Vercel AI Gateway key is live and the gateway serves BOTH
# certified wire formats (Anthropic Messages at /v1/messages, OpenAI Chat Completions at
# /v1/chat/completions), so the gate runs both dialects through it until fresh direct keys
# exist. To use direct keys: set ANTHROPIC_API_KEY / DEEPSEEK_API_KEY and unset the
# AEX_M0_*_BASE_URL overrides.
if [ -z "${ANTHROPIC_API_KEY:-}" ]; then
  GATEWAY_KEY="$(pick VERCEL_AI_GATEWAY_API_KEY)"
  export ANTHROPIC_API_KEY="$GATEWAY_KEY"
  export DEEPSEEK_API_KEY="$GATEWAY_KEY"
  export AEX_M0_ANTHROPIC_BASE_URL="https://ai-gateway.vercel.sh"
  export AEX_M0_ANTHROPIC_MODEL="anthropic/claude-haiku-4.5"
  export AEX_M0_DEEPSEEK_BASE_URL="https://ai-gateway.vercel.sh"
  # This gateway account is ZDR-locked and its DeepSeek capacity is not ZDR-attested; any
  # OpenAI model certifies the same Chat Completions dialect.
  export AEX_M0_DEEPSEEK_MODEL="openai/gpt-4o-mini"
  export AEX_M0_DEEPSEEK_PROVIDER="openai_compatible"
fi
unset AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY AWS_SESSION_TOKEN || true
export AWS_PROFILE=aex-admin
export AWS_REGION=eu-west-1
export AEX_JOURNAL_TABLE=aex-dev-journal
export AEX_KMS_KEY_ID=alias/aex-dev-session-keys
export AEX_SESSIONS_BUCKET=aex-dev-sessions-522921482290
export AEX_HAND_IMAGE=aex-hands-dev-1gb
export AEX_HAND_IMAGE_VERSION=3.0
export AEX_API_TOKEN=m0-dev-token
export AEX_MODE=aws
cd "$(dirname "$0")/.."
exec cargo run --bin m0 "$@"
