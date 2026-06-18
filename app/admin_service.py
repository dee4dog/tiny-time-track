"""Settings-screen operations: create/update employees and projects.

Centralises the rules that must hold no matter how they're triggered:

* Changing a salary writes a ``rate_history`` row (effective today) so past
  weeks stay costed at the old salary, and records an audit entry.
* Deactivating (not deleting) preserves history.
* Sensitive changes (salary, fee, role, settings) are written to audit_log.
"""
from __future__ import annotations

from datetime import date
from decimal import Decimal, InvalidOperation

from sqlalchemy import select
from sqlalchemy.orm import Session

from app.audit import log as audit_log
from app.models import (
    Employee,
    EmployeeProject,
    Project,
    ProjectStatus,
    RateHistory,
    Role,
)
from app.security import hash_password
from app.settings_store import set_setting


class AdminError(Exception):
    """Raised for invalid admin input (caller turns it into a UI message)."""


def parse_decimal(raw: str, *, field: str) -> Decimal:
    try:
        return Decimal(str(raw).replace(" ", "").replace(",", ""))
    except (InvalidOperation, ValueError):
        raise AdminError(f"{field} must be a number.")


# --------------------------------------------------------------------------- #
#  Employees
# --------------------------------------------------------------------------- #
def create_employee(
    db: Session,
    *,
    actor_id: int,
    name: str,
    email: str,
    role: Role,
    salary: Decimal,
    available_hours: int,
    password: str,
) -> Employee:
    email = email.strip().lower()
    if not name.strip() or not email:
        raise AdminError("Name and email are required.")
    if len(password) < 8:
        raise AdminError("Password must be at least 8 characters.")
    if db.scalar(select(Employee).where(Employee.email == email)):
        raise AdminError(f"An account with email {email!r} already exists.")

    emp = Employee(
        name=name.strip(),
        email=email,
        role=role,
        annual_salary=salary,
        available_hours_per_year=available_hours,
        password_hash=hash_password(password),
        active=True,
    )
    db.add(emp)
    db.flush()
    db.add(RateHistory(employee_id=emp.id, effective_date=date.today(), annual_salary=salary))
    audit_log(db, user_id=actor_id, action="employee.create",
              detail=f"{emp.name} <{emp.email}> role={role.value} salary={salary}")
    db.commit()
    return emp


def update_employee(
    db: Session,
    *,
    actor_id: int,
    employee: Employee,
    name: str,
    email: str,
    role: Role,
    salary: Decimal,
    available_hours: int,
    active: bool,
) -> None:
    email = email.strip().lower()
    if not name.strip() or not email:
        raise AdminError("Name and email are required.")
    clash = db.scalar(
        select(Employee).where(Employee.email == email, Employee.id != employee.id)
    )
    if clash:
        raise AdminError(f"Another account already uses {email!r}.")

    changes: list[str] = []
    if employee.name != name.strip():
        changes.append(f"name {employee.name!r}->{name.strip()!r}")
        employee.name = name.strip()
    if employee.email != email:
        changes.append(f"email {employee.email!r}->{email!r}")
        employee.email = email
    if employee.role != role:
        changes.append(f"role {employee.role.value}->{role.value}")
        employee.role = role
    if employee.available_hours_per_year != available_hours:
        changes.append(f"available_hours {employee.available_hours_per_year}->{available_hours}")
        employee.available_hours_per_year = available_hours
    if employee.active != active:
        changes.append("activated" if active else "deactivated")
        employee.active = active

    # Salary change -> new rate_history row (effective today) + dedicated audit.
    if salary != employee.annual_salary:
        old = employee.annual_salary
        employee.annual_salary = salary
        today = date.today()
        existing = db.scalar(
            select(RateHistory).where(
                RateHistory.employee_id == employee.id,
                RateHistory.effective_date == today,
            )
        )
        if existing:
            existing.annual_salary = salary
        else:
            db.add(RateHistory(employee_id=employee.id, effective_date=today, annual_salary=salary))
        audit_log(db, user_id=actor_id, action="employee.salary_change",
                  detail=f"{employee.name}: salary {old} -> {salary} (effective {today})")

    if changes:
        audit_log(db, user_id=actor_id, action="employee.update",
                  detail=f"{employee.name}: " + ", ".join(changes))
    db.commit()


def set_password(db: Session, *, actor_id: int, employee: Employee, password: str) -> None:
    if len(password) < 8:
        raise AdminError("Password must be at least 8 characters.")
    employee.password_hash = hash_password(password)
    audit_log(db, user_id=actor_id, action="employee.password_reset",
              detail=f"Password reset for {employee.email}")
    db.commit()


