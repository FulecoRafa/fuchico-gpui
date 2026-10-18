//! Tasks & Calendar main view (spec 04 §7–§9, original
//! `src/modules/agenda/**`).
//!
//! * Scans the vault with `fuchico_core::tasks::tasks_scan` off the UI thread
//!   on mount, on root change and on every `fs:file-written` / `fs:changed`
//!   under the vault (coalesced: at most one scan in flight plus one queued).
//!   "Scanning…" only shows before the first result.
//! * Left column: month calendar (Sunday first, dot on days with items,
//!   click toggles the day filter, "Clear filter") and the Routines panel.
//! * Right column: Overdue / Today / Upcoming / No date via
//!   `fuchico_core::agenda::bucket_items`, in scan order.
//! * Rows: checkbox (tasks) → `tasks_toggle`; click / Enter / Space →
//!   [`ViewEvent::OpenFile`] at the 1-based line plus a bus
//!   [`AppEvent::GotoLine`]; right-click → Open / Reveal / Copy path.
//!
//! Deviations (DECISIONS.md: genuine bugs are fixed):
//! * "today" is the **local** date (the original used UTC), and a clock
//!   re-renders the view when the local date rolls over.
//! * A successful toggle emits `fs:file-written {source: "agenda"}` so open
//!   editors reload instead of later overwriting the toggle with a stale
//!   buffer (the rescan rides on that same event).

mod model;
#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
mod ui_tests;

use std::time::Duration;

use fuchico_core::agenda::{bucket_items, items_by_date, month_weeks};
use fuchico_core::events::CoreEvent;
use fuchico_core::tasks::{AgendaItem, ItemKind, tasks_scan, tasks_toggle};
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, EventEmitter, FocusHandle, FontWeight, Hsla, KeyDownEvent,
    SharedString, Subscription, Task, Window, div,
};

use crate::assets::AppIcon;
use crate::bus::{self, AppEvent};
use crate::theme::AppTheme;
use crate::ui::{MenuEntry, build_menu, empty_state, u};
use crate::workspace::view::{MainView, ViewEvent};
use model::{MonthCursor, basename, day_of, iso, local_today, month_title, recurrence_label};

/// `useAgenda` state.
enum ScanState {
    Idle,
    Loading,
    Loaded(Vec<AgendaItem>),
    Error(String),
}

pub struct AgendaView {
    root: Option<String>,
    state: ScanState,
    /// Bumped on root change; stale scan results are dropped.
    generation: u64,
    scanning: bool,
    rescan_queued: bool,
    cursor: MonthCursor,
    selected: Option<String>,
    /// Last local date rendered (the clock notifies when it changes).
    today: String,
    /// Focus handles for keyboard-reachable rows, by render order.
    row_focus: Vec<FocusHandle>,
    _bus: Subscription,
    _clock: Task<()>,
}

impl AgendaView {
    pub fn new(root: Option<String>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sub = bus::subscribe(cx, |this: &mut AgendaView, ev, cx| this.on_bus(ev, cx));
        let clock = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(30))
                    .await;
                let alive = this
                    .update(cx, |this, cx| {
                        if iso(local_today()) != this.today {
                            cx.notify();
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        });
        let today = local_today();
        let mut this = AgendaView {
            root: None,
            state: ScanState::Idle,
            generation: 0,
            scanning: false,
            rescan_queued: false,
            cursor: MonthCursor::of(today),
            selected: None,
            today: iso(today),
            row_focus: Vec::new(),
            _bus: sub,
            _clock: clock,
        };
        this.set_root(root, cx);
        this
    }

    fn on_bus(&mut self, ev: &AppEvent, cx: &mut Context<Self>) {
        let Some(root) = self.root.as_deref() else {
            return;
        };
        match ev {
            // The original rescans on every `fs:file-written`, from any source.
            AppEvent::Core(CoreEvent::FileWritten { .. }) => self.scan(cx),
            AppEvent::Core(CoreEvent::FsChanged { dir, .. }) if is_under(dir, root) => {
                self.scan(cx)
            }
            _ => {}
        }
    }

