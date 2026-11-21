//! The tray icon and the background alerts (docs/ALERTS-PLAN.md, slice T1). Thin glue over Tauri: every rule (which toasts, what the
//! tooltip says, what is remembered) lives in `toast_watch`, `tray_text` and `prefs`, which are tested without a window.
//!
//! Closing the window destroys it (the web view's memory is freed) and the program keeps running here; the tray's Open creates it again
//! and Quit ends the program. A watcher thread looks at the findings every `WATCH_SECS` seconds and sends the toasts.

use std::time::Duration;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_notification::NotificationExt;

use crate::commands::SharedBackend;
use crate::health::HealthState;
use crate::prefs::PrefsHandle;
use crate::toast_watch::{FindingSource, Notifier, OpenFinding, TickReport, Toast, Watcher};
use crate::tray_text::{findings_item_label, tooltip};

/// How often the findings are looked at.
const WATCH_SECS: u64 = 30;
/// More open findings than this are not looked at one by one (the count is still exact up to it).
const MAX_OPEN_READ: usize = 500;

const TRAY_ID: &str = "syntrac";
const MAIN_WINDOW: &str = "main";

/// The menu items that change while the program runs.
pub struct TrayItems {
    findings: MenuItem<Wry>,
    notifications: CheckMenuItem<Wry>,
}

/// Shows the main window, creating it again when it was closed.
pub fn open_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    if let Some(config) = app.config().app.windows.first() {
        if let Ok(builder) = tauri::WebviewWindowBuilder::from_config(app, config) {
            if let Ok(window) = builder.build() {
                let _ = window.set_focus();
            }
        }
    }
}

/// Builds the tray icon and its menu. The icon is the window's icon.
pub fn install(app: &AppHandle, prefs: &PrefsHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open SYNTRAC", true, None::<&str>)?;
    let findings = MenuItem::with_id(app, "findings", findings_item_label(None), true, None::<&str>)?;
    let notifications =
        CheckMenuItem::with_id(app, "notifications", "Notifications", true, prefs.notifications_on(), None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &findings, &notifications, &separator, &quit])?;

    let menu_prefs = prefs.clone();
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(tooltip(None, true))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" | "findings" => open_main_window(app),
            "notifications" => {
                menu_prefs.set_notifications_on(!menu_prefs.notifications_on());
                refresh_notifications_item(app, menu_prefs.notifications_on());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                open_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    app.manage(TrayItems { findings, notifications });
    Ok(())
}

/// Makes the menu's check mark say what the settings say (it changes from the sidebar too).
pub fn refresh_notifications_item(app: &AppHandle, on: bool) {
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.notifications.set_checked(on);
    }
}

/// Puts the latest look into the tooltip and the Findings item.
fn apply_report(app: &AppHandle, report: &TickReport) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(tooltip(report.open, report.collector_running)));
    }
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.findings.set_text(findings_item_label(report.open));
    }
}

/// The open findings and the collector's state, as the app already reads them for its screens.
struct BackendSource(SharedBackend);

impl FindingSource for BackendSource {
    fn open_findings(&self) -> Result<Vec<OpenFinding>, String> {
        match self.0.findings(Some("open".to_owned()), Some(MAX_OPEN_READ)) {
            crate::backend::Data::Ok { data } => Ok(data
                .findings
                .into_iter()
                .map(|finding| OpenFinding {
                    id: finding.id,
                    tier: finding.tier.to_owned(),
                    title: finding.title,
                    why: finding.why,
                })
                .collect()),
            crate::backend::Data::NoDatabase { message }
            | crate::backend::Data::SchemaMismatch { message }
            | crate::backend::Data::Error { message } => Err(message),
        }
    }

    fn collector_running(&self) -> bool {
        !matches!(self.0.health().state, HealthState::Offline | HealthState::Unknown)
    }
}

/// Windows notifications through the official plugin (nothing leaves the machine).
struct ToastNotifier(AppHandle);

impl Notifier for ToastNotifier {
    fn show(&self, toast: &Toast) {
        let _ = self.0.notification().builder().title(&toast.title).body(&toast.body).show();
    }
}

/// Starts the thread that looks at the findings and sends the toasts, with or without a window.
pub fn start_watcher(app: AppHandle, backend: SharedBackend, prefs: PrefsHandle) {
    let notifier = ToastNotifier(app.clone());
    let mut watcher = Watcher::new(Box::new(BackendSource(backend)), Box::new(notifier), prefs);
    let _ = std::thread::Builder::new().name("syntrac-alerts".to_owned()).spawn(move || loop {
        let report = watcher.tick();
        apply_report(&app, &report);
        std::thread::sleep(Duration::from_secs(WATCH_SECS));
    });
}
