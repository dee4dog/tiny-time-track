//! Data model and every business rule. No UI code lives here, so it is all
//! unit-testable.
//!
//! Time is recorded as *blocks* on a day timeline, in half-hour slots:
//! slot 0 = 00:00-00:30 ... slot 47 = 23:30-24:00. A block covers
//! `[start, end)` and blocks in the same lane never overlap, so a day can
//! never hold more than 24 hours. Each day has a **Plan** lane and an
//! **Actual** lane. Saving a week turns its actual blocks into a
//! [`Timesheet`]: a permanent snapshot that keeps project names as they were.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, TimeDelta};
use serde::{Deserialize, Serialize};

pub const SLOTS: u8 = 48;
pub const DAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
pub const DATA_VERSION: u32 = 1;
pub const PALETTE: [[u8; 3]; 12] = [
    [0xC2, 0x67, 0x4A], // terracotta
    [0x4F, 0x8A, 0x6E], // sage
    [0xC9, 0x93, 0x30], // ochre
    [0x5B, 0x74, 0xA8], // slate blue
    [0x9C, 0x52, 0x78], // plum
    [0x3E, 0x8E, 0x8E], // teal
    [0xA8, 0x73, 0x5C], // clay
    [0x6E, 0x8E, 0x2E], // olive
    [0xC2, 0x6A, 0x8C], // rose
    [0x82, 0x64, 0xA8], // lavender
    [0x56, 0x72, 0x80], // steel
    [0xC8, 0x7A, 0x2C], // amber
];
pub const LOCKED: &str = "This week is saved as a timesheet and is read-only. Reopen it to make changes.";
const MAX_REPORT_DAYS: i64 = 3660;
const MATRIX_MAX_WEEKS: i64 = 14;

// --------------------------------------------------------------------------
//  Small helpers
// --------------------------------------------------------------------------
pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

pub fn now() -> NaiveDateTime {
    let t = Local::now().naive_local();
    t - TimeDelta::nanoseconds(t.and_utc().timestamp_subsec_nanos() as i64)
}

pub fn add_days(d: NaiveDate, n: i64) -> NaiveDate {
    d + TimeDelta::days(n)
}

pub fn monday_of(d: NaiveDate) -> NaiveDate {
    add_days(d, -(d.weekday().num_days_from_monday() as i64))
}

pub fn week_dates(start: NaiveDate) -> [NaiveDate; 7] {
    std::array::from_fn(|i| add_days(start, i as i64))
}

/// Half-hours as compact hours text: 16 -> "8", 15 -> "7.5", 0 -> "0".
pub fn fmt_h(half_hours: u32) -> String {
    if half_hours % 2 == 0 { (half_hours / 2).to_string() } else { format!("{}.5", half_hours / 2) }
}

/// Slot boundary as a clock time: 15 -> "07:30", 48 -> "24:00".
pub fn slot_time(slot: u8) -> String {
    format!("{:02}:{:02}", slot / 2, (slot % 2) * 30)
}

pub fn day_month(d: NaiveDate) -> String {
    format!("{} {}", d.day(), d.format("%b"))
}

pub fn long_date(d: NaiveDate) -> String {
    format!("{} {}", d.day(), d.format("%b %Y"))
}

pub fn date_time(t: NaiveDateTime) -> String {
    format!("{} {}", t.date().day(), t.format("%b %Y, %H:%M"))
}

pub fn period_label(start: NaiveDate, end: NaiveDate) -> String {
    if start == end {
        long_date(start)
    } else if start.year() != end.year() {
        format!("{} – {}", long_date(start), long_date(end))
    } else if start.month() != end.month() {
        format!("{} – {}", day_month(start), long_date(end))
    } else {
        format!("{} – {}", start.day(), long_date(end))
    }
}

pub fn week_label(start: NaiveDate) -> String {
    period_label(start, add_days(start, 6))
}

fn range_label(first: NaiveDate, last: NaiveDate) -> String {
    if first == last {
        day_month(first)
    } else if first.month() == last.month() {
        format!("{}–{}", first.day(), day_month(last))
    } else {
        format!("{}–{}", day_month(first), day_month(last))
    }
}

pub fn month_bounds(d: NaiveDate) -> (NaiveDate, NaiveDate) {
    let first = d.with_day(1).expect("day 1 exists");
    let next = add_days(first, 32).with_day(1).expect("day 1 exists");
    (first, add_days(next, -1))
}

pub fn quick_ranges(today: NaiveDate) -> Vec<(&'static str, NaiveDate, NaiveDate)> {
    let monday = monday_of(today);
    let this_month = month_bounds(today);
    let last_month = month_bounds(add_days(this_month.0, -1));
    let year = today.year();
    let ymd = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).expect("valid date");
    vec![
        ("This week", monday, add_days(monday, 6)),
        ("Last week", add_days(monday, -7), add_days(monday, -1)),
        ("This month", this_month.0, this_month.1),
        ("Last month", last_month.0, last_month.1),
        ("This year", ymd(year, 1, 1), ymd(year, 12, 31)),
        ("Last year", ymd(year - 1, 1, 1), ymd(year - 1, 12, 31)),
    ]
}

fn clean(text: &str, limit: usize) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(limit).collect()
}

// --------------------------------------------------------------------------
//  Stored types
// --------------------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Lane {
    Plan,
    Actual,
}

impl Lane {
    pub fn label(self) -> &'static str {
        match self {
            Lane::Plan => "Plan",
            Lane::Actual => "Actual",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub client: String,
    #[serde(default)]
    pub color: u8,
    #[serde(default = "yes")]
    pub active: bool,
    /// SACAP work stage 1-6 (see [`STAGES`]); `None` until set.
    #[serde(default)]
    pub stage: Option<u8>,
}

fn yes() -> bool {
    true
}

/// The six SACAP (South African Council for the Architectural Profession) work stages.
pub const STAGES: [&str; 6] = [
    "Inception",
    "Concept & Viability",
    "Design Development",
    "Documentation & Procurement",
    "Construction",
    "Close-out",
];
/// Stage 5 - the project is on site, so RFIs apply.
pub const ON_SITE_STAGE: u8 = 5;

pub fn stage_label(stage: Option<u8>) -> String {
    match stage {
        Some(s @ 1..=6) => format!("Stage {s} · {}", STAGES[usize::from(s - 1)]),
        _ => "Stage not set".into(),
    }
}

/// A to-do for a project. It appears from the week it was added in every later
/// week until it's ticked off; once ticked it stays in that week only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Todo {
    pub id: u64,
    pub project: u32,
    pub text: String,
    /// Monday of the week it was added.
    pub week: NaiveDate,
    /// Monday of the week it was ticked off.
    #[serde(default)]
    pub done: Option<NaiveDate>,
}

/// A Request for Information raised while a project is on site.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rfi {
    pub id: u64,
    pub project: u32,
    /// Per-project running number: RFI 1, 2, 3 ...
    pub number: u32,
    pub subject: String,
    #[serde(default)]
    pub details: String,
    pub raised: NaiveDate,
    #[serde(default)]
    pub due: Option<NaiveDate>,
    #[serde(default)]
    pub answered: Option<NaiveDate>,
    #[serde(default)]
    pub response: String,
}

impl Rfi {
    pub fn is_open(&self) -> bool {
        self.answered.is_none()
    }
    pub fn is_overdue(&self, today: NaiveDate) -> bool {
        self.is_open() && self.due.is_some_and(|d| d < today)
    }
    pub fn status(&self, today: NaiveDate) -> &'static str {
        if !self.is_open() {
            "Answered"
        } else if self.is_overdue(today) {
            "Overdue"
        } else {
            "Open"
        }
    }
}

