"""Business rules for Tiny Time Track Lite (no HTTP code here, so it's easy to test).

Conventions
* Weeks run Monday to Sunday; a week's ``start`` is always its Monday.
* Time is held as whole **minutes** in quarter-hour steps (0-1440 per cell)
  and only turned into hours for display and export.
* A *saved timesheet* is an immutable snapshot of a week's actual time.
  Saving locks the week; reopening removes the record so it can be edited.
"""
from __future__ import annotations

import json
import re
import sqlite3
from dataclasses import dataclass, field
from datetime import date, datetime, timedelta
from decimal import ROUND_HALF_UP, Decimal, InvalidOperation
from pathlib import Path

from sqlalchemy import delete, func, or_, select
from sqlalchemy.dialects.sqlite import insert as sqlite_insert
from sqlalchemy.exc import IntegrityError
from sqlalchemy.orm import Session, selectinload

from ttlite.models import (
    DAY_MINUTES,
    Entry,
    Project,
    Setting,
    Timesheet,
    TimesheetLine,
    TimesheetRevision,
    WeekNote,
)

DAY_NAMES = ("Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun")
FIELDS = ("planned", "actual")
STEP = 15  # minutes: entries are rounded to the nearest quarter hour
NOTE_MAX = 2000
MAX_REPORT_DAYS = 3660  # ~10 years
BACKUP_PREFIX = "tinytimetrack-lite-"
# Tables every version of the app has had; newer tables are added on restore.
CORE_TABLES = frozenset({"projects", "entries", "timesheets", "timesheet_lines"})
LOCKED_MSG = "This week is saved as a timesheet and is read-only. Reopen it to make changes."


class LockedError(Exception):
    """The week has a saved timesheet, so it is read-only until reopened."""


# --------------------------------------------------------------------------- #
#  Hours: parsing and formatting
# --------------------------------------------------------------------------- #
_HHMM = re.compile(r"^(\d{1,2}):([0-5]\d)$")
_BAD_HOURS = "isn't a number of hours (try 7.5 or 7:30)."


def parse_minutes(raw: str | None) -> int:
    """Turn what the user typed into minutes, rounded to the nearest 15.

    Accepts ``7.5``, ``7,5``, ``7:30`` and ``7h``; blank means 0.
    Raises ValueError with a message that can be shown to the user.
    """
    text = (raw or "").strip().lower().replace(",", ".")
    if text.endswith("h"):
        text = text[:-1].rstrip()
    if not text:
        return 0
    if len(text) > 10:
        raise ValueError(f"That {_BAD_HOURS}")
    match = _HHMM.match(text)
    try:
        minutes = Decimal(int(match[1]) * 60 + int(match[2])) if match else Decimal(text) * 60
    except InvalidOperation:
        raise ValueError(f"“{text}” {_BAD_HOURS}") from None
    if not minutes.is_finite():
        raise ValueError(f"“{text}” {_BAD_HOURS}")
    if minutes < 0:
        raise ValueError("Hours can't be negative.")
    rounded = int((minutes / STEP).quantize(Decimal(1), rounding=ROUND_HALF_UP)) * STEP
    if minutes > 0 and rounded == 0:  # don't let a tiny value silently clear the cell
        raise ValueError("The smallest amount is 15 minutes (0.25 h).")
    if rounded > DAY_MINUTES:
        raise ValueError("A day only has 24 hours.")
    return rounded


