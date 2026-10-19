use fuchico_mermaid::raster::{self, rasterize, rasterize_cached};
use fuchico_mermaid::samples::SAMPLES;
use fuchico_mermaid::{MermaidTheme, render_svg};

#[test]
fn every_sample_renders_light_and_dark() {
    for theme in [MermaidTheme::light(), MermaidTheme::dark()] {
        for (name, src) in SAMPLES {
            let svg = render_svg(src, &theme).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(svg.contains("<svg"), "{name}");
        }
    }
}

#[test]
fn every_sample_rasterizes() {
    let theme = MermaidTheme::light();
    for (name, src) in SAMPLES {
        let svg = render_svg(src, &theme).unwrap();
        let r = rasterize(&svg, 2.0).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(r.width > 10.0 && r.height > 10.0, "{name}");
        let px = r.image.size(0);
        assert!(px.width.0 as f32 >= r.width * 2.0 - 1.0, "{name}");
    }
}

#[test]
fn theme_colours_reach_the_svg() {
    let mut t = MermaidTheme::light();
    t.card = "#123456".into();
    let svg = render_svg("flowchart LR; A-->B", &t).unwrap();
    assert!(svg.to_lowercase().contains("#123456"));
}

#[test]
fn errors_carry_line_and_snippet() {
    let t = MermaidTheme::light();
    let e = render_svg("flowchart TD\n  A --> B\n  subgraph X\n  C --> D", &t).unwrap_err();
    // pele's message, verbatim (it reports the end of input here).
    assert!(e.to_string().starts_with("Parse error on line 5:"), "{e}");
    assert!(e.to_string().contains("Expecting 'end'"), "{e}");
    assert_eq!(e.line, Some(5));
    let e = render_svg("flowchart TD\n  A --> B\n  B --> --> C", &t).unwrap_err();
    assert_eq!(e.line, Some(3), "{e}");
    assert!(e.to_string().contains("B --> --> C\n"), "{e}");
    assert!(e.to_string().contains('^'), "{e}");
    let e = render_svg("   \n", &t).unwrap_err();
    assert_eq!(
        e.to_string(),
        "No diagram type detected for the given text."
    );
    assert!(render_svg("notadiagram foo bar", &t).is_err());
}

#[test]
fn raster_cache_hits_by_source_theme_scale() {
    let t = MermaidTheme::light();
    let src = "flowchart LR; CacheA-->CacheB";
    let a = rasterize_cached(src, &t, None, 2.0).unwrap();
    let b = rasterize_cached(src, &t, None, 2.0).unwrap();
    assert!(std::sync::Arc::ptr_eq(&a.image, &b.image));
    let c = rasterize_cached(src, &t, None, 3.0).unwrap();
    assert!(!std::sync::Arc::ptr_eq(&a.image, &c.image));
    let d = rasterize_cached(src, &MermaidTheme::dark(), None, 2.0).unwrap();
    assert!(!std::sync::Arc::ptr_eq(&a.image, &d.image));
    assert!(raster::cache_len() >= 3);
}

#[test]
fn quantized_scale_steps() {
    assert_eq!(raster::quantize_scale(0.3), 1.0);
    assert_eq!(raster::quantize_scale(2.01), 2.5);
    assert_eq!(raster::quantize_scale(4.0), 4.0);
}
