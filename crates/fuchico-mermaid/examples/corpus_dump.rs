//! Renders every `tests/corpus/*.mmd` through the port's pipeline and writes
//! `<out>/<name>.<light|dark>.svg/.png`, plus `<out>/port-results.txt`
//! (`OK|ERR name message`). Usage: `cargo run -p fuchico-mermaid --example
//! corpus_dump -- <out dir> [scale]`.

use fuchico_mermaid::{MermaidTheme, rasterize, render_svg};
use std::fmt::Write as _;
use std::path::PathBuf;

/// The fonts the app bundles (and registers at startup), so the dump lays
/// out and paints with the same faces as the app.
const APP_FONTS: &[&[u8]] = &[
    include_bytes!("../../../apps/fuchico/assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../../apps/fuchico/assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../../apps/fuchico/assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../../apps/fuchico/assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../../../apps/fuchico/assets/fonts/Inter-Italic.ttf"),
    include_bytes!("../../../apps/fuchico/assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../../apps/fuchico/assets/fonts/JetBrainsMono-Bold.ttf"),
    include_bytes!("../../../apps/fuchico/assets/fonts/JetBrainsMono-Italic.ttf"),
];

fn main() {
    fuchico_mermaid::add_fonts(APP_FONTS);
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().expect("out dir"));
    let scale: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(2.0);
    std::fs::create_dir_all(&out).unwrap();
    let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut files: Vec<_> = std::fs::read_dir(&corpus)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "mmd"))
        .collect();
    files.sort();
    let mut report = String::new();
    for f in files {
        let name = f.file_stem().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(&f).unwrap();
        for (mode, theme) in [
            ("light", MermaidTheme::light()),
            ("dark", MermaidTheme::dark()),
        ] {
            let t0 = std::time::Instant::now();
            match render_svg(&src, &theme) {
                Ok(svg) => {
                    let ms = t0.elapsed().as_secs_f32() * 1000.0;
                    std::fs::write(out.join(format!("{name}.{mode}.svg")), &svg).unwrap();
                    match rasterize(&svg, scale) {
                        Ok(r) => {
                            let img = &r.image.as_bytes(0).unwrap();
                            let s = r.image.size(0);
                            let mut rgba = img.to_vec();
                            // Premultiplied BGRA (what GPUI takes) -> straight RGBA.
                            for px in rgba.as_chunks_mut::<4>().0 {
                                px.swap(0, 2);
                                let a = px[3] as u32;
                                if a > 0 && a < 255 {
                                    for c in &mut px[..3] {
                                        *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                                    }
                                }
                            }
                            image::RgbaImage::from_raw(s.width.0 as u32, s.height.0 as u32, rgba)
                                .unwrap()
                                .save(out.join(format!("{name}.{mode}.png")))
                                .unwrap();
                            if mode == "light" {
                                let _ = writeln!(
                                    report,
                                    "OK  {name} {:.0}x{:.0} {ms:.1}ms",
                                    r.width, r.height
                                );
                            }
                        }
                        Err(e) => {
                            if mode == "light" {
                                let _ = writeln!(report, "RASTER-ERR {name} {e}");
                            }
                        }
                    }
                }
                Err(e) => {
                    if mode == "light" {
                        let _ = writeln!(report, "ERR {name} {:?}", e.message);
                    }
                }
            }
        }
    }
    print!("{report}");
    std::fs::write(out.join("port-results.txt"), report).unwrap();
}
