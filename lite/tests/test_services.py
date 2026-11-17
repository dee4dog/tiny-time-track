"""Business rules: hours parsing, the week grid, saved timesheets, reports, backups."""
from __future__ import annotations

from datetime import date, timedelta

import pytest

from ttlite import services as svc

MON = date(2026, 11, 16)  # a Monday


def day(offset: int) -> date:
    return MON + timedelta(days=offset)


# --------------------------------------------------------------------------- #
#  Hours
# --------------------------------------------------------------------------- #
@pytest.mark.parametrize(
    "raw, minutes",
    [("", 0), ("  ", 0), (None, 0), ("8", 480), ("7.5", 450), ("7,5", 450), ("7:30", 450),
     ("7h", 420), ("0.25", 15), ("0.2", 15), ("0:15", 15), ("1:20", 75), ("24", 1440), ("-0", 0)],
)
def test_parse_minutes(raw, minutes):
    assert svc.parse_minutes(raw) == minutes


@pytest.mark.parametrize("raw", ["abc", "-1", "25", "24.5", "nan", "inf", "1:75", "7.5.5", "12345678901", "0.1", "0:05"])
def test_parse_minutes_rejects(raw):
    with pytest.raises(ValueError):
        svc.parse_minutes(raw)


@pytest.mark.parametrize("minutes, text", [(0, "0"), (480, "8"), (450, "7.5"), (15, "0.25"),
                                           (45, "0.75"), (600, "10"), (2265, "37.75")])
def test_fmt(minutes, text):
    assert svc.fmt(minutes) == text


def test_fmt_cell_blank_for_zero():
    assert svc.fmt_cell(0) == "" and svc.fmt_cell(30) == "0.5"


def test_week_labels():
    assert svc.week_label(MON) == "16 – 22 Nov 2026"
    assert svc.week_label(date(2026, 11, 30)) == "30 Nov – 6 Dec 2026"
    assert svc.week_label(date(2026, 12, 28)) == "28 Dec 2026 – 3 Jan 2027"
    assert svc.monday_of(date(2026, 11, 22)) == MON
    assert svc.parse_week("2026-11-19") == MON


# --------------------------------------------------------------------------- #
#  Projects
# --------------------------------------------------------------------------- #
def test_projects_unique_and_cleaned(db):
    p = svc.add_project(db, "  Longlands   Clubhouse ", "3676")
    assert p.name == "Longlands Clubhouse"
    with pytest.raises(ValueError, match="already have"):
        svc.add_project(db, "longlands clubhouse", "3676")
    svc.add_project(db, "Longlands Clubhouse", "9999")  # different code is fine
    with pytest.raises(ValueError, match="name"):
        svc.add_project(db, "   ")


def test_delete_only_unused_projects(db):
    used = svc.add_project(db, "Used")
    unused = svc.add_project(db, "Unused")
    svc.save_cell(db, project_id=used.id, day=day(0), field="planned", raw="2")
    with pytest.raises(ValueError, match="archive"):
        svc.delete_project(db, used.id)
    assert svc.delete_project(db, unused.id) == "Unused"


def test_archived_projects_hidden_unless_used(db):
    keep = svc.add_project(db, "Keep")
    old = svc.add_project(db, "Old")
    svc.set_project_active(db, old.id, False)
    names = [r.project.name for r in svc.build_week(db, MON).rows]
    assert names == ["Keep"]
    svc.set_project_active(db, old.id, True)
    svc.save_cell(db, project_id=old.id, day=day(1), field="actual", raw="3")
    svc.set_project_active(db, old.id, False)
    names = [r.project.name for r in svc.build_week(db, MON).rows]
    assert names == ["Keep", "Old"]  # archived but has time this week
    assert keep.active


