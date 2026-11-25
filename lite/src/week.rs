//! The Week tab: a day-by-day timeline with a Plan and an Actual lane per day.
//!
//! Mouse: drag on empty time to paint a block with the chosen project, drag a
//! block's edge to extend/shrink it, drag its middle to move it, click empty
//! time to add half an hour, right-click a block for options.
//! Keyboard (selected block): ←/→ move, Shift+←/→ change the end, Delete removes.

use chrono::NaiveDate;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
    pos2, vec2,
};

use crate::model::{
    self, DAY_NAMES, Lane, SLOTS, add_days, create_span, day_month, fmt_h, move_span, resize_span, slot_time,
    week_dates, week_label,
};
use crate::ui::{self, App, Confirm, theme};

const LANE_H: f32 = 30.0;
const LABEL_W: f32 = 96.0;
const TAG_W: f32 = 54.0;
const TOTAL_W: f32 = 74.0;
const EDGE_PX: f32 = 7.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Part {
    Start,
    End,
    Body,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DragKind {
    Create { anchor: u8 },
    Resize { start_edge: bool },
    Move { grab: i32 },
}

#[derive(Clone, Debug)]
pub struct Drag {
    pub id: u64,
    pub date: NaiveDate,
    pub lane: Lane,
    pub kind: DragKind,
    pub orig: (u8, u8),
    pub free: (u8, u8),
    pub before: Vec<model::Block>,
}

/// Where a lane was drawn last frame (lets tests aim simulated mouse input).
#[derive(Clone, Debug)]
#[cfg_attr(not(test), allow(dead_code))] // read by the headless UI tests
pub struct LaneGeo {
    pub date: NaiveDate,
    pub lane: Lane,
    pub rect: Rect,
    pub x0: f32,
    pub slot_w: f32,
    pub lo: u8,
}

#[cfg_attr(not(test), allow(dead_code))]
impl LaneGeo {
    /// Screen point in the middle of `slot`.
    pub fn point(&self, slot: u8) -> Pos2 {
        pos2(self.x0 + (f32::from(slot - self.lo) + 0.5) * self.slot_w, self.rect.center().y)
    }
    /// Screen point on the boundary before `slot`.
    pub fn edge(&self, slot: u8) -> Pos2 {
        pos2(self.x0 + f32::from(slot - self.lo) * self.slot_w, self.rect.center().y)
    }
}

/// Maps between screen x and half-hour slots for the visible range.
#[derive(Clone, Copy)]
struct Geo {
    x0: f32,
    slot_w: f32,
    lo: u8,
    hi: u8,
}

impl Geo {
    fn x(&self, slot: u8) -> f32 {
        self.x0 + f32::from(slot.saturating_sub(self.lo)) * self.slot_w
    }
    /// x for a time of day in minutes, clamped to the visible range.
    fn x_min(&self, minutes: u16) -> f32 {
        let slot = (f32::from(minutes) / 30.0).clamp(f32::from(self.lo), f32::from(self.hi));
        self.x0 + (slot - f32::from(self.lo)) * self.slot_w
    }
    /// The slot under x (for painting and moving).
    fn slot_at(&self, x: f32) -> u8 {
        let raw = ((x - self.x0) / self.slot_w).floor() as i32 + i32::from(self.lo);
        raw.clamp(i32::from(self.lo), i32::from(self.hi) - 1) as u8
    }
    /// The nearest slot boundary to x (for resizing).
    fn boundary_at(&self, x: f32) -> u8 {
        let raw = ((x - self.x0) / self.slot_w).round() as i32 + i32::from(self.lo);
        raw.clamp(i32::from(self.lo), i32::from(self.hi)) as u8
    }
}

fn hit_test(blocks: &[(u64, u8, u8)], geo: &Geo, x: f32) -> Option<(u64, Part)> {
    for &(id, start, end) in blocks {
        let (left, right) = (geo.x(start), geo.x(end));
        if x >= left && x <= right {
            let edge = EDGE_PX.min((right - left) / 3.0);
            return Some((
                id,
                if x - left <= edge {
                    Part::Start
                } else if right - x <= edge {
                    Part::End
                } else {
                    Part::Body
                },
            ));
        }
    }
    None
}

// --------------------------------------------------------------------------
//  The tab
// --------------------------------------------------------------------------
pub fn show(app: &mut App, ui: &mut Ui) {
    let week = app.week;
    let locked = app.data.is_locked(week);
    header(app, ui, week, locked);
    ui.add_space(4.0);
    if app.data.projects.is_empty() {
        onboarding(app, ui);
        return;
    }
    palette(app, ui, locked);
    ui.add_space(6.0);
    timeline(app, ui, week, locked);
    ui.add_space(4.0);
    selected_panel(app, ui, locked);
    ui.add_space(8.0);
    todos(app, ui, week);
    ui.add_space(8.0);
    ui.columns(2, |cols| {
        summary(app, &mut cols[0], week);
        actions(app, &mut cols[1], week, locked);
    });
}

fn header(app: &mut App, ui: &mut Ui, week: NaiveDate, locked: bool) {
    ui.horizontal(|ui| {
        let this_week = model::monday_of(model::today());
        ui::page_title(ui, if week == this_week { "This week" } else { "Week" });
        ui.add_space(12.0);
        if ui.button("◀").on_hover_text("Previous week (Page Up)").clicked() {
            app.go_to_week(add_days(week, -7));
        }
        let mut pick = week;
        let picker = egui_extras::DatePickerButton::new(&mut pick).id_salt("week-pick").show_icon(true);
        if ui.add(picker).on_hover_text("Go to the week containing a date").changed() {
            app.go_to_week(model::monday_of(pick));
        }
        if ui.button("▶").on_hover_text("Next week (Page Down)").clicked() {
            app.go_to_week(add_days(week, 7));
        }
        if week != this_week && ui.button("Today").clicked() {
            app.go_to_week(this_week);
        }
        ui.label(RichText::new(week_label(week)).size(15.0).color(theme::MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if locked {
                let saved = app.data.timesheets.get(&week).map(|t| model::date_time(t.saved_at)).unwrap_or_default();
                ui::chip(ui, &format!("✔ Timesheet saved {saved}"), theme::OK, theme::OK_SOFT);
                let earlier = app.data.revisions_for(week).len();
                if earlier > 0 {
                    let text = format!("Revision {} · see earlier version{}", earlier + 1, if earlier == 1 { "" } else { "s" });
                    if ui.link(text).clicked() {
                        app.tab = ui::Tab::Timesheets;
                    }
                }
            } else if app.data.day_total_week(week, Lane::Actual) > 0 {
                ui::chip(ui, "Timesheet not saved yet", theme::MUTED, theme::TINT);
            }
        });
    });
    if locked {
        ui::banner(ui, "This week is saved as a timesheet, so it's read-only. Use “Reopen timesheet” below to change it.", false);
    }
}

fn onboarding(app: &mut App, ui: &mut Ui) {
    ui::card(ui, |ui| {
        ui.heading("Start by adding a project");
        ui.label(RichText::new("Projects are what you plan and record time against — a job, a client, or just “Admin”.").color(theme::MUTED));
        ui.add_space(6.0);
        ui::project_form(app, ui, "onboard");
    });
}

fn palette(app: &mut App, ui: &mut Ui, locked: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(if locked { "Projects:" } else { "Paint with:" }).strong());
        let projects: Vec<(u32, String, u8)> =
            app.data.active_projects().iter().map(|p| (p.id, p.label(), p.color)).collect();
        if app.brush.is_none_or(|b| !projects.iter().any(|p| p.0 == b)) {
            app.brush = projects.first().map(|p| p.0);
        }
        for (id, label, color) in projects {
            let selected = app.brush == Some(id);
            let fill = theme::project(color);
            let text = RichText::new(format!("  {label}  ")).color(if selected { theme::text_on(fill) } else { theme::TEXT });
            let button = egui::Button::new(text)
                .fill(if selected { fill } else { theme::tint(fill, 0.82) })
                .stroke(Stroke::new(if selected { 2.0_f32 } else { 1.0 }, fill));
            let response = ui.add(button).on_hover_text(if selected {
                "Painting with this project — drag on the timeline"
            } else {
                "Click to paint with this project"
            });
            if response.clicked() {
                app.brush = Some(id);
            }
        }
        ui.add_space(8.0);
        ui.menu_button("+ Add project", |ui| {
            ui::project_form(app, ui, "palette");
        });
    });
}

