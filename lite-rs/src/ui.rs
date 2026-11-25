//! The application window: state, theme, tabs and actions.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::NaiveDate;
use eframe::egui::{self, Color32, CornerRadius, Key, RichText, Stroke, Ui};

use crate::exports;
use crate::model::{self, Block, Data, Report, add_days, date_time, fmt_h, long_date, week_label};
use crate::pdf;
use crate::store;
use crate::week::{self, Drag};

pub mod theme {
    use eframe::egui::Color32;

    pub const BG: Color32 = Color32::from_rgb(0xFA, 0xF2, 0xEC);
    pub const CARD: Color32 = Color32::from_rgb(0xFF, 0xFA, 0xF6);
    pub const TINT: Color32 = Color32::from_rgb(0xFB, 0xEE, 0xE7);
    pub const SOFT: Color32 = Color32::from_rgb(0xF5, 0xE2, 0xD8);
    pub const BORDER: Color32 = Color32::from_rgb(0xEC, 0xDB, 0xD0);
    pub const GRID: Color32 = Color32::from_rgb(0xE2, 0xCC, 0xBF);
    pub const FIELD_BORDER: Color32 = Color32::from_rgb(0xC9, 0xAF, 0xA1);
    pub const TEXT: Color32 = Color32::from_rgb(0x3B, 0x2C, 0x25);
    pub const MUTED: Color32 = Color32::from_rgb(0x7F, 0x64, 0x57);
    pub const ACCENT: Color32 = Color32::from_rgb(0xB0, 0x5A, 0x3E);
    pub const ACCENT_DARK: Color32 = Color32::from_rgb(0x8F, 0x46, 0x30);
    pub const OK: Color32 = Color32::from_rgb(0x37, 0x6D, 0x4B);
    pub const OK_SOFT: Color32 = Color32::from_rgb(0xE3, 0xEF, 0xE6);
    pub const UNDER: Color32 = Color32::from_rgb(0x8A, 0x5A, 0x12);
    pub const OVER: Color32 = Color32::from_rgb(0xA2, 0x3E, 0x27);
    pub const WARN_SOFT: Color32 = Color32::from_rgb(0xF6, 0xEB, 0xD6);
    /// Working-hours markers on the week timeline.
    pub const WORK_LINE: Color32 = Color32::from_rgb(0x37, 0x6D, 0x4B);
    pub const LUNCH: Color32 = Color32::from_rgb(0xF4, 0xE8, 0xC8);
    pub const LUNCH_STRIPE: Color32 = Color32::from_rgb(0xE6, 0xD2, 0xA0);

    pub fn project(index: u8) -> Color32 {
        let c = crate::model::PALETTE[usize::from(index) % crate::model::PALETTE.len()];
        Color32::from_rgb(c[0], c[1], c[2])
    }

    pub fn tint(c: Color32, amount: f32) -> Color32 {
        let mix = |v: u8| (f32::from(v) + (255.0 - f32::from(v)) * amount) as u8;
        Color32::from_rgb(mix(c.r()), mix(c.g()), mix(c.b()))
    }

    pub fn text_on(fill: Color32) -> Color32 {
        let lum = 0.299 * f32::from(fill.r()) + 0.587 * f32::from(fill.g()) + 0.114 * f32::from(fill.b());
        if lum > 150.0 { TEXT } else { Color32::WHITE }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Week,
    Projects,
    Timesheets,
    Reports,
    Settings,
}

#[derive(Clone, Debug)]
pub enum Confirm {
    SaveWeek(NaiveDate),
    Reopen(NaiveDate),
    DeleteProject(u32),
    Restore(String),
    DeleteRfi(u64),
}

struct Flash {
    text: String,
    error: bool,
    until: f64,
}

#[derive(Clone)]
struct UndoStep {
    week: NaiveDate,
    blocks: Vec<Block>,
}

/// The "raise an RFI" form.
struct RfiForm {
    subject: String,
    details: String,
    raised: NaiveDate,
    has_due: bool,
    due: NaiveDate,
}

impl Default for RfiForm {
    fn default() -> Self {
        let today = model::today();
        RfiForm { subject: String::new(), details: String::new(), raised: today, has_due: true, due: add_days(today, 5) }
    }
}

pub struct App {
    pub data: Data,
    dir: PathBuf,
    _lock: Option<File>,
    fatal: Option<String>,
    pub tab: Tab,
    pub week: NaiveDate,
    pub brush: Option<u32>,
    pub selected: Option<u64>,
    pub menu_block: Option<u64>,
    pub drag: Option<Drag>,
    pub lanes: Vec<week::LaneGeo>,
    undo: Vec<UndoStep>,
    redo: Vec<UndoStep>,
    dirty: bool,
    revision: u64,
    last_edit: f64,
    save_error: Option<String>,
    flash: Option<Flash>,
    last_export: Option<PathBuf>,
    pub note_buf: (NaiveDate, String),
    pub form: [String; 3],
    editing: Option<(u32, [String; 3])>,
    pub confirm: Option<Confirm>,
    report_range: (NaiveDate, NaiveDate),
    report_saved_only: bool,
    report_cache: Option<((NaiveDate, NaiveDate, bool, u64), Arc<Report>)>,
    startup_note: Option<String>,
    now: f64,
    /// Text being typed into each project's "add a to-do" box.
    pub todo_inputs: std::collections::HashMap<u32, String>,
    /// To-do being edited: (id, text).
    pub todo_edit: Option<(u64, String)>,
    /// Project whose RFI register window is open.
    pub rfi_project: Option<u32>,
    rfi_form: RfiForm,
    /// RFI being answered/edited: (id, response, answered date).
    rfi_answer: Option<(u64, String, NaiveDate)>,
}

impl App {
    pub fn new(data: Data, dir: PathBuf, lock: Option<File>, warning: Option<String>) -> Self {
        let week = model::monday_of(model::today());
        let range = model::month_bounds(model::today());
        let note = data.note(week).to_string();
        App {
            data,
            dir,
            _lock: lock,
            fatal: None,
            tab: Tab::Week,
            week,
            brush: None,
            selected: None,
            menu_block: None,
            drag: None,
            lanes: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
            revision: 0,
            last_edit: 0.0,
            save_error: None,
            flash: None,
            last_export: None,
            note_buf: (week, note),
            form: Default::default(),
            editing: None,
            confirm: None,
            report_range: range,
            report_saved_only: false,
            report_cache: None,
            startup_note: warning,
            now: 0.0,
            todo_inputs: Default::default(),
            todo_edit: None,
            rfi_project: None,
            rfi_form: RfiForm::default(),
            rfi_answer: None,
        }
    }

    /// A window that only explains a problem (data unreadable, already open).
    pub fn fatal(message: String, dir: PathBuf) -> Self {
        let mut app = App::new(Data::default(), dir, None, None);
        app.fatal = Some(message);
        app
    }

    #[cfg(feature = "dev")]
    pub fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }

    // ---- change tracking, saving, undo ------------------------------------------
    pub fn changed(&mut self) {
        self.dirty = true;
        self.revision += 1;
        self.last_edit = self.now;
    }

    /// Record a timeline edit for undo (if anything actually changed).
    pub fn commit_week_edit(&mut self, before: Vec<Block>) {
        let mut after = self.data.week_blocks(self.week);
        let mut was = before.clone();
        after.sort_by_key(|b| b.id);
        was.sort_by_key(|b| b.id);
        if after != was {
            self.undo.push(UndoStep { week: self.week, blocks: before });
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.changed();
        }
    }

