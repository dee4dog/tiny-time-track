"""HTTP behaviour: pages, autosave API, printing, exports and the local-only guard."""
from __future__ import annotations

import csv
import io
from datetime import date, timedelta

from openpyxl import load_workbook

from ttlite import services as svc

MON = date(2026, 11, 16)
WEEK = MON.isoformat()


def add_project(client, name="Alpha", code="A1"):
    r = client.post("/projects", data={"name": name, "code": code})
    assert r.status_code == 200
    return r


def project_id(db, name="Alpha"):
    from sqlalchemy import select
    from ttlite.models import Project

    return db.scalar(select(Project.id).where(Project.name == name))


def put(client, pid, offset, field, value):
    return client.post("/api/cell", data={
        "project_id": pid, "day": (MON + timedelta(days=offset)).isoformat(), "field": field, "value": value,
    })


# --------------------------------------------------------------------------- #
#  Pages
# --------------------------------------------------------------------------- #
def test_pages_render(client):
    for url in ["/week", f"/week?week={WEEK}", "/projects", "/timesheets", "/report", "/settings",
                "/print/plan", "/print/timesheet", "/print/report"]:
        r = client.get(url)
        assert r.status_code == 200, url
        assert "Tiny Time Track" in r.text


def test_root_redirects_and_404(client):
    assert client.get("/", follow_redirects=False).headers["location"] == "/week"
    r = client.get("/nope")
    assert r.status_code == 404 and "That page doesn" in r.text


def test_empty_state_then_grid(client, db):
    assert "Start by adding a project" in client.get("/week").text
    r = client.post("/week/add-project", data={"week": WEEK, "name": "Alpha", "code": "A1"})
    assert "Added project" in r.text and 'id="grid"' in r.text
    assert project_id(db)


# --------------------------------------------------------------------------- #
#  Autosave API
# --------------------------------------------------------------------------- #
def test_cell_api_normalises_and_validates(client, db):
    add_project(client)
    pid = project_id(db)
    r = put(client, pid, 0, "planned", "7:30")
    assert r.status_code == 200 and r.json() == {"value": "7.5"}
    assert put(client, pid, 0, "actual", "abc").status_code == 400
    assert put(client, pid, 0, "bogus", "1").status_code == 400
    assert put(client, 99999, 0, "actual", "1").status_code == 400
    bad_day = client.post("/api/cell", data={"project_id": pid, "day": "x", "field": "actual", "value": "1"})
    assert bad_day.status_code == 400
    page = client.get(f"/week?week={WEEK}").text
    assert 'value="7.5"' in page


def test_week_note_api(client):
    add_project(client)  # the notes box sits under the grid
    assert client.post("/api/week-note", data={"week": WEEK, "text": "Goals"}).json() == {"ok": True}
    assert "Goals" in client.get(f"/week?week={WEEK}").text
    assert client.post("/api/week-note", data={"week": "nope", "text": "x"}).status_code == 400


def test_save_lock_and_reopen_flow(client, db):
    add_project(client)
    pid = project_id(db)
    put(client, pid, 0, "actual", "8")
    put(client, pid, 1, "actual", "4")
    r = client.post("/week/save", data={"week": WEEK})
    assert "Timesheet saved: 12 h" in r.text
    assert put(client, pid, 2, "actual", "1").status_code == 423
    assert client.post("/api/week-note", data={"week": WEEK, "text": "x"}).status_code == 423
    assert "already saved" in client.post("/week/save", data={"week": WEEK}).text
    timesheets = client.get("/timesheets").text
    assert "16 – 22 Nov 2026" in timesheets and "12" in timesheets
    r = client.post("/week/reopen", data={"week": WEEK})
    assert "reopened" in r.text
    assert put(client, pid, 2, "actual", "1").status_code == 200


