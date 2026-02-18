#!/usr/bin/env bash
set -euo pipefail

# Terminal throughput benchmark generator.
# Usage:
#   bun run bench:terminal
# Optional env:
#   LINES=20000 WIDTH=120 SLEEP_MS=0 bash scripts/terminal-bench.sh

LINES="${LINES:-20000}"
WIDTH="${WIDTH:-120}"
SLEEP_MS="${SLEEP_MS:-0}"

if ! [[ "$LINES" =~ ^[0-9]+$ && "$WIDTH" =~ ^[0-9]+$ && "$SLEEP_MS" =~ ^[0-9]+$ ]]; then
	echo "LINES, WIDTH, and SLEEP_MS must be integers." >&2
	exit 1
fi

pad_line() {
	local i="$1"
	local base="[$(date +%H:%M:%S)] line ${i} :: "
	local payload=""
	while ((${#payload} < WIDTH)); do
		payload+="0123456789abcdef"
	done
	payload="${payload:0:WIDTH}"
	printf "%s%s\n" "$base" "$payload"
}

echo "# terminal bench start lines=${LINES} width=${WIDTH} sleep_ms=${SLEEP_MS}" >&2
start_epoch_ms="$(($(date +%s%N) / 1000000))"

for ((i = 1; i <= LINES; i++)); do
	pad_line "$i"
	if ((SLEEP_MS > 0)); then
		sleep "$(awk "BEGIN { printf \"%.3f\", ${SLEEP_MS} / 1000 }")"
	fi
done

end_epoch_ms="$(($(date +%s%N) / 1000000))"
duration_ms="$((end_epoch_ms - start_epoch_ms))"
echo "# terminal bench end duration_ms=${duration_ms}" >&2