    fn undo_redo(&mut self, undo: bool) {
        let step = if undo { self.undo.pop() } else { self.redo.pop() };
        let Some(step) = step else {
            self.flash(if undo { "Nothing to undo." } else { "Nothing to redo." }, false);
            return;
        };
        if self.data.is_locked(step.week) {
            self.flash(model::LOCKED, true);
            if undo { self.undo.push(step) } else { self.redo.push(step) }
            return;
        }
        let current = UndoStep { week: step.week, blocks: self.data.week_blocks(step.week) };
        self.data.replace_week_blocks(step.week, step.blocks);
        if undo { self.redo.push(current) } else { self.undo.push(current) }
        self.week = step.week;
        self.selected = None;
        self.changed();
        self.flash(if undo { "Undone." } else { "Redone." }, false);
    }

    fn save_now(&mut self) {
        if self.fatal.is_some() || !self.dirty {
            return;
        }
        match store::save(&self.dir, &self.data) {
            Ok(()) => {
                self.dirty = false;
                self.save_error = None;
            }
            Err(e) => self.save_error = Some(e),
        }
    }

    pub fn flash(&mut self, text: &str, error: bool) {
        self.flash = Some(Flash { text: text.to_string(), error, until: self.now + if error { 7.0 } else { 4.0 } });
    }

    pub fn go_to_week(&mut self, week: NaiveDate) {
        self.week = model::monday_of(week);
        self.selected = None;
        self.drag = None;
    }

    pub fn delete_block(&mut self, id: u64) {
        let before = self.data.week_blocks(self.week);
        match self.data.delete_block(id) {
            Ok(()) => {
                self.selected = None;
                self.commit_week_edit(before);
            }
            Err(e) => self.flash(&e, true),
        }
    }

    pub fn sync_note_buffer(&mut self) {
        if self.note_buf.0 != self.week {
            self.note_buf = (self.week, self.data.note(self.week).to_string());
        }
    }

    // ---- exports ---------------------------------------------------------------
    fn deliver(&mut self, name: &str, bytes: Result<Vec<u8>, String>, open: bool) {
        match bytes.and_then(|b| exports::write_export(name, &b)) {
            Ok(path) => {
                if open {
                    exports::open_path(&path);
                }
                self.flash(&format!("Saved {}", path.display()), false);
                self.last_export = Some(path);
            }
            Err(e) => self.flash(&format!("Export failed: {e}"), true),
        }
    }

    pub fn export_pdf_plan(&mut self, week: NaiveDate) {
        let bytes = pdf::plan_pdf(&self.data, week);
        self.deliver(&format!("Weekly plan {}.pdf", week.format("%Y-%m-%d")), Ok(bytes), true);
    }

    pub fn export_pdf_timesheet(&mut self, week: NaiveDate) {
        let bytes = pdf::timesheet_pdf(&self.data, week);
        let draft = if self.data.is_locked(week) { "" } else { " (draft)" };
        self.deliver(&format!("Timesheet {}{draft}.pdf", week.format("%Y-%m-%d")), Ok(bytes), true);
    }

    fn report(&mut self) -> Arc<Report> {
        let key = (self.report_range.0, self.report_range.1, self.report_saved_only, self.revision);
        if let Some((k, r)) = &self.report_cache {
            if *k == key {
                return r.clone();
            }
        }
        let report = Arc::new(self.data.report(key.0, key.1, key.2));
        self.report_cache = Some((key, report.clone()));
        report
    }

    // ---- keyboard ----------------------------------------------------------------
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() || self.confirm.is_some() {
            return;
        }
        let (undo, redo, del, left, right, shift, esc, pgup, pgdn) = ctx.input(|i| {
            let cmd = i.modifiers.command;
            (
                cmd && !i.modifiers.shift && i.key_pressed(Key::Z),
                cmd && (i.key_pressed(Key::Y) || (i.modifiers.shift && i.key_pressed(Key::Z))),
                i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.modifiers.shift,
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::PageUp),
                i.key_pressed(Key::PageDown),
            )
        });
        if undo {
            self.undo_redo(true);
        }
        if redo {
            self.undo_redo(false);
        }
        if self.tab != Tab::Week {
            return;
        }
        if pgup {
            self.go_to_week(add_days(self.week, -7));
        }
        if pgdn {
            self.go_to_week(add_days(self.week, 7));
        }
        if esc {
            self.selected = None;
        }
        let Some(id) = self.selected else { return };
        if del {
            self.delete_block(id);
        } else if left || right {
            let Some(b) = self.data.block(id).cloned() else { return };
            let free = self.data.free_interval(b.date, b.lane, b.start, Some(id)).unwrap_or((b.start, b.end));
            let step: i32 = if right { 1 } else { -1 };
            let span = if shift {
                model::resize_span((b.start, b.end), false, (i32::from(b.end) + step).clamp(0, 48) as u8, free)
            } else {
                model::move_span((b.start, b.end), i32::from(b.start) + step, free)
            };
            let before = self.data.week_blocks(self.week);
            if self.data.set_block_span(id, span.0, span.1).is_ok() {
                self.commit_week_edit(before);
            } else {
                self.flash(model::LOCKED, true);
            }
        }
    }

    // ---- confirmation dialogs ------------------------------------------------------
    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(confirm) = self.confirm.clone() else { return };
        let (title, body, action) = match &confirm {
            Confirm::SaveWeek(w) => (
                "Save timesheet?",
                format!("Save {} as a timesheet record? The week becomes read-only; you can reopen it later (the saved version is kept).", week_label(*w)),
                "Save timesheet",
            ),
            Confirm::Reopen(w) => (
                "Reopen timesheet?",
                format!("Reopen {} for editing? The saved version is kept in your timesheet history; save the week again when you're done.", week_label(*w)),
                "Reopen",
            ),
            Confirm::DeleteProject(id) => (
                "Delete project?",
                format!("Delete “{}” permanently?", self.data.project(*id).map(|p| p.name.clone()).unwrap_or_default()),
                "Delete",
            ),
            Confirm::DeleteRfi(id) => (
                "Delete RFI?",
                format!(
                    "Delete RFI {} permanently? Consider marking it answered instead so the register stays complete.",
                    self.data.rfis.iter().find(|r| r.id == *id).map(|r| format!("{} “{}”", r.number, r.subject)).unwrap_or_default()
                ),
                "Delete",
            ),
            Confirm::Restore(name) => {
                let older = store::list_backups(&self.dir)
                    .iter()
                    .find(|b| &b.name == name)
                    .map(|b| b.modified)
                    .zip(self.data.timesheets.values().map(|t| t.saved_at).max())
                    .is_some_and(|(made, latest)| made < latest);
                (
                    "Restore backup?",
                    format!(
                        "Replace your data with the backup {name}? Everything entered since then will be replaced. A safety backup of your current data is made first, so this can be undone.{}",
                        if older { "\n\nNote: this backup is older than your latest saved timesheet." } else { "" }
                    ),
                    "Restore",
                )
            }
        };
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.heading(title);
            ui.add_space(6.0);
            ui.label(body);
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let go = egui::Button::new(RichText::new(action).color(Color32::WHITE).strong()).fill(theme::ACCENT);
                if ui.add(go).clicked() {
                    self.run_confirmed(&confirm);
                    close = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        if close || modal.should_close() {
            self.confirm = None;
        }
    }

    fn run_confirmed(&mut self, confirm: &Confirm) {
        match confirm {
            Confirm::SaveWeek(w) => match self.data.save_timesheet(*w, model::now()) {
                Ok(total) => {
                    self.changed();
                    self.flash(&format!("Timesheet saved: {} h for {}.", fmt_h(total), week_label(*w)), false);
                }
                Err(e) => self.flash(&e, true),
            },
            Confirm::Reopen(w) => match self.data.reopen_timesheet(*w, model::now()) {
                Ok(()) => {
                    self.changed();
                    self.flash("Timesheet reopened. Edit the week, then save it again.", false);
                }
                Err(e) => self.flash(&e, true),
            },
            Confirm::DeleteProject(id) => match self.data.delete_project(*id) {
                Ok(name) => {
                    self.changed();
                    self.flash(&format!("Deleted “{name}”."), false);
                }
                Err(e) => self.flash(&e, true),
            },
            Confirm::DeleteRfi(id) => {
                self.data.delete_rfi(*id);
                self.changed();
                self.flash("RFI deleted.", false);
            }
            Confirm::Restore(name) => {
                self.save_now();
                match store::restore_backup(&self.dir, name) {
                    Ok((data, safety)) => {
                        self.data = data;
                        self.undo.clear();
                        self.redo.clear();
                        self.selected = None;
                        self.note_buf.0 = add_days(self.week, -7);
                        self.dirty = false;
                        self.revision += 1;
                        let safety = safety.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        self.flash(&format!("Restored {name}. Your data from just before was kept as {safety}."), false);
                    }
                    Err(e) => self.flash(&e, true),
                }
            }
        }
    }
}

