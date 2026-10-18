//! Headless interaction tests for [`AgendaView`] through gpui's test
//! harness (real hit-testing and key dispatch, no OS input or focus).
//!
//! One test drives everything because `bus::send` uses a process-wide
//! sender bound to the first app that calls `bus::init`.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use chrono::Days;
use fuchico_core::events::CoreEvent;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Context, Entity, IntoElement, Modifiers, Pixels, Point,
    Render, Subscription, TestAppContext, VisualTestContext, Window, WindowBounds, WindowOptions,
    div, px, size,
};

use super::model::{MonthCursor, iso, local_today};
use super::{AgendaView, ScanState};
use crate::bus::{self, AppEvent};
use crate::workspace::view::ViewEvent;

/// Records bus events of interest.
struct BusRecorder {
    seen: Vec<AppEvent>,
    _sub: Subscription,
}

impl Render for BusRecorder {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn temp_vault() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "fuchico-agenda-test-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Element lookup by `debug_selector` (= the element id) plus real
/// pointer / key dispatch through the window's hit-testing.
struct Ui {
    cx: VisualTestContext,
}

impl Ui {
    fn frame(&mut self) {
        self.cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }

    fn bounds(&mut self, sel: &str) -> Option<Bounds<Pixels>> {
        self.frame();
        // `debug_bounds` wants `&'static str`; leaking a few test strings is fine.
        self.cx.debug_bounds(sel.to_string().leak())
    }

    fn has(&mut self, sel: &str) -> bool {
        self.bounds(sel).is_some()
    }

    fn click(&mut self, sel: &str) {
        let at = self
            .bounds(sel)
            .unwrap_or_else(|| panic!("no element {sel}"))
            .center();
        self.cx.simulate_mouse_move(at, None, Modifiers::none());
        self.frame();
        self.cx.simulate_click(at, Modifiers::none());
        self.cx.run_until_parked();
        self.frame();
    }

    fn press(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.cx.run_until_parked();
        self.frame();
    }