def test_copy_and_fill_buttons(client, db):
    add_project(client)
    pid = project_id(db)
    put(client, pid, -7, "planned", "5")
    r = client.post("/week/copy-plan", data={"week": WEEK})
    assert "Copied 1 planned entry" in r.text
    r = client.post("/week/fill-actuals", data={"week": WEEK})  # MON is in the past
    assert "Filled 1 actual cell" in r.text
    assert client.post("/week/copy-plan", data={"week": "garbage"}, follow_redirects=False).status_code == 303


# --------------------------------------------------------------------------- #
#  Projects
# --------------------------------------------------------------------------- #
def test_project_crud(client, db):
    add_project(client)
    assert "already have" in client.post("/projects", data={"name": "alpha", "code": "a1"}).text
    pid = project_id(db)
    assert "Saved “Alpha 2”" in client.post(f"/projects/{pid}", data={"name": "Alpha 2", "code": "A1"}).text
    assert "Archived" in client.post(f"/projects/{pid}/archive").text
    assert "Restored" in client.post(f"/projects/{pid}/restore").text
    assert "Deleted" in client.post(f"/projects/{pid}/delete").text
    assert "no longer exists" in client.post(f"/projects/{pid}/archive").text


# --------------------------------------------------------------------------- #
#  Printing and exports
# --------------------------------------------------------------------------- #
def _filled(client, db):
    client.post("/settings", data={"user_name": "Dirk Cilliers", "company": "ES", "daily_target": "8"})
    add_project(client, "Alpha", "A1")
    add_project(client, "=cmd()", "")  # formula-looking name must stay text in exports
    a, b = project_id(db, "Alpha"), project_id(db, "=cmd()")
    for i in range(5):
        put(client, a, i, "planned", "6")
        put(client, a, i, "actual", "6.5")
        put(client, b, i, "actual", "1.5")
    client.post("/api/week-note", data={"week": WEEK, "text": "Concept issue Friday"})
    return a, b


def test_print_documents(client, db):
    _filled(client, db)
    plan = client.get(f"/print/plan?week={WEEK}").text
    assert "Weekly plan" in plan and "Dirk Cilliers" in plan and "Concept issue Friday" in plan
    assert ">30<" in plan  # planned week total for Alpha
    sheet = client.get(f"/print/timesheet?week={WEEK}").text
    assert "DRAFT" in sheet and ">40<" in sheet  # 32.5 + 7.5
    client.post("/week/save", data={"week": WEEK})
    sheet = client.get(f"/print/timesheet?week={WEEK}").text
    assert "Saved record" in sheet and "is-draft" not in sheet
    report = client.get(f"/print/report?start={WEEK}&end=2026-11-22&detail=1").text
    assert "<title>Timesheet " in report and "Daily detail" in report and "32.5" in report


def test_xlsx_export(client, db):
    _filled(client, db)
    r = client.get(f"/export/report.xlsx?start={WEEK}&end=2026-11-22")
    assert r.status_code == 200
    assert "timesheet_2026-11-16_to_2026-11-22.xlsx" in r.headers["content-disposition"]
    wb = load_workbook(io.BytesIO(r.content))
    assert wb.sheetnames == ["Timesheet", "Summary", "By week", "Daily detail"]
    rows = [row for row in wb["Summary"].iter_rows(values_only=True) if row and row[1]]
    by_name = {row[1]: row[2] for row in rows}
    assert by_name["Alpha"] == 32.5 and by_name["=cmd()"] == 7.5
    assert str(by_name["Total"]).startswith("=SUM(C")  # a live formula, not a typed number
    cell = next(c for row in wb["Summary"].iter_rows() for c in row if c.value == "=cmd()")
    assert cell.data_type == "s"  # project name stays text, not a formula
    grid = wb["Timesheet"]
    assert grid.page_setup.orientation == "landscape" and grid.print_title_rows
    data = [row for row in grid.iter_rows(values_only=True) if row and row[1] in ("Alpha", "=cmd()")]
    assert {row[1]: row[2] for row in data} == {"Alpha": 32.5, "=cmd()": 7.5}
    assert all(str(row[3]).startswith("=SUM(") for row in data)  # row totals
    total_row = next(row for row in grid.iter_rows(values_only=True) if row and row[1] == "Total")
    assert str(total_row[2]).startswith("=SUM(C")  # column totals


