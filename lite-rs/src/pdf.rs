//! Printable PDFs, written directly with the 14 standard PDF fonts (Helvetica),
//! so no font files are embedded and the program stays small.
//!
//! Layout works in millimetres from the top-left of an A4 page.

use chrono::NaiveDate;
use pdf_writer::{Content, Name, Pdf, Rect, Ref, Str};

use crate::model::{
    self, DAY_NAMES, Data, Lane, PALETTE, Report, Revision, Timesheet, date_time, day_month, fmt_h, slot_time,
    week_dates, week_label,
};

const PT: f32 = 72.0 / 25.4; // points per millimetre
const MARGIN: f32 = 12.0;
type Rgb = [u8; 3];
const ACCENT: Rgb = [0xB0, 0x5A, 0x3E];
const ACCENT_DARK: Rgb = [0x8F, 0x46, 0x30];
const TEXT: Rgb = [0x2E, 0x22, 0x1D];
const MUTED: Rgb = [0x6B, 0x52, 0x46];
const HEAD_FILL: Rgb = [0xF5, 0xE2, 0xD8];
const TINT: Rgb = [0xFB, 0xEE, 0xE7];
const GRID: Rgb = [0xC9, 0xB4, 0xA8];
const WHITE: Rgb = [0xFF, 0xFF, 0xFF];

// --------------------------------------------------------------------------
//  Fonts: Helvetica / Helvetica-Bold widths (1/1000 em) for ASCII 32..=126
// --------------------------------------------------------------------------
const W_REG: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556,
    556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833,
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556,
    556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334,
    260, 334, 584,
];
const W_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556,
    556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833,
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611,
    556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389,
    280, 389, 584,
];

/// Encode text as WinAnsi (the standard fonts' encoding); unknown characters become '?'.
fn win_ansi(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match c {
            ' '..='~' => c as u8,
            '€' => 0x80,
            '‚' => 0x82,
            '„' => 0x84,
            '…' => 0x85,
            '‘' => 0x91,
            '’' => 0x92,
            '“' => 0x93,
            '”' => 0x94,
            '•' => 0x95,
            '–' => 0x96,
            '—' => 0x97,
            '™' => 0x99,
            '\u{A0}'..='\u{FF}' => c as u32 as u8,
            _ => b'?',
        })
        .collect()
}

fn glyph_width(byte: u8, bold: bool) -> u16 {
    match byte {
        32..=126 => (if bold { W_BOLD } else { W_REG })[usize::from(byte - 32)],
        0x85 | 0x97 => 1000,
        0x95 => 350,
        0x91 | 0x92 => 222,
        _ => 556,
    }
}

/// Width of `text` in millimetres at `size` points.
pub fn text_width(text: &str, size: f32, bold: bool) -> f32 {
    let units: u32 = win_ansi(text).iter().map(|&b| u32::from(glyph_width(b, bold))).sum();
    units as f32 / 1000.0 * size / PT
}

/// Shorten `text` with an ellipsis so it fits in `max` mm.
fn fit(text: &str, max: f32, size: f32, bold: bool) -> String {
    if text_width(text, size, bold) <= max {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "…";
        if text_width(&candidate, size, bold) <= max {
            return candidate;
        }
    }
    String::new()
}

fn rgb(c: Rgb) -> (f32, f32, f32) {
    (f32::from(c[0]) / 255.0, f32::from(c[1]) / 255.0, f32::from(c[2]) / 255.0)
}

/// Blend a colour towards white (for planned time and light fills).
fn tint(c: Rgb, amount: f32) -> Rgb {
    std::array::from_fn(|i| (f32::from(c[i]) + (255.0 - f32::from(c[i])) * amount) as u8)
}

fn readable_on(c: Rgb) -> Rgb {
    let lum = 0.299 * f32::from(c[0]) + 0.587 * f32::from(c[1]) + 0.114 * f32::from(c[2]);
    if lum > 160.0 { TEXT } else { WHITE }
}

// --------------------------------------------------------------------------
//  A tiny page-layout builder
// --------------------------------------------------------------------------
pub struct Doc {
    width: f32,
    height: f32,
    pages: Vec<Content>,
    title: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Align {
    Left,
    Right,
    Center,
}

impl Doc {
    fn new(landscape: bool, title: &str) -> Self {
        let (width, height) = if landscape { (297.0, 210.0) } else { (210.0, 297.0) };
        Doc { width, height, pages: vec![Content::new()], title: title.to_string() }
    }

    fn page(&mut self) -> &mut Content {
        self.pages.last_mut().expect("there is always a page")
    }

    fn new_page(&mut self) {
        self.pages.push(Content::new());
    }

    fn bottom(&self) -> f32 {
        self.height - MARGIN - 8.0
    }

    fn content_width(&self) -> f32 {
        self.width - 2.0 * MARGIN
    }

    fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgb) {
        let height = self.height;
        let (r, g, b) = rgb(color);
        let c = self.page();
        c.set_fill_rgb(r, g, b);
        c.rect(x * PT, (height - y - h) * PT, w * PT, h * PT);
        c.fill_nonzero();
    }

    fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgb, width: f32) {
        let height = self.height;
        let (r, g, b) = rgb(color);
        let c = self.page();
        c.set_stroke_rgb(r, g, b);
        c.set_line_width(width);
        c.rect(x * PT, (height - y - h) * PT, w * PT, h * PT);
        c.stroke();
    }

    fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, color: Rgb, width: f32) {
        let height = self.height;
        let (r, g, b) = rgb(color);
        let c = self.page();
        c.set_stroke_rgb(r, g, b);
        c.set_line_width(width);
        c.move_to(x1 * PT, (height - y1) * PT);
        c.line_to(x2 * PT, (height - y2) * PT);
        c.stroke();
    }

    /// Text with its baseline at `y`.
    fn text(&mut self, x: f32, y: f32, size: f32, bold: bool, color: Rgb, text: &str, align: Align) {
        let w = text_width(text, size, bold);
        let x = match align {
            Align::Left => x,
            Align::Right => x - w,
            Align::Center => x - w / 2.0,
        };
        let height = self.height;
        let (r, g, b) = rgb(color);
        let bytes = win_ansi(text);
        let c = self.page();
        c.set_fill_rgb(r, g, b);
        c.begin_text();
        c.set_font(Name(if bold { b"F2" } else { b"F1" }), size);
        c.next_line(x * PT, (height - y) * PT);
        c.show(Str(&bytes));
        c.end_text();
    }

    fn watermark(&mut self, text: &str) {
        let (w, h) = (self.width, self.height);
        let bytes = win_ansi(text);
        let (r, g, b) = rgb([0xF2, 0xDD, 0xD5]);
        let (cos, sin) = (0.906f32, 0.423f32); // 25 degrees
        let c = self.page();
        c.set_fill_rgb(r, g, b);
        c.begin_text();
        c.set_font(Name(b"F2"), 110.0);
        c.set_text_matrix([cos, sin, -sin, cos, (w * 0.18) * PT, (h * 0.18) * PT]);
        c.show(Str(&bytes));
        c.end_text();
    }

    fn finish(mut self, footer: &str) -> Vec<u8> {
        let count = self.pages.len();
        let (width, height) = (self.width, self.height);
        for (i, page) in self.pages.iter_mut().enumerate() {
            let label = format!("{footer}  ·  page {} of {count}", i + 1);
            let bytes = win_ansi(&label);
            let x = width - MARGIN - text_width(&label, 7.5, false);
            let (r, g, b) = rgb(MUTED);
            page.set_fill_rgb(r, g, b);
            page.begin_text();
            page.set_font(Name(b"F1"), 7.5);
            page.next_line(x * PT, (MARGIN - 4.0) * PT);
            page.show(Str(&bytes));
            page.end_text();
        }

        let mut pdf = Pdf::new();
        let catalog = Ref::new(1);
        let tree = Ref::new(2);
        let regular = Ref::new(3);
        let bold = Ref::new(4);
        let info = Ref::new(5);
        let page_ids: Vec<Ref> = (0..count).map(|i| Ref::new(10 + 2 * i as i32)).collect();
        pdf.catalog(catalog).pages(tree);
        pdf.pages(tree).kids(page_ids.iter().copied()).count(count as i32);
        pdf.type1_font(regular).base_font(Name(b"Helvetica")).encoding_predefined(Name(b"WinAnsiEncoding"));
        pdf.type1_font(bold).base_font(Name(b"Helvetica-Bold")).encoding_predefined(Name(b"WinAnsiEncoding"));
        pdf.document_info(info)
            .title(pdf_writer::TextStr(&self.title))
            .creator(pdf_writer::TextStr("Tiny Time Track Lite"));
        let media = Rect::new(0.0, 0.0, width * PT, height * PT);
        for (i, content) in self.pages.into_iter().enumerate() {
            let content_id = Ref::new(11 + 2 * i as i32);
            {
                let mut page = pdf.page(page_ids[i]);
                page.media_box(media).parent(tree).contents(content_id);
                page.resources().fonts().pair(Name(b"F1"), regular).pair(Name(b"F2"), bold);
            } // the page object is written when it goes out of scope
            pdf.stream(content_id, &content.finish());
        }
        pdf.finish()
    }

    // ---- composite pieces ------------------------------------------------------
    /// Title block; returns the y where content can start.
    fn header(&mut self, data: &Data, title: &str, subtitle: &str) -> f32 {
        let right = self.width - MARGIN;
        self.text(MARGIN, MARGIN + 7.0, 18.0, true, ACCENT_DARK, &title.to_uppercase(), Align::Left);
        self.text(MARGIN, MARGIN + 13.5, 11.0, true, TEXT, subtitle, Align::Left);
        let s = &data.settings;
        let mut y = MARGIN + 4.0;
        if !s.user_name.is_empty() {
            self.text(right, y, 10.5, true, TEXT, &s.user_name, Align::Right);
            y += 4.5;
        }
        if !s.company.is_empty() {
            self.text(right, y, 9.0, false, TEXT, &s.company, Align::Right);
            y += 4.2;
        }
        let printed = format!("Printed {}", date_time(model::now()));
        self.text(right, y.max(MARGIN + 13.5), 8.0, false, MUTED, &printed, Align::Right);
        let line_y = MARGIN + 17.0;
        self.fill_rect(MARGIN, line_y, self.content_width(), 0.9, ACCENT);
        line_y + 6.0
    }

    fn status(&mut self, y: f32, text: &str, color: Rgb) -> f32 {
        self.text(MARGIN, y, 9.0, true, color, text, Align::Left);
        y + 6.0
    }

    fn heading(&mut self, y: f32, text: &str) -> f32 {
        let y = self.ensure_space(y, 20.0);
        self.text(MARGIN, y + 3.0, 11.0, true, ACCENT_DARK, text, Align::Left);
        y + 6.5
    }

    fn ensure_space(&mut self, y: f32, needed: f32) -> f32 {
        if y + needed > self.bottom() {
            self.new_page();
            MARGIN + 4.0
        } else {
            y
        }
    }

    /// A day-by-day timeline: one row per (label, blocks).
    fn timeline(&mut self, y: f32, rows: &[(String, Vec<TlBlock>)], range: (u8, u8)) -> f32 {
        let label_w = 26.0;
        let x0 = MARGIN + label_w;
        let w = self.content_width() - label_w;
        let slots = f32::from(range.1 - range.0);
        let slot_w = w / slots;
        let row_h = 9.0;
        let mut y = self.ensure_space(y, 6.0 + row_h * rows.len().min(3) as f32);
        // half-hour labels (":30" when space is tight)
        for slot in range.0..=range.1 {
            let x = x0 + f32::from(slot - range.0) * slot_w;
            if slot % 2 == 0 {
                self.text(x, y + 2.5, 6.5, true, TEXT, &slot_time(slot), Align::Center);
            } else {
                let label = if slot_w >= 8.0 { slot_time(slot) } else { ":30".to_string() };
                self.text(x, y + 2.5, 5.5, false, MUTED, &label, Align::Center);
            }
        }
        y += 4.5;
        for (label, blocks) in rows {
            y = self.ensure_space(y, row_h + 1.0);
            self.fill_rect(x0, y, w, row_h, TINT);
            for slot in range.0..=range.1 {
                let x = x0 + f32::from(slot - range.0) * slot_w;
                let color = if slot % 2 == 0 { GRID } else { tint(GRID, 0.55) };
                self.line(x, y, x, y + row_h, color, if slot % 2 == 0 { 0.5 } else { 0.25 });
            }
            self.text(MARGIN, y + row_h / 2.0 + 1.3, 8.5, true, TEXT, label, Align::Left);
            for b in blocks {
                let bx = x0 + f32::from(b.start - range.0) * slot_w;
                let bw = f32::from(b.end - b.start) * slot_w;
                let fill = if b.plan { tint(b.color, 0.35) } else { b.color };
                self.fill_rect(bx + 0.3, y + 0.8, bw - 0.6, row_h - 1.6, fill);
                let text = fit(&b.label, bw - 2.0, 7.0, true);
                if !text.is_empty() {
                    self.text(bx + 1.2, y + row_h / 2.0 + 1.2, 7.0, true, readable_on(fill), &text, Align::Left);
                }
            }
            self.stroke_rect(x0, y, w, row_h, GRID, 0.5);
            y += row_h + 1.0;
        }
        y + 3.0
    }

    /// A table with a repeated header row on every page.
    fn table(&mut self, y: f32, cols: &[(&str, f32, bool)], rows: &[Vec<String>], footer: Option<&[String]>, size: f32) -> f32 {
        let row_h = size * 0.55 + 2.6;
        let total_w: f32 = cols.iter().map(|c| c.1).sum();
        let mut y = self.ensure_space(y, row_h * 3.0);
        y = self.table_header(y, cols, total_w, row_h, size);
        for (i, row) in rows.iter().enumerate() {
            if y + row_h > self.bottom() {
                self.new_page();
                y = self.table_header(MARGIN + 4.0, cols, total_w, row_h, size);
            }
            if i % 2 == 1 {
                self.fill_rect(MARGIN, y, total_w, row_h, [0xFD, 0xF8, 0xF5]);
            }
            self.table_row(y, cols, row, row_h, size, false);
            y += row_h;
        }
        if let Some(footer) = footer {
            if y + row_h > self.bottom() {
                self.new_page();
                y = self.table_header(MARGIN + 4.0, cols, total_w, row_h, size);
            }
            self.fill_rect(MARGIN, y, total_w, row_h, TINT);
            self.line(MARGIN, y, MARGIN + total_w, y, GRID, 0.8);
            self.table_row(y, cols, footer, row_h, size, true);
            y += row_h;
        }
        self.line(MARGIN, y, MARGIN + total_w, y, GRID, 0.5);
        y + 4.0
    }

    fn table_header(&mut self, y: f32, cols: &[(&str, f32, bool)], total_w: f32, row_h: f32, size: f32) -> f32 {
        self.fill_rect(MARGIN, y, total_w, row_h, HEAD_FILL);
        let header: Vec<String> = cols.iter().map(|c| c.0.to_string()).collect();
        self.table_row(y, cols, &header, row_h, size, true);
        y + row_h
    }

    fn table_row(&mut self, y: f32, cols: &[(&str, f32, bool)], cells: &[String], row_h: f32, size: f32, bold: bool) {
        let mut x = MARGIN;
        for (i, (_, w, right)) in cols.iter().enumerate() {
            let text = fit(cells.get(i).map(String::as_str).unwrap_or(""), w - 2.4, size, bold);
            let baseline = y + row_h / 2.0 + size * 0.13;
            if *right {
                self.text(x + w - 1.2, baseline, size, bold, TEXT, &text, Align::Right);
            } else {
                self.text(x + 1.2, baseline, size, bold, TEXT, &text, Align::Left);
            }
            x += w;
        }
    }

    fn notes(&mut self, y: f32, title: &str, text: &str, min_h: f32) -> f32 {
        let y = self.heading(y, title);
        let lines = wrap(text, self.content_width() - 6.0, 9.0);
        let h = (lines.len() as f32 * 4.4 + 4.0).max(min_h);
        let y = self.ensure_space(y, h);
        self.stroke_rect(MARGIN, y, self.content_width(), h, GRID, 0.6);
        for (i, line) in lines.iter().enumerate() {
            self.text(MARGIN + 3.0, y + 5.0 + i as f32 * 4.4, 9.0, false, TEXT, line, Align::Left);
        }
        y + h + 4.0
    }

    fn signatures(&mut self, y: f32) {
        let y = self.ensure_space(y + 10.0, 14.0);
        let half = (self.content_width() - 20.0) / 2.0;
        for (i, label) in ["Signature", "Date"].iter().enumerate() {
            let x = MARGIN + i as f32 * (half + 20.0);
            self.line(x, y, x + half, y, TEXT, 0.6);
            self.text(x, y + 4.0, 8.5, false, MUTED, label, Align::Left);
        }
    }
}

