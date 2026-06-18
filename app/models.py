"""Database schema (SQLAlchemy 2.0 ORM models).

This mirrors section 4 of the project brief. A few conventions:

* Money is stored as ``Numeric`` (exact decimal) and always in ZAR.
* Hours are ``Numeric(4, 1)`` so 0.5-hour steps are exact.
* We never hard-delete people or projects - they get deactivated/archived
  so historical numbers stay intact.
* Salaries are NOT stored on the employee row alone; every change is also
  written to ``rate_history`` so past weeks are costed at the salary that
  was in effect at the time.
"""
from __future__ import annotations

import enum
from datetime import date, datetime, timezone
from decimal import Decimal

from sqlalchemy import (
    Boolean,
    CheckConstraint,
    Date,
    DateTime,
    Enum,
    ForeignKey,
    Integer,
    Numeric,
    String,
    Text,
    UniqueConstraint,
)
from sqlalchemy.orm import Mapped, mapped_column, relationship

from app.database import Base


def _utcnow() -> datetime:
    """Timezone-aware UTC timestamp (stored for audit/created fields)."""
    return datetime.now(timezone.utc)


class Role(str, enum.Enum):
    employee = "employee"
    manager = "manager"


class ProjectStatus(str, enum.Enum):
    active = "active"
    on_hold = "on_hold"
    complete = "complete"
    archived = "archived"


class Employee(Base):
    __tablename__ = "employees"

    id: Mapped[int] = mapped_column(primary_key=True)
    name: Mapped[str] = mapped_column(String(120), nullable=False)
    email: Mapped[str] = mapped_column(String(255), unique=True, nullable=False)
    role: Mapped[Role] = mapped_column(
        Enum(Role, native_enum=False), default=Role.employee, nullable=False
    )

    # SENSITIVE - manager-visible only. Never expose via an employee-role API.
    annual_salary: Mapped[Decimal] = mapped_column(
        Numeric(12, 2), default=Decimal("0"), nullable=False
    )
    available_hours_per_year: Mapped[int] = mapped_column(
        Integer, default=1760, nullable=False
    )

    active: Mapped[bool] = mapped_column(Boolean, default=True, nullable=False)
    password_hash: Mapped[str] = mapped_column(String(255), nullable=False)
    created_at: Mapped[datetime] = mapped_column(DateTime, default=_utcnow)

    # Relationships
    rate_history: Mapped[list["RateHistory"]] = relationship(
        back_populates="employee",
        cascade="all, delete-orphan",
        order_by="RateHistory.effective_date",
    )
    assigned_projects: Mapped[list["Project"]] = relationship(
        secondary="employee_projects", back_populates="assigned_employees"
    )

    def __repr__(self) -> str:  # pragma: no cover - debugging aid
        return f"<Employee {self.id} {self.name} ({self.role.value})>"


class RateHistory(Base):
    """Salary as it changed over time. The salary effective in a given week
    is the most recent row whose ``effective_date`` is <= that week."""

    __tablename__ = "rate_history"

    id: Mapped[int] = mapped_column(primary_key=True)
    employee_id: Mapped[int] = mapped_column(
        ForeignKey("employees.id"), nullable=False, index=True
    )
    effective_date: Mapped[date] = mapped_column(Date, nullable=False)
    annual_salary: Mapped[Decimal] = mapped_column(Numeric(12, 2), nullable=False)

    employee: Mapped["Employee"] = relationship(back_populates="rate_history")

    __table_args__ = (
        UniqueConstraint("employee_id", "effective_date", name="uq_rate_emp_date"),
    )


