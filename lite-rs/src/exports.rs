//! Excel and CSV exports of a period report, and where export files go.

use std::path::{Path, PathBuf};

use rust_xlsxwriter::{Color, ExcelDateTime, Format, FormatAlign, Formula, Workbook, Worksheet, XlsxError};

use crate::model::{DAY_NAMES, Data, Report, week_label};

const HOURS: &str = "0.00";

fn hours(hh: u32) -> f64 {
    f64::from(hh) / 2.0
}

fn col_letter(mut col: u16) -> String {
    let mut s = String::new();
    col += 1;
    while col > 0 {
        let rem = (col - 1) % 26;
        s.insert(0, (b'A' + rem as u8) as char);
        col = (col - 1) / 26;
    }
    s
}

struct Styles {
    title: Format,
    head: Format,
    head_right: Format,
    num: Format,
    num_bold: Format,
    pct: Format,
    pct_bold: Format,
    bold: Format,
    date: Format,
    muted: Format,
}

fn styles() -> Styles {
    let head = Format::new().set_bold().set_background_color(Color::RGB(0xF5E2D8)).set_font_color(Color::RGB(0x3B2C25));
    Styles {
        title: Format::new().set_bold().set_font_size(14).set_font_color(Color::RGB(0x8F4630)),
        head_right: head.clone().set_align(FormatAlign::Right),
        head,
        num: Format::new().set_num_format(HOURS),
        num_bold: Format::new().set_num_format(HOURS).set_bold().set_background_color(Color::RGB(0xFBEEE7)),
        pct: Format::new().set_num_format("0.0%"),
        pct_bold: Format::new().set_num_format("0.0%").set_bold().set_background_color(Color::RGB(0xFBEEE7)),
        bold: Format::new().set_bold().set_background_color(Color::RGB(0xFBEEE7)),
        date: Format::new().set_num_format("yyyy-mm-dd"),
        muted: Format::new().set_font_color(Color::RGB(0x6B5246)),
    }
}

/// Title lines; returns the next free row.
fn heading(ws: &mut Worksheet, s: &Styles, data: &Data, report: &Report, title: &str) -> Result<u32, XlsxError> {
    ws.write_string_with_format(0, 0, title, &s.title)?;
    let mut row = 1;
    let who: Vec<&str> =
        [data.settings.user_name.as_str(), data.settings.company.as_str()].into_iter().filter(|v| !v.is_empty()).collect();
    if !who.is_empty() {
        ws.write_string(row, 0, who.join(" · "))?;
        row += 1;
    }
    ws.write_string(row, 0, format!("Period: {}", report.label()))?;
    let scope = if report.saved_only { "Saved timesheets only" } else { "Saved timesheets plus weeks not yet saved" };
    ws.write_string_with_format(row + 1, 0, scope, &s.muted)?;
    Ok(row + 3)
}

fn page_setup(ws: &mut Worksheet, header_row: u32, freeze_col: u16, landscape: bool) -> Result<(), XlsxError> {
    if landscape {
        ws.set_landscape();
    }
    ws.set_paper_size(9); // A4
    ws.set_print_fit_to_pages(1, 0);
    ws.set_repeat_rows(header_row, header_row)?;
    ws.set_freeze_panes(header_row + 1, freeze_col)?;
    Ok(())
}

/// A text cell; a name starting with '=' stays text (no formula injection).
fn text(ws: &mut Worksheet, row: u32, col: u16, value: &str) -> Result<(), XlsxError> {
    ws.write_string(row, col, value)?;
    Ok(())
}

fn sum(ws: &mut Worksheet, row: u32, col: u16, range: &str, cached: f64, fmt: &Format) -> Result<(), XlsxError> {
    let formula = Formula::new(format!("=SUM({range})")).set_result(format!("{cached}"));
    ws.write_formula_with_format(row, col, formula, fmt)?;
    Ok(())
}

pub fn report_xlsx(data: &Data, report: &Report) -> Result<Vec<u8>, String> {
    build_xlsx(data, report).map_err(|e| e.to_string())
}