fn wrap(text: &str, max: f32, size: f32) -> Vec<String> {
    let mut out = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if text_width(&candidate, size, false) > max && !line.is_empty() {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        out.push(line);
    }
    out
}

struct TlBlock {
    start: u8,
    end: u8,
    color: Rgb,
    label: String,
    plan: bool,
}

fn block_label(name: &str, code: &str, len: u32) -> String {
    let who = if code.is_empty() { name } else { code };
    format!("{who} · {}h", fmt_h(len))
}

fn day_label(date: NaiveDate) -> String {
    format!("{} {}", DAY_NAMES[date.weekday_index()], day_month(date))
}

trait WeekdayIndex {
    fn weekday_index(&self) -> usize;
}
impl WeekdayIndex for NaiveDate {
    fn weekday_index(&self) -> usize {
        use chrono::Datelike;
        self.weekday().num_days_from_monday() as usize
    }
}

fn hours_table(doc: &mut Doc, y: f32, week: NaiveDate, visible: &[usize], lines: &[(String, String, [u32; 7])]) -> f32 {
    let dates = week_dates(week);
    let fixed = 30.0 + 18.0;
    let day_w = ((doc.content_width() - fixed - 22.0) / visible.len() as f32).min(26.0);
    let name_w = doc.content_width() - 18.0 - 18.0 - day_w * visible.len() as f32;
    let headers: Vec<String> = visible.iter().map(|&i| format!("{} {}", DAY_NAMES[i], dates[i].format("%-d"))).collect();
    let mut cols: Vec<(&str, f32, bool)> = vec![("Project", name_w, false), ("Code", 18.0, false)];
    for h in &headers {
        cols.push((h.as_str(), day_w, true));
    }
    cols.push(("Total", 18.0, true));
    let rows: Vec<Vec<String>> = lines
        .iter()
        .map(|(name, code, days)| {
            let mut row = vec![name.clone(), code.clone()];
            row.extend(visible.iter().map(|&i| if days[i] > 0 { fmt_h(days[i]) } else { String::new() }));
            row.push(fmt_h(days.iter().sum()));
            row
        })
        .collect();
    let mut footer = vec!["Day total".to_string(), String::new()];
    let day_totals: [u32; 7] = std::array::from_fn(|i| lines.iter().map(|l| l.2[i]).sum());
    footer.extend(visible.iter().map(|&i| fmt_h(day_totals[i])));
    footer.push(fmt_h(day_totals.iter().sum()));
    doc.table(y, &cols, &rows, Some(&footer), 9.0)
}

