"""Top-level HTML pages (home + role-gated stubs for later phases).

The stubs prove the role gate works end-to-end now: an employee hitting
``/manager`` gets a 403, a manager gets the page. The real dashboard and
settings interfaces arrive in Phases 3-4.
"""
from __future__ import annotations

from datetime import date

from fastapi import APIRouter, Depends, Request
from sqlalchemy import select
from sqlalchemy.orm import Session

from app.database import get_db
from app.dependencies import get_current_user
from app.models import Deadline, Employee, Role
from app.templating import templates

router = APIRouter()


@router.get("/")
def home(
    request: Request,
    user: Employee = Depends(get_current_user),
    db: Session = Depends(get_db),
):
    today = date.today()
    deadlines = list(
        db.scalars(
            select(Deadline).where(Deadline.due_date >= today).order_by(Deadline.due_date)
        )
    )
    return templates.TemplateResponse(
        "home.html",
        {"request": request, "user": user, "is_manager": user.role == Role.manager,
         "deadlines": deadlines, "today": today},
    )