def test_csv_export(client, db):
    _filled(client, db)
    r = client.get(f"/export/report.csv?start={WEEK}&end=2026-11-22")
    assert "_daily.csv" in r.headers["content-disposition"]
    text = r.content.decode("utf-8-sig")
    rows = list(csv.reader(io.StringIO(text)))
    assert rows[0] == ["Date", "Day", "Project code", "Project", "Hours", "Status"]
    assert len(rows) == 11
    assert sum(float(row[4]) for row in rows[1:]) == 40
    assert any(row[3] == "'=cmd()" for row in rows[1:])  # neutralised formula


def test_report_page_saved_only_banner(client, db):
    _filled(client, db)
    page = client.get(f"/report?start={WEEK}&end=2026-11-22").text
    assert "Includes 40 h from weeks not yet saved" in page
    page = client.get(f"/report?start={WEEK}&end=2026-11-22&saved=1").text
    assert "40 h from weeks that aren" in page


# --------------------------------------------------------------------------- #
#  Settings and the local-only guard
# --------------------------------------------------------------------------- #
def test_settings_validation(client):
    add_project(client)
    assert "Sun<span" not in client.get("/week").text
    assert "Settings saved" in client.post("/settings", data={"daily_target": "7.5", "show_weekend": "1"}).text
    assert "Sun<span" in client.get("/week").text  # weekend columns now shown
    assert "more than 0" in client.post("/settings", data={"daily_target": "0"}).text


def test_backup_button(client, app):
    r = client.post("/settings/backup")
    assert "Backup saved" in r.text
    assert svc.list_backups(app.state.backup_dir)


def test_guard_blocks_foreign_host_and_origin(client):
    assert client.get("/week", headers={"host": "evil.example"}).status_code == 400
    blocked = client.post("/projects", data={"name": "x"}, headers={"origin": "http://evil.example"},
                          follow_redirects=False)
    assert blocked.status_code == 403
    assert client.post("/projects", data={"name": "x"}, headers={"origin": "null"},
                       follow_redirects=False).status_code == 403
    assert client.post("/projects", data={"name": "y"}, headers={"referer": "http://evil.example/page"},
                       follow_redirects=False).status_code == 403
    ok = client.post("/projects", data={"name": "z"}, headers={"origin": "http://127.0.0.1:8100"},
                     follow_redirects=False)
    assert ok.status_code == 303
    assert client.post("/projects", data={"name": "w"}, headers={"origin": "http://localhost:8100"},
                       follow_redirects=False).status_code == 303


# --------------------------------------------------------------------------- #
#  Round 2: totals CSV, restore, friendly errors, edge dates
# --------------------------------------------------------------------------- #
def test_totals_csv(client, db):
    _filled(client, db)
    r = client.get(f"/export/report.csv?start={WEEK}&end=2026-11-22&kind=totals")
    assert "_totals.csv" in r.headers["content-disposition"]
    rows = list(csv.reader(io.StringIO(r.content.decode("utf-8-sig"))))
    assert rows[0] == ["Project code", "Project", "2026-11-16 to 2026-11-22", "Total"]
    assert rows[-1] == ["", "Total", "40.00", "40.00"]
    assert ["A1", "Alpha", "32.50", "32.50"] in rows


def test_report_page_and_print_show_timesheet_grid(client, db):
    _filled(client, db)
    page = client.get(f"/report?start={WEEK}&end=2026-11-22").text
    assert "hours per project per week" in page and "32.5" in page
    printed = client.get(f"/print/report?start={WEEK}&end=2026-11-22").text
    assert 'class="doc-table matrix"' in printed and ">40<" in printed
    year = client.get("/report?start=2026-01-01&end=2026-12-31").text
    assert "per month" in year and "weeks in this period with no time are not listed" in year


