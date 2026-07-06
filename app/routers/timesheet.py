"""Employee weekly timesheet (the Excel-style Mon–Fri grid).

Routes:
  GET  /timesheet                 render the week's grid (defaults to this week)
  POST /timesheet/cell            autosave one cell field (htmx, returns 204)
  POST /timesheet/submit          stamp plan/actuals submission
  POST /timesheet/copy-last-week  copy previous week's plan into this week
  POST /timesheet/add-project     add an active project to the grid
"""
from __future__ import annotations

from datetime import date

from fastapi import APIRouter, Depends, Form, Request
from fastapi.responses import RedirectResponse, Response
from sqlalchemy import select
from sqlalchemy.orm import Session

from app import timesheet_service as ts
from app.database import get_db
from app.dependencies import get_current_user
from app.models import Employee, PlanReview
from app.templating import templates
from app.weeks import DAY_NAMES, format_week_range, parse_week, shift_week, week_days

router = APIRouter(prefix="/timesheet")


def _redirect_to_week(week_start: date, saved: str | None = None) -> RedirectResponse:
    url = f"/timesheet?week={week_start.isoformat()}"
    if saved:
        url += f"&saved={saved}"
    return RedirectResponse(url, status_code=303)


@router.get("")
def timesheet(
    request: Request,
    week: str | None = None,
    saved: str | None = None,
    user: Employee = Depends(get_current_user),
    db: Session = Depends(get_db),
):
    week_start = parse_week(week)
    rows = ts.build_grid(db, user, week_start)
    status = ts.get_week_status(db, user, week_start)
    locked = ts.is_locked(db, user, week_start)
    review = db.scalar(
        select(PlanReview).where(
            PlanReview.employee_id == user.id, PlanReview.week_start == week_start
        )
    )

    return templates.TemplateResponse(
        "timesheet.html",
        {
            "request": request,
            "user": user,
            "week_start": week_start,
            "week_label": format_week_range(week_start),
            "prev_week": shift_week(week_start, -1),
            "next_week": shift_week(week_start, 1),
            "day_names": DAY_NAMES,
            "day_dates": week_days(week_start),
            "rows": rows,
            "status": status,
            "locked": locked,
            "review": review,
            "addable": ts.addable_projects(db, user),
            "saved": saved,
        },
    )


@router.post("/cell")
def save_cell(
    request: Request,
    project_id: int = Form(...),
    day: int = Form(...),
    field: str = Form(...),
    value: str = Form(""),
    week: str = Form(...),
    user: Employee = Depends(get_current_user),
    db: Session = Depends(get_db),
):
    """Autosave a single cell field. Called by htmx on input change."""
    week_start = parse_week(week)

    if ts.is_locked(db, user, week_start):
        return Response("locked", status_code=423)  # 423 Locked

    try:
        ts.save_cell(
            db, user,
            project_id=project_id, week_start=week_start,
            day=day, field=field, value=value,
        )
    except ValueError as exc:
        return Response(str(exc), status_code=400)

    return Response(status_code=204)  # success, nothing to swap


@router.post("/submit")
def submit(
    which: str = Form(...),
    week: str = Form(...),
    user: Employee = Depends(get_current_user),
    db: Session = Depends(get_db),
):
    week_start = parse_week(week)
    try:
        ts.submit_week(db, user, week_start, which)
    except ValueError:
        return _redirect_to_week(week_start)
    return _redirect_to_week(week_start, saved=which)


@router.post("/copy-last-week")
def copy_last_week(
    week: str = Form(...),
    user: Employee = Depends(get_current_user),
    db: Session = Depends(get_db),
):
    week_start = parse_week(week)
    if not ts.is_locked(db, user, week_start):
        ts.copy_last_week_plan(db, user, week_start)
    return _redirect_to_week(week_start, saved="copied")


@router.post("/add-project")
def add_project(
    project_id: int = Form(...),
    week: str = Form(...),
    user: Employee = Depends(get_current_user),
    db: Session = Depends(get_db),
):
    week_start = parse_week(week)
    ts.add_project_to_grid(db, user, project_id)
    return _redirect_to_week(week_start)