fn snapshot_rows(sheet: &Timesheet, visible: &[usize]) -> Vec<(String, Vec<TlBlock>)> {
    let dates = week_dates(sheet.week);
    visible
        .iter()
        .map(|&i| {
            let blocks = sheet
                .blocks
                .iter()
                .filter(|b| b.date == dates[i])
                .map(|b| TlBlock {
                    start: b.start,
                    end: b.end,
                    color: PALETTE[usize::from(b.color) % PALETTE.len()],
                    label: block_label(&b.name, &b.code, u32::from(b.end - b.start)),
                    plan: false,
                })
                .collect();
            (day_label(dates[i]), blocks)
        })
        .collect()
}

fn sheet_range(data: &Data, sheet: &Timesheet) -> (u8, u8) {
    data.timeline_range(sheet.blocks.iter().map(|b| (b.start, b.end)))
}

fn sheet_visible(data: &Data, sheet: &Timesheet) -> Vec<usize> {
    let weekend = data.settings.show_weekend || sheet.lines.iter().any(|l| l.slots[5] > 0 || l.slots[6] > 0);
    (0..if weekend { 7 } else { 5 }).collect()
}

// --------------------------------------------------------------------------
//  Documents
// --------------------------------------------------------------------------
/// The week's plan: a timeline of planned blocks, planned hours, and notes.
pub fn plan_pdf(data: &Data, week: NaiveDate) -> Vec<u8> {
    let label = week_label(week);
    let mut doc = Doc::new(true, &format!("Weekly plan {label}"));
    let mut y = doc.header(data, "Weekly plan", &label);
    let visible = data.visible_days(week);
    let dates = week_dates(week);
    let rows: Vec<(String, Vec<TlBlock>)> = visible
        .iter()
        .map(|&i| {
            let blocks = data
                .lane_blocks(dates[i], Lane::Plan)
                .into_iter()
                .map(|b| {
                    let p = data.project(b.project);
                    TlBlock {
                        start: b.start,
                        end: b.end,
                        color: PALETTE[usize::from(p.map_or(10, |p| p.color)) % PALETTE.len()],
                        label: block_label(p.map_or("?", |p| p.name.as_str()), p.map_or("", |p| p.code.as_str()), b.len()),
                        plan: true,
                    }
                })
                .collect();
            (day_label(dates[i]), blocks)
        })
        .collect();
    let planned: Vec<(String, String, [u32; 7])> = data
        .week_summary(week)
        .into_iter()
        .filter(|r| r.plan_total() > 0)
        .map(|r| {
            let p = data.project(r.project);
            (p.map_or("?".into(), |p| p.name.clone()), p.map_or(String::new(), |p| p.code.clone()), r.plan)
        })
        .collect();
    if planned.is_empty() {
        y = doc.status(y, "No time planned for this week yet — the timeline is left blank to fill in by hand.", MUTED);
    }
    y = doc.timeline(y, &rows, data.visible_slots(week));
    if !planned.is_empty() {
        y = doc.heading(y, "Planned hours");
        y = hours_table(&mut doc, y, week, &visible, &planned);
    }
    y = doc.notes(y, "Notes & goals", data.note(week), 22.0);
    todo_section(&mut doc, y, data, week);
    doc.finish(&format!("Tiny Time Track Lite · weekly plan {label}"))
}