// --------------------------------------------------------------------------
//  Shared widgets
// --------------------------------------------------------------------------
pub fn page_title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).font(egui::FontId::new(26.0, egui::FontFamily::Name("brand".into()))).color(theme::TEXT));
}

pub fn chip(ui: &mut Ui, text: &str, color: Color32, fill: Color32) {
    egui::Frame::default()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, theme::tint(color, 0.6)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color).size(13.0));
        });
}

pub fn banner(ui: &mut Ui, text: &str, warn: bool) {
    egui::Frame::default()
        .fill(if warn { theme::WARN_SOFT } else { theme::TINT })
        .stroke(Stroke::new(1.0_f32, if warn { Color32::from_rgb(0xEC, 0xD6, 0xA8) } else { theme::SOFT }))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).color(if warn { theme::UNDER } else { theme::TEXT }));
        });
}

pub fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::default()
        .fill(theme::CARD)
        .stroke(Stroke::new(1.0_f32, theme::BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Normal (non-justified) layout, so buttons keep their natural size
            // even inside egui's justified columns.
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), add);
        });
}

fn primary(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).color(Color32::WHITE).strong()).fill(theme::ACCENT))
}

/// Name / code / client fields + Add button.
pub fn project_form(app: &mut App, ui: &mut Ui, salt: &str) {
    let mut submit = false;
    ui.horizontal_wrapped(|ui| {
        ui.label("Name");
        let r = ui.add(egui::TextEdit::singleline(&mut app.form[0]).id_salt((salt, "n")).hint_text("e.g. Longlands Clubhouse").desired_width(200.0));
        submit |= r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.label("Code");
        let r = ui.add(egui::TextEdit::singleline(&mut app.form[1]).id_salt((salt, "c")).hint_text("optional").desired_width(80.0));
        submit |= r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.label("Client");
        let r = ui.add(egui::TextEdit::singleline(&mut app.form[2]).id_salt((salt, "k")).hint_text("optional").desired_width(140.0));
        submit |= r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        submit |= primary(ui, "Add project").clicked();
    });
    if submit {
        let [name, code, client] = app.form.clone();
        match app.data.add_project(&name, &code, &client) {
            Ok(id) => {
                app.form = Default::default();
                app.brush = Some(id);
                app.changed();
                app.flash(&format!("Added “{}”.", app.data.project(id).map(|p| p.name.clone()).unwrap_or_default()), false);
                ui.close_menu();
            }
            Err(e) => app.flash(&e, true),
        }
    }
}

// --------------------------------------------------------------------------
//  Setup: fonts, colours
// --------------------------------------------------------------------------
pub fn setup(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let font_dir = Path::new(&windir).join("Fonts");
    let mut load = |key: &str, file: &str| -> bool {
        match std::fs::read(font_dir.join(file)) {
            Ok(bytes) => {
                fonts.font_data.insert(key.into(), Arc::new(egui::FontData::from_owned(bytes)));
                true
            }
            Err(_) => false,
        }
    };
    let segoe = load("segoe", "segoeui.ttf");
    let symbols = load("symbols", "seguisym.ttf");
    let stencil = load("stencil", "STENCIL.TTF");
    let proportional = fonts.families.entry(egui::FontFamily::Proportional).or_default();
    if segoe {
        proportional.insert(0, "segoe".into());
    }
    if symbols {
        proportional.push("symbols".into());
    }
    let mut brand: Vec<String> = Vec::new();
    if stencil {
        brand.push("stencil".into());
    }
    brand.extend(fonts.families[&egui::FontFamily::Proportional].iter().cloned());
    fonts.families.insert(egui::FontFamily::Name("brand".into()), brand);
    ctx.set_fonts(fonts);

    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = theme::BG;
    visuals.window_fill = theme::CARD;
    visuals.extreme_bg_color = Color32::WHITE;
    visuals.faint_bg_color = theme::TINT;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, theme::TEXT);
    visuals.hyperlink_color = theme::ACCENT_DARK;
    visuals.selection.bg_fill = theme::SOFT;
    visuals.selection.stroke = Stroke::new(1.0_f32, theme::ACCENT_DARK);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, theme::BORDER);
    visuals.widgets.inactive.weak_bg_fill = Color32::WHITE;
    visuals.widgets.inactive.bg_fill = Color32::WHITE;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, theme::FIELD_BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, theme::TEXT);
    visuals.widgets.hovered.weak_bg_fill = theme::TINT;
    visuals.widgets.hovered.bg_fill = theme::TINT;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, theme::ACCENT);
    visuals.widgets.active.weak_bg_fill = theme::SOFT;
    visuals.widgets.active.bg_fill = theme::SOFT;
    visuals.widgets.open.weak_bg_fill = theme::SOFT;
    for w in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active, &mut visuals.widgets.open] {
        w.corner_radius = CornerRadius::same(5);
    }
    // Always the light theme (egui otherwise follows the system and would use
    // its default dark style); style it explicitly.
    ctx.set_theme(egui::Theme::Light);
    ctx.set_visuals_of(egui::Theme::Light, visuals);
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.interact_size.y = 28.0;
        style.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.5));
        style.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(14.5));
        style.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(20.0));
    });
}

// --------------------------------------------------------------------------
//  The frame
// --------------------------------------------------------------------------
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ui(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.drag = None;
        self.save_now();
    }
}

impl App {
    /// One frame of the whole window (also called directly by headless UI tests).
    pub fn ui(&mut self, ctx: &egui::Context) {
        self.now = ctx.input(|i| i.time);
        if let Some(message) = self.fatal.clone() {
            fatal_screen(self, ctx, &message);
            return;
        }
        if let Some(note) = self.startup_note.take() {
            self.flash(&note, true);
        }
        self.shortcuts(ctx);
        top_bar(self, ctx);
        status_bar(self, ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(theme::BG).inner_margin(egui::Margin::symmetric(18, 12)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.tab {
                    Tab::Week => week::show(self, ui),
                    Tab::Projects => projects_tab(self, ui),
                    Tab::Timesheets => timesheets_tab(self, ui),
                    Tab::Reports => reports_tab(self, ui),
                    Tab::Settings => settings_tab(self, ui),
                });
            });
        self.confirm_dialog(ctx);
        rfi_window(self, ctx);

