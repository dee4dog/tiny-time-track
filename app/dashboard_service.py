"""Aggregations for the People and This-week sections of the dashboard."""
from __future__ import annotations

from dataclasses import dataclass
from datetime import date, timedelta
from decimal import Decimal

from sqlalchemy import select
from sqlalchemy.orm import Session

from app.costing import ONE, ZERO, RateResolver, overtime_for_entry
from app.models import Employee, TimeEntry, WeekStatus
from app.weeks import monday_of


@dataclass
class PersonStats:
    employee: Employee
    capacity: int                 # available hours per year
    planned: Decimal = ZERO       # year-to-date
    actual: Decimal = ZERO
    overtime: Decimal = ZERO      # of which over 8h/day (derived)
    cost: Decimal = ZERO

    @property
    def utilisation(self) -> Decimal | None:
        """Actual hours as a fraction of annual capacity (year-to-date)."""
        if not self.capacity:
            return None
        return self.actual / Decimal(self.capacity)


def people_overview(db: Session, *, year: int | None = None) -> list[PersonStats]:
    """Per-employee planned/actual/overtime/cost for the given calendar year."""
    year = year or date.today().year
    year_start = date(year, 1, 1)
    year_end = date(year, 12, 31)
    resolver = RateResolver(db)
    overtime_factor = resolver.overtime_factor

    employees = list(
        db.scalars(select(Employee).where(Employee.active).order_by(Employee.name))
    )
    stats = {e.id: PersonStats(employee=e, capacity=e.available_hours_per_year) for e in employees}

    # Column tuples, not ORM objects: a full year of entries is thousands of
    # rows, and hydration is the expensive part.
    stmt = select(
        TimeEntry.employee_id,
        TimeEntry.week_start,
        TimeEntry.day,
        TimeEntry.planned_hours,
        TimeEntry.actual_hours,
    ).where(TimeEntry.week_start >= year_start, TimeEntry.week_start <= year_end)
    for e in db.execute(stmt):
        s = stats.get(e.employee_id)
        if s is None:
            continue  # entry belongs to a deactivated employee; skip in this view
        worked = e.actual_hours or ZERO
        overtime = overtime_for_entry(
            worked, resolver.day_total(e.employee_id, e.week_start, e.day)
        )
        paid = worked + overtime * (overtime_factor - ONE)
        s.planned += e.planned_hours or ZERO
        s.actual += worked
        s.overtime += overtime
        s.cost += paid * resolver.hourly_rate(e.employee_id, e.week_start)

    return list(stats.values())


@dataclass
class WeekCompliance:
    week_start: date
    plan: bool
    actuals: bool


def compliance_recent(
    db: Session, employees: list[Employee], *, weeks: int = 6
) -> dict[int, list[WeekCompliance]]:
    """Plan/actuals flags for the last ``weeks`` weeks, per employee.

    Lists run oldest -> newest so the newest week renders on the right, as
    the People page legend says. One query covers everyone (instead of
    weeks x employees point lookups).
    """
    this_monday = monday_of(date.today())
    week_list = [this_monday - timedelta(weeks=i) for i in range(weeks - 1, -1, -1)]

    stmt = select(WeekStatus).where(
        WeekStatus.employee_id.in_([e.id for e in employees]),
        WeekStatus.week_start >= week_list[0],
    )
    by_key = {(st.employee_id, st.week_start): st for st in db.scalars(stmt)}

    out: dict[int, list[WeekCompliance]] = {}
    for e in employees:
        flags: list[WeekCompliance] = []
        for wk in week_list:
            st = by_key.get((e.id, wk))
            flags.append(
                WeekCompliance(
                    week_start=wk,
                    plan=bool(st and st.planned_submitted_at),
                    actuals=bool(st and st.actuals_submitted_at),
                )
            )
        out[e.id] = flags
    return out


@dataclass
class BoardRow:
    employee: Employee
    plan: bool
    actuals: bool


def this_week_board(db: Session) -> tuple[date, list[BoardRow]]:
    """Current-week submission status for every active employee."""
    this_monday = monday_of(date.today())
    employees = list(
        db.scalars(select(Employee).where(Employee.active).order_by(Employee.name))
    )
    statuses = {
        st.employee_id: st
        for st in db.scalars(
            select(WeekStatus).where(WeekStatus.week_start == this_monday)
        )
    }
    rows: list[BoardRow] = []
    for e in employees:
        st = statuses.get(e.id)
        rows.append(
            BoardRow(
                employee=e,
                plan=bool(st and st.planned_submitted_at),
                actuals=bool(st and st.actuals_submitted_at),
            )
        )
    return this_monday, rows