/// Per-project to-dos for the week, with tick boxes.
fn todo_section(doc: &mut Doc, y: f32, data: &Data, week: NaiveDate) -> f32 {
    let groups: Vec<(String, Vec<(String, bool, Option<NaiveDate>)>)> = data
        .projects_sorted()
        .iter()
        .filter_map(|p| {
            let items: Vec<(String, bool, Option<NaiveDate>)> = data
                .todos_for(p.id, week)
                .iter()
                .map(|t| (t.text.clone(), t.done == Some(week), (t.week < week).then_some(t.week)))
                .collect();
            (!items.is_empty()).then(|| (format!("{}  ·  {}", p.label(), model::stage_label(p.stage)), items))
        })
        .collect();
    if groups.is_empty() {
        return y;
    }
    let mut y = doc.heading(y, "To do this week");
    for (title, items) in groups {
        y = doc.ensure_space(y, 10.0);
        doc.text(MARGIN, y + 3.0, 9.5, true, TEXT, &title, Align::Left);
        y += 5.0;
        for (text, done, from) in items {
            y = doc.ensure_space(y, 5.0);
            doc.stroke_rect(MARGIN + 2.0, y + 0.2, 3.2, 3.2, MUTED, 0.6);
            if done {
                doc.text(MARGIN + 2.45, y + 3.0, 8.5, true, TEXT, "x", Align::Left);
            }
            let suffix = from.map(|d| format!("   (carried over from {})", day_month(d))).unwrap_or_default();
            let line = fit(&format!("{text}{suffix}"), doc.content_width() - 10.0, 9.0, false);
            doc.text(MARGIN + 7.5, y + 3.0, 9.0, false, if done { MUTED } else { TEXT }, &line, Align::Left);
            y += 5.0;
        }
        y += 1.5;
    }
    y
}