def fmt(minutes: int | None) -> str:
    """Minutes as compact hours text: 480 -> '8', 450 -> '7.5', 15 -> '0.25'."""
    minutes = int(minutes or 0)
    if minutes % 60 == 0:
        return str(minutes // 60)
    return format((Decimal(minutes) / 60).normalize(), "f")


def fmt_cell(minutes: int | None) -> str:
    """Like fmt() but blank for zero - used to pre-fill the grid inputs."""
    return fmt(minutes) if minutes else ""


def fmt_avg(minutes: float) -> str:
    """An average (not necessarily a quarter hour) as hours with <=2 decimals."""
    return f"{minutes / 60:.2f}".rstrip("0").rstrip(".") or "0"


# --------------------------------------------------------------------------- #
#  Dates
# --------------------------------------------------------------------------- #
def monday_of(day: date) -> date:
    return day - timedelta(days=day.weekday())


MIN_DATE = date(1900, 1, 1)
MAX_DATE = date(2999, 12, 31)


def parse_date(raw: str | None) -> date | None:
    """ISO date within 1900-2999 (anything else is treated as invalid)."""
    try:
        parsed = date.fromisoformat((raw or "").strip()[:10])
    except ValueError:
        return None
    return parsed if MIN_DATE <= parsed <= MAX_DATE else None


def parse_week(raw: str | None) -> date:
    """Monday of the week containing ``raw`` (falls back to this week)."""
    return monday_of(parse_date(raw) or date.today())


def week_dates(start: date) -> list[date]:
    return [start + timedelta(days=i) for i in range(7)]


def period_label(start: date, end: date) -> str:
    if start == end:
        return f"{start.day} {start:%b %Y}"
    if start.year != end.year:
        return f"{start.day} {start:%b %Y} – {end.day} {end:%b %Y}"
    if start.month != end.month:
        return f"{start.day} {start:%b} – {end.day} {end:%b %Y}"
    return f"{start.day} – {end.day} {end:%b %Y}"


def week_label(start: date) -> str:
    return period_label(start, start + timedelta(days=6))


def month_bounds(day: date) -> tuple[date, date]:
    first = day.replace(day=1)
    following = (first + timedelta(days=32)).replace(day=1)
    return first, following - timedelta(days=1)


def quick_ranges(today: date) -> list[tuple[str, date, date]]:
    monday = monday_of(today)
    this_month = month_bounds(today)
    last_month = month_bounds(this_month[0] - timedelta(days=1))
    return [
        ("This week", monday, monday + timedelta(days=6)),
        ("Last week", monday - timedelta(days=7), monday - timedelta(days=1)),
        ("This month", *this_month),
        ("Last month", *last_month),
        ("This year", date(today.year, 1, 1), date(today.year, 12, 31)),
        ("Last year", date(today.year - 1, 1, 1), date(today.year - 1, 12, 31)),
    ]


# --------------------------------------------------------------------------- #
#  Settings
# --------------------------------------------------------------------------- #
DEFAULT_SETTINGS = {"user_name": "", "company": "", "daily_target": "480", "show_weekend": "0"}


def get_settings(db: Session) -> dict[str, str]:
    values = dict(DEFAULT_SETTINGS)
    for key, value in db.execute(select(Setting.key, Setting.value)):
        values[key] = value
    return values


def daily_target(settings: dict[str, str]) -> int:
    """Daily target in minutes (drives the green/amber/red day totals)."""
    try:
        minutes = int(settings.get("daily_target", "480"))
    except ValueError:
        return 480
    return minutes if 0 < minutes <= DAY_MINUTES else 480


def show_weekend(settings: dict[str, str]) -> bool:
    return settings.get("show_weekend") == "1"


def save_settings(
    db: Session, *, user_name: str, company: str, daily_target_text: str, weekend: bool
) -> None:
    target = parse_minutes(daily_target_text)
    if target <= 0:
        raise ValueError("The daily target must be more than 0 hours.")
    values = {
        "user_name": _clean(user_name, 120),
        "company": _clean(company, 120),
        "daily_target": str(target),
        "show_weekend": "1" if weekend else "0",
    }
    for key, value in values.items():
        row = db.get(Setting, key)
        if row is None:
            db.add(Setting(key=key, value=value))
        else:
            row.value = value
    db.commit()


# --------------------------------------------------------------------------- #
#  Projects
# --------------------------------------------------------------------------- #
def _clean(text: str | None, limit: int) -> str:
    return " ".join((text or "").split())[:limit]


def project_label(name: str, code: str) -> str:
    return f"{name} ({code})" if code else name


def _project_fields(name: str, code: str, client: str) -> tuple[str, str, str]:
    name, code, client = _clean(name, 160), _clean(code, 40), _clean(client, 160)
    if not name:
        raise ValueError("Please give the project a name.")
    return name, code, client


def _ensure_unique(db: Session, name: str, code: str, exclude_id: int | None = None) -> None:
    stmt = select(Project.id).where(
        func.lower(Project.name) == name.lower(), func.lower(Project.code) == code.lower()
    )
    if exclude_id is not None:
        stmt = stmt.where(Project.id != exclude_id)
    if db.scalar(stmt) is not None:
        raise ValueError(f"You already have a project called “{project_label(name, code)}”.")


def add_project(db: Session, name: str, code: str = "", client: str = "") -> Project:
    name, code, client = _project_fields(name, code, client)
    _ensure_unique(db, name, code)
    project = Project(name=name, code=code, client=client)
    db.add(project)
    db.commit()
    return project


def get_project(db: Session, project_id: int) -> Project:
    project = db.get(Project, project_id)
    if project is None:
        raise ValueError("That project no longer exists.")
    return project


def update_project(db: Session, project_id: int, name: str, code: str, client: str) -> Project:
    project = get_project(db, project_id)
    name, code, client = _project_fields(name, code, client)
    _ensure_unique(db, name, code, exclude_id=project_id)
    project.name, project.code, project.client = name, code, client
    db.commit()
    return project


def set_project_active(db: Session, project_id: int, active: bool) -> Project:
    project = get_project(db, project_id)
    project.active = active
    db.commit()
    return project


def delete_project(db: Session, project_id: int) -> str:
    """Delete a project that has never had any time recorded against it."""
    project = get_project(db, project_id)
    used = db.scalar(
        select(func.count())
        .select_from(Entry)
        .where(Entry.project_id == project_id, or_(Entry.planned > 0, Entry.actual > 0))
    )
    recorded = db.scalar(
        select(func.count()).select_from(TimesheetLine).where(TimesheetLine.project_id == project_id)
    )
    if used or recorded:
        raise ValueError(
            f"“{project.name}” has hours recorded, so it can't be deleted — archive it instead."
        )
    name = project.name
    db.execute(delete(Entry).where(Entry.project_id == project_id))
    db.delete(project)
    db.commit()
    return name


def has_projects(db: Session) -> bool:
    return db.scalar(select(Project.id).limit(1)) is not None


@dataclass
class ProjectStats:
    project: Project
    planned: int = 0
    actual: int = 0
    saved: int = 0
    last_day: date | None = None

    @property
    def deletable(self) -> bool:
        return not (self.planned or self.actual or self.saved)


def project_overview(db: Session) -> list[ProjectStats]:
    """Every project with its planned / actual / saved minutes to date."""
    sums = {
        pid: (planned or 0, actual or 0, last)
        for pid, planned, actual, last in db.execute(
            select(Entry.project_id, func.sum(Entry.planned), func.sum(Entry.actual), func.max(Entry.day))
            .group_by(Entry.project_id)
        )
    }
    saved = {
        pid: total or 0
        for pid, total in db.execute(
            select(TimesheetLine.project_id, func.sum(TimesheetLine.total))
            .where(TimesheetLine.project_id.is_not(None))
            .group_by(TimesheetLine.project_id)
        )
    }
    stats = []
    for project in db.scalars(select(Project).order_by(Project.active.desc(), func.lower(Project.name))):
        planned, actual, last = sums.get(project.id, (0, 0, None))
        stats.append(ProjectStats(project, int(planned), int(actual), int(saved.get(project.id, 0)), last))
    return stats


# --------------------------------------------------------------------------- #
#  The week grid
# --------------------------------------------------------------------------- #
@dataclass
class Row:
    project: Project
    planned: list[int]  # minutes, Mon..Sun
    actual: list[int]

    @property
    def planned_total(self) -> int:
        return sum(self.planned)

    @property
    def actual_total(self) -> int:
        return sum(self.actual)


@dataclass
class Week:
    start: date
    rows: list[Row]
    visible: list[int]  # day indexes (0=Mon) shown as columns
    note: str = ""
    timesheet: Timesheet | None = None

    @property
    def dates(self) -> list[date]:
        return week_dates(self.start)

    @property
    def locked(self) -> bool:
        return self.timesheet is not None

    def day_totals(self, kind: str) -> list[int]:
        return [sum(getattr(row, kind)[i] for row in self.rows) for i in range(7)]

    @property
    def planned_total(self) -> int:
        return sum(row.planned_total for row in self.rows)

    @property
    def actual_total(self) -> int:
        return sum(row.actual_total for row in self.rows)


def get_timesheet(db: Session, start: date) -> Timesheet | None:
    return db.scalar(
        select(Timesheet).options(selectinload(Timesheet.lines)).where(Timesheet.week_start == start)
    )


def is_locked(db: Session, start: date) -> bool:
    return db.scalar(select(Timesheet.id).where(Timesheet.week_start == start)) is not None


def _require_unlocked(db: Session, start: date) -> None:
    if is_locked(db, start):
        raise LockedError(LOCKED_MSG)


def build_week(db: Session, start: date, weekend: bool = False) -> Week:
    """Rows for every active project (plus archived ones with time that week)."""
    end = start + timedelta(days=6)
    cells: dict[int, dict[int, Entry]] = {}
    for entry in db.scalars(select(Entry).where(Entry.day.between(start, end))):
        cells.setdefault(entry.project_id, {})[(entry.day - start).days] = entry
    used = [pid for pid, days in cells.items() if any(e.planned or e.actual for e in days.values())]
    timesheet = db.scalar(select(Timesheet).where(Timesheet.week_start == start))

    query = select(Project).order_by(Project.active.desc(), func.lower(Project.name))
    if timesheet is None:
        query = query.where(or_(Project.active.is_(True), Project.id.in_(used)))
    else:  # a saved week only shows the projects that have time in it
        query = query.where(Project.id.in_(used))

    rows = []
    for project in db.scalars(query):
        days = cells.get(project.id, {})
        rows.append(
            Row(
                project,
                [days[i].planned if i in days else 0 for i in range(7)],
                [days[i].actual if i in days else 0 for i in range(7)],
            )
        )
    weekend_used = any(r.planned[i] or r.actual[i] for r in rows for i in (5, 6))
    visible = list(range(7)) if (weekend or weekend_used) else list(range(5))
    note = db.get(WeekNote, start)
    return Week(start, rows, visible, note.text if note else "", timesheet)


def save_cell(db: Session, *, project_id: int, day: date, field: str, raw: str) -> int:
    """Store planned or actual time for one project/day. Returns the minutes saved."""
    if field not in FIELDS:
        raise ValueError("Unknown field.")
    get_project(db, project_id)
    _require_unlocked(db, monday_of(day))
    minutes = parse_minutes(raw)
    column = getattr(Entry, field)
    if minutes:
        others = db.scalar(
            select(func.coalesce(func.sum(column), 0)).where(
                Entry.day == day, Entry.project_id != project_id
            )
        )
        if others + minutes > DAY_MINUTES:
            raise ValueError(
                f"That would put {day:%a} {day.day} {day:%b} over 24 hours in total "
                f"({fmt(DAY_MINUTES - others)} h left that day)."
            )
    # Atomic upsert: two quick saves of the same cell (e.g. plan and actual
    # typed back to back) can't both try to insert the row.
    values = {"project_id": project_id, "day": day, "planned": 0, "actual": 0}
    values[field] = minutes
    db.execute(
        sqlite_insert(Entry)
        .values(**values)
        .on_conflict_do_update(index_elements=["project_id", "day"], set_={field: minutes})
    )
    db.execute(  # keep the table free of empty rows
        delete(Entry).where(
            Entry.project_id == project_id, Entry.day == day, Entry.planned == 0, Entry.actual == 0
        )
    )
    db.commit()
    return minutes


def save_week_note(db: Session, start: date, text: str) -> None:
    _require_unlocked(db, start)
    text = (text or "").replace("\r\n", "\n")[:NOTE_MAX]
    note = db.get(WeekNote, start)
    if note is None:
        if not text.strip():
            return
        db.add(WeekNote(week_start=start, text=text))
    else:
        note.text = text
    db.commit()


def copy_last_week_plan(db: Session, start: date) -> int:
    """Copy last week's planned hours into empty planned cells this week."""
    _require_unlocked(db, start)
    previous = db.scalars(
        select(Entry)
        .join(Project)
        .where(
            Entry.day.between(start - timedelta(days=7), start - timedelta(days=1)),
            Entry.planned > 0,
            Project.active.is_(True),
        )
    ).all()
    current = {
        (e.project_id, e.day): e
        for e in db.scalars(select(Entry).where(Entry.day.between(start, start + timedelta(days=6))))
    }
    per_day: dict[date, int] = {}
    for e in current.values():
        per_day[e.day] = per_day.get(e.day, 0) + e.planned
    copied = 0
    for old in previous:
        day = old.day + timedelta(days=7)
        entry = current.get((old.project_id, day))
        if (entry is not None and entry.planned) or per_day.get(day, 0) + old.planned > DAY_MINUTES:
            continue
        if entry is None:
            entry = Entry(project_id=old.project_id, day=day, planned=0, actual=0)
            db.add(entry)
            current[(old.project_id, day)] = entry
        entry.planned = old.planned
        per_day[day] = per_day.get(day, 0) + old.planned
        copied += 1
    db.commit()
    return copied


def fill_actuals_from_plan(db: Session, start: date, today: date) -> int:
    """Set actual = planned for empty actual cells, for days up to today."""
    _require_unlocked(db, start)
    last = min(start + timedelta(days=6), today)
    if last < start:
        return 0
    entries = db.scalars(
        select(Entry).where(Entry.day.between(start, last)).order_by(Entry.day, Entry.id)
    ).all()
    per_day: dict[date, int] = {}
    for e in entries:
        per_day[e.day] = per_day.get(e.day, 0) + e.actual
    filled = 0
    for e in entries:
        if e.planned and not e.actual and per_day[e.day] + e.planned <= DAY_MINUTES:
            e.actual = e.planned
            per_day[e.day] += e.planned
            filled += 1
    db.commit()
    return filled


# --------------------------------------------------------------------------- #
#  Saved timesheets (the permanent record)
# --------------------------------------------------------------------------- #
def save_timesheet(db: Session, start: date) -> Timesheet:
    """Snapshot this week's actual time as a timesheet record and lock the week."""
    if is_locked(db, start):
        raise ValueError("This week's timesheet is already saved.")
    week = build_week(db, start, weekend=True)
    lines = [
        TimesheetLine(
            project_id=row.project.id,
            project_name=row.project.name,
            project_code=row.project.code,
            total=row.actual_total,
            **{f"m{i}": minutes for i, minutes in enumerate(row.actual)},
        )
        for row in week.rows
        if row.actual_total
    ]
    if not lines:
        raise ValueError("There's no actual time entered for this week, so there's nothing to save yet.")
    sheet = Timesheet(week_start=start, total=sum(line.total for line in lines), note=week.note, lines=lines)
    db.add(sheet)
    try:
        db.commit()
    except IntegrityError:  # e.g. a double-click saved it a moment ago
        db.rollback()
        raise ValueError("This week's timesheet is already saved.") from None
    return sheet


def reopen_timesheet(db: Session, start: date) -> None:
    """Unlock a saved week. The saved version is kept as a revision, never lost."""
    sheet = get_timesheet(db, start)
    if sheet is None:
        raise ValueError("This week has no saved timesheet.")
    lines = [
        {"code": tl.project_code, "name": tl.project_name, "days": tl.days, "total": tl.total}
        for tl in sheet.lines
    ]
    db.add(TimesheetRevision(week_start=start, saved_at=sheet.saved_at, total=sheet.total,
                             note=sheet.note, lines=json.dumps(lines)))
    db.delete(sheet)
    db.commit()


def revisions(db: Session, start: date | None = None) -> list[TimesheetRevision]:
    """Earlier (reopened) versions of saved timesheets, newest first."""
    stmt = select(TimesheetRevision).order_by(TimesheetRevision.week_start.desc(), TimesheetRevision.reopened_at.desc())
    if start is not None:
        stmt = stmt.where(TimesheetRevision.week_start == start)
    return list(db.scalars(stmt))


def revision_lines(revision: TimesheetRevision) -> list[SheetLine]:
    return [SheetLine(item["code"], item["name"], list(item["days"])) for item in json.loads(revision.lines or "[]")]


def list_timesheets(db: Session) -> list[Timesheet]:
    return list(db.scalars(select(Timesheet).order_by(Timesheet.week_start.desc())))


def unsaved_weeks(db: Session) -> list[tuple[date, int]]:
    """Weeks that have actual time but no saved timesheet yet (newest first)."""
    saved = set(db.scalars(select(Timesheet.week_start)))
    totals: dict[date, int] = {}
    for day, minutes in db.execute(
        select(Entry.day, func.sum(Entry.actual)).where(Entry.actual > 0).group_by(Entry.day)
    ):
        start = monday_of(day)
        if start not in saved:
            totals[start] = totals.get(start, 0) + int(minutes)
    return sorted(totals.items(), reverse=True)


@dataclass
class SheetLine:
    code: str
    name: str
    days: list[int]

    @property
    def total(self) -> int:
        return sum(self.days)


@dataclass
class SheetDoc:
    """A week's actual time, from the saved record if there is one (else live)."""

    start: date
    lines: list[SheetLine]
    visible: list[int]
    note: str
    record: Timesheet | None

    @property
    def dates(self) -> list[date]:
        return week_dates(self.start)

    @property
    def day_totals(self) -> list[int]:
        return [sum(line.days[i] for line in self.lines) for i in range(7)]

    @property
    def total(self) -> int:
        return sum(line.total for line in self.lines)


def timesheet_document(db: Session, start: date, weekend: bool = False) -> SheetDoc:
    record = get_timesheet(db, start)
    if record is not None:
        lines = [SheetLine(tl.project_code, tl.project_name, tl.days) for tl in record.lines]
        note = record.note
    else:
        week = build_week(db, start, weekend=True)
        lines = [SheetLine(r.project.code, r.project.name, r.actual) for r in week.rows if r.actual_total]
        note = week.note
    weekend = weekend or any(line.days[5] or line.days[6] for line in lines)
    return SheetDoc(start, lines, list(range(7 if weekend else 5)), note, record)


# --------------------------------------------------------------------------- #
#  Period reports ("totalled-out timesheet" for any date range)
# --------------------------------------------------------------------------- #
@dataclass
class ReportLine:
    day: date
    project_id: int | None
    code: str
    name: str  # as recorded (saved weeks keep the name the project had then)
    minutes: int
    saved: bool

    @property
    def key(self) -> object:
        return self.project_id if self.project_id is not None else (self.code, self.name)


@dataclass
class ReportProject:
    key: object
    code: str
    name: str  # current name (or the recorded one if the project is gone)
    minutes: int = 0
    share: float = 0.0  # percent of the period total
    recorded_as: set[str] = field(default_factory=set)  # older names in saved records

    @property
    def display(self) -> str:
        if not self.recorded_as:
            return self.name
        return f"{self.name} (recorded as {', '.join(sorted(self.recorded_as))})"


@dataclass
class ReportWeek:
    start: date
    minutes: int
    saved: bool
    partial: bool  # the week sticks out of the chosen period


@dataclass
class MatrixColumn:
    label: str
    start: date
    end: date
    partial: bool = False


@dataclass
class MatrixRow:
    project: ReportProject
    cells: list[int]

    @property
    def total(self) -> int:
        return sum(self.cells)


@dataclass
class Matrix:
    """The totalled-out timesheet: projects x weeks (or months), totals both ways."""

    unit: str  # "week" or "month"
    columns: list[MatrixColumn]
    rows: list[MatrixRow]

    @property
    def column_totals(self) -> list[int]:
        return [sum(row.cells[i] for row in self.rows) for i in range(len(self.columns))]

    @property
    def total(self) -> int:
        return sum(row.total for row in self.rows)

    @property
    def has_partial(self) -> bool:
        return any(c.partial for c in self.columns)


MATRIX_MAX_WEEKS = 14  # longer periods are grouped by month


@dataclass
class Report:
    start: date
    end: date
    saved_only: bool
    lines: list[ReportLine]
    projects: list[ReportProject]
    weeks: list[ReportWeek]
    unsaved_minutes: int  # time in weeks not saved yet (included unless saved_only)
    matrix: Matrix

    @property
    def total(self) -> int:
        return sum(line.minutes for line in self.lines)

    @property
    def label(self) -> str:
        return period_label(self.start, self.end)

    @property
    def days_worked(self) -> int:
        return len({line.day for line in self.lines})

    @property
    def avg_per_day(self) -> str:
        return fmt_avg(self.total / self.days_worked) if self.days_worked else "0"

    @property
    def includes_unsaved(self) -> bool:
        return any(not line.saved for line in self.lines)

    @property
    def active_weeks(self) -> list[ReportWeek]:
        """Weeks with time (or a saved timesheet); empty weeks are left out of lists."""
        return [w for w in self.weeks if w.minutes or w.saved]

    @property
    def empty_weeks(self) -> int:
        return len(self.weeks) - len(self.active_weeks)


def clamp_range(start: date, end: date) -> tuple[date, date]:
    if end < start:
        start, end = end, start
    if (end - start).days > MAX_REPORT_DAYS:
        start = end - timedelta(days=MAX_REPORT_DAYS)
    return start, end


def _range_label(first: date, last: date) -> str:
    """Short label for part of a week/month: '18–22 Nov', '31 Dec–3 Jan', '5 Jan'."""
    if first == last:
        return f"{first.day} {first:%b}"
    if first.month == last.month:
        return f"{first.day}–{last.day} {last:%b}"
    return f"{first.day} {first:%b}–{last.day} {last:%b}"


def _matrix_columns(start: date, end: date) -> tuple[str, list[MatrixColumn]]:
    """Week columns for periods up to MATRIX_MAX_WEEKS weeks, otherwise months."""
    first, last = monday_of(start), monday_of(end)
    columns: list[MatrixColumn] = []
    if (last - first).days // 7 + 1 <= MATRIX_MAX_WEEKS:
        week = first
        while week <= last:
            col_start, col_end = max(week, start), min(week + timedelta(days=6), end)
            partial = (col_start, col_end) != (week, week + timedelta(days=6))
            label = _range_label(col_start, col_end) if partial else f"{col_start.day} {col_start:%b}"
            columns.append(MatrixColumn(label, col_start, col_end, partial))
            week += timedelta(days=7)
        return "week", columns
    month = start.replace(day=1)
    while month <= end:
        m_start, m_end = month_bounds(month)
        col_start, col_end = max(m_start, start), min(m_end, end)
        partial = (col_start, col_end) != (m_start, m_end)
        label = f"{_range_label(col_start, col_end)} {month:%Y}" if partial else f"{month:%b %Y}"
        columns.append(MatrixColumn(label, col_start, col_end, partial))
        month = m_end + timedelta(days=1)
    return "month", columns


def build_matrix(start: date, end: date, projects: list[ReportProject], lines: list[ReportLine]) -> Matrix:
    unit, columns = _matrix_columns(start, end)
    index = {p.key: i for i, p in enumerate(projects)}
    cells = [[0] * len(columns) for _ in projects]
    col = 0
    for line in lines:  # lines are sorted by day
        while line.day > columns[col].end:
            col += 1
        cells[index[line.key]][col] += line.minutes
    return Matrix(unit, columns, [MatrixRow(p, c) for p, c in zip(projects, cells)])


def build_report(db: Session, start: date, end: date, saved_only: bool = False) -> Report:
    """Actual time per day/project/week over a period.

    Weeks with a saved timesheet use the saved record; other weeks use the
    live entries (marked unsaved) unless ``saved_only`` is set.
    """
    start, end = clamp_range(start, end)
    first, last = monday_of(start), monday_of(end)
    sheets = {
        ts.week_start: ts
        for ts in db.scalars(
            select(Timesheet).options(selectinload(Timesheet.lines)).where(Timesheet.week_start.between(first, last))
        )
    }
    lines: list[ReportLine] = []
    for sheet in sheets.values():
        for tl in sheet.lines:
            for i, minutes in enumerate(tl.days):
                day = sheet.week_start + timedelta(days=i)
                if minutes and start <= day <= end:
                    lines.append(ReportLine(day, tl.project_id, tl.project_code, tl.project_name, minutes, True))

    projects = {p.id: p for p in db.scalars(select(Project))}
    unsaved = 0
    for e in db.scalars(select(Entry).where(Entry.day.between(start, end), Entry.actual > 0)):
        if monday_of(e.day) in sheets:
            continue  # the saved record wins for saved weeks
        unsaved += e.actual
        if not saved_only:
            p = projects.get(e.project_id)
            lines.append(ReportLine(e.day, e.project_id, p.code if p else "", p.name if p else "?", e.actual, False))
    lines.sort(key=lambda line: (line.day, line.name.lower(), line.code.lower()))

    # One summary line per project, under its current name; if saved records
    # used an older name, that's shown too ("recorded as ...").
    groups: dict[object, ReportProject] = {}
    for line in lines:
        group = groups.get(line.key)
        if group is None:
            current = projects.get(line.project_id) if line.project_id is not None else None
            group = groups[line.key] = ReportProject(
                line.key, current.code if current else line.code, current.name if current else line.name
            )
        if line.name != group.name:
            group.recorded_as.add(line.name)
        group.minutes += line.minutes
    total = sum(line.minutes for line in lines)
    for group in groups.values():
        group.share = group.minutes * 100 / total if total else 0.0

    per_week: dict[date, int] = {}
    for line in lines:
        week = monday_of(line.day)
        per_week[week] = per_week.get(week, 0) + line.minutes
    weeks = []
    week = first
    while week <= last:
        partial = week < start or week + timedelta(days=6) > end
        weeks.append(ReportWeek(week, per_week.get(week, 0), week in sheets, partial))
        week += timedelta(days=7)

    ordered = sorted(groups.values(), key=lambda g: (-g.minutes, g.name.lower()))
    matrix = build_matrix(start, end, ordered, lines)
    return Report(start, end, saved_only, lines, ordered, weeks, unsaved, matrix)


def saved_totals(db: Session) -> Report | None:
    """All-time totals per project across every saved timesheet."""
    first = db.scalar(select(func.min(Timesheet.week_start)))
    last = db.scalar(select(func.max(Timesheet.week_start)))
    if first is None or last is None:
        return None
    return build_report(db, first, last + timedelta(days=6), saved_only=True)


# --------------------------------------------------------------------------- #
#  Backups
# --------------------------------------------------------------------------- #
@dataclass
class BackupInfo:
    path: Path
    size: int
    modified: datetime

    @property
    def name(self) -> str:
        return self.path.name


def list_backups(folder: Path) -> list[BackupInfo]:
    if not folder.is_dir():
        return []
    found = []
    for path in folder.glob(f"{BACKUP_PREFIX}*.db"):
        stat = path.stat()
        info = BackupInfo(path, stat.st_size, datetime.fromtimestamp(stat.st_mtime).replace(microsecond=0))
        found.append((stat.st_mtime_ns, info))
    # Newest first, by nanosecond mtime so backups made in the same second keep their order.
    return [info for _, info in sorted(found, key=lambda pair: pair[0], reverse=True)]


def make_backup(db_path: Path, folder: Path, keep: int = 30) -> Path:
    """Consistent copy of the database (SQLite online backup); keeps the newest ``keep``."""
    folder.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    target = folder / f"{BACKUP_PREFIX}{stamp}.db"
    n = 1
    while target.exists():
        n += 1
        target = folder / f"{BACKUP_PREFIX}{stamp}-{n}.db"
    source = sqlite3.connect(db_path)
    try:
        copy = sqlite3.connect(target)
        try:
            source.backup(copy)
        finally:
            copy.close()
    finally:
        source.close()
    for old in list_backups(folder)[keep:]:
        old.path.unlink(missing_ok=True)
    return target


def _has_data(db_path: Path) -> bool:
    con = sqlite3.connect(db_path)
    try:
        return bool(con.execute("SELECT EXISTS(SELECT 1 FROM projects)").fetchone()[0])
    except sqlite3.Error:
        return False
    finally:
        con.close()


def restore_backup(db_path: Path, folder: Path, name: str) -> Path:
    """Replace the live database with the backup called ``name``.

    The backup is checked and loaded into memory first, then a safety backup
    of the current data is made, then the backup is written into the live
    database with SQLite's backup API (never a raw file copy). The caller must
    close the app's own database connections beforehand. Returns the safety
    backup's path.
    """
    chosen = next((b.path for b in list_backups(folder) if b.name == name), None)
    if chosen is None:
        raise ValueError("That backup no longer exists.")
    memory = sqlite3.connect(":memory:")
    try:
        source = sqlite3.connect(f"{chosen.resolve().as_uri()}?mode=ro", uri=True)
        try:
            source.backup(memory)
        finally:
            source.close()
        tables = {row[0] for row in memory.execute("SELECT name FROM sqlite_master WHERE type='table'")}
        if not CORE_TABLES <= tables:
            raise ValueError(f"{name} isn't a {BACKUP_PREFIX.rstrip('-')} backup.")
        if memory.execute("PRAGMA integrity_check").fetchone()[0] != "ok":
            raise ValueError(f"{name} is damaged and can't be restored.")
        safety = make_backup(db_path, folder)
        live = sqlite3.connect(db_path)
        try:
            memory.backup(live)
        finally:
            live.close()
    except sqlite3.DatabaseError as exc:
        raise ValueError(f"{name} can't be read as a database ({exc}).") from None
    finally:
        memory.close()
    upgrade_schema(db_path)  # a backup from an older version may lack newer tables
    return safety


def upgrade_schema(db_path: Path) -> None:
    """Add any tables this version needs that the database doesn't have yet."""
    from sqlalchemy import create_engine

    from ttlite.db import Base

    engine = create_engine(f"sqlite:///{db_path}")
    try:
        Base.metadata.create_all(engine)
    finally:
        engine.dispose()


def backup_if_due(db_path: Path, folder: Path, max_age: timedelta = timedelta(hours=20)) -> Path | None:
    """Automatic backup at start-up, at most about once a day."""
    if not db_path.exists() or not _has_data(db_path):
        return None
    backups = list_backups(folder)
    if backups and datetime.now() - backups[0].modified < max_age:
        return None
    return make_backup(db_path, folder)