fn timeline(app: &mut App, ui: &mut Ui, week: NaiveDate, locked: bool) {
    let (lo, hi) = app.data.visible_slots(week);
    let days = app.data.visible_days(week);
    let dates = week_dates(week);
    let today = model::today();
    let full_w = ui.available_width();
    let area_w = (full_w - LABEL_W - TAG_W - TOTAL_W).max(200.0);
    app.lanes.clear();

    // Hour ruler
    let (ruler, _) = ui.allocate_exact_size(vec2(full_w, 22.0), Sense::hover());
    let geo = Geo { x0: ruler.left() + LABEL_W + TAG_W, slot_w: area_w / f32::from(hi - lo), lo, hi };
    let painter = ui.painter().clone();
    // Half-hour scale: every slot gets a tick and a label (":30" when space is tight)
    let tight = geo.slot_w < 36.0;
    for slot in lo..=hi {
        let x = geo.x(slot);
        let hour = slot % 2 == 0;
        let label = if hour || !tight { slot_time(slot) } else { ":30".to_string() };
        painter.text(
            pos2(x, ruler.center().y - 2.0),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(if hour { 11.5 } else { 10.0 }),
            if hour { theme::TEXT } else { theme::MUTED },
        );
        let tick = if hour { 5.0 } else { 3.0 };
        painter.vline(x, (ruler.bottom() - tick)..=ruler.bottom(), Stroke::new(1.0_f32, if hour { theme::MUTED } else { theme::GRID }));
    }
    painter.text(pos2(ruler.right() - TOTAL_W / 2.0, ruler.center().y), Align2::CENTER_CENTER, "Total", FontId::proportional(11.0), theme::MUTED);

    for &i in &days {
        let date = dates[i];
        ui.add_space(3.0);
        let day_top = ui.cursor().top();
        for lane in [Lane::Plan, Lane::Actual] {
            let (row, _) = ui.allocate_exact_size(vec2(full_w, LANE_H), Sense::hover());
            let lane_rect = Rect::from_min_max(pos2(geo.x0, row.top() + 2.0), pos2(geo.x0 + area_w, row.bottom() - 2.0));
            app.lanes.push(LaneGeo { date, lane, rect: lane_rect, x0: geo.x0, slot_w: geo.slot_w, lo: geo.lo });
            lane_row(app, ui, &painter, &geo, date, lane, row, lane_rect, locked, today);
        }
        // Day label spanning both lanes
        let day_rect = Rect::from_min_max(pos2(ruler.left(), day_top), pos2(ruler.left() + LABEL_W - 6.0, ui.cursor().top()));
        let is_today = date == today;
        if is_today {
            painter.rect_filled(day_rect.shrink(1.0), CornerRadius::same(6), theme::SOFT);
        }
        painter.text(
            pos2(day_rect.left() + 8.0, day_rect.center().y - 14.0),
            Align2::LEFT_CENTER,
            DAY_NAMES[i],
            FontId::proportional(15.0),
            if is_today { theme::ACCENT_DARK } else { theme::TEXT },
        );
        painter.text(
            pos2(day_rect.left() + 8.0, day_rect.center().y + 2.0),
            Align2::LEFT_CENTER,
            format!("{}{}", day_month(date), if is_today { " · today" } else { "" }),
            FontId::proportional(12.0),
            theme::MUTED,
        );
        let hours_text = app
            .data
            .day_hours(date)
            .map_or("Not a work day".to_string(), |h| format!("{}–{}", model::clock(h.start), model::clock(h.end)));
        painter.text(
            pos2(day_rect.left() + 8.0, day_rect.center().y + 17.0),
            Align2::LEFT_CENTER,
            hours_text,
            FontId::proportional(11.0),
            theme::ACCENT_DARK,
        );
        ui.add_space(3.0);
        painter.hline(ruler.left()..=ruler.right(), ui.cursor().top() - 1.0, Stroke::new(1.0_f32, theme::BORDER));
    }

    ui.add_space(4.0);
    ui.label(
        RichText::new(if locked {
            "Read-only. Hover a block to see its times."
        } else {
            "Drag on empty time to add · drag an edge to extend or shrink · drag the middle to move · click for half an hour · right-click for options · Ctrl+Z undo"
        })
        .size(12.0)
        .color(theme::MUTED),
    );
}

