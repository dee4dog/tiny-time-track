"""Aggregations for the People and This-week sections of the dashboard."""
from __future__ import annotations

from dataclasses import dataclass
from datetime import date, timedelta
from decimal import Decimal

from sqlalchemy import select
from sqlalchemy.orm import Session

from app.costing import ZERO, RateResolver
from app.models import Employee, TimeEntry, WeekStatus
from app.settings_store import get_decimal
from app.weeks import monday_of


@dataclass
class PersonStats:
    employee: Employee
    capacity: int                 # available hours per year
    planned: Decimal = ZERO       # year-to-date
    actual: Decimal = ZERO
    overtime: Decimal = ZERO
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
    overtime_factor = get_decimal(db, "overtime_factor")

    employees = list(
        db.scalars(select(Employee).where(Employee.active).order_by(Employee.name))
    )
    stats = {e.id: PersonStats(employee=e, capacity=e.available_hours_per_year) for e in employees}

    stmt = select(TimeEntry).where(
        TimeEntry.week_start >= year_start, TimeEntry.week_start <= year_end
    )
    for e in db.scalars(stmt):
        s = stats.get(e.employee_id)
        if s is None:
            continue  # entry belongs to a deactivated employee; skip in this view
        worked = (e.actual_hours or ZERO) + (e.overtime_hours or ZERO)
        paid = (e.actual_hours or ZERO) + (e.overtime_hours or ZERO) * overtime_factor
        s.planned += e.planned_hours or ZERO
        s.actual += worked
        s.overtime += e.overtime_hours or ZERO
        s.cost += paid * resolver.hourly_rate(e.employee_id, e.week_start)

    return list(stats.values())


@dataclass
class WeekCompliance:
    week_start: date
    plan: bool
    actuals: bool


def compliance_recent(
    db: Session, employee: Employee, *, weeks: int = 6
) -> list[WeekCompliance]:
    """Plan/actuals submission flags for the last ``weeks`` weeks (newest first)."""
    this_monday = monday_of(date.today())
    out: list[WeekCompliance] = []
    for i in range(weeks):
        wk = this_monday - timedelta(weeks=i)
        st = db.get(WeekStatus, {"employee_id": employee.id, "week_start": wk})
        out.append(
            WeekCompliance(
                week_start=wk,
                plan=bool(st and st.planned_submitted_at),
                actuals=bool(st and st.actuals_submitted_at),
            )
        )
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
    rows: list[BoardRow] = []
    for e in employees:
        st = db.get(WeekStatus, {"employee_id": e.id, "week_start": this_monday})
        rows.append(
            BoardRow(
                employee=e,
                plan=bool(st and st.planned_submitted_at),
                actuals=bool(st and st.actuals_submitted_at),
            )
        )
    return this_monday, rows