        // Autosave shortly after the last change (never mid-drag).
        if self.dirty && self.drag.is_none() {
            if self.now - self.last_edit > 0.6 {
                self.save_now();
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(650));
            }
        }
        if let Some(flash) = &self.flash {
            if self.now > flash.until {
                self.flash = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
    }
}

fn top_bar(app: &mut App, ctx: &egui::Context) {
    egui::TopBottomPanel::top("top")
        .frame(egui::Frame::default().fill(theme::CARD).inner_margin(egui::Margin::symmetric(16, 8)).stroke(Stroke::new(1.0_f32, theme::BORDER)))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("TINY TIME TRACK").font(egui::FontId::new(22.0, egui::FontFamily::Name("brand".into()))).color(theme::ACCENT));
                egui::Frame::default().fill(theme::ACCENT).corner_radius(CornerRadius::same(9)).inner_margin(egui::Margin::symmetric(7, 1)).show(ui, |ui| {
                    ui.label(RichText::new("LITE").color(Color32::WHITE).strong().size(11.0));
                });
                ui.add_space(18.0);
                for (tab, name) in [
                    (Tab::Week, "Week"),
                    (Tab::Projects, "Projects"),
                    (Tab::Timesheets, "Timesheets"),
                    (Tab::Reports, "Reports"),
                    (Tab::Settings, "Settings"),
                ] {
                    let selected = app.tab == tab;
                    let text = RichText::new(name).color(if selected { theme::ACCENT_DARK } else { theme::TEXT }).strong();
                    let button = egui::Button::new(text).fill(if selected { theme::SOFT } else { Color32::TRANSPARENT }).stroke(Stroke::NONE).corner_radius(CornerRadius::same(14));
                    if ui.add(button).clicked() {
                        app.tab = tab;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !app.data.settings.user_name.is_empty() {
                        ui.label(RichText::new(&app.data.settings.user_name).color(theme::MUTED));
                    }
                });
            });
        });
}

fn status_bar(app: &mut App, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("status")
        .frame(egui::Frame::default().fill(theme::CARD).inner_margin(egui::Margin::symmetric(16, 6)).stroke(Stroke::new(1.0_f32, theme::BORDER)))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                match (&app.save_error, app.dirty) {
                    (Some(e), _) => {
                        ui.label(RichText::new(format!("⚠ Not saved: {e}")).color(theme::OVER).strong());
                    }
                    (None, true) => {
                        ui.label(RichText::new("Saving…").color(theme::MUTED));
                    }
                    (None, false) => {
                        ui.label(RichText::new("✔ All changes saved").color(theme::OK));
                    }
                }
                if let Some(flash) = &app.flash {
                    ui.separator();
                    let color = if flash.error { theme::OVER } else { theme::TEXT };
                    ui.label(RichText::new(&flash.text).color(color).strong());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(path) = app.last_export.clone() {
                        if ui.small_button("Show in folder").clicked() {
                            exports::reveal(&path);
                        }
                        if ui.small_button("Open last export").clicked() {
                            exports::open_path(&path);
                        }
                    }
                });
            });
        });
}

fn fatal_screen(app: &mut App, ctx: &egui::Context, message: &str) {
    egui::CentralPanel::default().frame(egui::Frame::default().fill(theme::BG).inner_margin(egui::Margin::same(40))).show(ctx, |ui| {
        card(ui, |ui| {
            ui.heading("Tiny Time Track can't start");
            ui.add_space(8.0);
            ui.label(message);
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Open data folder").clicked() {
                    exports::open_path(&app.dir);
                }
                if ui.button("Close").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
    });
}

// --------------------------------------------------------------------------
//  Projects
// --------------------------------------------------------------------------
fn projects_tab(app: &mut App, ui: &mut Ui) {
    page_title(ui, "Projects");
    ui.add_space(6.0);
    card(ui, |ui| {
        ui.label(RichText::new("Add a project").strong());
        project_form(app, ui, "projects");
    });
    ui.add_space(10.0);
    let totals = app.data.project_totals();
    let projects: Vec<model::Project> = app.data.projects_sorted().into_iter().cloned().collect();
    if projects.is_empty() {
        ui.label(RichText::new("No projects yet. Add your first one above.").color(theme::MUTED));
        return;
    }
    card(ui, |ui| {
        let today = model::today();
        egui::Grid::new("projects").striped(true).num_columns(10).spacing([16.0, 8.0]).show(ui, |ui| {
            for h in ["Colour", "Project", "Code", "Client", "Stage", "RFIs", "Planned", "Actual", "Saved", ""] {
                ui.label(RichText::new(h).color(theme::MUTED));
            }
            ui.end_row();
            for p in projects {
                let (plan, actual, saved) = totals.get(&p.id).copied().unwrap_or_default();
                let editing = app.editing.as_ref().is_some_and(|e| e.0 == p.id);
                // colour picker
                let swatch = egui::Button::new("    ").fill(theme::project(p.color)).corner_radius(CornerRadius::same(6));
                ui.menu_button_or(swatch, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.set_max_width(180.0);
                        for c in 0..model::PALETTE.len() as u8 {
                            if ui.add(egui::Button::new("   ").fill(theme::project(c))).clicked() {
                                app.data.set_project_color(p.id, c);
                                app.changed();
                                ui.close_menu();
                            }
                        }
                    });
                });
                if editing {
                    let fields = &mut app.editing.as_mut().expect("editing").1;
                    ui.add(egui::TextEdit::singleline(&mut fields[0]).desired_width(200.0));
                    ui.add(egui::TextEdit::singleline(&mut fields[1]).desired_width(80.0));
                    ui.add(egui::TextEdit::singleline(&mut fields[2]).desired_width(140.0));
                } else {
                    let name = if p.active { RichText::new(&p.name).strong() } else { RichText::new(format!("{} (archived)", p.name)).color(theme::MUTED) };
                    ui.label(name);
                    ui.label(&p.code);
                    ui.label(&p.client);
                }
                stage_picker(app, ui, &p);
                rfi_button(app, ui, p.id, today);
                ui.label(format!("{} h", fmt_h(plan)));
                ui.label(format!("{} h", fmt_h(actual)));
                ui.label(format!("{} h", fmt_h(saved)));
                ui.horizontal(|ui| {
                    if editing {
                        if primary(ui, "Save").clicked() {
                            let [n, c, k] = app.editing.as_ref().expect("editing").1.clone();
                            match app.data.update_project(p.id, &n, &c, &k) {
                                Ok(()) => {
                                    app.editing = None;
                                    app.changed();
                                    app.flash(&format!("Saved “{n}”."), false);
                                }
                                Err(e) => app.flash(&e, true),
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            app.editing = None;
                        }
                    } else {
                        if ui.button("Edit").clicked() {
                            app.editing = Some((p.id, [p.name.clone(), p.code.clone(), p.client.clone()]));
                        }
                        let toggle = if p.active { "Archive" } else { "Restore" };
                        let tip = if p.active { "Hide from the Week page; its time is kept" } else { "Show it on the Week page again" };
                        if ui.button(toggle).on_hover_text(tip).clicked() {
                            let _ = app.data.set_project_active(p.id, !p.active);
                            app.changed();
                        }
                        if !app.data.project_in_use(p.id) && ui.button("Delete").clicked() {
                            app.confirm = Some(Confirm::DeleteProject(p.id));
                        }
                    }
                });
                ui.end_row();
            }
        });
    });
    ui.label(RichText::new("Projects with recorded time, to-dos or RFIs can't be deleted — archive them instead so your records stay complete. Stages are the SACAP work stages; RFIs open up at Stage 5 · Construction (on site).").size(12.0).color(theme::MUTED));
}