#[allow(clippy::too_many_arguments)]
fn lane_row(
    app: &mut App,
    ui: &mut Ui,
    painter: &egui::Painter,
    geo: &Geo,
    date: NaiveDate,
    lane: Lane,
    row: Rect,
    rect: Rect,
    locked: bool,
    today: NaiveDate,
) {
    // ---- paint ----
    let plan = lane == Lane::Plan;
    painter.text(
        pos2(row.left() + LABEL_W + 4.0, row.center().y),
        Align2::LEFT_CENTER,
        lane.label(),
        FontId::proportional(12.0),
        if plan { theme::MUTED } else { theme::ACCENT_DARK },
    );
    let bg = if plan { theme::CARD } else { theme::TINT };
    painter.rect_filled(rect, CornerRadius::same(5), bg);
    for slot in geo.lo..=geo.hi {
        let x = geo.x(slot);
        let stroke = if slot % 2 == 0 { Stroke::new(1.0_f32, theme::FIELD_BORDER) } else { Stroke::new(1.0_f32, theme::GRID) };
        painter.vline(x, rect.y_range(), stroke);
    }
    // Working hours: shade time outside them, mark lunch.
    let hours = app.data.day_hours(date);
    let (left, right) = (geo.x(geo.lo), geo.x(geo.hi));
    let outside = Color32::from_rgba_unmultiplied(0x6B, 0x52, 0x46, 26);
    match hours {
        Some(h) => {
            let (ws, we) = (geo.x_min(h.start), geo.x_min(h.end));
            for (a, b) in [(left, ws), (we, right)] {
                if b > a {
                    painter.rect_filled(Rect::from_x_y_ranges(a..=b, rect.y_range()), CornerRadius::ZERO, outside);
                }
            }
            if let Some((ls, le)) = h.lunch {
                let r = Rect::from_x_y_ranges(geo.x_min(ls)..=geo.x_min(le), rect.y_range());
                painter.rect_filled(r, CornerRadius::ZERO, theme::LUNCH);
                for k in 0..((r.width() / 6.0) as i32 + 4) {
                    // light diagonal stripes so lunch reads as "not work time"
                    let x = r.left() + k as f32 * 6.0 - r.height();
                    painter.with_clip_rect(r).line_segment(
                        [pos2(x, r.bottom()), pos2(x + r.height(), r.top())],
                        Stroke::new(1.0_f32, theme::LUNCH_STRIPE),
                    );
                }
                if plan && r.width() > 34.0 {
                    painter.text(r.center(), Align2::CENTER_CENTER, "Lunch", FontId::proportional(10.5), theme::UNDER);
                }
            }
        }
        None => {
            painter.rect_filled(rect, CornerRadius::same(5), outside); // not a working day
        }
    }

    let blocks: Vec<(u64, u8, u8, u32)> =
        app.data.lane_blocks(date, lane).iter().map(|b| (b.id, b.start, b.end, b.project)).collect();
    let mut summary = Vec::new();
    for &(id, start, end, project) in &blocks {
        let (name, code, color) = app
            .data
            .project(project)
            .map(|p| (p.name.clone(), p.code.clone(), p.color))
            .unwrap_or(("?".into(), String::new(), 10));
        let base = theme::project(color);
        let fill = if plan { theme::tint(base, 0.45) } else { base };
        let r = Rect::from_min_max(pos2(geo.x(start) + 1.0, rect.top() + 2.0), pos2(geo.x(end) - 1.0, rect.bottom() - 2.0));
        painter.rect_filled(r, CornerRadius::same(5), fill);
        painter.rect_stroke(r, CornerRadius::same(5), Stroke::new(1.0_f32, base), StrokeKind::Inside);
        if app.selected == Some(id) {
            painter.rect_stroke(r.expand(1.5), CornerRadius::same(6), Stroke::new(2.5_f32, theme::TEXT), StrokeKind::Outside);
        }
        let hours = fmt_h(u32::from(end - start));
        let who = if code.is_empty() { name.clone() } else { code.clone() };
        let font = FontId::proportional(12.0);
        let candidates = [format!("{who} · {hours}h"), who.clone(), hours.clone()];
        if let Some(text) = candidates.iter().find(|t| painter.layout_no_wrap((*t).clone(), font.clone(), Color32::WHITE).size().x < r.width() - 8.0) {
            painter.with_clip_rect(r).text(
                pos2(r.left() + 5.0, r.center().y),
                Align2::LEFT_CENTER,
                text,
                font,
                theme::text_on(fill),
            );
        }
        summary.push(format!("{name} {}–{} ({hours} h)", slot_time(start), slot_time(end)));
    }
    // Clear lines at the start and end of the working day (drawn over blocks), thin ones around lunch
    if let Some(h) = hours {
        let span = (rect.top() - 1.0)..=(rect.bottom() + 1.0);
        for m in [h.start, h.end] {
            painter.vline(geo.x_min(m), span.clone(), Stroke::new(2.5_f32, theme::WORK_LINE));
        }
        if let Some((ls, le)) = h.lunch {
            for m in [ls, le] {
                painter.vline(geo.x_min(m), rect.y_range(), Stroke::new(1.0_f32, theme::UNDER));
            }
        }
    }
    // Now-line on today's actual lane
    if date == today && !plan {
        let now = model::now();
        let slot_f = (now.time().signed_duration_since(chrono::NaiveTime::MIN).num_minutes() as f32) / 30.0;
        if slot_f >= f32::from(geo.lo) && slot_f <= f32::from(geo.hi) {
            let x = geo.x0 + (slot_f - f32::from(geo.lo)) * geo.slot_w;
            painter.vline(x, rect.y_range(), Stroke::new(2.0_f32, theme::OVER));
        }
    }
    // Day total with a colour + symbol against the day's working hours (excluding lunch)
    let total = app.data.day_total(date, lane);
    let target = hours.map(|h| i64::from(h.working_minutes()));
    let (symbol, color) = match target {
        _ if total == 0 => ("", theme::MUTED),
        None => ("", theme::TEXT),
        Some(t) => match i64::from(total) * 30 - t {
            d if d.abs() <= 15 => (" ✔", theme::OK),
            d if d < 0 => (" ▾", theme::UNDER),
            _ => (" ▴", theme::OVER),
        },
    };
    painter.text(
        pos2(row.right() - 8.0, row.center().y),
        Align2::RIGHT_CENTER,
        format!("{} h{symbol}", fmt_h(total)),
        FontId::proportional(14.0),
        color,
    );

    // ---- interact ----
    let id = ui.id().with(("lane", date, lane));
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let spans: Vec<(u64, u8, u8)> = blocks.iter().map(|b| (b.0, b.1, b.2)).collect();
    let described = if summary.is_empty() { "empty".to_string() } else { summary.join("; ") };
    let a11y = format!("{} {} {}: {described}", DAY_NAMES[usize::from(weekday(date))], day_month(date), lane.label());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, a11y.clone()));

    let hover = response.hover_pos().map(|p| (p, hit_test(&spans, geo, p.x)));
    if let Some((pos, hit)) = hover {
        if app.drag.is_none() {
            match hit {
                Some((bid, part)) => {
                    if !locked {
                        ui.ctx().set_cursor_icon(if part == Part::Body { CursorIcon::Grab } else { CursorIcon::ResizeHorizontal });
                    }
                    if let Some(b) = app.data.block(bid) {
                        let name = app.data.project(b.project).map(|p| p.label()).unwrap_or_default();
                        let tip = format!(
                            "{name}\n{}–{} · {} h{}",
                            slot_time(b.start),
                            slot_time(b.end),
                            fmt_h(b.len()),
                            if locked { "" } else { "\nDrag edges to resize, the middle to move · right-click for options" }
                        );
                        response.clone().on_hover_text_at_pointer(tip);
                    }
                }
                None if !locked => {
                    ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
                    let slot = geo.slot_at(pos.x);
                    let r = Rect::from_min_max(pos2(geo.x(slot), rect.top() + 2.0), pos2(geo.x(slot + 1), rect.bottom() - 2.0));
                    if let Some(color) = app.brush.and_then(|b| app.data.project(b)).map(|p| theme::project(p.color)) {
                        painter.rect_filled(r, CornerRadius::same(4), theme::tint(color, 0.7));
                    }
                }
                None => {}
            }
        }
    }

    if response.drag_started() {
        if locked {
            app.flash(model::LOCKED, true);
        } else if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
            let hit = hit_test(&spans, geo, origin.x);
            begin_drag(app, date, lane, hit, geo.slot_at(origin.x));
        }
    }
    if response.dragged() {
        if let (Some(pos), Some(drag)) = (response.interact_pointer_pos(), app.drag.clone()) {
            if drag.date == date && drag.lane == lane {
                update_drag(app, &drag, geo, pos);
            }
        }
    }
    if response.drag_stopped() {
        end_drag(app);
    }
    if response.clicked() {
        let x = response.interact_pointer_pos().map_or(rect.left(), |p| p.x);
        match hit_test(&spans, geo, x) {
            Some((bid, _)) => app.selected = Some(bid),
            None if locked => app.flash(model::LOCKED, true),
            None => click_add(app, date, lane, geo.slot_at(x)),
        }
    }
    if response.secondary_clicked() {
        let x = response.interact_pointer_pos().map_or(rect.left(), |p| p.x);
        app.menu_block = hit_test(&spans, geo, x).map(|h| h.0);
        if let Some(bid) = app.menu_block {
            app.selected = Some(bid);
        }
    }
    if app.menu_block.is_some_and(|bid| spans.iter().any(|s| s.0 == bid)) {
        response.context_menu(|ui| block_menu(app, ui, locked));
    }
}

