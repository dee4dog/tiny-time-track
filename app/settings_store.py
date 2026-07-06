"""Helpers for the key-value ``settings`` table.

Business settings a manager can change at runtime live here (not in .env).
Values are stored as text and converted on read. ``DEFAULTS`` documents
every known key and seeds the table on first run.
"""
from __future__ import annotations

from decimal import Decimal

from sqlalchemy.orm import Session

from app.models import Setting

# Every setting the app understands, with its default value as a string.
# Phase 5 (reminders) will add reminder_* schedule/template keys here.
DEFAULTS: dict[str, str] = {
    # Globals
    "overhead_multiplier": "1.6",   # salary -> cost-to-company factor
    "overtime_factor": "1.5",       # >8h/day costs this much; set 1.0 to disable
    "available_hours_default": "1760",
    "currency_symbol": "R",
    "company_name": "ES",
    # SMTP (edited on the Settings > Email screen). The password is stored in
    # the database, not in code. Default host assumes Microsoft 365.
    "smtp_host": "smtp.office365.com",
    "smtp_port": "587",
    "smtp_use_tls": "true",         # STARTTLS on the configured port
    "smtp_username": "",
    "smtp_password": "",
    "smtp_from": "",                # From address; falls back to smtp_username
    # Nightly database backup (Phase 6).
    "backup_enabled": "true",
    "backup_time": "02:00",
    # Reminder scheduler (Africa/Johannesburg). dow: 0=Mon .. 6=Sun.
    "reminder_enabled": "true",
    "email_base_url": "http://localhost:8000",   # used to build timesheet links
    "reminder_plan_dow": "0",       # Monday
    "reminder_plan_time": "08:00",
    "reminder_actuals_dow": "4",    # Friday
    "reminder_actuals_time": "15:00",
    "reminder_followup_dow": "0",   # next Monday
    "reminder_followup_time": "09:00",
    # Email templates. Placeholders: {name}, {week}, {link}
    "template_plan_subject": "Plan your week — {week}",
    "template_plan_body": (
        "Hi {name},\n\n"
        "Please fill in your planned hours for the week of {week}.\n\n"
        "Open your timesheet: {link}\n\n"
        "Thanks."
    ),
    "template_actuals_subject": "Submit your hours — {week}",
    "template_actuals_body": (
        "Hi {name},\n\n"
        "Please log your actual hours for the week of {week} and submit them.\n\n"
        "Open your timesheet: {link}\n\n"
        "Thanks."
    ),
    "template_followup_subject": "Reminder: timesheet outstanding — {week}",
    "template_followup_body": (
        "Hi {name},\n\n"
        "Your timesheet for {week} is still outstanding. Please complete it.\n\n"
        "{link}\n\n"
        "Thanks."
    ),
}


def get_bool(db: Session, key: str) -> bool:
    """Return a setting parsed as a boolean (true/1/yes/on)."""
    return (get_setting(db, key) or "").strip().lower() in {"1", "true", "yes", "on"}


def get_setting(db: Session, key: str, default: str | None = None) -> str | None:
    """Return a raw string setting value, falling back to DEFAULTS then ``default``."""
    row = db.get(Setting, key)
    if row is not None:
        return row.value
    return DEFAULTS.get(key, default)


def get_decimal(db: Session, key: str) -> Decimal:
    """Return a setting parsed as Decimal (for money/factor settings)."""
    return Decimal(get_setting(db, key) or "0")


def set_setting(db: Session, key: str, value: str) -> None:
    """Insert or update a setting. Caller is responsible for db.commit()."""
    row = db.get(Setting, key)
    if row is None:
        db.add(Setting(key=key, value=str(value)))
    else:
        row.value = str(value)


def ensure_defaults(db: Session) -> None:
    """Seed any missing default settings. Safe to call on every startup."""
    existing = {s.key for s in db.query(Setting.key).all()}
    added = False
    for key, value in DEFAULTS.items():
        if key not in existing:
            db.add(Setting(key=key, value=value))
            added = True
    if added:
        db.commit()