    /// Starts a scan, or queues one if a scan is already running.
    fn scan(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if self.scanning {
            self.rescan_queued = true;
            return;
        }
        self.scanning = true;
        if !matches!(self.state, ScanState::Loaded(_)) {
            self.state = ScanState::Loading;
            cx.notify();
        }
        let generation = self.generation;
        let task = cx.background_spawn(async move { tasks_scan(root) });
        cx.spawn(async move |this, cx| {
            let res = task.await;
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.scanning = false;
                this.state = match res {
                    Ok(items) => ScanState::Loaded(items),
                    Err(e) => ScanState::Error(e),
                };
                cx.notify();
                if std::mem::take(&mut this.rescan_queued) {
                    this.scan(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// `toggle(item)`: tasks only. Failure is logged and the checkbox keeps
    /// its old state (it mirrors the scanned `checked`).
    fn toggle(&mut self, item: &AgendaItem, cx: &mut Context<Self>) {
        if item.kind != ItemKind::Task {
            return;
        }
        let (path, line) = (item.file.clone(), item.line);
        let task = cx.background_spawn({
            let path = path.clone();
            async move { tasks_toggle(path, line) }
        });
        cx.spawn(async move |this, cx| match task.await {
            Ok(()) => {
                bus::send(AppEvent::Core(CoreEvent::FileWritten {
                    path,
                    source: Some("agenda".into()),
                }));
            }
            Err(e) => {
                log::error!("tasks_toggle failed: {e}");
                this.update(cx, |this, cx| this.scan(cx)).ok();
            }
        })
        .detach();
    }

    /// `openFile(item.file, item.line)`.
    fn open(&mut self, path: &str, line: usize, cx: &mut Context<Self>) {
        let line = line as u32;
        cx.emit(ViewEvent::OpenFile {
            path: path.to_string(),
            line: Some(line),
        });
        bus::emit(
            cx,
            AppEvent::GotoLine {
                path: path.to_string(),
                line,
            },
        );
    }

    fn focus_handle_at(&mut self, ix: usize, cx: &mut Context<Self>) -> FocusHandle {
        while self.row_focus.len() <= ix {
            self.row_focus.push(cx.focus_handle().tab_stop(true));
        }
        self.row_focus[ix].clone()
    }

    /// `fileRowMenuItems(path, {onOpen})`.
    fn row_menu(&self, path: &str, line: usize, cx: &mut Context<Self>) -> Vec<MenuEntry> {
        let this = cx.entity().downgrade();
        let (p_open, p_reveal, p_copy) = (path.to_string(), path.to_string(), path.to_string());
        vec![
            MenuEntry::item(crate::t!(CommonOpen), move |_, cx| {
                this.update(cx, |this, cx| this.open(&p_open, line, cx))
                    .ok();
            }),
            MenuEntry::Separator,
            MenuEntry::item(crate::t!(CommonRevealInFileManager), move |_, _| {
                reveal(&p_reveal)
            })
            .icon(IconName::Eye),
            MenuEntry::item(crate::t!(CommonCopyPath), move |_, cx| {
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(p_copy.clone()))
            })
            .icon(IconName::Copy),
        ]
    }

    // ----- rendering --------------------------------------------------------

    fn render_calendar(
        &mut self,
        items: &[AgendaItem],
        routines: &[&AgendaItem],
        focus_ix: &mut usize,
        th: &AppTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let by_date = items_by_date(items);
        let weeks = month_weeks(self.cursor.year, self.cursor.month);
        let letters: Vec<String> = crate::t!(AgendaWeekdayLetters)
            .split(',')
            .map(String::from)
            .collect();
        let hover = th.fg_alpha(0.06);
        let (muted, fg) = (th.colors.muted_foreground, th.colors.foreground);
        let radius = th.radius.sm;

        let nav = |id: &'static str, icon: IconName, prev: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .debug_selector(|| id.to_string())
                .flex()
                .items_center()
                .justify_center()
                .size(u(24.))
                .rounded(u(radius))
                .text_color(muted)
                .cursor_pointer()
                .hover(move |s| s.bg(hover).text_color(fg))
                .child(Icon::new(icon).size(u(14.)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.cursor = if prev {
                        this.cursor.prev()
                    } else {
                        this.cursor.next()
                    };
                    cx.notify();
                }))
        };

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .mb(u(12.))
            .child(nav("agenda-prev", IconName::ChevronLeft, true, cx))
            .child(
                div()
                    .text_size(u(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(month_title(crate::i18n::locale(), self.cursor)),
            )
            .child(nav("agenda-next", IconName::ChevronRight, false, cx));

        let weekday_row = div().flex().gap(u(2.)).children((0..7).map(|i| {
            div()
                .flex_1()
                .pb(u(4.))
                .flex()
                .justify_center()
                .text_size(u(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(muted)
                .child(letters.get(i).cloned().unwrap_or_default())
        }));

        let mut grid = div().flex().flex_col().gap(u(2.)).child(weekday_row);
        for week in weeks {
            let mut row = div().flex().gap(u(2.));
            for cell in week {
                let Some(day) = cell.iso else {
                    row = row.child(div().flex_1().h(u(30.)));
                    continue;
                };
                let is_today = day == self.today;
                let is_sel = self.selected.as_deref() == Some(day.as_str());
                let has_items = by_date.contains_key(&day);
                let dot = if is_sel {
                    th.colors.accent_foreground
                } else {
                    th.colors.syntax_function
                };
                let d2 = day.clone();
                row = row.child(
                    div()
                        .id(SharedString::from(format!("agenda-day-{day}")))
                        .debug_selector(|| format!("agenda-day-{day}"))
                        .relative()
                        .flex_1()
                        .h(u(30.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .border_1()
                        .border_color(if is_today {
                            th.colors.ring
                        } else {
                            gpui_kit::transparent_black()
                        })
                        .rounded(u(radius))
                        .text_size(u(12.))
                        .cursor_pointer()
                        .when(is_today, |d| d.font_weight(FontWeight::BOLD))
                        .when(is_sel, |d| {
                            d.bg(th.colors.accent)
                                .text_color(th.colors.accent_foreground)
                        })
                        .when(!is_sel, |d| d.text_color(fg).hover(move |s| s.bg(hover)))
                        .child(day_of(&day).to_string())
                        .when(has_items, |d| {
                            d.child(
                                div()
                                    .absolute()
                                    .bottom(u(3.))
                                    .left_0()
                                    .right_0()
                                    .flex()
                                    .justify_center()
                                    .child(div().size(u(4.)).rounded_full().bg(dot)),
                            )
                        })
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.selected = if this.selected.as_deref() == Some(d2.as_str()) {
                                None
                            } else {
                                Some(d2.clone())
                            };
                            cx.notify();
                        })),
                );
            }
            grid = grid.child(row);
        }

        let clear = self.selected.clone().map(|date| {
            div()
                .id("agenda-clear-filter")
                .debug_selector(|| "agenda-clear-filter".into())
                .mt(u(12.))
                .w_full()
                .flex()
                .justify_center()
                .border_1()
                .border_color(th.colors.border)
                .rounded(u(radius))
                .px(u(8.))
                .py(u(5.))
                .text_size(u(11.))
                .text_color(muted)
                .cursor_pointer()
                .hover(move |s| s.text_color(fg).border_color(fg))
                .child(crate::t!(AgendaClearFilter, date = date))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.selected = None;
                    cx.notify();
                }))
        });

        let mut routines_panel = div()
            .mt(u(16.))
            .pt(u(12.))
            .border_t_1()
            .border_color(th.colors.border)
            .flex()
            .flex_col()
            .child(section_title(crate::t!(AgendaRoutines), th));
        if routines.is_empty() {
            routines_panel = routines_panel.child(rich_status(
                vec![
                    Part::Text(crate::t!(AgendaNoRoutinesPrefix)),
                    Part::Code("🔁 daily|weekdays|weekends|mon,…".into()),
                    Part::Text(crate::t!(AgendaNoRoutinesSuffix)),
                ],
                th,
            ));
        }
        for (i, item) in routines.iter().enumerate() {
            let focus = self.focus_handle_at(*focus_ix, cx);
            *focus_ix += 1;
            routines_panel = routines_panel.child(self.render_routine(i, item, focus, th, cx));
        }

        div()
            .id("agenda-calendar")
            .w(u(280.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .p(u(16.))
            .border_r_1()
            .border_color(th.colors.border)
            .child(header)
            .child(grid)
            .children(clear)
            .child(routines_panel)
            .into_any_element()
    }

    fn render_routine(
        &self,
        ix: usize,
        item: &AgendaItem,
        focus: FocusHandle,
        th: &AppTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (path, line) = (item.file.clone(), item.line);
        let (p2, p3) = (path.clone(), path.clone());
        let view = cx.entity().downgrade();
        let rule = recurrence_label(
            item.recurrence.as_deref().unwrap_or_default(),
            item.recur_time.as_deref(),
        );
        let tint = th.fg_alpha(0.05);
        div()
            .id(("agenda-routine", ix))
            .debug_selector(|| format!("agenda-routine-{ix}"))
            .track_focus(&focus)
            .flex()
            .flex_col()
            .items_start()
            .gap(u(2.))
            .w_full()
            .px(u(6.))
            .py(u(5.))
            .rounded(u(th.radius.sm))
            .cursor_pointer()
            .hover(move |s| s.bg(tint))
            .focus_visible(move |s| s.bg(tint))
            .child(
                div()
                    .w_full()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(u(12.))
                    .child(item.text.clone()),
            )
            .child(
                div()
                    .text_size(u(10.))
                    .text_color(th.colors.muted_foreground)
                    .child(rule),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open(&path, line, cx)))
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, _, cx| {
                if is_activate_key(ev) {
                    cx.stop_propagation();
                    this.open(&p2, line, cx);
                }
            }))
            .context_menu(move |menu, _, cx| {
                let items = view
                    .update(cx, |this, cx| this.row_menu(&p3, line, cx))
                    .unwrap_or_default();
                build_menu(menu, items)
            })
            .into_any_element()
    }

    fn render_row(
        &self,
        key: &str,
        item: &AgendaItem,
        focus: FocusHandle,
        th: &AppTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (path, line) = (item.file.clone(), item.line);
        let (p2, p3) = (path.clone(), path.clone());
        let view = cx.entity().downgrade();
        let leading: AnyElement = match item.kind {
            ItemKind::Task => {
                let it = item.clone();
                let checked = item.checked;
                div()
                    .id(SharedString::from(format!("{key}-cb")))
                    .debug_selector(|| format!("{key}-cb"))
                    .flex_none()
                    .size(u(14.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(u(3.))
                    .border_1()
                    .cursor_pointer()
                    .when(checked, |d| {
                        d.bg(th.colors.primary)
                            .border_color(th.colors.primary)
                            .text_color(th.colors.primary_foreground)
                            .child(Icon::new(IconName::Check).size(u(11.)))
                    })
                    .when(!checked, |d| {
                        d.border_color(th.colors.muted_foreground)
                            .bg(th.colors.background)
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.toggle(&it, cx);
                    }))
                    .into_any_element()
            }
            ItemKind::Event => row_icon(AppIcon::CalendarClock, th),
            ItemKind::Todo => row_icon(AppIcon::ListTodo, th),
        };
        let text: String = if item.text.is_empty() {
            crate::t!(AgendaEmptyItem)
        } else {
            item.text.clone()
        };
        let recur = item.recurrence.as_deref().map(|r| {
            let tip: SharedString = crate::t!(AgendaRecurringTask).into();
            div()
                .id(SharedString::from(format!("{key}-recur")))
                .flex_none()
                .text_size(u(10.))
                .text_color(th.colors.muted_foreground)
                .bg(th.colors.secondary)
                .rounded(u(th.radius.sm))
                .px(u(6.))
                .py(u(1.))
                .whitespace_nowrap()
                .child(recurrence_label(r, item.recur_time.as_deref()))
                .tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx))
        });
        let full: SharedString = item.file.clone().into();
        let tint = th.fg_alpha(0.05);
        div()
            .id(SharedString::from(key.to_string()))
            .debug_selector(|| key.to_string())
            .track_focus(&focus)
            .flex()
            .items_center()
            .gap(u(8.))
            .w_full()
            .min_w_0()
            .px(u(6.))
            .py(u(5.))
            .rounded(u(th.radius.sm))
            .cursor_pointer()
            .text_size(u(13.))
            .hover(move |s| s.bg(tint))
            .focus_visible(move |s| s.bg(tint))
            .child(leading)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .when(item.checked, |d| {
                        d.text_color(th.colors.muted_foreground).line_through()
                    })
                    .child(text),
            )
            .children(item.time.clone().map(|t| {
                div()
                    .flex_none()
                    .font_family(th.mono_font.clone())
                    .text_size(u(11.))
                    .text_color(th.colors.muted_foreground)
                    .child(t)
            }))
            .children(recur)
            .child(
                div()
                    .id(SharedString::from(format!("{key}-file")))
                    .flex_none()
                    .max_w(u(140.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(u(11.))
                    .text_color(th.colors.muted_foreground)
                    .child(basename(&item.file).to_string())
                    .tooltip(move |w, cx| Tooltip::new(full.clone()).build(w, cx)),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open(&path, line, cx)))
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, _, cx| {
                if is_activate_key(ev) {
                    cx.stop_propagation();
                    this.open(&p2, line, cx);
                }
            }))
            .context_menu(move |menu, _, cx| {
                let items = view
                    .update(cx, |this, cx| this.row_menu(&p3, line, cx))
                    .unwrap_or_default();
                build_menu(menu, items)
            })
            .into_any_element()
    }

    fn render_list(
        &mut self,
        items: &[AgendaItem],
        focus_ix: &mut usize,
        th: &AppTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut list = div()
            .id("agenda-list")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .px(u(20.))
            .py(u(16.))
            .flex()
            .flex_col();
        match &self.state {
            ScanState::Idle => {}
            ScanState::Loading => {
                list = list.child(status(
                    crate::t!(CommonScanning),
                    th.colors.muted_foreground,
                ))
            }
            ScanState::Error(e) => list = list.child(status(e.clone(), th.colors.destructive)),
            ScanState::Loaded(_) if items.is_empty() => {
                list = list.child(rich_status(
                    vec![
                        Part::Text(crate::t!(AgendaNoTasksPrefix)),
                        // `<code>- [ ] …</code>,` — the comma hugs the chip.
                        Part::CodeThen("- [ ] …".into(), ",".into()),
                        Part::CodeThen("TODO: …".into(), ",".into()),
                        Part::Text(crate::t!(AgendaOr)),
                        Part::Code("📅 YYYY-MM-DD …".into()),
                        Part::Text(crate::t!(AgendaNoTasksSuffix)),
                    ],
                    th,
                ))
            }
            ScanState::Loaded(_) => {
                let buckets = bucket_items(items, &self.today, self.selected.as_deref());
                let sections = [
                    ("overdue", crate::t!(AgendaOverdue), &buckets.overdue),
                    ("today", crate::t!(AgendaToday), &buckets.due),
                    ("upcoming", crate::t!(AgendaUpcoming), &buckets.upcoming),
                    ("nodate", crate::t!(AgendaNoDate), &buckets.no_date),
                ];
                let mut any = false;
                for (id, title, rows) in sections {
                    if rows.is_empty() {
                        continue;
                    }
                    any = true;
                    let mut section = div()
                        .flex()
                        .flex_col()
                        .mb(u(18.))
                        .child(section_title(title, th));
                    for (i, item) in rows.iter().enumerate() {
                        let focus = self.focus_handle_at(*focus_ix, cx);
                        *focus_ix += 1;
                        section = section.child(self.render_row(
                            &format!("agenda-{id}-{i}"),
                            item,
                            focus,
                            th,
                            cx,
                        ));
                    }
                    list = list.child(section);
                }
                if !any {
                    list = list.child(status(
                        crate::t!(AgendaNothingHere),
                        th.colors.muted_foreground,
                    ));
                }
            }
        }
        list.into_any_element()
    }
}