fn weekday(date: NaiveDate) -> u8 {
    use chrono::Datelike;
    date.weekday().num_days_from_monday() as u8
}

fn begin_drag(app: &mut App, date: NaiveDate, lane: Lane, hit: Option<(u64, Part)>, slot: u8) {
    let before = app.data.week_blocks(app.week);
    match hit {
        None => {
            let Some(brush) = app.brush else {
                app.flash("Pick a project to paint with first.", true);
                return;
            };
            let Some(free) = app.data.free_interval(date, lane, slot, None) else { return };
            match app.data.add_block(date, lane, brush, slot, slot + 1) {
                Ok(id) => {
                    app.selected = Some(id);
                    app.drag = Some(Drag { id, date, lane, kind: DragKind::Create { anchor: slot }, orig: (slot, slot + 1), free, before });
                }
                Err(e) => app.flash(&e, true),
            }
        }
        Some((id, part)) => {
            let Some(b) = app.data.block(id).cloned() else { return };
            let free = app.data.free_interval(date, lane, b.start, Some(id)).unwrap_or((b.start, b.end));
            let kind = match part {
                Part::Start => DragKind::Resize { start_edge: true },
                Part::End => DragKind::Resize { start_edge: false },
                Part::Body => DragKind::Move { grab: i32::from(slot) - i32::from(b.start) },
            };
            app.selected = Some(id);
            app.drag = Some(Drag { id, date, lane, kind, orig: (b.start, b.end), free, before });
        }
    }
}