# --------------------------------------------------------------------------- #
#  Week grid
# --------------------------------------------------------------------------- #
def test_save_cell_and_totals(db):
    a = svc.add_project(db, "A")
    b = svc.add_project(db, "B")
    assert svc.save_cell(db, project_id=a.id, day=day(0), field="planned", raw="7:30") == 450
    svc.save_cell(db, project_id=b.id, day=day(0), field="planned", raw="0.5")
    svc.save_cell(db, project_id=a.id, day=day(0), field="actual", raw="9")
    week = svc.build_week(db, MON)
    assert week.day_totals("planned")[0] == 480
    assert week.day_totals("actual")[0] == 540
    assert week.planned_total == 480 and week.actual_total == 540
    assert week.visible == [0, 1, 2, 3, 4]


def test_clearing_both_values_removes_row(db):
    from sqlalchemy import func, select
    from ttlite.models import Entry

    a = svc.add_project(db, "A")
    svc.save_cell(db, project_id=a.id, day=day(0), field="planned", raw="2")
    svc.save_cell(db, project_id=a.id, day=day(0), field="planned", raw="")
    assert db.scalar(select(func.count()).select_from(Entry)) == 0


def test_day_cannot_exceed_24_hours(db):
    a = svc.add_project(db, "A")
    b = svc.add_project(db, "B")
    svc.save_cell(db, project_id=a.id, day=day(0), field="actual", raw="20")
    with pytest.raises(ValueError, match="24 hours"):
        svc.save_cell(db, project_id=b.id, day=day(0), field="actual", raw="5")
    svc.save_cell(db, project_id=b.id, day=day(0), field="actual", raw="4")  # exactly 24 is fine


def test_weekend_columns_appear_when_used(db):
    a = svc.add_project(db, "A")
    assert svc.build_week(db, MON).visible == [0, 1, 2, 3, 4]
    assert svc.build_week(db, MON, weekend=True).visible == list(range(7))
    svc.save_cell(db, project_id=a.id, day=day(5), field="actual", raw="2")
    assert svc.build_week(db, MON).visible == list(range(7))


def test_week_note_round_trip(db):
    svc.save_week_note(db, MON, "Finish drawings\r\nCall client")
    assert svc.build_week(db, MON).note == "Finish drawings\nCall client"


def test_copy_last_week_plan(db):
    a = svc.add_project(db, "A")
    svc.save_cell(db, project_id=a.id, day=day(-7), field="planned", raw="6")
    svc.save_cell(db, project_id=a.id, day=day(-6), field="planned", raw="4")
    svc.save_cell(db, project_id=a.id, day=day(1), field="planned", raw="1")  # already planned: kept
    assert svc.copy_last_week_plan(db, MON) == 1
    week = svc.build_week(db, MON)
    assert week.rows[0].planned[:2] == [360, 60]
    assert svc.copy_last_week_plan(db, MON) == 0


def test_fill_actuals_only_up_to_today(db):
    a = svc.add_project(db, "A")
    for i in range(5):
        svc.save_cell(db, project_id=a.id, day=day(i), field="planned", raw="8")
    svc.save_cell(db, project_id=a.id, day=day(1), field="actual", raw="6")
    filled = svc.fill_actuals_from_plan(db, MON, today=day(2))
    assert filled == 2  # Mon and Wed; Tue already had actual time, Thu/Fri are future
    assert svc.build_week(db, MON).rows[0].actual == [480, 360, 480, 0, 0, 0, 0]


# --------------------------------------------------------------------------- #
#  Saved timesheets
# --------------------------------------------------------------------------- #
def test_save_timesheet_locks_and_snapshots(db):
    a = svc.add_project(db, "Alpha", "A1")
    b = svc.add_project(db, "Beta")
    svc.save_cell(db, project_id=a.id, day=day(0), field="actual", raw="5")
    svc.save_cell(db, project_id=a.id, day=day(2), field="actual", raw="3.25")
    svc.save_cell(db, project_id=b.id, day=day(0), field="planned", raw="2")  # plan only: not on the sheet
    svc.save_week_note(db, MON, "Good week")

    sheet = svc.save_timesheet(db, MON)
    assert sheet.total == 495
    assert [(l.project_name, l.total) for l in sheet.lines] == [("Alpha", 495)]
    assert sheet.note == "Good week"

    with pytest.raises(svc.LockedError):
        svc.save_cell(db, project_id=a.id, day=day(1), field="actual", raw="1")
    with pytest.raises(svc.LockedError):
        svc.save_week_note(db, MON, "edit")
    with pytest.raises(ValueError, match="already saved"):
        svc.save_timesheet(db, MON)

    # Renaming the project later doesn't change the record.
    svc.update_project(db, a.id, "Alpha Renamed", "A1", "")
    doc = svc.timesheet_document(db, MON)
    assert doc.record is not None and doc.lines[0].name == "Alpha"
    assert doc.total == 495

    svc.reopen_timesheet(db, MON)
    assert not svc.is_locked(db, MON)
    svc.save_cell(db, project_id=a.id, day=day(1), field="actual", raw="1")