    /// Polls (10 ms of test time per step) until `pred` holds.
    async fn wait(&mut self, mut pred: impl FnMut(&gpui_kit::App) -> bool) {
        for _ in 0..500 {
            self.frame();
            if self.cx.update(|_, cx| pred(cx)) {
                return;
            }
            self.cx.executor().timer(Duration::from_millis(10)).await;
        }
        panic!("condition not reached");
    }
}

fn loaded(
    view: &Entity<AgendaView>,
    cx: &gpui_kit::App,
) -> Option<Vec<fuchico_core::tasks::AgendaItem>> {
    match &view.read(cx).state {
        ScanState::Loaded(items) => Some(items.clone()),
        _ => None,
    }
}

#[gpui_kit::test]
async fn agenda_sections_toggle_filter_nav_open_and_rescan(cx: &mut TestAppContext) {
    let today = local_today();
    let (yesterday, tomorrow) = (iso(today - Days::new(1)), iso(today + Days::new(1)));
    let today_s = iso(today);
    let vault = temp_vault();
    let note = vault.join("tasks.md");
    std::fs::write(
        &note,
        format!(
            "- [ ] past task 📅 {yesterday}\n\
             - [ ] today task 📅 {today_s}\n\
             - [ ] future task 📅 {tomorrow}\n\
             TODO: plain todo\n\
             📅 {today_s} 10:00 Team sync\n\
             - [ ] water plants 🔁 daily 08:00\n"
        ),
    )
    .unwrap();
    let root = vault.to_string_lossy().to_string();
    let note_s = note.to_string_lossy().to_string();

    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::apply::apply(
            crate::theme::resolve(&crate::settings::Settings::default(), true),
            cx,
        );
        bus::init(cx);
    });
    let recorder = cx.update(|cx| {
        cx.new(|cx| BusRecorder {
            seen: Vec::new(),
            _sub: bus::subscribe(cx, |this: &mut BusRecorder, ev, _| {
                if matches!(ev, AppEvent::GotoLine { .. }) {
                    this.seen.push(ev.clone());
                }
            }),
        })
    });
    let opened: Rc<RefCell<Vec<ViewEvent>>> = Rc::default();
    let (handle, view): (AnyWindowHandle, Entity<AgendaView>) = cx.update(|cx| {
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1200.), px(900.)),
        };
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| AgendaView::new(Some(root.clone()), window, cx)),
        )
        .expect("open test window")
    });
    let sink = opened.clone();
    let _view_sub = cx.update(|cx| {
        cx.subscribe(&view, move |_, ev: &ViewEvent, _| {
            sink.borrow_mut().push(ev.clone())
        })
    });
    let mut ui = Ui {
        cx: VisualTestContext::from_window(handle, cx),
    };

    // ---- scan + sections ----------------------------------------------------
    ui.wait(|cx| loaded(&view, cx).is_some()).await;
    // Overdue: yesterday. Today: today task + event. Upcoming: tomorrow.
    // No date: the TODO (the 🔁 routine is excluded and listed apart).
    assert!(ui.has("agenda-overdue-0"));
    assert!(!ui.has("agenda-overdue-1"));
    assert!(ui.has("agenda-today-1"));
    assert!(!ui.has("agenda-today-2"));
    assert!(ui.has("agenda-upcoming-0"));
    assert!(ui.has("agenda-nodate-0"));
    assert!(!ui.has("agenda-nodate-1"));
    assert!(ui.has("agenda-routine-0"));
    // Only tasks get a checkbox (the event and TODO don't).
    assert!(ui.has("agenda-today-0-cb"));
    assert!(!ui.has("agenda-today-1-cb"));
    assert!(!ui.has("agenda-nodate-0-cb"));
    assert!(ui.has(&format!("agenda-day-{today_s}")));

    // ---- checkbox toggle writes the file; rescan via fs:file-written --------
    ui.click("agenda-today-0-cb");
    ui.wait(|cx| {
        loaded(&view, cx).is_some_and(|items| {
            items
                .iter()
                .any(|i| i.text.contains("today task") && i.checked)
        })
    })
    .await;
    let text = std::fs::read_to_string(&note).unwrap();
    assert!(
        text.contains(&format!("- [x] today task 📅 {today_s}")),
        "{text}"
    );
    // The checkbox click must not also open the row.
    assert!(opened.borrow().is_empty());
    assert!(ui.cx.read(|cx| recorder.read(cx).seen.is_empty()));
    // Checked rows stay where they are (display-only, no auto-reset).
    assert!(ui.has("agenda-today-0-cb"));

    // ---- row click → OpenFile + GotoLine (1-based line) ----------------------
    ui.click("agenda-overdue-0");
    assert_eq!(
        opened.borrow().as_slice(),
        &[ViewEvent::OpenFile {
            path: note_s.clone(),
            line: Some(1)
        }]
    );
    assert_eq!(
        ui.cx.read(|cx| recorder.read(cx).seen.clone()),
        vec![AppEvent::GotoLine {
            path: note_s.clone(),
            line: 1
        }]
    );

    // ---- keyboard: Enter / Tab + Space on focused rows ----------------------
    opened.borrow_mut().clear();
    // Focus order: routine (calendar column), then overdue-0, today-0,
    // today-1 (the event, line 5), upcoming-0 (line 3), nodate-0.
    let focus = ui.cx.read(|cx| view.read(cx).row_focus.clone());
    ui.cx.update(|window, cx| window.focus(&focus[3], cx));
    ui.press("enter");
    assert_eq!(
        opened.borrow().as_slice(),
        &[ViewEvent::OpenFile {
            path: note_s.clone(),
            line: Some(5)
        }]
    );
    opened.borrow_mut().clear();
    ui.press("tab");
    assert!(ui.cx.update(|window, _| focus[4].is_focused(window)));
    ui.press("space");
    assert_eq!(
        opened.borrow().as_slice(),
        &[ViewEvent::OpenFile {
            path: note_s.clone(),
            line: Some(3)
        }]
    );
    opened.borrow_mut().clear();
    ui.press("shift-tab");
    assert!(ui.cx.update(|window, _| focus[3].is_focused(window)));

    // ---- routine row opens too -----------------------------------------------
    ui.click("agenda-routine-0");
    assert_eq!(
        opened.borrow().as_slice(),
        &[ViewEvent::OpenFile {
            path: note_s.clone(),
            line: Some(6)
        }]
    );

    // ---- day filter + clear --------------------------------------------------
    let selected = |ui: &mut Ui| ui.cx.read(|cx| view.read(cx).selected.clone());
    assert!(!ui.has("agenda-clear-filter"));
    ui.click(&format!("agenda-day-{tomorrow}"));
    assert_eq!(selected(&mut ui), Some(tomorrow.clone()));
    // Filtered to tomorrow: it is both "today" (selected) and upcoming;
    // nothing overdue, nothing undated.
    assert!(ui.has("agenda-clear-filter"));
    assert!(!ui.has("agenda-overdue-0"));
    assert!(!ui.has("agenda-nodate-0"));
    assert!(ui.has("agenda-today-0"));
    assert!(!ui.has("agenda-today-1"));
    assert!(ui.has("agenda-upcoming-0"));
    // Clicking the selected day again un-selects it.
    ui.click(&format!("agenda-day-{tomorrow}"));
    assert_eq!(selected(&mut ui), None);
    ui.click(&format!("agenda-day-{yesterday}"));
    assert_eq!(selected(&mut ui), Some(yesterday.clone()));
    assert!(ui.has("agenda-overdue-0"));
    ui.click("agenda-clear-filter");
    assert_eq!(selected(&mut ui), None);
    assert!(!ui.has("agenda-clear-filter"));
    assert!(ui.has("agenda-nodate-0"));

    // ---- month navigation ----------------------------------------------------
    let cursor = |ui: &mut Ui| ui.cx.read(|cx| view.read(cx).cursor);
    let start = MonthCursor::of(today);
    ui.click("agenda-next");
    assert_eq!(cursor(&mut ui), start.next());
    assert!(!ui.has(&format!("agenda-day-{today_s}")));
    ui.click("agenda-prev");
    ui.click("agenda-prev");
    assert_eq!(cursor(&mut ui), start.prev());
    ui.click("agenda-next");
    assert_eq!(cursor(&mut ui), start);
    assert!(ui.has(&format!("agenda-day-{today_s}")));

    // ---- fs:changed under the vault → rescan; outside it → ignored ----------
    std::fs::write(vault.join("more.md"), "- [ ] brand new task\n").unwrap();
    ui.cx.update(|_, cx| {
        bus::emit(
            cx,
            AppEvent::Core(CoreEvent::FsChanged {
                dir: root.clone(),
                from: None,
            }),
        )
    });
    ui.wait(|cx| {
        loaded(&view, cx)
            .is_some_and(|items| items.iter().any(|i| i.text.contains("brand new task")))
    })
    .await;
    std::fs::write(vault.join("ignored.md"), "- [ ] should wait\n").unwrap();
    ui.cx.update(|_, cx| {
        bus::emit(
            cx,
            AppEvent::Core(CoreEvent::FsChanged {
                dir: "/elsewhere".into(),
                from: None,
            }),
        )
    });
    ui.cx.run_until_parked();
    assert!(ui.cx.read(|cx| {
        !loaded(&view, cx)
            .unwrap()
            .iter()
            .any(|i| i.text.contains("should wait"))
    }));

    let _ = std::fs::remove_dir_all(&vault);
}