fn update_drag(app: &mut App, drag: &Drag, geo: &Geo, pos: Pos2) {
    let span = match drag.kind {
        DragKind::Create { anchor } => create_span(anchor, geo.slot_at(pos.x), drag.free),
        DragKind::Resize { start_edge } => resize_span(drag.orig, start_edge, geo.boundary_at(pos.x), drag.free),
        DragKind::Move { grab } => move_span(drag.orig, i32::from(geo.slot_at(pos.x)) - grab, drag.free),
    };
    let _ = app.data.set_block_span(drag.id, span.0, span.1);
}

fn end_drag(app: &mut App) {
    if let Some(drag) = app.drag.take() {
        app.data.merge_lane(drag.date, drag.lane);
        if app.data.block(drag.id).is_none() {
            app.selected = None; // merged into a neighbour
        }
        app.commit_week_edit(drag.before);
    }
}

fn click_add(app: &mut App, date: NaiveDate, lane: Lane, slot: u8) {
    let Some(brush) = app.brush else {
        app.flash("Pick a project to paint with first.", true);
        return;
    };
    let before = app.data.week_blocks(app.week);
    match app.data.add_block(date, lane, brush, slot, slot + 1) {
        Ok(id) => {
            app.data.merge_lane(date, lane);
            app.selected = app.data.block(id).map(|b| b.id).or_else(|| {
                app.data.lane_blocks(date, lane).iter().find(|b| b.start <= slot && slot < b.end).map(|b| b.id)
            });
            app.commit_week_edit(before);
        }
        Err(e) => app.flash(&e, true),
    }
}

fn block_menu(app: &mut App, ui: &mut Ui, locked: bool) {
    let Some(id) = app.menu_block else { return };
    let Some(block) = app.data.block(id).cloned() else { return };
    let name = app.data.project(block.project).map(|p| p.label()).unwrap_or_default();
    ui.label(RichText::new(format!("{name}\n{}–{} · {} h", slot_time(block.start), slot_time(block.end), fmt_h(block.len()))).strong());
    ui.separator();
    if locked {
        ui.label("Read-only (saved timesheet).");
        return;
    }
    ui.menu_button("Change project", |ui| {
        let projects: Vec<(u32, String)> = app.data.active_projects().iter().map(|p| (p.id, p.label())).collect();
        for (pid, label) in projects {
            if ui.selectable_label(pid == block.project, label).clicked() {
                let before = app.data.week_blocks(app.week);
                if app.data.set_block_project(id, pid).is_ok() {
                    app.data.merge_lane(block.date, block.lane);
                    app.commit_week_edit(before);
                }
                ui.close_menu();
            }
        }
    });
    if block.lane == Lane::Plan && ui.button("Copy to Actual").clicked() {
        let before = app.data.week_blocks(app.week);
        match app.data.copy_block_to_actual(id) {
            Ok(0) => app.flash("The actual lane is already full at that time.", true),
            Ok(hh) => {
                app.flash(&format!("Copied {} h to Actual.", fmt_h(hh)), false);
                app.commit_week_edit(before);
            }
            Err(e) => app.flash(&e, true),
        }
        ui.close_menu();
    }
    if ui.button("Delete block").clicked() {
        app.delete_block(id);
        ui.close_menu();
    }
}

/// Precise editing of the selected block (also the keyboard-friendly way in).
fn selected_panel(app: &mut App, ui: &mut Ui, locked: bool) {
    let Some(block) = app.selected.and_then(|id| app.data.block(id).cloned()) else { return };
    if model::monday_of(block.date) != app.week {
        return;
    }
    let free = app.data.free_interval(block.date, block.lane, block.start, Some(block.id)).unwrap_or((block.start, block.end));
    ui::card(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("Selected · {} {} · {}", DAY_NAMES[usize::from(weekday(block.date))], day_month(block.date), block.lane.label())).strong());
            if locked {
                let name = app.data.project(block.project).map(|p| p.label()).unwrap_or_default();
                ui.label(format!("{name} · {}–{} · {} h", slot_time(block.start), slot_time(block.end), fmt_h(block.len())));
                return;
            }
            let mut project = block.project;
            let current = app.data.project(project).map(|p| p.label()).unwrap_or_default();
            egui::ComboBox::from_id_salt("sel-project").selected_text(current).show_ui(ui, |ui| {
                for p in app.data.active_projects() {
                    ui.selectable_value(&mut project, p.id, p.label());
                }
            });
            let mut start = block.start;
            let mut end = block.end;
            egui::ComboBox::from_id_salt("sel-start").selected_text(slot_time(start)).width(70.0).show_ui(ui, |ui| {
                for s in free.0..end {
                    ui.selectable_value(&mut start, s, slot_time(s));
                }
            });
            ui.label("to");
            egui::ComboBox::from_id_salt("sel-end").selected_text(slot_time(end)).width(70.0).show_ui(ui, |ui| {
                for e in (start + 1)..=free.1 {
                    ui.selectable_value(&mut end, e, slot_time(e));
                }
            });
            ui.label(RichText::new(format!("{} h", fmt_h(u32::from(end - start)))).strong());
            if project != block.project || start != block.start || end != block.end {
                let before = app.data.week_blocks(app.week);
                let _ = app.data.set_block_project(block.id, project);
                let _ = app.data.set_block_span(block.id, start, end.max(start + 1).min(SLOTS));
                app.data.merge_lane(block.date, block.lane);
                app.commit_week_edit(before);
            }
            if block.lane == Lane::Plan && ui.button("Copy to Actual").clicked() {
                app.menu_block = Some(block.id);
                let before = app.data.week_blocks(app.week);
                match app.data.copy_block_to_actual(block.id) {
                    Ok(0) => app.flash("The actual lane is already full at that time.", true),
                    Ok(hh) => {
                        app.flash(&format!("Copied {} h to Actual.", fmt_h(hh)), false);
                        app.commit_week_edit(before);
                    }
                    Err(e) => app.flash(&e, true),
                }
            }
            if ui.button("Delete").clicked() {
                app.delete_block(block.id);
            }
            ui.label(RichText::new("←/→ move · Shift+←/→ resize · Delete").size(12.0).color(theme::MUTED));
        });
    });
}

