"""Seed demo data for trying the app out.

Creates the five known projects from the brief plus two sample accounts
(one manager, one employee) so you can log in immediately. Idempotent:
running it again will not duplicate rows.

Demo logins (CHANGE or remove before real use):
    manager:  manager@tiny.local  / changeme123
    employee: anel@tiny.local     / changeme123
"""
from __future__ import annotations

from datetime import date
from decimal import Decimal

from sqlalchemy.orm import Session

from app.models import Employee, EmployeeProject, Project, ProjectStatus, RateHistory, Role
from app.security import hash_password

DEMO_PASSWORD = "changeme123"

# (leader, name, number, fee, billable). Leader is blank for the non-billable
# admin/library rows so they render as "Revit Library | C-1000" etc.
DEMO_PROJECTS = [
    ("ES", "Longlands Clubhouse", "3676", Decimal("1850000"), True),
    ("ES", "Chekkers", "3785", Decimal("920000"), True),
    ("ES", "Van Rijn Meent", "3671", Decimal("1340000"), True),
    ("", "Revit Library", "C-1000", Decimal("0"), False),
    ("", "General / Admin", "C-0001", Decimal("0"), False),
]


def _get_or_create_employee(
    db: Session, *, name: str, email: str, role: Role, salary: Decimal
) -> Employee:
    user = db.query(Employee).filter(Employee.email == email).one_or_none()
    if user is not None:
        return user
    user = Employee(
        name=name,
        email=email,
        role=role,
        annual_salary=salary,
        password_hash=hash_password(DEMO_PASSWORD),
        active=True,
    )
    db.add(user)
    db.flush()
    db.add(
        RateHistory(employee_id=user.id, effective_date=date.today(), annual_salary=salary)
    )
    return user


def seed_demo_data(db: Session) -> None:
    # Projects
    projects: list[Project] = []
    for leader, name, number, fee, billable in DEMO_PROJECTS:
        existing = db.query(Project).filter(Project.number == number).one_or_none()
        if existing is None:
            existing = Project(
                leader=leader,
                name=name,
                number=number,
                fee=fee,
                billable=billable,
                status=ProjectStatus.active,
            )
            db.add(existing)
            db.flush()
        projects.append(existing)

    # Sample accounts
    manager = _get_or_create_employee(
        db, name="Manager", email="manager@tiny.local", role=Role.manager,
        salary=Decimal("780000"),
    )
    anel = _get_or_create_employee(
        db, name="Anel", email="anel@tiny.local", role=Role.employee,
        salary=Decimal("480000"),
    )

    # Put every project on both sample staff members' grids.
    for user in (manager, anel):
        for project in projects:
            link = (
                db.query(EmployeeProject)
                .filter_by(employee_id=user.id, project_id=project.id)
                .one_or_none()
            )
            if link is None:
                db.add(EmployeeProject(employee_id=user.id, project_id=project.id))

    db.commit()
