//! Interaction tests: the real view in a headless GPUI window, driven
//! through GPUI's event pipeline (hit-testing, focus, listeners, input
//! handler), asserting on the resulting scene JSON and save callbacks.

#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use fuchico_excalidraw::{ExcalidrawEvent, ExcalidrawStyle, ExcalidrawView, OnChange, StyleFn};
use gpui::{
    Entity, InputEvent, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, TestAppContext, VisualTestContext, point, px,
};
use serde_json::Value;

struct H {
    view: Entity<ExcalidrawView>,
    cx: VisualTestContext,
    saves: Rc<RefCell<Vec<String>>>,
}

fn open(cx: &mut TestAppContext, json: &str) -> H {
    let saves = Rc::new(RefCell::new(Vec::new()));
    let s2 = saves.clone();
    let on_change: OnChange = Rc::new(move |ev, _| {
        if let ExcalidrawEvent::Save(j) = ev {
            s2.borrow_mut().push(j);
        }
        Ok(())
    });
    let style: StyleFn = Rc::new(|_| ExcalidrawStyle::light());
    let json = json.to_string();
    let (view, vcx) =
        cx.add_window_view(|window, cx| ExcalidrawView::new(json, style, on_change, window, cx));
    let mut h = H {
        view,
        cx: vcx.clone(),
        saves,
    };
    // Never touch the user's real personal library.
    let lib = temp_path("library.excalidrawlib");
    h.view.update(&mut h.cx, |v, _| v.set_library_path(lib));
    let v = h.view.clone();
    h.cx.update(|w, cx| {
        let f = v.read(cx).focus_handle().clone();
        w.focus(&f, cx);
    });
    h.frame();
    h
}

