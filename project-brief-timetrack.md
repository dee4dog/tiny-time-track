# Project Brief: ES TimeTrack — Office Time & Profitability App

## 1. Overview

Build a self-hosted web application for an architectural office (ES team, Hermanus, South Africa) that replaces an Excel-based weekly time-planning workbook. The app tracks **planned hours, actual hours, and overtime** per employee per project per week, and gives managers **project profitability** based on man-hour cost-to-company versus project fees.

The app runs **entirely on a local company server** (Windows), is accessed by staff via browser on the office LAN, and sends **email reminders** for timesheet completion. No cloud dependencies for core function.

**Origin:** The office currently plans time in a weekly Excel grid (projects as rows, Monday–Friday as columns, 1–8 hours logged per day as a count, not time-of-day). This is a **from-scratch build** — no data migration from the workbook is required — but the app's timesheet must preserve that familiar weekly-grid mental model so adoption is effortless.

## 2. Users and Roles

| Role | Access |
|---|---|
| **Employee** | Own timesheets only. Fill in planned hours (start of week) and actual hours + overtime (end of week). No visibility of salaries, rates, fees, or other people's data. |
| **Manager** | Everything employees see, plus: all employees' timesheets, salary/rate table, project fees, profitability dashboard, and the Settings interface. |

Authentication: username (email) + password, hashed with bcrypt/argon2. Server-side session or JWT. **Every API endpoint enforces role checks** — salary and fee data must never be obtainable by an employee role, even via direct API calls. (POPIA: salary data is personal information.)

## 3. Technology Stack

- **Backend:** Python 3.11+, FastAPI, SQLAlchemy, SQLite (single-file DB at a configurable path, e.g. `D:\TimeTrack\timetrack.db`)
- **Frontend:** Server-rendered HTML (Jinja2) with light vanilla JS or htmx for interactivity. No build pipeline, no Node toolchain. Must work in Edge/Chrome.
- **Scheduler:** APScheduler inside the app process for email reminders.
- **Email:** SMTP, configurable in Settings (host, port, TLS, username, password, from-address). Default assumption: Microsoft 365 (`smtp.office365.com:587`).
- **Deployment:** Runs as a Windows service (use `nssm` or a documented Task Scheduler "run at startup" task). Listens on `0.0.0.0:8000`. Provide an install README with exact steps.
- **Backups:** Nightly job copies the SQLite file to a configurable backup folder, keeping 30 days.

Keep dependencies minimal. The maintainer is a competent Python scripter (Dynamo background), not a professional web developer — favour readable, well-commented code over cleverness.

## 4. Data Model

### employees
| Field | Type | Notes |
|---|---|---|
| id | int PK | |
| name | text | e.g. "Dirk" |
| email | text unique | login + reminder address |
| role | enum | `employee` / `manager` |
| annual_salary | decimal | **sensitive** — manager-visible only |
| available_hours_per_year | int | default 1760 (after leave + SA public holidays) |
| active | bool | deactivate instead of delete (history preserved) |
| password_hash | text | |

**Derived hourly cost-to-company** = `(annual_salary × overhead_multiplier) / available_hours_per_year`, where `overhead_multiplier` is a global setting (default 1.6). Compute at query time; also store a **rate history table** (`rate_history`: employee_id, effective_date, annual_salary) so past weeks are costed at the salary in effect at the time, not today's.

### projects
| Field | Type | Notes |
|---|---|---|
| id | int PK | |
| leader | text | e.g. "ES" |
| name | text | e.g. "Longlands Clubhouse" |
| number | text | e.g. "3676", "C-1000" (text — supports prefixes) |
| fee | decimal | total professional fee (ZAR), editable over time |
| status | enum | `active` / `on_hold` / `complete` / `archived` |
| billable | bool | false for admin/library entries like "Revit Library", "Vergaderings/algemeen" |

Display format everywhere: **`Leader | Project | No.`** (e.g. `ES | Longlands Clubhouse | 3676`).

Optional (phase 2): `project_stages` table (stage name, % of fee) for SACAP work-stage fee splits.

### time_entries
| Field | Type | Notes |
|---|---|---|
| id | int PK | |
| employee_id | FK | |
| project_id | FK | |
| week_start | date | always a Monday |
| day | int 0–4 | Mon–Fri |
| planned_hours | decimal | 0–12, 0.5 steps |
| actual_hours | decimal | 0–12, 0.5 steps |
| overtime_hours | decimal | typed value, can exceed 8 |
| note | text | optional task description (mirrors the "Beskrywing"/task labels in Excel) |

Unique constraint on (employee_id, project_id, week_start, day).

### week_status
| Field | Type | Notes |
|---|---|---|
| employee_id + week_start | composite PK | |
| planned_submitted_at | datetime nullable | set when employee clicks "Submit plan" |
| actuals_submitted_at | datetime nullable | set when employee clicks "Submit actuals" |

Drives reminder logic and manager compliance view.

### employee_projects
Join table: which projects appear on each employee's weekly grid (manager-assignable in Settings; employees may also add an active project to their own grid).

### settings (key–value)
overhead_multiplier, smtp_* fields, reminder schedule (cron-like: day-of-week + time for "plan" and "actuals" reminders, plus follow-up), week start convention, currency symbol (R), backup path, company name/logo path.

### audit_log
(timestamp, user_id, action, detail) for salary changes, fee changes, and settings changes.

## 5. Business Rules