/// SACAP work-stage selector for a project.
pub fn stage_picker(app: &mut App, ui: &mut Ui, p: &model::Project) {
    let mut stage = p.stage;
    let text = match p.stage {
        Some(s) => format!("{s} · {}", model::STAGES[usize::from(s - 1)]),
        None => "Not set".into(),
    };
    egui::ComboBox::from_id_salt(("stage", p.id)).selected_text(text).width(190.0).show_ui(ui, |ui| {
        ui.selectable_value(&mut stage, None, "Not set");
        for (i, name) in model::STAGES.iter().enumerate() {
            let n = i as u8 + 1;
            let label = if n == model::ON_SITE_STAGE { format!("{n} · {name} (on site)") } else { format!("{n} · {name}") };
            ui.selectable_value(&mut stage, Some(n), label);
        }
    });
    if stage != p.stage && app.data.set_project_stage(p.id, stage).is_ok() {
        app.changed();
        if stage == Some(model::ON_SITE_STAGE) {
            app.flash(&format!("“{}” is on site — its RFI register is now available.", p.name), false);
        }
    }
}

/// "RFIs (2 open)" button: shown when on site or when the project has RFIs.
pub fn rfi_button(app: &mut App, ui: &mut Ui, project: u32, today: NaiveDate) {
    let has = !app.data.rfis_for(project).is_empty();
    if !app.data.is_on_site(project) && !has {
        ui.label(RichText::new("—").color(theme::MUTED)).on_hover_text("RFIs open up when the project is at Stage 5 · Construction (on site)");
        return;
    }
    let (open, overdue) = app.data.rfi_counts(project, today);
    let text = match (open, overdue) {
        (0, _) => "RFIs".to_string(),
        (o, 0) => format!("RFIs ({o} open)"),
        (o, d) => format!("RFIs ({o} open, {d} overdue)"),
    };
    let label = if overdue > 0 { RichText::new(text).color(theme::OVER).strong() } else { RichText::new(text) };
    if ui.button(label).on_hover_text("Open the request-for-information register").clicked() {
        app.rfi_project = Some(project);
    }
}

trait MenuButtonOr {
    fn menu_button_or(&mut self, button: egui::Button<'_>, add: impl FnOnce(&mut Ui)) -> egui::Response;
}
impl MenuButtonOr for Ui {
    fn menu_button_or(&mut self, button: egui::Button<'_>, add: impl FnOnce(&mut Ui)) -> egui::Response {
        let response = self.add(button).on_hover_text("Choose a colour");
        egui::popup::popup_below_widget(self, response.id.with("colours"), &response, egui::PopupCloseBehavior::CloseOnClickOutside, add);
        if response.clicked() {
            self.memory_mut(|m| m.toggle_popup(response.id.with("colours")));
        }
        response
    }
}

// --------------------------------------------------------------------------
//  Timesheets
// --------------------------------------------------------------------------
fn timesheets_tab(app: &mut App, ui: &mut Ui) {
    page_title(ui, "Timesheets");
    ui.label(RichText::new("Saved timesheets are your permanent record of time spent.").color(theme::MUTED));
    ui.add_space(8.0);

    let unsaved = app.data.unsaved_weeks();
    if !unsaved.is_empty() {
        card(ui, |ui| {
            ui.label(RichText::new("Weeks with time not saved yet").strong().color(theme::UNDER));
            egui::Grid::new("unsaved").striped(true).num_columns(3).spacing([24.0, 6.0]).show(ui, |ui| {
                for (week, hh) in unsaved {
                    ui.label(week_label(week));
                    ui.label(format!("{} h", fmt_h(hh)));
                    if ui.button("Open & save").clicked() {
                        app.go_to_week(week);
                        app.tab = Tab::Week;
                    }
                    ui.end_row();
                }
            });
        });
        ui.add_space(10.0);
    }

    card(ui, |ui| {
        ui.label(RichText::new("Saved timesheets").strong());
        if app.data.timesheets.is_empty() {
            ui.label(RichText::new("None yet. Fill in a week's actual time on the Week page and press “Save timesheet”.").color(theme::MUTED));
            return;
        }
        let sheets: Vec<(NaiveDate, u32, String)> =
            app.data.timesheets.values().rev().map(|t| (t.week, t.total(), date_time(t.saved_at))).collect();
        let grand: u32 = sheets.iter().map(|s| s.1).sum();
        egui::Grid::new("saved").striped(true).num_columns(4).spacing([24.0, 6.0]).show(ui, |ui| {
            for h in ["Week", "Hours", "Saved", ""] {
                ui.label(RichText::new(h).color(theme::MUTED));
            }
            ui.end_row();
            for (week, total, saved) in sheets {
                if ui.link(week_label(week)).clicked() {
                    app.go_to_week(week);
                    app.tab = Tab::Week;
                }
                ui.label(format!("{} h", fmt_h(total)));
                ui.label(saved);
                ui.horizontal(|ui| {
                    if ui.button("PDF").clicked() {
                        app.export_pdf_timesheet(week);
                    }
                    if ui.button("Excel").clicked() {
                        let report = app.data.report(week, add_days(week, 6), true);
                        let name = format!("Timesheet {}.xlsx", week.format("%Y-%m-%d"));
                        let bytes = exports::report_xlsx(&app.data, &report);
                        app.deliver(&name, bytes, true);
                    }
                });
                ui.end_row();
            }
            ui.label(RichText::new("Total").strong());
            ui.label(RichText::new(format!("{} h", fmt_h(grand))).strong());
            ui.end_row();
        });
    });

    if !app.data.revisions.is_empty() {
        ui.add_space(10.0);
        card(ui, |ui| {
            egui::CollapsingHeader::new(RichText::new(format!("Earlier versions of reopened timesheets ({})", app.data.revisions.len())).strong())
                .default_open(false)
                .show(ui, |ui| {
                    ui.label(RichText::new("When a saved week is reopened, the saved version is kept here so no record is lost.").color(theme::MUTED));
                    let revisions: Vec<(usize, NaiveDate, u32, String, String)> = app
                        .data
                        .revisions
                        .iter()
                        .enumerate()
                        .rev()
                        .map(|(i, r)| (i, r.sheet.week, r.sheet.total(), date_time(r.sheet.saved_at), date_time(r.reopened_at)))
                        .collect();
                    egui::Grid::new("revisions").striped(true).num_columns(4).spacing([20.0, 6.0]).show(ui, |ui| {
                        for h in ["Week", "Hours", "Saved", "Reopened", ""] {
                            ui.label(RichText::new(h).color(theme::MUTED));
                        }
                        ui.end_row();
                        for (i, week, total, saved, reopened) in revisions {
                            ui.label(week_label(week));
                            ui.label(format!("{} h", fmt_h(total)));
                            ui.label(saved);
                            ui.label(reopened);
                            if ui.button("PDF").clicked() {
                                let bytes = pdf::revision_pdf(&app.data, &app.data.revisions[i]);
                                app.deliver(&format!("Timesheet {} (earlier version).pdf", week.format("%Y-%m-%d")), Ok(bytes), true);
                            }
                            ui.end_row();
                        }
                    });
                });
        });
    }

    if let (Some(first), Some(last)) = (app.data.timesheets.keys().next().copied(), app.data.timesheets.keys().last().copied()) {
        ui.add_space(10.0);
        let report = app.data.report(first, add_days(last, 6), true);
        card(ui, |ui| {
            ui.label(RichText::new(format!("Total time per project (all saved timesheets, {})", report.label())).strong());
            share_table(ui, &report);
        });
    }
}

fn share_table(ui: &mut Ui, report: &Report) {
    egui::Grid::new(("share", report.start)).striped(true).num_columns(3).spacing([20.0, 6.0]).show(ui, |ui| {
        for p in &report.projects {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(10.0), egui::Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::same(2), theme::project(p.color));
                ui.label(p.display());
            });
            ui.label(format!("{} h", fmt_h(p.hh)));
            let (bar, _) = ui.allocate_exact_size(egui::vec2(220.0, 12.0), egui::Sense::hover());
            ui.painter().rect_filled(bar, CornerRadius::same(6), theme::TINT);
            let fill = egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * (p.share as f32 / 100.0), bar.height()));
            ui.painter().rect_filled(fill, CornerRadius::same(6), theme::project(p.color));
            ui.painter().text(bar.right_center() + egui::vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, format!("{:.0}%", p.share), egui::FontId::proportional(12.0), theme::MUTED);
            ui.end_row();
        }
        ui.label(RichText::new("Total").strong());
        ui.label(RichText::new(format!("{} h", fmt_h(report.total()))).strong());
        ui.end_row();
    });
}

