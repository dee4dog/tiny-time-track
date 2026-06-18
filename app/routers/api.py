"""JSON API endpoints.

Phase 1 ships a single endpoint, ``/api/me``, that returns the current
user's *safe* fields only. Note what is deliberately absent: annual_salary
and any rate data. This is the pattern every future API endpoint follows -
sensitive fields are never serialized for an employee-role response.
"""
from __future__ import annotations

from fastapi import APIRouter, Depends

from app.dependencies import get_current_user
from app.models import Employee

router = APIRouter(prefix="/api")


@router.get("/me")
def me(user: Employee = Depends(get_current_user)) -> dict:
    return {
        "id": user.id,
        "name": user.name,
        "email": user.email,
        "role": user.role.value,
        # annual_salary / rates intentionally omitted.
    }
