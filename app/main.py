"""FastAPI application factory and wiring.

Run for development with:
    python -m app
or:
    uvicorn app.main:app --host 0.0.0.0 --port 8000
"""
from __future__ import annotations

from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse, RedirectResponse
from fastapi.staticfiles import StaticFiles
from starlette.middleware.sessions import SessionMiddleware

from app import __version__, scheduler
from app.config import config
from app.database import SessionLocal, create_all_tables
from app.dependencies import NotAuthenticated, NotAuthorized
from app.routers import api, auth, manager, pages, settings, timesheet
from app.settings_store import ensure_defaults
from app.templating import templates

STATIC_DIR = Path(__file__).parent / "static"


@asynccontextmanager
async def lifespan(_app: FastAPI):
    """Create tables, seed default settings, and start the reminder scheduler."""
    create_all_tables()
    with SessionLocal() as db:
        ensure_defaults(db)
    scheduler.start()
    try:
        yield
    finally:
        scheduler.shutdown()


app = FastAPI(title="ES TimeTrack", version=__version__, lifespan=lifespan)

# Signs the session cookie so its contents can't be tampered with.
# same_site="lax" is fine for a same-origin LAN app; https_only is left off
# because the office server runs plain HTTP on the LAN.
app.add_middleware(
    SessionMiddleware,
    secret_key=config.secret_key,
    same_site="lax",
    max_age=14 * 24 * 60 * 60,  # 14 days
)

app.mount("/static", StaticFiles(directory=str(STATIC_DIR)), name="static")

app.include_router(auth.router)
app.include_router(pages.router)
app.include_router(timesheet.router)
app.include_router(manager.router)
app.include_router(settings.router)
app.include_router(api.router)


def _wants_json(request: Request) -> bool:
    """True for API calls (serve JSON errors instead of HTML redirects)."""
    return request.url.path.startswith("/api")


@app.exception_handler(NotAuthenticated)
async def _not_authenticated(request: Request, _exc: NotAuthenticated):
    if _wants_json(request):
        return JSONResponse({"detail": "Not authenticated"}, status_code=401)
    # Send humans to the login page.
    return RedirectResponse("/login", status_code=303)


@app.exception_handler(NotAuthorized)
async def _not_authorized(request: Request, _exc: NotAuthorized):
    if _wants_json(request):
        return JSONResponse({"detail": "Forbidden"}, status_code=403)
    return templates.TemplateResponse(
        "error.html",
        {"request": request, "code": 403, "message": "You don't have access to that page."},
        status_code=403,
    )


def run() -> None:
    """Entry point used by ``python -m app``."""
    import uvicorn

    uvicorn.run(
        "app.main:app",
        host=config.host,
        port=config.port,
        reload=False,
    )
