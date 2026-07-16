"""Cost & profitability calculations (the heart of the manager dashboard).

Business rules (brief section 5):

* Hourly cost-to-company for employee E in week W:
      salary_effective_in_W x overhead_multiplier / available_hours_per_year
  The salary effective in W comes from rate_history (most recent row whose
  effective_date <= the week's Monday), so past weeks are costed at the salary
  that applied then - not today's salary.

* Project cost to date:
      sum over entries of actual_hours x rate
  where ``rate`` is that employee's hourly cost for the entry's week.

* Profit = fee - cost.   Margin % = profit / fee.
  Non-billable projects report cost only (no profit/margin).

All money is ZAR ``Decimal``. ≤25 staff, so straightforward Python aggregation
over the entries is plenty fast.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from datetime import date, timedelta
from decimal import Decimal

from sqlalchemy import select
from sqlalchemy.orm import Session

from app.models import Employee, Project, RateHistory, TimeEntry
from app.settings_store import get_decimal
from app.weeks import monday_of

ZERO = Decimal("0")
ONE = Decimal("1")

# Hours an employee can work in one day before the surplus counts as overtime.
DAY_REGULAR_HOURS = Decimal("8")


def overtime_for_entry(actual: Decimal, day_total: Decimal) -> Decimal:
    """Overtime portion of a single entry's actual hours.

    Overtime is measured per employee per day: anything over 8 hours across
    *all* that day's projects is overtime. That day surplus is apportioned
    back to each entry in proportion to its share of the day's hours, so a
    9-hour day split 6h/3h across two projects books 2/3 and 1/3 of the 1
    overtime hour to those projects.
    """
    if actual <= ZERO or day_total <= ZERO:
        return ZERO
    day_overtime = day_total - DAY_REGULAR_HOURS
    if day_overtime <= ZERO:
        return ZERO
    return actual * day_overtime / day_total


# --------------------------------------------------------------------------- #
#  Rates
# --------------------------------------------------------------------------- #
class RateResolver:
    """Resolves an employee's hourly cost for a given week, with caching.

    Reuse one resolver across a whole dashboard render so each employee's
    rate-history and the global overhead multiplier are read only once.
    """

    def __init__(self, db: Session):
        self.db = db
        self.overhead = get_decimal(db, "overhead_multiplier")
        self.overtime_factor = get_decimal(db, "overtime_factor")
        self._employees: dict[int, Employee] = {}
        self._history: dict[int, list[RateHistory]] = {}
        self._cache: dict[tuple[int, date], Decimal] = {}
        # Lazily-built total actual hours per (employee, week_start, day), summed
        # across all projects — needed to split per-day overtime.
        self._day_totals: dict[tuple[int, date, int], Decimal] | None = None

    def day_total(self, employee_id: int, week_start: date, day: int) -> Decimal:
        """Total actual hours this employee logged that day across all projects."""
        if self._day_totals is None:
            self._day_totals = {}
            stmt = select(
                TimeEntry.employee_id, TimeEntry.week_start,
                TimeEntry.day, TimeEntry.actual_hours,
            )
            for emp_id, wk, d, actual in self.db.execute(stmt):
                key = (emp_id, wk, d)
                self._day_totals[key] = self._day_totals.get(key, ZERO) + (actual or ZERO)
        return self._day_totals.get((employee_id, week_start, day), ZERO)

    def _employee(self, employee_id: int) -> Employee | None:
        if employee_id not in self._employees:
            self._employees[employee_id] = self.db.get(Employee, employee_id)
        return self._employees[employee_id]

    def _rows(self, employee_id: int) -> list[RateHistory]:
        if employee_id not in self._history:
            stmt = (
                select(RateHistory)
                .where(RateHistory.employee_id == employee_id)
                .order_by(RateHistory.effective_date)
            )
            self._history[employee_id] = list(self.db.scalars(stmt))
        return self._history[employee_id]

    def effective_salary(self, employee_id: int, week_start: date) -> Decimal:
        """Annual salary in effect for the week (rate_history, else current)."""
        rows = self._rows(employee_id)
        chosen: Decimal | None = None
        for row in rows:  # ordered ascending by effective_date
            if row.effective_date <= week_start:
                chosen = row.annual_salary
            else:
                break
        if chosen is not None:
            return chosen
        emp = self._employee(employee_id)
        return emp.annual_salary if emp else ZERO

    def hourly_rate(self, employee_id: int, week_start: date) -> Decimal:
        """Cost-to-company per hour for this employee in this week."""
        key = (employee_id, week_start)
        if key in self._cache:
            return self._cache[key]
        emp = self._employee(employee_id)
        available = Decimal(emp.available_hours_per_year) if emp and emp.available_hours_per_year else Decimal("1760")
        salary = self.effective_salary(employee_id, week_start)
        rate = (salary * self.overhead / available) if available else ZERO
        self._cache[key] = rate
        return rate


# --------------------------------------------------------------------------- #
#  Project costing
# --------------------------------------------------------------------------- #
@dataclass
class EmployeeBreakdown:
    employee_id: int
    name: str
    hours: Decimal = ZERO          # actual hours worked
    overtime: Decimal = ZERO       # of which over 8h/day (derived)
    cost: Decimal = ZERO


@dataclass
class ProjectSummary:
    project: Project
    hours: Decimal = ZERO          # total worked (actual) hours
    cost: Decimal = ZERO
    by_employee: dict[int, EmployeeBreakdown] = field(default_factory=dict)
    weekly_cost: dict[date, Decimal] = field(default_factory=dict)
    weekly_planned: dict[date, Decimal] = field(default_factory=dict)
    weekly_actual: dict[date, Decimal] = field(default_factory=dict)

    @property
    def fee(self) -> Decimal:
        return self.project.fee or ZERO

    @property
    def billable(self) -> bool:
        return self.project.billable

    @property
    def profit(self) -> Decimal | None:
        if not self.billable:
            return None
        return self.fee - self.cost

    @property
    def margin(self) -> Decimal | None:
        """Profit as a fraction of fee (e.g. 0.25 = 25%). None if N/A."""
        if not self.billable or self.fee == 0:
            return None
        return (self.fee - self.cost) / self.fee


# Only the columns the aggregations below need. Selecting plain tuples skips
# ORM object hydration, which dominates the cost once entries number in the
# thousands.
_ENTRY_COLS = (
    TimeEntry.employee_id,
    TimeEntry.week_start,
    TimeEntry.day,
    TimeEntry.planned_hours,
    TimeEntry.actual_hours,
)


def summarise_project(
    db: Session,
    project: Project,
    *,
    resolver: RateResolver | None = None,
    entries: list | None = None,
) -> ProjectSummary:
    """Full cost breakdown for one project (employee split + weekly series).

    ``entries`` lets a caller that already fetched this project's entry rows
    (see ``summarise_all_projects``) skip the per-project query.
    """
    resolver = resolver or RateResolver(db)
    overtime_factor = resolver.overtime_factor
    summary = ProjectSummary(project=project)

    if entries is None:
        entries = db.execute(
            select(*_ENTRY_COLS).where(TimeEntry.project_id == project.id)
        ).all()
    for e in entries:
        worked = e.actual_hours or ZERO
        overtime = overtime_for_entry(
            worked, resolver.day_total(e.employee_id, e.week_start, e.day)
        )
        # Overtime hours are paid at the overtime factor; the rest at 1x.
        paid = worked + overtime * (overtime_factor - ONE)
        rate = resolver.hourly_rate(e.employee_id, e.week_start)
        cost = paid * rate

        summary.hours += worked
        summary.cost += cost

        bd = summary.by_employee.get(e.employee_id)
        if bd is None:
            emp = resolver._employee(e.employee_id)
            bd = EmployeeBreakdown(
                employee_id=e.employee_id,
                name=emp.name if emp else f"#{e.employee_id}",
            )
            summary.by_employee[e.employee_id] = bd
        bd.hours += worked
        bd.overtime += overtime
        bd.cost += cost

        wk = e.week_start
        summary.weekly_cost[wk] = summary.weekly_cost.get(wk, ZERO) + cost
        summary.weekly_planned[wk] = summary.weekly_planned.get(wk, ZERO) + (e.planned_hours or ZERO)
        summary.weekly_actual[wk] = summary.weekly_actual.get(wk, ZERO) + worked

    return summary


def summarise_all_projects(
    db: Session, projects: list[Project]
) -> list[ProjectSummary]:
    """Summaries for many projects, sharing one rate resolver.

    Fetches every project's entries in a single query (instead of one query
    per project) and buckets them in Python.
    """
    resolver = RateResolver(db)
    by_project: dict[int, list] = {p.id: [] for p in projects}
    if by_project:
        stmt = select(TimeEntry.project_id, *_ENTRY_COLS).where(
            TimeEntry.project_id.in_(by_project)
        )
        for row in db.execute(stmt):
            by_project[row.project_id].append(row)
    return [
        summarise_project(db, p, resolver=resolver, entries=by_project[p.id])
        for p in projects
    ]


def last_n_week_costs(summary: ProjectSummary, n: int = 4) -> list[Decimal]:
    """Cost incurred in each of the last ``n`` weeks ending this week.

    Drives the burn sparkline on the projects table.
    """
    this_monday = monday_of(date.today())
    weeks = [this_monday - timedelta(weeks=i) for i in range(n - 1, -1, -1)]
    return [summary.weekly_cost.get(w, ZERO) for w in weeks]


def cumulative_burn(summary: ProjectSummary) -> tuple[list[str], list[float]]:
    """Ordered (week labels, cumulative cost) for the burn-vs-fee chart."""
    labels: list[str] = []
    values: list[float] = []
    running = ZERO
    for wk in sorted(summary.weekly_cost):
        running += summary.weekly_cost[wk]
        labels.append(wk.isoformat())
        values.append(float(running))
    return labels, values


def planned_vs_actual(summary: ProjectSummary) -> tuple[list[str], list[float], list[float]]:
    """Ordered (week labels, planned hours, actual hours) for the variance chart."""
    weeks = sorted(set(summary.weekly_planned) | set(summary.weekly_actual))
    labels = [w.isoformat() for w in weeks]
    planned = [float(summary.weekly_planned.get(w, ZERO)) for w in weeks]
    actual = [float(summary.weekly_actual.get(w, ZERO)) for w in weeks]
    return labels, planned, actual
