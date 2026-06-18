"""Manager dashboard (manager-only).

Sections:
  GET  /manager                       Projects profitability table (default)
  GET  /manager/projects/{id}         Project detail + burn/variance charts
  POST /manager/projects/{id}/fee     Edit a project fee (audited)
  GET  /manager/people                People: capacity, utilisation, compliance
  GET  /manager/rates                 Salary/rate table (extra confirmation gate)
  GET  /manager/this-week             Live submission compliance board

Every route depends on ``require_manager`` so employees can't reach any of it -
not the pages, and (because the cost math only runs here) not the money data.
"""
from __future__ import annotations

from decimal import Decimal, InvalidOperation

from urllib.parse import quote

from fastapi import APIRouter, Depends, Form, Request
from fastapi.responses import RedirectResponse, Response
from sqlalchemy import select
from sqlalchemy.orm import Session

from app import costing, exporting, reminders
from app.audit import log as audit_log
from app.database import get_db
from app.dependencies import require_manager
from app.models import Employee, Project, ProjectStatus
from app.dashboard_service import (
    compliance_recent,
    people_overview,
    this_week_board,
)
from app.templating import templates

router = APIRouter(prefix="/manager")


# --------------------------------------------------------------------------- #
#  Projects table (default)
# --------------------------------------------------------------------------- #
@router.get("")
def projects_table(
    request: Request,
    user: Employee = Depends(require_manager),
    db: Session = Depends(get_db),
):
    projects = list(
        db.scalars(
            select(Project)
            .where(Project.status != ProjectStatus.archived)
            .order_by(Project.billable.desc(), Project.name)
        )
    )
    summaries = costing.summarise_all_projects(db, projects)
    rows = [
        {"summary": s, "spark": costing.last_n_week_costs(s, 4)} for s in summaries
    ]

    totals = {
        "fee": sum((s.fee for s in summaries if s.billable), Decimal("0")),
        "cost": sum((s.cost for s in summaries), Decimal("0")),
    }
    totals["profit"] = totals["fee"] - sum(
        (s.cost for s in summaries if s.billable), Decimal("0")
    )

    return templates.TemplateResponse(
        "manager/projects.html",
        {"request": request, "user": user, "rows": rows, "totals": totals, "tab": "projects"},
    )


# --------------------------------------------------------------------------- #
#  Project detail
# --------------------------------------------------------------------------- #
@router.get("/projects/{project_id}")
def project_detail(
    project_id: int,
    request: Request,
    saved: str | None = None,
    user: Employee = Depends(require_manager),
    db: Session = Depends(get_db),
):
    project = db.get(Project, project_id)
    if project is None:
        return RedirectResponse("/manager", status_code=303)

    summary = costing.summarise_project(db, project)
    burn_labels, burn_values = costing.cumulative_burn(summary)
    pva_labels, pva_planned, pva_actual = costing.planned_vs_actual(summary)

    return templates.TemplateResponse(
        "manager/project_detail.html",
        {
            "request": request,
            "user": user,
            "tab": "projects",
            "summary": summary,
            "project": project,
            "breakdown": sorted(
                summary.by_employee.values(), key=lambda b: b.cost, reverse=True
            ),
            "burn": {"labels": burn_labels, "values": burn_values, "fee": float(summary.fee)},
            "pva": {"labels": pva_labels, "planned": pva_planned, "actual": pva_actual},
            "saved": saved,
        },
    )


@router.post("/projects/{project_id}/fee")
def update_fee(
    project_id: int,
    fee: str = Form(...),
    user: Employee = Depends(require_manager),
    db: Session = Depends(get_db),
):
    project = db.get(Project, project_id)
    if project is None:
        return RedirectResponse("/manager", status_code=303)
    try:
        new_fee = Decimal(fee.replace(" ", "").replace(",", ""))
    except (InvalidOperation, ValueError):
        return RedirectResponse(f"/manager/projects/{project_id}", status_code=303)

    old_fee = project.fee
    if new_fee != old_fee:
        project.fee = new_fee
        audit_log(
            db,
            user_id=user.id,
            action="project.fee_change",
            detail=f"Project {project.display_name!r}: fee {old_fee} -> {new_fee}",
        )
        db.commit()

    return RedirectResponse(f"/manager/projects/{project_id}?saved=fee", status_code=303)


