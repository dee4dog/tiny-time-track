//! Tiny Time Track Lite - a standalone weekly planner and timesheet for one
//! person on one computer. No sign-in, no server: just this program and one
//! data file.
#![cfg_attr(not(feature = "dev"), windows_subsystem = "windows")]

mod exports;
mod model;
mod pdf;
mod store;
mod ui;
mod week;

#[cfg(feature = "dev")]
mod dev;

use eframe::egui;

/// A terracotta clock face, drawn in code (no image file needed).
fn icon() -> egui::IconData {
    const SIZE: u32 = 64;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let c = (SIZE as f32 - 1.0) / 2.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (dx, dy) = (x as f32 - c, y as f32 - c);
            let r = (dx * dx + dy * dy).sqrt();
            if r > 31.5 {
                continue;
            }
            let alpha = ((31.5 - r).clamp(0.0, 1.0) * 255.0) as u8;
            let hand = (dx.abs() <= 2.6 && (-21.0..=1.5).contains(&dy)) || (dy.abs() <= 2.6 && (-1.5..=15.0).contains(&dx));
            let rim = r > 27.0;
            let color = if hand { [255, 255, 255] } else if rim { [0x8F, 0x46, 0x30] } else { [0xB0, 0x5A, 0x3E] };
            let i = ((y * SIZE + x) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&[color[0], color[1], color[2], alpha]);
        }
    }
    egui::IconData { rgba, width: SIZE, height: SIZE }
}

fn main() -> eframe::Result {
    #[cfg(feature = "dev")]
    let dev = dev::Options::from_args();
    let dir = store::default_dir();
    #[cfg(feature = "dev")]
    if dev.demo {
        dev::seed_demo(&dir);
    }

    let app = match store::acquire_lock(&dir) {
        Err(message) => ui::App::fatal(message, dir),
        Ok(lock) => match store::load(&dir) {
            Err(message) => ui::App::fatal(message, dir),
            Ok((data, warning)) => {
                store::backup_if_due(&dir);
                ui::App::new(data, dir, Some(lock), warning)
            }
        },
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Tiny Time Track Lite")
            .with_inner_size([1320.0, 880.0])
            .with_min_inner_size([880.0, 560.0])
            .with_icon(std::sync::Arc::new(icon())),
        ..Default::default()
    };
    eframe::run_native(
        "Tiny Time Track Lite",
        options,
        Box::new(move |cc| {
            ui::setup(&cc.egui_ctx);
            #[cfg(feature = "dev")]
            return Ok(Box::new(dev::Wrapper::new(app, dev)));
            #[cfg(not(feature = "dev"))]
            Ok(Box::new(app))
        }),
    )
}
