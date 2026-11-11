//! Icon in the system tray (StatusNotifierItem, shown by KDE, GNOME with the AppIndicator
//! extension, XFCE and others), like the original's menu-bar icon: closing the window keeps
//! Drive running there.
use ksni::blocking::TrayMethods;
use std::sync::mpsc::Sender;

#[derive(Debug, Clone, PartialEq)]
pub enum TrayMsg {
    Open,
    Preferences,
    TogglePause,
    Quit,
    /// A file manager menu entry: (action, path).
    Action(String, String),
}

pub struct DriveTray {
    tx: Sender<TrayMsg>,
    ctx: eframe::egui::Context,
    pub paused: bool,
    pub status: String,
}

impl DriveTray {
    fn send(&self, m: TrayMsg) {
        let _ = self.tx.send(m);
        self.ctx.request_repaint();
    }
}

impl ksni::Tray for DriveTray {
    fn id(&self) -> String {
        "drive-linux".into()
    }
    fn title(&self) -> String {
        crate::APP_NAME.into()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let icon = crate::theme::app_icon();
        // RGBA → ARGB (network byte order)
        let mut data = Vec::with_capacity(icon.rgba.len());
        for px in icon.rgba.chunks(4) {
            data.extend_from_slice(&[px[3], px[0], px[1], px[2]]);
        }
        vec![ksni::Icon { width: icon.width as i32, height: icon.height as i32, data }]
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: crate::APP_NAME.into(), description: self.status.clone(), ..Default::default() }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayMsg::Open);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let t = crate::i18n::tr;
        vec![
            StandardItem { label: t("Open Google Drive").into(), activate: Box::new(|s: &mut Self| s.send(TrayMsg::Open)), ..Default::default() }.into(),
            StandardItem { label: if self.paused { t("Resume syncing") } else { t("Pause syncing") }.into(), activate: Box::new(|s: &mut Self| s.send(TrayMsg::TogglePause)), ..Default::default() }
                .into(),
            StandardItem { label: t("Preferences").into(), activate: Box::new(|s: &mut Self| s.send(TrayMsg::Preferences)), ..Default::default() }.into(),
            MenuItem::Separator,
            StandardItem { label: t("Quit Google Drive").into(), activate: Box::new(|s: &mut Self| s.send(TrayMsg::Quit)), ..Default::default() }.into(),
        ]
    }
}

/// Starts the tray icon; None when the desktop has no tray to show it in.
pub fn start(tx: Sender<TrayMsg>, ctx: eframe::egui::Context, paused: bool) -> Option<ksni::blocking::Handle<DriveTray>> {
    DriveTray { tx, ctx, paused, status: String::new() }.spawn().ok()
}