impl EventEmitter<ViewEvent> for AgendaView {}

impl MainView for AgendaView {
    fn set_root(&mut self, root: Option<String>, cx: &mut Context<Self>) {
        if self.root == root && !matches!(self.state, ScanState::Idle) {
            return;
        }
        self.root = root;
        self.generation += 1;
        self.scanning = false;
        self.rescan_queued = false;
        self.state = ScanState::Idle;
        self.selected = None;
        self.scan(cx);
        cx.notify();
    }
}

impl Render for AgendaView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.root.is_none() {
            return empty_state(crate::t!(AgendaOpenFolder), cx)
                .p(u(24.))
                .into_any_element();
        }
        // Evaluated on each render like the original, but in local time.
        self.today = iso(local_today());
        let th = AppTheme::get(cx).clone();
        // Items are moved out for the render so rows can borrow them while
        // `self` hands out focus handles; they go back right after.
        let items = match &mut self.state {
            ScanState::Loaded(items) => std::mem::take(items),
            _ => Vec::new(),
        };
        let routines: Vec<&AgendaItem> = items.iter().filter(|i| i.recurrence.is_some()).collect();
        let mut focus_ix = 0usize;
        let calendar = self.render_calendar(&items, &routines, &mut focus_ix, &th, cx);
        let list = self.render_list(&items, &mut focus_ix, &th, cx);
        drop(routines);
        if let ScanState::Loaded(slot) = &mut self.state {
            *slot = items;
        }

        div()
            .id("agenda-view")
            .key_context("AgendaView")
            .flex()
            .size_full()
            .min_h_0()
            .overflow_hidden()
            .bg(th.colors.background)
            .text_color(th.colors.foreground)
            // CSS `line-height: normal` (~1.2) instead of gpui's default phi,
            // so row/section pitch matches the original.
            .line_height(gpui_kit::relative(1.2))
            .on_key_down(|ev: &KeyDownEvent, window, cx| {
                // Rows are tab stops (role=button tabIndex=0 in the original).
                if ev.keystroke.key == "tab"
                    && !ev.keystroke.modifiers.platform
                    && !ev.keystroke.modifiers.control
                {
                    if ev.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }
            })
            .child(calendar)
            .child(list)
            .into_any_element()
    }
}