impl Project {
    pub fn label(&self) -> String {
        if self.code.is_empty() { self.name.clone() } else { format!("{} ({})", self.name, self.code) }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub id: u64,
    pub date: NaiveDate,
    pub lane: Lane,
    pub project: u32,
    pub start: u8,
    pub end: u8,
}

impl Block {
    pub fn len(&self) -> u32 {
        u32::from(self.end - self.start)
    }
}

/// A block as it was when a timesheet was saved (names frozen).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapBlock {
    pub date: NaiveDate,
    pub start: u8,
    pub end: u8,
    pub project: Option<u32>,
    pub name: String,
    pub code: String,
    pub color: u8,
}

/// One project's half-hours per weekday (Mon..Sun) in a timesheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetLine {
    pub project: Option<u32>,
    pub name: String,
    pub code: String,
    pub slots: [u16; 7],
}

impl SheetLine {
    pub fn total(&self) -> u32 {
        self.slots.iter().map(|&s| u32::from(s)).sum()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Timesheet {
    pub week: NaiveDate,
    pub saved_at: NaiveDateTime,
    #[serde(default)]
    pub note: String,
    pub lines: Vec<SheetLine>,
    #[serde(default)]
    pub blocks: Vec<SnapBlock>,
}

impl Timesheet {
    pub fn total(&self) -> u32 {
        self.lines.iter().map(SheetLine::total).sum()
    }
}

/// A saved timesheet that was later reopened - kept so no record is lost.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Revision {
    pub sheet: Timesheet,
    pub reopened_at: NaiveDateTime,
}

/// A day's working hours, in minutes from midnight (480 = 08:00).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayHours {
    pub start: u16,
    pub end: u16,
    #[serde(default)]
    pub lunch: Option<(u16, u16)>,
}

impl DayHours {
    pub const fn new(start: u16, end: u16, lunch: Option<(u16, u16)>) -> Self {
        DayHours { start, end, lunch }
    }

    /// Minutes actually working: the day minus lunch.
    pub fn working_minutes(&self) -> u16 {
        let lunch = self.lunch.map_or(0, |(s, e)| e.saturating_sub(s));
        self.end.saturating_sub(self.start).saturating_sub(lunch)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.start >= self.end || self.end > 24 * 60 {
            return Err("The working day must end after it starts.".into());
        }
        if let Some((s, e)) = self.lunch {
            if s >= e || s < self.start || e > self.end {
                return Err("Lunch must fall inside the working day and end after it starts.".into());
            }
        }
        Ok(())
    }
}

/// Mon-Thu 08:00-17:00 with lunch 14:00-14:45; Fri 08:00-16:00 with lunch 13:00-14:00.
pub fn default_week_hours() -> [Option<DayHours>; 7] {
    let mon_thu = Some(DayHours::new(8 * 60, 17 * 60, Some((14 * 60, 14 * 60 + 45))));
    let fri = Some(DayHours::new(8 * 60, 16 * 60, Some((13 * 60, 14 * 60))));
    [mon_thu, mon_thu, mon_thu, mon_thu, fri, None, None]
}