fn build_xlsx(data: &Data, report: &Report) -> Result<Vec<u8>, XlsxError> {
    let s = styles();
    let mut wb = Workbook::new();
    let m = &report.matrix;

    // ---- Timesheet: projects x weeks/months, totals both ways ----
    let ws = wb.add_worksheet().set_name("Timesheet")?;
    let header = heading(ws, &s, data, report, "Timesheet")?;
    ws.write_string_with_format(header, 0, "Code", &s.head)?;
    ws.write_string_with_format(header, 1, "Project", &s.head)?;
    let first_col: u16 = 2;
    for (i, c) in m.columns.iter().enumerate() {
        let label = format!("{}{}", c.label, if c.partial { "*" } else { "" });
        ws.write_string_with_format(header, first_col + i as u16, label, &s.head_right)?;
    }
    let total_col = first_col + m.columns.len() as u16;
    ws.write_string_with_format(header, total_col, "Total", &s.head_right)?;
    let first_row = header + 1;
    for (r, (p, cells)) in report.projects.iter().zip(&m.cells).enumerate() {
        let row = first_row + r as u32;
        text(ws, row, 0, &p.code)?;
        text(ws, row, 1, &p.display())?;
        for (c, &hh) in cells.iter().enumerate() {
            if hh > 0 {
                ws.write_number_with_format(row, first_col + c as u16, hours(hh), &s.num)?;
            }
        }
        let range = format!("{}{}:{}{}", col_letter(first_col), row + 1, col_letter(total_col - 1), row + 1);
        sum(ws, row, total_col, &range, hours(p.hh), &s.num_bold)?;
    }
    let total_row = first_row + report.projects.len() as u32;
    ws.write_string_with_format(total_row, 0, "", &s.bold)?;
    ws.write_string_with_format(total_row, 1, "Total", &s.bold)?;
    let column_totals = m.column_totals();
    for c in 0..=m.columns.len() {
        let col = first_col + c as u16;
        let cached = if c < m.columns.len() { hours(column_totals[c]) } else { hours(report.total()) };
        if report.projects.is_empty() {
            ws.write_number_with_format(total_row, col, 0.0, &s.num_bold)?;
        } else {
            let range = format!("{0}{1}:{0}{2}", col_letter(col), first_row + 1, total_row);
            sum(ws, total_row, col, &range, cached, &s.num_bold)?;
        }
    }
    if m.has_partial() {
        ws.write_string_with_format(total_row + 2, 0, format!("* only part of this {} falls inside the period", m.unit), &s.muted)?;
    }
    ws.set_column_width(0, 12)?;
    ws.set_column_width(1, 40)?;
    for c in first_col..=total_col {
        ws.set_column_width(c, 11)?;
    }
    page_setup(ws, header, 2, true)?;

    // ---- Summary ----
    let ws = wb.add_worksheet().set_name("Summary")?;
    let header = heading(ws, &s, data, report, "Time per project")?;
    for (c, (name, right)) in [("Code", false), ("Project", false), ("Hours", true), ("Share", true)].iter().enumerate() {
        ws.write_string_with_format(header, c as u16, *name, if *right { &s.head_right } else { &s.head })?;
    }
    for (r, p) in report.projects.iter().enumerate() {
        let row = header + 1 + r as u32;
        text(ws, row, 0, &p.code)?;
        text(ws, row, 1, &p.display())?;
        ws.write_number_with_format(row, 2, hours(p.hh), &s.num)?;
        ws.write_number_with_format(row, 3, p.share / 100.0, &s.pct)?;
    }
    let row = header + 1 + report.projects.len() as u32;
    ws.write_string_with_format(row, 1, "Total", &s.bold)?;
    if report.projects.is_empty() {
        ws.write_number_with_format(row, 2, 0.0, &s.num_bold)?;
    } else {
        sum(ws, row, 2, &format!("C{}:C{}", header + 2, row), hours(report.total()), &s.num_bold)?;
        sum(ws, row, 3, &format!("D{}:D{}", header + 2, row), 1.0, &s.pct_bold)?;
    }
    ws.set_column_width(0, 12)?;
    ws.set_column_width(1, 46)?;
    ws.set_column_width(2, 11)?;
    ws.set_column_width(3, 9)?;
    page_setup(ws, header, 0, false)?;

    // ---- By week ----
    let ws = wb.add_worksheet().set_name("By week")?;
    let header = heading(ws, &s, data, report, "Hours by week")?;
    for (c, (name, right)) in [("Week starting", false), ("Week", false), ("Status", false), ("Hours", true)].iter().enumerate() {
        ws.write_string_with_format(header, c as u16, *name, if *right { &s.head_right } else { &s.head })?;
    }
    for (r, w) in report.weeks.iter().enumerate() {
        let row = header + 1 + r as u32;
        let date = ExcelDateTime::parse_from_str(&w.start.format("%Y-%m-%d").to_string())?;
        ws.write_date_with_format(row, 0, &date, &s.date)?;
        ws.write_string(row, 1, format!("{}{}", week_label(w.start), if w.partial { " (part)" } else { "" }))?;
        ws.write_string(row, 2, if w.saved { "Saved" } else if w.hh > 0 { "Not saved" } else { "" })?;
        ws.write_number_with_format(row, 3, hours(w.hh), &s.num)?;
    }
    let row = header + 1 + report.weeks.len() as u32;
    ws.write_string_with_format(row, 2, "Total", &s.bold)?;
    sum(ws, row, 3, &format!("D{}:D{}", header + 2, row), hours(report.total()), &s.num_bold)?;
    ws.set_column_width(0, 14)?;
    ws.set_column_width(1, 30)?;
    ws.set_column_width(2, 11)?;
    ws.set_column_width(3, 10)?;
    page_setup(ws, header, 0, false)?;

    // ---- Daily detail ----
    let ws = wb.add_worksheet().set_name("Daily detail")?;
    let header = heading(ws, &s, data, report, "Daily detail")?;
    for (c, (name, right)) in
        [("Date", false), ("Day", false), ("Code", false), ("Project (as recorded)", false), ("Hours", true), ("Status", false)]
            .iter()
            .enumerate()
    {
        ws.write_string_with_format(header, c as u16, *name, if *right { &s.head_right } else { &s.head })?;
    }
    for (r, l) in report.lines.iter().enumerate() {
        let row = header + 1 + r as u32;
        let date = ExcelDateTime::parse_from_str(&l.day.format("%Y-%m-%d").to_string())?;
        ws.write_date_with_format(row, 0, &date, &s.date)?;
        ws.write_string(row, 1, DAY_NAMES[chrono::Datelike::weekday(&l.day).num_days_from_monday() as usize])?;
        text(ws, row, 2, &l.code)?;
        text(ws, row, 3, &l.name)?;
        ws.write_number_with_format(row, 4, hours(l.hh), &s.num)?;
        ws.write_string(row, 5, if l.saved { "Saved" } else { "Not saved" })?;
    }
    let row = header + 1 + report.lines.len() as u32;
    ws.write_string_with_format(row, 3, "Total", &s.bold)?;
    if report.lines.is_empty() {
        ws.write_number_with_format(row, 4, 0.0, &s.num_bold)?;
    } else {
        sum(ws, row, 4, &format!("E{}:E{}", header + 2, row), hours(report.total()), &s.num_bold)?;
    }
    ws.set_column_width(0, 12)?;
    ws.set_column_width(1, 6)?;
    ws.set_column_width(2, 12)?;
    ws.set_column_width(3, 40)?;
    ws.set_column_width(4, 10)?;
    ws.set_column_width(5, 11)?;
    page_setup(ws, header, 0, false)?;

    wb.save_to_buffer()
}