// --------------------------------------------------------------------------
//  Reports
// --------------------------------------------------------------------------
fn reports_tab(app: &mut App, ui: &mut Ui) {
    page_title(ui, "Reports");
    ui.label(RichText::new("A totalled-out timesheet for any period.").color(theme::MUTED));
    ui.add_space(6.0);
    card(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label("From");
            ui.add(egui_extras::DatePickerButton::new(&mut app.report_range.0).id_salt("from"));
            ui.label("to");
            ui.add(egui_extras::DatePickerButton::new(&mut app.report_range.1).id_salt("to"));
            ui.checkbox(&mut app.report_saved_only, "Saved timesheets only");
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Quick:").color(theme::MUTED));
            for (name, s, e) in model::quick_ranges(model::today()) {
                if ui.small_button(name).clicked() {
                    app.report_range = (s, e);
                }
            }
        });
    });
    let report = app.report();
    app.report_range = (report.start, report.end);
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        for (label, value) in [
            ("Total hours", fmt_h(report.total())),
            ("Projects", report.projects.len().to_string()),
            ("Days with time", report.days_worked().to_string()),
            ("Average per day", report.avg_per_day()),
        ] {
            egui::Frame::default().fill(theme::CARD).stroke(Stroke::new(1.0_f32, theme::BORDER)).corner_radius(CornerRadius::same(12)).inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
                ui.set_min_width(140.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(label.to_uppercase()).size(11.0).color(theme::MUTED));
                    ui.label(RichText::new(value).size(22.0).strong());
                });
            });
        }
    });
    if report.unsaved_hh > 0 {
        ui.add_space(6.0);
        if report.saved_only {
            banner(ui, &format!("{} h from weeks that aren't saved yet are left out.", fmt_h(report.unsaved_hh)), false);
        } else {
            banner(ui, &format!("Includes {} h from weeks not yet saved as timesheets (marked “not saved”). Tick “Saved timesheets only” for records only.", fmt_h(report.unsaved_hh)), true);
        }
    }
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        let stem = format!("Timesheet {} to {}", report.start.format("%Y-%m-%d"), report.end.format("%Y-%m-%d"));
        if primary(ui, "📄 Print / PDF").clicked() {
            let bytes = pdf::report_pdf(&app.data, &report, false);
            app.deliver(&format!("{stem}.pdf"), Ok(bytes), true);
        }
        if ui.button("PDF with daily detail").clicked() {
            let bytes = pdf::report_pdf(&app.data, &report, true);
            app.deliver(&format!("{stem} (detail).pdf"), Ok(bytes), true);
        }
        if ui.button("Excel").clicked() {
            let bytes = exports::report_xlsx(&app.data, &report);
            app.deliver(&format!("{stem}.xlsx"), bytes, true);
        }
        if ui.button("CSV (totals)").clicked() {
            app.deliver(&format!("{stem} totals.csv"), Ok(exports::report_csv_totals(&report)), false);
        }
        if ui.button("CSV (daily)").clicked() {
            app.deliver(&format!("{stem} daily.csv"), Ok(exports::report_csv_daily(&report)), false);
        }
    });
    ui.add_space(8.0);

    if report.projects.is_empty() {
        ui.label(RichText::new("No time recorded in this period.").color(theme::MUTED));
        return;
    }
    let m = &report.matrix;
    card(ui, |ui| {
        ui.label(RichText::new(format!("Timesheet — hours per project per {}", m.unit)).strong());
        egui::ScrollArea::horizontal().id_salt("matrix").show(ui, |ui| {
            egui::Grid::new("matrix-grid").striped(true).num_columns(m.columns.len() + 3).spacing([16.0, 6.0]).show(ui, |ui| {
                ui.label(RichText::new("Project").color(theme::MUTED));
                for c in &m.columns {
                    ui.label(RichText::new(format!("{}{}", c.label, if c.partial { "*" } else { "" })).color(theme::MUTED))
                        .on_hover_text(format!("{} – {}", long_date(c.start), long_date(c.end)));
                }
                ui.label(RichText::new("Total").color(theme::MUTED));
                ui.label(RichText::new("Share").color(theme::MUTED));
                ui.end_row();
                for (p, cells) in report.projects.iter().zip(&m.cells) {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(10.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, CornerRadius::same(2), theme::project(p.color));
                        ui.label(p.display());
                    });
                    for &hh in cells {
                        ui.label(if hh > 0 { fmt_h(hh) } else { String::new() });
                    }
                    ui.label(RichText::new(fmt_h(p.hh)).strong());
                    ui.label(format!("{:.0}%", p.share));
                    ui.end_row();
                }
                ui.label(RichText::new("Total").strong());
                for hh in m.column_totals() {
                    ui.label(RichText::new(fmt_h(hh)).strong());
                }
                ui.label(RichText::new(fmt_h(report.total())).strong());
                ui.end_row();
            });
        });
        if m.has_partial() {
            ui.label(RichText::new(format!("* only part of this {} falls inside the period", m.unit)).size(12.0).color(theme::MUTED));
        }
    });
    ui.add_space(8.0);
    card(ui, |ui| {
        let weeks = report.active_weeks();
        ui.label(RichText::new("Weeks").strong());
        egui::Grid::new("weeks").striped(true).num_columns(3).spacing([24.0, 6.0]).show(ui, |ui| {
            for w in &weeks {
                if ui.link(format!("{}{}", week_label(w.start), if w.partial { " (part)" } else { "" })).clicked() {
                    app.go_to_week(w.start);
                    app.tab = Tab::Week;
                }
                if w.saved {
                    ui.label(RichText::new("Saved").color(theme::OK));
                } else {
                    ui.label(RichText::new("Not saved").color(theme::UNDER));
                }
                ui.label(format!("{} h", fmt_h(w.hh)));
                ui.end_row();
            }
        });
        let empty = report.weeks.len() - weeks.len();
        if empty > 0 {
            ui.label(RichText::new(format!("{empty} week{} in this period with no time not listed.", if empty == 1 { "" } else { "s" })).size(12.0).color(theme::MUTED));
        }
    });
    ui.add_space(8.0);
    card(ui, |ui| {
        egui::CollapsingHeader::new(RichText::new(format!("Daily detail ({} entries)", report.lines.len())).strong()).show(ui, |ui| {
            egui::Grid::new("detail").striped(true).num_columns(4).spacing([20.0, 4.0]).show(ui, |ui| {
                for l in &report.lines {
                    ui.label(format!("{} {}", model::DAY_NAMES[chrono::Datelike::weekday(&l.day).num_days_from_monday() as usize], long_date(l.day)));
                    ui.label(&l.name);
                    ui.label(if l.saved { "Saved" } else { "Not saved" });
                    ui.label(format!("{} h", fmt_h(l.hh)));
                    ui.end_row();
                }
            });
        });
    });
}