def test_cannot_save_empty_week(db):
    svc.add_project(db, "A")
    with pytest.raises(ValueError, match="nothing to save"):
        svc.save_timesheet(db, MON)


def test_unsaved_weeks_listed(db):
    a = svc.add_project(db, "A")
    svc.save_cell(db, project_id=a.id, day=day(0), field="actual", raw="2")
    svc.save_cell(db, project_id=a.id, day=day(7), field="actual", raw="3")
    svc.save_timesheet(db, MON)
    assert svc.unsaved_weeks(db) == [(day(7), 180)]


# --------------------------------------------------------------------------- #
#  Reports
# --------------------------------------------------------------------------- #
def _two_weeks(db):
    a = svc.add_project(db, "Alpha", "A1")
    b = svc.add_project(db, "Beta", "B2")
    for i in range(5):
        svc.save_cell(db, project_id=a.id, day=day(i), field="actual", raw="6")
        svc.save_cell(db, project_id=b.id, day=day(i), field="actual", raw="2")
    svc.save_timesheet(db, MON)  # week 1 saved: 30 h Alpha + 10 h Beta
    for i in range(7, 12):
        svc.save_cell(db, project_id=a.id, day=day(i), field="actual", raw="8")  # week 2 unsaved: 40 h
    return a, b


def test_report_totals_and_partial_weeks(db):
    _two_weeks(db)
    report = svc.build_report(db, day(3), day(8))  # Thu wk1 .. Tue wk2
    assert report.total == (6 + 2) * 2 * 60 + 8 * 2 * 60  # Thu+Fri of week 1, Mon+Tue of week 2
    assert [(p.name, p.minutes) for p in report.projects] == [("Alpha", 1680), ("Beta", 240)]
    assert [w.partial for w in report.weeks] == [True, True]
    assert [w.saved for w in report.weeks] == [True, False]
    assert report.includes_unsaved and report.unsaved_minutes == 960
    assert round(sum(p.share for p in report.projects), 6) == 100


def test_report_saved_only(db):
    _two_weeks(db)
    report = svc.build_report(db, MON, day(13), saved_only=True)
    assert report.total == 40 * 60
    assert not report.includes_unsaved
    assert report.unsaved_minutes == 40 * 60  # reported as left out
    assert report.days_worked == 5 and report.avg_per_day == "8"


def test_report_uses_snapshot_lines_and_current_project_name(db):
    a, _ = _two_weeks(db)
    svc.update_project(db, a.id, "Alpha Renamed", "A1", "")
    report = svc.build_report(db, MON, day(13))
    assert report.projects[0].name == "Alpha Renamed"  # one line per project, current name
    saved_lines = [line for line in report.lines if line.saved and line.code == "A1"]
    assert saved_lines and all(line.name == "Alpha" for line in saved_lines)  # record kept as saved


def test_report_swaps_reversed_range_and_saved_totals(db):
    _two_weeks(db)
    report = svc.build_report(db, day(13), MON)
    assert report.start == MON and report.end == day(13)
    totals = svc.saved_totals(db)
    assert totals is not None and totals.total == 40 * 60


