use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::events::{CoreEvent, EventSink};

use super::client::{CalDavClient, CalDavError};
use super::config::{self, Account, FolderLink};
use super::sidecar;
use super::sync::{self, SyncReport};
use crate::fs::to_canon;

/// Everything the CalDAV commands need that Tauri used to provide through
/// `AppHandle`: where `config.json` lives and where to emit events.
#[derive(Clone)]
pub struct CalDavCtx {
    /// App data dir (see [`crate::paths::app_data_dir`]); config lives in
    /// `<data_dir>/caldav/config.json`.
    pub data_dir: PathBuf,
    pub sink: Arc<dyn EventSink>,
}

impl CalDavCtx {
    pub fn new(data_dir: impl Into<PathBuf>, sink: Arc<dyn EventSink>) -> Self {
        Self { data_dir: data_dir.into(), sink }
    }
}

impl From<CalDavError> for String {
    fn from(e: CalDavError) -> Self {
        e.to_string()
    }
}

#[derive(Serialize)]
pub struct AccountInfo {
    #[serde(rename = "calendarHomeUrl")]
    pub calendar_home_url: String,
}

#[derive(Serialize)]
pub struct CalendarInfo {
    pub href: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
}

/// Verifies credentials against a CalDAV server by walking the discovery
/// chain (principal -> calendar-home-set) without touching any calendar
/// data. Used by the account form's "Test Connection" action before saving.
pub async fn caldav_test_connection(
    server_url: String,
    username: String,
    password: String,
) -> Result<AccountInfo, String> {
    let client = CalDavClient::new(server_url, username, password);
    let calendar_home_url = client.discover_calendar_home().await?;
    Ok(AccountInfo { calendar_home_url })
}

/// Lists task-capable (VTODO-supporting) calendars under the account's
/// calendar-home-set -- v1 syncs checkboxes only, so calendars without
/// VTODO support (plain event calendars) are filtered out.
pub async fn caldav_discover_calendars(
    server_url: String,
    username: String,
    password: String,
) -> Result<Vec<CalendarInfo>, String> {
    let client = CalDavClient::new(server_url, username, password);
    let home_url = client.discover_calendar_home().await?;
    let calendars = client.list_task_calendars(&home_url).await?;
    Ok(calendars
        .into_iter()
        .map(|c| CalendarInfo {
            display_name: c.displayname.clone().unwrap_or_else(|| c.href.clone()),
            href: client.resolve(&c.href),
        })
        .collect())
}

/// Same as [`caldav_discover_calendars`], but for an already-saved account
/// -- loads its password from the keyring instead of taking one over IPC.
/// Used when linking a folder, so the folder-link flow never needs the
/// password typed in again after the account was saved.
pub async fn caldav_discover_calendars_for_account(
    ctx: &CalDavCtx,
    account_id: String,
) -> Result<Vec<CalendarInfo>, String> {
    let cfg = config::load(&ctx.data_dir)?;
    let account = cfg
        .accounts
        .iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| format!("no saved account for id {account_id}"))?;
    let password = config::load_password(&account_id)?;
    let client = CalDavClient::new(account.server_url.clone(), account.username.clone(), password);
    let home_url = client.discover_calendar_home().await?;
    let calendars = client.list_task_calendars(&home_url).await?;
    Ok(calendars
        .into_iter()
        .map(|c| CalendarInfo {
            display_name: c.displayname.clone().unwrap_or_else(|| c.href.clone()),
            href: client.resolve(&c.href),
        })
        .collect())
}

#[derive(Deserialize)]
pub struct AccountInput {
    #[serde(rename = "serverUrl")]
    pub server_url: String,
    pub username: String,
    pub password: String,
}

/// Saves a new account: the non-secret half (`serverUrl`/`username`) goes to
/// `config.json`, the password goes to the OS keyring keyed by the freshly
/// generated `id`. Callers should already have called
/// [`caldav_test_connection`] first -- this command does not itself verify
/// the credentials.
pub async fn caldav_save_account(ctx: &CalDavCtx, input: AccountInput) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    config::save_password(&id, &input.password)?;

    let mut cfg = config::load(&ctx.data_dir)?;
    cfg.accounts.push(Account {
        id: id.clone(),
        server_url: input.server_url,
        username: input.username,
    });
    config::save(&ctx.data_dir, &cfg)?;
    Ok(id)
}

/// Never includes passwords -- `config::Account` has no password field by
/// design, see `config.rs`.
pub fn caldav_list_accounts(ctx: &CalDavCtx) -> Result<Vec<Account>, String> {
    Ok(config::load(&ctx.data_dir)?.accounts)
}

/// Removes the account, its stored password, and every folder link that
/// pointed at it. Linked folders keep their `.fuchico-sync.json` sidecar on
/// disk (harmless once the link is gone -- nothing scans it anymore) rather
/// than deleting user-adjacent files as a side effect of an account removal.
pub fn caldav_remove_account(ctx: &CalDavCtx, account_id: String) -> Result<(), String> {
    let mut cfg = config::load(&ctx.data_dir)?;
    cfg.accounts.retain(|a| a.id != account_id);
    cfg.links.retain(|l| l.account_id != account_id);
    config::save(&ctx.data_dir, &cfg)?;
    config::delete_password(&account_id)
}