# --------------------------------------------------------------------------- #
#  People
# --------------------------------------------------------------------------- #
@router.get("/people")
def people(
    request: Request,
    user: Employee = Depends(require_manager),
    db: Session = Depends(get_db),
):
    stats = people_overview(db)
    compliance = {s.employee.id: compliance_recent(db, s.employee, weeks=6) for s in stats}
    return templates.TemplateResponse(
        "manager/people.html",
        {"request": request, "user": user, "tab": "people", "stats": stats, "compliance": compliance},
    )


@router.get("/rates")
def rates(
    request: Request,
    confirm: int = 0,
    user: Employee = Depends(require_manager),
    db: Session = Depends(get_db),
):
    """Salary table - gated behind an explicit confirmation click."""
    if not confirm:
        return templates.TemplateResponse(
            "manager/rates_gate.html",
            {"request": request, "user": user, "tab": "people"},
        )

    resolver = costing.RateResolver(db)
    from datetime import date

    today = date.today()
    employees = list(
        db.scalars(select(Employee).where(Employee.active).order_by(Employee.name))
    )
    rows = []
    for e in employees:
        rows.append(
            {
                "employee": e,
                "salary": e.annual_salary,
                "available": e.available_hours_per_year,
                "hourly": resolver.hourly_rate(e.id, today),
            }
        )
    return templates.TemplateResponse(
        "manager/rates.html",
        {"request": request, "user": user, "tab": "people", "rows": rows,
         "overhead": resolver.overhead},
    )


# --------------------------------------------------------------------------- #
#  This week
# --------------------------------------------------------------------------- #
@router.get("/this-week")
def this_week(
    request: Request,
    msg: str = "", err: str = "",
    user: Employee = Depends(require_manager),
    db: Session = Depends(get_db),
):
    week_start, rows = this_week_board(db)
    failures = set(reminders.recent_failures(db))
    return templates.TemplateResponse(
        "manager/this_week.html",
        {"request": request, "user": user, "tab": "this_week",
         "week_start": week_start, "rows": rows, "failures": failures,
         "msg": msg, "err": err},
    )


_XLSX_MEDIA = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"


@router.get("/export/projects.xlsx")
def export_projects(
    user: Employee = Depends(require_manager), db: Session = Depends(get_db)
):
    data = exporting.export_projects_xlsx(db)
    return Response(
        content=data, media_type=_XLSX_MEDIA,
        headers={"Content-Disposition": 'attachment; filename="projects.xlsx"'},
    )


@router.get("/export/people.xlsx")
def export_people(
    user: Employee = Depends(require_manager), db: Session = Depends(get_db)
):
    data = exporting.export_people_xlsx(db)
    return Response(
        content=data, media_type=_XLSX_MEDIA,
        headers={"Content-Disposition": 'attachment; filename="people.xlsx"'},
    )


@router.post("/this-week/remind")
def send_reminder_now(
    employee_id: int = Form(...),
    kind: str = Form("actuals"),
    user: Employee = Depends(require_manager),
    db: Session = Depends(get_db),
):
    """Manager's one-click 'send reminder now' for a single person."""
    employee = db.get(Employee, employee_id)
    if employee is None or kind not in reminders.KINDS:
        return RedirectResponse("/manager/this-week", status_code=303)
    ok, message = reminders.send_to_employee(db, kind, employee)
    flag = "msg" if ok else "err"
    label = "Reminder sent to" if ok else "Reminder failed for"
    return RedirectResponse(
        f"/manager/this-week?{flag}={quote(f'{label} {employee.name}: {message}')}",
        status_code=303,
    )