def test_quick_ranges():
    ranges = dict((name, (s, e)) for name, s, e in svc.quick_ranges(date(2026, 3, 18)))
    assert ranges["This week"] == (date(2026, 3, 16), date(2026, 3, 22))
    assert ranges["Last month"] == (date(2026, 2, 1), date(2026, 2, 28))
    assert ranges["This month"] == (date(2026, 3, 1), date(2026, 3, 31))


# --------------------------------------------------------------------------- #
#  Settings & backups
# --------------------------------------------------------------------------- #
def test_settings(db):
    svc.save_settings(db, user_name=" Dirk ", company="ES", daily_target_text="7:30", weekend=True)
    s = svc.get_settings(db)
    assert s["user_name"] == "Dirk" and svc.daily_target(s) == 450 and svc.show_weekend(s)
    with pytest.raises(ValueError):
        svc.save_settings(db, user_name="", company="", daily_target_text="0", weekend=False)


def test_backups(app, db, tmp_path):
    svc.add_project(db, "A")
    folder = tmp_path / "bk"
    first = svc.make_backup(app.state.db_path, folder, keep=2)
    svc.make_backup(app.state.db_path, folder, keep=2)
    svc.make_backup(app.state.db_path, folder, keep=2)
    backups = svc.list_backups(folder)
    assert len(backups) == 2
    assert not first.exists()  # the oldest was pruned
    assert svc.backup_if_due(app.state.db_path, folder) is None  # a fresh backup exists


# --------------------------------------------------------------------------- #
#  Round 2: robustness fixes
# --------------------------------------------------------------------------- #
def test_tiny_value_never_silently_clears_a_cell(db):
    a = svc.add_project(db, "A")
    svc.save_cell(db, project_id=a.id, day=day(0), field="actual", raw="10")
    with pytest.raises(ValueError, match="15 minutes"):
        svc.save_cell(db, project_id=a.id, day=day(0), field="actual", raw="0.1")
    assert svc.build_week(db, MON).rows[0].actual[0] == 600


def test_out_of_range_dates_are_invalid():
    assert svc.parse_date("9999-12-31") is None
    assert svc.parse_date("0001-01-01") is None
    assert svc.parse_date("2999-12-31") == date(2999, 12, 31)
    assert svc.parse_week("9999-12-31") == svc.monday_of(date.today())