// --------------------------------------------------------------------------
//  CSV
// --------------------------------------------------------------------------
fn csv_field(value: &str) -> String {
    // Neutralise values a spreadsheet would treat as a formula.
    let value = if value.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{value}") } else { value.to_string() };
    if value.contains([',', '"', '\n', '\r']) { format!("\"{}\"", value.replace('"', "\"\"")) } else { value }
}

fn csv(rows: Vec<Vec<String>>) -> Vec<u8> {
    let mut out = String::from("\u{FEFF}"); // BOM so Excel reads accents correctly
    for row in rows {
        out += &row.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(",");
        out += "\r\n";
    }
    out.into_bytes()
}

/// One row per day and project.
pub fn report_csv_daily(report: &Report) -> Vec<u8> {
    let mut rows = vec![vec!["Date", "Day", "Project code", "Project", "Hours", "Status"].into_iter().map(String::from).collect()];
    for l in &report.lines {
        rows.push(vec![
            l.day.format("%Y-%m-%d").to_string(),
            DAY_NAMES[chrono::Datelike::weekday(&l.day).num_days_from_monday() as usize].to_string(),
            l.code.clone(),
            l.name.clone(),
            format!("{:.2}", hours(l.hh)),
            if l.saved { "Saved" } else { "Not saved" }.to_string(),
        ]);
    }
    csv(rows)
}