/// A project's request-for-information register.
pub fn rfi_register_pdf(data: &Data, project: u32) -> Vec<u8> {
    let p = data.project(project);
    let label = p.map(|p| p.label()).unwrap_or_default();
    let today = model::today();
    let mut doc = Doc::new(true, &format!("RFI register {label}"));
    let subtitle = format!("{label}  ·  {}", model::stage_label(p.and_then(|p| p.stage)));
    let mut y = doc.header(data, "RFI register", &subtitle);
    let rfis = data.rfis_for(project);
    let (open, overdue) = data.rfi_counts(project, today);
    y = doc.status(
        y,
        &format!("{} RFIs  ·  {open} open  ·  {overdue} overdue  (as at {})", rfis.len(), model::long_date(today)),
        if overdue > 0 { [0x8A, 0x3A, 0x22] } else { MUTED },
    );
    if rfis.is_empty() {
        doc.status(y, "No RFIs recorded.", MUTED);
    } else {
        let w = doc.content_width();
        let cols: [(&str, f32, bool); 6] = [
            ("No.", 16.0, false),
            ("Subject", w - 16.0 - 26.0 * 3.0 - 22.0, false),
            ("Raised", 26.0, false),
            ("Reply by", 26.0, false),
            ("Status", 22.0, false),
            ("Answered", 26.0, false),
        ];
        let rows: Vec<Vec<String>> = rfis
            .iter()
            .map(|r| {
                vec![
                    format!("RFI {}", r.number),
                    r.subject.clone(),
                    model::long_date(r.raised),
                    r.due.map(model::long_date).unwrap_or_else(|| "—".into()),
                    r.status(today).to_string(),
                    r.answered.map(model::long_date).unwrap_or_else(|| "—".into()),
                ]
            })
            .collect();
        y = doc.table(y, &cols, &rows, None, 9.0);
        y = doc.heading(y, "Questions and answers");
        for r in &rfis {
            y = doc.ensure_space(y, 12.0);
            doc.text(MARGIN, y + 3.0, 9.5, true, TEXT, &format!("RFI {} — {}", r.number, r.subject), Align::Left);
            y += 5.0;
            for (label, body) in [("Question", &r.details), ("Answer", &r.response)] {
                if body.trim().is_empty() {
                    continue;
                }
                for (i, line) in wrap(body, w - 26.0, 9.0).iter().enumerate() {
                    y = doc.ensure_space(y, 4.6);
                    if i == 0 {
                        doc.text(MARGIN + 2.0, y + 3.0, 8.5, true, MUTED, label, Align::Left);
                    }
                    doc.text(MARGIN + 24.0, y + 3.0, 9.0, false, TEXT, line, Align::Left);
                    y += 4.4;
                }
            }
            y += 2.5;
        }
    }
    doc.finish(&format!("Tiny Time Track Lite · RFI register {label}"))
}

fn sheet_document(data: &Data, sheet: &Timesheet, status: &str, status_color: Rgb, draft: bool) -> Vec<u8> {
    let label = week_label(sheet.week);
    let mut doc = Doc::new(true, &format!("Timesheet {label}"));
    if draft {
        doc.watermark("DRAFT");
    }
    let mut y = doc.header(data, "Timesheet", &label);
    y = doc.status(y, status, status_color);
    let visible = sheet_visible(data, sheet);
    if sheet.lines.is_empty() {
        y = doc.status(y, "No actual time has been recorded for this week.", MUTED);
    } else {
        y = doc.timeline(y, &snapshot_rows(sheet, &visible), sheet_range(data, sheet));
        y = doc.heading(y, "Hours");
        let lines: Vec<(String, String, [u32; 7])> = sheet
            .lines
            .iter()
            .map(|l| (l.name.clone(), l.code.clone(), std::array::from_fn(|i| u32::from(l.slots[i]))))
            .collect();
        y = hours_table(&mut doc, y, sheet.week, &visible, &lines);
    }
    if !sheet.note.trim().is_empty() {
        y = doc.notes(y, "Notes", &sheet.note, 12.0);
    }
    doc.signatures(y);
    doc.finish(&format!("Tiny Time Track Lite · timesheet {label}"))
}

