"""Week / date helpers.

The whole app thinks in terms of Monday-started weeks. A "week" is identified
by its Monday (a ``date``); days within it are 0=Mon .. 4=Fri.
"""
from __future__ import annotations

from datetime import date, timedelta

DAY_NAMES = ["Mon", "Tue", "Wed", "Thu", "Fri"]


def monday_of(d: date) -> date:
    """Return the Monday on or before ``d``."""
    return d - timedelta(days=d.weekday())


def parse_week(value: str | None, *, today: date | None = None) -> date:
    """Turn a ``?week=YYYY-MM-DD`` query value into that week's Monday.

    Falls back to the current week if the value is missing or unparseable.
    """
    base = today or date.today()
    if not value:
        return monday_of(base)
    try:
        d = date.fromisoformat(value)
    except ValueError:
        return monday_of(base)
    return monday_of(d)


def week_days(week_start: date) -> list[date]:
    """The five calendar dates (Mon..Fri) for a week."""
    return [week_start + timedelta(days=i) for i in range(5)]


def shift_week(week_start: date, weeks: int) -> date:
    """Monday ``weeks`` weeks before/after ``week_start``."""
    return week_start + timedelta(weeks=weeks)


def format_week_range(week_start: date) -> str:
    """Human label like ``2 – 6 Jun 2026`` for the week header."""
    end = week_start + timedelta(days=4)
    if week_start.month == end.month:
        return f"{week_start.day} – {end.day} {end:%b %Y}"
    if week_start.year == end.year:
        return f"{week_start.day} {week_start:%b} – {end.day} {end:%b %Y}"
    return f"{week_start.day} {week_start:%b %Y} – {end.day} {end:%b %Y}"
