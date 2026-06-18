"""Business logic for the employee weekly timesheet.

Kept separate from the HTTP layer so the rules (what's editable, how a cell
upserts, how a week locks) are easy to read and test in one place.
"""
from __future__ import annotations

from datetime import date, datetime, timedelta, timezone
from decimal import ROUND_HALF_UP, Decimal, InvalidOperation

from sqlalchemy import select
from sqlalchemy.orm import Session

from app.models import (
    Employee,
    EmployeeProject,
    Project,
    ProjectStatus,
    TimeEntry,
    WeekStatus,
)
from app.weeks import week_days

# A week becomes read-only for the employee this many days after they submit
# actuals (managers can still unlock by clearing the stamp).
LOCK_AFTER_DAYS = 7

# Allowed value ranges (hours). Planned/actual cap at a long-but-sane day;
# overtime is a typed value that may exceed a normal 8-hour day.
HOURS_MIN = Decimal("0")
HOURS_MAX = Decimal("12")
OVERTIME_MAX = Decimal("16")

VALID_FIELDS = {"planned", "actual", "overtime", "note"}
NOTE_MAX_LEN = 500


def clean_hours(raw: str | float | None, *, maximum: Decimal) -> Decimal:
    """Parse a user value into a clean 0.5-step Decimal within [0, maximum]."""
    if raw is None or raw == "":
        return Decimal("0")
    try:
        value = Decimal(str(raw))
    except (InvalidOperation, ValueError):
        return Decimal("0")
    # Round to the nearest half hour.
    value = (value * 2).quantize(Decimal("1"), rounding=ROUND_HALF_UP) / 2
    if value < HOURS_MIN:
        return HOURS_MIN
    if value > maximum:
        return maximum
    return value


def assigned_projects(db: Session, employee: Employee) -> list[Project]:
    """Projects on this employee's grid, excluding archived ones."""
    stmt = (
        select(Project)
        .join(EmployeeProject, EmployeeProject.project_id == Project.id)
        .where(EmployeeProject.employee_id == employee.id)
        .where(Project.status != ProjectStatus.archived)
        .order_by(Project.billable.desc(), Project.name)
    )
    return list(db.scalars(stmt))


def addable_projects(db: Session, employee: Employee) -> list[Project]:
    """Active projects the employee could add to their grid (not already on it)."""
    already = {p.id for p in assigned_projects(db, employee)}
    stmt = (
        select(Project)
        .where(Project.status == ProjectStatus.active)
        .order_by(Project.billable.desc(), Project.name)
    )
    return [p for p in db.scalars(stmt) if p.id not in already]


def get_week_status(db: Session, employee: Employee, week_start: date) -> WeekStatus | None:
    return db.get(WeekStatus, {"employee_id": employee.id, "week_start": week_start})


def is_locked(db: Session, employee: Employee, week_start: date) -> bool:
    """True if the week is locked for employee editing (actuals submitted > 7d ago)."""
    status = get_week_status(db, employee, week_start)
    if status is None or status.actuals_submitted_at is None:
        return False
    submitted = status.actuals_submitted_at
    if submitted.tzinfo is None:
        submitted = submitted.replace(tzinfo=timezone.utc)
    return datetime.now(timezone.utc) - submitted > timedelta(days=LOCK_AFTER_DAYS)


def entries_for_week(
    db: Session, employee: Employee, week_start: date
) -> dict[tuple[int, int], TimeEntry]:
    """Existing entries keyed by (project_id, day) for quick lookup."""
    stmt = select(TimeEntry).where(
        TimeEntry.employee_id == employee.id,
        TimeEntry.week_start == week_start,
    )
    return {(e.project_id, e.day): e for e in db.scalars(stmt)}


def build_grid(db: Session, employee: Employee, week_start: date) -> list[dict]:
    """Rows for the template: one per assigned project, each with five day cells."""
    projects = assigned_projects(db, employee)
    existing = entries_for_week(db, employee, week_start)
    days = week_days(week_start)

    rows: list[dict] = []
    for project in projects:
        cells = []
        for i, day_date in enumerate(days):
            entry = existing.get((project.id, i))
            cells.append(
                {
                    "day": i,
                    "date": day_date,
                    "planned": entry.planned_hours if entry else Decimal("0"),
                    "actual": entry.actual_hours if entry else Decimal("0"),
                    "overtime": entry.overtime_hours if entry else Decimal("0"),
                    "note": (entry.note if entry and entry.note else ""),
                }
            )
        rows.append({"project": project, "cells": cells})
    return rows


