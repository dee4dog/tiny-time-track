"""SMTP email sending, configured from the settings table.

Used now for the "Send test email" button on the Settings > Email screen, and
reused by the Phase 5 reminder scheduler. Connection details (host, port,
credentials, from-address) come from the database, never from code.
"""
from __future__ import annotations

import smtplib
import ssl
from dataclasses import dataclass
from email.message import EmailMessage

from sqlalchemy.orm import Session

from app.settings_store import get_bool, get_setting


@dataclass
class SmtpConfig:
    host: str
    port: int
    use_tls: bool
    username: str
    password: str
    from_addr: str

    @property
    def configured(self) -> bool:
        return bool(self.host and self.from_addr)


def load_smtp(db: Session) -> SmtpConfig:
    username = get_setting(db, "smtp_username") or ""
    from_addr = get_setting(db, "smtp_from") or username
    try:
        port = int(get_setting(db, "smtp_port") or "587")
    except ValueError:
        port = 587
    return SmtpConfig(
        host=get_setting(db, "smtp_host") or "",
        port=port,
        use_tls=get_bool(db, "smtp_use_tls"),
        username=username,
        password=get_setting(db, "smtp_password") or "",
        from_addr=from_addr,
    )


def send_email(
    db: Session, *, to: str, subject: str, body: str
) -> tuple[bool, str]:
    """Send a plain-text email. Returns (ok, message).

    Never raises - the caller (a web request or the scheduler) gets a tidy
    (False, "reason") instead of a stack trace.
    """
    cfg = load_smtp(db)
    if not cfg.configured:
        return False, "SMTP is not configured (set host and from-address first)."

    message = EmailMessage()
    message["From"] = cfg.from_addr
    message["To"] = to
    message["Subject"] = subject
    message.set_content(body)

    try:
        with smtplib.SMTP(cfg.host, cfg.port, timeout=20) as server:
            server.ehlo()
            if cfg.use_tls:
                server.starttls(context=ssl.create_default_context())
                server.ehlo()
            if cfg.username:
                server.login(cfg.username, cfg.password)
            server.send_message(message)
        return True, f"Sent to {to}."
    except Exception as exc:  # noqa: BLE001 - surface any failure to the UI
        return False, f"{type(exc).__name__}: {exc}"


def send_test_email(db: Session, to: str) -> tuple[bool, str]:
    return send_email(
        db,
        to=to,
        subject="Tiny Time Track — test email",
        body=(
            "This is a test email from Tiny Time Track.\n\n"
            "If you received this, your SMTP settings are working."
        ),
    )
