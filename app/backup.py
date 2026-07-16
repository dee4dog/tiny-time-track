"""SQLite backup: nightly copy to a configured folder, keeping 30 days.

Uses SQLite's online backup API (not a plain file copy) so a consistent
snapshot is taken even while the app is running and using WAL mode.
"""
from __future__ import annotations

import sqlite3
from dataclasses import dataclass
from datetime import datetime, timedelta
from pathlib import Path

from app.config import config

RETENTION_DAYS = 30
_PREFIX = "tinytimetrack-"
# Backups written before the rename; still pruned and listed.
_LEGACY_PREFIX = "timetrack-"
_SUFFIX = ".db"


def _backup_files() -> list[Path]:
    d = backup_dir()
    return list(d.glob(f"{_PREFIX}*{_SUFFIX}")) + list(d.glob(f"{_LEGACY_PREFIX}*{_SUFFIX}"))


def backup_dir() -> Path:
    d = Path(config.backup_path)
    d.mkdir(parents=True, exist_ok=True)
    return d


def run_backup() -> Path:
    """Create a timestamped backup of the live database. Returns its path."""
    src = Path(config.db_path).resolve()
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    dest = backup_dir() / f"{_PREFIX}{stamp}{_SUFFIX}"

    source = sqlite3.connect(str(src))
    try:
        target = sqlite3.connect(str(dest))
        try:
            source.backup(target)  # consistent online snapshot
        finally:
            target.close()
    finally:
        source.close()

    prune_old_backups()
    return dest


def prune_old_backups(retention_days: int = RETENTION_DAYS) -> int:
    """Delete backups older than the retention window. Returns count removed."""
    cutoff = datetime.now() - timedelta(days=retention_days)
    removed = 0
    for path in _backup_files():
        try:
            mtime = datetime.fromtimestamp(path.stat().st_mtime)
            if mtime < cutoff:
                path.unlink()
                removed += 1
        except OSError:
            pass
    return removed


@dataclass
class BackupInfo:
    name: str
    size_kb: int
    when: datetime


def list_backups() -> list[BackupInfo]:
    """Most-recent-first list of existing backups for the Settings screen."""
    out: list[BackupInfo] = []
    for path in _backup_files():
        st = path.stat()
        out.append(BackupInfo(
            name=path.name,
            size_kb=max(1, round(st.st_size / 1024)),
            when=datetime.fromtimestamp(st.st_mtime),
        ))
    out.sort(key=lambda b: b.when, reverse=True)
    return out
