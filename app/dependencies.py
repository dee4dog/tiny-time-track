"""Authentication & authorization dependencies.

Central place where role checks live. Section 2 of the brief requires that
*every* endpoint enforces role checks - salary and fee data must never be
reachable by an employee role, even by calling the API directly. Manager-only
routes therefore depend on ``require_manager``.

Two custom exceptions let us respond differently to humans and machines:
HTML page requests get redirected to the login screen, while API requests
(paths starting with ``/api``) get a clean 401/403 JSON response. The handlers
are registered in ``app/main.py``.
"""
from __future__ import annotations

from fastapi import Depends, Request
from sqlalchemy.orm import Session

from app.database import get_db
from app.models import Employee, Role


class NotAuthenticated(Exception):
    """Raised when there is no valid logged-in user."""


class NotAuthorized(Exception):
    """Raised when a logged-in user lacks the required role."""


def get_current_user(
    request: Request, db: Session = Depends(get_db)
) -> Employee:
    """Return the logged-in employee, or raise NotAuthenticated.

    The user id is read from the signed session cookie. We re-load the
    employee from the database on every request so a deactivated account
    immediately loses access.
    """
    user_id = request.session.get("user_id")
    if not user_id:
        raise NotAuthenticated()

    user = db.get(Employee, user_id)
    if user is None or not user.active:
        # Stale/closed session - clear it so the cookie stops being sent.
        request.session.clear()
        raise NotAuthenticated()
    return user


def require_manager(
    user: Employee = Depends(get_current_user),
) -> Employee:
    """Allow only manager-role users through. Employees get 403."""
    if user.role != Role.manager:
        raise NotAuthorized()
    return user