/// The totalled grid: projects x weeks/months with totals both ways.
pub fn report_csv_totals(report: &Report) -> Vec<u8> {
    let m = &report.matrix;
    let mut header = vec!["Project code".to_string(), "Project".to_string()];
    header.extend(m.columns.iter().map(|c| format!("{} to {}", c.start.format("%Y-%m-%d"), c.end.format("%Y-%m-%d"))));
    header.push("Total".into());
    let mut rows = vec![header];
    for (p, cells) in report.projects.iter().zip(&m.cells) {
        let mut row = vec![p.code.clone(), p.display()];
        row.extend(cells.iter().map(|&hh| format!("{:.2}", hours(hh))));
        row.push(format!("{:.2}", hours(p.hh)));
        rows.push(row);
    }
    let mut total = vec![String::new(), "Total".to_string()];
    total.extend(m.column_totals().iter().map(|&hh| format!("{:.2}", hours(hh))));
    total.push(format!("{:.2}", hours(report.total())));
    rows.push(total);
    csv(rows)
}

// --------------------------------------------------------------------------
//  Files
// --------------------------------------------------------------------------
pub fn export_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TTT_EXPORT_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::document_dir().unwrap_or_else(|| PathBuf::from(".")).join("Tiny Time Track")
}

/// Write an export, picking "name (2).pdf" etc. if the file is open elsewhere.
pub fn write_export(name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let dir = export_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    for n in 1..100 {
        let candidate = if n == 1 { dir.join(name) } else { dir.join(format!("{stem} ({n}).{ext}")) };
        if std::fs::write(&candidate, bytes).is_ok() {
            return Ok(candidate);
        }
    }
    Err(format!("Couldn't write {name} in {}", dir.display()))
}

/// Open a file or folder with the system's default program.
pub fn open_path(path: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

/// Show a file selected in its folder.
pub fn reveal(path: &Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    }
    #[cfg(not(windows))]
    open_path(path.parent().unwrap_or(path));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Lane, add_days};
    use chrono::NaiveDate;

    fn report() -> (Data, Report) {
        let mut data = Data::default();
        let a = data.add_project("Alpha", "A1", "").unwrap();
        let b = data.add_project("=cmd()", "", "").unwrap();
        let mon = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        for i in 0..5 {
            data.add_block(add_days(mon, i), Lane::Actual, a, 16, 29).unwrap(); // 6.5 h
            data.add_block(add_days(mon, i), Lane::Actual, b, 30, 33).unwrap(); // 1.5 h
        }
        let r = data.report(mon, add_days(mon, 6), false);
        (data, r)
    }

    #[test]
    fn xlsx_is_a_workbook() {
        let (data, r) = report();
        let bytes = report_xlsx(&data, &r).unwrap();
        assert!(bytes.starts_with(b"PK"), "xlsx is a zip file");
        assert!(bytes.len() > 4000);
    }

    #[test]
    fn csv_totals_and_daily() {
        let (_, r) = report();
        let totals = String::from_utf8(report_csv_totals(&r)).unwrap();
        let lines: Vec<&str> = totals.trim_start_matches('\u{FEFF}').lines().collect();
        assert_eq!(lines[0], "Project code,Project,2026-11-16 to 2026-11-22,Total");
        assert_eq!(lines[1], "A1,Alpha,32.50,32.50");
        assert_eq!(lines[2], ",'=cmd(),7.50,7.50");
        assert_eq!(lines[3], ",Total,40.00,40.00");
        let daily = String::from_utf8(report_csv_daily(&r)).unwrap();
        assert_eq!(daily.lines().count(), 11);
        assert!(daily.contains("2026-11-16,Mon,A1,Alpha,6.50,Not saved"));
    }

    #[test]
    fn helpers() {
        assert_eq!(col_letter(0), "A");
        assert_eq!(col_letter(25), "Z");
        assert_eq!(col_letter(26), "AA");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
    }
}
