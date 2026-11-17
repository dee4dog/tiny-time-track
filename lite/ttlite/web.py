"""HTTP layer for Tiny Time Track Lite: one local FastAPI app, no login.

There is no password because only one person uses the computer. Instead the
app listens on 127.0.0.1 only, and the LocalOnly guard rejects requests whose
Host isn't local (DNS-rebinding protection) and writes whose Origin/Referer
isn't local - so other websites open in the same browser can't post to it.
"""
from __future__ import annotations

import logging
import os
import sqlite3
import subprocess
import sys
from contextlib import asynccontextmanager
from datetime import date, datetime, timedelta
from pathlib import Path
from urllib.parse import urlencode, urlsplit

from fastapi import APIRouter, Depends, FastAPI, Form, Request
from fastapi.responses import JSONResponse, PlainTextResponse, RedirectResponse, Response
from fastapi.staticfiles import StaticFiles
from fastapi.templating import Jinja2Templates
from sqlalchemy.orm import Session
from fastapi.exceptions import RequestValidationError
from starlette.exceptions import HTTPException as StarletteHTTPException

from ttlite import APP_NAME, __version__, exports
from ttlite import services as svc
from ttlite.config import default_data_dir
from ttlite.db import get_db, init_db

log = logging.getLogger("ttlite")

PKG_DIR = Path(__file__).parent
STATIC_DIR = PKG_DIR / "static"
DB_FILENAME = "tinytimetrack-lite.db"
LOCAL_HOSTS = frozenset({"127.0.0.1", "localhost", "[::1]"})
XLSX_TYPE = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"


def _asset_ver(name: str) -> str:
    """Cache-busting token (file mtime) so edited CSS/JS is never served stale."""
    try:
        return str(int((STATIC_DIR / name).stat().st_mtime))
    except OSError:
        return __version__


templates = Jinja2Templates(directory=str(PKG_DIR / "templates"))
templates.env.globals.update(
    APP_NAME=APP_NAME,
    VERSION=__version__,
    DAY_NAMES=svc.DAY_NAMES,
    hrs=svc.fmt,
    cell=svc.fmt_cell,
    week_label=svc.week_label,
    asset_ver=_asset_ver,
)
templates.env.filters["dt"] = lambda v: f"{v.day} {v:%b %Y, %H:%M}" if v else ""
templates.env.filters["d"] = lambda v: f"{v.day} {v:%b %Y}" if v else ""
templates.env.filters["dday"] = lambda v: f"{v:%a} {v.day} {v:%b}" if v else ""


# --------------------------------------------------------------------------- #
#  Local-only guard
# --------------------------------------------------------------------------- #
def _hostname(netloc: str) -> str:
    netloc = netloc.strip().lower()
    if netloc.startswith("["):
        return netloc[: netloc.find("]") + 1]
    return netloc.split(":", 1)[0]


class LocalOnly:
    """Pure-ASGI middleware: local Host always; local Origin/Referer on writes."""

    SAFE_METHODS = frozenset({"GET", "HEAD", "OPTIONS"})

    def __init__(self, app, hosts: frozenset[str] = LOCAL_HOSTS) -> None:
        self.app = app
        self.hosts = hosts

    async def __call__(self, scope, receive, send) -> None:
        if scope["type"] == "http":
            headers = {k.decode("latin-1"): v.decode("latin-1") for k, v in scope.get("headers", [])}
            if _hostname(headers.get("host", "")) not in self.hosts:
                await PlainTextResponse("Invalid host.", status_code=400)(scope, receive, send)
                return
            if scope["method"] not in self.SAFE_METHODS:
                source = headers.get("origin") or headers.get("referer")
                if source is not None and (
                    source == "null" or _hostname(urlsplit(source).netloc) not in self.hosts
                ):
                    await PlainTextResponse("Cross-site request blocked.", status_code=403)(scope, receive, send)
                    return
        await self.app(scope, receive, send)


# --------------------------------------------------------------------------- #
#  Helpers
# --------------------------------------------------------------------------- #
def _go(url: str, *, msg: str = "", err: str = "") -> RedirectResponse:
    """Redirect (303) with an optional flash message in the query string."""
    params = {k: v for k, v in (("msg", msg), ("err", err)) if v}
    if params:
        url += ("&" if "?" in url else "?") + urlencode(params)
    return RedirectResponse(url, status_code=303)


def _week_url(start: date) -> str:
    return f"/week?week={start.isoformat()}"