/// The week's actual time: the saved record if there is one, otherwise a DRAFT.
pub fn timesheet_pdf(data: &Data, week: NaiveDate) -> Vec<u8> {
    match data.timesheets.get(&week) {
        Some(sheet) => {
            let earlier = data.revisions_for(week).len();
            let mut status = format!("Saved record · saved {}", date_time(sheet.saved_at));
            if earlier > 0 {
                status += &format!(" · revision {} ({earlier} earlier version{} kept)", earlier + 1, if earlier == 1 { "" } else { "s" });
            }
            sheet_document(data, sheet, &status, [0x2E, 0x5E, 0x40], false)
        }
        None => {
            let sheet = data.live_sheet(week);
            sheet_document(data, &sheet, "DRAFT — this week hasn't been saved as a timesheet yet, so these hours may still change.", [0x8A, 0x3A, 0x22], true)
        }
    }
}

/// An earlier (reopened) version of a saved timesheet.
pub fn revision_pdf(data: &Data, revision: &Revision) -> Vec<u8> {
    let status = format!(
        "Earlier saved version · saved {}, reopened for editing {} · kept for your records",
        date_time(revision.sheet.saved_at),
        date_time(revision.reopened_at)
    );
    sheet_document(data, &revision.sheet, &status, MUTED, false)
}

