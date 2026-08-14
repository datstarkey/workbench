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
bunx tauri build --target universal-apple-darwin "$@"

# Tauri notarizes and staples the .app but only *signs* the .dmg — and Gatekeeper
# assesses the container the user actually downloads. Without this the DMG reports
# "Unnotarized Developer ID" and still warns on open.
if [[ -n "${APPLE_API_KEY_PATH:-}" ]]; then
	DMG=$(find "$REPO_ROOT/target/universal-apple-darwin/release/bundle/dmg" -name '*.dmg' -maxdepth 1 2>/dev/null | head -1)
	if [[ -n "$DMG" ]]; then
		echo "Notarizing $(basename "$DMG")"
		xcrun notarytool submit "$DMG" \
			--key "$APPLE_API_KEY_PATH" \
			--key-id "$APPLE_API_KEY" \
			--issuer "$APPLE_API_ISSUER" \
			--wait
		xcrun stapler staple "$DMG"
		spctl -a -vvv -t open --context context:primary-signature "$DMG"
	fi
fi