fn summary(app: &mut App, ui: &mut Ui, week: NaiveDate) {
    ui::card(ui, |ui| {
        ui.label(RichText::new("This week per project").strong());
        let rows = app.data.week_summary(week);
        if rows.is_empty() {
            ui.label(RichText::new("No time yet.").color(theme::MUTED));
            return;
        }
        egui::Grid::new("summary").striped(true).num_columns(4).spacing([18.0, 4.0]).show(ui, |ui| {
            ui.label(RichText::new("Project").color(theme::MUTED));
            ui.label(RichText::new("Plan").color(theme::MUTED));
            ui.label(RichText::new("Actual").color(theme::MUTED));
            ui.label(RichText::new("Difference").color(theme::MUTED));
            ui.end_row();
            let (mut plan, mut actual) = (0, 0);
            for row in &rows {
                let p = app.data.project(row.project);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                    ui.painter().rect_filled(r, CornerRadius::same(2), theme::project(p.map_or(10, |p| p.color)));
                    ui.label(p.map(|p| p.label()).unwrap_or_default());
                });
                ui.label(format!("{} h", fmt_h(row.plan_total())));
                ui.label(format!("{} h", fmt_h(row.actual_total())));
                ui.label(diff_text(row.actual_total(), row.plan_total()));
                ui.end_row();
                plan += row.plan_total();
                actual += row.actual_total();
            }
            ui.label(RichText::new("Total").strong());
            ui.label(RichText::new(format!("{} h", fmt_h(plan))).strong());
            ui.label(RichText::new(format!("{} h", fmt_h(actual))).strong());
            ui.label(diff_text(actual, plan));
            ui.end_row();
        });
    });
}

fn diff_text(actual: u32, plan: u32) -> RichText {
    match actual.cmp(&plan) {
        std::cmp::Ordering::Equal => RichText::new("on plan").color(theme::OK),
        std::cmp::Ordering::Greater => RichText::new(format!("+{} h", fmt_h(actual - plan))).color(theme::OVER),
        std::cmp::Ordering::Less => RichText::new(format!("−{} h", fmt_h(plan - actual))).color(theme::UNDER),
    }
}

fn actions(app: &mut App, ui: &mut Ui, week: NaiveDate, locked: bool) {
    ui::card(ui, |ui| {
        ui.label(RichText::new("Notes & goals for the week").strong());
        app.sync_note_buffer();
        let edit = egui::TextEdit::multiline(&mut app.note_buf.1)
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .hint_text("What do you want to get done this week?")
            .interactive(!locked);
        if ui.add(edit).changed() {
            let text = app.note_buf.1.clone();
            if app.data.set_note(week, &text).is_ok() {
                app.changed();
            }
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("📄 Print plan (PDF)").clicked() {
                app.export_pdf_plan(week);
            }
            if ui.button("📄 Print timesheet (PDF)").clicked() {
                app.export_pdf_timesheet(week);
            }
        });
        if locked {
            ui.add_space(4.0);
            if ui.button("Reopen timesheet").clicked() {
                app.confirm = Some(Confirm::Reopen(week));
            }
            return;
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("Copy last week's plan").on_hover_text("Copies last week's planned blocks into free plan time").clicked() {
                let before = app.data.week_blocks(week);
                match app.data.copy_last_week_plan(week) {
                    Ok(0) => app.flash("Nothing to copy: last week has no plan for active projects, or that time is already planned.", false),
                    Ok(n) => {
                        app.flash(&format!("Copied {n} planned block{} from last week.", if n == 1 { "" } else { "s" }), false);
                        app.commit_week_edit(before);
                    }
                    Err(e) => app.flash(&e, true),
                }
            }
            if ui.button("Actual = plan (to today)").on_hover_text("For days up to today with no actual time yet, copy the plan into Actual").clicked() {
                let before = app.data.week_blocks(week);
                match app.data.actual_from_plan(week, model::today()) {
                    Ok(0) if week > model::today() => app.flash("This week hasn't started yet.", false),
                    Ok(0) => app.flash("Nothing to fill: every planned day up to today already has actual time.", false),
                    Ok(n) => {
                        app.flash(&format!("Filled {n} day{} from the plan.", if n == 1 { "" } else { "s" }), false);
                        app.commit_week_edit(before);
                    }
                    Err(e) => app.flash(&e, true),
                }
            }
        });
        ui.add_space(4.0);
        let save = egui::Button::new(RichText::new("Save timesheet").color(Color32::WHITE).strong()).fill(theme::ACCENT);
        if ui.add(save).on_hover_text("Keep this week's actual time as a permanent record").clicked() {
            if app.data.day_total_week(week, Lane::Actual) == 0 {
                app.flash("There's no actual time on this week yet, so there's nothing to save.", true);
            } else {
                app.confirm = Some(Confirm::SaveWeek(week));
            }
        }
    });
}