def _form_week(raw: str) -> date | None:
    """Strict week parsing for writes (never silently fall back to 'this week')."""
    day = svc.parse_date(raw)
    return svc.monday_of(day) if day else None


def _page(request: Request, db: Session, name: str, *, nav: str = "",
          settings: dict[str, str] | None = None, status_code: int = 200, **ctx):
    ctx.update(
        settings=settings if settings is not None else svc.get_settings(db),
        nav=nav,
        msg=request.query_params.get("msg", "")[:300],
        err=request.query_params.get("err", "")[:300],
    )
    return templates.TemplateResponse(request, name, ctx, status_code=status_code)


def _doc(request: Request, name: str, settings: dict[str, str], **ctx):
    """Render a standalone printable document."""
    ctx.update(settings=settings, generated=datetime.now())
    return templates.TemplateResponse(request, name, ctx)


def _report_args(start: str | None, end: str | None, saved: str | None) -> tuple[date, date, bool]:
    default_start, default_end = svc.month_bounds(date.today())
    first = svc.parse_date(start) or default_start
    last = svc.parse_date(end) or default_end
    first, last = svc.clamp_range(first, last)
    return first, last, saved == "1"


def _report_query(report: svc.Report) -> str:
    params = {"start": report.start.isoformat(), "end": report.end.isoformat()}
    if report.saved_only:
        params["saved"] = "1"
    return urlencode(params)


router = APIRouter()


# --------------------------------------------------------------------------- #
#  Week: plan + actual
# --------------------------------------------------------------------------- #
@router.get("/")
def home() -> RedirectResponse:
    return RedirectResponse("/week", status_code=303)


@router.get("/healthz")
def healthz() -> dict:
    return {"app": APP_NAME, "version": __version__}


@router.get("/week")
def week_page(request: Request, week: str | None = None, db: Session = Depends(get_db)):
    settings = svc.get_settings(db)
    start = svc.parse_week(week)
    today = date.today()
    return _page(
        request, db, "week.html", nav="week", settings=settings,
        week=svc.build_week(db, start, svc.show_weekend(settings)),
        label=svc.week_label(start),
        prev=start - timedelta(days=7),
        next=start + timedelta(days=7),
        this_week=svc.monday_of(today),
        today=today,
        target=svc.daily_target(settings),
        has_projects=svc.has_projects(db),
        revision_count=len(svc.revisions(db, start)),
    )


@router.post("/api/cell")
def api_cell(
    project_id: int = Form(...),
    day: str = Form(...),
    field: str = Form(...),
    value: str = Form("", max_length=40),
    db: Session = Depends(get_db),
):
    when = svc.parse_date(day)
    if when is None:
        return JSONResponse({"error": "Invalid date."}, status_code=400)
    try:
        minutes = svc.save_cell(db, project_id=project_id, day=when, field=field, raw=value)
    except svc.LockedError as exc:
        return JSONResponse({"error": str(exc)}, status_code=423)
    except ValueError as exc:
        return JSONResponse({"error": str(exc)}, status_code=400)
    return {"value": svc.fmt_cell(minutes)}


@router.post("/api/week-note")
def api_week_note(week: str = Form(...), text: str = Form("", max_length=4000),
                  db: Session = Depends(get_db)):
    start = _form_week(week)
    if start is None:
        return JSONResponse({"error": "Invalid week."}, status_code=400)
    try:
        svc.save_week_note(db, start, text)
    except svc.LockedError as exc:
        return JSONResponse({"error": str(exc)}, status_code=423)
    return {"ok": True}


@router.post("/week/copy-plan")
def week_copy_plan(week: str = Form(...), db: Session = Depends(get_db)):
    start = _form_week(week)
    if start is None:
        return _go("/week", err="Invalid week.")
    try:
        count = svc.copy_last_week_plan(db, start)
    except svc.LockedError as exc:
        return _go(_week_url(start), err=str(exc))
    if count:
        return _go(_week_url(start), msg=f"Copied {count} planned {'entry' if count == 1 else 'entries'} from last week.")
    return _go(_week_url(start), msg="Nothing to copy: last week has no plan for active projects, "
                                     "or those cells are already planned.")


