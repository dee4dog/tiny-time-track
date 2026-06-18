"""Login / logout routes."""
from __future__ import annotations

from fastapi import APIRouter, Depends, Form, Request
from fastapi.responses import RedirectResponse
from sqlalchemy.orm import Session

from app.database import get_db
from app.models import Employee
from app.security import verify_password
from app.templating import templates

router = APIRouter()


@router.get("/login")
def login_form(request: Request):
    """Show the login page (or bounce to home if already signed in)."""
    if request.session.get("user_id"):
        return RedirectResponse("/", status_code=303)
    return templates.TemplateResponse("login.html", {"request": request, "error": None})


@router.post("/login")
def login_submit(
    request: Request,
    email: str = Form(...),
    password: str = Form(...),
    db: Session = Depends(get_db),
):
    """Validate credentials and start a session."""
    email = email.strip().lower()
    user = db.query(Employee).filter(Employee.email == email).one_or_none()

    # Same generic error whether the email is unknown or the password wrong,
    # so we don't reveal which accounts exist.
    if user is None or not user.active or not verify_password(password, user.password_hash):
        return templates.TemplateResponse(
            "login.html",
            {"request": request, "error": "Incorrect email or password."},
            status_code=401,
        )

    # Store only the id in the signed cookie; everything else is re-read
    # from the database on each request.
    request.session["user_id"] = user.id
    return RedirectResponse("/", status_code=303)


@router.post("/logout")
def logout(request: Request):
    request.session.clear()
    return RedirectResponse("/login", status_code=303)
