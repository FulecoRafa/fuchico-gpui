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
        Self {
            data_dir: data_dir.into(),
            sink,
        }
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
    let client = CalDavClient::new(
        account.server_url.clone(),
        account.username.clone(),
        password,
    );
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
    for link in cfg
        .links
        .iter()
        .filter(|l| target.as_deref().is_none_or(|t| t == l.folder_path))
    {
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

        let client = CalDavClient::new(
            account.server_url.clone(),
            account.username.clone(),
            password,
        );
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
    ctx.sink.emit(CoreEvent::CaldavSyncComplete {
        reports: reports.len(),
    });
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
pub fn on_file_written(
    ctx: &CalDavCtx,
    debounce: DebounceState,
    path: String,
    source: Option<String>,
) {
    if source.as_deref() == Some("caldav") {
        return;
    }
    let ctx = ctx.clone();
    crate::runtime::spawn(async move {
        let ctx = &ctx;
        let Ok(cfg) = config::load(&ctx.data_dir) else {
            return;
        };
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
        ctx.sink.emit(CoreEvent::CaldavSyncComplete {
            reports: reports.len(),
        });
    });
}

/// Reconciles every linked folder. Used by the periodic poll and the
/// launch-time sync in `lib.rs`'s `.setup()`.
pub async fn sync_all(ctx: &CalDavCtx) {
    let reports = run_sync(ctx, None).await;
    ctx.sink.emit(CoreEvent::CaldavSyncComplete {
        reports: reports.len(),
    });
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
                    sc.tasks
                        .iter()
                        .filter(|t| t.pending_delete_since.is_some())
                        .count(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caldav::client::tests::{
        CALENDAR_LIST_RESPONSE, HOME_SET_RESPONSE, PRINCIPAL_RESPONSE,
    };
    use crate::caldav::sidecar::{Sidecar, SidecarTask};
    use crossbeam_channel::{Receiver, unbounded};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ctx() -> (tempfile::TempDir, CalDavCtx, Receiver<CoreEvent>) {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = unbounded();
        let ctx = CalDavCtx::new(dir.path().join("appdata"), Arc::new(tx));
        (dir, ctx, rx)
    }

    fn drain(rx: &Receiver<CoreEvent>) -> Vec<CoreEvent> {
        rx.try_iter().collect()
    }

    /// Mock CalDAV server answering the two-hop discovery and the
    /// calendar-list PROPFIND. Local only, never a real network service.
    async fn discovery_server() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("PROPFIND"))
            .and(path("/"))
            .and(header("Depth", "0"))
            .respond_with(ResponseTemplate::new(207).set_body_string(PRINCIPAL_RESPONSE))
            .mount(&server)
            .await;
        Mock::given(method("PROPFIND"))
            .and(path("/1234567/principal/"))
            .respond_with(ResponseTemplate::new(207).set_body_string(HOME_SET_RESPONSE))
            .mount(&server)
            .await;
        Mock::given(method("PROPFIND"))
            .and(path("/1234567/calendars/"))
            .and(header("Depth", "1"))
            .respond_with(ResponseTemplate::new(207).set_body_string(CALENDAR_LIST_RESPONSE))
            .mount(&server)
            .await;
        server
    }

    fn input(server_url: &str) -> AccountInput {
        AccountInput {
            server_url: server_url.into(),
            username: "u@x".into(),
            password: "secret".into(),
        }
    }

    fn task(uid: &str, pending: bool) -> SidecarTask {
        SidecarTask {
            uid: uid.into(),
            file: "Tasks.md".into(),
            line_hint: 1,
            content_hash: String::new(),
            last_text: uid.into(),
            checked: false,
            etag: None,
            last_modified_local: None,
            last_modified_remote: None,
            pending_delete_since: pending.then(chrono::Utc::now),
            pending_delete_streak: u32::from(pending),
        }
    }

    #[tokio::test]
    async fn test_connection_walks_discovery_and_saves_nothing() {
        let server = discovery_server().await;
        let info = caldav_test_connection(server.uri(), "u".into(), "p".into())
            .await
            .unwrap();
        assert_eq!(
            info.calendar_home_url,
            format!("{}/1234567/calendars/", server.uri())
        );
        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            serde_json::json!({"calendarHomeUrl": format!("{}/1234567/calendars/", server.uri())})
        );
    }

    #[tokio::test]
    async fn test_connection_surfaces_auth_failure() {
        let server = MockServer::start().await;
        Mock::given(method("PROPFIND"))
            .respond_with(ResponseTemplate::new(401).set_body_string("nope"))
            .mount(&server)
            .await;
        assert!(
            caldav_test_connection(server.uri(), "u".into(), "bad".into())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn discover_calendars_lists_vtodo_calendars_with_absolute_hrefs() {
        let server = discovery_server().await;
        let cals = caldav_discover_calendars(server.uri(), "u".into(), "p".into())
            .await
            .unwrap();
        assert_eq!(cals.len(), 1);
        assert_eq!(
            cals[0].href,
            format!("{}/1234567/calendars/home/", server.uri())
        );
        assert_eq!(cals[0].display_name, "Reminders");
        assert_eq!(
            serde_json::to_value(&cals[0]).unwrap()["displayName"],
            "Reminders"
        );
    }

    #[tokio::test]
    async fn discover_display_name_falls_back_to_raw_href() {
        let server = MockServer::start().await;
        Mock::given(method("PROPFIND"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(207).set_body_string(PRINCIPAL_RESPONSE))
            .mount(&server)
            .await;
        Mock::given(method("PROPFIND"))
            .and(path("/1234567/principal/"))
            .respond_with(ResponseTemplate::new(207).set_body_string(HOME_SET_RESPONSE))
            .mount(&server)
            .await;
        let list = r#"<?xml version="1.0"?>
<multistatus xmlns="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <response><href>/1234567/calendars/todo/</href><propstat><prop>
    <resourcetype><collection/><C:calendar/></resourcetype>
    <C:supported-calendar-component-set><C:comp name="VTODO"/></C:supported-calendar-component-set>
  </prop><status>HTTP/1.1 200 OK</status></propstat></response>
</multistatus>"#;
        Mock::given(method("PROPFIND"))
            .and(path("/1234567/calendars/"))
            .respond_with(ResponseTemplate::new(207).set_body_string(list))
            .mount(&server)
            .await;
        let cals = caldav_discover_calendars(server.uri(), "u".into(), "p".into())
            .await
            .unwrap();
        assert_eq!(cals.len(), 1);
        assert_eq!(cals[0].display_name, "/1234567/calendars/todo/");
        assert_eq!(
            cals[0].href,
            format!("{}/1234567/calendars/todo/", server.uri())
        );
    }

    #[tokio::test]
    async fn save_list_and_discover_for_saved_account() {
        let server = discovery_server().await;
        let (_d, ctx, _rx) = ctx();

        let err = caldav_discover_calendars_for_account(&ctx, "missing".into())
            .await
            .err()
            .unwrap();
        assert_eq!(err, "no saved account for id missing");

        let id = caldav_save_account(&ctx, input(&server.uri()))
            .await
            .unwrap();
        assert!(uuid::Uuid::parse_str(&id).is_ok());
        assert_eq!(config::load_password(&id).unwrap(), "secret");

        let accounts = caldav_list_accounts(&ctx).unwrap();
        assert_eq!(accounts.len(), 1);
        let json = serde_json::to_value(&accounts[0]).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"id": id, "serverUrl": server.uri(), "username": "u@x"})
        );
        // The password never reaches config.json.
        let raw = std::fs::read_to_string(ctx.data_dir.join("caldav/config.json")).unwrap();
        assert!(!raw.contains("secret"));

        let cals = caldav_discover_calendars_for_account(&ctx, id.clone())
            .await
            .unwrap();
        assert_eq!(cals[0].display_name, "Reminders");

        // Duplicates are allowed (no credential check, no dedup).
        let id2 = caldav_save_account(&ctx, input(&server.uri()))
            .await
            .unwrap();
        assert_ne!(id, id2);
        assert_eq!(caldav_list_accounts(&ctx).unwrap().len(), 2);
    }

    #[tokio::test]
    async fn remove_account_drops_its_links_and_password_but_keeps_sidecars() {
        let (d, ctx, _rx) = ctx();
        let a = caldav_save_account(&ctx, input("https://a")).await.unwrap();
        let b = caldav_save_account(&ctx, input("https://b")).await.unwrap();
        let fa = d.path().join("fa");
        let fb = d.path().join("fb");
        std::fs::create_dir_all(&fa).unwrap();
        std::fs::create_dir_all(&fb).unwrap();
        caldav_link_folder(
            &ctx,
            a.clone(),
            "https://a/cal/".into(),
            "A".into(),
            to_canon(&fa),
        )
        .unwrap();
        caldav_link_folder(
            &ctx,
            b.clone(),
            "https://b/cal/".into(),
            "B".into(),
            to_canon(&fb),
        )
        .unwrap();

        caldav_remove_account(&ctx, a.clone()).unwrap();
        let accounts = caldav_list_accounts(&ctx).unwrap();
        assert_eq!(
            accounts.iter().map(|x| x.id.clone()).collect::<Vec<_>>(),
            vec![b.clone()]
        );
        let links = caldav_list_links(&ctx).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].account_id, b);
        assert!(config::load_password(&a).is_err());
        assert!(fa.join(sidecar::SIDECAR_FILENAME).exists());
        // Removing again (no keyring entry left) still succeeds.
        caldav_remove_account(&ctx, a).unwrap();
    }

    #[test]
    fn link_creates_sidecar_relink_replaces_and_unlink_keeps_sidecar() {
        let (d, ctx, _rx) = ctx();
        let folder = d.path().join("notes");
        std::fs::create_dir_all(&folder).unwrap();
        let fp = to_canon(&folder);

        caldav_link_folder(
            &ctx,
            "acct-1".into(),
            "https://x/cal1/".into(),
            "One".into(),
            fp.clone(),
        )
        .unwrap();
        let sc = sidecar::load(&folder).unwrap().unwrap();
        assert_eq!(
            (sc.account_id.as_str(), sc.calendar_href.as_str()),
            ("acct-1", "https://x/cal1/")
        );
        assert!(sc.tasks.is_empty());

        // Re-link the same folder: the link is replaced, the existing
        // sidecar is left untouched even though it names another calendar.
        caldav_link_folder(
            &ctx,
            "acct-2".into(),
            "https://x/cal2/".into(),
            "Two".into(),
            fp.clone(),
        )
        .unwrap();
        let links = caldav_list_links(&ctx).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(
            serde_json::to_value(&links[0]).unwrap(),
            serde_json::json!({"accountId": "acct-2", "calendarHref": "https://x/cal2/", "calendarDisplayName": "Two", "folderPath": fp})
        );
        let sc = sidecar::load(&folder).unwrap().unwrap();
        assert_eq!(sc.account_id, "acct-1");

        caldav_unlink_folder(&ctx, fp).unwrap();
        assert!(caldav_list_links(&ctx).unwrap().is_empty());
        assert!(folder.join(sidecar::SIDECAR_FILENAME).exists());
    }

    #[test]
    fn sync_status_reads_each_sidecar() {
        let (d, ctx, _rx) = ctx();
        let with = d.path().join("with");
        let without = d.path().join("without");
        std::fs::create_dir_all(&with).unwrap();
        std::fs::create_dir_all(&without).unwrap();
        caldav_link_folder(&ctx, "a".into(), "h".into(), "With".into(), to_canon(&with)).unwrap();
        let mut sc = Sidecar::new("a", "h");
        sc.last_synced_at = Some(chrono::Utc::now());
        sc.tasks = vec![task("1", false), task("2", true), task("3", true)];
        sidecar::save(&with, &sc).unwrap();

        // A link whose sidecar does not exist (written by hand, not via link).
        let mut cfg = config::load(&ctx.data_dir).unwrap();
        cfg.links.push(FolderLink {
            account_id: "a".into(),
            calendar_href: "h".into(),
            calendar_display_name: "Without".into(),
            folder_path: to_canon(&without),
        });
        config::save(&ctx.data_dir, &cfg).unwrap();

        let status = caldav_get_sync_status(&ctx).unwrap();
        assert_eq!(status.len(), 2);
        assert_eq!((status[0].task_count, status[0].pending_deletes), (3, 2));
        assert!(status[0].last_synced_at.is_some());
        let json = serde_json::to_value(&status[1]).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"folderPath": to_canon(&without), "calendarDisplayName": "Without", "lastSyncedAt": null, "taskCount": 0, "pendingDeletes": 0})
        );

        // A corrupt sidecar fails the whole command.
        std::fs::write(with.join(sidecar::SIDECAR_FILENAME), "{not json").unwrap();
        assert!(caldav_get_sync_status(&ctx).is_err());
    }

    const REMOTE_ONLY_REPORT: &str = "<?xml version=\"1.0\"?>\n<multistatus xmlns=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\">\n  <response>\n    <href>/cal/new.ics</href>\n    <propstat>\n      <prop>\n        <getetag>\"r1\"</getetag>\n        <C:calendar-data>BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:remote-uid\r\nSUMMARY:Call the vet\r\nEND:VTODO\r\nEND:VCALENDAR\r\n</C:calendar-data>\n      </prop>\n      <status>HTTP/1.1 200 OK</status>\n    </propstat>\n  </response>\n</multistatus>";

    #[tokio::test]
    async fn sync_now_reconciles_and_emits_written_progress_complete_in_order() {
        let server = MockServer::start().await;
        Mock::given(method("REPORT"))
            .and(path("/cal/"))
            .respond_with(ResponseTemplate::new(207).set_body_string(REMOTE_ONLY_REPORT))
            .mount(&server)
            .await;
        let (d, ctx, rx) = ctx();
        let id = caldav_save_account(&ctx, input(&server.uri()))
            .await
            .unwrap();
        let folder = d.path().join("notes");
        std::fs::create_dir_all(&folder).unwrap();
        let fp = to_canon(&folder);
        caldav_link_folder(
            &ctx,
            id,
            format!("{}/cal/", server.uri()),
            "Reminders".into(),
            fp.clone(),
        )
        .unwrap();

        // A folder that matches no link: nothing synced, still completes.
        let none = caldav_sync_now(&ctx, Some("/nowhere".into()))
            .await
            .unwrap();
        assert!(none.is_empty());
        assert_eq!(
            drain(&rx),
            vec![CoreEvent::CaldavSyncComplete { reports: 0 }]
        );

        let reports = caldav_sync_now(&ctx, Some(fp.clone())).await.unwrap();
        assert_eq!(reports.len(), 1);
        assert!(reports[0].errors.is_empty(), "{:?}", reports[0].errors);
        assert_eq!(reports[0].pulled_new, 1);
        let synced = to_canon(folder.join("Synced Tasks.md"));
        assert_eq!(reports[0].touched_files, vec![synced.clone()]);
        assert_eq!(
            drain(&rx),
            vec![
                CoreEvent::FileWritten {
                    path: synced,
                    source: Some("caldav".into())
                },
                CoreEvent::CaldavSyncProgress {
                    folder_path: fp.clone()
                },
                CoreEvent::CaldavSyncComplete { reports: 1 },
            ]
        );
        let status = caldav_get_sync_status(&ctx).unwrap();
        assert_eq!(status[0].task_count, 1);
        assert!(status[0].last_synced_at.is_some());
    }

    #[tokio::test]
    async fn sync_failures_go_into_reports_not_errors() {
        let (d, ctx, rx) = ctx();
        let folder = d.path().join("orphan");
        std::fs::create_dir_all(&folder).unwrap();
        let fp = to_canon(&folder);
        // Link to an account id that was never saved.
        caldav_link_folder(
            &ctx,
            "ghost".into(),
            "https://x/cal/".into(),
            "X".into(),
            fp.clone(),
        )
        .unwrap();
        // And one whose saved account has no keyring entry.
        let id = caldav_save_account(&ctx, input("https://x")).await.unwrap();
        config::delete_password(&id).unwrap();
        let other = d.path().join("nopw");
        std::fs::create_dir_all(&other).unwrap();
        caldav_link_folder(
            &ctx,
            id,
            "https://x/cal/".into(),
            "Y".into(),
            to_canon(&other),
        )
        .unwrap();

        let reports = caldav_sync_now(&ctx, None).await.unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].folder, fp);
        assert_eq!(
            reports[0].errors,
            vec!["no saved account for id ghost".to_string()]
        );
        assert!(reports[1].errors[0].starts_with("failed to read password from keyring"));
        // Accounts that fail before syncing skip the progress event.
        assert_eq!(
            drain(&rx),
            vec![CoreEvent::CaldavSyncComplete { reports: 2 }]
        );
    }

    #[test]
    fn deepest_linked_folder_wins() {
        let cfg = config::Config {
            accounts: vec![],
            links: ["/v", "/v/work", "/v/work/deep", "/vault2"]
                .iter()
                .map(|f| FolderLink {
                    account_id: "a".into(),
                    calendar_href: "h".into(),
                    calendar_display_name: "n".into(),
                    folder_path: f.to_string(),
                })
                .collect(),
        };
        assert_eq!(
            find_linked_folder(&cfg, "/v/work/deep/x.md").as_deref(),
            Some("/v/work/deep")
        );
        assert_eq!(
            find_linked_folder(&cfg, "/v/work/y.md").as_deref(),
            Some("/v/work")
        );
        assert_eq!(
            find_linked_folder(&cfg, "/v/workshop/z.md").as_deref(),
            Some("/v")
        );
        assert_eq!(find_linked_folder(&cfg, "/v").as_deref(), Some("/v"));
        assert_eq!(find_linked_folder(&cfg, "/elsewhere/a.md"), None);
    }

    /// Real 2 s debounce, run once: two quick writes in a linked subfolder
    /// collapse into one sync of the deepest link; caldav-sourced writes and
    /// writes outside any link never trigger one.
    #[test]
    fn on_file_written_debounces_guards_loops_and_picks_deepest_folder() {
        let (d, ctx, rx) = ctx();
        let outer = d.path().join("v");
        let inner = outer.join("work");
        std::fs::create_dir_all(&inner).unwrap();
        // Unsaved account ids: the sync fails fast offline but still reports.
        caldav_link_folder(
            &ctx,
            "ghost".into(),
            "h".into(),
            "Outer".into(),
            to_canon(&outer),
        )
        .unwrap();
        caldav_link_folder(
            &ctx,
            "ghost".into(),
            "h".into(),
            "Inner".into(),
            to_canon(&inner),
        )
        .unwrap();
        let debounce: DebounceState = Default::default();
        let file = to_canon(inner.join("note.md"));

        on_file_written(
            &ctx,
            debounce.clone(),
            to_canon(outer.join("loop.md")),
            Some("caldav".into()),
        );
        on_file_written(
            &ctx,
            debounce.clone(),
            to_canon(d.path().join("outside.md")),
            Some("editor".into()),
        );
        on_file_written(&ctx, debounce.clone(), file.clone(), Some("editor".into()));
        std::thread::sleep(Duration::from_millis(300));
        on_file_written(&ctx, debounce.clone(), file, None);

        // Nothing before the debounce elapses.
        std::thread::sleep(Duration::from_millis(1200));
        assert!(drain(&rx).is_empty());

        let first = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(first, CoreEvent::CaldavSyncComplete { reports: 1 });
        std::thread::sleep(Duration::from_millis(600));
        assert!(drain(&rx).is_empty(), "the superseded write must not sync");
        assert_eq!(
            debounce.lock().unwrap().get(&to_canon(&inner)).copied(),
            Some(2)
        );
        assert!(!debounce.lock().unwrap().contains_key(&to_canon(&outer)));
    }
}
