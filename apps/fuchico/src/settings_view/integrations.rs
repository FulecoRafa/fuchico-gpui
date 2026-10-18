//! Integrations: CalDAV accounts, folder links and sync (spec 5 §3.14),
//! wired to `fuchico_core::caldav` through the app's shared [`CalDavCtx`].
//! Unlike the original, every string is translated.
//!
//! Network calls run on the core tokio runtime, config reads/writes on the
//! background executor; the UI thread never blocks.

use std::future::Future;
use std::path::Path;

use chrono::{DateTime, Local, Utc};
use fuchico_core::caldav::commands::{self as caldav, AccountInput, CalDavCtx};
use fuchico_core::events::CoreEvent;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, FontWeight, PathPromptOptions, Subscription, Window, div};

use super::widgets::{BtnKind, Section, btn, field, hint, hint_text, row_box, status, text_input};
use super::{Opt, SettingsView};
use crate::assets::AppIcon;
use crate::bus::{self, AppEvent};
use crate::i18n::{Locale, Msg, translate};
use crate::theme::AppTheme;
use crate::ui::{icon_button, u};

pub const ICLOUD_URL: &str = "https://caldav.icloud.com";
pub const APPLE_APP_PASSWORD_URL: &str = "https://appleid.apple.com/account/manage";