class Project(Base):
    __tablename__ = "projects"

    id: Mapped[int] = mapped_column(primary_key=True)
    leader: Mapped[str] = mapped_column(String(60), default="", nullable=False)
    name: Mapped[str] = mapped_column(String(200), nullable=False)
    # Text, not int - project numbers can carry prefixes like "C-1000".
    number: Mapped[str] = mapped_column(String(60), default="", nullable=False)
    fee: Mapped[Decimal] = mapped_column(
        Numeric(14, 2), default=Decimal("0"), nullable=False
    )
    status: Mapped[ProjectStatus] = mapped_column(
        Enum(ProjectStatus, native_enum=False),
        default=ProjectStatus.active,
        nullable=False,
    )
    # False for admin/library rows (e.g. "Revit Library", "Vergaderings").
    billable: Mapped[bool] = mapped_column(Boolean, default=True, nullable=False)
    created_at: Mapped[datetime] = mapped_column(DateTime, default=_utcnow)

    assigned_employees: Mapped[list["Employee"]] = relationship(
        secondary="employee_projects", back_populates="assigned_projects"
    )

    @property
    def display_name(self) -> str:
        """Standard label used everywhere: ``Leader | Project | No.``"""
        parts = [p for p in (self.leader, self.name, self.number) if p]
        return " | ".join(parts)

    def __repr__(self) -> str:  # pragma: no cover
        return f"<Project {self.id} {self.display_name!r}>"


class EmployeeProject(Base):
    """Join table: which projects appear on each employee's weekly grid."""

    __tablename__ = "employee_projects"

    employee_id: Mapped[int] = mapped_column(
        ForeignKey("employees.id"), primary_key=True
    )
    project_id: Mapped[int] = mapped_column(
        ForeignKey("projects.id"), primary_key=True
    )


class TimeEntry(Base):
    __tablename__ = "time_entries"

    id: Mapped[int] = mapped_column(primary_key=True)
    employee_id: Mapped[int] = mapped_column(
        ForeignKey("employees.id"), nullable=False, index=True
    )
    project_id: Mapped[int] = mapped_column(
        ForeignKey("projects.id"), nullable=False, index=True
    )
    # Always a Monday. Combined with `day` to get the calendar date.
    week_start: Mapped[date] = mapped_column(Date, nullable=False, index=True)
    day: Mapped[int] = mapped_column(Integer, nullable=False)  # 0=Mon .. 4=Fri

    planned_hours: Mapped[Decimal] = mapped_column(
        Numeric(4, 1), default=Decimal("0"), nullable=False
    )
    actual_hours: Mapped[Decimal] = mapped_column(
        Numeric(4, 1), default=Decimal("0"), nullable=False
    )
    overtime_hours: Mapped[Decimal] = mapped_column(
        Numeric(4, 1), default=Decimal("0"), nullable=False
    )
    note: Mapped[str | None] = mapped_column(Text, nullable=True)

    __table_args__ = (
        UniqueConstraint(
            "employee_id", "project_id", "week_start", "day", name="uq_entry"
        ),
        CheckConstraint("day >= 0 AND day <= 4", name="ck_day_range"),
    )


class WeekStatus(Base):
    """Tracks submission of a week's plan and actuals (drives reminders)."""

    __tablename__ = "week_status"

    employee_id: Mapped[int] = mapped_column(
        ForeignKey("employees.id"), primary_key=True
    )
    week_start: Mapped[date] = mapped_column(Date, primary_key=True)
    planned_submitted_at: Mapped[datetime | None] = mapped_column(
        DateTime, nullable=True
    )
    actuals_submitted_at: Mapped[datetime | None] = mapped_column(
        DateTime, nullable=True
    )


class Setting(Base):
    """Key-value store for manager-editable business settings."""

    __tablename__ = "settings"

    key: Mapped[str] = mapped_column(String(80), primary_key=True)
    value: Mapped[str] = mapped_column(Text, nullable=False)


class AuditLog(Base):
    """Append-only record of sensitive changes (salaries, fees, settings)."""

    __tablename__ = "audit_log"

    id: Mapped[int] = mapped_column(primary_key=True)
    timestamp: Mapped[datetime] = mapped_column(DateTime, default=_utcnow, index=True)
    user_id: Mapped[int | None] = mapped_column(
        ForeignKey("employees.id"), nullable=True
    )
    action: Mapped[str] = mapped_column(String(120), nullable=False)
    detail: Mapped[str | None] = mapped_column(Text, nullable=True)
