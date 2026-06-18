# ES TimeTrack

Self-hosted office time tracking and project profitability for the ES team.
Runs entirely on a local Windows server — no cloud dependencies.

> **Build status:** Complete — all six phases built and verified.
> **Phase 1 (Foundation):** database schema, login & role enforcement, settings
> storage, admin CLI, config.
> **Phase 2 (Employee timesheet):** Excel-style Mon–Fri grid with Plan/Actual
> mode toggle, per-cell autosave, overtime & notes, live day-total colour
> coding (green = 8h, amber < 8, red > 8), keyboard navigation, submit
> plan/actuals, copy-last-week, and add-project.
> **Phase 3 (Manager dashboard):** projects profitability table (fee / hours /
> cost / profit / margin with burn sparkline), project detail with cost-burn-vs-fee
> and planned-vs-actual charts (Chart.js bundled locally) and an audited fee
> editor, People view (capacity / utilisation / 6-week compliance), a
> confirmation-gated Rates table, and a This-week submission board. Cost uses
> each person's salary in effect that week (rate history) with overtime
> weighting.
> **Phase 4 (Settings):** manager-only settings — employee add/edit/deactivate
> (salary changes write rate_history + audit), assigned-project management,
> password reset; project add/edit/archive; global settings (overhead, overtime
> factor, available-hours default, branding); SMTP config with a "send test
> email" button; and an audit-log viewer.
> **Phase 5 (Reminders):** an in-process APScheduler (Africa/Johannesburg) runs
> plan / actuals / follow-up reminder jobs that email only non-submitters, using
> editable templates with `{name}`/`{week}`/`{link}` placeholders; the schedule
> is editable from Settings → Reminders and reapplied live; managers can send a
> one-click reminder from the This-week board; every send is logged and failures
> surface on the board.
> **Phase 6 (Polish & deploy):** `.xlsx` export of the Projects and People views
> (openpyxl), a nightly SQLite online-backup job with 30-day retention plus a
> Settings → Data "back up now" button and backup list, and the Windows-service /
> backup-restore / firewall documentation below.

---

## Requirements

- **Python 3.11 or newer** (the current stack needs modern SQLAlchemy/FastAPI).
  Check with `python --version`. If you have 3.6/3.7, install a newer Python
  from <https://www.python.org/downloads/> first.
- Windows 10/11 or Windows Server. Works on any OS for development.

## Quick start (development)

```powershell
# 1. From the project folder, create and activate a virtual environment
python -m venv .venv
.\.venv\Scripts\Activate.ps1

# 2. Install dependencies
pip install -r requirements.txt

# 3. Create your configuration
copy .env.example .env
#    Then edit .env and set a real TIMETRACK_SECRET_KEY:
python -c "import secrets; print(secrets.token_hex(32))"

# 4. Create the first manager account (you'll be prompted for a password)
python -m app.cli create-manager --name "Dirk" --email dirk@es.co.za

# 5. (Optional) load demo projects + sample logins to explore
python -m app.cli seed

# 6. Run the server
python -m app
```

Open <http://localhost:8000> and sign in. On the LAN, other staff use
`http://<server-name-or-ip>:8000`.

### Demo logins (only after `seed`)

| Role     | Email              | Password      |
| -------- | ------------------ | ------------- |
| Manager  | `manager@es.local` | `changeme123` |
| Employee | `anel@es.local`    | `changeme123` |

**Delete or change these before real use.**

## Admin CLI

```powershell
python -m app.cli init-db                       # create tables
python -m app.cli create-manager --name N --email E [--salary 780000]
python -m app.cli create-employee --name N --email E [--salary 480000]
python -m app.cli set-password --email E         # reset a password
python -m app.cli seed                           # demo data
```

## Configuration

All bootstrap config lives in `.env` (see `.env.example`). Business settings
that managers change at runtime (overhead multiplier, overtime factor, SMTP,
reminder schedule) are stored in the database and edited from the Settings
screen — they are **not** in `.env`.

