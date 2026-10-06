#!/usr/bin/env python3
"""Check the versioned Codex union inventory; optionally compare fresh bindings.

codex app-server generate-ts --experimental --out /tmp/codex-bindings
python3 scripts/check-codex-protocol.py --bindings /tmp/codex-bindings

The checked-in inventory makes protocol decisions reviewable in normal CI. A
CLI bump must regenerate bindings and classify additions before changing it.
"""
import argparse
import json
import re
from pathlib import Path

root = Path(__file__).resolve().parents[1]
p = argparse.ArgumentParser()
p.add_argument("--bindings", type=Path)
args = p.parse_args()
inventory = json.loads((root / "crates/workbench-core/src/codex_transcript/fixtures/protocol-0.160.0.json").read_text())
sources = "\n".join(file.read_text() for directory in [root / "crates/workbench-core/src", root / "apps/server/src/agent"] for file in directory.rglob("*.rs"))
for category, filename, field in [("requests", "ServerRequest.ts", "method"), ("notifications", "ServerNotification.ts", "method"), ("items", "v2/ThreadItem.ts", "type")]:
    classified = inventory[category]
    assert all(value in {"handled", "ignored", "rejected"} for value in classified.values()), f"Invalid {category} classification"
    for method, action in classified.items():
        if action == "handled":
            assert f'"{method}"' in sources, f"{method} has no adapter"
    if args.bindings:
        generated = set(re.findall(r'"' + field + r'": "([^\"]+)"', (args.bindings / filename).read_text()))
        assert generated == set(classified), f"{category} changed: added {sorted(generated - set(classified))}; removed {sorted(set(classified) - generated)}"
print(f"Codex {inventory['version']} protocol inventory checked")