1. **Cost of an hour** for employee E in week W = salary effective in W × overhead_multiplier ÷ available_hours_per_year.
2. **Project cost to date** = Σ over all employees of (actual_hours + overtime_hours × overtime_factor) × that employee's rate. `overtime_factor` is a setting, default 1.5 (BCEA convention); manager can set 1.0 if overtime isn't paid extra.
3. **Profit** = fee − cost to date. **Margin %** = profit ÷ fee. Non-billable projects show cost only (overhead view), no profit line.
4. **Planned vs actual variance** per project and per employee per week.
5. A week locks for employee editing 7 days after `actuals_submitted_at` (managers can unlock).
6. All money in ZAR, formatted `R 1 234 567`. Negative profit shown in red with parentheses.

## 6. Interfaces (three)

### 6.1 Employee Timesheet
- After login, lands on **current week** (Mon–Fri grid). Week navigation: ‹ prev | week picker | next ›.
- Layout mirrors the Excel sheet: rows = the employee's assigned projects (shown as `ES | Longlands Clubhouse | 3676`), columns = Mon–Fri. Each cell: planned hours and actual hours (two small inputs or a toggle between "Plan" and "Actual" mode for the week), plus a per-day overtime input per project, plus an optional note per cell.
- Quick entry is critical: tab/arrow-key navigation between cells, 0.5-hour steps, a per-day column total that turns **green at 8.0, amber below, red above 8** (excluding overtime).
- Buttons: **Submit plan** (Monday) and **Submit actuals** (Friday). Submitting stamps `week_status` and stops reminders. Resubmission allowed until lock.
- "Copy last week's plan" button.
- Employee can add any *active* project to their grid from a dropdown; cannot see fees or rates anywhere.

### 6.2 Manager Dashboard
Tabs/sections:

1. **Projects (default):** table of active projects — Fee | Hours to date | Cost to date | Profit | Margin % | last-4-weeks burn sparkline. Sortable; click a project → detail page: per-employee hours/cost breakdown, weekly burn chart (cumulative cost vs fee line — the "underwater" early-warning view), planned-vs-actual chart, and an editable fee field (audited).
2. **People:** each employee — capacity (available hrs), planned, actual, overtime, utilisation %, and submission compliance for recent weeks (✓/✗ plan, ✓/✗ actuals). Salary/rate table behind a "Rates" sub-tab with an extra confirmation click.
3. **This week:** live compliance board — who has/hasn't submitted plan and actuals; one-click "send reminder now" per person.
4. **Export:** any view to .xlsx (use openpyxl) — keep the Excel escape hatch for the accountant.

Charts: simple, no heavy libraries — Chart.js from a locally bundled file (no CDN; the server may be offline).

### 6.3 Settings (manager only)
- **Employees:** add/edit/deactivate, set salary (writes rate_history), role, assigned projects, reset password.
- **Projects:** add/edit/archive, fee, billable flag, leader, number.
- **Reminders:** schedule for plan reminder (default Mon 08:00), actuals reminder (default Fri 15:00), follow-up (default next Mon 09:00 to non-submitters only); editable email templates with placeholders `{name}`, `{week}`, `{link}`; SMTP config with a "Send test email" button.
- **Globals:** overhead multiplier, overtime factor, available hours default, backup folder, company branding.
- **Data:** backup-now button, audit log viewer.

Design language for all three: clean, neutral, generous whitespace, system font stack, single accent colour, fully usable on a laptop screen; the timesheet grid must also work acceptably on a phone (employees may submit actuals from site).

## 7. Email Reminders

- APScheduler jobs fire per the Settings schedule (Africa/Johannesburg timezone).
- Plan reminder → all active employees without `planned_submitted_at` for the current week. Actuals reminder → all without `actuals_submitted_at`. Follow-up → still-missing only.
- Email contains a direct link to that week's timesheet (`http://{server}/week/{date}`).
- Log every send in audit_log; failures retried once, then surfaced on the manager "This week" board.

## 8. Non-Functional Requirements

- Office size ≤ 25 users; performance is a non-issue, simplicity wins every trade-off.
- All pages load without internet access (bundle fonts/JS locally).
- Language: UI in English; field naming may mirror existing Afrikaans terms in tooltips where helpful (e.g. "Beskrywing = task description").
- Config via a single `.env`/`config.toml`; secrets (SMTP password) never in code.
- Provide: README (install, service setup, backup/restore, adding the first manager account via CLI command), and seed script creating demo data (the four known projects: ES | Longlands Clubhouse | 3676; ES | Chekkers | 3785; ES | Van Rijn Meent | 3671; Revit Library | C-1000 non-billable; plus a "General / Admin" non-billable project).

## 9. Build Phases (each independently usable)

1. **Foundation:** schema, auth, roles, settings storage, CLI to create first manager, 
2. **Employee timesheet:** weekly grid, submit flow, copy-last-week.
3. **Manager dashboard:** projects table, project detail with burn chart, people view.
4. **Settings UI:** employees, projects, globals, SMTP test.
5. **Reminders:** scheduler + templates + compliance board.
6. **Polish & deploy:** xlsx export, backups, audit viewer, Windows service docs.

## 10. Acceptance Criteria (sample)

- An employee can plan a week in under 2 minutes and never sees money data (verified at API level, not just UI).
- A manager sees correct profit for a project where two employees with different salaries logged hours across two weeks spanning a salary change (rate_history respected).
- Reminder emails go only to non-submitters and stop after submission.
- Deleting nothing: deactivating an employee or archiving a project preserves all historical numbers.
- Server reboot → app and scheduler come back automatically; nightly backup file appears.
