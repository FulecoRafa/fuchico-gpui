//! Event abstraction replacing Tauri's `app.emit`. Payloads mirror the
//! original Tauri events (names are kept via [`CoreEvent::name`]).

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum CoreEvent {
    /// `fs:file-written` -- a file was written by the core (editor save,
    /// vault replace, CalDAV sync). `source` is e.g. `"replace"`, `"caldav"`.
    FileWritten {
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<String>,
    },
    /// `fs:changed` -- a directory's contents changed on disk. `from` is the
    /// originating window label in the Tauri app; `None` for watcher events.
    FsChanged { dir: String, from: Option<String> },
    /// `caldav:sync-progress` -- one linked folder finished reconciling.
    CaldavSyncProgress {
        #[serde(rename = "folderPath")]
        folder_path: String,
    },
    /// `caldav:sync-complete` -- a batch of syncs finished.
    CaldavSyncComplete { reports: usize },
    /// `app:open-files` -- the OS handed the app these files.
    OpenFiles(Vec<String>),
}

impl CoreEvent {
    /// The Tauri event name this variant corresponded to.
    pub fn name(&self) -> &'static str {
        match self {
            CoreEvent::FileWritten { .. } => "fs:file-written",
            CoreEvent::FsChanged { .. } => "fs:changed",
            CoreEvent::CaldavSyncProgress { .. } => "caldav:sync-progress",
            CoreEvent::CaldavSyncComplete { .. } => "caldav:sync-complete",
            CoreEvent::OpenFiles(_) => "app:open-files",
        }
    }
}

/// Receives events from core operations. Must not block.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: CoreEvent);
}

impl EventSink for crossbeam_channel::Sender<CoreEvent> {
    fn emit(&self, event: CoreEvent) {
        let _ = self.send(event);
    }
}

impl<F: Fn(CoreEvent) + Send + Sync> EventSink for F {
    fn emit(&self, event: CoreEvent) {
        self(event)
    }
}

/// Discards every event.
pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _event: CoreEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_sink_delivers_and_names_match() {
        let (tx, rx) = crossbeam_channel::unbounded();
        tx.emit(CoreEvent::CaldavSyncComplete { reports: 2 });
        let ev = rx.recv().unwrap();
        assert_eq!(ev.name(), "caldav:sync-complete");
        assert_eq!(serde_json::to_value(&ev).unwrap(), serde_json::json!({"reports": 2}));
    }

    #[test]
    fn file_written_payload_omits_missing_source() {
        let ev = CoreEvent::FileWritten { path: "/a".into(), source: None };
        assert_eq!(serde_json::to_value(&ev).unwrap(), serde_json::json!({"path": "/a"}));
    }
}