/// "To do this week": per-project to-dos; unticked ones carry over to next week.
fn todos(app: &mut App, ui: &mut Ui, week: NaiveDate) {
    let mut ids: Vec<u32> = app.data.active_projects().iter().map(|p| p.id).collect();
    let archived_with_todos: Vec<u32> = app
        .data
        .projects_sorted()
        .iter()
        .filter(|p| !p.active && !app.data.todos_for(p.id, week).is_empty())
        .map(|p| p.id)
        .collect();
    ids.extend(archived_with_todos);
    if ids.is_empty() {
        return;
    }
    let today = model::today();
    let (mut done, mut total) = (0, 0);
    for id in &ids {
        for t in app.data.todos_for(*id, week) {
            total += 1;
            done += usize::from(t.done == Some(week));
        }
    }
    ui::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("To do this week").strong());
            let summary = if total == 0 {
                "Nothing yet — add to-dos per project below. Anything not ticked off carries over to next week.".to_string()
            } else {
                format!("{done} of {total} done · anything not ticked off carries over to next week")
            };
            ui.label(RichText::new(summary).size(12.5).color(theme::MUTED));
        });
        ui.add_space(4.0);
        ui.columns(2, |cols| {
            for (i, id) in ids.iter().enumerate() {
                project_todos(app, &mut cols[i % 2], *id, week, today);
            }
        });
    });
}