// --------------------------------------------------------------------------
//  Settings
// --------------------------------------------------------------------------
/// A time-of-day picker in 15-minute steps between `min` and `max` (minutes).
fn time_pick(ui: &mut Ui, id: impl std::hash::Hash, value: &mut u16, min: u16, max: u16) -> bool {
    let before = *value;
    egui::ComboBox::from_id_salt(id).selected_text(model::clock(*value)).width(72.0).height(260.0).show_ui(ui, |ui| {
        for m in (min..=max).step_by(15) {
            ui.selectable_value(value, m, model::clock(m));
        }
    });
    *value != before
}

/// Per-day working hours with an optional lunch break. Drives the work lines and day targets.
fn working_hours(app: &mut App, ui: &mut Ui) {
    ui.label(RichText::new("Working hours").strong());
    ui.label(
        RichText::new("Shown as green lines on the week timeline; lunch is shaded. Day totals are compared with the hours worked (lunch excluded).")
            .size(12.0)
            .color(theme::MUTED),
    );
    ui.add_space(4.0);
    const NAMES: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    let mut update: Option<(usize, Option<model::DayHours>)> = None;
    egui::Grid::new("work-hours").num_columns(5).spacing([14.0, 8.0]).striped(true).show(ui, |ui| {
        ui.label(RichText::new("Day").color(theme::MUTED));
        ui.label(RichText::new("Hours").color(theme::MUTED));
        ui.label(RichText::new("Lunch").color(theme::MUTED));
        ui.label(RichText::new("Work time").color(theme::MUTED));
        ui.end_row();
        for (day, name) in NAMES.iter().enumerate() {
            let current = app.data.settings.hours[day];
            let mut works = current.is_some();
            if ui.checkbox(&mut works, *name).changed() {
                let default = model::default_week_hours()[day.min(4)];
                update = Some((day, if works { default } else { None }));
            }
            match current {
                Some(mut h) => {
                    let mut dirty = false;
                    ui.horizontal(|ui| {
                        dirty |= time_pick(ui, ("ws", day), &mut h.start, 0, h.end.saturating_sub(30));
                        ui.label("to");
                        dirty |= time_pick(ui, ("we", day), &mut h.end, h.start + 30, 24 * 60);
                    });
                    let mut lunch = h.lunch.is_some();
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut lunch, "").on_hover_text("Lunch break").changed() {
                            let mid = (h.start + h.end) / 2 / 15 * 15;
                            h.lunch = lunch.then_some((mid, (mid + 60).min(h.end)));
                            dirty = true;
                        }
                        if let Some((mut ls, mut le)) = h.lunch {
                            dirty |= time_pick(ui, ("ls", day), &mut ls, h.start, le.saturating_sub(15).max(h.start));
                            ui.label("to");
                            dirty |= time_pick(ui, ("le", day), &mut le, ls + 15, h.end);
                            h.lunch = Some((ls, le));
                        }
                    });
                    let m = h.working_minutes();
                    ui.label(format!("{}h{:02}", m / 60, m % 60));
                    if dirty && update.is_none() {
                        // keep lunch inside the day when the day shrinks
                        if let Some((ls, le)) = h.lunch {
                            let ls = ls.clamp(h.start, h.end.saturating_sub(15));
                            h.lunch = Some((ls, le.clamp(ls + 15, h.end)));
                        }
                        update = Some((day, Some(h)));
                    }
                }
                None => {
                    ui.label(RichText::new("Not a work day").color(theme::MUTED));
                    ui.label("");
                    ui.label("");
                }
            }
            ui.end_row();
        }
    });
    let total: u32 = app.data.settings.hours.iter().flatten().map(|h| u32::from(h.working_minutes())).sum();
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("Week: {}h{:02}", total / 60, total % 60)).strong());
        if app.data.settings.hours != model::default_week_hours() && ui.button("Reset to office hours").clicked() {
            app.data.settings.hours = model::default_week_hours();
            app.changed();
        }
    });
    if let Some((day, hours)) = update {
        match app.data.set_day_hours(day, hours) {
            Ok(()) => app.changed(),
            Err(e) => app.flash(&e, true),
        }
    }
}

fn settings_tab(app: &mut App, ui: &mut Ui) {
    page_title(ui, "Settings");
    ui.add_space(6.0);
    card(ui, |ui| {
        ui.label(RichText::new("You").strong());
        let s = &mut app.data.settings;
        let mut changed = false;
        egui::Grid::new("you").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label("Your name (printed on documents)");
            changed |= ui.add(egui::TextEdit::singleline(&mut s.user_name).char_limit(120).desired_width(260.0)).changed();
            ui.end_row();
            ui.label("Company (optional)");
            changed |= ui.add(egui::TextEdit::singleline(&mut s.company).char_limit(120).desired_width(260.0)).changed();
            ui.end_row();
            ui.label("Weekend");
            changed |= ui.checkbox(&mut s.show_weekend, "Show Saturday and Sunday").changed();
            ui.end_row();
        });
        ui.label(RichText::new("Changes are saved automatically.").size(12.0).color(theme::MUTED));
        if changed {
            app.changed();
        }
    });
    ui.add_space(10.0);
    card(ui, |ui| working_hours(app, ui));
    ui.add_space(10.0);
    card(ui, |ui| {
        ui.label(RichText::new("Your data").strong());
        ui.label(RichText::new("Everything is stored on this computer in one file — nothing is sent anywhere.").color(theme::MUTED));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Data folder:");
            ui.monospace(app.dir.display().to_string());
        });
        ui.horizontal(|ui| {
            ui.label("Exports go to:");
            ui.monospace(exports::export_dir().display().to_string());
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if primary(ui, "Back up now").clicked() {
                app.save_now();
                match store::make_backup(&app.dir) {
                    Ok(path) => app.flash(&format!("Backup saved: {}", path.file_name().unwrap_or_default().to_string_lossy()), false),
                    Err(e) => app.flash(&e, true),
                }
            }
            if ui.button("Open data folder").clicked() {
                exports::open_path(&app.dir);
            }
            if ui.button("Open exports folder").clicked() {
                let dir = exports::export_dir();
                let _ = std::fs::create_dir_all(&dir);
                exports::open_path(&dir);
            }
        });
        let backups = store::list_backups(&app.dir);
        ui.add_space(6.0);
        ui.label(format!("{} backup{} kept (one is made automatically each day you use the app; the newest 30 are kept).", backups.len(), if backups.len() == 1 { "" } else { "s" }));
        if !backups.is_empty() {
            egui::ScrollArea::vertical().id_salt("backups").max_height(220.0).show(ui, |ui| {
                egui::Grid::new("backups-grid").striped(true).num_columns(4).spacing([20.0, 4.0]).show(ui, |ui| {
                    for b in backups {
                        ui.monospace(&b.name);
                        ui.label(date_time(b.modified));
                        ui.label(format!("{} KB", (b.size + 1023) / 1024));
                        if ui.button("Restore").clicked() {
                            app.confirm = Some(Confirm::Restore(b.name.clone()));
                        }
                        ui.end_row();
                    }
                });
            });
        }
        ui.label(RichText::new("Restoring first makes a safety backup of your current data, so a restore can always be undone.").size(12.0).color(theme::MUTED));
    });
    ui.add_space(10.0);
    ui.label(RichText::new(format!("Tiny Time Track Lite {}", env!("CARGO_PKG_VERSION"))).color(theme::MUTED));
}

