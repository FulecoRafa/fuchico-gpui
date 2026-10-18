//! Headless test harness: a gpui test window (never shown, no OS input)
//! with an [`EditorView`] focused inside it.
#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use fuchico_editor::{EditorEvent, EditorOptions, EditorStyle, EditorView};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, Focusable, Modifiers, MouseButton, Pixels, Point,
    TestAppContext, VisualTestContext, WindowBounds, WindowOptions, point, px, size,
};

pub fn register_fonts(cx: &mut App) {
    let fonts: Vec<std::borrow::Cow<'static, [u8]>> = vec![
        include_bytes!("../../../../apps/fuchico/assets/fonts/JetBrainsMono-Regular.ttf").as_slice().into(),
        include_bytes!("../../../../apps/fuchico/assets/fonts/JetBrainsMono-Bold.ttf").as_slice().into(),
        include_bytes!("../../../../apps/fuchico/assets/fonts/JetBrainsMono-Italic.ttf").as_slice().into(),
        include_bytes!("../../../../apps/fuchico/assets/fonts/JetBrainsMono-BoldItalic.ttf").as_slice().into(),
        include_bytes!("../../../../apps/fuchico/assets/fonts/Inter-Regular.ttf").as_slice().into(),
        include_bytes!("../../../../apps/fuchico/assets/fonts/Inter-Medium.ttf").as_slice().into(),
        include_bytes!("../../../../apps/fuchico/assets/fonts/Inter-SemiBold.ttf").as_slice().into(),
        include_bytes!("../../../../apps/fuchico/assets/fonts/Inter-Bold.ttf").as_slice().into(),
        include_bytes!("../../../../apps/fuchico/assets/fonts/Inter-Italic.ttf").as_slice().into(),
    ];
    let _ = cx.text_system().add_fonts(fonts);
}

pub struct Ui {
    pub cx: VisualTestContext,
    pub view: Entity<EditorView>,
    pub events: Rc<RefCell<Vec<EditorEvent>>>,
    pub handle: AnyWindowHandle,
}

pub fn open(cx: &mut TestAppContext, text: &str, options: EditorOptions) -> Ui {
    open_sized(cx, text, options, size(px(1000.), px(700.)))
}

pub fn open_sized(cx: &mut TestAppContext, text: &str, options: EditorOptions, sz: gpui_kit::Size<Pixels>) -> Ui {
    cx.update(|cx| {
        gpui_kit::init(cx);
        register_fonts(cx);
        fuchico_editor::init(cx);
    });
    let events: Rc<RefCell<Vec<EditorEvent>>> = Rc::default();
    let rec = events.clone();
    let text = text.to_string();
    let (handle, view): (AnyWindowHandle, Entity<EditorView>) = cx.update(|cx| {
        let bounds = Bounds { origin: Point::default(), size: sz };
        gpui_kit::open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
            cx,
            move |window, cx| {
                let v = cx.new(|cx| EditorView::new(&text, options, EditorStyle::ayu_dark(), window, cx));
                cx.subscribe(&v, move |_, ev: &EditorEvent, _| rec.borrow_mut().push(ev.clone())).detach();
                let fh = v.focus_handle(cx);
                window.focus(&fh, cx);
                v
            },
        )
        .expect("open test window")
    });
    let mut ui = Ui { cx: VisualTestContext::from_window(handle, cx), view, events, handle };
    ui.frame();
    ui.frame();
    ui
}

impl Ui {
    pub fn frame(&mut self) {
        self.cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }

    pub fn press(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.cx.run_until_parked();
        self.frame();
    }

    pub fn type_text(&mut self, text: &str) {
        self.cx.simulate_input(text);
        self.cx.run_until_parked();
        self.frame();
    }

    pub fn read<R>(&mut self, f: impl FnOnce(&EditorView, &App) -> R) -> R {
        let v = self.view.clone();
        self.cx.update(|_, cx| f(v.read(cx), cx))
    }

    pub fn update<R>(&mut self, f: impl FnOnce(&mut EditorView, &mut gpui_kit::Window, &mut gpui_kit::Context<EditorView>) -> R) -> R {
        let v = self.view.clone();
        let r = self.cx.update(|w, cx| v.update(cx, |v, cx| f(v, w, cx)));
        self.cx.run_until_parked();
        self.frame();
        r
    }

    pub fn text(&mut self) -> String {
        self.read(|v, _| v.text())
    }

    pub fn head(&mut self) -> usize {
        self.read(|v, _| v.selection().main().head)
    }

    pub fn sel(&mut self) -> (usize, usize) {
        self.read(|v, _| {
            let r = v.selection().main();
            (r.anchor, r.head)
        })
    }

    pub fn events(&self) -> Vec<EditorEvent> {
        self.events.borrow().clone()
    }

    pub fn clear_events(&self) {
        self.events.borrow_mut().clear();
    }

    /// Centre of the char at `pos`.
    pub fn char_point(&mut self, pos: usize) -> Point<Pixels> {
        self.frame();
        self.read(|v, _| v.bounds_for_char(pos)).unwrap_or_else(|| panic!("char {pos} not visible")).center()
    }

    pub fn hit(&mut self, prefix: &str) -> Point<Pixels> {
        self.frame();
        let hits = self.read(|v, _| v.debug_hits());
        hits.iter()
            .find(|(n, _)| n.starts_with(prefix))
            .unwrap_or_else(|| panic!("no hit {prefix}: {:?}", hits.iter().map(|h| &h.0).collect::<Vec<_>>()))
            .1
            .center()
    }

    pub fn click(&mut self, at: Point<Pixels>, mods: Modifiers) {
        self.cx.simulate_mouse_move(at, None, Modifiers::none());
        self.frame();
        self.cx.simulate_click(at, mods);
        self.cx.run_until_parked();
        self.frame();
    }

    pub fn drag(&mut self, from: Point<Pixels>, to: Point<Pixels>) {
        self.cx.simulate_mouse_move(from, None, Modifiers::none());
        self.cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
        self.frame();
        let mid = point((from.x + to.x) / 2., (from.y + to.y) / 2.);
        self.cx.simulate_mouse_move(mid, Some(MouseButton::Left), Modifiers::none());
        self.cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
        self.cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
        self.cx.run_until_parked();
        self.frame();
    }
}
