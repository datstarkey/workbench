# Terminal Performance Notes

## Performance Mode

- `auto`: enabled whenever a terminal is offscreen (inactive tab/workspace or app hidden).
- `always`: keeps performance mode enabled even for the active pane.

When performance mode is active, Workbench favors throughput:

- More aggressive output batching before writes to xterm.
- Reduced xterm scrollback target.
- Deferred linkification (auto mode loads linkification only once a pane is actively viewed).

## Telemetry

Enable **Terminal telemetry** in Workbench settings to emit periodic console metrics:

- output events and bytes
- flush count and average flush cost
- input event count and average input-to-first-output latency
- max queued bytes between flushes

## Local Stress Harness

Use the bundled generator to produce high-volume terminal output:

```bash
bun run bench:terminal
```

Optional controls:

```bash
LINES=50000 WIDTH=160 SLEEP_MS=0 bun run bench:terminal
```