// --------------------------------------------------------------------------
//  RFI register (window)
// --------------------------------------------------------------------------
fn rfi_window(app: &mut App, ctx: &egui::Context) {
    let Some(project_id) = app.rfi_project else { return };
    let Some(project) = app.data.project(project_id).cloned() else {
        app.rfi_project = None;
        return;
    };
    let today = model::today();
    let mut open = true;
    egui::Window::new(format!("RFI register — {}", project.label()))
        .id(egui::Id::new("rfi-window"))
        .open(&mut open)
        .collapsible(false)
        .default_size([820.0, 560.0])
        .show(ctx, |ui| {
            let (open_count, overdue) = app.data.rfi_counts(project_id, today);
            ui.horizontal(|ui| {
                ui.label(RichText::new(model::stage_label(project.stage)).color(theme::MUTED));
                ui.separator();
                ui.label(format!("{open_count} open"));
                if overdue > 0 {
                    ui.label(RichText::new(format!("{overdue} overdue")).color(theme::OVER).strong());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("📄 Register (PDF)").clicked() {
                        let bytes = pdf::rfi_register_pdf(&app.data, project_id);
                        let who = if project.code.is_empty() { project.name.clone() } else { project.code.clone() };
                        let name: String =
                            who.chars().map(|c| if c.is_alphanumeric() || " -_".contains(c) { c } else { '-' }).collect();
                        app.deliver(&format!("RFI register {name}.pdf"), Ok(bytes), true);
                    }
                });
            });
            ui.add_space(4.0);

            if app.data.is_on_site(project_id) {
                card(ui, |ui| {
                    ui.label(RichText::new("Raise an RFI").strong());
                    let f = &mut app.rfi_form;
                    ui.horizontal(|ui| {
                        ui.label("Subject");
                        ui.add(
                            egui::TextEdit::singleline(&mut f.subject)
                                .hint_text("e.g. Window sill height, north elevation")
                                .desired_width(420.0),
                        );
                    });
                    ui.add(
                        egui::TextEdit::multiline(&mut f.details)
                            .hint_text("Question / details (optional)")
                            .desired_rows(2)
                            .desired_width(f32::INFINITY),
                    );
                    ui.horizontal(|ui| {
                        ui.label("Raised");
                        ui.add(egui_extras::DatePickerButton::new(&mut f.raised).id_salt("rfi-raised"));
                        ui.checkbox(&mut f.has_due, "Reply needed by");
                        if f.has_due {
                            ui.add(egui_extras::DatePickerButton::new(&mut f.due).id_salt("rfi-due"));
                        }
                    });
                    if primary(ui, "Add RFI").clicked() {
                        let (subject, details, raised) = (f.subject.clone(), f.details.clone(), f.raised);
                        let due = f.has_due.then_some(f.due);
                        match app.data.add_rfi(project_id, &subject, &details, raised, due) {
                            Ok(n) => {
                                app.rfi_form = RfiForm::default();
                                app.changed();
                                app.flash(&format!("RFI {n} added."), false);
                            }
                            Err(e) => app.flash(&e, true),
                        }
                    }
                });
            } else {
                banner(
                    ui,
                    &format!(
                        "This project is at {} — not on site, so no new RFIs. The register below is kept.",
                        model::stage_label(project.stage)
                    ),
                    false,
                );
            }
            ui.add_space(6.0);

            let rfis: Vec<model::Rfi> = app.data.rfis_for(project_id).into_iter().cloned().collect();
            if rfis.is_empty() {
                ui.label(RichText::new("No RFIs yet.").color(theme::MUTED));
                return;
            }
            egui::ScrollArea::vertical().id_salt("rfi-list").show(ui, |ui| {
                egui::Grid::new("rfi-grid").striped(true).num_columns(6).spacing([14.0, 6.0]).show(ui, |ui| {
                    for h in ["No.", "Subject", "Raised", "Reply by", "Status", ""] {
                        ui.label(RichText::new(h).color(theme::MUTED));
                    }
                    ui.end_row();
                    for r in &rfis {
                        ui.label(RichText::new(format!("RFI {}", r.number)).strong());
                        ui.vertical(|ui| {
                            ui.set_width(360.0); // wrap at a readable width, not per letter
                            ui.label(&r.subject);
                            if !r.details.is_empty() {
                                ui.label(RichText::new(&r.details).size(12.5).color(theme::MUTED));
                            }
                            if let Some(on) = r.answered {
                                let text = if r.response.is_empty() {
                                    format!("Answered {}", long_date(on))
                                } else {
                                    format!("Answer ({}): {}", long_date(on), r.response)
                                };
                                ui.label(RichText::new(text).size(12.5).color(theme::OK));
                            }
                        });
                        ui.label(long_date(r.raised));
                        ui.label(r.due.map(long_date).unwrap_or_else(|| "—".into()));
                        let status = r.status(today);
                        let color = match status {
                            "Answered" => theme::OK,
                            "Overdue" => theme::OVER,
                            _ => theme::UNDER,
                        };
                        ui.label(RichText::new(status).color(color).strong());
                        ui.horizontal(|ui| {
                            if r.is_open() {
                                if ui.button("Answer…").clicked() {
                                    app.rfi_answer = Some((r.id, String::new(), today));
                                }
                            } else if ui.button("Reopen").clicked() {
                                app.data.reopen_rfi(r.id);
                                app.changed();
                            }
                            if ui.button("Delete").clicked() {
                                app.confirm = Some(Confirm::DeleteRfi(r.id));
                            }
                        });
                        ui.end_row();

                        if app.rfi_answer.as_ref().is_some_and(|a| a.0 == r.id) {
                            ui.label("");
                            let mut save = false;
                            let mut cancel = false;
                            ui.vertical(|ui| {
                                let answer = app.rfi_answer.as_mut().expect("answer in progress");
                                ui.add(
                                    egui::TextEdit::multiline(&mut answer.1)
                                        .hint_text("Response (optional)")
                                        .desired_rows(2)
                                        .desired_width(380.0),
                                );
                                ui.horizontal(|ui| {
                                    ui.label("Answered on");
                                    ui.add(egui_extras::DatePickerButton::new(&mut answer.2).id_salt(&format!("rfi-answer-{}", r.id)));
                                    save = primary(ui, "Mark answered").clicked();
                                    cancel = ui.button("Cancel").clicked();
                                });
                            });
                            ui.end_row();
                            if save {
                                let (id, response, on) = app.rfi_answer.clone().expect("answer in progress");
                                match app.data.answer_rfi(id, on, &response) {
                                    Ok(()) => {
                                        app.rfi_answer = None;
                                        app.changed();
                                        app.flash(&format!("RFI {} answered.", r.number), false);
                                    }
                                    Err(e) => app.flash(&e, true),
                                }
                            }
                            if cancel {
                                app.rfi_answer = None;
                            }
                        }
                    }
                });
            });
        });
    if !open {
        app.rfi_project = None;
        app.rfi_answer = None;
    }
}
