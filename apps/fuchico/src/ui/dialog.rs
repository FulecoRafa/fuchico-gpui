//! Confirm dialog (replaces the original's native `confirm()`).

use std::rc::Rc;

use gpui_kit::component::WindowExt;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::{App, SharedString, Window};

/// OK / Cancel. The original used the OS `confirm()`, whose buttons the OS
/// localizes, so these are not part of the 300-key catalog.
fn native_label(ok: bool) -> &'static str {
    match (crate::i18n::locale(), ok) {
        (_, true) => "OK",
        (crate::i18n::Locale::En, false) => "Cancel",
        (crate::i18n::Locale::PtBr, false) => "Cancelar",
    }
}

pub struct ConfirmOptions {
    pub title: SharedString,
    pub message: Option<SharedString>,
    pub ok_text: SharedString,
    pub danger: bool,
}

impl ConfirmOptions {
    pub fn new(title: impl Into<SharedString>) -> Self {
        ConfirmOptions { title: title.into(), message: None, ok_text: native_label(true).into(), danger: false }
    }

    pub fn message(mut self, m: impl Into<SharedString>) -> Self {
        self.message = Some(m.into());
        self
    }

    pub fn ok_text(mut self, t: impl Into<SharedString>) -> Self {
        self.ok_text = t.into();
        self
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

/// Opens a modal confirm; `on_ok` runs only when confirmed.
pub fn confirm(window: &mut Window, cx: &mut App, opts: ConfirmOptions, on_ok: impl Fn(&mut Window, &mut App) + 'static) {
    let on_ok = Rc::new(on_ok);
    let cancel: SharedString = native_label(false).into();
    window.open_alert_dialog(cx, move |dialog, _, _| {
        let on_ok = on_ok.clone();
        let mut d = dialog
            .title(opts.title.clone())
            .ok_text(opts.ok_text.clone())
            .cancel_text(cancel.clone())
            .show_cancel(true)
            .on_ok(move |_, w, cx| {
                on_ok(w, cx);
                true
            });
        if let Some(m) = &opts.message {
            d = d.description(m.clone());
        }
        if opts.danger {
            d = d.ok_variant(ButtonVariant::Danger);
        }
        d
    });
}