| `.env` key              | Default                | Purpose                              |
| ----------------------- | ---------------------- | ------------------------------------ |
| `TIMETRACK_DB_PATH`     | `timetrack.db`         | SQLite file location                 |
| `TIMETRACK_SECRET_KEY`  | *(change me)*          | Signs the session cookie             |
| `TIMETRACK_HOST`        | `0.0.0.0`              | Bind address                         |
| `TIMETRACK_PORT`        | `8000`                 | Port                                 |
| `TIMETRACK_TIMEZONE`    | `Africa/Johannesburg`  | Scheduler / week handling            |
| `TIMETRACK_BACKUP_PATH` | `backups`              | Nightly backup folder (Phase 6)      |

## Project layout

```
app/
  config.py          bootstrap settings from .env
  database.py        SQLAlchemy engine, session, table creation
  models.py          all database tables (the data model)
  security.py        Argon2 password hashing
  settings_store.py  key-value business settings + defaults
  dependencies.py    auth + role enforcement (get_current_user / require_manager)
  templating.py      shared Jinja2 environment + money formatter
  main.py            FastAPI app wiring, session cookie, error handlers
  cli.py             admin command line (create-manager, seed, ...)
  seed.py            demo projects + sample staff
  routers/           auth, pages, api route modules
  templates/         Jinja2 HTML
  static/            css, js (htmx bundled locally)
```

## Roles & data privacy

Two roles: **employee** (own timesheets only) and **manager** (everything,
plus salaries, fees, profitability, and settings). Role checks are enforced
on the server for every route — salary and fee data are never serialized for
an employee-role request, even via the API. Salary data is personal
information under POPIA; treat the database file accordingly.

## Exports

Managers can export the **Projects** and **People** views to `.xlsx` from the
buttons on those pages (money written as real numbers with a ZAR format, so the
accountant can sum and filter). Files: `/manager/export/projects.xlsx`,
`/manager/export/people.xlsx`.

## Backups

A nightly job copies the SQLite database to the backup folder
(`TIMETRACK_BACKUP_PATH`, default `backups/`), keeping the last **30 days**.
It uses SQLite's online-backup API, so the snapshot is consistent even while the
app is running. Configure the time (and run an on-demand backup) under
**Settings → Data**.

**Restore:**
1. Stop the app (stop the Windows service, see below).
2. Copy the chosen `timetrack-YYYYMMDD-HHMMSS.db` over your live database file
   (the path in `TIMETRACK_DB_PATH`), replacing it.
3. Start the app again.

## Running as a Windows service

The app must keep running and restart on reboot. Two documented options:

### Option A — nssm (recommended)

[nssm](https://nssm.cc/) wraps any program as a proper Windows service.

```powershell
# 1. Download nssm and put nssm.exe somewhere on PATH.
# 2. Install the service (run PowerShell as Administrator):
nssm install ESTimeTrack "C:\Claud dev\timetrack\.venv\Scripts\python.exe" "-m" "app"
nssm set ESTimeTrack AppDirectory "C:\Claud dev\timetrack"
nssm set ESTimeTrack Start SERVICE_AUTO_START
nssm set ESTimeTrack AppStdout "C:\Claud dev\timetrack\logs\service.log"
nssm set ESTimeTrack AppStderr "C:\Claud dev\timetrack\logs\service.log"

# 3. Start it
nssm start ESTimeTrack
```

Manage with `nssm restart ESTimeTrack`, `nssm stop ESTimeTrack`, or the
Services snap-in (`services.msc`). On reboot it starts automatically, and the
APScheduler reminders + nightly backup come back with it.

### Option B — Task Scheduler (no extra software)

1. Open **Task Scheduler → Create Task**.
2. **General:** name `ESTimeTrack`; "Run whether user is logged on or not";
   "Run with highest privileges".
3. **Triggers:** New → "At startup".
4. **Actions:** New → Start a program:
   - Program: `C:\Claud dev\timetrack\.venv\Scripts\python.exe`
   - Arguments: `-m app`
   - Start in: `C:\Claud dev\timetrack`
5. **Settings:** tick "If the task fails, restart every 1 minute".

To verify after a reboot, browse to `http://<server>:8000` from another PC.

### Firewall

Allow inbound TCP 8000 on the office LAN so staff can reach the server:

```powershell
New-NetFirewallRule -DisplayName "ES TimeTrack 8000" -Direction Inbound `
  -Protocol TCP -LocalPort 8000 -Action Allow -Profile Domain,Private
```
