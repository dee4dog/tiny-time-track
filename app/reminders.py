"""Timesheet reminder logic (who to email, what to send, logging the result).

Three kinds of reminder:
  * plan      - current week, employees who haven't submitted their plan
  * actuals   - current week, employees who haven't submitted actuals
  * followup  - previous week, employees who still haven't submitted actuals

Sends go only to non-submitters. Each attempt is retried once on failure, then
the outcome is written to audit_log (reminder.sent / reminder.failed) so the
manager's "This week" board can surface problems.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from datetime import date, timedelta

from sqlalchemy import select
from sqlalchemy.orm import Session

from app.audit import log as audit_log
from app.mailer import send_email
from app.models import Employee, WeekStatus
from app.settings_store import get_setting
from app.weeks import format_week_range, monday_of

KINDS = ("plan", "actuals", "followup")


@dataclass
class ReminderResult:
    kind: str
    week_start: date
    sent: list[str] = field(default_factory=list)
    failed: list[tuple[str, str]] = field(default_factory=list)  # (email, error)
    skipped_no_smtp: bool = False

    @property
    def summary(self) -> str:
        if self.skipped_no_smtp:
            return "SMTP is not configured — no reminders sent."
        parts = [f"{len(self.sent)} sent"]
        if self.failed:
            parts.append(f"{len(self.failed)} failed")
        return ", ".join(parts) + "."


def _target_week_and_field(kind: str) -> tuple[date, str]:
    """Return (week_start, week_status attribute) for the reminder kind."""
    this_monday = monday_of(date.today())
    if kind == "plan":
        return this_monday, "planned_submitted_at"
    if kind == "actuals":
        return this_monday, "actuals_submitted_at"
    if kind == "followup":
        return this_monday - timedelta(weeks=1), "actuals_submitted_at"
    raise ValueError(f"Unknown reminder kind {kind!r}")


def _has_submitted(db: Session, employee_id: int, week_start: date, attr: str) -> bool:
    st = db.get(WeekStatus, {"employee_id": employee_id, "week_start": week_start})
    return bool(st and getattr(st, attr))


def _render(template: str, *, name: str, week: str, link: str) -> str:
    return (
        template.replace("{name}", name)
        .replace("{week}", week)
        .replace("{link}", link)
    )


def _build_message(db: Session, kind: str, employee: Employee, week_start: date) -> tuple[str, str]:
    subject_tpl = get_setting(db, f"template_{kind}_subject") or "Timesheet reminder — {week}"
    body_tpl = get_setting(db, f"template_{kind}_body") or "{link}"
    base = (get_setting(db, "email_base_url") or "").rstrip("/")
    link = f"{base}/timesheet?week={week_start.isoformat()}"
    week_label = format_week_range(week_start)
    subject = _render(subject_tpl, name=employee.name, week=week_label, link=link)
    body = _render(body_tpl, name=employee.name, week=week_label, link=link)
    return subject, body


def _send_with_retry(db: Session, *, to: str, subject: str, body: str) -> tuple[bool, str]:
    ok, msg = send_email(db, to=to, subject=subject, body=body)
    if ok:
        return True, msg
    # one retry
    ok, msg = send_email(db, to=to, subject=subject, body=body)
    return ok, msg


def send_to_employee(db: Session, kind: str, employee: Employee) -> tuple[bool, str]:
    """Send one reminder of ``kind`` to ``employee`` regardless of submission.

    Used by the manager's "send reminder now" button. Logs the outcome.
    """
    week_start, _attr = _target_week_and_field(kind)
    subject, body = _build_message(db, kind, employee, week_start)
    ok, msg = _send_with_retry(db, to=employee.email, subject=subject, body=body)
    audit_log(
        db, user_id=None,
        action="reminder.sent" if ok else "reminder.failed",
        detail=f"{kind} -> {employee.email} ({week_start}): {msg}",
    )
    db.commit()
    return ok, msg


def run_reminder(db: Session, kind: str) -> ReminderResult:
    """Send ``kind`` reminders to all active non-submitters. Returns a result."""
    week_start, attr = _target_week_and_field(kind)
    result = ReminderResult(kind=kind, week_start=week_start)

    employees = list(db.scalars(select(Employee).where(Employee.active)))
    targets = [e for e in employees if not _has_submitted(db, e.id, week_start, attr)]
    if not targets:
        return result

    for employee in targets:
        subject, body = _build_message(db, kind, employee, week_start)
        ok, msg = _send_with_retry(db, to=employee.email, subject=subject, body=body)
        if ok:
            result.sent.append(employee.email)
            action = "reminder.sent"
        else:
            result.failed.append((employee.email, msg))
            action = "reminder.failed"
            if "not configured" in msg:
                result.skipped_no_smtp = True
        audit_log(db, user_id=None, action=action,
                  detail=f"{kind} -> {employee.email} ({week_start}): {msg}")

    db.commit()
    return result


def recent_failures(db: Session, *, since_days: int = 8) -> list[str]:
    """Emails with a reminder.failed entry recently (for the This-week board)."""
    from app.models import AuditLog

    cutoff = date.today() - timedelta(days=since_days)
    rows = db.scalars(
        select(AuditLog).where(AuditLog.action == "reminder.failed")
    )
    failed: set[str] = set()
    for r in rows:
        if r.timestamp.date() >= cutoff and r.detail:
            # detail format: "kind -> email (week): msg"
            try:
                email = r.detail.split("->", 1)[1].split("(", 1)[0].strip()
                failed.add(email)
            except IndexError:
                pass
    return sorted(failed)