def test_concurrent_saves_of_one_cell_do_not_fail(app):
    """Plan and actual typed back to back hit the same new row from two threads."""
    import threading

    setup = app.state.SessionLocal()
    pid = svc.add_project(setup, "Race").id
    setup.close()
    errors: list[Exception] = []

    def worker(field: str, value: str) -> None:
        session = app.state.SessionLocal()
        try:
            svc.save_cell(session, project_id=pid, day=day(3), field=field, raw=value)
        except Exception as exc:  # noqa: BLE001 - collected and asserted below
            errors.append(exc)
        finally:
            session.close()

    threads = [threading.Thread(target=worker, args=(("planned", "2") if i % 2 else ("actual", "3")))
               for i in range(20)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert not errors
    check = app.state.SessionLocal()
    row = svc.build_week(check, MON).rows[0]
    assert (row.planned[3], row.actual[3]) == (120, 180)
    check.close()


def test_matrix_weeks_and_months(db):
    _two_weeks(db)
    report = svc.build_report(db, day(3), day(8))
    m = report.matrix
    assert m.unit == "week" and [c.label for c in m.columns] == ["19–22 Nov", "23–24 Nov"]
    assert all(c.partial for c in m.columns)
    assert [(r.project.name, r.cells, r.total) for r in m.rows] == [("Alpha", [720, 960], 1680), ("Beta", [240, 0], 240)]
    assert m.column_totals == [960, 960] and m.total == report.total
    long = svc.build_report(db, date(2026, 1, 1), date(2026, 12, 31))
    assert long.matrix.unit == "month" and len(long.matrix.columns) == 12
    assert long.matrix.columns[10].label == "Nov 2026" and long.matrix.total == long.total
    assert len(long.active_weeks) == 2 and long.empty_weeks == len(long.weeks) - 2


def test_renamed_project_shows_recorded_name(db):
    a, _ = _two_weeks(db)
    svc.update_project(db, a.id, "Alpha Renamed", "A1", "")
    report = svc.build_report(db, MON, day(13))
    assert report.projects[0].display == "Alpha Renamed (recorded as Alpha)"
    assert svc.build_report(db, day(7), day(13)).projects[0].display == "Alpha Renamed"  # unsaved week only


def test_restore_backup_replaces_live_data(app, db, tmp_path):
    a = svc.add_project(db, "Before")
    svc.save_cell(db, project_id=a.id, day=day(0), field="actual", raw="4")
    snapshot = svc.make_backup(app.state.db_path, app.state.backup_dir)
    svc.add_project(db, "After")
    db.close()
    app.state.engine.dispose()
    safety = svc.restore_backup(app.state.db_path, app.state.backup_dir, snapshot.name)
    fresh = app.state.SessionLocal()
    assert [s.project.name for s in svc.project_overview(fresh)] == ["Before"]
    fresh.close()
    assert safety.exists() and safety != snapshot
    with pytest.raises(ValueError, match="no longer exists"):
        svc.restore_backup(app.state.db_path, app.state.backup_dir, "../../etc/passwd")
    junk = app.state.backup_dir / f"{svc.BACKUP_PREFIX}junk.db"
    junk.write_bytes(b"not a database at all" * 100)
    with pytest.raises(ValueError):
        svc.restore_backup(app.state.db_path, app.state.backup_dir, junk.name)


def test_database_is_a_single_self_contained_file(app, db):
    from sqlalchemy import text

    assert db.execute(text("PRAGMA journal_mode")).scalar() == "delete"
    svc.add_project(db, "A")
    assert not (app.state.db_path.parent / (app.state.db_path.name + "-wal")).exists()


# --------------------------------------------------------------------------- #
#  Round 3: revisions and labels
# --------------------------------------------------------------------------- #
def test_reopen_keeps_saved_version_as_revision(db):
    a = svc.add_project(db, "Alpha", "A1")
    svc.save_cell(db, project_id=a.id, day=day(0), field="actual", raw="5")
    svc.save_timesheet(db, MON)
    svc.update_project(db, a.id, "Alpha Renamed", "A1", "")
    svc.reopen_timesheet(db, MON)
    svc.save_cell(db, project_id=a.id, day=day(1), field="actual", raw="2")
    svc.save_timesheet(db, MON)
    history = svc.revisions(db, MON)
    assert len(history) == 1 and history[0].total == 300
    lines = svc.revision_lines(history[0])
    assert [(l.name, l.total) for l in lines] == [("Alpha", 300)]  # the original name survives
    assert svc.get_timesheet(db, MON).total == 420


def test_range_labels():
    assert svc._range_label(date(2026, 12, 31), date(2027, 1, 3)) == "31 Dec–3 Jan"
    assert svc._range_label(date(2026, 11, 18), date(2026, 11, 22)) == "18–22 Nov"
    assert svc._range_label(date(2026, 1, 5), date(2026, 1, 5)) == "5 Jan"


def test_restore_backup_from_older_version_upgrades_schema(app, db):
    """A backup taken before timesheet_revisions existed must work right after restore."""
    import sqlite3

    a = svc.add_project(db, "Old")
    svc.save_cell(db, project_id=a.id, day=day(0), field="actual", raw="3")
    svc.save_timesheet(db, MON)
    old = svc.make_backup(app.state.db_path, app.state.backup_dir)
    con = sqlite3.connect(old)
    con.execute("DROP TABLE timesheet_revisions")
    con.commit()
    con.close()
    db.close()
    app.state.engine.dispose()
    svc.restore_backup(app.state.db_path, app.state.backup_dir, old.name)
    fresh = app.state.SessionLocal()
    svc.reopen_timesheet(fresh, MON)  # needs the revisions table
    assert len(svc.revisions(fresh, MON)) == 1
    fresh.close()
