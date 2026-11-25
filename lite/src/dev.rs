//! Development helpers, compiled only with `--features dev` (never shipped):
//! `--demo` seeds sample data, `--screenshot FILE` saves a PNG of the window
//! and exits, `--tab NAME` / `--week YYYY-MM-DD` choose what to show,
//! `--select` selects the first block of the week.

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use eframe::egui;

use crate::model::{self, Data, Lane, add_days};
use crate::store;
use crate::ui::{App, Tab};

#[derive(Default)]
pub struct Options {
    pub demo: bool,
    screenshot: Option<PathBuf>,
    tab: Option<String>,
    week: Option<NaiveDate>,
    select: bool,
    rfi: bool,
}

impl Options {
    pub fn from_args() -> Self {
        let mut o = Options::default();
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--demo" => o.demo = true,
                "--select" => o.select = true,
                "--rfi" => o.rfi = true,
                "--screenshot" => o.screenshot = args.next().map(PathBuf::from),
                "--tab" => o.tab = args.next(),
                "--week" => o.week = args.next().and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
                _ => {}
            }
        }
        o
    }
}

pub struct Wrapper {
    app: App,
    options: Options,
    frames: u32,
}

impl Wrapper {
    pub fn new(mut app: App, options: Options) -> Self {
        if let Some(tab) = &options.tab {
            app.set_tab(match tab.as_str() {
                "projects" => Tab::Projects,
                "timesheets" => Tab::Timesheets,
                "reports" => Tab::Reports,
                "settings" => Tab::Settings,
                _ => Tab::Week,
            });
        }
        if let Some(week) = options.week {
            app.go_to_week(week);
        }
        if options.select {
            app.selected = app.data.week_blocks(app.week).iter().find(|b| b.lane == Lane::Actual).map(|b| b.id);
        }
        if options.rfi {
            app.rfi_project = app.data.projects.iter().find(|p| p.stage == Some(model::ON_SITE_STAGE)).map(|p| p.id);
        }
        Wrapper { app, options, frames: 0 }
    }
}

impl eframe::App for Wrapper {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.update(ctx, frame);
        self.frames += 1;
        let Some(path) = self.options.screenshot.clone() else { return };
        if self.frames == 6 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = shot {
            let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Err(e) = write_png(&path, image.size[0] as u32, image.size[1] as u32, &rgba) {
                eprintln!("screenshot failed: {e}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        self.app.on_exit(gl);
    }
}

/// Minimal PNG writer (uncompressed deflate) - dev only.
fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> std::io::Result<()> {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in bytes {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(data);
        out.extend(&body);
        out.extend(crc32(&body).to_be_bytes());
    }
    let mut raw = Vec::with_capacity(rgba.len() + height as usize);
    for row in rgba.chunks((width * 4) as usize) {
        raw.push(0);
        raw.extend(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, block) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let len = block.len() as u16;
        z.extend(len.to_le_bytes());
        z.extend((!len).to_le_bytes());
        z.extend(*block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend(width.to_be_bytes());
    ihdr.extend(height.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    std::fs::write(path, out)
}

/// Realistic sample data for screenshots and evaluation (only into an empty folder).
pub fn seed_demo(dir: &Path) {
    if dir.join(store::DATA_FILE).exists() {
        return;
    }
    let mut data = Data::default();
    data.settings.user_name = "Dirk Cilliers".into();
    data.settings.company = "ES Architects".into();
    let ids: Vec<u32> = [
        ("Longlands Clubhouse", "3676", "Longlands HOA"),
        ("Chekkers", "3785", "Chekkers Retail"),
        ("Van Rijn Meent", "3671", ""),
        ("General / Admin", "C-0001", ""),
    ]
    .iter()
    .map(|(n, c, k)| data.add_project(n, c, k).expect("demo project"))
    .collect();
    let this_week = model::monday_of(model::today());
    // (project index, start slot, end slot) per weekday
    let plan: [&[(usize, u8, u8)]; 5] = [
        &[(3, 16, 17), (0, 17, 24), (1, 26, 34)],
        &[(0, 16, 24), (2, 26, 30), (1, 30, 34)],
        &[(1, 16, 22), (0, 22, 24), (0, 26, 34)],
        &[(0, 16, 24), (2, 26, 34)],
        &[(1, 16, 24), (3, 26, 28), (0, 28, 32)],
    ];
    let actual: [&[(usize, u8, u8)]; 5] = [
        &[(3, 16, 18), (0, 18, 25), (1, 26, 35)],
        &[(0, 16, 23), (2, 26, 31), (1, 31, 34)],
        &[(1, 16, 23), (0, 23, 24), (0, 26, 33)],
        &[(0, 15, 24), (2, 26, 35)],
        &[(1, 16, 24), (3, 26, 27), (0, 27, 31)],
    ];
    for (week, actual_days) in [(add_days(this_week, -7), 5i64), (this_week, (model::today() - this_week).num_days() + 1)] {
        for day in 0..5usize {
            let date = add_days(week, day as i64);
            for &(p, s, e) in plan[day] {
                let _ = data.add_block(date, Lane::Plan, ids[p], s, e);
            }
            if (day as i64) < actual_days {
                for &(p, s, e) in actual[day] {
                    let _ = data.add_block(date, Lane::Actual, ids[p], s, e);
                }
            }
        }
    }
    let last = add_days(this_week, -7);
    let _ = data.set_note(last, "Longlands concept issue Thursday.");
    let _ = data.save_timesheet(last, model::now());
    let _ = data.set_note(this_week, "Chekkers council submission Friday.\nCatch up on Van Rijn site notes.");
    // Stages, to-dos and RFIs
    let _ = data.set_project_stage(ids[0], Some(4));
    let _ = data.set_project_stage(ids[1], Some(5));
    let _ = data.set_project_stage(ids[2], Some(3));
    let _ = data.add_todo(ids[0], last, "Coordinate structural layout with engineer");
    let done = data.add_todo(ids[0], last, "Issue council submission set").unwrap_or(0);
    let _ = data.toggle_todo(done, last);
    let _ = data.add_todo(ids[0], this_week, "Door & window schedule");
    let site = data.add_todo(ids[1], this_week, "Site meeting Wednesday 10:00").unwrap_or(0);
    let _ = data.toggle_todo(site, this_week);
    let _ = data.add_todo(ids[1], this_week, "Respond to contractor on paving");
    let _ = data.add_todo(ids[2], last, "Revise floor plan options");
    let _ = data.add_rfi(
        ids[1],
        "Shopfront sill height",
        "North elevation W3 - drawing shows 900, detail shows 600?",
        add_days(last, 1),
        Some(add_days(last, 4)),
    );
    let _ = data.add_rfi(ids[1], "Paving pattern at entrance", "Herringbone or stretcher bond?", add_days(last, 3), Some(add_days(this_week, -1)));
    let _ = data.add_rfi(ids[1], "Roof sheeting colour", "", this_week, Some(add_days(this_week, 7)));
    if let Some(first) = data.rfis.first().map(|r| r.id) {
        let _ = data.answer_rfi(first, add_days(last, 2), "900 mm above FFL as per elevation.");
    }
    let _ = store::save(dir, &data);
}