type OptSelect = Entity<SelectState<SearchableVec<Opt>>>;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct AccountRow {
    pub id: String,
    pub server_url: String,
    pub username: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct LinkRow {
    pub folder: String,
    pub calendar: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct StatusRow {
    pub folder: String,
    pub last_synced: Option<DateTime<Utc>>,
    pub task_count: usize,
    pub pending: usize,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub(super) struct ReportRow {
    pub folder: String,
    pub pushed_new: usize,
    pub pushed_updated: usize,
    pub pulled_updated: usize,
    pub pulled_new: usize,
    pub errors: Vec<String>,
}

pub(super) struct State {
    ctx: Option<CalDavCtx>,
    accounts: Vec<AccountRow>,
    links: Vec<LinkRow>,
    statuses: Vec<StatusRow>,
    load_error: Option<String>,
    // Account form.
    server: Entity<InputState>,
    user: Entity<InputState>,
    pass: Entity<InputState>,
    testing: bool,
    saving: bool,
    form_status: Option<(String, bool)>,
    // Link form.
    account_select: OptSelect,
    selected_account: Option<String>,
    /// Account list changed; the select is refreshed on render.
    accounts_dirty: bool,
    calendars: Vec<(String, String)>,
    calendar_select: OptSelect,
    selected_calendar: Option<String>,
    discovering: bool,
    /// Folder to link; defaults to the open vault.
    pub folder: Option<String>,
    link_error: Option<String>,
    // Sync.
    syncing: bool,
    sync_error: Option<String>,
    reports: Option<Vec<ReportRow>>,
}

/// `+{new} new, {pushed} pushed, {pulled} pulled, {fromServer} from server[, n error(s)]`.
pub(super) fn report_summary(locale: Locale, r: &ReportRow) -> String {
    let mut s = translate(
        locale,
        Msg::CaldavReportSummary,
        &[("new", &r.pushed_new), ("pushed", &r.pushed_updated), ("pulled", &r.pulled_updated), ("fromServer", &r.pulled_new)],
    );
    if !r.errors.is_empty() {
        s.push_str(&translate(locale, Msg::CaldavReportErrors, &[("n", &r.errors.len())]));
    }
    s
}

/// `Date.toLocaleString()` for the two UI locales.
pub(super) fn format_synced(locale: Locale, at: DateTime<Utc>) -> String {
    let local = at.with_timezone(&Local);
    match locale {
        Locale::En => local.format("%-m/%-d/%Y, %-I:%M:%S %p").to_string(),
        Locale::PtBr => local.format("%d/%m/%Y, %H:%M:%S").to_string(),
    }
}

/// A link row's meta line (task count, pending deletes, last sync).
pub(super) fn link_meta(locale: Locale, st: Option<&StatusRow>) -> String {
    let Some(st) = st else { return translate(locale, Msg::CaldavNeverSynced, &[]) };
    let mut s = translate(locale, Msg::CaldavTaskCount, &[("n", &st.task_count)]);
    if st.pending > 0 {
        s.push_str(&translate(locale, Msg::CaldavPendingDelete, &[("n", &st.pending)]));
    }
    if let Some(at) = st.last_synced {
        s.push_str(&translate(locale, Msg::CaldavSyncedAt, &[("date", &format_synced(locale, at))]));
    }
    s
}

pub(super) fn basename(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

fn account_options(accounts: &[AccountRow]) -> SearchableVec<Opt> {
    SearchableVec::new(
        accounts.iter().map(|a| Opt::new(format!("{} ({})", a.username, a.server_url), a.id.clone())).collect::<Vec<_>>(),
    )
}

fn calendar_options(cals: &[(String, String)]) -> SearchableVec<Opt> {
    SearchableVec::new(cals.iter().map(|(href, name)| Opt::new(name.clone(), href.clone())).collect::<Vec<_>>())
}

struct Snapshot {
    accounts: Result<Vec<AccountRow>, String>,
    links: Result<Vec<LinkRow>, String>,
    statuses: Result<Vec<StatusRow>, String>,
}

fn snapshot(ctx: &CalDavCtx) -> Snapshot {
    Snapshot {
        accounts: caldav::caldav_list_accounts(ctx).map(|v| {
            v.into_iter()
                .map(|a| AccountRow { id: a.id, server_url: a.server_url, username: a.username })
                .collect()
        }),
        links: caldav::caldav_list_links(ctx).map(|v| {
            v.into_iter().map(|l| LinkRow { folder: l.folder_path, calendar: l.calendar_display_name }).collect()
        }),
        statuses: caldav::caldav_get_sync_status(ctx).map(|v| {
            v.into_iter()
                .map(|s| StatusRow {
                    folder: s.folder_path,
                    last_synced: s.last_synced_at,
                    task_count: s.task_count,
                    pending: s.pending_deletes,
                })
                .collect()
        }),
    }
}

impl State {
    pub fn new(
        root: Option<String>,
        window: &mut Window,
        cx: &mut Context<SettingsView>,
        subs: &mut Vec<Subscription>,
    ) -> State {
        let server = cx.new(|cx| InputState::new(window, cx).default_value(ICLOUD_URL));
        let user = cx.new(|cx| InputState::new(window, cx).placeholder(crate::t!(CaldavUsernamePlaceholder)));
        let pass = cx.new(|cx| InputState::new(window, cx).masked(true));
        for input in [&server, &user, &pass] {
            subs.push(cx.subscribe_in(input, window, |_, _, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Change) {
                    cx.notify();
                }
            }));
        }
        let account_select = cx.new(|cx| SelectState::new(account_options(&[]), None, window, cx));
        subs.push(cx.subscribe_in(&account_select, window, |this, _, ev: &SelectEvent<SearchableVec<Opt>>, window, cx| {
            let SelectEvent::Confirm(Some(v)) = ev else { return };
            let st = &mut this.caldav;
            if st.selected_account.as_deref() != Some(v.as_ref()) {
                st.selected_account = Some(v.to_string());
                st.calendars.clear();
                st.selected_calendar = None;
                st.calendar_select.update(cx, |s, cx| s.set_items(calendar_options(&[]), window, cx));
            }
            cx.notify();
        }));
        let calendar_select = cx.new(|cx| SelectState::new(calendar_options(&[]), None, window, cx));
        subs.push(cx.subscribe_in(&calendar_select, window, |this, _, ev: &SelectEvent<SearchableVec<Opt>>, _, cx| {
            if let SelectEvent::Confirm(Some(v)) = ev {
                this.caldav.selected_calendar = Some(v.to_string());
                cx.notify();
            }
        }));
        // Background syncs (launch, poll, after save) refresh the status.
        subs.push(bus::subscribe(cx, |this, ev, cx| {
            if let AppEvent::Core(CoreEvent::CaldavSyncComplete { .. }) = ev {
                this.refresh_caldav(cx);
            }
        }));
        State {
            ctx: crate::caldav::ctx(cx),
            accounts: Vec::new(),
            links: Vec::new(),
            statuses: Vec::new(),
            load_error: None,
            server,
            user,
            pass,
            testing: false,
            saving: false,
            form_status: None,
            account_select,
            selected_account: None,
            accounts_dirty: false,
            calendars: Vec::new(),
            calendar_select,
            selected_calendar: None,
            discovering: false,
            folder: root,
            link_error: None,
            syncing: false,
            sync_error: None,
            reports: None,
        }
    }
}

impl SettingsView {
    /// Runs `fut` on the core tokio runtime, then `done` on the view.
    fn caldav_async<T: Send + 'static>(
        &mut self,
        cx: &mut Context<Self>,
        fut: impl Future<Output = T> + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        let handle = fuchico_core::runtime::spawn(fut);
        cx.spawn(async move |this, cx| {
            let Ok(v) = handle.await else { return };
            this.update(cx, |this, cx| {
                done(this, v, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Runs blocking config IO on the background executor, then `done`.
    fn caldav_bg<T: Send + 'static>(
        &mut self,
        cx: &mut Context<Self>,
        work: impl FnOnce(&CalDavCtx) -> T + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        let Some(ctx) = self.caldav.ctx.clone() else { return };
        let task = cx.background_spawn(async move { work(&ctx) });
        cx.spawn(async move |this, cx| {
            let v = task.await;
            this.update(cx, |this, cx| {
                done(this, v, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Reloads accounts, links and per-folder status.
    pub(super) fn refresh_caldav(&mut self, cx: &mut Context<Self>) {
        self.caldav_bg(cx, snapshot, |this, snap, _| {
            let st = &mut this.caldav;
            let mut errors = Vec::new();
            match snap.accounts {
                Ok(a) => st.accounts = a,
                Err(e) => errors.push(e),
            }
            match snap.links {
                Ok(l) => st.links = l,
                Err(e) => errors.push(e),
            }
            match snap.statuses {
                Ok(s) => st.statuses = s,
                Err(e) => errors.push(e),
            }
            st.load_error = (!errors.is_empty()).then(|| errors.join("; "));
            if st.selected_account.as_ref().is_none_or(|id| !st.accounts.iter().any(|a| &a.id == id)) {
                st.selected_account = st.accounts.first().map(|a| a.id.clone());
                st.calendars.clear();
                st.selected_calendar = None;
            }
            // Select updates need a window: applied on the next render.
            st.accounts_dirty = true;
        });
    }

    fn form_values(&self, cx: &gpui_kit::App) -> (String, String, String) {
        let st = &self.caldav;
        (
            st.server.read(cx).value().trim().to_string(),
            st.user.read(cx).value().trim().to_string(),
            st.pass.read(cx).value().to_string(),
        )
    }

    fn test_connection(&mut self, cx: &mut Context<Self>) {
        let (server, user, pass) = self.form_values(cx);
        self.caldav.testing = true;
        self.caldav.form_status = None;
        self.caldav_async(cx, caldav::caldav_test_connection(server, user, pass), |this, res, _| {
            this.caldav.testing = false;
            this.caldav.form_status = Some(match res {
                Ok(info) => (crate::t!(CaldavConnected, url = info.calendar_home_url), true),
                Err(e) => (e, false),
            });
        });
    }

    fn save_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ctx) = self.caldav.ctx.clone() else { return };
        let (server_url, username, password) = self.form_values(cx);
        self.caldav.saving = true;
        let fut = async move { caldav::caldav_save_account(&ctx, AccountInput { server_url, username, password }).await };
        let (user, pass) = (self.caldav.user.clone(), self.caldav.pass.clone());
        let handle = fuchico_core::runtime::spawn(fut);
        cx.spawn_in(window, async move |this, cx| {
            let Ok(res) = handle.await else { return };
            this.update_in(cx, |this, window, cx| {
                this.caldav.saving = false;
                match res {
                    Ok(_) => {
                        user.update(cx, |i, cx| i.set_value("", window, cx));
                        pass.update(cx, |i, cx| i.set_value("", window, cx));
                        this.caldav.form_status = None;
                        this.refresh_caldav(cx);
                    }
                    Err(e) => this.caldav.form_status = Some((e, false)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn remove_account(&mut self, id: String, cx: &mut Context<Self>) {
        self.caldav_bg(cx, move |ctx| caldav::caldav_remove_account(ctx, id), |this, res, cx| {
            this.caldav.load_error = res.err();
            this.refresh_caldav(cx);
        });
    }

    fn discover_calendars(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(ctx), Some(id)) = (self.caldav.ctx.clone(), self.caldav.selected_account.clone()) else { return };
        self.caldav.discovering = true;
        self.caldav.link_error = None;
        let handle = fuchico_core::runtime::spawn(async move {
            caldav::caldav_discover_calendars_for_account(&ctx, id)
                .await
                .map(|v| v.into_iter().map(|c| (c.href, c.display_name)).collect::<Vec<_>>())
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(res) = handle.await else { return };
            this.update_in(cx, |this, window, cx| {
                let st = &mut this.caldav;
                st.discovering = false;
                match res {
                    Ok(cals) => {
                        st.selected_calendar = cals.first().map(|c| c.0.clone());
                        let items = calendar_options(&cals);
                        st.calendars = cals;
                        let selected = st.selected_calendar.clone();
                        st.calendar_select.update(cx, |s, cx| {
                            s.set_items(items, window, cx);
                            if let Some(h) = selected {
                                s.set_selected_value(&h.into(), window, cx);
                            }
                        });
                    }
                    Err(e) => st.link_error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn pick_folder(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(crate::t!(CaldavChooseFolder).into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(p) = paths.into_iter().next() else { return };
            this.update(cx, |this, cx| {
                this.caldav.folder = Some(p.to_string_lossy().to_string());
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn link_folder(&mut self, cx: &mut Context<Self>) {
        let st = &self.caldav;
        let (Some(account), Some(href), Some(folder)) =
            (st.selected_account.clone(), st.selected_calendar.clone(), st.folder.clone())
        else {
            return;
        };
        let name = st.calendars.iter().find(|c| c.0 == href).map(|c| c.1.clone()).unwrap_or_else(|| href.clone());
        self.caldav.link_error = None;
        self.caldav_bg(cx, move |ctx| caldav::caldav_link_folder(ctx, account, href, name, folder), |this, res, cx| {
            this.caldav.link_error = res.err();
            this.refresh_caldav(cx);
        });
    }

    fn unlink_folder(&mut self, folder: String, cx: &mut Context<Self>) {
        self.caldav_bg(cx, move |ctx| caldav::caldav_unlink_folder(ctx, folder), |this, res, cx| {
            this.caldav.link_error = res.err();
            this.refresh_caldav(cx);
        });
    }

    fn sync_now(&mut self, folder: Option<String>, cx: &mut Context<Self>) {
        let Some(ctx) = self.caldav.ctx.clone() else { return };
        if self.caldav.syncing {
            return;
        }
        self.caldav.syncing = true;
        self.caldav.sync_error = None;
        let fut = async move {
            caldav::caldav_sync_now(&ctx, folder).await.map(|reports| {
                reports
                    .into_iter()
                    .map(|r| ReportRow {
                        folder: r.folder,
                        pushed_new: r.pushed_new,
                        pushed_updated: r.pushed_updated,
                        pulled_updated: r.pulled_updated,
                        pulled_new: r.pulled_new,
                        errors: r.errors,
                    })
                    .collect::<Vec<_>>()
            })
        };
        self.caldav_async(cx, fut, |this, res, cx| {
            this.caldav.syncing = false;
            match res {
                Ok(r) => this.caldav.reports = Some(r),
                Err(e) => this.caldav.sync_error = Some(e),
            }
            this.refresh_caldav(cx);
        });
    }

    pub(super) fn integrations_sections(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Section> {
        if std::mem::take(&mut self.caldav.accounts_dirty) {
            let st = &self.caldav;
            let (items, selected) = (account_options(&st.accounts), st.selected_account.clone());
            st.account_select.update(cx, |s, cx| {
                s.set_items(items, window, cx);
                if let Some(id) = selected {
                    s.set_selected_value(&id.into(), window, cx);
                }
            });
        }
        let mut head = Section::new(crate::t!(CaldavTitle), crate::t!(CaldavDesc));
        // Without a data dir (no CalDavCtx) the actions stay disabled.
        if let Some(e) = self.caldav.load_error.clone() {
            head = head.other(status(e, false, cx));
        }
        vec![head, self.accounts_section(cx), self.links_section(cx), self.sync_section(cx)]
    }

    fn accounts_section(&mut self, cx: &mut Context<Self>) -> Section {
        let th = AppTheme::get(cx).clone();
        let mut sec = Section::new(crate::t!(CaldavAccounts), String::new());
        for a in self.caldav.accounts.clone() {
            let id = a.id.clone();
            let row = row_box(cx)
                .debug_selector({
                    let id = a.id.clone();
                    move || format!("caldav-account-{id}")
                })
                .text_size(u(12.))
                .child(div().font_weight(FontWeight::MEDIUM).child(a.username.clone()))
                .child(div().flex_1().min_w_0().truncate().text_color(th.colors.muted_foreground).child(a.server_url.clone()))
                .child(icon_button(
                    gpui_kit::SharedString::from(format!("caldav-remove-{}", a.id)),
                    AppIcon::Trash,
                    22.,
                    13.,
                    crate::t!(CaldavRemoveAccount),
                    false,
                    cx,
                    cx.listener(move |this, _, _, cx| this.remove_account(id.clone(), cx)),
                ));
            sec = sec.field(format!("{} {}", a.username, a.server_url), row);
        }
        let (server, user, _) = self.form_values(cx);
        let pass_set = !self.caldav.pass.read(cx).value().trim().is_empty();
        let can_submit = !server.is_empty() && !user.is_empty() && pass_set;
        let st = &self.caldav;
        let (l_server, l_user, l_pass) =
            (crate::t!(CaldavServerUrl), crate::t!(CaldavUsername), crate::t!(CaldavPassword));
        sec = sec
            .field(l_server.clone(), field(&l_server, text_input(&st.server), cx))
            .field(l_user.clone(), field(&l_user, text_input(&st.user), cx))
            .field(l_pass.clone(), field(&l_pass, text_input(&st.pass).mask_toggle(), cx));
        if server.contains("icloud.com") {
            let link = div()
                .id("caldav-icloud-link")
                .cursor_pointer()
                .text_color(th.colors.primary)
                .hover(|s| s.underline())
                .on_click(|_, _, cx| cx.open_url(APPLE_APP_PASSWORD_URL))
                .child(crate::t!(CaldavIcloudLink));
            sec = sec.field(
                format!("{} {}", crate::t!(CaldavIcloudHint), crate::t!(CaldavIcloudLink)),
                hint(cx).child(crate::t!(CaldavIcloudHint)).child(link),
            );
        }
        let actions = div()
            .flex()
            .flex_wrap()
            .gap(u(8.))
            .child(btn(
                "caldav-test",
                if st.testing { crate::t!(CaldavTesting) } else { crate::t!(CaldavTestConnection) },
                BtnKind::Secondary,
                None,
                !can_submit || st.testing,
                cx,
                cx.listener(|this, _, _, cx| this.test_connection(cx)),
            ))
            .child(btn(
                "caldav-save",
                if st.saving { crate::t!(CaldavSaving) } else { crate::t!(CaldavSaveAccount) },
                BtnKind::Primary,
                None,
                !can_submit || st.saving || st.ctx.is_none(),
                cx,
                cx.listener(|this, _, window, cx| this.save_account(window, cx)),
            ));
        sec = sec.field(format!("{} {}", crate::t!(CaldavTestConnection), crate::t!(CaldavSaveAccount)), actions);
        if let Some((text, ok)) = st.form_status.clone() {
            sec = sec.other(status(text, ok, cx));
        }
        sec
    }

    fn links_section(&mut self, cx: &mut Context<Self>) -> Section {
        let th = AppTheme::get(cx).clone();
        let locale = crate::i18n::locale();
        let st = &self.caldav;
        let mut sec = Section::new(crate::t!(CaldavLinkedFolders), String::new());
        if st.accounts.is_empty() {
            return sec.other(hint_text(crate::t!(CaldavSaveAccountFirst), cx));
        }
        let l_account = crate::t!(CaldavAccount);
        sec = sec.field(
            l_account.clone(),
            field(&l_account, Select::new(&st.account_select).placeholder(l_account.clone()), cx),
        );
        sec = sec.field(
            crate::t!(CaldavDiscoverCalendars),
            div().flex().child(btn(
                "caldav-discover",
                if st.discovering { crate::t!(CaldavDiscovering) } else { crate::t!(CaldavDiscoverCalendars) },
                BtnKind::Secondary,
                Some(AppIcon::RefreshCw),
                st.selected_account.is_none() || st.discovering,
                cx,
                cx.listener(|this, _, window, cx| this.discover_calendars(window, cx)),
            )),
        );
        if !st.calendars.is_empty() {
            let l = crate::t!(CaldavCalendar);
            sec = sec.field(l.clone(), field(&l, Select::new(&st.calendar_select), cx));
        }
        let l_folder = crate::t!(CaldavFolder);
        let folder_text = st.folder.clone().unwrap_or_else(|| crate::t!(CaldavNoFolder));
        let picker = div()
            .flex()
            .items_center()
            .gap(u(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(u(12.))
                    .font_family(th.mono_font.clone())
                    .text_color(th.colors.muted_foreground)
                    .child(folder_text.clone()),
            )
            .child(btn(
                "caldav-choose-folder",
                crate::t!(CaldavChooseFolder),
                BtnKind::Secondary,
                Some(AppIcon::FolderOpen),
                false,
                cx,
                cx.listener(|this, _, _, cx| this.pick_folder(cx)),
            ));
        sec = sec.field(format!("{l_folder} {folder_text}"), field(&l_folder, picker, cx));
        let can_link = st.selected_account.is_some() && st.selected_calendar.is_some() && st.folder.is_some();
        sec = sec.field(
            crate::t!(CaldavLinkFolder),
            div().flex().child(btn(
                "caldav-link",
                crate::t!(CaldavLinkFolder),
                BtnKind::Primary,
                Some(AppIcon::Link),
                !can_link,
                cx,
                cx.listener(|this, _, _, cx| this.link_folder(cx)),
            )),
        );
        if let Some(e) = st.link_error.clone() {
            sec = sec.other(status(e, false, cx));
        }
        if st.links.is_empty() {
            return sec.other(hint_text(crate::t!(CaldavNoLinks), cx));
        }
        for link in st.links.clone() {
            let meta = link_meta(locale, st.statuses.iter().find(|s| s.folder == link.folder));
            let (f1, f2) = (link.folder.clone(), link.folder.clone());
            let row = row_box(cx)
                .text_size(u(12.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(div().font_weight(FontWeight::MEDIUM).truncate().child(basename(&link.folder)))
                        .child(div().text_color(th.colors.muted_foreground).child(link.calendar.clone()))
                        .child(div().text_size(u(11.)).text_color(th.colors.muted_foreground).child(meta)),
                )
                .child(icon_button(
                    gpui_kit::SharedString::from(format!("caldav-sync-{}", link.folder)),
                    AppIcon::RefreshCw,
                    22.,
                    13.,
                    crate::t!(CaldavSyncFolder),
                    false,
                    cx,
                    cx.listener(move |this, _, _, cx| this.sync_now(Some(f1.clone()), cx)),
                ))
                .child(icon_button(
                    gpui_kit::SharedString::from(format!("caldav-unlink-{}", link.folder)),
                    AppIcon::Unlink,
                    22.,
                    13.,
                    crate::t!(CaldavUnlinkFolder),
                    false,
                    cx,
                    cx.listener(move |this, _, _, cx| this.unlink_folder(f2.clone(), cx)),
                ));
            sec = sec.field(format!("{} {}", link.folder, link.calendar), row);
        }
        sec
    }

    fn sync_section(&mut self, cx: &mut Context<Self>) -> Section {
        let th = AppTheme::get(cx).clone();
        let locale = crate::i18n::locale();
        let st = &self.caldav;
        let mut sec = Section::new(crate::t!(CaldavSync), String::new()).field(
            crate::t!(CaldavSyncNow),
            div().flex().child(btn(
                "caldav-sync-now",
                if st.syncing { crate::t!(CaldavSyncing) } else { crate::t!(CaldavSyncNow) },
                BtnKind::Primary,
                Some(AppIcon::RefreshCw),
                st.syncing || st.ctx.is_none(),
                cx,
                cx.listener(|this, _, _, cx| this.sync_now(None, cx)),
            )),
        );
        if let Some(e) = st.sync_error.clone() {
            sec = sec.other(status(e, false, cx));
        }
        if let Some(reports) = st.reports.clone() {
            let log = div().flex().flex_col().gap(u(4.)).children(reports.into_iter().map(|r| {
                row_box(cx)
                    .flex_col()
                    .items_start()
                    .text_size(u(12.))
                    .child(
                        div()
                            .flex()
                            .gap(u(8.))
                            .child(div().font_weight(FontWeight::MEDIUM).child(basename(&r.folder)))
                            .child(div().text_color(th.colors.muted_foreground).child(report_summary(locale, &r))),
                    )
                    .children(r.errors.iter().map(|e| status(e.clone(), false, cx).mt(u(0.))))
            }));
            sec = sec.other(log);
        }
        sec
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_en_and_pt() {
        let r = ReportRow { pushed_new: 1, pushed_updated: 2, pulled_updated: 3, pulled_new: 4, ..Default::default() };
        assert_eq!(report_summary(Locale::En, &r), "+1 new, 2 pushed, 3 pulled, 4 from server");
        let r = ReportRow { errors: vec!["x".into(), "y".into()], ..r };
        assert!(report_summary(Locale::En, &r).ends_with(", 2 error(s)"));
        assert_ne!(report_summary(Locale::PtBr, &r), report_summary(Locale::En, &r));
    }

    #[test]
    fn meta_lines() {
        assert_eq!(link_meta(Locale::En, None), "Never synced");
        let st = StatusRow { folder: "/a".into(), last_synced: None, task_count: 5, pending: 0 };
        assert_eq!(link_meta(Locale::En, Some(&st)), "5 tasks");
        let st = StatusRow { pending: 2, last_synced: Some(Utc::now()), ..st };
        let m = link_meta(Locale::En, Some(&st));
        assert!(m.starts_with("5 tasks, 2 pending delete · synced "), "{m}");
    }

    #[test]
    fn basenames() {
        assert_eq!(basename("/Users/me/notes/"), "notes");
        assert_eq!(basename("/Users/me/notes/work"), "work");
    }
}
