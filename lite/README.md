# Tiny Time Track Lite

A personal weekly planner and timesheet for **one person on one computer** — no
login, no manager, no server to look after. Plan your week, record the time you
actually spent, keep each finished week as a permanent timesheet record, and print
or export totals for any period.

It is a separate, much smaller sibling of the full **Tiny Time Track** office app
(in the parent folder), which stays unchanged.

## What it does

| | |
|---|---|
| **Projects** | Your project list (name, optional code and client). Archive finished projects; projects with recorded time can't be deleted, so records stay complete. |
| **Weekly planning** | A Mon–Fri grid (weekends optional) with a **Plan** row and an **Actual** row per project. Type `7.5`, `7,5` or `7:30`; everything saves automatically. Notes & goals per week. *Copy last week's plan* and *Actual = plan* save typing. |
| **Printable plan** | *Print plan* gives a clean A4 document of the week's planned hours and notes — print it or save it as PDF. |
| **Timesheet records** | *Save timesheet* keeps the week's actual hours as a permanent record and makes the week read-only (you can reopen it — the saved version is kept in the history, so no record is ever lost). Records keep project names as they were, even if you rename a project later. Each one prints as a signed-off timesheet. |
| **Totals per project** | The *Timesheets* page totals all saved time per project, and lists weeks with time you haven't saved yet. |
| **Period reports** | Choose any date range (or *This month*, *Last month*, …) for a **totalled-out timesheet**: projects down the side, weeks across (months for periods over 14 weeks), totals both ways. Print / PDF, **Excel** (live `SUM` totals, print-ready) or **CSV** (totals grid or daily detail). Optionally *saved timesheets only*. |
| **Your data is safe** | One SQLite file on this computer. A backup is made automatically each day you use the app (newest 30 kept), plus *Back up now* and one-click *Restore*. |

## Install (once)

1. Install **Python 3.11 or newer** from <https://www.python.org/downloads/> — tick
   *Add python.exe to PATH*.
2. In this `lite` folder, right-click **`setup.ps1`** → *Run with PowerShell*.
   It creates a private `.venv` and installs the libraries (about a minute).

## Start

Double-click **`Start Tiny Time Track Lite.bat`**. The app opens in your browser at
<http://127.0.0.1:8100>. Keep the small black window open while you work; close it to
stop the app. Starting it again while it's running just reopens the browser.

Tip: right-click the `.bat` → *Send to → Desktop (create shortcut)* for a desktop icon.

From a terminal you can also run `python -m ttlite` with these options:

```
--port 8101          use another port (default 8100, or TTLITE_PORT)
--data-dir D:\Time   keep the data somewhere else (or TTLITE_DATA_DIR)
--no-browser         don't open a browser window
```

## Using it

1. **Projects** → add your projects (or use *+ Add a project* under the week grid).
2. **Week** → fill in the **Plan** row at the start of the week; *Print plan* if you
   want it on paper. Arrow keys / Enter move between cells, Esc undoes a cell.
3. During or at the end of the week, fill in the **Actual** row. Day totals turn
   green at your daily target (Settings, default 8 h), amber below and red above.
4. When the week is done, press **Save timesheet**. It's now a record.
5. **Reports** → pick a period → *Print / PDF*, *Excel* or *CSV*. If a project was
   renamed after its timesheets were saved, reports show e.g.
   “Alpha Renamed (recorded as Alpha)”.

To make a PDF, use *Print / Save as PDF* and choose **Save as PDF** or
**Microsoft Print to PDF** as the printer.

## Where your data lives

| | |
|---|---|
| Data folder | `%LOCALAPPDATA%\TinyTimeTrackLite` (e.g. `C:\Users\you\AppData\Local\TinyTimeTrackLite`) |
| Database | `tinytimetrack-lite.db` in that folder |
| Backups | `backups\tinytimetrack-lite-YYYYMMDD-HHMMSS.db` |

*Settings → Open data folder* opens it for you. Updating or moving the program
folder never touches your data.

**Restore a backup:** *Settings → Recent backups → Restore*. The app first saves a
safety backup of your current data, so a restore can itself be undone.
(By hand also works: close the app, then copy the backup file over
`tinytimetrack-lite.db` in the data folder. The database is always a single
self-contained file, so there are no extra journal files to worry about.)

**Move to a new computer:** copy the whole data folder to the same place on the new
computer (after installing the app there).

## Privacy & security

There's no password because only you use this computer. The app listens **only on
this computer** (127.0.0.1) — nothing on the network can reach it — and it refuses
requests coming from other websites, so a web page open in your browser can't
change your data. Nothing is ever sent to the internet.

## For developers

```
pip install -r requirements-dev.txt
python -m pytest            # from this folder
```

- `ttlite/services.py` — all business rules (weeks, hours, saved timesheets, reports, backups)
- `ttlite/web.py` — FastAPI routes and the local-only guard; `ttlite/exports.py` — Excel/CSV
- `ttlite/models.py` — SQLite tables. Time is stored as **whole minutes** (quarter-hour
  steps), so all totals are exact.
- No JavaScript libraries: `static/app.js` (autosave, totals, keyboard), `static/*.css`.

## Lite vs. the full app

| Full Tiny Time Track | Lite |
|---|---|
| Office server, many users, logins and roles | One person, one computer, no login |
| Manager dashboards, salaries, profitability, reviews, deadlines, email reminders | Not included |
| Plan/actual weekly grid | ✓ (plus notes, quarter-hour entry, weekends) |
| — | Printable plan, saved timesheet records, period reports (print/PDF, Excel, CSV) |

© 2026 Dirk Cilliers. All rights reserved — see `../LICENSE`.
