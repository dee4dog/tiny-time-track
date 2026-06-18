"""Settings interface (manager only).

Sections: Employees, Projects, Globals, Email (SMTP + test), Audit log.
Flash messages are passed back via ``?msg=`` / ``?err=`` query parameters.
"""
from __future__ import annotations

from urllib.parse import quote

from fastapi import APIRouter, Depends, Form, Request
from fastapi.responses import RedirectResponse
from sqlalchemy import select
from sqlalchemy.orm import Session

from app import admin_service as adm
from app.database import get_db
from app.dependencies import require_manager
from app.mailer import send_test_email
from app.models import AuditLog, Employee, Project, ProjectStatus, Role
from app.settings_store import get_setting
from app.templating import templates

router = APIRouter(prefix="/settings")


def _redirect(path: str, *, msg: str = "", err: str = "") -> RedirectResponse:
    sep = "?"
    if msg:
        path += f"{sep}msg={quote(msg)}"; sep = "&"
    if err:
        path += f"{sep}err={quote(err)}"
    return RedirectResponse(path, status_code=303)


def _checkbox(value: str | None) -> bool:
    return value is not None


# --------------------------------------------------------------------------- #
#  Landing
# --------------------------------------------------------------------------- #
@router.get("")
def settings_home(user: Employee = Depends(require_manager)):
    return RedirectResponse("/settings/employees", status_code=303)