@router.post("/week/fill-actuals")
def week_fill_actuals(week: str = Form(...), db: Session = Depends(get_db)):
    start = _form_week(week)
    if start is None:
        return _go("/week", err="Invalid week.")
    today = date.today()
    try:
        count = svc.fill_actuals_from_plan(db, start, today)
    except svc.LockedError as exc:
        return _go(_week_url(start), err=str(exc))
    if count:
        return _go(_week_url(start), msg=f"Filled {count} actual {'cell' if count == 1 else 'cells'} from the plan.")
    if start > today:
        return _go(_week_url(start), msg="This week hasn't started yet, so there are no actual hours to fill in.")
    return _go(_week_url(start), msg="Nothing to fill: every planned day up to today already has actual time.")


@router.post("/week/save")
def week_save(week: str = Form(...), db: Session = Depends(get_db)):
    start = _form_week(week)
    if start is None:
        return _go("/week", err="Invalid week.")
    try:
        sheet = svc.save_timesheet(db, start)
    except ValueError as exc:
        return _go(_week_url(start), err=str(exc))
    return _go(_week_url(start), msg=f"Timesheet saved: {svc.fmt(sheet.total)} h for {svc.week_label(start)}.")


@router.post("/week/reopen")
def week_reopen(week: str = Form(...), db: Session = Depends(get_db)):
    start = _form_week(week)
    if start is None:
        return _go("/week", err="Invalid week.")
    try:
        svc.reopen_timesheet(db, start)
    except ValueError as exc:
        return _go(_week_url(start), err=str(exc))
    return _go(_week_url(start), msg="Timesheet reopened. Edit the week, then save it again.")


@router.post("/week/add-project")
def week_add_project(week: str = Form(...), name: str = Form(""), code: str = Form(""),
                     db: Session = Depends(get_db)):
    start = _form_week(week) or svc.monday_of(date.today())
    try:
        project = svc.add_project(db, name, code)
    except ValueError as exc:
        return _go(_week_url(start), err=str(exc))
    return _go(_week_url(start), msg=f"Added project “{project.name}”.")


# --------------------------------------------------------------------------- #
#  Printable documents
# --------------------------------------------------------------------------- #
@router.get("/print/plan")
def print_plan(request: Request, week: str | None = None, db: Session = Depends(get_db)):
    settings = svc.get_settings(db)
    start = svc.parse_week(week)
    wk = svc.build_week(db, start, svc.show_weekend(settings))
    planned_rows = [r for r in wk.rows if r.planned_total]
    rows = planned_rows or [r for r in wk.rows if r.project.active]
    return _doc(request, "print_plan.html", settings, week=wk, rows=rows,
                blank=not planned_rows, label=svc.week_label(start))


@router.get("/print/timesheet")
def print_timesheet(request: Request, week: str | None = None, db: Session = Depends(get_db)):
    settings = svc.get_settings(db)
    start = svc.parse_week(week)
    sheet = svc.timesheet_document(db, start, svc.show_weekend(settings))
    return _doc(request, "print_timesheet.html", settings, sheet=sheet, label=svc.week_label(start),
                revisions=svc.revisions(db, start), revision=None)


@router.get("/print/revision/{revision_id}")
def print_revision(request: Request, revision_id: int, db: Session = Depends(get_db)):
    """Print an earlier (reopened) version of a saved timesheet."""
    revision = db.get(svc.TimesheetRevision, revision_id)
    if revision is None:
        raise StarletteHTTPException(status_code=404)
    settings = svc.get_settings(db)
    lines = svc.revision_lines(revision)
    weekend = svc.show_weekend(settings) or any(line.days[5] or line.days[6] for line in lines)
    sheet = svc.SheetDoc(revision.week_start, lines, list(range(7 if weekend else 5)), revision.note, None)
    return _doc(request, "print_timesheet.html", settings, sheet=sheet,
                label=svc.week_label(revision.week_start), revisions=[], revision=revision)


@router.get("/print/report")
def print_report(request: Request, start: str | None = None, end: str | None = None,
                 saved: str | None = None, detail: str | None = None, db: Session = Depends(get_db)):
    first, last, saved_only = _report_args(start, end, saved)
    report = svc.build_report(db, first, last, saved_only)
    return _doc(request, "print_report.html", svc.get_settings(db), report=report, detail=detail == "1")


# --------------------------------------------------------------------------- #
#  Projects
# --------------------------------------------------------------------------- #
@router.get("/projects")
def projects_page(request: Request, db: Session = Depends(get_db)):
    return _page(request, db, "projects.html", nav="projects", stats=svc.project_overview(db))