/// The totalled-out timesheet for a period.
pub fn report_pdf(data: &Data, report: &Report, detail: bool) -> Vec<u8> {
    let label = report.label();
    let mut doc = Doc::new(true, &format!("Timesheet {label}"));
    let scope = if report.saved_only { "saved timesheets only" } else { "all recorded time" };
    let mut y = doc.header(data, "Timesheet", &format!("{label} · {scope}"));

    // Key figures
    let boxes = [
        ("TOTAL HOURS", fmt_h(report.total())),
        ("PROJECTS", report.projects.len().to_string()),
        ("DAYS WITH TIME", report.days_worked().to_string()),
        ("AVERAGE PER DAY", report.avg_per_day()),
    ];
    let bw = (doc.content_width() - 9.0) / 4.0;
    for (i, (name, value)) in boxes.iter().enumerate() {
        let x = MARGIN + i as f32 * (bw + 3.0);
        doc.stroke_rect(x, y, bw, 13.0, GRID, 0.6);
        doc.text(x + 3.0, y + 4.5, 7.0, false, MUTED, name, Align::Left);
        doc.text(x + 3.0, y + 10.5, 13.0, true, TEXT, value, Align::Left);
    }
    y += 18.0;
    if report.includes_unsaved() {
        y = doc.status(y, &format!("Includes {} h from weeks not yet saved as timesheets (marked *).", fmt_h(report.unsaved_hh)), [0x8A, 0x3A, 0x22]);
    } else if report.saved_only && report.unsaved_hh > 0 {
        y = doc.status(y, &format!("{} h from unsaved weeks are not included.", fmt_h(report.unsaved_hh)), MUTED);
    }

    let m = &report.matrix;
    if report.projects.is_empty() {
        y = doc.status(y, "No time recorded in this period.", MUTED);
    } else {
        let n = m.columns.len() as f32;
        let (code_w, total_w, share_w) = (16.0, 16.0, 13.0);
        let col_w = ((doc.content_width() - 60.0 - code_w - total_w - share_w) / n).clamp(11.0, 24.0);
        let name_w = doc.content_width() - code_w - total_w - share_w - col_w * n;
        let size = if n > 10.0 { 7.5 } else { 8.5 };
        let headers: Vec<String> = m.columns.iter().map(|c| format!("{}{}", c.label, if c.partial { "*" } else { "" })).collect();
        let mut cols: Vec<(&str, f32, bool)> = vec![("Project", name_w, false), ("Code", code_w, false)];
        for h in &headers {
            cols.push((h.as_str(), col_w, true));
        }
        cols.push(("Total", total_w, true));
        cols.push(("Share", share_w, true));
        let rows: Vec<Vec<String>> = report
            .projects
            .iter()
            .zip(&m.cells)
            .map(|(p, cells)| {
                let mut row = vec![p.display(), p.code.clone()];
                row.extend(cells.iter().map(|&hh| if hh > 0 { fmt_h(hh) } else { String::new() }));
                row.push(fmt_h(p.hh));
                row.push(format!("{:.1}%", p.share));
                row
            })
            .collect();
        let mut footer = vec!["Total".to_string(), String::new()];
        footer.extend(m.column_totals().iter().map(|&hh| fmt_h(hh)));
        footer.push(fmt_h(report.total()));
        footer.push("100%".into());
        y = doc.table(y, &cols, &rows, Some(&footer), size);
        let note = format!(
            "Columns are {}s.{}",
            m.unit,
            if m.has_partial() { format!(" * only part of this {} falls inside the period.", m.unit) } else { String::new() }
        );
        doc.text(MARGIN, y, 7.5, false, MUTED, &note, Align::Left);
        y += 5.0;
    }

    if detail && !report.lines.is_empty() {
        doc.new_page();
        let mut yy = doc.heading(MARGIN + 2.0, "Daily detail");
        let cols: [(&str, f32, bool); 5] = [
            ("Date", 34.0, false),
            ("Project (as recorded)", doc.content_width() - 34.0 - 22.0 - 26.0 - 20.0, false),
            ("Code", 22.0, false),
            ("Status", 26.0, false),
            ("Hours", 20.0, true),
        ];
        let rows: Vec<Vec<String>> = report
            .lines
            .iter()
            .map(|l| {
                vec![
                    format!("{} {}", DAY_NAMES[l.day.weekday_index()], model::long_date(l.day)),
                    l.name.clone(),
                    l.code.clone(),
                    if l.saved { "Saved".into() } else { "Not saved *".into() },
                    fmt_h(l.hh),
                ]
            })
            .collect();
        let footer = vec!["Total".to_string(), String::new(), String::new(), String::new(), fmt_h(report.total())];
        yy = doc.table(yy, &cols, &rows, Some(&footer), 8.5);
        y = yy;
    }
    doc.signatures(y);
    doc.finish(&format!("Tiny Time Track Lite · timesheet {label}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::add_days;
    use chrono::NaiveDate;

    fn sample() -> Data {
        let mut data = Data::default();
        data.settings.user_name = "Dirk Cilliers".into();
        let a = data.add_project("Longlands Clubhouse", "3676", "").unwrap();
        let b = data.add_project("Chekkers", "3785", "").unwrap();
        let mon = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        for i in 0..5 {
            let day = add_days(mon, i);
            data.add_block(day, Lane::Plan, a, 16, 24).unwrap();
            data.add_block(day, Lane::Actual, a, 16, 25).unwrap();
            data.add_block(day, Lane::Actual, b, 26, 32).unwrap();
        }
        data.set_note(mon, "Concept issue Friday – don’t forget").unwrap();
        data
    }

    fn assert_pdf(bytes: &[u8], needles: &[&str]) {
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(bytes.ends_with(b"%%EOF") || bytes.ends_with(b"%%EOF\n"));
        // Strings with non-ASCII bytes (e.g. "·") are written hex-encoded, so look for both forms.
        let text = String::from_utf8_lossy(bytes).to_lowercase();
        for needle in needles {
            let hex: String = win_ansi(needle).iter().map(|b| format!("{b:02x}")).collect();
            assert!(
                text.contains(&needle.to_lowercase()) || text.contains(&hex),
                "PDF should contain {needle:?}"
            );
        }
    }

    #[test]
    fn plan_timesheet_and_report_pdfs() {
        let mut data = sample();
        let mon = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        assert_pdf(&plan_pdf(&data, mon), &["WEEKLY PLAN", "Dirk Cilliers", "Planned hours", "Helvetica"]);
        assert_pdf(&timesheet_pdf(&data, mon), &["DRAFT", "TIMESHEET", "Signature"]);
        data.save_timesheet(mon, model::now()).unwrap();
        let saved = timesheet_pdf(&data, mon);
        assert_pdf(&saved, &["Saved record"]);
        assert!(!String::from_utf8_lossy(&saved).contains("(DRAFT)"));
        data.reopen_timesheet(mon, model::now()).unwrap();
        assert_pdf(&revision_pdf(&data, &data.revisions[0]), &["Earlier saved version"]);
        let report = data.report(mon, add_days(mon, 6), false);
        assert_pdf(&report_pdf(&data, &report, true), &["Daily detail", "Share", "page 2 of 2"]);
    }

    #[test]
    fn plan_lists_todos_and_rfi_register_prints() {
        let mut data = sample();
        let mon = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        let a = data.projects[0].id;
        data.add_todo(a, add_days(mon, -7), "Issue drawings").unwrap();
        let t = data.add_todo(a, mon, "Call engineer").unwrap();
        data.toggle_todo(t, mon).unwrap();
        assert_pdf(&plan_pdf(&data, mon), &["To do this week", "Issue drawings", "carried over from 9 Nov", "Call engineer"]);
        data.set_project_stage(a, Some(5)).unwrap();
        data.add_rfi(a, "Window sill height", "North elevation, W3?", mon, Some(add_days(mon, 3))).unwrap();
        let id = data.rfis[0].id;
        data.answer_rfi(id, add_days(mon, 1), "900 mm above FFL").unwrap();
        data.add_rfi(a, "Roof fall", "", mon, None).unwrap();
        assert_pdf(
            &rfi_register_pdf(&data, a),
            &["RFI REGISTER", "Window sill height", "900 mm above FFL", "Questions and answers", "Stage 5"],
        );
    }

    #[test]
    fn text_encoding_and_widths() {
        assert_eq!(win_ansi("a–b…é€☃"), vec![b'a', 0x96, b'b', 0x85, 0xE9, 0x80, b'?']);
        assert!((text_width("W", 10.0, false) - 0.944 * 10.0 / PT).abs() < 1e-4);
        assert!(text_width("Hello", 10.0, true) > text_width("Hello", 10.0, false));
        let short = fit("A very long project name indeed", 20.0, 9.0, false);
        assert!(short.ends_with('…') && text_width(&short, 9.0, false) <= 20.0);
    }
}
