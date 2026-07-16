"""APScheduler integration for timed reminder emails.

A single in-process BackgroundScheduler runs three cron jobs (plan, actuals,
follow-up) in the configured timezone. The schedule is read from the settings
table; call ``reschedule()`` after a manager edits the reminder settings to
apply the new times without a restart.
"""
from __future__ import annotations

import logging

from apscheduler.schedulers.background import BackgroundScheduler
from apscheduler.triggers.cron import CronTrigger

from app.backup import run_backup
from app.config import config
from app.database import SessionLocal
from app.reminders import run_reminder
from app.settings_store import get_bool, get_setting

log = logging.getLogger("tinytimetrack.scheduler")

_DOW = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]

_scheduler: BackgroundScheduler | None = None


def _job(kind: str) -> None:
    """Scheduled entry point: run one kind of reminder in its own session."""
    with SessionLocal() as db:
        if not get_bool(db, "reminder_enabled"):
            log.info("Reminders disabled; skipping %s job.", kind)
            return
        result = run_reminder(db, kind)
        log.info("Reminder %s: %s", kind, result.summary)


def _backup_job() -> None:
    """Scheduled nightly database backup."""
    with SessionLocal() as db:
        if not get_bool(db, "backup_enabled"):
            log.info("Backups disabled; skipping nightly backup.")
            return
    try:
        path = run_backup()
        log.info("Backup written: %s", path)
    except Exception:  # noqa: BLE001
        log.exception("Nightly backup failed")


def _parse_time(value: str, default: str) -> tuple[int, int]:
    try:
        hh, mm = (value or default).split(":")
        return int(hh), int(mm)
    except (ValueError, AttributeError):
        hh, mm = default.split(":")
        return int(hh), int(mm)


def _dow(value: str, default: int) -> str:
    try:
        i = int(value)
    except (ValueError, TypeError):
        i = default
    return _DOW[i % 7]


def _add_jobs(db) -> None:
    assert _scheduler is not None
    specs = [
        ("plan", "reminder_plan_dow", "reminder_plan_time", 0, "08:00"),
        ("actuals", "reminder_actuals_dow", "reminder_actuals_time", 4, "15:00"),
        ("followup", "reminder_followup_dow", "reminder_followup_time", 0, "09:00"),
    ]
    for kind, dow_key, time_key, dow_def, time_def in specs:
        hour, minute = _parse_time(get_setting(db, time_key), time_def)
        dow = _dow(get_setting(db, dow_key), dow_def)
        trigger = CronTrigger(
            day_of_week=dow, hour=hour, minute=minute, timezone=config.timezone
        )
        _scheduler.add_job(
            _job, trigger=trigger, args=[kind], id=f"reminder_{kind}",
            replace_existing=True,
        )
        log.info("Scheduled %s reminder: %s %02d:%02d %s", kind, dow, hour, minute, config.timezone)

    # Nightly database backup
    b_hour, b_minute = _parse_time(get_setting(db, "backup_time"), "02:00")
    _scheduler.add_job(
        _backup_job,
        trigger=CronTrigger(hour=b_hour, minute=b_minute, timezone=config.timezone),
        id="nightly_backup", replace_existing=True,
    )
    log.info("Scheduled nightly backup: %02d:%02d %s", b_hour, b_minute, config.timezone)


def start() -> None:
    """Start the scheduler and register the reminder jobs."""
    global _scheduler
    if _scheduler is not None:
        return
    _scheduler = BackgroundScheduler(timezone=config.timezone)
    with SessionLocal() as db:
        _add_jobs(db)
    _scheduler.start()


def reschedule() -> None:
    """Re-read the schedule from settings and update the jobs."""
    if _scheduler is None:
        return
    with SessionLocal() as db:
        _add_jobs(db)


def shutdown() -> None:
    global _scheduler
    if _scheduler is not None:
        _scheduler.shutdown(wait=False)
        _scheduler = None