@router.post("/projects")
def projects_add(name: str = Form(""), code: str = Form(""), client: str = Form(""),
                 db: Session = Depends(get_db)):
    try:
        project = svc.add_project(db, name, code, client)
    except ValueError as exc:
        return _go("/projects", err=str(exc))
    return _go("/projects", msg=f"Added “{project.name}”.")


@router.post("/projects/{project_id}")
def projects_update(project_id: int, name: str = Form(""), code: str = Form(""),
                    client: str = Form(""), db: Session = Depends(get_db)):
    try:
        project = svc.update_project(db, project_id, name, code, client)
    except ValueError as exc:
        return _go("/projects", err=str(exc))
    return _go("/projects", msg=f"Saved “{project.name}”.")


@router.post("/projects/{project_id}/archive")
def projects_archive(project_id: int, db: Session = Depends(get_db)):
    try:
        project = svc.set_project_active(db, project_id, False)
    except ValueError as exc:
        return _go("/projects", err=str(exc))
    return _go("/projects", msg=f"Archived “{project.name}”: hidden from new weeks, its hours are kept.")


@router.post("/projects/{project_id}/restore")
def projects_restore(project_id: int, db: Session = Depends(get_db)):
    try:
        project = svc.set_project_active(db, project_id, True)
    except ValueError as exc:
        return _go("/projects", err=str(exc))
    return _go("/projects", msg=f"Restored “{project.name}”.")


@router.post("/projects/{project_id}/delete")
def projects_delete(project_id: int, db: Session = Depends(get_db)):
    try:
        name = svc.delete_project(db, project_id)
    except ValueError as exc:
        return _go("/projects", err=str(exc))
    return _go("/projects", msg=f"Deleted “{name}”.")


# --------------------------------------------------------------------------- #
#  Timesheet records and reports
# --------------------------------------------------------------------------- #
@router.get("/timesheets")
def timesheets_page(request: Request, db: Session = Depends(get_db)):
    saved = svc.list_timesheets(db)
    return _page(
        request, db, "timesheets.html", nav="timesheets",
        saved=saved,
        saved_total=sum(t.total for t in saved),
        unsaved=svc.unsaved_weeks(db),
        totals=svc.saved_totals(db),
        revisions=svc.revisions(db),
    )


@router.get("/report")
def report_page(request: Request, start: str | None = None, end: str | None = None,
                saved: str | None = None, db: Session = Depends(get_db)):
    first, last, saved_only = _report_args(start, end, saved)
    report = svc.build_report(db, first, last, saved_only)
    return _page(request, db, "report.html", nav="report", report=report,
                 query=_report_query(report), ranges=svc.quick_ranges(date.today()))


@router.get("/export/report.xlsx")
def export_xlsx(start: str | None = None, end: str | None = None, saved: str | None = None,
                db: Session = Depends(get_db)):
    first, last, saved_only = _report_args(start, end, saved)
    report = svc.build_report(db, first, last, saved_only)
    return Response(
        exports.report_xlsx(report, svc.get_settings(db)),
        media_type=XLSX_TYPE,
        headers={"Content-Disposition": f'attachment; filename="{exports.filename(report, "xlsx")}"'},
    )


@router.get("/export/report.csv")
def export_csv(start: str | None = None, end: str | None = None, saved: str | None = None,
               kind: str = "detail", db: Session = Depends(get_db)):
    """``kind=detail``: one row per day and project. ``kind=totals``: the totalled timesheet grid."""
    first, last, saved_only = _report_args(start, end, saved)
    report = svc.build_report(db, first, last, saved_only)
    if kind == "totals":
        data, name = exports.report_totals_csv(report), exports.filename(report, "csv", "totals")
    else:
        data, name = exports.report_csv(report), exports.filename(report, "csv", "daily")
    return Response(data, media_type="text/csv; charset=utf-8",
                    headers={"Content-Disposition": f'attachment; filename="{name}"'})


# --------------------------------------------------------------------------- #
#  Settings and data safety
# --------------------------------------------------------------------------- #
@router.get("/settings")
def settings_page(request: Request, db: Session = Depends(get_db)):
    state = request.app.state
    sheets = svc.list_timesheets(db)
    return _page(request, db, "settings.html", nav="settings",
                 data_dir=state.data_dir, db_path=state.db_path,
                 backups=svc.list_backups(state.backup_dir),
                 latest_saved=max((t.saved_at for t in sheets), default=None))


