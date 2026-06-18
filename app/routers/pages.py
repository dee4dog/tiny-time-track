"""Top-level HTML pages (home + role-gated stubs for later phases).

The stubs prove the role gate works end-to-end now: an employee hitting
``/manager`` gets a 403, a manager gets the page. The real dashboard and
settings interfaces arrive in Phases 3-4.
"""
from __future__ import annotations

from fastapi import APIRouter, Depends, Request

from app.dependencies import get_current_user
from app.models import Employee, Role
from app.templating import templates

router = APIRouter()


@router.get("/")
def home(request: Request, user: Employee = Depends(get_current_user)):
    return templates.TemplateResponse(
        "home.html",
        {"request": request, "user": user, "is_manager": user.role == Role.manager},
    )