// ----- helpers ---------------------------------------------------------------

/// `dir` is the vault root or inside it.
fn is_under(dir: &str, root: &str) -> bool {
    dir == root
        || dir
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/') || rest.starts_with('\\'))
}

fn is_activate_key(ev: &KeyDownEvent) -> bool {
    let k = &ev.keystroke;
    !k.modifiers.modified() && (k.key == "enter" || k.key == "space")
}

fn row_icon(icon: AppIcon, th: &AppTheme) -> AnyElement {
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(u(14.))
        .text_color(th.colors.muted_foreground)
        .child(Icon::new(icon).size(u(13.)))
        .into_any_element()
}

fn section_title(title: String, th: &AppTheme) -> gpui_kit::Div {
    div()
        .mb(u(6.))
        .text_size(u(11.))
        .font_weight(FontWeight::BOLD)
        .text_color(th.colors.muted_foreground)
        .child(title.to_uppercase())
}

fn status(text: String, color: Hsla) -> gpui_kit::Div {
    div()
        .py(u(8.))
        .text_size(u(12.))
        .text_color(color)
        .child(text)
}

enum Part {
    Text(String),
    Code(String),
    /// A chip followed by punctuation with no space between.
    CodeThen(String, String),
}

/// `.agenda-status` with inline `<code>` chips.
fn rich_status(parts: Vec<Part>, th: &AppTheme) -> gpui_kit::Div {
    div()
        .py(u(8.))
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(u(4.))
        .gap_y(u(2.))
        .text_size(u(12.))
        .text_color(th.colors.muted_foreground)
        .children(parts.into_iter().map(|p| {
            let chip = |c: String| {
                div()
                    .font_family(th.mono_font.clone())
                    .text_size(u(11.))
                    .bg(th.colors.secondary)
                    .rounded(u(th.radius.sm))
                    .px(u(4.))
                    .py(u(1.))
                    .child(c)
            };
            match p {
                Part::Text(t) => div().child(t),
                Part::Code(c) => chip(c),
                Part::CodeThen(c, then) => div().flex().items_center().child(chip(c)).child(then),
            }
        }))
}

/// Shows `path` in the OS file manager.
fn reveal(path: &str) {
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("explorer")
        .arg(format!("/select,{path}"))
        .spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open")
        .arg(
            std::path::Path::new(path)
                .parent()
                .unwrap_or(std::path::Path::new("/")),
        )
        .spawn();
    if let Err(e) = r {
        log::warn!("reveal failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::is_under;

    #[test]
    fn fs_changed_scope() {
        assert!(is_under("/v", "/v"));
        assert!(is_under("/v/sub", "/v"));
        assert!(!is_under("/vault2", "/v"));
        assert!(!is_under("/other", "/v"));
    }
}