/// Links a folder to a calendar, creating the folder's sidecar if it
/// doesn't exist yet. Re-linking an already-linked folder replaces its
/// existing link entry rather than duplicating it.
pub fn caldav_link_folder(
    ctx: &CalDavCtx,
    account_id: String,
    calendar_href: String,
    calendar_display_name: String,
    folder_path: String,
) -> Result<(), String> {
    let folder_path = to_canon(&folder_path);

    let mut cfg = config::load(&ctx.data_dir)?;
    cfg.links.retain(|l| l.folder_path != folder_path);
    cfg.links.push(FolderLink {
        account_id: account_id.clone(),
        calendar_href: calendar_href.clone(),
        calendar_display_name,
        folder_path: folder_path.clone(),
    });
    config::save(&ctx.data_dir, &cfg)?;

    let folder = std::path::PathBuf::from(&folder_path);
    if sidecar::load(&folder)?.is_none() {
        sidecar::save(&folder, &sidecar::Sidecar::new(account_id, calendar_href))?;
    }
    Ok(())
}

/// Drops the config-level link only. The sidecar file is left in place --
/// unlinking is reversible (re-link the same folder and syncing picks up
/// exactly where the sidecar left off) rather than a destructive reset.
pub fn caldav_unlink_folder(ctx: &CalDavCtx, folder_path: String) -> Result<(), String> {
    let folder_path = to_canon(&folder_path);
    let mut cfg = config::load(&ctx.data_dir)?;
    cfg.links.retain(|l| l.folder_path != folder_path);
    config::save(&ctx.data_dir, &cfg)
}

pub fn caldav_list_links(ctx: &CalDavCtx) -> Result<Vec<FolderLink>, String> {
    Ok(config::load(&ctx.data_dir)?.links)
}

/// Shared body behind [`caldav_sync_now`] and the background triggers
/// (on-save debounce, periodic poll, launch sync) -- everything reconciles
/// through this single path so events fire identically regardless of what
/// kicked the sync off. Doesn't emit `caldav:sync-complete` itself, since
/// callers batch that around possibly-multiple invocations.
async fn run_sync(ctx: &CalDavCtx, target: Option<String>) -> Vec<SyncReport> {
    let cfg = match config::load(&ctx.data_dir) {
        Ok(c) => c,
        Err(e) => {
            return vec![SyncReport {
                folder: target.unwrap_or_default(),
                errors: vec![e],
                ..Default::default()
            }];
        }
    };

    let mut reports = Vec::new();
    for link in cfg.links.iter().filter(|l| target.as_deref().is_none_or(|t| t == l.folder_path)) {
        let Some(account) = cfg.accounts.iter().find(|a| a.id == link.account_id) else {
            reports.push(SyncReport {
                folder: link.folder_path.clone(),
                errors: vec![format!("no saved account for id {}", link.account_id)],
                ..Default::default()
            });
            continue;
        };
        let password = match config::load_password(&account.id) {
            Ok(p) => p,
            Err(e) => {
                reports.push(SyncReport {
                    folder: link.folder_path.clone(),
                    errors: vec![e],
                    ..Default::default()
                });
                continue;
            }
        };

        let client = CalDavClient::new(account.server_url.clone(), account.username.clone(), password);
        let folder = std::path::PathBuf::from(&link.folder_path);
        let report = match sync::sync_folder(
            &client,
            &folder,
            &link.account_id,
            &link.calendar_href,
            &link.calendar_display_name,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => SyncReport {
                folder: link.folder_path.clone(),
                errors: vec![e],
                ..Default::default()
            },
        };

        for path in &report.touched_files {
            ctx.sink.emit(CoreEvent::FileWritten {
                path: path.clone(),
                source: Some("caldav".to_string()),
            });
        }
        ctx.sink.emit(CoreEvent::CaldavSyncProgress {
            folder_path: link.folder_path.clone(),
        });
        reports.push(report);
    }
    reports
}

/// Runs one reconciliation pass over every linked folder, or just
/// `folder_path` if given. Each folder's touched files get an
/// `fs:file-written` emission (so `useAgenda.ts`'s existing listener
/// refreshes for free, same as a manual edit would) plus a
/// `caldav:sync-progress` event; `caldav:sync-complete` fires once at the
/// end with the full batch of reports.
pub async fn caldav_sync_now(
    ctx: &CalDavCtx,
    folder_path: Option<String>,
) -> Result<Vec<SyncReport>, String> {
    let target = folder_path.map(to_canon);
    let reports = run_sync(ctx, target).await;
    ctx.sink.emit(CoreEvent::CaldavSyncComplete { reports: reports.len() });
    Ok(reports)
}