# --------------------------------------------------------------------------- #
#  Employees
# --------------------------------------------------------------------------- #
@router.get("/employees")
def employees_list(
    request: Request, msg: str = "", err: str = "",
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    employees = list(db.scalars(select(Employee).order_by(Employee.active.desc(), Employee.name)))
    return templates.TemplateResponse(
        "settings/employees.html",
        {"request": request, "user": user, "tab": "employees", "employees": employees,
         "default_hours": get_setting(db, "available_hours_default"), "msg": msg, "err": err},
    )


@router.post("/employees")
def employee_create(
    name: str = Form(...), email: str = Form(...), role: str = Form("employee"),
    salary: str = Form("0"), available_hours: int = Form(1760), password: str = Form(...),
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    try:
        adm.create_employee(
            db, actor_id=user.id, name=name, email=email,
            role=Role(role), salary=adm.parse_decimal(salary, field="Salary"),
            available_hours=available_hours, password=password,
        )
    except adm.AdminError as e:
        return _redirect("/settings/employees", err=str(e))
    return _redirect("/settings/employees", msg=f"Added {name}.")


@router.get("/employees/{employee_id}")
def employee_edit(
    employee_id: int, request: Request, msg: str = "", err: str = "",
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    employee = db.get(Employee, employee_id)
    if employee is None:
        return _redirect("/settings/employees", err="No such employee.")
    projects = list(db.scalars(select(Project).order_by(Project.name)))
    assigned = {p.id for p in employee.assigned_projects}
    return templates.TemplateResponse(
        "settings/employee_edit.html",
        {"request": request, "user": user, "tab": "employees", "employee": employee,
         "projects": projects, "assigned": assigned, "msg": msg, "err": err},
    )


@router.post("/employees/{employee_id}")
def employee_update(
    employee_id: int,
    name: str = Form(...), email: str = Form(...), role: str = Form("employee"),
    salary: str = Form("0"), available_hours: int = Form(1760),
    active: str | None = Form(None),
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    employee = db.get(Employee, employee_id)
    if employee is None:
        return _redirect("/settings/employees", err="No such employee.")
    try:
        adm.update_employee(
            db, actor_id=user.id, employee=employee, name=name, email=email,
            role=Role(role), salary=adm.parse_decimal(salary, field="Salary"),
            available_hours=available_hours, active=_checkbox(active),
        )
    except adm.AdminError as e:
        return _redirect(f"/settings/employees/{employee_id}", err=str(e))
    return _redirect(f"/settings/employees/{employee_id}", msg="Saved.")


@router.post("/employees/{employee_id}/password")
def employee_password(
    employee_id: int, password: str = Form(...),
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    employee = db.get(Employee, employee_id)
    if employee is None:
        return _redirect("/settings/employees", err="No such employee.")
    try:
        adm.set_password(db, actor_id=user.id, employee=employee, password=password)
    except adm.AdminError as e:
        return _redirect(f"/settings/employees/{employee_id}", err=str(e))
    return _redirect(f"/settings/employees/{employee_id}", msg="Password reset.")


@router.post("/employees/{employee_id}/projects")
def employee_projects(
    employee_id: int, project_ids: list[int] = Form(default=[]),
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    employee = db.get(Employee, employee_id)
    if employee is None:
        return _redirect("/settings/employees", err="No such employee.")
    adm.set_assigned_projects(db, employee=employee, project_ids=project_ids)
    return _redirect(f"/settings/employees/{employee_id}", msg="Project assignment updated.")


# --------------------------------------------------------------------------- #
#  Projects
# --------------------------------------------------------------------------- #
@router.get("/projects")
def projects_list(
    request: Request, msg: str = "", err: str = "",
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    projects = list(db.scalars(select(Project).order_by(Project.status, Project.name)))
    return templates.TemplateResponse(
        "settings/projects.html",
        {"request": request, "user": user, "tab": "projects", "projects": projects,
         "msg": msg, "err": err},
    )


@router.post("/projects")
def project_create(
    leader: str = Form(""), name: str = Form(...), number: str = Form(""),
    fee: str = Form("0"), billable: str | None = Form(None),
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    try:
        adm.create_project(
            db, actor_id=user.id, leader=leader, name=name, number=number,
            fee=adm.parse_decimal(fee, field="Fee"), billable=_checkbox(billable),
        )
    except adm.AdminError as e:
        return _redirect("/settings/projects", err=str(e))
    return _redirect("/settings/projects", msg=f"Added {name}.")


@router.get("/projects/{project_id}")
def project_edit(
    project_id: int, request: Request, msg: str = "", err: str = "",
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    project = db.get(Project, project_id)
    if project is None:
        return _redirect("/settings/projects", err="No such project.")
    return templates.TemplateResponse(
        "settings/project_edit.html",
        {"request": request, "user": user, "tab": "projects", "project": project,
         "statuses": list(ProjectStatus), "msg": msg, "err": err},
    )


@router.post("/projects/{project_id}")
def project_update(
    project_id: int,
    leader: str = Form(""), name: str = Form(...), number: str = Form(""),
    fee: str = Form("0"), billable: str | None = Form(None), status: str = Form("active"),
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    project = db.get(Project, project_id)
    if project is None:
        return _redirect("/settings/projects", err="No such project.")
    try:
        adm.update_project(
            db, actor_id=user.id, project=project, leader=leader, name=name,
            number=number, fee=adm.parse_decimal(fee, field="Fee"),
            billable=_checkbox(billable), status=ProjectStatus(status),
        )
    except adm.AdminError as e:
        return _redirect(f"/settings/projects/{project_id}", err=str(e))
    return _redirect(f"/settings/projects/{project_id}", msg="Saved.")


# --------------------------------------------------------------------------- #
#  Globals
# --------------------------------------------------------------------------- #
@router.get("/globals")
def globals_view(
    request: Request, msg: str = "", err: str = "",
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    values = {k: get_setting(db, k) for k in adm.GLOBAL_KEYS}
    return templates.TemplateResponse(
        "settings/globals.html",
        {"request": request, "user": user, "tab": "globals", "values": values,
         "msg": msg, "err": err},
    )


@router.post("/globals")
def globals_save(
    request: Request,
    overhead_multiplier: str = Form(...), overtime_factor: str = Form(...),
    available_hours_default: str = Form(...), currency_symbol: str = Form(...),
    company_name: str = Form(...),
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    adm.update_globals(db, actor_id=user.id, values={
        "overhead_multiplier": overhead_multiplier,
        "overtime_factor": overtime_factor,
        "available_hours_default": available_hours_default,
        "currency_symbol": currency_symbol,
        "company_name": company_name,
    })
    return _redirect("/settings/globals", msg="Globals saved.")


# --------------------------------------------------------------------------- #
#  Email (SMTP)
# --------------------------------------------------------------------------- #
@router.get("/email")
def email_view(
    request: Request, msg: str = "", err: str = "",
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    values = {k: get_setting(db, k) for k in
              ["smtp_host", "smtp_port", "smtp_use_tls", "smtp_username", "smtp_from"]}
    has_password = bool(get_setting(db, "smtp_password"))
    return templates.TemplateResponse(
        "settings/email.html",
        {"request": request, "user": user, "tab": "email", "values": values,
         "has_password": has_password, "msg": msg, "err": err},
    )


@router.post("/email")
def email_save(
    smtp_host: str = Form(""), smtp_port: str = Form("587"),
    smtp_use_tls: str | None = Form(None), smtp_username: str = Form(""),
    smtp_from: str = Form(""), smtp_password: str = Form(""),
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    adm.update_smtp(db, actor_id=user.id, values={
        "smtp_host": smtp_host, "smtp_port": smtp_port,
        "smtp_use_tls": "true" if _checkbox(smtp_use_tls) else "false",
        "smtp_username": smtp_username, "smtp_from": smtp_from,
        "smtp_password": smtp_password,
    })
    return _redirect("/settings/email", msg="Email settings saved.")


@router.post("/email/test")
def email_test(
    to: str = Form(...),
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    ok, message = send_test_email(db, to.strip())
    if ok:
        return _redirect("/settings/email", msg=message)
    return _redirect("/settings/email", err=message)


# --------------------------------------------------------------------------- #
#  Reminders
# --------------------------------------------------------------------------- #
DOW_NAMES = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"]


@router.get("/reminders")
def reminders_view(
    request: Request, msg: str = "", err: str = "",
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    values = {k: get_setting(db, k) for k in adm.REMINDER_KEYS}
    return templates.TemplateResponse(
        "settings/reminders.html",
        {"request": request, "user": user, "tab": "reminders", "values": values,
         "dow_names": list(enumerate(DOW_NAMES)), "msg": msg, "err": err},
    )


@router.post("/reminders")
def reminders_save(
    request: Request,
    reminder_enabled: str | None = Form(None),
    email_base_url: str = Form(...),
    reminder_plan_dow: str = Form("0"), reminder_plan_time: str = Form("08:00"),
    reminder_actuals_dow: str = Form("4"), reminder_actuals_time: str = Form("15:00"),
    reminder_followup_dow: str = Form("0"), reminder_followup_time: str = Form("09:00"),
    template_plan_subject: str = Form(""), template_plan_body: str = Form(""),
    template_actuals_subject: str = Form(""), template_actuals_body: str = Form(""),
    template_followup_subject: str = Form(""), template_followup_body: str = Form(""),
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    adm.update_reminders(db, actor_id=user.id, values={
        "reminder_enabled": "true" if _checkbox(reminder_enabled) else "false",
        "email_base_url": email_base_url.strip(),
        "reminder_plan_dow": reminder_plan_dow, "reminder_plan_time": reminder_plan_time,
        "reminder_actuals_dow": reminder_actuals_dow, "reminder_actuals_time": reminder_actuals_time,
        "reminder_followup_dow": reminder_followup_dow, "reminder_followup_time": reminder_followup_time,
        "template_plan_subject": template_plan_subject, "template_plan_body": template_plan_body,
        "template_actuals_subject": template_actuals_subject, "template_actuals_body": template_actuals_body,
        "template_followup_subject": template_followup_subject, "template_followup_body": template_followup_body,
    })
    # Apply the new schedule to the running scheduler immediately.
    from app import scheduler
    scheduler.reschedule()
    return _redirect("/settings/reminders", msg="Reminder settings saved and rescheduled.")


# --------------------------------------------------------------------------- #
#  Data (backups)
# --------------------------------------------------------------------------- #
@router.get("/data")
def data_view(
    request: Request, msg: str = "", err: str = "",
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    from app import backup
    return templates.TemplateResponse(
        "settings/data.html",
        {"request": request, "user": user, "tab": "data",
         "backups": backup.list_backups(), "backup_dir": str(backup.backup_dir()),
         "enabled": get_setting(db, "backup_enabled") in ("true", "1", "yes", "on"),
         "backup_time": get_setting(db, "backup_time"),
         "msg": msg, "err": err},
    )


@router.post("/data/backup")
def data_backup_now(
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    from app import backup
    from app.audit import log as audit_log
    try:
        path = backup.run_backup()
    except Exception as e:  # noqa: BLE001
        return _redirect("/settings/data", err=f"Backup failed: {e}")
    audit_log(db, user_id=user.id, action="backup.manual", detail=f"Backup created: {path.name}")
    db.commit()
    return _redirect("/settings/data", msg=f"Backup created: {path.name}")


@router.post("/data/schedule")
def data_schedule(
    backup_enabled: str | None = Form(None), backup_time: str = Form("02:00"),
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    from app import scheduler
    from app.settings_store import set_setting
    set_setting(db, "backup_enabled", "true" if _checkbox(backup_enabled) else "false")
    set_setting(db, "backup_time", backup_time.strip() or "02:00")
    db.commit()
    scheduler.reschedule()
    return _redirect("/settings/data", msg="Backup schedule saved.")


# --------------------------------------------------------------------------- #
#  Audit log
# --------------------------------------------------------------------------- #
@router.get("/audit")
def audit_view(
    request: Request,
    user: Employee = Depends(require_manager), db: Session = Depends(get_db),
):
    entries = list(db.scalars(select(AuditLog).order_by(AuditLog.id.desc()).limit(200)))
    # Resolve actor names for display.
    names = {e.id: e.name for e in db.scalars(select(Employee))}
    return templates.TemplateResponse(
        "settings/audit.html",
        {"request": request, "user": user, "tab": "audit", "entries": entries, "names": names},
    )
