#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

if [[ ! -s .env ]]; then
  echo 'Create /opt/tinythought/.env before deploying (see .env.example).' >&2
  exit 1
fi
chmod 600 .env

compose=(docker compose --env-file .env -f compose.yml)
"${compose[@]}" config --quiet
"${compose[@]}" pull app
"${compose[@]}" up -d --wait --wait-timeout 120 app
"${compose[@]}" ps
