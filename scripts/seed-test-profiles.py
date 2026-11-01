#!/usr/bin/env python3
"""Generate profiles only in a separate config directory for manual UI comparison."""
import json
import pathlib

root = pathlib.Path(__file__).resolve().parent.parent
destination = root / ".test-data/ui-config/harbor"
destination.mkdir(parents=True, exist_ok=True)
settings = {"profiles": [], "desktop_theme": "original", "tui_theme": "turbo_classic", "refresh_seconds": 5, "terminal_colors": "auto", "ascii": False}
for version in ["10.11", "11.4"]:
    socket = root / f".test-data/mariadb-{version}/run/mariadb.sock"
    if not socket.exists():
        raise SystemExit(f"Missing isolated test socket: {socket}")
    settings["profiles"].append({"name": f"IZOLOVANÝ TEST MariaDB {version}", "transport": {"Unix": {"path": str(socket)}}, "user": "root", "tls": "VerifyIdentity", "ca": None})
(destination / "settings.json").write_text(json.dumps(settings, indent=2) + "\n")
print(f"Test configuration: {destination.parent}")