def test_restore_via_settings(client, app, db):
    add_project(client, "Keep")
    client.post("/settings/backup")
    backup = svc.list_backups(app.state.backup_dir)[0].name
    add_project(client, "Later")
    r = client.post("/settings/restore", data={"name": backup})
    assert "Restored" in r.text and "kept as" in r.text
    projects = client.get("/projects").text
    assert "Keep" in projects and "Later" not in projects
    assert "no longer exists" in client.post("/settings/restore", data={"name": "nope.db"}).text


def test_friendly_errors_and_edge_dates(client):
    r = client.post("/projects/abc/archive")
    assert r.status_code == 400 and "missing or invalid" in r.text and "{" not in r.text.split("<main")[1][:200]
    assert client.post("/api/cell", data={}).json()["error"]
    for url in ["/week?week=9999-12-31", "/week?week=0001-01-01", "/report?start=9999-12-01&end=9999-12-31",
                "/print/plan?week=9999-12-31"]:
        assert client.get(url).status_code == 200, url


def test_fill_actuals_future_week_message(client, db):
    add_project(client)
    future = (date.today() + timedelta(days=21)).isoformat()
    r = client.post("/week/fill-actuals", data={"week": future})
    assert "hasn" in r.text and "started yet" in r.text


# --------------------------------------------------------------------------- #
#  Round 3: revision history, all backups, Excel recalculation
# --------------------------------------------------------------------------- #
def test_revision_history_pages(client, db):
    add_project(client)
    pid = project_id(db)
    put(client, pid, 0, "actual", "6")
    client.post("/week/save", data={"week": WEEK})
    client.post("/week/reopen", data={"week": WEEK})
    put(client, pid, 1, "actual", "2")
    client.post("/week/save", data={"week": WEEK})
    assert "Revision 2" in client.get(f"/week?week={WEEK}").text
    page = client.get("/timesheets").text
    assert "Earlier versions of reopened timesheets (1)" in page
    from ttlite.models import TimesheetRevision
    from sqlalchemy import select
    rid = db.scalar(select(TimesheetRevision.id))
    printed = client.get(f"/print/revision/{rid}").text
    assert "Earlier saved version" in printed and ">6<" in printed
    assert "revision 2" in client.get(f"/print/timesheet?week={WEEK}").text
    assert client.get("/print/revision/9999").status_code == 404


def test_settings_lists_every_backup(client, app):
    add_project(client)
    for _ in range(12):
        svc.make_backup(app.state.db_path, app.state.backup_dir)
    page = client.get("/settings").text
    assert page.count('action="/settings/restore"') == len(svc.list_backups(app.state.backup_dir)) >= 12


def test_xlsx_recalculates_on_open(client, db):
    _filled(client, db)
    wb = load_workbook(io.BytesIO(client.get(f"/export/report.xlsx?start={WEEK}&end=2026-11-22").content))
    assert wb.calculation.fullCalcOnLoad


def test_restore_older_backup_keeps_every_page_working(client, app, db):
    import sqlite3

    add_project(client)
    put(client, project_id(db), 0, "actual", "4")
    client.post("/week/save", data={"week": WEEK})
    client.post("/settings/backup")
    old = svc.list_backups(app.state.backup_dir)[0].path
    con = sqlite3.connect(old)
    con.execute("DROP TABLE timesheet_revisions")
    con.commit()
    con.close()
    assert "Restored" in client.post("/settings/restore", data={"name": old.name}).text
    for url in ["/timesheets", f"/week?week={WEEK}", f"/print/timesheet?week={WEEK}", "/report", "/projects"]:
        assert client.get(url).status_code == 200, url
    assert "reopened" in client.post("/week/reopen", data={"week": WEEK}).text
    client.post("/week/save", data={"week": WEEK})  # re-save: the week now has an earlier version
    assert 'href="/timesheets#revisions"' in client.get(f"/week?week={WEEK}").text