def _get_or_create_entry(
    db: Session, employee: Employee, project_id: int, week_start: date, day: int
) -> TimeEntry:
    entry = db.scalar(
        select(TimeEntry).where(
            TimeEntry.employee_id == employee.id,
            TimeEntry.project_id == project_id,
            TimeEntry.week_start == week_start,
            TimeEntry.day == day,
        )
    )
    if entry is None:
        entry = TimeEntry(
            employee_id=employee.id,
            project_id=project_id,
            week_start=week_start,
            day=day,
        )
        db.add(entry)
    return entry


def save_cell(
    db: Session,
    employee: Employee,
    *,
    project_id: int,
    week_start: date,
    day: int,
    field: str,
    value: str,
) -> None:
    """Upsert a single field of one (project, day) cell.

    Raises ValueError on invalid input or a project the employee isn't
    assigned to. The caller turns that into an HTTP error.
    """
    if field not in VALID_FIELDS:
        raise ValueError(f"Unknown field {field!r}")
    if not 0 <= day <= 4:
        raise ValueError("day out of range")

    # The project must be on this employee's grid (prevents logging time to
    # arbitrary projects via a crafted request).
    assigned_ids = {p.id for p in assigned_projects(db, employee)}
    if project_id not in assigned_ids:
        raise ValueError("project not assigned to this employee")

    entry = _get_or_create_entry(db, employee, project_id, week_start, day)

    if field == "note":
        text = (value or "").strip()[:NOTE_MAX_LEN]
        entry.note = text or None
    elif field == "overtime":
        entry.overtime_hours = clean_hours(value, maximum=OVERTIME_MAX)
    elif field == "planned":
        entry.planned_hours = clean_hours(value, maximum=HOURS_MAX)
    elif field == "actual":
        entry.actual_hours = clean_hours(value, maximum=HOURS_MAX)

    db.commit()


def copy_last_week_plan(
    db: Session, employee: Employee, week_start: date
) -> int:
    """Copy the previous week's planned hours into this week.

    Only fills planned_hours where this week's cell is currently 0, so it
    won't clobber edits already made. Returns the number of cells filled.
    """
    prev_start = week_start - timedelta(weeks=1)
    prev = entries_for_week(db, employee, week_start=prev_start)
    if not prev:
        return 0

    assigned_ids = {p.id for p in assigned_projects(db, employee)}
    filled = 0
    for (project_id, day), prev_entry in prev.items():
        if project_id not in assigned_ids:
            continue
        if prev_entry.planned_hours <= 0:
            continue
        entry = _get_or_create_entry(db, employee, project_id, week_start, day)
        if entry.planned_hours and entry.planned_hours > 0:
            continue  # don't overwrite an existing plan
        entry.planned_hours = prev_entry.planned_hours
        filled += 1

    db.commit()
    return filled


def add_project_to_grid(db: Session, employee: Employee, project_id: int) -> bool:
    """Add an active project to the employee's grid. Returns True if added."""
    project = db.get(Project, project_id)
    if project is None or project.status != ProjectStatus.active:
        return False
    existing = db.get(
        EmployeeProject, {"employee_id": employee.id, "project_id": project_id}
    )
    if existing is not None:
        return False
    db.add(EmployeeProject(employee_id=employee.id, project_id=project_id))
    db.commit()
    return True


def submit_week(
    db: Session, employee: Employee, week_start: date, which: str
) -> None:
    """Stamp the plan or actuals submission time for the week."""
    if which not in {"plan", "actuals"}:
        raise ValueError("which must be 'plan' or 'actuals'")
    status = get_week_status(db, employee, week_start)
    if status is None:
        status = WeekStatus(employee_id=employee.id, week_start=week_start)
        db.add(status)
    now = datetime.now(timezone.utc)
    if which == "plan":
        status.planned_submitted_at = now
    else:
        status.actuals_submitted_at = now
    db.commit()