fn temp_path(name: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fuchico-excalidraw-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

const EMPTY: &str = r##"{"type":"excalidraw","version":2,"source":"test","elements":[],"appState":{"viewBackgroundColor":"#ffffff"},"files":{}}"##;

fn m(s: &str) -> Modifiers {
    let mut m = Modifiers::default();
    for p in s.split('-') {
        match p {
            "cmd" => m.platform = true,
            "shift" => m.shift = true,
            "alt" => m.alt = true,
            "ctrl" => m.control = true,
            _ => {}
        }
    }
    m
}

fn p(x: f32, y: f32) -> Point<Pixels> {
    point(px(x), px(y))
}

impl H {
    fn frame(&mut self) {
        self.cx.update(|w, cx| {
            w.refresh();
            w.draw(cx).clear(cx);
        });
    }
    fn ev(&mut self, e: impl InputEvent) {
        self.cx.simulate_event(e);
        self.frame();
    }
    fn mv(&mut self, at: Point<Pixels>, pressed: Option<MouseButton>, mods: Modifiers) {
        self.ev(MouseMoveEvent {
            position: at,
            pressed_button: pressed,
            modifiers: mods,
        });
    }
    fn down(&mut self, at: Point<Pixels>, button: MouseButton, mods: Modifiers, clicks: usize) {
        self.ev(MouseDownEvent {
            button,
            position: at,
            modifiers: mods,
            click_count: clicks,
            first_mouse: false,
        });
    }
    fn up(&mut self, at: Point<Pixels>, button: MouseButton, mods: Modifiers, clicks: usize) {
        self.ev(MouseUpEvent {
            button,
            position: at,
            modifiers: mods,
            click_count: clicks,
        });
    }
    fn click(&mut self, at: Point<Pixels>) {
        self.click_m(at, Modifiers::default());
    }
    fn click_m(&mut self, at: Point<Pixels>, mods: Modifiers) {
        self.mv(at, None, mods);
        self.down(at, MouseButton::Left, mods, 1);
        self.up(at, MouseButton::Left, mods, 1);
    }
    fn dbl(&mut self, at: Point<Pixels>) {
        self.mv(at, None, Modifiers::default());
        for k in 1..=2 {
            self.down(at, MouseButton::Left, Modifiers::default(), k);
            self.up(at, MouseButton::Left, Modifiers::default(), k);
        }
    }
    fn rclick(&mut self, at: Point<Pixels>) {
        self.mv(at, None, Modifiers::default());
        self.down(at, MouseButton::Right, Modifiers::default(), 1);
        self.up(at, MouseButton::Right, Modifiers::default(), 1);
    }
    fn drag(&mut self, a: Point<Pixels>, b: Point<Pixels>) {
        self.drag_m(a, b, Modifiers::default());
    }
    fn drag_m(&mut self, a: Point<Pixels>, b: Point<Pixels>, mods: Modifiers) {
        self.mv(a, None, mods);
        self.down(a, MouseButton::Left, mods, 1);
        for i in 1..=4 {
            let t = i as f32 / 4.0;
            self.mv(a + (b - a) * t, Some(MouseButton::Left), mods);
        }
        self.up(b, MouseButton::Left, mods, 1);
    }
    fn press(&mut self, k: &str) {
        self.cx.simulate_keystrokes(k);
        self.frame();
    }
    fn input(&mut self, t: &str) {
        self.cx.simulate_input(t);
        self.frame();
    }
    fn bounds(&mut self, sel: &str) -> Option<gpui::Bounds<Pixels>> {
        let sel: &'static str = Box::leak(sel.to_string().into_boxed_str());
        self.cx.debug_bounds(sel)
    }
    /// Clicks a chrome element tagged with `debug_selector`.
    fn button(&mut self, sel: &str) {
        let b = self
            .bounds(sel)
            .unwrap_or_else(|| panic!("no element {sel}"));
        self.click(b.center());
    }
    fn exists(&mut self, sel: &str) -> bool {
        self.bounds(sel).is_some()
    }
    fn json(&mut self) -> Value {
        let s = self.cx.update(|_, cx| self.view.read(cx).to_json());
        serde_json::from_str(&s).unwrap()
    }
    fn elements(&mut self) -> Vec<Value> {
        self.json()["elements"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| !e["isDeleted"].as_bool().unwrap_or(false))
            .cloned()
            .collect()
    }
    fn of(&mut self, kind: &str) -> Vec<Value> {
        self.elements()
            .into_iter()
            .filter(|e| e["type"] == kind)
            .collect()
    }
    fn selection(&mut self) -> Vec<String> {
        self.cx.update(|_, cx| self.view.read(cx).selection())
    }
    /// Advances the test clock past the save debounce.
    fn settle(&mut self) {
        self.cx
            .executor()
            .advance_clock(Duration::from_millis(1100));
        self.cx.run_until_parked();
        self.frame();
    }
    fn last_save(&mut self) -> Value {
        self.settle();
        let s = self.saves.borrow().last().cloned().expect("a save");
        serde_json::from_str(&s).unwrap()
    }
}

// --------------------------------------------------------------- tools

#[gpui::test]
fn draws_every_shape_tool_and_saves(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    let tools = [
        ("r", "rectangle"),
        ("d", "diamond"),
        ("o", "ellipse"),
        ("f", "frame"),
    ];
    for (i, (key, kind)) in tools.iter().enumerate() {
        h.press(key);
        let y = 120.0 + i as f32 * 120.0;
        h.drag(p(300.0, y), p(400.0, y + 80.0));
        let els = h.of(kind);
        assert_eq!(els.len(), 1, "{kind} created");
        let e = &els[0];
        assert!(
            (e["width"].as_f64().unwrap() - 100.0).abs() < 1.0,
            "{kind} width {e}"
        );
        assert!((e["height"].as_f64().unwrap() - 80.0).abs() < 1.0);
    }
    let saved = h.last_save();
    assert_eq!(saved["elements"].as_array().unwrap().len(), 4);
    assert_eq!(h.saves.borrow().len(), 1, "debounced into one save");
}

fn f(v: &Value, k: &str) -> f64 {
    v[k].as_f64().unwrap_or_else(|| panic!("{k} in {v}"))
}

impl H {
    fn rect(&mut self, a: Point<Pixels>, b: Point<Pixels>) -> Value {
        self.press("r");
        self.drag(a, b);
        self.press("escape");
        self.of("rectangle").last().cloned().unwrap()
    }
    fn el(&mut self, id: &str) -> Value {
        self.elements().into_iter().find(|e| e["id"] == id).unwrap()
    }
}

// --------------------------------------------------------- context menu

#[gpui::test]
fn context_menu_actions(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    let r = h.rect(p(300.0, 300.0), p(400.0, 380.0));
    let id = r["id"].as_str().unwrap().to_string();
    // Right-click selects the element and opens the element menu.
    h.rclick(p(300.0, 340.0));
    assert!(h.exists("context-menu"));
    assert_eq!(h.selection(), vec![id.clone()]);
    assert!(h.exists("cm-flip-h") && h.exists("cm-bring-front") && h.exists("cm-copy-styles"));
    h.button("cm-duplicate");
    assert!(
        !h.exists("context-menu"),
        "running an action closes the menu"
    );
    assert_eq!(h.of("rectangle").len(), 2);
    // Lock via the menu, then canvas menu offers "unlock all".
    h.rclick(p(300.0, 340.0));
    h.button("cm-lock");
    assert!(h.elements().iter().any(|e| e["locked"] == true));
    h.rclick(p(700.0, 700.0));
    assert!(h.selection().is_empty());
    assert!(h.exists("cm-unlock-all") && h.exists("cm-paste") && !h.exists("cm-delete"));
    h.button("cm-unlock-all");
    assert!(h.elements().iter().all(|e| e["locked"] != true));
    // Escape closes; delete removes.
    h.rclick(p(700.0, 700.0));
    h.press("escape");
    assert!(!h.exists("context-menu"));
    h.rclick(p(300.0, 340.0));
    h.button("cm-delete");
    assert_eq!(h.of("rectangle").len(), 1);
}

#[gpui::test]
fn flip_mirrors_arrow_points(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    h.press("a");
    h.drag(p(300.0, 300.0), p(400.0, 350.0));
    let before = h.of("arrow")[0].clone();
    h.press("shift-h");
    let a = h.of("arrow")[0].clone();
    let (p0, p1) = (&a["points"][0], &a["points"][1]);
    assert!(
        (p0[0].as_f64().unwrap() - 0.0).abs() < 1e-6,
        "renormalised to first point {a}"
    );
    assert!(
        (p1[0].as_f64().unwrap() + 100.0).abs() < 1.0,
        "x mirrored {a}"
    );
    assert!((f(&a, "x") - f(&before, "x") - 100.0).abs() < 1.0);
    h.press("shift-v");
    let a = h.of("arrow")[0].clone();
    assert!(
        (a["points"][1][1].as_f64().unwrap() + 50.0).abs() < 1.0,
        "y mirrored {a}"
    );
}

// ---------------------------------------------------------- colour picker

#[gpui::test]
fn colour_picker_palette_shades_hex(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    h.rect(p(300.0, 300.0), p(400.0, 380.0));
    h.click(p(300.0, 340.0));
    h.button("s-picker");
    assert!(h.exists("picker") && h.exists("pk-violet") && h.exists("pk-hex"));
    h.button("pk-red");
    assert_eq!(h.of("rectangle")[0]["strokeColor"], "#e03131");
    // 1-5 pick a shade of the current hue; q..b pick hues.
    h.press("2");
    assert_eq!(h.of("rectangle")[0]["strokeColor"], "#ffc9c9");
    h.button("pk-shade-4");
    assert_eq!(h.of("rectangle")[0]["strokeColor"], "#fa5252");
    h.press("f"); // grape (stock order: q w e r t / a s d f g / z x c v b)
    assert_eq!(h.of("rectangle")[0]["strokeColor"], "#9c36b5");
    h.press("d"); // violet
    assert_eq!(h.of("rectangle")[0]["strokeColor"], "#6741d9");
    assert_eq!(
        h.cx.update(|_, cx| h.view.read(cx).tool()),
        fuchico_excalidraw::Tool::Selection,
        "picker keys don't switch tools"
    );
    // Hex field.
    h.button("pk-hex");
    h.press("1 2 a b 5 6 enter");
    assert_eq!(h.of("rectangle")[0]["strokeColor"], "#12ab56");
    h.press("escape");
    assert!(!h.exists("picker"));
}

#[gpui::test]
fn eyedropper_picks_canvas_colour(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    h.rect(p(300.0, 300.0), p(400.0, 380.0));
    h.click(p(300.0, 340.0));
    h.button("b-picker");
    h.button("pk-blue");
    h.button("fill-solid");
    assert_eq!(h.of("rectangle")[0]["backgroundColor"], "#a5d8ff");
    h.press("escape");
    h.button("menu");
    h.button("cbg-picker");
    h.button("pk-eyedropper");
    h.click(p(350.0, 340.0));
    assert_eq!(h.json()["appState"]["viewBackgroundColor"], "#a5d8ff");
}

// ------------------------------------------------------------ properties

#[gpui::test]
fn arrow_type_elbow_routes_orthogonally(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    h.press("a");
    h.drag(p(300.0, 300.0), p(500.0, 420.0));
    h.button("at-elbow");
    let a = h.of("arrow")[0].clone();
    assert_eq!(a["elbowed"], true);
    assert!(a["roundness"].is_null());
    let pts = a["points"].as_array().unwrap();
    assert!(pts.len() >= 3, "routed {a}");
    for w in pts.windows(2) {
        let (dx, dy) = (
            w[1][0].as_f64().unwrap() - w[0][0].as_f64().unwrap(),
            w[1][1].as_f64().unwrap() - w[0][1].as_f64().unwrap(),
        );
        assert!(dx.abs() < 1e-6 || dy.abs() < 1e-6, "axis-aligned {a}");
    }
    h.button("at-round");
    let a = h.of("arrow")[0].clone();
    assert_eq!(a["elbowed"], false);
    assert_eq!(a["roundness"]["type"], 2);
    // New arrows use the chosen type; every Excalidraw arrowhead is offered
    // in the end-arrowhead popover.
    h.button("ah-end");
    assert!(h.exists("ah-popover"));
    for k in [
        "None", "Arrow", "Bar", "●", "○", "▲", "△", "◆", "◇", "Cf1", "CfN", "Cf1N",
    ] {
        assert!(h.exists(&format!("ah-false-{k}")), "{k}");
    }
    h.button("ah-false-◇");
    assert!(!h.exists("ah-popover"));
    assert_eq!(h.of("arrow")[0]["endArrowhead"], "diamond_outline");
}

#[gpui::test]
fn elbow_arrow_binds_with_fixed_points(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    let a = h.rect(p(400.0, 200.0), p(500.0, 280.0));
    let b = h.rect(p(700.0, 400.0), p(800.0, 480.0));
    h.press("a");
    h.button("at-elbow");
    h.drag(p(450.0, 240.0), p(750.0, 440.0));
    let arrow = h.of("arrow")[0].clone();
    assert_eq!(arrow["elbowed"], true);
    assert_eq!(arrow["startBinding"]["elementId"], a["id"]);
    assert_eq!(arrow["endBinding"]["elementId"], b["id"]);
    assert!(arrow["startBinding"]["fixedPoint"].is_array(), "{arrow}");
    // Moving a bound shape re-routes the arrow.
    h.press("escape");
    h.drag(p(780.0, 400.0), p(780.0, 560.0));
    let moved = h.of("arrow")[0].clone();
    assert_ne!(moved["points"], arrow["points"]);
}

#[gpui::test]
fn align_and_distribute(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    h.rect(p(400.0, 200.0), p(460.0, 240.0));
    h.rect(p(530.0, 300.0), p(600.0, 340.0));
    h.rect(p(720.0, 420.0), p(760.0, 460.0));
    h.press("cmd-a");
    h.button("al-left");
    let xs: Vec<f64> = h.of("rectangle").iter().map(|e| f(e, "x")).collect();
    assert!(xs.iter().all(|x| (x - xs[0]).abs() < 1e-6), "{xs:?}");
    h.button("al-top");
    let ys: Vec<f64> = h.of("rectangle").iter().map(|e| f(e, "y")).collect();
    assert!(ys.iter().all(|y| (y - ys[0]).abs() < 1e-6), "{ys:?}");
    h.button("dist-h");
}

#[gpui::test]
fn opacity_slider_drags(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    h.rect(p(300.0, 300.0), p(400.0, 380.0));
    h.click(p(300.0, 340.0));
    let b = h.bounds("op-slider").unwrap();
    let y = b.center().y;
    h.down(
        point(b.left() + b.size.width * 0.25, y),
        MouseButton::Left,
        Modifiers::default(),
        1,
    );
    h.mv(
        point(b.left() + b.size.width * 0.6, y),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    h.up(
        point(b.left() + b.size.width * 0.6, y),
        MouseButton::Left,
        Modifiers::default(),
        1,
    );
    assert_eq!(f(&h.of("rectangle")[0], "opacity"), 60.0);
}

#[gpui::test]
fn font_families_and_vertical_align(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    h.rect(p(300.0, 300.0), p(400.0, 380.0));
    h.click(p(300.0, 340.0));
    h.press("enter");
    h.input("Hi");
    h.press("escape");
    h.click(p(300.0, 340.0));
    assert!(h.exists("ff-7") && h.exists("ff-9"));
    h.button("ff-7");
    h.button("va-top");
    let t = h.of("text")[0].clone();
    assert_eq!(f(&t, "fontFamily"), 7.0);
    assert_eq!(t["verticalAlign"], "top");
}

// ------------------------------------------------------------------ text

#[gpui::test]
fn ime_composition_through_input_handler(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;
    let mut h = open(cx, EMPTY);
    h.dbl(p(300.0, 300.0));
    assert!(h.cx.update(|_, cx| h.view.read(cx).editing_text().is_some()));
    h.input("Hé");
    // Compose "か" the way macOS IME does: marked text, then commit.
    h.view.update_in(&mut h.cx, |v, w, cx| {
        v.replace_and_mark_text_in_range(None, "k", None, w, cx);
        assert_eq!(v.marked_text_range(w, cx), Some(2..3));
        v.replace_and_mark_text_in_range(None, "か", None, w, cx);
        v.replace_text_in_range(None, "か", w, cx);
        assert_eq!(v.marked_text_range(w, cx), None);
        let mut adj = None;
        assert_eq!(
            v.text_for_range(0..3, &mut adj, w, cx).as_deref(),
            Some("Héか")
        );
    });
    h.frame();
    h.press("escape");
    assert_eq!(h.of("text")[0]["text"], "Héか");
}

#[gpui::test]
fn arrow_labels(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    h.press("a");
    h.drag(p(300.0, 300.0), p(500.0, 300.0));
    let a = h.of("arrow")[0].clone();
    h.dbl(p(400.0, 300.0));
    h.input("label");
    h.press("escape");
    let t = h.of("text")[0].clone();
    assert_eq!(t["containerId"], a["id"]);
    assert_eq!(t["text"], "label");
    let cx_ = f(&t, "x") + f(&t, "width") / 2.0;
    assert!((cx_ - 400.0).abs() < 1.0, "centred on the midpoint {t}");
    let a = h.of("arrow")[0].clone();
    assert!(
        a["boundElements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["id"] == t["id"])
    );
}

#[gpui::test]
fn text_side_handle_fixes_width_and_wraps(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    h.dbl(p(300.0, 300.0));
    h.input("hello wide world");
    h.press("escape");
    let t = h.of("text")[0].clone();
    let (x, y, w, hh) = (f(&t, "x"), f(&t, "y"), f(&t, "width"), f(&t, "height"));
    h.click(p((x + w / 2.0) as f32, (y + hh / 2.0) as f32));
    let e = p((x + w + 4.0) as f32, (y + hh / 2.0) as f32);
    h.drag(e, p((x + w * 0.45) as f32, (y + hh / 2.0) as f32));
    let t = h.of("text")[0].clone();
    assert_eq!(t["autoResize"], false);
    assert!(f(&t, "width") < w * 0.6, "{t}");
    assert!(f(&t, "height") > hh * 1.5, "wrapped {t}");
    assert_eq!(t["originalText"], "hello wide world");
}

// ------------------------------------------------------------ grid / snap

#[gpui::test]
fn grid_toggle_and_snapping(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    h.press("cmd-'");
    assert_eq!(h.json()["appState"]["gridModeEnabled"], true);
    let r = h.rect(p(303.0, 207.0), p(398.0, 289.0));
    for k in ["x", "y", "width", "height"] {
        assert_eq!(f(&r, k) % 20.0, 0.0, "{k} snapped {r}");
    }
    // Cmd disables snapping while held.
    h.drag_m(p(330.0, 200.0), p(337.0, 203.0), m("cmd"));
    let r2 = h.of("rectangle")[0].clone();
    assert!((f(&r2, "x") - f(&r, "x") - 7.0).abs() < 1e-6, "{r} -> {r2}");
    h.press("cmd-'");
    assert_eq!(h.json()["appState"]["gridModeEnabled"], false);
}

#[gpui::test]
fn object_snapping(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    h.rect(p(400.0, 200.0), p(500.0, 260.0));
    let b = h.rect(p(605.0, 400.0), p(705.0, 460.0));
    h.press("alt-s");
    assert_eq!(h.json()["appState"]["objectsSnapModeEnabled"], true);
    // Drag b so its left edge is 3px off a's left edge: it snaps to 200.
    // Grab the top stroke away from the resize handles.
    h.drag(p(630.0, 400.0), p(428.0, 330.0));
    let b2 = h.el(b["id"].as_str().unwrap());
    assert!((f(&b2, "x") - 400.0).abs() < 1e-6, "{b2}");
}

// ---------------------------------------------------------------- export

#[gpui::test]
fn export_dialog_writes_svg_with_scene(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    h.rect(p(300.0, 300.0), p(400.0, 380.0));
    h.press("cmd-shift-e");
    assert!(h.exists("dialog-export-image"));
    assert!(h.exists("ex-preview"));
    h.button("ex-embed");
    h.button("ex-3");
    h.button("ex-svg");
    let out = temp_path("drawing.svg");
    let o2 = out.clone();
    h.cx.simulate_new_path_selection(move |_| Some(o2));
    h.cx.run_until_parked();
    h.frame();
    let svg = std::fs::read_to_string(&out).expect("exported");
    assert!(
        svg.starts_with("<svg") && svg.contains("payload-type:application/vnd.excalidraw+json"),
        "{}",
        &svg[..200.min(svg.len())]
    );
    assert!(!h.exists("dialog-export-image"), "closes after export");
    // PNG export too.
    h.press("cmd-shift-e");
    h.button("ex-png");
    let out = temp_path("drawing.png");
    let o2 = out.clone();
    h.cx.simulate_new_path_selection(move |_| Some(o2));
    h.cx.run_until_parked();
    let png = std::fs::read(&out).unwrap();
    assert_eq!(&png[1..4], b"PNG");
}

#[gpui::test]
fn help_dialog(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    h.press("?");
    assert!(h.exists("dialog-help"));
    h.press("escape");
    assert!(!h.exists("dialog-help"));
    h.button("help");
    assert!(h.exists("dialog-help"));
    h.button("dlg-close");
    assert!(!h.exists("dialog-help"));
}

// --------------------------------------------------------------- library

#[gpui::test]
fn library_add_insert_drag_export(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    h.rect(p(300.0, 300.0), p(400.0, 380.0));
    h.click(p(300.0, 340.0));
    h.button("lib-toggle");
    assert!(h.exists("library"));
    h.button("lib-add");
    assert!(h.exists("lib-item-0"));
    // Persisted in .excalidrawlib v2 format at the (temp) library path.
    let lib = h.cx.update(|_, cx| h.view.read(cx).library_path().unwrap());
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&lib).unwrap()).unwrap();
    assert_eq!(saved["type"], "excalidrawlib");
    assert_eq!(saved["libraryItems"].as_array().unwrap().len(), 1);
    // Click inserts at the viewport centre.
    h.button("lib-item-0");
    assert_eq!(h.of("rectangle").len(), 2);
    // Drag onto the canvas inserts at the drop point.
    let from = h.bounds("lib-item-0").unwrap().center();
    h.down(from, MouseButton::Left, Modifiers::default(), 1);
    h.mv(
        point(from.x - px(20.0), from.y + px(10.0)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    h.mv(
        p(500.0, 600.0),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    h.up(p(500.0, 600.0), MouseButton::Left, Modifiers::default(), 1);
    let rects = h.of("rectangle");
    assert_eq!(rects.len(), 3);
    let dropped = rects
        .iter()
        .find(|e| (f(e, "x") + f(e, "width") / 2.0 - 500.0).abs() < 1.0);
    assert!(dropped.is_some(), "{rects:?}");
    // Export via the ⋮ menu's "Save to..." and the save dialog.
    h.button("lib-more");
    assert!(h.exists("lib-menu"));
    h.button("lib-export");
    assert!(!h.exists("lib-menu"));
    let out = temp_path("mine.excalidrawlib");
    let o2 = out.clone();
    h.cx.simulate_new_path_selection(move |_| Some(o2));
    h.cx.run_until_parked();
    let exported: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(exported["libraryItems"].as_array().unwrap().len(), 1);
    // Remove (the × shows on hover).
    let cell = h.bounds("lib-item-0").unwrap().center();
    h.mv(cell, None, Modifiers::default());
    h.button("lib-remove-0");
    assert!(!h.exists("lib-item-0"));
    // Reset asks first; Cancel keeps the items.
    h.button("lib-add");
    h.button("lib-more");
    h.button("lib-reset");
    assert!(h.exists("dialog-reset-library"));
    h.button("dlg-cancel");
    assert!(h.exists("lib-item-0"));
    h.button("lib-more");
    h.button("lib-reset");
    h.button("dlg-confirm");
    assert!(!h.exists("lib-item-0"));
    // The sidebar closes with ×, bringing the Library button back.
    assert!(!h.exists("lib-toggle"));
    h.button("lib-close");
    assert!(!h.exists("library"));
    assert!(h.exists("lib-toggle"));
}

// ------------------------------------------------------- stock chrome

const COMPARE: &str = include_str!("fixtures/compare.excalidraw");

#[gpui::test]
fn find_on_canvas_searches_text(cx: &mut TestAppContext) {
    let mut h = open(cx, COMPARE);
    h.press("cmd-f");
    assert!(h.exists("library"));
    assert!(h.exists("search-input"));
    h.press("f u c h");
    assert!(h.exists("search-hit-0"));
    assert!(!h.exists("search-hit-1"));
    assert_eq!(h.selection(), vec!["el5".to_string()]);
    h.press("backspace backspace backspace backspace z z");
    assert!(!h.exists("search-hit-0"));
    assert!(h.exists("search-count"));
    // Escape leaves the field; keys go back to the canvas.
    h.press("escape");
    h.press("r");
    let tool = h.cx.update(|_, cx| h.view.read(cx).tool());
    assert_eq!(tool, fuchico_excalidraw::Tool::Rectangle);
    // The Library tab is one click away.
    h.button("sb-library");
    assert!(h.exists("lib-browse"));
    assert!(!h.exists("search-input"));
}

#[gpui::test]
fn zen_and_view_modes(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    assert!(h.exists("t-rect"));
    h.press("alt-z");
    assert!(h.cx.update(|_, cx| h.view.read(cx).zen_mode()));
    assert!(!h.exists("t-rect"));
    h.button("exit-zen");
    assert!(h.exists("t-rect"));
    // View mode: read-only, no toolbar, drags pan instead of drawing.
    h.press("alt-r");
    assert!(h.cx.update(|_, cx| h.view.read(cx).view_mode()));
    assert!(!h.exists("t-rect"));
    h.press("r");
    h.drag(p(300.0, 300.0), p(400.0, 400.0));
    assert!(h.elements().is_empty());
    // The canvas context menu offers the mode toggles.
    h.rclick(p(500.0, 500.0));
    assert!(h.exists("cm-view-mode"));
    h.button("cm-view-mode");
    assert!(!h.cx.update(|_, cx| h.view.read(cx).view_mode()));
    assert!(h.exists("t-rect"));
}

#[gpui::test]
fn laser_pointer_draws_nothing(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    h.press("k");
    let tool = h.cx.update(|_, cx| h.view.read(cx).tool());
    assert_eq!(tool, fuchico_excalidraw::Tool::Laser);
    h.drag(p(300.0, 300.0), p(400.0, 400.0));
    assert!(h.elements().is_empty());
}

#[gpui::test]
fn extra_tools_menu_and_mermaid_stub(cx: &mut TestAppContext) {
    let mut h = open(cx, EMPTY);
    h.button("t-extra");
    assert!(h.exists("extra-tools"));
    h.button("x-laser");
    assert!(!h.exists("extra-tools"));
    let tool = h.cx.update(|_, cx| h.view.read(cx).tool());
    assert_eq!(tool, fuchico_excalidraw::Tool::Laser);
    h.button("t-extra");
    h.button("x-mermaid");
    assert!(h.exists("dialog-mermaid-to-excalidraw"));
    // Like the original app: the converter is stubbed out.
    assert!(h.exists("mermaid-error"));
    h.press("escape");
    assert!(!h.exists("dialog-mermaid-to-excalidraw"));
}

#[gpui::test]
fn stats_panel_toggles(cx: &mut TestAppContext) {
    let mut h = open(cx, COMPARE);
    h.press("alt-/");
    assert!(h.exists("stats"));
    h.button("stats-close");
    assert!(!h.exists("stats"));
}

#[gpui::test]
fn main_menu_save_and_open_scene(cx: &mut TestAppContext) {
    let mut h = open(cx, COMPARE);
    let out = temp_path("scene.excalidraw");
    let o2 = out.clone();
    h.button("menu");
    h.button("m-save");
    h.cx.simulate_new_path_selection(move |_| Some(o2));
    h.cx.run_until_parked();
    let saved = std::fs::read_to_string(&out).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&saved).unwrap(),
        serde_json::from_str::<Value>(&h.cx.update(|_, cx| h.view.read(cx).to_json())).unwrap()
    );
    // Clear, then open the saved file back.
    h.press("cmd-a");
    h.press("delete");
    assert!(h.elements().is_empty());
    h.button("menu");
    h.button("m-open");
    let o3 = out.clone();
    h.cx.simulate_path_prompt_response(move |_| Some(vec![o3]));
    h.cx.run_until_parked();
    h.frame();
    assert_eq!(h.elements().len(), 5);
}

#[gpui::test]
fn align_font_size_and_layer_shortcuts(cx: &mut TestAppContext) {
    let mut h = open(cx, COMPARE);
    h.view
        .update(&mut h.cx, |v, cx| v.select_ids(&["el1", "el2"], cx));
    h.press("cmd-shift-up");
    let (a, b) = (
        h.json()["elements"][0].clone(),
        h.json()["elements"][1].clone(),
    );
    assert_eq!(f(&a, "y"), f(&b, "y"));
    h.view.update(&mut h.cx, |v, cx| v.select_ids(&["el5"], cx));
    let before = f(&h.of("text")[0], "fontSize");
    h.press("cmd-shift-.");
    assert!(f(&h.of("text")[0], "fontSize") > before);
    // ⌘⌥[ sends to back.
    h.press("cmd-alt-[");
    assert_eq!(h.json()["elements"][0]["id"], "el5");
}