/// Minutes from midnight as a clock time: 885 -> "14:45".
pub fn clock(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub user_name: String,
    pub company: String,
    /// Working hours per weekday (Mon..Sun); `None` = not a working day.
    /// They draw the work lines on the timeline and set each day's target.
    pub hours: [Option<DayHours>; 7],
    pub show_weekend: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { user_name: String::new(), company: String::new(), hours: default_week_hours(), show_weekend: false }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Data {
    pub version: u32,
    pub settings: Settings,
    pub projects: Vec<Project>,
    pub blocks: Vec<Block>,
    pub notes: BTreeMap<NaiveDate, String>,
    pub timesheets: BTreeMap<NaiveDate, Timesheet>,
    pub revisions: Vec<Revision>,
    pub todos: Vec<Todo>,
    pub rfis: Vec<Rfi>,
    pub next_id: u64,
}

impl Default for Data {
    fn default() -> Self {
        Data {
            version: DATA_VERSION,
            settings: Settings::default(),
            projects: Vec::new(),
            blocks: Vec::new(),
            notes: BTreeMap::new(),
            timesheets: BTreeMap::new(),
            revisions: Vec::new(),
            todos: Vec::new(),
            rfis: Vec::new(),
            next_id: 1,
        }
    }
}

// --------------------------------------------------------------------------
//  Derived (view) types
// --------------------------------------------------------------------------
#[derive(Clone, Debug, PartialEq)]
pub struct SummaryRow {
    pub project: u32,
    pub plan: [u32; 7],
    pub actual: [u32; 7],
}

impl SummaryRow {
    pub fn plan_total(&self) -> u32 {
        self.plan.iter().sum()
    }
    pub fn actual_total(&self) -> u32 {
        self.actual.iter().sum()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    Id(u32),
    Name(String, String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReportLine {
    pub day: NaiveDate,
    pub project: Option<u32>,
    pub code: String,
    pub name: String,
    pub hh: u32,
    pub saved: bool,
}

impl ReportLine {
    pub fn key(&self) -> Key {
        match self.project {
            Some(id) => Key::Id(id),
            None => Key::Name(self.code.clone(), self.name.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReportProject {
    pub key: Key,
    pub code: String,
    pub name: String,
    pub color: u8,
    pub hh: u32,
    pub share: f64,
    pub recorded_as: BTreeSet<String>,
}

impl ReportProject {
    pub fn display(&self) -> String {
        if self.recorded_as.is_empty() {
            self.name.clone()
        } else {
            let names: Vec<&str> = self.recorded_as.iter().map(String::as_str).collect();
            format!("{} (recorded as {})", self.name, names.join(", "))
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReportWeek {
    pub start: NaiveDate,
    pub hh: u32,
    pub saved: bool,
    pub partial: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatrixColumn {
    pub label: String,
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub partial: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Matrix {
    pub unit: &'static str,
    pub columns: Vec<MatrixColumn>,
    /// One row per entry in `Report::projects`, same order.
    pub cells: Vec<Vec<u32>>,
}

impl Matrix {
    pub fn column_totals(&self) -> Vec<u32> {
        (0..self.columns.len()).map(|c| self.cells.iter().map(|row| row[c]).sum()).collect()
    }
    pub fn has_partial(&self) -> bool {
        self.columns.iter().any(|c| c.partial)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub saved_only: bool,
    pub lines: Vec<ReportLine>,
    pub projects: Vec<ReportProject>,
    pub weeks: Vec<ReportWeek>,
    pub unsaved_hh: u32,
    pub matrix: Matrix,
}

impl Report {
    pub fn total(&self) -> u32 {
        self.lines.iter().map(|l| l.hh).sum()
    }
    pub fn label(&self) -> String {
        period_label(self.start, self.end)
    }
    pub fn days_worked(&self) -> usize {
        self.lines.iter().map(|l| l.day).collect::<BTreeSet<_>>().len()
    }
    pub fn avg_per_day(&self) -> String {
        let days = self.days_worked();
        if days == 0 {
            return "0".into();
        }
        let avg = self.total() as f64 / 2.0 / days as f64;
        let text = format!("{avg:.2}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
    pub fn includes_unsaved(&self) -> bool {
        self.lines.iter().any(|l| !l.saved)
    }
    pub fn active_weeks(&self) -> Vec<&ReportWeek> {
        self.weeks.iter().filter(|w| w.hh > 0 || w.saved).collect()
    }
}

// --------------------------------------------------------------------------
//  Drag maths (pure, used by the timeline widget)
// --------------------------------------------------------------------------
/// Span covered while painting from `anchor` to `current`, kept inside `free`.
pub fn create_span(anchor: u8, current: u8, free: (u8, u8)) -> (u8, u8) {
    let current = current.clamp(free.0, free.1.saturating_sub(1));
    (anchor.min(current), anchor.max(current) + 1)
}

/// Move one edge of `span` to `boundary`, keeping at least one slot and staying inside `free`.
pub fn resize_span(span: (u8, u8), start_edge: bool, boundary: u8, free: (u8, u8)) -> (u8, u8) {
    if start_edge {
        (boundary.clamp(free.0, span.1 - 1), span.1)
    } else {
        (span.0, boundary.clamp(span.0 + 1, free.1))
    }
}

/// Slide `span` so it starts at `new_start`, keeping its length and staying inside `free`.
pub fn move_span(span: (u8, u8), new_start: i32, free: (u8, u8)) -> (u8, u8) {
    let len = i32::from(span.1 - span.0);
    let start = new_start.clamp(i32::from(free.0), i32::from(free.1) - len) as u8;
    (start, start + len as u8)
}

// --------------------------------------------------------------------------
//  Rules
// --------------------------------------------------------------------------
impl Data {
    fn new_id(&mut self) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    // ---- projects -----------------------------------------------------------
    pub fn project(&self, id: u32) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }

    /// Active first, then alphabetical.
    pub fn projects_sorted(&self) -> Vec<&Project> {
        let mut list: Vec<&Project> = self.projects.iter().collect();
        list.sort_by_key(|p| (!p.active, p.name.to_lowercase()));
        list
    }

    pub fn active_projects(&self) -> Vec<&Project> {
        self.projects_sorted().into_iter().filter(|p| p.active).collect()
    }

    fn check_project(&self, name: &str, code: &str, exclude: Option<u32>) -> Result<(), String> {
        if name.is_empty() {
            return Err("Please give the project a name.".into());
        }
        let clash = self.projects.iter().any(|p| {
            Some(p.id) != exclude && p.name.to_lowercase() == name.to_lowercase() && p.code.to_lowercase() == code.to_lowercase()
        });
        if clash {
            let label = if code.is_empty() { name.to_string() } else { format!("{name} ({code})") };
            return Err(format!("You already have a project called “{label}”."));
        }
        Ok(())
    }

    pub fn add_project(&mut self, name: &str, code: &str, client: &str) -> Result<u32, String> {
        let (name, code, client) = (clean(name, 120), clean(code, 30), clean(client, 120));
        self.check_project(&name, &code, None)?;
        let id = self.projects.iter().map(|p| p.id).max().unwrap_or(0) + 1;
        let used: Vec<u8> = self.projects.iter().filter(|p| p.active).map(|p| p.color).collect();
        let color = (0..PALETTE.len() as u8)
            .find(|c| !used.contains(c))
            .unwrap_or((self.projects.len() % PALETTE.len()) as u8);
        self.projects.push(Project { id, name, code, client, color, active: true, stage: None });
        Ok(id)
    }

    pub fn set_project_stage(&mut self, id: u32, stage: Option<u8>) -> Result<(), String> {
        if stage.is_some_and(|s| !(1..=6).contains(&s)) {
            return Err("Stages run from 1 to 6.".into());
        }
        let p = self.projects.iter_mut().find(|p| p.id == id).ok_or("That project no longer exists.")?;
        p.stage = stage;
        Ok(())
    }

    pub fn is_on_site(&self, id: u32) -> bool {
        self.project(id).is_some_and(|p| p.stage == Some(ON_SITE_STAGE))
    }

    // ---- to-dos -----------------------------------------------------------------
    /// The project's to-dos for `week`: added that week or earlier and not
    /// ticked off before it (unfinished ones carry over), oldest first.
    pub fn todos_for(&self, project: u32, week: NaiveDate) -> Vec<&Todo> {
        let mut list: Vec<&Todo> = self
            .todos
            .iter()
            .filter(|t| t.project == project && t.week <= week && t.done.is_none_or(|d| d >= week))
            .collect();
        list.sort_by_key(|t| (t.week, t.id));
        list
    }

    pub fn add_todo(&mut self, project: u32, week: NaiveDate, text: &str) -> Result<u64, String> {
        let text = clean(text, 300);
        if text.is_empty() {
            return Err("Type what needs doing first.".into());
        }
        if self.project(project).is_none() {
            return Err("That project no longer exists.".into());
        }
        let id = self.new_id();
        self.todos.push(Todo { id, project, text, week: monday_of(week), done: None });
        Ok(id)
    }

    /// Tick off in `week`, or untick if it was ticked off in that week.
    pub fn toggle_todo(&mut self, id: u64, week: NaiveDate) -> Result<bool, String> {
        let todo = self.todos.iter_mut().find(|t| t.id == id).ok_or("That to-do no longer exists.")?;
        let week = monday_of(week);
        todo.done = if todo.done == Some(week) { None } else { Some(week) };
        Ok(todo.done.is_some())
    }

    pub fn edit_todo(&mut self, id: u64, text: &str) -> Result<(), String> {
        let text = clean(text, 300);
        if text.is_empty() {
            return Err("A to-do can't be empty — delete it instead.".into());
        }
        self.todos.iter_mut().find(|t| t.id == id).ok_or("That to-do no longer exists.")?.text = text;
        Ok(())
    }

    pub fn delete_todo(&mut self, id: u64) {
        self.todos.retain(|t| t.id != id);
    }

    // ---- RFIs ---------------------------------------------------------------------
    pub fn rfis_for(&self, project: u32) -> Vec<&Rfi> {
        let mut list: Vec<&Rfi> = self.rfis.iter().filter(|r| r.project == project).collect();
        list.sort_by_key(|r| r.number);
        list
    }

    /// (open, overdue) RFI counts for a project.
    pub fn rfi_counts(&self, project: u32, today: NaiveDate) -> (usize, usize) {
        let list = self.rfis_for(project);
        (list.iter().filter(|r| r.is_open()).count(), list.iter().filter(|r| r.is_overdue(today)).count())
    }

    pub fn add_rfi(
        &mut self,
        project: u32,
        subject: &str,
        details: &str,
        raised: NaiveDate,
        due: Option<NaiveDate>,
    ) -> Result<u32, String> {
        let subject = clean(subject, 200);
        if subject.is_empty() {
            return Err("Give the RFI a subject.".into());
        }
        if !self.is_on_site(project) {
            return Err("RFIs are for projects on site (Stage 5 · Construction).".into());
        }
        if due.is_some_and(|d| d < raised) {
            return Err("The due date can't be before the date it was raised.".into());
        }
        let number = self.rfis_for(project).iter().map(|r| r.number).max().unwrap_or(0) + 1;
        let id = self.new_id();
        let details: String = details.trim().chars().take(4000).collect();
        self.rfis.push(Rfi { id, project, number, subject, details, raised, due, answered: None, response: String::new() });
        Ok(number)
    }

    pub fn rfi_mut(&mut self, id: u64) -> Option<&mut Rfi> {
        self.rfis.iter_mut().find(|r| r.id == id)
    }

    pub fn answer_rfi(&mut self, id: u64, on: NaiveDate, response: &str) -> Result<(), String> {
        let rfi = self.rfi_mut(id).ok_or("That RFI no longer exists.")?;
        if on < rfi.raised {
            return Err("It can't be answered before it was raised.".into());
        }
        rfi.answered = Some(on);
        rfi.response = response.trim().chars().take(4000).collect();
        Ok(())
    }

    pub fn reopen_rfi(&mut self, id: u64) {
        if let Some(rfi) = self.rfi_mut(id) {
            rfi.answered = None;
        }
    }

    pub fn delete_rfi(&mut self, id: u64) {
        self.rfis.retain(|r| r.id != id);
    }

    pub fn update_project(&mut self, id: u32, name: &str, code: &str, client: &str) -> Result<(), String> {
        let (name, code, client) = (clean(name, 120), clean(code, 30), clean(client, 120));
        self.check_project(&name, &code, Some(id))?;
        let p = self.projects.iter_mut().find(|p| p.id == id).ok_or("That project no longer exists.")?;
        p.name = name;
        p.code = code;
        p.client = client;
        Ok(())
    }

    pub fn set_project_active(&mut self, id: u32, active: bool) -> Result<(), String> {
        let p = self.projects.iter_mut().find(|p| p.id == id).ok_or("That project no longer exists.")?;
        p.active = active;
        Ok(())
    }

    pub fn set_project_color(&mut self, id: u32, color: u8) {
        if let Some(p) = self.projects.iter_mut().find(|p| p.id == id) {
            p.color = color % PALETTE.len() as u8;
        }
    }

    /// Has any time (plan, actual or in a saved record) ever been put on the project?
    pub fn project_in_use(&self, id: u32) -> bool {
        self.blocks.iter().any(|b| b.project == id)
            || self.todos.iter().any(|t| t.project == id)
            || self.rfis.iter().any(|r| r.project == id)
            || self.timesheets.values().chain(self.revisions.iter().map(|r| &r.sheet)).any(|t| {
                t.lines.iter().any(|l| l.project == Some(id)) || t.blocks.iter().any(|b| b.project == Some(id))
            })
    }

    pub fn delete_project(&mut self, id: u32) -> Result<String, String> {
        let name = self.project(id).ok_or("That project no longer exists.")?.name.clone();
        if self.project_in_use(id) {
            return Err(format!("“{name}” has time recorded, so it can't be deleted — archive it instead."));
        }
        self.projects.retain(|p| p.id != id);
        Ok(name)
    }

    /// Planned, actual and saved half-hours ever recorded per project.
    pub fn project_totals(&self) -> HashMap<u32, (u32, u32, u32)> {
        let mut totals: HashMap<u32, (u32, u32, u32)> = HashMap::new();
        for b in &self.blocks {
            let t = totals.entry(b.project).or_default();
            match b.lane {
                Lane::Plan => t.0 += b.len(),
                Lane::Actual => t.1 += b.len(),
            }
        }
        for sheet in self.timesheets.values() {
            for line in &sheet.lines {
                if let Some(id) = line.project {
                    totals.entry(id).or_default().2 += line.total();
                }
            }
        }
        totals
    }

    // ---- locking --------------------------------------------------------------
    pub fn is_locked(&self, week: NaiveDate) -> bool {
        self.timesheets.contains_key(&week)
    }

    fn require_unlocked(&self, date: NaiveDate) -> Result<(), String> {
        if self.is_locked(monday_of(date)) { Err(LOCKED.into()) } else { Ok(()) }
    }

    // ---- blocks ---------------------------------------------------------------
    pub fn block(&self, id: u64) -> Option<&Block> {
        self.blocks.iter().find(|b| b.id == id)
    }

    pub fn lane_blocks(&self, date: NaiveDate, lane: Lane) -> Vec<&Block> {
        let mut list: Vec<&Block> = self.blocks.iter().filter(|b| b.date == date && b.lane == lane).collect();
        list.sort_by_key(|b| b.start);
        list
    }

    /// The largest free stretch `[lo, hi)` around `slot`, ignoring block `exclude`.
    /// `None` when `slot` is inside another block.
    pub fn free_interval(&self, date: NaiveDate, lane: Lane, slot: u8, exclude: Option<u64>) -> Option<(u8, u8)> {
        let (mut lo, mut hi) = (0u8, SLOTS);
        for b in self.lane_blocks(date, lane) {
            if Some(b.id) == exclude {
                continue;
            }
            if b.start <= slot && slot < b.end {
                return None;
            }
            if b.end <= slot {
                lo = lo.max(b.end);
            } else if b.start > slot {
                hi = hi.min(b.start);
            }
        }
        Some((lo, hi))
    }

    fn overlaps(&self, date: NaiveDate, lane: Lane, start: u8, end: u8, exclude: Option<u64>) -> bool {
        self.blocks
            .iter()
            .any(|b| Some(b.id) != exclude && b.date == date && b.lane == lane && b.start < end && start < b.end)
    }

    pub fn add_block(&mut self, date: NaiveDate, lane: Lane, project: u32, start: u8, end: u8) -> Result<u64, String> {
        self.require_unlocked(date)?;
        if self.project(project).is_none() {
            return Err("That project no longer exists.".into());
        }
        if start >= end || end > SLOTS {
            return Err("A block needs at least half an hour.".into());
        }
        if self.overlaps(date, lane, start, end, None) {
            return Err("That time is already taken.".into());
        }
        let id = self.new_id();
        self.blocks.push(Block { id, date, lane, project, start, end });
        Ok(id)
    }

    pub fn set_block_span(&mut self, id: u64, start: u8, end: u8) -> Result<(), String> {
        let b = self.block(id).ok_or("That block no longer exists.")?.clone();
        self.require_unlocked(b.date)?;
        if start >= end || end > SLOTS {
            return Err("A block needs at least half an hour.".into());
        }
        if self.overlaps(b.date, b.lane, start, end, Some(id)) {
            return Err("That time is already taken.".into());
        }
        let b = self.blocks.iter_mut().find(|b| b.id == id).expect("checked above");
        b.start = start;
        b.end = end;
        Ok(())
    }

    pub fn set_block_project(&mut self, id: u64, project: u32) -> Result<(), String> {
        let date = self.block(id).ok_or("That block no longer exists.")?.date;
        self.require_unlocked(date)?;
        if self.project(project).is_none() {
            return Err("That project no longer exists.".into());
        }
        self.blocks.iter_mut().find(|b| b.id == id).expect("checked above").project = project;
        Ok(())
    }

    pub fn delete_block(&mut self, id: u64) -> Result<(), String> {
        let date = self.block(id).ok_or("That block no longer exists.")?.date;
        self.require_unlocked(date)?;
        self.blocks.retain(|b| b.id != id);
        Ok(())
    }

    /// Copy a plan block into the actual lane, filling whatever part of it is free.
    pub fn copy_block_to_actual(&mut self, id: u64) -> Result<u32, String> {
        let b = self.block(id).ok_or("That block no longer exists.")?.clone();
        self.require_unlocked(b.date)?;
        let mut copied = 0;
        let mut slot = b.start;
        while slot < b.end {
            match self.free_interval(b.date, Lane::Actual, slot, None) {
                Some((_, hi)) => {
                    let end = hi.min(b.end);
                    self.add_block(b.date, Lane::Actual, b.project, slot, end)?;
                    copied += u32::from(end - slot);
                    slot = end;
                }
                None => slot += 1,
            }
        }
        self.merge_lane(b.date, Lane::Actual);
        Ok(copied)
    }

    /// Join touching blocks of the same project into one.
    pub fn merge_lane(&mut self, date: NaiveDate, lane: Lane) {
        let mut ids: Vec<(u64, u8, u8, u32)> =
            self.lane_blocks(date, lane).iter().map(|b| (b.id, b.start, b.end, b.project)).collect();
        ids.sort_by_key(|x| x.1);
        let mut remove = Vec::new();
        let mut i = 0;
        while i + 1 < ids.len() {
            if ids[i].2 == ids[i + 1].1 && ids[i].3 == ids[i + 1].3 {
                ids[i].2 = ids[i + 1].2;
                remove.push(ids[i + 1].0);
                ids.remove(i + 1);
            } else {
                i += 1;
            }
        }
        for (id, _, end, _) in &ids {
            if let Some(b) = self.blocks.iter_mut().find(|b| b.id == *id) {
                b.end = *end;
            }
        }
        self.blocks.retain(|b| !remove.contains(&b.id));
    }

    pub fn week_blocks(&self, week: NaiveDate) -> Vec<Block> {
        let end = add_days(week, 6);
        self.blocks.iter().filter(|b| b.date >= week && b.date <= end).cloned().collect()
    }

    /// Put back a week's blocks exactly as given (used by undo/redo).
    pub fn replace_week_blocks(&mut self, week: NaiveDate, blocks: Vec<Block>) {
        let end = add_days(week, 6);
        self.blocks.retain(|b| b.date < week || b.date > end);
        self.blocks.extend(blocks);
    }

    pub fn day_total(&self, date: NaiveDate, lane: Lane) -> u32 {
        self.blocks.iter().filter(|b| b.date == date && b.lane == lane).map(Block::len).sum()
    }

    pub fn day_total_week(&self, week: NaiveDate, lane: Lane) -> u32 {
        week_dates(week).iter().map(|d| self.day_total(*d, lane)).sum()
    }

    /// Per-project plan/actual half-hours for the week (projects with time only).
    pub fn week_summary(&self, week: NaiveDate) -> Vec<SummaryRow> {
        let mut rows: BTreeMap<u32, SummaryRow> = BTreeMap::new();
        for b in self.week_blocks(week) {
            let day = (b.date - week).num_days() as usize;
            let row = rows.entry(b.project).or_insert(SummaryRow { project: b.project, plan: [0; 7], actual: [0; 7] });
            match b.lane {
                Lane::Plan => row.plan[day] += b.len(),
                Lane::Actual => row.actual[day] += b.len(),
            }
        }
        let mut list: Vec<SummaryRow> = rows.into_values().collect();
        list.sort_by_key(|r| self.project(r.project).map(|p| p.name.to_lowercase()).unwrap_or_default());
        list
    }

    /// Day indexes (0 = Mon) to show: weekdays, plus the weekend if wanted,
    /// used, or set up with working hours.
    pub fn visible_days(&self, week: NaiveDate) -> Vec<usize> {
        let weekend_used = self.week_blocks(week).iter().any(|b| b.date >= add_days(week, 5));
        let weekend_hours = self.settings.hours[5].is_some() || self.settings.hours[6].is_some();
        if self.settings.show_weekend || weekend_used || weekend_hours { (0..7).collect() } else { (0..5).collect() }
    }

    pub fn day_hours(&self, date: NaiveDate) -> Option<DayHours> {
        self.settings.hours[date.weekday().num_days_from_monday() as usize]
    }

    /// Set (or clear, with `None`) a weekday's working hours.
    pub fn set_day_hours(&mut self, day: usize, hours: Option<DayHours>) -> Result<(), String> {
        if day > 6 {
            return Err("No such day.".into());
        }
        if let Some(h) = hours {
            h.validate()?;
        }
        self.settings.hours[day] = hours;
        Ok(())
    }

    /// Timeline range: an hour either side of the working hours, widened for
    /// any blocks outside them.
    pub fn timeline_range<'a>(&self, extra: impl Iterator<Item = (u8, u8)> + 'a) -> (u8, u8) {
        let days = self.settings.hours.iter().flatten();
        let start = days.clone().map(|h| h.start).min().unwrap_or(8 * 60);
        let end = days.map(|h| h.end).max().unwrap_or(17 * 60);
        let mut lo = (start / 30).saturating_sub(2) as u8;
        let mut hi = (end.div_ceil(30) + 2).min(u16::from(SLOTS)) as u8;
        for (s, e) in extra {
            lo = lo.min(s);
            hi = hi.max(e);
        }
        (lo, hi.max(lo + 2))
    }

    /// Timeline range for a week.
    pub fn visible_slots(&self, week: NaiveDate) -> (u8, u8) {
        let blocks = self.week_blocks(week);
        self.timeline_range(blocks.iter().map(|b| (b.start, b.end)))
    }

    /// Copy last week's plan into this week's free plan time (active projects only).
    pub fn copy_last_week_plan(&mut self, week: NaiveDate) -> Result<usize, String> {
        if self.is_locked(week) {
            return Err(LOCKED.into());
        }
        let previous: Vec<Block> = self
            .week_blocks(add_days(week, -7))
            .into_iter()
            .filter(|b| b.lane == Lane::Plan && self.project(b.project).is_some_and(|p| p.active))
            .collect();
        let mut copied = 0;
        for b in previous {
            let date = add_days(b.date, 7);
            if !self.overlaps(date, Lane::Plan, b.start, b.end, None) {
                self.add_block(date, Lane::Plan, b.project, b.start, b.end)?;
                copied += 1;
            }
        }
        for date in week_dates(week) {
            self.merge_lane(date, Lane::Plan);
        }
        Ok(copied)
    }

    /// For days up to `today` with nothing in Actual yet, copy the plan into Actual.
    pub fn actual_from_plan(&mut self, week: NaiveDate, today: NaiveDate) -> Result<usize, String> {
        if self.is_locked(week) {
            return Err(LOCKED.into());
        }
        let mut days = 0;
        for date in week_dates(week) {
            if date > today || self.day_total(date, Lane::Actual) > 0 {
                continue;
            }
            let plan: Vec<Block> = self.lane_blocks(date, Lane::Plan).into_iter().cloned().collect();
            if plan.is_empty() {
                continue;
            }
            for b in plan {
                self.add_block(date, Lane::Actual, b.project, b.start, b.end)?;
            }
            days += 1;
        }
        Ok(days)
    }

    // ---- notes ----------------------------------------------------------------
    pub fn note(&self, week: NaiveDate) -> &str {
        self.notes.get(&week).map(String::as_str).unwrap_or("")
    }

    pub fn set_note(&mut self, week: NaiveDate, text: &str) -> Result<(), String> {
        if self.is_locked(week) {
            return Err(LOCKED.into());
        }
        let text: String = text.chars().take(4000).collect();
        if text.trim().is_empty() {
            self.notes.remove(&week);
        } else {
            self.notes.insert(week, text);
        }
        Ok(())
    }

    // ---- timesheets ---------------------------------------------------------------
    /// The week's actual time as a timesheet (not stored).
    pub fn live_sheet(&self, week: NaiveDate) -> Timesheet {
        let mut lines: Vec<SheetLine> = Vec::new();
        let mut blocks = Vec::new();
        let mut actual: Vec<Block> = self.week_blocks(week).into_iter().filter(|b| b.lane == Lane::Actual).collect();
        actual.sort_by_key(|b| (b.date, b.start));
        for b in &actual {
            let (name, code, color) =
                self.project(b.project).map(|p| (p.name.clone(), p.code.clone(), p.color)).unwrap_or(("?".into(), String::new(), 0));
            let day = (b.date - week).num_days() as usize;
            match lines.iter_mut().find(|l| l.project == Some(b.project)) {
                Some(line) => line.slots[day] += b.len() as u16,
                None => {
                    let mut slots = [0u16; 7];
                    slots[day] = b.len() as u16;
                    lines.push(SheetLine { project: Some(b.project), name: name.clone(), code: code.clone(), slots });
                }
            }
            blocks.push(SnapBlock { date: b.date, start: b.start, end: b.end, project: Some(b.project), name, code, color });
        }
        lines.sort_by_key(|l| l.name.to_lowercase());
        Timesheet { week, saved_at: now(), note: self.note(week).to_string(), lines, blocks }
    }

    pub fn save_timesheet(&mut self, week: NaiveDate, at: NaiveDateTime) -> Result<u32, String> {
        if self.is_locked(week) {
            return Err("This week's timesheet is already saved.".into());
        }
        let mut sheet = self.live_sheet(week);
        if sheet.lines.is_empty() {
            return Err("There's no actual time on this week yet, so there's nothing to save.".into());
        }
        sheet.saved_at = at;
        let total = sheet.total();
        self.timesheets.insert(week, sheet);
        Ok(total)
    }

    /// Unlock a saved week; the saved version is kept as a revision.
    pub fn reopen_timesheet(&mut self, week: NaiveDate, at: NaiveDateTime) -> Result<(), String> {
        let sheet = self.timesheets.remove(&week).ok_or("This week has no saved timesheet.")?;
        self.revisions.push(Revision { sheet, reopened_at: at });
        Ok(())
    }

    pub fn revisions_for(&self, week: NaiveDate) -> Vec<&Revision> {
        self.revisions.iter().filter(|r| r.sheet.week == week).collect()
    }

    /// Weeks with actual time but no saved timesheet, newest first.
    pub fn unsaved_weeks(&self) -> Vec<(NaiveDate, u32)> {
        let mut totals: BTreeMap<NaiveDate, u32> = BTreeMap::new();
        for b in self.blocks.iter().filter(|b| b.lane == Lane::Actual) {
            let week = monday_of(b.date);
            if !self.is_locked(week) {
                *totals.entry(week).or_default() += b.len();
            }
        }
        totals.into_iter().rev().collect()
    }

    // ---- reports ------------------------------------------------------------------
    /// Actual time over a period: saved weeks from their records, other weeks
    /// live (marked unsaved) unless `saved_only`.
    pub fn report(&self, start: NaiveDate, end: NaiveDate, saved_only: bool) -> Report {
        let (mut start, end) = if end < start { (end, start) } else { (start, end) };
        if (end - start).num_days() > MAX_REPORT_DAYS {
            start = add_days(end, -MAX_REPORT_DAYS);
        }
        let (first, last) = (monday_of(start), monday_of(end));
        let mut lines = Vec::new();
        for (week, sheet) in self.timesheets.range(first..=last) {
            for line in &sheet.lines {
                for (i, &hh) in line.slots.iter().enumerate() {
                    let day = add_days(*week, i as i64);
                    if hh > 0 && start <= day && day <= end {
                        lines.push(ReportLine {
                            day,
                            project: line.project,
                            code: line.code.clone(),
                            name: line.name.clone(),
                            hh: u32::from(hh),
                            saved: true,
                        });
                    }
                }
            }
        }
        let mut live: BTreeMap<(NaiveDate, u32), u32> = BTreeMap::new();
        for b in &self.blocks {
            if b.lane == Lane::Actual && start <= b.date && b.date <= end && !self.is_locked(monday_of(b.date)) {
                *live.entry((b.date, b.project)).or_default() += b.len();
            }
        }
        let unsaved_hh = live.values().sum();
        if !saved_only {
            for ((day, id), hh) in live {
                let (code, name) = self.project(id).map(|p| (p.code.clone(), p.name.clone())).unwrap_or_default();
                lines.push(ReportLine { day, project: Some(id), code, name, hh, saved: false });
            }
        }
        lines.sort_by(|a, b| (a.day, a.name.to_lowercase()).cmp(&(b.day, b.name.to_lowercase())));

        let mut groups: BTreeMap<Key, ReportProject> = BTreeMap::new();
        for line in &lines {
            let group = groups.entry(line.key()).or_insert_with(|| {
                let current = line.project.and_then(|id| self.project(id));
                ReportProject {
                    key: line.key(),
                    code: current.map(|p| p.code.clone()).unwrap_or_else(|| line.code.clone()),
                    name: current.map(|p| p.name.clone()).unwrap_or_else(|| line.name.clone()),
                    color: current.map(|p| p.color).unwrap_or(10),
                    hh: 0,
                    share: 0.0,
                    recorded_as: BTreeSet::new(),
                }
            });
            if line.name != group.name {
                group.recorded_as.insert(line.name.clone());
            }
            group.hh += line.hh;
        }
        let total: u32 = lines.iter().map(|l| l.hh).sum();
        let mut projects: Vec<ReportProject> = groups.into_values().collect();
        for p in &mut projects {
            p.share = if total > 0 { f64::from(p.hh) * 100.0 / f64::from(total) } else { 0.0 };
        }
        projects.sort_by(|a, b| b.hh.cmp(&a.hh).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));

        let mut per_week: BTreeMap<NaiveDate, u32> = BTreeMap::new();
        for line in &lines {
            *per_week.entry(monday_of(line.day)).or_default() += line.hh;
        }
        let mut weeks = Vec::new();
        let mut week = first;
        while week <= last {
            weeks.push(ReportWeek {
                start: week,
                hh: per_week.get(&week).copied().unwrap_or(0),
                saved: self.is_locked(week),
                partial: week < start || add_days(week, 6) > end,
            });
            week = add_days(week, 7);
        }
        let matrix = build_matrix(start, end, &projects, &lines);
        Report { start, end, saved_only, lines, projects, weeks, unsaved_hh, matrix }
    }
}

fn matrix_columns(start: NaiveDate, end: NaiveDate) -> (&'static str, Vec<MatrixColumn>) {
    let (first, last) = (monday_of(start), monday_of(end));
    let mut columns = Vec::new();
    if (last - first).num_days() / 7 < MATRIX_MAX_WEEKS {
        let mut week = first;
        while week <= last {
            let (s, e) = (week.max(start), add_days(week, 6).min(end));
            let partial = (s, e) != (week, add_days(week, 6));
            let label = if partial { range_label(s, e) } else { day_month(s) };
            columns.push(MatrixColumn { label, start: s, end: e, partial });
            week = add_days(week, 7);
        }
        return ("week", columns);
    }
    let mut month = start.with_day(1).expect("day 1 exists");
    while month <= end {
        let (m_start, m_end) = month_bounds(month);
        let (s, e) = (m_start.max(start), m_end.min(end));
        let partial = (s, e) != (m_start, m_end);
        let label = if partial {
            format!("{} {}", range_label(s, e), month.year())
        } else {
            month.format("%b %Y").to_string()
        };
        columns.push(MatrixColumn { label, start: s, end: e, partial });
        month = add_days(m_end, 1);
    }
    ("month", columns)
}

fn build_matrix(start: NaiveDate, end: NaiveDate, projects: &[ReportProject], lines: &[ReportLine]) -> Matrix {
    let (unit, columns) = matrix_columns(start, end);
    let index: HashMap<&Key, usize> = projects.iter().enumerate().map(|(i, p)| (&p.key, i)).collect();
    let mut cells = vec![vec![0u32; columns.len()]; projects.len()];
    let mut col = 0;
    for line in lines {
        while line.day > columns[col].end {
            col += 1;
        }
        let key = line.key();
        cells[index[&key]][col] += line.hh;
    }
    Matrix { unit, columns, cells }
}

// --------------------------------------------------------------------------
//  Tests
// --------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }
    const MON: (i32, u32, u32) = (2026, 11, 16);
    fn mon() -> NaiveDate {
        d(MON.0, MON.1, MON.2)
    }
    fn at() -> NaiveDateTime {
        mon().and_hms_opt(17, 0, 0).unwrap()
    }

    #[test]
    fn hours_and_labels() {
        assert_eq!(fmt_h(0), "0");
        assert_eq!(fmt_h(15), "7.5");
        assert_eq!(fmt_h(16), "8");
        assert_eq!(slot_time(15), "07:30");
        assert_eq!(slot_time(48), "24:00");
        assert_eq!(week_label(mon()), "16 – 22 Nov 2026");
        assert_eq!(week_label(d(2026, 11, 30)), "30 Nov – 6 Dec 2026");
        assert_eq!(week_label(d(2026, 12, 28)), "28 Dec 2026 – 3 Jan 2027");
        assert_eq!(monday_of(d(2026, 11, 22)), mon());
        assert_eq!(range_label(d(2026, 12, 31), d(2027, 1, 3)), "31 Dec–3 Jan");
    }

    #[test]
    fn drag_maths() {
        assert_eq!(create_span(20, 24, (0, 48)), (20, 25));
        assert_eq!(create_span(20, 16, (18, 48)), (18, 21)); // clamped at a neighbour
        assert_eq!(resize_span((20, 24), false, 30, (0, 28)), (20, 28));
        assert_eq!(resize_span((20, 24), true, 26, (0, 48)), (23, 24)); // keeps half an hour
        assert_eq!(move_span((20, 24), 46, (0, 48)), (44, 48));
        assert_eq!(move_span((20, 24), -3, (10, 48)), (10, 14));
    }

    #[test]
    fn projects_unique_and_deletable_only_when_unused() {
        let mut data = Data::default();
        let a = data.add_project("  Longlands   Clubhouse ", "3676", "").unwrap();
        assert_eq!(data.project(a).unwrap().name, "Longlands Clubhouse");
        assert!(data.add_project("longlands clubhouse", "3676", "").is_err());
        assert!(data.add_project("Longlands Clubhouse", "9999", "").is_ok());
        assert!(data.add_project("   ", "", "").is_err());
        data.add_block(mon(), Lane::Plan, a, 18, 20).unwrap();
        assert!(data.delete_project(a).unwrap_err().contains("archive"));
        let b = data.add_project("Unused", "", "").unwrap();
        assert_eq!(data.delete_project(b).unwrap(), "Unused");
        assert_ne!(data.project(a).unwrap().color, data.projects[1].color);
    }

    #[test]
    fn blocks_never_overlap_and_merge() {
        let mut data = Data::default();
        let a = data.add_project("A", "", "").unwrap();
        let b = data.add_project("B", "", "").unwrap();
        let first = data.add_block(mon(), Lane::Actual, a, 18, 22).unwrap();
        assert!(data.add_block(mon(), Lane::Actual, b, 21, 24).is_err());
        assert_eq!(data.free_interval(mon(), Lane::Actual, 24, None), Some((22, 48)));
        assert_eq!(data.free_interval(mon(), Lane::Actual, 19, None), None);
        assert_eq!(data.free_interval(mon(), Lane::Actual, 19, Some(first)), Some((0, 48)));
        data.add_block(mon(), Lane::Actual, a, 22, 24).unwrap();
        data.merge_lane(mon(), Lane::Actual);
        let lane = data.lane_blocks(mon(), Lane::Actual);
        assert_eq!(lane.len(), 1);
        assert_eq!((lane[0].start, lane[0].end), (18, 24));
        assert_eq!(data.day_total(mon(), Lane::Actual), 6);
        // Plan lane is independent.
        data.add_block(mon(), Lane::Plan, b, 18, 24).unwrap();
        assert!(data.set_block_span(lane_id(&data, Lane::Plan), 10, 30).is_ok());
    }

    fn lane_id(data: &Data, lane: Lane) -> u64 {
        data.lane_blocks(mon(), lane)[0].id
    }

    #[test]
    fn copy_to_actual_fills_only_free_time() {
        let mut data = Data::default();
        let a = data.add_project("A", "", "").unwrap();
        let b = data.add_project("B", "", "").unwrap();
        let plan = data.add_block(mon(), Lane::Plan, a, 16, 24).unwrap();
        data.add_block(mon(), Lane::Actual, b, 18, 20).unwrap();
        assert_eq!(data.copy_block_to_actual(plan).unwrap(), 6);
        assert_eq!(data.day_total(mon(), Lane::Actual), 8);
    }

    #[test]
    fn copy_week_plan_and_actual_from_plan() {
        let mut data = Data::default();
        let a = data.add_project("A", "", "").unwrap();
        data.add_block(add_days(mon(), -7), Lane::Plan, a, 18, 26).unwrap();
        data.add_block(add_days(mon(), -6), Lane::Plan, a, 18, 26).unwrap();
        data.add_block(add_days(mon(), 1), Lane::Plan, a, 20, 22).unwrap(); // clashes with Tue's copy
        assert_eq!(data.copy_last_week_plan(mon()).unwrap(), 1);
        for i in 2..5 {
            data.add_block(add_days(mon(), i), Lane::Plan, a, 18, 26).unwrap();
        }
        data.add_block(add_days(mon(), 1), Lane::Actual, a, 18, 20).unwrap();
        // Up to Wednesday: Mon and Wed get the plan; Tue already has actual time.
        assert_eq!(data.actual_from_plan(mon(), add_days(mon(), 2)).unwrap(), 2);
        assert_eq!(data.day_total(mon(), Lane::Actual), 8);
        assert_eq!(data.day_total(add_days(mon(), 3), Lane::Actual), 0);
    }

    #[test]
    fn save_lock_reopen_keeps_revision_and_old_names() {
        let mut data = Data::default();
        let a = data.add_project("Alpha", "A1", "").unwrap();
        data.add_block(mon(), Lane::Actual, a, 18, 28).unwrap();
        data.add_block(add_days(mon(), 2), Lane::Actual, a, 18, 24).unwrap();
        data.add_block(mon(), Lane::Plan, a, 18, 30).unwrap();
        data.set_note(mon(), "Good week").unwrap();
        assert_eq!(data.save_timesheet(mon(), at()).unwrap(), 16);
        assert!(data.add_block(add_days(mon(), 1), Lane::Actual, a, 18, 20).unwrap_err().contains("read-only"));
        assert!(data.set_note(mon(), "x").is_err());
        assert!(data.save_timesheet(mon(), at()).is_err());
        let sheet = &data.timesheets[&mon()];
        assert_eq!(sheet.lines[0].slots, [10, 0, 6, 0, 0, 0, 0]);
        assert_eq!(sheet.blocks.len(), 2);
        assert_eq!(sheet.note, "Good week");

        data.update_project(a, "Alpha Renamed", "A1", "").unwrap();
        assert_eq!(data.timesheets[&mon()].lines[0].name, "Alpha");
        data.reopen_timesheet(mon(), at()).unwrap();
        assert!(!data.is_locked(mon()));
        assert_eq!(data.revisions_for(mon()).len(), 1);
        assert_eq!(data.revisions_for(mon())[0].sheet.lines[0].name, "Alpha");
        data.add_block(add_days(mon(), 1), Lane::Actual, a, 18, 20).unwrap();
        assert_eq!(data.save_timesheet(mon(), at()).unwrap(), 18);
    }

    #[test]
    fn cannot_save_empty_week_and_unsaved_weeks_listed() {
        let mut data = Data::default();
        let a = data.add_project("A", "", "").unwrap();
        assert!(data.save_timesheet(mon(), at()).is_err());
        data.add_block(mon(), Lane::Actual, a, 18, 22).unwrap();
        data.add_block(add_days(mon(), 7), Lane::Actual, a, 18, 24).unwrap();
        data.save_timesheet(mon(), at()).unwrap();
        assert_eq!(data.unsaved_weeks(), vec![(add_days(mon(), 7), 6)]);
    }

    fn two_weeks() -> (Data, u32, u32) {
        let mut data = Data::default();
        let a = data.add_project("Alpha", "A1", "").unwrap();
        let b = data.add_project("Beta", "B2", "").unwrap();
        for i in 0..5 {
            data.add_block(add_days(mon(), i), Lane::Actual, a, 16, 28).unwrap(); // 6 h
            data.add_block(add_days(mon(), i), Lane::Actual, b, 28, 32).unwrap(); // 2 h
        }
        data.save_timesheet(mon(), at()).unwrap();
        for i in 7..12 {
            data.add_block(add_days(mon(), i), Lane::Actual, a, 16, 32).unwrap(); // 8 h, unsaved
        }
        (data, a, b)
    }

    #[test]
    fn report_totals_partial_weeks_and_matrix() {
        let (data, _, _) = two_weeks();
        let r = data.report(add_days(mon(), 3), add_days(mon(), 8), false);
        assert_eq!(r.total(), (12 + 4) * 2 + 16 * 2);
        let names: Vec<(&str, u32)> = r.projects.iter().map(|p| (p.name.as_str(), p.hh)).collect();
        assert_eq!(names, vec![("Alpha", 56), ("Beta", 8)]);
        assert_eq!(r.unsaved_hh, 32);
        assert!(r.includes_unsaved());
        assert_eq!(r.matrix.unit, "week");
        let labels: Vec<&str> = r.matrix.columns.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["19–22 Nov", "23–24 Nov"]);
        assert_eq!(r.matrix.cells, vec![vec![24, 32], vec![8, 0]]);
        assert_eq!(r.matrix.column_totals(), vec![32, 32]);
        assert!((r.projects.iter().map(|p| p.share).sum::<f64>() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn report_saved_only_and_renames() {
        let (mut data, a, _) = two_weeks();
        let r = data.report(mon(), add_days(mon(), 13), true);
        assert_eq!(r.total(), 80);
        assert_eq!(r.unsaved_hh, 80);
        assert!(!r.includes_unsaved());
        assert_eq!(r.days_worked(), 5);
        assert_eq!(r.avg_per_day(), "8");
        data.update_project(a, "Alpha Renamed", "A1", "").unwrap();
        let r = data.report(mon(), add_days(mon(), 13), false);
        assert_eq!(r.projects[0].display(), "Alpha Renamed (recorded as Alpha)");
        let year = data.report(d(2026, 1, 1), d(2026, 12, 31), false);
        assert_eq!(year.matrix.unit, "month");
        assert_eq!(year.matrix.columns.len(), 12);
        assert_eq!(year.active_weeks().len(), 2);
        let reversed = data.report(add_days(mon(), 13), mon(), false);
        assert_eq!(reversed.start, mon());
    }

    #[test]
    fn quick_ranges_are_right() {
        let r = quick_ranges(d(2026, 3, 18));
        assert_eq!((r[0].1, r[0].2), (d(2026, 3, 16), d(2026, 3, 22)));
        assert_eq!((r[3].1, r[3].2), (d(2026, 2, 1), d(2026, 2, 28)));
    }

    #[test]
    fn visible_range_widens_for_blocks() {
        let mut data = Data::default();
        let a = data.add_project("A", "", "").unwrap();
        assert_eq!(data.visible_slots(mon()), (14, 36));
        data.add_block(add_days(mon(), 5), Lane::Actual, a, 10, 12).unwrap();
        data.add_block(add_days(mon(), 2), Lane::Actual, a, 36, 40).unwrap();
        assert_eq!(data.visible_slots(mon()), (10, 40));
        assert_eq!(data.visible_days(mon()).len(), 7);
    }

    #[test]
    fn json_round_trip() {
        let (data, _, _) = two_weeks();
        let text = serde_json::to_string(&data).unwrap();
        let back: Data = serde_json::from_str(&text).unwrap();
        assert_eq!(back, data);
        let empty: Data = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.settings.hours, default_week_hours());
        // Files from before working hours existed (with the old fields) still load.
        let old: Data = serde_json::from_str(r#"{"settings":{"user_name":"D","daily_target":16,"day_start":14,"day_end":38}}"#).unwrap();
        assert_eq!(old.settings.user_name, "D");
        assert_eq!(old.settings.hours, default_week_hours());
    }

    #[test]
    fn working_hours() {
        let hours = default_week_hours();
        let m1 = hours[0].unwrap();
        assert_eq!((clock(m1.start), clock(m1.end)), ("08:00".into(), "17:00".into()));
        assert_eq!(m1.lunch, Some((840, 885)), "Mon-Thu lunch 14:00-14:45");
        assert_eq!(m1.working_minutes(), 8 * 60 + 15);
        let fri = hours[4].unwrap();
        assert_eq!((fri.end, fri.lunch), (16 * 60, Some((780, 840))));
        assert_eq!(fri.working_minutes(), 7 * 60);
        assert!(hours[5].is_none() && hours[6].is_none());

        let mut data = Data::default();
        assert_eq!(data.visible_slots(mon()), (14, 36), "07:00-18:00: an hour either side");
        assert_eq!(data.visible_days(mon()).len(), 5);
        assert!(data.set_day_hours(0, Some(DayHours::new(600, 540, None))).is_err());
        assert!(data.set_day_hours(0, Some(DayHours::new(480, 1020, Some((470, 500))))).is_err());
        data.set_day_hours(5, Some(DayHours::new(540, 780, None))).unwrap();
        assert_eq!(data.visible_days(mon()).len(), 7, "a working Saturday shows the weekend");
        assert_eq!(data.day_hours(add_days(mon(), 5)).unwrap().working_minutes(), 240);
    }

    #[test]
    fn old_data_files_load_with_new_fields_defaulted() {
        let old = r#"{"projects":[{"id":1,"name":"Alpha","code":"","client":"","color":0,"active":true}],"next_id":5}"#;
        let data: Data = serde_json::from_str(old).unwrap();
        assert_eq!(data.projects[0].stage, None);
        assert!(data.todos.is_empty() && data.rfis.is_empty());
    }

    #[test]
    fn todos_carry_over_until_ticked() {
        let mut data = Data::default();
        let a = data.add_project("Alpha", "", "").unwrap();
        let wk1 = mon();
        let (wk2, wk3) = (add_days(wk1, 7), add_days(wk1, 14));
        let draw = data.add_todo(a, add_days(wk1, 2), "Issue drawings").unwrap(); // added mid-week
        let call = data.add_todo(a, wk1, "Call engineer").unwrap();
        assert!(data.add_todo(a, wk1, "   ").is_err());
        assert_eq!(data.todos_for(a, wk1).len(), 2);
        assert!(data.todos_for(a, add_days(wk1, -7)).is_empty(), "not before it was added");
        // Tick one off in week 2: it shows ticked in week 2, gone from week 3.
        assert!(data.toggle_todo(call, wk2).unwrap());
        let wk2_list: Vec<(u64, Option<NaiveDate>)> = data.todos_for(a, wk2).iter().map(|t| (t.id, t.done)).collect();
        assert_eq!(wk2_list, vec![(draw, None), (call, Some(wk2))]);
        let wk3_ids: Vec<u64> = data.todos_for(a, wk3).iter().map(|t| t.id).collect();
        assert_eq!(wk3_ids, vec![draw], "the unticked one carries over");
        // In week 1 (before it was ticked) it was still open.
        assert!(data.todos_for(a, wk1).iter().all(|t| t.done != Some(wk1)));
        // Untick in the same week brings it back.
        assert!(!data.toggle_todo(call, wk2).unwrap());
        assert_eq!(data.todos_for(a, wk3).len(), 2);
        data.edit_todo(draw, "Issue revised drawings").unwrap();
        data.delete_todo(call);
        assert_eq!(data.todos_for(a, wk3)[0].text, "Issue revised drawings");
        assert!(data.project_in_use(a));
    }

    #[test]
    fn rfis_need_a_project_on_site_and_number_per_project() {
        let mut data = Data::default();
        let a = data.add_project("Alpha", "", "").unwrap();
        let b = data.add_project("Beta", "", "").unwrap();
        assert!(data.add_rfi(a, "Window detail", "", mon(), None).unwrap_err().contains("on site"));
        assert!(data.set_project_stage(a, Some(7)).is_err());
        data.set_project_stage(a, Some(ON_SITE_STAGE)).unwrap();
        data.set_project_stage(b, Some(5)).unwrap();
        assert_eq!(stage_label(Some(5)), "Stage 5 · Construction");
        assert_eq!(stage_label(None), "Stage not set");
        assert_eq!(data.add_rfi(a, "Window detail", "Sill height?", mon(), Some(add_days(mon(), 3))).unwrap(), 1);
        assert_eq!(data.add_rfi(a, "Roof fall", "", mon(), None).unwrap(), 2);
        assert_eq!(data.add_rfi(b, "Paving", "", mon(), None).unwrap(), 1, "numbering is per project");
        assert!(data.add_rfi(a, "Bad due", "", mon(), Some(add_days(mon(), -1))).is_err());
        assert!(data.add_rfi(a, "  ", "", mon(), None).is_err());
        let later = add_days(mon(), 10);
        assert_eq!(data.rfi_counts(a, later), (2, 1));
        let first = data.rfis_for(a)[0].id;
        assert_eq!(data.rfis_for(a)[0].status(later), "Overdue");
        assert!(data.answer_rfi(first, add_days(mon(), -2), "x").is_err());
        data.answer_rfi(first, add_days(mon(), 2), "900 mm").unwrap();
        assert_eq!(data.rfis_for(a)[0].status(later), "Answered");
        assert_eq!(data.rfi_counts(a, later), (1, 0));
        data.reopen_rfi(first);
        assert_eq!(data.rfi_counts(a, later), (2, 1));
        // Leaving site doesn't lose the register.
        data.set_project_stage(a, Some(6)).unwrap();
        assert_eq!(data.rfis_for(a).len(), 2);
        assert!(data.delete_project(a).is_err());
    }
}
