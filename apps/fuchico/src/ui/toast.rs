//! Toast helpers over gpui-component notifications.

use gpui_kit::component::WindowExt;
use gpui_kit::component::notification::Notification;
use gpui_kit::{App, SharedString, Window};

pub fn toast_info(window: &mut Window, cx: &mut App, msg: impl Into<SharedString>) {
    window.push_notification(Notification::info(msg), cx);
}

pub fn toast_success(window: &mut Window, cx: &mut App, msg: impl Into<SharedString>) {
    window.push_notification(Notification::success(msg), cx);
}

pub fn toast_error(window: &mut Window, cx: &mut App, msg: impl Into<SharedString>) {
    window.push_notification(Notification::error(msg), cx);
}