@router.post("/settings")
def settings_save(user_name: str = Form(""), company: str = Form(""), daily_target: str = Form("8"),
                  show_weekend: str | None = Form(None), db: Session = Depends(get_db)):
    try:
        svc.save_settings(db, user_name=user_name, company=company,
                          daily_target_text=daily_target, weekend=bool(show_weekend))
    except ValueError as exc:
        return _go("/settings", err=str(exc))
    return _go("/settings", msg="Settings saved.")


@router.post("/settings/backup")
def settings_backup(request: Request):
    try:
        path = svc.make_backup(request.app.state.db_path, request.app.state.backup_dir)
    except (OSError, sqlite3.Error) as exc:
        return _go("/settings", err=f"Backup failed: {exc}")
    return _go("/settings", msg=f"Backup saved: {path.name}")


@router.post("/settings/restore")
def settings_restore(request: Request, name: str = Form(...)):
    state = request.app.state
    state.engine.dispose()  # close the app's connections before overwriting the database
    try:
        safety = svc.restore_backup(state.db_path, state.backup_dir, name)
    except (ValueError, OSError) as exc:
        return _go("/settings", err=str(exc))
    return _go("/settings", msg=f"Restored {name}. Your data from just before the restore was kept as {safety.name}.")


@router.post("/settings/open-folder")
def settings_open_folder(request: Request):
    folder = str(request.app.state.data_dir)
    try:
        if sys.platform == "win32":
            os.startfile(folder)  # noqa: S606 - local single-user app opening its own folder
        elif sys.platform == "darwin":
            subprocess.Popen(["open", folder])  # noqa: S603,S607
        else:
            subprocess.Popen(["xdg-open", folder])  # noqa: S603,S607
    except OSError as exc:
        return _go("/settings", err=f"Couldn't open the folder: {exc}")
    return _go("/settings", msg="Opened the data folder in your file manager.")


# --------------------------------------------------------------------------- #
#  App factory
# --------------------------------------------------------------------------- #
def _error_page(request: Request, code: int, message: str):
    if request.url.path.startswith("/api/"):
        return JSONResponse({"error": message}, status_code=code)
    return templates.TemplateResponse(
        request, "error.html",
        {"settings": svc.DEFAULT_SETTINGS, "nav": "", "code": code, "message": message},
        status_code=code,
    )


async def _http_error(request: Request, exc: StarletteHTTPException):
    if exc.status_code == 404:
        return _error_page(request, 404, "That page doesn't exist.")
    if exc.status_code == 405:
        return _error_page(request, 405, "That address can't be used like that.")
    return PlainTextResponse(str(exc.detail), status_code=exc.status_code)


async def _validation_error(request: Request, _exc: RequestValidationError):
    return _error_page(request, 400, "Something in that request was missing or invalid. "
                                     "Please go back and try again.")


async def _server_error(request: Request, exc: Exception):
    log.exception("Unexpected error on %s", request.url.path, exc_info=exc)
    return _error_page(request, 500, "Something went wrong on our side. Your saved data is safe; "
                                     "please go back and try again.")


def create_app(data_dir: str | Path | None = None) -> FastAPI:
    """Build the app around a data folder (created if missing)."""
    data_dir = Path(data_dir) if data_dir else default_data_dir()
    data_dir.mkdir(parents=True, exist_ok=True)
    db_path = data_dir / DB_FILENAME
    engine, session_factory = init_db(db_path)

    @asynccontextmanager
    async def lifespan(_app: FastAPI):
        yield
        engine.dispose()  # release the database file cleanly on shutdown

    app = FastAPI(title=APP_NAME, version=__version__, lifespan=lifespan,
                  docs_url=None, redoc_url=None, openapi_url=None)
    app.state.engine = engine
    app.state.SessionLocal = session_factory
    app.state.data_dir = data_dir
    app.state.db_path = db_path
    app.state.backup_dir = data_dir / "backups"

    try:
        svc.backup_if_due(db_path, app.state.backup_dir)
    except (OSError, sqlite3.Error) as exc:  # never block start-up on a backup problem
        log.warning("Automatic backup failed: %s", exc)

    app.add_middleware(LocalOnly)
    app.mount("/static", StaticFiles(directory=str(STATIC_DIR)), name="static")
    app.include_router(router)
    app.add_exception_handler(StarletteHTTPException, _http_error)
    app.add_exception_handler(RequestValidationError, _validation_error)
    app.add_exception_handler(Exception, _server_error)
    return app
