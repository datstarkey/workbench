#!/usr/bin/env bash
# Signed + notarized local release build. Credentials come from `signing.env`
# at the repo root (see signing.env.example / docs/SIGNING.md).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
ENV_FILE="$REPO_ROOT/signing.env"

if [[ "$(uname -s)" != "Darwin" ]]; then
	echo "build-signed.sh is macOS-only." >&2
	exit 1
fi

if [[ ! -f "$ENV_FILE" ]]; then
	echo "Missing $ENV_FILE — copy signing.env.example and fill it in." >&2
	exit 1
fi

# shellcheck disable=SC1090
source "$ENV_FILE"

: "${APPLE_SIGNING_IDENTITY:?not set in signing.env}"

if ! security find-identity -v -p codesigning | grep -qF "$APPLE_SIGNING_IDENTITY"; then
	echo "Identity '$APPLE_SIGNING_IDENTITY' is not in the login keychain." >&2
	echo "Available:" >&2
	security find-identity -v -p codesigning >&2
	exit 1
fi

if [[ -z "${APPLE_API_KEY_PATH:-}" || ! -f "${APPLE_API_KEY_PATH:-}" ]]; then
	echo "Warning: no App Store Connect key at APPLE_API_KEY_PATH — signing but NOT notarizing." >&2
	unset APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH
fi

cd "$REPO_ROOT/apps/desktop"
exec bunx tauri build --target universal-apple-darwin "$@"
