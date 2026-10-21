//! fern-icon-sheet: renders icons in all five modes into one PNG, for
//! reviewing the icon set and for the HIG's pictures.
//!
//!   fern-icon-sheet --dir DIR [--dir DIR ...] [--size PX] [--accent #rrggbb]
//!                   [--wallpaper PNG] --out SHEET.png [ID ...]
//!
//! One row per icon (every icon in the directories when no ids are
//! given), one column per mode: Default, Dark, Tinted, Wallpaper, Clear,
//! each on a backdrop like the one it is meant for.

use std::path::PathBuf;
use std::process::ExitCode;

use fernicons::{Engine, Mode, Request};
use resvg::tiny_skia::{Color, IntSize, Paint, Pixmap, PixmapPaint, Rect, Transform};

struct Args {
    dirs: Vec<PathBuf>,
    size: u32,
    accent: u32,
    wallpaper: Option<PathBuf>,
    out: PathBuf,
    ids: Vec<String>,
}

fn parse() -> Result<Args, String> {
    let mut args = Args {
        dirs: Vec::new(),
        size: 96,
        accent: 0x002D_64C8,
        wallpaper: None,
        out: PathBuf::new(),
        ids: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut value = || it.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--dir" => args.dirs.push(value()?.into()),
            "--size" => args.size = value()?.parse().map_err(|e| format!("--size: {e}"))?,
            "--accent" => {
                let v = value()?;
                args.accent = v
                    .strip_prefix('#')
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .ok_or(format!("--accent {v}: not #rrggbb"))?;
            }
            "--wallpaper" => args.wallpaper = Some(value()?.into()),
            "--out" => args.out = value()?.into(),
            _ if a.starts_with('-') => return Err(format!("unknown option {a}")),
            _ => args.ids.push(a),
        }
    }
    if args.dirs.is_empty() || args.out.as_os_str().is_empty() {
        return Err("usage: fern-icon-sheet --dir DIR ... --out SHEET.png [ID ...]".into());
    }
    Ok(args)
}

/// Every icon id in the directories, sorted.
fn all_ids(dirs: &[PathBuf]) -> Vec<String> {
    let mut ids: Vec<String> = dirs
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|e| {
            e.file_name()
                .to_str()?
                .strip_suffix(".fernicon")
                .map(str::to_string)
        })
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

fn run() -> Result<(), String> {
    let args = parse()?;
    let engine = Engine::new(args.dirs.clone(), None);
    let ids = if args.ids.is_empty() {
        all_ids(&args.dirs)
    } else {
        args.ids.clone()
    };
    if ids.is_empty() {
        return Err("no icons found".into());
    }
    let wallpaper_accent = match &args.wallpaper {
        Some(w) => fernicons::wallpaper_accent(w).ok_or(format!("cannot read {}", w.display()))?,
        None => args.accent,
    };
    let cell = args.size + args.size / 4;
    let cols = u32::try_from(Mode::ALL.len()).unwrap_or(5);
    let rows = u32::try_from(ids.len()).map_err(|_| "too many icons")?;
    let mut sheet = Pixmap::new(cell * cols, cell * rows).ok_or("sheet too large")?;
    // Backdrops: light, dark, dark, dark green, and a blue-green for glass.
    let backdrops = [
        0x00EC_EFE9,
        0x001A_1F1C,
        0x001A_1F1C,
        0x0023_3631,
        0x003B_6E8C,
    ];
    for (col, (&mode, &backdrop)) in Mode::ALL.iter().zip(backdrops.iter()).enumerate() {
        let col = u32::try_from(col).unwrap_or(0);
        let mut paint = Paint::default();
        let [_, r, g, b] = u32::to_be_bytes(backdrop);
        paint.set_color(Color::from_rgba8(r, g, b, 255));
        #[allow(clippy::cast_precision_loss)]
        let rect = Rect::from_xywh((col * cell) as f32, 0.0, cell as f32, (cell * rows) as f32)
            .ok_or("bad rect")?;
        sheet.fill_rect(rect, &paint, Transform::identity(), None);
        for (row, id) in ids.iter().enumerate() {
            let row = u32::try_from(row).unwrap_or(0);
            let accent = if mode == Mode::Wallpaper {
                wallpaper_accent
            } else {
                args.accent
            };
            let image = engine
                .render(&Request {
                    id,
                    legacy: None,
                    mode,
                    accent,
                    size: args.size,
                })
                .map_err(|e| format!("{id}: {e}"))?;
            let icon = Pixmap::from_vec(
                image.data.clone(),
                IntSize::from_wh(image.size, image.size).ok_or("size")?,
            )
            .ok_or("bad image")?;
            let x = i32::try_from(col * cell + (cell - args.size) / 2).unwrap_or(0);
            let y = i32::try_from(row * cell + (cell - args.size) / 2).unwrap_or(0);
            sheet.draw_pixmap(
                x,
                y,
                icon.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
    }
    sheet
        .save_png(&args.out)
        .map_err(|e| format!("{}: {e}", args.out.display()))?;
    println!(
        "{} icons x {} modes -> {}",
        ids.len(),
        cols,
        args.out.display()
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fern-icon-sheet: {e}");
            ExitCode::FAILURE
        }
    }
}
