# Tiny Time Track Lite (Rust)

A personal weekly planner and timesheet for **one person on one computer**. It's a
single native Windows program (about 5 MB): no sign-in, no browser, no local server,
and nothing to install.

## What it does

| | |
|---|---|
| **Week timeline** | Each day has a **Plan** lane and an **Actual** lane, split into half hours. Drag across empty time to add a block, drag an edge to extend or shrink it, and drag the middle to move it. A click adds half an hour. Right-click for options. Ctrl+Z / Ctrl+Y undo and redo. |
| **Working hours** | Green lines mark the start and end of each work day, and lunch is shaded. The defaults are Mon–Thu 08:00–17:00 with lunch 14:00–14:45, and Fri 08:00–16:00 with lunch 13:00–14:00. You can change them per day in *Settings*. Day totals are checked against the hours worked (lunch excluded). |
| **To do per project** | Each project gets a to-do list per week. Anything not ticked off carries over to the next week. |
| **Project stages** | Each project can be set to a SACAP work stage: 1 Inception, 2 Concept & Viability, 3 Design Development, 4 Documentation & Procurement, 5 Construction, 6 Close-out. |
| **RFIs** | Projects on site (stage 5) keep a Request for Information register: numbered, with due dates, open/answered/overdue status and a printable PDF register. |
| **Timesheets** | *Save timesheet* keeps the week as a permanent record and locks it. Reopening a week keeps the saved version as a revision. |
| **Reports** | Any date range, totalled per project by week or month. Export as PDF, Excel (live `SUM` totals) or CSV. |
| **Your data** | Everything is in one file: `%LOCALAPPDATA%\TinyTimeTrackLite\data.json`. Saves are atomic. A backup is made each day the app is used (the newest 30 are kept), and *Settings* has *Back up now* and *Restore*. Exports go to `Documents\Tiny Time Track`. |

## Build and run

Building needs Rust (<https://rustup.rs>).

```powershell
cargo build --release
.\target\release\tiny-time-track.exe
```

The program is `target\release\tiny-time-track.exe`. Copy it anywhere and double-click it.
Only one copy of the app can run at a time.

## Development

```powershell
cargo test --features dev
cargo run --features dev -- --demo --tab week
```

The `dev` feature adds sample data (`--demo`) and screenshot options. It is never part of
release builds. Set `TTT_DATA_DIR` to use a separate data folder while testing.