fn project_todos(app: &mut App, ui: &mut Ui, id: u32, week: NaiveDate, today: NaiveDate) {
    let Some(p) = app.data.project(id).cloned() else { return };
    egui::Frame::default()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0_f32, theme::BORDER))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.horizontal_wrapped(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(11.0), Sense::hover());
                    ui.painter().rect_filled(r, CornerRadius::same(3), theme::project(p.color));
                    ui.label(RichText::new(p.label()).strong());
                    let stage = match p.stage {
                        Some(s) => format!("Stage {s} · {}", model::STAGES[usize::from(s - 1)]),
                        None => "Stage not set".into(),
                    };
                    let on_site = p.stage == Some(model::ON_SITE_STAGE);
                    let color = if on_site { theme::ACCENT_DARK } else { theme::MUTED };
                    ui.label(RichText::new(stage).size(12.0).color(color))
                        .on_hover_text("SACAP work stage — change it on the Projects page");
                    if on_site || !app.data.rfis_for(id).is_empty() {
                        ui::rfi_button(app, ui, id, today);
                    }
                });

                let items: Vec<model::Todo> = app.data.todos_for(id, week).into_iter().cloned().collect();
                for t in items {
                    let editing = app.todo_edit.as_ref().is_some_and(|e| e.0 == t.id);
                    ui.horizontal(|ui| {
                        if editing {
                            let mut save = false;
                            if let Some(edit) = app.todo_edit.as_mut() {
                                let r = ui.add(egui::TextEdit::singleline(&mut edit.1).desired_width(240.0));
                                save = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            }
                            save |= ui.small_button("Save").clicked();
                            if save {
                                let (tid, text) = app.todo_edit.clone().expect("editing");
                                match app.data.edit_todo(tid, &text) {
                                    Ok(()) => {
                                        app.todo_edit = None;
                                        app.changed();
                                    }
                                    Err(e) => app.flash(&e, true),
                                }
                            }
                            if ui.small_button("Cancel").clicked() {
                                app.todo_edit = None;
                            }
                            return;
                        }
                        let mut ticked = t.done == Some(week);
                        let text = if ticked {
                            RichText::new(&t.text).strikethrough().color(theme::MUTED)
                        } else {
                            RichText::new(&t.text)
                        };
                        if ui.checkbox(&mut ticked, text).changed() && app.data.toggle_todo(t.id, week).is_ok() {
                            app.changed();
                        }
                        if t.week < week {
                            ui.label(RichText::new(format!("from {}", day_month(t.week))).size(11.5).color(theme::UNDER))
                                .on_hover_text(format!("Carried over from the week of {}", week_label(t.week)));
                        }
                        if ui.small_button("✏").on_hover_text("Edit").clicked() {
                            app.todo_edit = Some((t.id, t.text.clone()));
                        }
                        if ui.small_button("✕").on_hover_text("Delete this to-do").clicked() {
                            app.data.delete_todo(t.id);
                            app.changed();
                        }
                    });
                }

                let mut text = app.todo_inputs.get(&id).cloned().unwrap_or_default();
                ui.horizontal(|ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut text)
                            .id_salt(("todo-add", id))
                            .hint_text("Add a to-do…")
                            .desired_width(240.0),
                    );
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (enter || ui.small_button("Add").clicked()) && !text.trim().is_empty() {
                        match app.data.add_todo(id, week, &text) {
                            Ok(_) => {
                                text.clear();
                                app.changed();
                                r.request_focus();
                            }
                            Err(e) => app.flash(&e, true),
                        }
                    }
                });
                app.todo_inputs.insert(id, text);
            });
        });
    ui.add_space(6.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Data;
    use eframe::egui::{Event, PointerButton, RawInput};

    /// Drives the real UI headlessly: one call = one frame with these input events.
    struct Harness {
        ctx: egui::Context,
        app: App,
        time: f64,
    }

    impl Harness {
        fn new(data: Data, week: NaiveDate) -> Self {
            let dir = std::env::temp_dir().join(format!("ttt-ui-{}-{}", std::process::id(), rand_tag()));
            let _ = std::fs::create_dir_all(&dir);
            let mut app = App::new(data, dir, None, None);
            app.go_to_week(week);
            let ctx = egui::Context::default();
            ui::setup(&ctx); // the real fonts and theme
            let mut h = Harness { ctx, app, time: 0.0 };
            h.frame(vec![]);
            h.frame(vec![]);
            h
        }
        fn frame(&mut self, events: Vec<Event>) {
            self.time += 0.05;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1320.0, 880.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let _ = self.ctx.run(input, |ctx| self.app.ui(ctx));
        }
        fn lane(&self, date: NaiveDate, lane: Lane) -> LaneGeo {
            self.app.lanes.iter().find(|l| l.date == date && l.lane == lane).cloned().expect("lane is drawn")
        }
        fn press(pos: Pos2, pressed: bool) -> Event {
            Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() }
        }
        fn drag(&mut self, from: Pos2, to: Pos2) {
            self.frame(vec![Event::PointerMoved(from)]);
            self.frame(vec![Self::press(from, true)]);
            let steps = 6;
            for i in 1..=steps {
                let t = i as f32 / steps as f32;
                self.frame(vec![Event::PointerMoved(from + (to - from) * t)]);
            }
            self.frame(vec![Self::press(to, false)]);
            self.frame(vec![]);
        }
        fn click(&mut self, at: Pos2) {
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![Self::press(at, true)]);
            self.frame(vec![Self::press(at, false)]);
            self.frame(vec![]);
        }
        fn spans(&self, date: NaiveDate, lane: Lane) -> Vec<(u8, u8)> {
            self.app.data.lane_blocks(date, lane).iter().map(|b| (b.start, b.end)).collect()
        }
    }

    fn rand_tag() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    }

    fn setup() -> (Harness, NaiveDate) {
        let mut data = Data::default();
        data.add_project("Alpha", "A1", "").unwrap();
        let mon = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        (Harness::new(data, mon), mon)
    }

    #[test]
    fn drag_on_empty_time_paints_a_block() {
        let (mut h, mon) = setup();
        let lane = h.lane(mon, Lane::Plan);
        h.drag(lane.point(18), lane.point(23)); // 09:00 .. 12:00
        assert_eq!(h.spans(mon, Lane::Plan), vec![(18, 24)]);
        assert_eq!(h.app.data.day_total(mon, Lane::Plan), 6);
        // Dragging backwards works too.
        let actual = h.lane(mon, Lane::Actual);
        h.drag(actual.point(30), actual.point(27));
        assert_eq!(h.spans(mon, Lane::Actual), vec![(27, 31)]);
    }

    #[test]
    fn drag_edge_extends_and_middle_moves() {
        let (mut h, mon) = setup();
        let lane = h.lane(mon, Lane::Actual);
        h.drag(lane.point(18), lane.point(21)); // (18, 22)
        assert_eq!(h.spans(mon, Lane::Actual), vec![(18, 22)]);
        let lane = h.lane(mon, Lane::Actual);
        let right_edge = lane.edge(22) - vec2(2.0, 0.0);
        h.drag(right_edge, lane.edge(26)); // extend the end to 13:00
        assert_eq!(h.spans(mon, Lane::Actual), vec![(18, 26)]);
        let lane = h.lane(mon, Lane::Actual);
        h.drag(lane.point(21), lane.point(25)); // grab the middle, move 4 slots later
        assert_eq!(h.spans(mon, Lane::Actual), vec![(22, 30)]);
        // Undo walks back through the edits.
        h.time += 0.05;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1320.0, 880.0))),
            time: Some(h.time),
            modifiers: egui::Modifiers::COMMAND,
            events: vec![Event::Key { key: egui::Key::Z, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }],
            ..Default::default()
        };
        let _ = h.ctx.run(input, |ctx| h.app.ui(ctx));
        assert_eq!(h.spans(mon, Lane::Actual), vec![(18, 26)]);
    }

    #[test]
    fn click_adds_half_an_hour_and_blocks_never_overlap() {
        let (mut h, mon) = setup();
        let lane = h.lane(mon, Lane::Plan);
        h.click(lane.point(20));
        assert_eq!(h.spans(mon, Lane::Plan), vec![(20, 21)]);
        let lane = h.lane(mon, Lane::Plan);
        h.click(lane.point(21)); // touching the same project: merged
        assert_eq!(h.spans(mon, Lane::Plan), vec![(20, 22)]);
        let lane = h.lane(mon, Lane::Plan);
        h.drag(lane.point(16), lane.point(30)); // stops at the existing block
        assert_eq!(h.spans(mon, Lane::Plan), vec![(16, 22)]);
    }

    #[test]
    fn saved_week_ignores_the_mouse() {
        let (mut h, mon) = setup();
        let lane = h.lane(mon, Lane::Actual);
        h.drag(lane.point(18), lane.point(21));
        h.app.data.save_timesheet(mon, model::now()).unwrap();
        h.frame(vec![]);
        let lane = h.lane(mon, Lane::Actual);
        h.drag(lane.point(26), lane.point(30));
        h.click(lane.point(32));
        assert_eq!(h.spans(mon, Lane::Actual), vec![(18, 22)]);
    }

    #[test]
    fn geometry_and_hits() {
        let geo = Geo { x0: 100.0, slot_w: 20.0, lo: 14, hi: 38 };
        assert_eq!(geo.slot_at(100.0), 14);
        assert_eq!(geo.slot_at(139.0), 15);
        assert_eq!(geo.slot_at(5000.0), 37);
        assert_eq!(geo.boundary_at(131.0), 16);
        assert_eq!(geo.x(16), 140.0);
        let blocks = [(7, 16, 20)]; // x 140..220
        assert_eq!(hit_test(&blocks, &geo, 141.0), Some((7, Part::Start)));
        assert_eq!(hit_test(&blocks, &geo, 180.0), Some((7, Part::Body)));
        assert_eq!(hit_test(&blocks, &geo, 218.0), Some((7, Part::End)));
        assert_eq!(hit_test(&blocks, &geo, 230.0), None);
    }
}