def set_assigned_projects(db: Session, *, employee: Employee, project_ids: list[int]) -> None:
    """Replace the employee's grid assignment set."""
    wanted = set(project_ids)
    current = {
        ep.project_id
        for ep in db.scalars(
            select(EmployeeProject).where(EmployeeProject.employee_id == employee.id)
        )
    }
    for pid in wanted - current:
        db.add(EmployeeProject(employee_id=employee.id, project_id=pid))
    for pid in current - wanted:
        link = db.get(EmployeeProject, {"employee_id": employee.id, "project_id": pid})
        if link:
            db.delete(link)
    db.commit()


# --------------------------------------------------------------------------- #
#  Projects
# --------------------------------------------------------------------------- #
def create_project(
    db: Session,
    *,
    actor_id: int,
    leader: str,
    name: str,
    number: str,
    fee: Decimal,
    billable: bool,
) -> Project:
    if not name.strip():
        raise AdminError("Project name is required.")
    project = Project(
        leader=leader.strip(),
        name=name.strip(),
        number=number.strip(),
        fee=fee,
        billable=billable,
        status=ProjectStatus.active,
    )
    db.add(project)
    db.flush()
    audit_log(db, user_id=actor_id, action="project.create",
              detail=f"{project.display_name} fee={fee} billable={billable}")
    db.commit()
    return project


def update_project(
    db: Session,
    *,
    actor_id: int,
    project: Project,
    leader: str,
    name: str,
    number: str,
    fee: Decimal,
    billable: bool,
    status: ProjectStatus,
) -> None:
    changes: list[str] = []
    if project.leader != leader.strip():
        changes.append("leader"); project.leader = leader.strip()
    if project.name != name.strip():
        changes.append("name"); project.name = name.strip()
    if project.number != number.strip():
        changes.append("number"); project.number = number.strip()
    if project.billable != billable:
        changes.append(f"billable->{billable}"); project.billable = billable
    if project.status != status:
        changes.append(f"status {project.status.value}->{status.value}")
        project.status = status
    if fee != project.fee:
        audit_log(db, user_id=actor_id, action="project.fee_change",
                  detail=f"{project.display_name}: fee {project.fee} -> {fee}")
        project.fee = fee

    if changes:
        audit_log(db, user_id=actor_id, action="project.update",
                  detail=f"{project.display_name}: " + ", ".join(changes))
    db.commit()


# --------------------------------------------------------------------------- #
#  Globals
# --------------------------------------------------------------------------- #
GLOBAL_KEYS = [
    "overhead_multiplier",
    "overtime_factor",
    "available_hours_default",
    "currency_symbol",
    "company_name",
]


def update_globals(db: Session, *, actor_id: int, values: dict[str, str]) -> None:
    changed = []
    for key in GLOBAL_KEYS:
        if key in values:
            set_setting(db, key, values[key].strip())
            changed.append(key)
    if changed:
        audit_log(db, user_id=actor_id, action="settings.update",
                  detail="globals: " + ", ".join(changed))
    db.commit()


SMTP_KEYS = ["smtp_host", "smtp_port", "smtp_use_tls", "smtp_username", "smtp_from", "smtp_password"]


REMINDER_KEYS = [
    "reminder_enabled", "email_base_url",
    "reminder_plan_dow", "reminder_plan_time",
    "reminder_actuals_dow", "reminder_actuals_time",
    "reminder_followup_dow", "reminder_followup_time",
    "template_plan_subject", "template_plan_body",
    "template_actuals_subject", "template_actuals_body",
    "template_followup_subject", "template_followup_body",
]


def update_reminders(db: Session, *, actor_id: int, values: dict[str, str]) -> None:
    changed = []
    for key in REMINDER_KEYS:
        if key in values:
            set_setting(db, key, values[key])
            changed.append(key)
    if changed:
        audit_log(db, user_id=actor_id, action="settings.update",
                  detail="reminders: " + ", ".join(changed))
    db.commit()


def update_smtp(db: Session, *, actor_id: int, values: dict[str, str]) -> None:
    changed = []
    for key in SMTP_KEYS:
        if key in values:
            # Don't overwrite a stored password with a blank submit.
            if key == "smtp_password" and values[key] == "":
                continue
            set_setting(db, key, values[key].strip())
            changed.append(key)
    if changed:
        # Never log the password value itself.
        logged = [k for k in changed if k != "smtp_password"]
        if "smtp_password" in changed:
            logged.append("smtp_password(updated)")
        audit_log(db, user_id=actor_id, action="settings.update",
                  detail="smtp: " + ", ".join(logged))
    db.commit()