/// Shared debounce map for the on-save trigger: folder path -> generation
/// counter. A file-write bumps its linked folder's generation and schedules
/// a sync after [`ON_SAVE_DEBOUNCE`]; if another write bumps the same
/// folder before the delay elapses, the stale scheduled sync sees its
/// generation is no longer current and skips running.
pub type DebounceState = Arc<Mutex<HashMap<String, u64>>>;

const ON_SAVE_DEBOUNCE: Duration = Duration::from_secs(2);

/// Deepest (longest) linked-folder path that is an ancestor of `file_path`,
/// matching the nested-folder-link rule used elsewhere (most specific link
/// wins). Returns `None` for files outside any linked folder.
fn find_linked_folder(cfg: &config::Config, file_path: &str) -> Option<String> {
    let file_path = to_canon(file_path);
    cfg.links
        .iter()
        .map(|l| l.folder_path.clone())
        .filter(|f| file_path == *f || file_path.starts_with(&format!("{f}/")))
        .max_by_key(|f| f.len())
}

/// Call this for every [`CoreEvent::FileWritten`] the app observes (was the
/// `fs:file-written` listener in `lib.rs`'s `.setup()`). Ignores writes that came from a sync itself (`source ==
/// "caldav"`) to avoid a sync-triggers-sync loop, and writes outside any
/// linked folder.
pub fn on_file_written(ctx: &CalDavCtx, debounce: DebounceState, path: String, source: Option<String>) {
    if source.as_deref() == Some("caldav") {
        return;
    }
    let ctx = ctx.clone();
    crate::runtime::spawn(async move {
        let ctx = &ctx;
        let Ok(cfg) = config::load(&ctx.data_dir) else { return };
        let Some(folder) = find_linked_folder(&cfg, &path) else {
            return;
        };
        let generation = {
            let mut map = debounce.lock().unwrap();
            let g = map.entry(folder.clone()).or_insert(0);
            *g += 1;
            *g
        };

        tokio::time::sleep(ON_SAVE_DEBOUNCE).await;

        let still_current = {
            let map = debounce.lock().unwrap();
            map.get(&folder).copied() == Some(generation)
        };
        if !still_current {
            return;
        }

        let reports = run_sync(ctx, Some(folder)).await;
        ctx.sink.emit(CoreEvent::CaldavSyncComplete { reports: reports.len() });
    });
}

/// Reconciles every linked folder. Used by the periodic poll and the
/// launch-time sync in `lib.rs`'s `.setup()`.
pub async fn sync_all(ctx: &CalDavCtx) {
    let reports = run_sync(ctx, None).await;
    ctx.sink.emit(CoreEvent::CaldavSyncComplete { reports: reports.len() });
}

#[derive(Serialize)]
pub struct FolderSyncStatus {
    #[serde(rename = "folderPath")]
    pub folder_path: String,
    #[serde(rename = "calendarDisplayName")]
    pub calendar_display_name: String,
    #[serde(rename = "lastSyncedAt")]
    pub last_synced_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(rename = "taskCount")]
    pub task_count: usize,
    #[serde(rename = "pendingDeletes")]
    pub pending_deletes: usize,
}

/// Status is read straight off each linked folder's sidecar (the source of
/// truth for "how did the last sync go"), not from any separate log --
/// there is no in-memory sync history to fall back on between app runs.
pub fn caldav_get_sync_status(ctx: &CalDavCtx) -> Result<Vec<FolderSyncStatus>, String> {
    let cfg = config::load(&ctx.data_dir)?;
    cfg.links
        .iter()
        .map(|link| {
            let folder = std::path::PathBuf::from(&link.folder_path);
            let sc = sidecar::load(&folder)?;
            let (last_synced_at, task_count, pending_deletes) = match &sc {
                Some(sc) => (
                    sc.last_synced_at,
                    sc.tasks.len(),
                    sc.tasks.iter().filter(|t| t.pending_delete_since.is_some()).count(),
                ),
                None => (None, 0, 0),
            };
            Ok(FolderSyncStatus {
                folder_path: link.folder_path.clone(),
                calendar_display_name: link.calendar_display_name.clone(),
                last_synced_at,
                task_count,
                pending_deletes,
            })
        })
        .collect()
}

/// How often the background poll reconciles every linked folder -- catches
/// changes made only on a phone/other device, since CalDAV has no push
/// notification mechanism available here.
pub const SYNC_POLL_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// Launch-time pass followed by the periodic poll (was the task spawned in
/// the Tauri `.setup()`). Runs on the shared runtime; abort the returned
/// handle to stop it.
pub fn start_background_sync(ctx: CalDavCtx) -> tokio::task::JoinHandle<()> {
    crate::runtime::spawn(async move {
        sync_all(&ctx).await;
        let mut interval = tokio::time::interval(SYNC_POLL_INTERVAL);
        interval.tick().await; // first tick fires immediately; already synced above
        loop {
            interval.tick().await;
            sync_all(&ctx).await;
        }
    })
}
