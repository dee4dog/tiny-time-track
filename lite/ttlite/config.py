"""Where the app keeps its data and which local port it listens on.

Data lives in the user's profile (not the program folder) so updating or
moving the app never touches the timesheet records:

* Windows: %LOCALAPPDATA%\\TinyTimeTrackLite
* elsewhere: ~/.tinytimetrack-lite

Override with the TTLITE_DATA_DIR environment variable or --data-dir.
"""
from __future__ import annotations

import os
from pathlib import Path

HOST = "127.0.0.1"  # local only: this is a single-computer app
DEFAULT_PORT = 8100


def default_data_dir() -> Path:
    override = os.environ.get("TTLITE_DATA_DIR")
    if override:
        return Path(override).expanduser()
    local = os.environ.get("LOCALAPPDATA")
    if local:
        return Path(local) / "TinyTimeTrackLite"
    return Path.home() / ".tinytimetrack-lite"


def default_port() -> int:
    try:
        return int(os.environ.get("TTLITE_PORT", DEFAULT_PORT))
    except ValueError:
        return DEFAULT_PORT
