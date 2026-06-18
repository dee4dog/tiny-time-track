"""Tiny helper for writing to the audit_log (salary/fee/settings changes)."""
from __future__ import annotations

from sqlalchemy.orm import Session

from app.models import AuditLog


def log(db: Session, *, user_id: int | None, action: str, detail: str = "") -> None:
    """Append an audit entry. Caller commits (or relies on a later commit)."""
    db.add(AuditLog(user_id=user_id, action=action, detail=detail))
