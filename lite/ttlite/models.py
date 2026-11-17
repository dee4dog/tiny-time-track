"""Database tables.

Time is stored as whole **minutes** (in quarter-hour steps), so every total
is exact integer arithmetic - no floating-point drift in the records.
"""
from __future__ import annotations

from datetime import date, datetime, timedelta

from sqlalchemy import (
    Boolean,
    CheckConstraint,
    Date,
    DateTime,
    ForeignKey,
    Integer,
    String,
    Text,
    UniqueConstraint,
)
from sqlalchemy.orm import Mapped, mapped_column, relationship

from ttlite.db import Base

DAY_MINUTES = 24 * 60


def now_local() -> datetime:
    """Local wall-clock time (single-computer app, so local is what matters)."""
    return datetime.now().replace(microsecond=0)


class Project(Base):
    __tablename__ = "projects"

    id: Mapped[int] = mapped_column(primary_key=True)
    name: Mapped[str] = mapped_column(String(160), nullable=False)
    code: Mapped[str] = mapped_column(String(40), nullable=False, default="")
    client: Mapped[str] = mapped_column(String(160), nullable=False, default="")
    active: Mapped[bool] = mapped_column(Boolean, nullable=False, default=True)
    created_at: Mapped[datetime] = mapped_column(DateTime, nullable=False, default=now_local)


class Entry(Base):
    """Planned and actual minutes for one project on one calendar day."""

    __tablename__ = "entries"

    id: Mapped[int] = mapped_column(primary_key=True)
    project_id: Mapped[int] = mapped_column(ForeignKey("projects.id"), nullable=False)
    day: Mapped[date] = mapped_column(Date, nullable=False, index=True)
    planned: Mapped[int] = mapped_column(Integer, nullable=False, default=0)
    actual: Mapped[int] = mapped_column(Integer, nullable=False, default=0)

    __table_args__ = (
        UniqueConstraint("project_id", "day", name="uq_entry_project_day"),
        CheckConstraint(f"planned BETWEEN 0 AND {DAY_MINUTES}", name="ck_entry_planned"),
        CheckConstraint(f"actual BETWEEN 0 AND {DAY_MINUTES}", name="ck_entry_actual"),
    )


class WeekNote(Base):
    """Free-text notes/goals for a week (printed on the weekly plan)."""

    __tablename__ = "week_notes"

    week_start: Mapped[date] = mapped_column(Date, primary_key=True)
    text: Mapped[str] = mapped_column(Text, nullable=False, default="")


class Timesheet(Base):
    """A saved week: the permanent record of actual time spent.

    Saving locks the week. The lines are a snapshot, so the record keeps the
    project names exactly as they were, even if a project is renamed later.
    """

    __tablename__ = "timesheets"

    id: Mapped[int] = mapped_column(primary_key=True)
    week_start: Mapped[date] = mapped_column(Date, nullable=False, unique=True)
    saved_at: Mapped[datetime] = mapped_column(DateTime, nullable=False, default=now_local)
    total: Mapped[int] = mapped_column(Integer, nullable=False)  # minutes
    note: Mapped[str] = mapped_column(Text, nullable=False, default="")
    lines: Mapped[list[TimesheetLine]] = relationship(
        back_populates="timesheet", cascade="all, delete-orphan", order_by="TimesheetLine.id"
    )

    @property
    def week_end(self) -> date:
        return self.week_start + timedelta(days=6)


class TimesheetLine(Base):
    """One project's actual minutes (Mon..Sun) inside a saved timesheet."""

    __tablename__ = "timesheet_lines"

    id: Mapped[int] = mapped_column(primary_key=True)
    timesheet_id: Mapped[int] = mapped_column(
        ForeignKey("timesheets.id", ondelete="CASCADE"), nullable=False, index=True
    )
    project_id: Mapped[int | None] = mapped_column(
        ForeignKey("projects.id", ondelete="SET NULL"), nullable=True, index=True
    )
    project_name: Mapped[str] = mapped_column(String(160), nullable=False)
    project_code: Mapped[str] = mapped_column(String(40), nullable=False, default="")
    m0: Mapped[int] = mapped_column(Integer, nullable=False, default=0)
    m1: Mapped[int] = mapped_column(Integer, nullable=False, default=0)
    m2: Mapped[int] = mapped_column(Integer, nullable=False, default=0)
    m3: Mapped[int] = mapped_column(Integer, nullable=False, default=0)
    m4: Mapped[int] = mapped_column(Integer, nullable=False, default=0)
    m5: Mapped[int] = mapped_column(Integer, nullable=False, default=0)
    m6: Mapped[int] = mapped_column(Integer, nullable=False, default=0)
    total: Mapped[int] = mapped_column(Integer, nullable=False)

    timesheet: Mapped[Timesheet] = relationship(back_populates="lines")

    @property
    def days(self) -> list[int]:
        return [self.m0, self.m1, self.m2, self.m3, self.m4, self.m5, self.m6]


class TimesheetRevision(Base):
    """A saved timesheet that was later reopened - kept so no record is ever lost.

    ``lines`` is a JSON list of {"code", "name", "days": [m0..m6], "total"}.
    """

    __tablename__ = "timesheet_revisions"

    id: Mapped[int] = mapped_column(primary_key=True)
    week_start: Mapped[date] = mapped_column(Date, nullable=False, index=True)
    saved_at: Mapped[datetime] = mapped_column(DateTime, nullable=False)
    reopened_at: Mapped[datetime] = mapped_column(DateTime, nullable=False, default=now_local)
    total: Mapped[int] = mapped_column(Integer, nullable=False)
    note: Mapped[str] = mapped_column(Text, nullable=False, default="")
    lines: Mapped[str] = mapped_column(Text, nullable=False, default="[]")


class Setting(Base):
    """Key-value preferences (your name, daily target, weekend columns)."""

    __tablename__ = "settings"

    key: Mapped[str] = mapped_column(String(60), primary_key=True)
    value: Mapped[str] = mapped_column(Text, nullable=False)
