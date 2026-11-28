#!/usr/bin/env bash
# Writes the ignored .env.local at the repository root with what local builds need from EAS:
#
#   URSA_DEFAULT_RELAY_TOKEN    the token for Ursa's relays, baked into the core when it builds
#                               (crates/ursa-proto/src/relays.rs), as CI does from the
#                               URSA_RELAY_TOKEN secret
#
#   scripts/env-pull.sh [environment]      # default: production; or `mise run env-pull`
#
# Ursa's relays are Street Cryptid's, so the token is Street Cryptid's EXPO_PUBLIC_IROH_RELAY_TOKEN,
# read from its EAS project. That needs its checkout (STREET_CRYPTID_DIR, default
# ../streetCryptid) and an Expo login that can see the project (`bunx eas-cli login`).
# mise loads .env.local into every shell in this repository.
set -euo pipefail

environment="${1:-production}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
project="${STREET_CRYPTID_DIR:-$root/../streetCryptid}"
if [[ ! -f "$project/app.json" ]]; then
  echo "No Street Cryptid checkout at $project: set STREET_CRYPTID_DIR." >&2
  exit 1
fi

umask 077
vars="$(cd "$project" && bunx eas-cli env:list "$environment" --include-sensitive)"
token="$(sed -n 's/^EXPO_PUBLIC_IROH_RELAY_TOKEN=//p' <<<"$vars" | head -n 1)"
token="${token%\"}"
token="${token#\"}"
if [[ -z "$token" ]]; then
  echo "EAS $environment has no EXPO_PUBLIC_IROH_RELAY_TOKEN." >&2
  exit 1
fi

out="$root/.env.local"
{
  echo "# Written by scripts/env-pull.sh from Street Cryptid's EAS $environment environment. Not committed."
  echo "URSA_DEFAULT_RELAY_TOKEN=$token"
} > "$out"
chmod 600 "$out"
echo "Wrote $out (URSA_DEFAULT_RELAY_TOKEN). Rebuild the core to bake it in."
