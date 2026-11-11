use crate::engine::{ActState, Activity, Cmd, Engine, PROXY_DIRECT};
use crate::i18n::{is_vi, set_vietnamese, tr};
use crate::icons::{self, Icon};
use crate::settings::*;
use crate::theme::{self, Palette, DARK, LIGHT};
use eframe::egui::{self, vec2, Align, Color32, Layout, RichText, Sense, Stroke};
use std::path::PathBuf;
use std::time::SystemTime;

// Layout of the original home window (home_dialog.css): 272 px navigation, 64 px top bar.
const NAV_WIDTH: f32 = 272.0;
const TOP_BAR: f32 = 64.0;

#[derive(PartialEq, Clone, Copy, Debug)]
enum View {
    Home,
    Activity,
    Notifications,
    Folders,
    Offline,
    Shared,
    Settings,
}

/// "Generate diagnostic info": None idle, Some(None) generating, Some(Some(result)) finished.
type DiagState = Option<Option<Result<PathBuf, String>>>;

pub struct DriveApp {
    engine: Engine,
    view: View,
    pal: Palette,
    applied_dark: Option<bool>,
    screenshot: Option<PathBuf>,
    frames: u32,
    shot_requested: bool,
    search: String,
    down_text: String,
    up_text: String,
    cache_text: String,
    needs_restart: bool,
    confirm_disconnect: Option<String>,
    show_about: bool,
    confirm_quit: bool,
    /// "Push changes and quit": quit as soon as nothing is left to upload
    /// (RequestPushChangesAndQuit 0x1003070c0 / MaybePerformSafeQuit 0x100312a10).
    quit_after_sync: Option<std::time::Instant>,
    /// A link found for "Copy link to clipboard", waiting to be put on the clipboard.
    link_to_copy: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    tray: Option<ksni::blocking::Handle<crate::tray::DriveTray>>,
    tray_rx: Option<std::sync::mpsc::Receiver<crate::tray::TrayMsg>>,
    tray_paused: Option<bool>,
    allow_close: bool,
    hidden: bool,
    remember_device: bool,
    /// Account whose "Ransomware detected" dialog is open ("Take action").
    ransom_dialog: Option<String>,
    /// Account whose "I recognize it" confirmation is open.
    ransom_allow: Option<String>,
    /// Restarting into a newer copy of the app: when the restart began.
    restart_at: Option<std::time::Instant>,
    last_update_check: std::time::Instant,
    /// "Generate diagnostic info": None idle, Some(None) generating, Some(Some(result)) finished.
    diag: std::sync::Arc<std::sync::Mutex<DiagState>>,
    /// Profile pictures by account, reloaded when the file changes.
    avatars: std::collections::HashMap<String, (Option<std::time::SystemTime>, Option<egui::TextureHandle>)>,
}

struct Snap {
    account: Option<String>,
    photos: Option<String>,
    rclone_ok: Option<bool>,
    checked: bool,
    connecting: bool,
    connecting_photos: bool,
    job: Option<String>,
    pending: u64,
    transfers: usize,
    quota: Option<(u64, u64)>,
    mounted: bool,
    unread: usize,
    /// The selected account's syncing is held by a ransomware alert.
    ransomware: bool,
}

impl DriveApp {
    pub fn new(cc: &eframe::CreationContext<'_>, screenshot: Option<PathBuf>) -> Self {
        CLASS_SET.store(set_window_class(cc), std::sync::atomic::Ordering::Relaxed);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        theme::setup_fonts(&cc.egui_ctx);
        let st = Settings::load();
        set_vietnamese(st.language == "vi");
        PROXY_DIRECT.store(st.proxy_direct, std::sync::atomic::Ordering::Relaxed);
        let (d, u, c) = (st.download_kbps.to_string(), st.upload_kbps.to_string(), st.cache_limit_gb.to_string());
        if screenshot.is_none() {
            install_launcher();
            if st.autostart {
                set_autostart(true);
            }
        }
        let pal = if st.dark { DARK } else { LIGHT };
        let paused = st.paused;
        let engine = Engine::new(cc.egui_ctx.clone(), st, screenshot.is_none());
        if screenshot.is_none() {
            engine.quit_on_system_signals();
            // Sync status badges for Dolphin and other file managers.
            crate::badges::serve(engine.shared.clone(), engine.settings.clone());
        }
        // Tray icon like the original's menu-bar icon (not in screenshot mode).
        let (tray, tray_rx) = if screenshot.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            // A second start of the app asks this one to show its window (single instance).
            if let Some(listener) = crate::INSTANCE.lock().unwrap().take() {
                let tx2 = tx.clone();
                let ctx = cc.egui_ctx.clone();
                std::thread::spawn(move || {
                    use std::io::BufRead;
                    for conn in listener.incoming().flatten() {
                        let _ = conn.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                        let mut line = String::new();
                        let _ = std::io::BufReader::new(conn).read_line(&mut line);
                        let line = line.trim_end_matches('\n');
                        let msg = match line.strip_prefix("action ").and_then(|r| r.split_once('\t')) {
                            Some((name, path)) => crate::tray::TrayMsg::Action(name.to_string(), path.to_string()),
                            None => crate::tray::TrayMsg::Open,
                        };
                        let _ = tx2.send(msg);
                        ctx.request_repaint();
                    }
                });
                for (name, path) in crate::PENDING_ACTIONS.lock().unwrap().drain(..) {
                    let _ = tx.send(crate::tray::TrayMsg::Action(name, path));
                }
            }
            (crate::tray::start(tx, cc.egui_ctx.clone(), paused), Some(rx))
        } else {
            (None, None)
        };
        let background = std::env::args().any(|a| a == "--background");
        if screenshot.is_none() {
            let st = engine.settings.lock().unwrap().clone();
            let new_plugin = crate::filemenu::install(st.file_manager_menu, st.language == "vi");
            // Only once, when the plugin is first put in place (not on every start).
            if new_plugin && crate::badges::dolphin_needs_relogin() {
                let vi = st.language == "vi";
                engine.shared.lock().unwrap().notify(
                    if vi { "Trạng thái đồng bộ trong Dolphin" } else { "Sync status in Dolphin" },
                    if vi {
                        "Hãy đăng xuất rồi đăng nhập lại để Dolphin hiện biểu tượng trạng thái đồng bộ trên tệp."
                    } else {
                        "Sign out of your computer and back in for Dolphin to show sync status on files."
                    },
                );
            }
        }
        if background && tray.is_some() {
            cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        Self {
            engine,
            // `--view <page>` opens on another page (handy with --screenshot).
            view: start_view(),
            pal,
            applied_dark: None,
            screenshot,
            frames: 0,
            shot_requested: false,
            search: String::new(),
            down_text: d,
            up_text: u,
            cache_text: c,
            needs_restart: false,
            confirm_disconnect: None,
            show_about: false,
            confirm_quit: false,
            quit_after_sync: None,
            link_to_copy: Default::default(),
            tray,
            tray_rx,
            tray_paused: None,
            allow_close: false,
            hidden: background,
            remember_device: false,
            ransom_dialog: None,
            ransom_allow: None,
            restart_at: None,
            last_update_check: std::time::Instant::now(),
            diag: Default::default(),
            avatars: Default::default(),
        }
    }

    fn settings(&self) -> Settings {
        self.engine.settings.lock().unwrap().clone()
    }

    fn update_settings(&mut self, f: impl FnOnce(&mut Settings)) {
        let mut s = self.engine.settings.lock().unwrap();
        f(&mut s);
        s.save();
    }

    fn snap(&self) -> Snap {
        let sh = self.engine.shared.lock().unwrap();
        Snap {
            account: sh.account.clone(),
            photos: sh.photos_account.clone(),
            rclone_ok: sh.rclone_ok,
            checked: sh.checked_account,
            connecting: sh.connecting,
            connecting_photos: sh.connecting_photos,
            job: sh.job.clone(),
            pending: sh.pending,
            transfers: sh.transfers.len(),
            quota: sh.quota,
            mounted: sh.mounted,
            unread: sh.notices.iter().filter(|n| !n.read).count(),
            ransomware: sh.account.as_ref().map(|a| sh.ransomware.contains_key(a)).unwrap_or(false),
        }
    }

    fn quit(&mut self, ctx: &egui::Context) {
        // Like the original: ask first when files are still syncing ("Quit Google Drive?").
        let busy = {
            let sh = self.engine.shared.lock().unwrap();
            sh.job.is_some() || !sh.transfers.is_empty() || sh.pending > 0
        };
        if busy && !self.confirm_quit {
            self.confirm_quit = true;
            self.hidden = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            return;
        }
        self.confirm_quit = false;
        self.allow_close = true;
        self.engine.send(Cmd::Quit);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn go(&mut self, v: View) {
        self.view = v;
        match v {
            View::Shared => self.engine.load_shared(),
            View::Offline => self.engine.load_offline(),
            _ => {}
        }
    }

    // ------------------------------------------------------------------ navigation
    fn nav(&mut self, ui: &mut egui::Ui, s: &Snap) {
        let p = self.pal;
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            ui.add(theme::logo(40.0));
            ui.label(RichText::new("Drive").size(22.0).color(p.on_variant));
            ui.label(RichText::new("For Linux").size(14.0).color(p.on_variant));
        });
        ui.add_space(14.0);
        let items = [
            (View::Home, Icon::Home, tr("Home"), 0),
            (View::Activity, Icon::Sync, tr("Sync activity"), 0),
            (View::Notifications, Icon::Bell, tr("Notifications"), s.unread),
            (View::Folders, Icon::Folder, tr("Connected folders"), 0),
            (View::Offline, Icon::Offline, tr("Offline files"), 0),
            (View::Shared, Icon::People, tr("Shared with me"), 0),
        ];
        for (v, icon, label, badge) in items {
            if nav_item(ui, &p, self.view == v, icon, label, badge) {
                self.go(v);
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(12.0);
            if nav_item(ui, &p, self.view == View::Settings, Icon::Settings, tr("Settings"), 0) {
                self.go(View::Settings);
            }
        });
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, s: &Snap) {
        let p = self.pal;
        let ctx = ui.ctx().clone();
        let st = self.settings();
        let mut search_rect = None;
        ui.horizontal_centered(|ui| {
            // search box (48 px pill, like the original's "Search in Drive")
            let w = (ui.available_width() - 190.0).clamp(240.0, 720.0);
            let (rect, _) = ui.allocate_exact_size(vec2(w, 48.0), Sense::hover());
            ui.painter().rect_filled(rect, 24.0, p.container_high);
            icons::image(Icon::Search, 22.0, p.on_variant).paint_at(ui, egui::Rect::from_center_size(rect.left_center() + vec2(26.0, 0.0), vec2(22.0, 22.0)));
            let inner = egui::Rect::from_min_max(rect.min + vec2(52.0, 12.0), rect.max - vec2(16.0, 12.0));
            let resp =
                ui.put(inner, egui::TextEdit::singleline(&mut self.search).hint_text(tr("Search in Drive")).frame(egui::Frame::NONE).text_color(p.on_surface).font(egui::FontId::proportional(16.0)));
            search_rect = Some(rect);
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.search.clear();
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(14.0);
                // account: profile picture opening the account card (switcher for up to 4 accounts)
                let (r, resp) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
                if resp.hovered() {
                    ui.painter().circle_filled(r.center(), 20.0, p.container_high);
                }
                match s.account.clone() {
                    Some(a) => self.paint_avatar(ui, &a, &st, r.center(), 16.0),
                    None => {
                        icons::image(Icon::People, 22.0, p.on_variant).paint_at(ui, egui::Rect::from_center_size(r.center(), vec2(22.0, 22.0)));
                    }
                }
                let tip = s.account.as_ref().map(|a| format!("{}\n{}", st.names.get(a).cloned().unwrap_or_else(|| tr("Google Account").into()), st.account_label(a)));
                let resp = resp.on_hover_text(tip.unwrap_or_else(|| tr("Google Account").into()));
                // DRIVE_LINUX_SHOW_ACCOUNT opens the card for a preview picture.
                let force = self.screenshot.is_some() && std::env::var_os("DRIVE_LINUX_SHOW_ACCOUNT").is_some();
                let popup = egui::Popup::menu(&resp);
                let popup = if force { popup.open(true) } else { popup };
                popup
                    .width(340.0)
                    .gap(6.0)
                    .frame(egui::Frame::new().fill(p.container).corner_radius(24.0).inner_margin(10).stroke(Stroke::new(1.0, p.outline)))
                    .show(|ui| self.account_card(ui, s, &st));
                // settings menu (gear), like the original's menu
                let resp = icons::button(ui, Icon::Settings, p.on_variant, p.container_high, tr("Settings"));
                egui::Popup::menu(&resp).show(|ui| {
                    ui.set_min_width(220.0);
                    let paused = self.settings().paused;
                    if ui.button(if paused { tr("Resume syncing") } else { tr("Pause syncing") }).clicked() {
                        self.engine.set_paused(!paused);
                    }
                    if ui.button(tr("Preferences")).clicked() {
                        self.go(View::Settings);
                    }
                    if ui.button(tr("Error list")).clicked() {
                        self.go(View::Notifications);
                    }
                    if ui.button(tr("About")).clicked() {
                        self.show_about = true;
                    }
                    ui.separator();
                    if ui.button(tr("Quit")).clicked() {
                        self.quit(&ctx);
                    }
                });
                let resp = icons::button(ui, Icon::Help, p.on_variant, p.container_high, tr("Help"));
                egui::Popup::menu(&resp).show(|ui| {
                    ui.set_min_width(200.0);
                    if ui.button(tr("Help")).clicked() {
                        open("https://support.google.com/drive/answer/10838124");
                    }
                    if ui.button(tr("Send feedback")).clicked() {
                        open("https://support.google.com/drive/");
                    }
                    ui.separator();
                    if ui.button(tr("Generate diagnostic info")).clicked() {
                        self.generate_diagnostics(&ctx);
                    }
                    let verbose = st.verbose_logging;
                    if ui.button(if verbose { tr("Disable verbose logging") } else { tr("Enable verbose logging") }).clicked() {
                        self.update_settings(|s| s.verbose_logging = !verbose);
                        crate::diag::VERBOSE.store(!verbose, std::sync::atomic::Ordering::Relaxed);
                        // Running rclone jobs pick up the new level when they restart.
                        self.engine.send(Cmd::Reconfigure);
                    }
                });
            });
        });
        if let Some(rect) = search_rect {
            self.search_results(&ctx, rect, &st);
        }
    }

    /// Loads (or reloads) the account's profile picture as a texture.
    fn avatar_texture(&mut self, ctx: &egui::Context, acc: &str) -> Option<egui::TextureHandle> {
        let path = crate::engine::avatar_path(acc);
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        let entry = self.avatars.entry(acc.to_string()).or_insert((None, None));
        if entry.0 != mtime || (mtime.is_some() && entry.1.is_none()) {
            entry.0 = mtime;
            entry.1 = image::open(&path).ok().map(|img| {
                let mut rgba = img.to_rgba8();
                // Cut the picture to a circle with a soft edge.
                let (w, h) = (rgba.width() as f32, rgba.height() as f32);
                let (cx, cy, rad) = (w / 2.0, h / 2.0, w.min(h) / 2.0);
                for (x, y, px) in rgba.enumerate_pixels_mut() {
                    let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                    let a = (rad - d + 0.5).clamp(0.0, 1.0);
                    px[3] = (px[3] as f32 * a) as u8;
                }
                let size = [rgba.width() as usize, rgba.height() as usize];
                let ci = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                ctx.load_texture(format!("avatar-{acc}"), ci, egui::TextureOptions::LINEAR)
            });
        }
        entry.1.clone()
    }

    /// A round profile picture, or the first letter on a coloured circle when there is none.
    fn paint_avatar(&mut self, ui: &mut egui::Ui, acc: &str, st: &Settings, center: egui::Pos2, radius: f32) {
        let rect = egui::Rect::from_center_size(center, vec2(radius * 2.0, radius * 2.0));
        if let Some(tex) = self.avatar_texture(ui.ctx(), acc) {
            ui.painter().circle_filled(center, radius, Color32::WHITE);
            egui::Image::from_texture(&tex).fit_to_exact_size(rect.size()).paint_at(ui, rect);
            ui.painter().circle_stroke(center, radius + 0.5, Stroke::new(1.0, self.pal.outline));
        } else {
            let label = st.names.get(acc).cloned().unwrap_or_else(|| st.account_label(acc));
            let letter = label.chars().next().unwrap_or('?').to_uppercase().to_string();
            ui.painter().circle_filled(center, radius, account_color(&st.account_label(acc)));
            ui.painter().text(center, egui::Align2::CENTER_CENTER, letter, egui::FontId::proportional(radius * 1.05), Color32::WHITE);
        }
    }

    /// The account card: like Google's account menu — the current account with its picture,
    /// name, e-mail and storage, then the other accounts and the account actions.
    fn account_card(&mut self, ui: &mut egui::Ui, s: &Snap, st: &Settings) {
        let p = self.pal;
        let (accounts, quotas) = {
            let sh = self.engine.shared.lock().unwrap();
            (sh.accounts.clone(), sh.quotas.clone())
        };
        let Some(cur) = s.account.clone() else {
            egui::Frame::new().fill(p.background).corner_radius(18.0).inner_margin(20).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    icons::show(ui, Icon::People, 48.0, p.on_variant);
                    ui.add_space(6.0);
                    ui.label(RichText::new(tr("You are signed out")).size(16.0));
                    ui.add_space(10.0);
                    if ui.add_enabled(!s.connecting, filled(tr("Sign in with browser"), &p)).clicked() {
                        self.engine.send(Cmd::Connect);
                        ui.close();
                    }
                });
            });
            return;
        };
        // current account
        ui.vertical_centered(|ui| {
            ui.add_space(6.0);
            ui.label(RichText::new(st.account_label(&cur)).size(13.0).color(p.on_variant));
        });
        ui.add_space(8.0);
        egui::Frame::new().fill(p.background).corner_radius(18.0).inner_margin(egui::Margin::symmetric(16, 18)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(80.0, 80.0), Sense::hover());
                self.paint_avatar(ui, &cur, st, r.center(), 38.0);
                ui.add_space(8.0);
                let hello = match st.names.get(&cur) {
                    Some(n) => {
                        if is_vi() {
                            format!("Xin chào, {}!", n.split_whitespace().last().unwrap_or(n))
                        } else {
                            format!("Hi, {}!", n.split_whitespace().next().unwrap_or(n))
                        }
                    }
                    None => tr("Google Account").to_string(),
                };
                ui.label(RichText::new(hello).size(20.0).color(p.on_surface));
                ui.add_space(6.0);
                if let Some((used, total)) = quotas.get(&cur).copied() {
                    let frac = if total > 0 { (used as f32 / total as f32).clamp(0.0, 1.0) } else { 0.0 };
                    ui.add(egui::ProgressBar::new(frac).desired_width(220.0).desired_height(6.0).fill(if frac > 0.9 { p.red } else { p.primary }).corner_radius(3.0));
                    ui.label(RichText::new(storage_text(used, total)).small().color(p.on_variant));
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let w = 240.0;
                    ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                    if ui.add(outlined(tr("Manage storage"), &p).min_size(vec2(w, 36.0))).clicked() {
                        open("https://one.google.com/storage");
                        ui.close();
                    }
                });
            });
        });
        ui.add_space(6.0);
        // other accounts and actions
        egui::Frame::new().fill(p.background).corner_radius(18.0).inner_margin(egui::Margin::symmetric(6, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for a in accounts.iter().filter(|a| **a != cur) {
                let label = st.account_label(a);
                let name = st.names.get(a).cloned().unwrap_or_else(|| label.clone());
                let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(row, 12.0, p.container_high);
                }
                self.paint_avatar(ui, a, st, row.left_center() + vec2(28.0, 0.0), 16.0);
                ui.painter().text(row.left_center() + vec2(56.0, -9.0), egui::Align2::LEFT_CENTER, &name, egui::FontId::proportional(14.0), p.on_surface);
                ui.painter().text(row.left_center() + vec2(56.0, 10.0), egui::Align2::LEFT_CENTER, &label, egui::FontId::proportional(12.0), p.on_variant);
                if resp.clicked() {
                    self.update_settings(|x| x.select_account(a));
                    self.engine.send(Cmd::Refresh);
                    let mut sh = self.engine.shared.lock().unwrap();
                    sh.account = Some(a.clone());
                    sh.shared_items = None;
                    drop(sh);
                    ui.close();
                }
            }
            let can_add = accounts.len() < MAX_ACCOUNTS;
            if action_row(ui, &p, Icon::Add, if can_add { tr("Add another account") } else { tr("Disconnect an account to add another") }, can_add && !s.connecting) {
                self.engine.send(Cmd::Connect);
                ui.close();
            }
            if action_row(ui, &p, Icon::CloudOff, tr("Disconnect account"), true) {
                self.confirm_disconnect = Some(cur.clone());
                ui.close();
            }
        });
        ui.add_space(4.0);
    }

    /// Results under the search box, from the file list of every connected account
    /// (the original sends searches to Google from Spotlight/the search box).
    fn search_results(&mut self, ctx: &egui::Context, rect: egui::Rect, st: &Settings) {
        let q = self.search.trim().to_lowercase();
        if q.is_empty() {
            return;
        }
        let p = self.pal;
        let hits: Vec<(String, crate::engine::IndexItem)> = {
            let sh = self.engine.shared.lock().unwrap();
            let mut v = Vec::new();
            for (acc, items) in &sh.index {
                for it in items {
                    if it.name.to_lowercase().contains(&q) {
                        v.push((acc.clone(), it.clone()));
                        if v.len() >= 30 {
                            break;
                        }
                    }
                }
            }
            v
        };
        let mut close = false;
        egui::Area::new(egui::Id::unique("search-results")).fixed_pos(rect.left_bottom() + vec2(0.0, 4.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::new().fill(p.background).stroke(Stroke::new(1.0, p.outline)).corner_radius(12.0).inner_margin(8).show(ui, |ui| {
                ui.set_width(rect.width() - 16.0);
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for (acc, it) in &hits {
                        let (icon, col) = if it.is_dir { (Icon::Folder, p.on_variant) } else { file_icon(&it.name, &p) };
                        let resp = ui
                            .horizontal(|ui| {
                                ui.add(icons::image(icon, 20.0, col));
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&it.name).color(p.on_surface));
                                    ui.label(RichText::new(format!("{} · {}", st.account_label(acc), it.path)).small().color(p.on_variant));
                                });
                            })
                            .response
                            .interact(Sense::click());
                        if resp.clicked() {
                            let s2 = st.for_account(acc);
                            let local = match s2.drive_mode {
                                DriveMode::Stream => PathBuf::from(s2.stream_dir(acc)).join("My Drive").join(&it.path),
                                DriveMode::Mirror => PathBuf::from(&s2.mirror_folder).join(&it.path),
                            };
                            if local.exists() {
                                open(&local.to_string_lossy());
                            } else {
                                open(&format!("https://drive.google.com/open?id={}", it.id));
                            }
                            close = true;
                        }
                    }
                    if hits.is_empty() {
                        ui.label(RichText::new(tr("No matching results")).color(p.on_variant));
                    }
                    ui.separator();
                    if ui.add(text_button(tr("Search in Drive web"), &p)).clicked() {
                        open(&format!("https://drive.google.com/drive/search?q={}", url_encode(self.search.trim())));
                        close = true;
                    }
                });
            });
        });
        if close {
            self.search.clear();
        }
    }

    // ------------------------------------------------------------------ views
    fn home(&mut self, ui: &mut egui::Ui, s: &Snap) {
        let p = self.pal;
        let st = self.settings();
        if s.rclone_ok == Some(false) {
            bento(ui, &p, |ui| {
                ui.horizontal(|ui| {
                    icons::show(ui, Icon::Error, 24.0, p.red);
                    ui.label(RichText::new(tr("rclone is not installed. Open a terminal and run: sudo apt install rclone")).strong());
                });
            });
        } else if s.account.is_none() {
            self.sign_in_box(ui, s);
        }
        self.ransomware_cards(ui, &st);

        let cols = if ui.available_width() > 640.0 { 2 } else { 1 };
        let (status_icon, status_col, title, sub) = status_texts(s, &st, &p);
        ui.columns(cols, |c| {
            // --- Sync status
            let ui = &mut c[0];
            bento(ui, &p, |ui| {
                card_title(ui, &p, tr("Sync status"));
                ui.horizontal(|ui| {
                    icons::show(ui, status_icon, 32.0, status_col);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(title).size(18.0));
                        ui.label(RichText::new(sub).color(p.on_variant));
                    });
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if s.account.is_some() {
                        let paused = st.paused;
                        if ui.add(outlined(if paused { tr("Resume syncing") } else { tr("Pause syncing") }, &p)).clicked() {
                            self.engine.set_paused(!paused);
                        }
                    }
                    if ui.add(text_button(tr("View all sync activity"), &p)).clicked() {
                        self.go(View::Activity);
                    }
                });
            });
            // --- Notifications
            let ui = if cols == 2 { &mut c[1] } else { &mut c[0] };
            bento(ui, &p, |ui| {
                card_title(ui, &p, tr("Notifications"));
                let list = self.engine.shared.lock().unwrap().notices.clone();
                if list.is_empty() {
                    ui.label(RichText::new(tr("No new notifications")).size(16.0));
                    ui.label(RichText::new(tr("Notifications will show up here")).color(p.on_variant));
                } else {
                    // Like the original's card (home_dialog.css .QNY0bf max-height 370px, rows 77px):
                    // fixed-height rows, scrolled inside the card, never wider than it.
                    egui::ScrollArea::vertical().id_salt("home-notices").max_height(NOTICE_ROW_H * 3.0).auto_shrink([false, true]).show(ui, |ui| {
                        for n in &list {
                            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), NOTICE_ROW_H), Sense::hover());
                            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(row.shrink2(vec2(0.0, 4.0))).layout(Layout::top_down(Align::Min)));
                            child.horizontal(|ui| {
                                icons::show(ui, Icon::Bell, 18.0, if n.read { p.on_variant } else { p.primary });
                                fit_text(ui, &n.title, 14.0, p.on_surface, 1);
                            });
                            fit_text(&mut child, &n.body, 12.0, p.on_variant, 2);
                        }
                    });
                }
                ui.add_space(4.0);
                if ui.add(text_button(tr("View all notifications"), &p)).clicked() {
                    self.go(View::Notifications);
                }
            });
        });
        ui.columns(cols, |c| {
            // --- Quick links
            let ui = &mut c[0];
            bento(ui, &p, |ui| {
                card_title(ui, &p, tr("Quick links"));
                if link_row(ui, &p, Icon::Folder, tr("Open Drive folder"), s.account.is_some()) {
                    open(&st.drive_folder());
                }
                if link_row(ui, &p, Icon::OpenNew, tr("Open Drive web"), true) {
                    open("https://drive.google.com");
                }
                if link_row(ui, &p, Icon::Add, tr("Add more folders to sync"), true) {
                    self.go(View::Folders);
                }
            });
            // --- Storage usage
            let ui = if cols == 2 { &mut c[1] } else { &mut c[0] };
            bento(ui, &p, |ui| {
                card_title(ui, &p, tr("Storage usage"));
                match s.quota {
                    Some((used, total)) => {
                        let frac = if total > 0 { used as f32 / total as f32 } else { 0.0 };
                        ui.label(RichText::new(storage_text(used, total)).size(16.0));
                        ui.add(egui::ProgressBar::new(frac).desired_height(6.0).fill(if frac > 0.9 { p.red } else { p.primary }).corner_radius(3.0));
                    }
                    None => {
                        ui.label(RichText::new(if s.account.is_some() { tr("Loading items...") } else { tr("Not connected") }).color(p.on_variant));
                    }
                }
                ui.add_space(4.0);
                if ui.add(text_button(tr("Manage storage"), &p)).clicked() {
                    open("https://one.google.com/storage");
                }
            });
        });
        ui.columns(cols, |c| {
            let ui = &mut c[0];
            bento(ui, &p, |ui| {
                card_title(ui, &p, tr("Folders connected to Drive"));
                let n = st.folders.len();
                ui.label(RichText::new(folders_text(n)).size(16.0));
                ui.add_space(4.0);
                if ui.add(text_button(tr("Connect a folder"), &p)).clicked() {
                    self.go(View::Folders);
                }
            });
            let ui = if cols == 2 { &mut c[1] } else { &mut c[0] };
            bento(ui, &p, |ui| {
                card_title(ui, &p, tr("Files available offline"));
                let txt = match st.drive_mode {
                    DriveMode::Mirror => tr("All files are automatically available offline"),
                    DriveMode::Stream => tr("Files you open while streaming are kept in the local cache."),
                };
                ui.label(RichText::new(txt).color(p.on_variant));
                ui.add_space(4.0);
                if ui.add(text_button(tr("Offline files"), &p)).clicked() {
                    self.go(View::Offline);
                }
            });
        });
        let _ = s.mounted;
    }

    /// The original's ransomware notification: "Syncing paused: Ransomware detected on {0}" with
    /// "Take action" and "I recognize it".
    fn ransomware_cards(&mut self, ui: &mut egui::Ui, st: &Settings) {
        let p = self.pal;
        let alerts: Vec<crate::ransom::Alert> = self.engine.shared.lock().unwrap().ransomware.values().cloned().collect();
        for a in alerts {
            let who = st.account_label(&a.account);
            egui::Frame::new().fill(p.container_low).stroke(Stroke::new(1.5, p.red)).corner_radius(16.0).inner_margin(20).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    icons::show(ui, Icon::Error, 28.0, p.red);
                    ui.vertical(|ui| {
                        fit_text(ui, &ransom_title(&who), 18.0, p.on_surface, usize::MAX);
                        fit_text(ui, &ransom_body(&a.date, &a.time), 14.0, p.on_variant, usize::MAX);
                    });
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add(filled(tr("Take action"), &p)).clicked() {
                        self.ransom_dialog = Some(a.account.clone());
                    }
                    if ui.add(text_button(tr("I recognize it"), &p)).clicked() {
                        self.ransom_allow = Some(a.account.clone());
                    }
                });
            });
            ui.add_space(12.0);
        }
    }

    fn sign_in_box(&mut self, ui: &mut egui::Ui, s: &Snap) {
        let p = self.pal;
        egui::Frame::new().fill(p.primary_container).corner_radius(16.0).inner_margin(20).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(tr("Sign in to Google Drive")).size(20.0));
            ui.label(RichText::new(tr("You’ll be directed to your browser to finish signing into your account")).color(p.on_variant));
            ui.add_space(8.0);
            if s.connecting && !s.connecting_photos {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr("Waiting for you to finish signing in in the browser…"));
                    if ui.add(text_button(tr("Cancel"), &p)).clicked() {
                        self.engine.cancel_connect();
                    }
                });
            } else if !s.checked {
                ui.spinner();
            } else if ui.add(filled(tr("Sign in with browser"), &p)).clicked() {
                self.engine.send(Cmd::Connect);
            }
        });
        ui.add_space(12.0);
    }

    fn activity_view(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        page_title(ui, &p, tr("Sync activity"));
        let sh = self.engine.shared.lock().unwrap();
        let rows: Vec<Activity> = sh.transfers.iter().chain(sh.activity.iter()).cloned().collect();
        drop(sh);
        if rows.is_empty() {
            empty(ui, &p, Icon::Check, tr("Your files are up to date"), tr("Activity on your files and folders will show up here"));
            return;
        }
        table_header(ui, &p, &[tr("Name"), tr("File size"), tr("Status")], &ACTIVITY_COLS);
        for a in rows {
            table_row(ui, &p, &ACTIVITY_COLS, |col, ui| match col {
                0 => {
                    let (icon, c) = file_icon(&a.name, &p);
                    ui.add(icons::image(icon, 22.0, c));
                }
                1 => {
                    ui.add(egui::Label::new(RichText::new(&a.name).color(p.on_surface)).truncate()).on_hover_text(&a.place);
                }
                2 => {
                    ui.label(RichText::new(a.size.map(theme::human_bytes).unwrap_or_else(|| "—".into())).color(p.on_variant));
                }
                _ => {
                    let (txt, c) = match &a.state {
                        ActState::Synced => (tr("Synced").to_string(), p.on_variant),
                        ActState::Deleted => (tr("Moved to trash").to_string(), p.on_variant),
                        ActState::Transfer { pct, upload: Some(true) } => (pct_text(true, *pct), p.primary),
                        ActState::Transfer { pct, upload: Some(false) } => (pct_text(false, *pct), p.primary),
                        ActState::Transfer { pct, upload: None } => (format!("{} {:.0}%", tr("Syncing items"), pct), p.primary),
                        ActState::Error(e) => (e.clone(), p.red),
                    };
                    ui.vertical(|ui| {
                        // Centre the text (and progress bar) in the row like the other columns.
                        let h = if matches!(a.state, ActState::Transfer { .. }) { 28.0 } else { 18.0 };
                        ui.add_space(((ui.available_height() - h) / 2.0).max(0.0));
                        let r = ui.add(egui::Label::new(RichText::new(&txt).color(c)).truncate());
                        if matches!(a.state, ActState::Error(_)) {
                            r.on_hover_text(&txt);
                        }
                        if let ActState::Transfer { pct, .. } = a.state {
                            ui.add(egui::ProgressBar::new(pct / 100.0).desired_height(4.0).desired_width(ui.available_width().min(160.0)));
                        }
                    });
                }
            });
        }
    }

    fn notifications_view(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        page_title(ui, &p, tr("Notifications"));
        let mut sh = self.engine.shared.lock().unwrap();
        let list = sh.notices.clone();
        if sh.notices.iter().any(|n| !n.read) {
            for n in sh.notices.iter_mut() {
                n.read = true;
            }
            sh.history_dirty = true;
        }
        drop(sh);
        if list.is_empty() {
            empty(ui, &p, Icon::Bell, tr("No new notifications"), tr("Notifications will show up here"));
            return;
        }
        let mut hide = None;
        let mut dismiss = None;
        for (i, n) in list.iter().enumerate() {
            list_row(ui, &p, |ui| {
                ui.add_sized([50.0, 24.0], icons::image(Icon::Bell, 22.0, if n.read { p.on_variant } else { p.primary }));
                ui.vertical(|ui| {
                    fit_text(ui, &n.title, 14.0, p.on_surface, usize::MAX);
                    fit_text(ui, &n.body, 14.0, p.on_variant, usize::MAX);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(ago(n.at)).small().color(p.on_variant));
                        if ui.add(text_button(tr("Dismiss"), &p)).clicked() {
                            dismiss = Some(i);
                        }
                        if ui.add(text_button(tr("Don’t show this again"), &p)).clicked() {
                            hide = Some(n.title.clone());
                        }
                    });
                });
            });
        }
        if let Some(i) = dismiss {
            let mut sh = self.engine.shared.lock().unwrap();
            if i < sh.notices.len() {
                sh.notices.remove(i);
                sh.history_dirty = true;
            }
        }
        if let Some(t) = hide {
            {
                let mut sh = self.engine.shared.lock().unwrap();
                sh.hidden_notices.insert(t.clone());
                sh.notices.retain(|n| n.title != t);
                sh.history_dirty = true;
            }
            self.update_settings(|s| {
                if !s.hidden_notices.contains(&t) {
                    s.hidden_notices.push(t);
                }
            });
        }
    }

    fn folders_view(&mut self, ui: &mut egui::Ui, s: &Snap) {
        let p = self.pal;
        ui.horizontal(|ui| {
            page_title(ui, &p, tr("Folders connected to Drive"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(filled(tr("Connect a folder"), &p)).clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        let path = dir.to_string_lossy().to_string();
                        let st0 = self.settings();
                        if let Some(used_by) = st0.overlap_with_drive_folder(&path, &st0.remote) {
                            let msg = format!("The folder cannot be within {used_by} which is being used by {}", st0.remote);
                            self.engine.shared.lock().unwrap().notify("Can’t sync folder to Drive", &msg);
                            self.go(View::Notifications);
                            return;
                        }
                        self.update_settings(|s| {
                            if !s.folders.iter().any(|f| f.path == path) {
                                let account = s.remote.clone();
                                s.folders.push(LocalFolder { path, to_drive: true, to_photos: false, account, device: false, media_id: String::new() });
                            }
                        });
                        self.engine.send(Cmd::SyncNow);
                    }
                }
            });
        });
        let st = self.settings();
        if st.folders.is_empty() {
            empty(ui, &p, Icon::Folder, tr("Safely back up your files"), tr("Upload, store, and sync your files to Google Drive and Google Photos. Choose a folder on your computer to get started."));
            return;
        }
        let mut remove = None;
        let mut changed = None;
        let mut raw_changed = None;
        for (i, f) in st.folders.iter().enumerate() {
            bento(ui, &p, |ui| {
                ui.horizontal(|ui| {
                    icons::show(ui, Icon::Folder, 24.0, p.on_variant);
                    ui.vertical(|ui| {
                        let name = std::path::Path::new(&f.path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                        ui.label(RichText::new(name).size(16.0));
                        ui.label(RichText::new(&f.path).small().color(p.on_variant));
                        let accounts = self.engine.shared.lock().unwrap().accounts.clone();
                        let acc = if accounts.contains(&f.account) { f.account.clone() } else { st.remote.clone() };
                        let mut badge = format!("{} {}", tr("Account:"), st.account_label(&acc));
                        if f.device {
                            badge.push_str(&format!(" · {}", tr("USB devices backed up when connected")));
                        } else if crate::engine::is_network_path(&f.path) {
                            badge.push_str(&format!(" · {}", tr("Network-attached storage backed up every 6 hours")));
                        }
                        ui.label(RichText::new(badge).small().color(p.primary));
                        if let Some(used_by) = st.overlap_with_drive_folder(&f.path, &st.remote) {
                            ui.label(
                                RichText::new(if is_vi() {
                                    format!("Không đồng bộ hóa được: thư mục này trùng với thư mục Google Drive ({used_by}). Hãy dừng đồng bộ hóa thư mục này hoặc đổi vị trí thư mục Drive.")
                                } else {
                                    format!("Not syncing: this folder overlaps the Google Drive folder ({used_by}). Stop syncing it or move the Drive folder.")
                                })
                                .small()
                                .color(p.red),
                            );
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let stop = match (f.to_drive, f.to_photos) {
                            (true, true) => tr("Stop syncing and backing up"),
                            (false, true) => tr("Stop backing up"),
                            _ => tr("Stop syncing"),
                        };
                        if ui.add(text_button(stop, &p)).clicked() {
                            remove = Some(i);
                        }
                        if ui.add(text_button(tr("Open folder"), &p)).clicked() {
                            open(&f.path);
                        }
                    });
                });
                ui.add_space(6.0);
                let (mut d, mut ph) = (f.to_drive, f.to_photos);
                if ui.checkbox(&mut d, tr("Sync with Google Drive")).changed() {
                    changed = Some((i, d, ph));
                }
                sub_text(ui, &p, tr("Upload all files and folders, including photos and videos. If you edit or remove files, those changes will be automatically synced."));
                ui.add_enabled_ui(s.photos.is_some() || ph, |ui| {
                    if ui.checkbox(&mut ph, tr("Back up to Google Photos")).changed() {
                        changed = Some((i, d, ph));
                    }
                });
                sub_text(ui, &p, tr("Upload only photos and videos. If you remove files, those changes won’t be automatically synced."));
                if ph {
                    let mut raw = st.upload_raw;
                    egui::Frame::new().inner_margin(egui::Margin { left: 26, right: 0, top: 0, bottom: 0 }).show(ui, |ui| {
                        if ui.checkbox(&mut raw, tr("Upload RAW files")).changed() {
                            raw_changed = Some(raw);
                        }
                    });
                }
                if s.photos.is_none() {
                    egui::Frame::new().inner_margin(egui::Margin { left: 26, right: 0, top: 0, bottom: 0 }).show(ui, |ui| {
                        ui.label(RichText::new(tr("Google Photos needs to be connected first.")).small().color(p.on_variant));
                        ui.label(
                            RichText::new(tr(
                                "Google may block this sign-in: since March 2025 it only allows newer apps to add photos. Use “Sync with Google Drive” to keep photos safe in Drive instead.",
                            ))
                            .small()
                            .color(p.on_variant),
                        );
                        ui.horizontal(|ui| {
                            if s.connecting && s.connecting_photos {
                                ui.spinner();
                                ui.label(RichText::new(tr("Waiting for you to finish signing in in the browser…")).small());
                                if ui.add(text_button(tr("Cancel"), &p)).clicked() {
                                    self.engine.cancel_connect();
                                }
                            } else if ui.add_enabled(!s.connecting, text_button(tr("Connect Google Photos"), &p)).clicked() {
                                self.engine.send(Cmd::ConnectPhotos);
                            }
                        });
                    });
                }
            });
        }
        if let Some(i) = remove {
            self.update_settings(|s| {
                s.folders.remove(i);
            });
        }
        if let Some(r) = raw_changed {
            self.update_settings(|s| s.upload_raw = r);
        }
        if let Some((i, d, ph)) = changed {
            self.update_settings(|s| {
                if let Some(f) = s.folders.get_mut(i) {
                    f.to_drive = d;
                    f.to_photos = ph;
                }
            });
            self.engine.send(Cmd::SyncNow);
        }
        ui.label(RichText::new(format!("{} “Linux computers/{}”.", tr("Folders are synced to Google Drive under"), hostname())).small().color(p.on_variant));
    }

    fn offline_view(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        let st = self.settings();
        ui.horizontal(|ui| {
            page_title(ui, &p, tr("Offline files"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(outlined(tr("Refresh"), &p)).clicked() {
                    self.engine.load_offline();
                }
            });
        });
        ui.label(
            RichText::new(match st.drive_mode {
                DriveMode::Stream => tr("Files you open while streaming are kept in the local cache."),
                DriveMode::Mirror => tr("In mirror mode every file is already on this computer."),
            })
            .color(p.on_variant),
        );
        ui.add_space(8.0);
        if st.drive_mode == DriveMode::Stream {
            // "Available offline": files and folders chosen here stay on this computer.
            let mount = PathBuf::from(st.stream_dir(&st.remote));
            bento(ui, &p, |ui| {
                ui.horizontal(|ui| {
                    card_title(ui, &p, tr("Files available offline"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(filled(tr("Add folder"), &p)).clicked() {
                            if let Some(d) = rfd::FileDialog::new().set_directory(&mount).pick_folder() {
                                self.add_pin(d, &mount);
                            }
                        }
                        if ui.add(outlined(tr("Add files"), &p)).clicked() {
                            if let Some(files) = rfd::FileDialog::new().set_directory(&mount).pick_files() {
                                for f in files {
                                    self.add_pin(f, &mount);
                                }
                            }
                        }
                    });
                });
                let pins: Vec<String> = st.offline_pins.iter().filter(|p| p.starts_with(&*mount.to_string_lossy())).cloned().collect();
                if pins.is_empty() {
                    ui.label(RichText::new(tr("Add new files to be available offline")).color(p.on_variant));
                }
                let mut remove = None;
                for pin in &pins {
                    ui.horizontal(|ui| {
                        let is_dir = std::path::Path::new(pin).is_dir();
                        let (icon, c) = if is_dir { (Icon::Folder, p.on_variant) } else { file_icon(pin, &p) };
                        ui.add(icons::image(icon, 20.0, c));
                        let rel = pin.strip_prefix(&*mount.to_string_lossy()).unwrap_or(pin).trim_start_matches('/');
                        ui.label(rel);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(text_button(tr("Remove"), &p)).clicked() {
                                remove = Some(pin.clone());
                            }
                        });
                    });
                }
                if let Some(r) = remove {
                    self.update_settings(|s| s.offline_pins.retain(|x| *x != r));
                }
                if self.engine.shared.lock().unwrap().pins_busy {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(tr("Making files available offline…")).small());
                    });
                }
            });
        }
        let items = self.engine.shared.lock().unwrap().offline_items.clone();
        match items {
            None => {
                ui.spinner();
            }
            Some(v) if v.is_empty() => empty(ui, &p, Icon::Offline, tr("Files available offline"), tr("Files made offline will appear here")),
            Some(v) => {
                // Same figures the original logs in 0x100e65c20: used bytes, and used*100/capacity.
                let used: u64 = v.iter().map(|(_, z)| *z).sum();
                let capacity = if st.drive_mode == DriveMode::Stream && st.cache_limit_on { Some(st.cache_limit_gb.clamp(CACHE_MIN, CACHE_MAX) * 1024 * 1024 * 1024) } else { None };
                ui.label(RichText::new(cache_used_text(used, capacity)).strong());
                ui.add_space(6.0);
                table_header(ui, &p, &[tr("Name"), tr("Size")], &OFFLINE_COLS);
                for (name, size) in v.iter().take(500) {
                    table_row(ui, &p, &OFFLINE_COLS, |col, ui| match col {
                        0 => {
                            let (icon, c) = file_icon(name, &p);
                            ui.add(icons::image(icon, 22.0, c));
                        }
                        1 => {
                            ui.add(egui::Label::new(name.as_str()).truncate());
                        }
                        _ => {
                            ui.label(RichText::new(theme::human_bytes(*size)).color(p.on_variant));
                        }
                    });
                }
            }
        }
    }

    /// Runs a file manager menu entry for `path` (see filemenu.rs).
    fn file_action(&mut self, ctx: &egui::Context, action: &str, path: &str) {
        use crate::filemenu::{locate, web_url, Located};
        let st = self.settings();
        let accounts = self.engine.shared.lock().unwrap().accounts.clone();
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
        let fail = |this: &mut Self, body: String| {
            this.engine.shared.lock().unwrap().notify("Can’t complete action", &body);
            crate::engine::desktop_notify(tr("Can’t complete action"), &body);
        };
        let place = locate(&st, &accounts, path);
        if let Located::NotInDrive = place {
            fail(self, format!("“{name}” {}", tr("isn’t in a Google Drive folder.")));
            return;
        }
        match action {
            "offline" | "online_only" => {
                let (acc, in_stream) = match &place {
                    Located::MyDrive(a, _) | Located::Streamed(a, _) => (a.clone(), st.for_account(a).drive_mode == DriveMode::Stream),
                    Located::NotInDrive => return,
                };
                if !in_stream {
                    crate::engine::desktop_notify(tr("Available offline"), tr("All files are automatically available offline"));
                    return;
                }
                let mount = PathBuf::from(st.for_account(&acc).stream_dir(&acc));
                if action == "offline" {
                    self.add_pin(PathBuf::from(path), &mount);
                    crate::engine::desktop_notify(tr("Available offline"), &name);
                } else {
                    let p = path.to_string();
                    self.update_settings(|s| s.offline_pins.retain(|x| *x != p));
                    crate::engine::desktop_notify(tr("Online only"), &name);
                }
            }
            _ => {
                let (acc, spec) = match &place {
                    Located::MyDrive(a, rel) => (a.clone(), format!("{a}:{rel}")),
                    Located::Streamed(a, rel) => (a.clone(), format!("{}:{rel}", crate::engine::combined_name(a))),
                    Located::NotInDrive => return,
                };
                let action = action.to_string();
                let slot = self.link_to_copy.clone();
                let ctx2 = ctx.clone();
                std::thread::spawn(move || {
                    let url = match crate::engine::item_id(&spec) {
                        // Drive's own link for the item, like the original; a built one if that fails.
                        Some((id, dir)) => match crate::engine::web_view_link(&acc, &id) {
                            Some(link) if action != "share" => link,
                            _ => web_url(&action, &id, dir),
                        },
                        None => {
                            // Not uploaded yet (or no ID through the combined view): search for it instead.
                            format!("https://drive.google.com/drive/search?q={}", url_encode(&name))
                        }
                    };
                    if action == "copy_link" {
                        *slot.lock().unwrap() = Some(url);
                        ctx2.request_repaint();
                    } else {
                        open(&url);
                    }
                });
            }
        }
    }

    fn add_pin(&mut self, path: PathBuf, mount: &std::path::Path) {
        if !path.starts_with(mount) {
            self.engine.shared.lock().unwrap().notify("Can’t make this available offline", "Choose files inside the Google Drive streaming folder.");
            self.go(View::Notifications);
            return;
        }
        let p = path.to_string_lossy().to_string();
        self.update_settings(|s| {
            if !s.offline_pins.contains(&p) {
                s.offline_pins.push(p);
            }
        });
        self.engine.send(Cmd::PinsChanged);
    }

    fn shared_view(&mut self, ui: &mut egui::Ui, s: &Snap) {
        let p = self.pal;
        ui.horizontal(|ui| {
            page_title(ui, &p, tr("Shared with me"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add_enabled(s.account.is_some(), outlined(tr("Refresh"), &p)).clicked() {
                    self.engine.load_shared();
                }
            });
        });
        let (items, loading) = {
            let sh = self.engine.shared.lock().unwrap();
            (sh.shared_items.clone(), sh.shared_loading)
        };
        if loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(tr("Loading items..."));
            });
            return;
        }
        let items = items.unwrap_or_default();
        if items.is_empty() {
            empty(ui, &p, Icon::People, tr("Nothing has been shared with you yet"), "");
            return;
        }
        table_header(ui, &p, &[tr("Name"), tr("Date shared"), tr("Size"), ""], &SHARED_COLS);
        let mut add_shortcut: Option<(String, String)> = None;
        for it in items {
            table_row(ui, &p, &SHARED_COLS, |col, ui| match col {
                0 => {
                    let (icon, c) = if it.is_dir { (Icon::Folder, p.on_variant) } else { file_icon(&it.name, &p) };
                    ui.add(icons::image(icon, 22.0, c));
                }
                1 => {
                    ui.add(egui::Label::new(it.name.as_str()).truncate());
                }
                2 => {
                    ui.label(RichText::new(&it.mod_time).color(p.on_variant));
                }
                3 => {
                    let size = if it.size >= 0 && !it.is_dir { theme::human_bytes(it.size as u64) } else { "—".into() };
                    ui.label(RichText::new(size).color(p.on_variant));
                }
                _ => {
                    if !it.id.is_empty() && icons::button(ui, Icon::Add, p.on_variant, p.container_high, tr("Add shortcut to Google Drive")).clicked() {
                        add_shortcut = Some((it.id.clone(), it.name.clone()));
                    }
                    if !it.id.is_empty() && icons::button(ui, Icon::OpenNew, p.on_variant, p.container_high, tr("Open in web")).clicked() {
                        open(&format!("https://drive.google.com/open?id={}", it.id));
                    }
                }
            });
        }
        // Like the original: pick where the shortcut goes; it must be inside the Drive folder.
        if let Some((id, name)) = add_shortcut {
            let st = self.settings();
            let start = st.drive_folder();
            if let Some(dir) = rfd::FileDialog::new().set_title(tr("Add shortcut to Google Drive")).set_directory(&start).pick_folder() {
                self.engine.add_shortcut(id, name, dir);
            }
        }
    }

    fn settings_view(&mut self, ui: &mut egui::Ui, s: &Snap) {
        let p = self.pal;
        let st = self.settings();
        page_title(ui, &p, tr("Settings"));

        section_title(ui, &p, tr("General settings"));
        bento(ui, &p, |ui| {
            setting_title(ui, tr("Appearance"));
            ui.horizontal(|ui| {
                let mut dark = st.dark;
                ui.radio_value(&mut dark, false, tr("Light"));
                ui.radio_value(&mut dark, true, tr("Dark"));
                if dark != st.dark {
                    self.update_settings(|s| s.dark = dark);
                    self.pal = if dark { DARK } else { LIGHT };
                }
            });
            ui.add_space(6.0);
            setting_title(ui, tr("Language"));
            ui.horizontal(|ui| {
                let mut vi = is_vi();
                ui.radio_value(&mut vi, false, "English");
                ui.radio_value(&mut vi, true, "Tiếng Việt");
                if vi != is_vi() {
                    set_vietnamese(vi);
                    self.update_settings(|s| s.language = if vi { "vi".into() } else { "en".into() });
                }
            });
        });
        bento(ui, &p, |ui| {
            setting_title(ui, tr("Launch on login"));
            let mut on = st.autostart;
            if ui.checkbox(&mut on, tr("Launch Google Drive when you login to your computer")).changed() {
                self.update_settings(|s| s.autostart = on);
                set_autostart(on);
            }
            let mut tray = st.close_keeps_running;
            let has_tray = self.tray.is_some();
            ui.add_enabled_ui(has_tray, |ui| {
                if ui.checkbox(&mut tray, tr("Keep running in the system tray when the window is closed")).changed() {
                    self.update_settings(|s| s.close_keeps_running = tray);
                }
            });
            if !has_tray {
                ui.label(RichText::new(tr("No system tray was found on this desktop.")).small().color(p.on_variant));
            }
            let mut menu = st.file_manager_menu;
            if ui.checkbox(&mut menu, tr("Show Google Drive in the file manager menu")).changed() {
                self.update_settings(|s| s.file_manager_menu = menu);
                let _ = crate::filemenu::install(menu, is_vi());
            }
        });
        bento(ui, &p, |ui| {
            setting_title(ui, tr("Proxy settings"));
            let mut direct = st.proxy_direct;
            ui.radio_value(&mut direct, false, tr("Auto-detect"));
            ui.radio_value(&mut direct, true, tr("Direct connection"));
            if direct != st.proxy_direct {
                self.update_settings(|s| s.proxy_direct = direct);
                PROXY_DIRECT.store(direct, std::sync::atomic::Ordering::Relaxed);
                self.needs_restart = true;
            }
        });
        bento(ui, &p, |ui| {
            setting_title(ui, tr("Bandwidth settings"));
            let mut d = st.download_limit_on;
            if ui.checkbox(&mut d, tr("Download rate")).changed() {
                self.update_settings(|s| s.download_limit_on = d);
                self.needs_restart = true;
            }
            if d {
                if let Some(v) = number_field(ui, &p, &mut self.down_text, tr("kB/s"), RATE_MIN, RATE_MAX) {
                    if v != st.download_kbps {
                        self.update_settings(|s| s.download_kbps = v);
                        self.needs_restart = true;
                    }
                }
            }
            let mut u = st.upload_limit_on;
            if ui.checkbox(&mut u, tr("Upload rate")).changed() {
                self.update_settings(|s| s.upload_limit_on = u);
                self.needs_restart = true;
            }
            if u {
                if let Some(v) = number_field(ui, &p, &mut self.up_text, tr("kB/s"), RATE_MIN, RATE_MAX) {
                    if v != st.upload_kbps {
                        self.update_settings(|s| s.upload_kbps = v);
                        self.needs_restart = true;
                    }
                }
            }
        });

        section_title(ui, &p, tr("Account settings"));
        bento(ui, &p, |ui| {
            setting_title(ui, tr("My Drive syncing options"));
            let mut mode = st.drive_mode;
            mode_option(
                ui,
                &p,
                &mut mode,
                DriveMode::Stream,
                tr("Stream files"),
                &[tr("Store all My Drive files in the cloud only"), tr("Access files from a folder on your computer"), tr("Streaming uses less hard drive space.")],
            );
            ui.add_space(6.0);
            mode_option(
                ui,
                &p,
                &mut mode,
                DriveMode::Mirror,
                tr("Mirror files"),
                &[tr("Store all My Drive files in the cloud and on your computer"), tr("Access files from a folder on your computer"), tr("All files are automatically available offline")],
            );
            if mode != st.drive_mode {
                self.update_settings(|s| s.drive_mode = mode);
                self.engine.send(Cmd::Reconfigure);
            }
            ui.add_space(10.0);
            let (title, path) = match mode {
                DriveMode::Stream => (tr("Google Drive streaming location"), st.stream_dir(&st.remote)),
                DriveMode::Mirror => (tr("My Drive folder location"), st.mirror_folder.clone()),
            };
            setting_title(ui, title);
            ui.label(RichText::new(&path).color(p.on_variant));
            ui.horizontal(|ui| {
                if ui.add(outlined(tr("Change folder location"), &p)).clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        let np = dir.to_string_lossy().to_string();
                        // Same checks as the original's CheckForValidDirMountPoint (0x10152f980).
                        let check = if mode == DriveMode::Stream { check_mount_dir(&np) } else { MountCheck::Ok };
                        let overlaps = st.folders.iter().any(|f| {
                            let mut t = st.clone();
                            t.drive_mode = mode;
                            match mode {
                                DriveMode::Stream => t.stream_folder = np.clone(),
                                DriveMode::Mirror => t.mirror_folder = np.clone(),
                            }
                            t.overlap_with_drive_folder(&f.path, &t.remote).is_some()
                        });
                        if overlaps {
                            self.engine
                                .shared
                                .lock()
                                .unwrap()
                                .notify("Can’t change folder location", "This folder is already synced from your computer (Connected folders). Choose a different folder.");
                            self.go(View::Notifications);
                            return;
                        }
                        if check != MountCheck::Ok {
                            self.engine.shared.lock().unwrap().notify("Unable to mount to selected folder.", mount_check_message(&check));
                            self.go(View::Notifications);
                            return;
                        }
                        self.update_settings(|s| match mode {
                            DriveMode::Stream => s.stream_folder = np,
                            DriveMode::Mirror => {
                                s.mirror_folder = np;
                                s.mirror_initialized = false;
                            }
                        });
                        self.engine.send(Cmd::Reconfigure);
                    }
                }
                if ui.add(text_button(tr("Open folder"), &p)).clicked() {
                    open(&path);
                }
            });
            if mode == DriveMode::Stream {
                ui.label(RichText::new(tr("This folder must be empty.")).small().color(p.on_variant));
                let drives = self.engine.shared.lock().unwrap().shared_drives.get(&st.remote).cloned().unwrap_or_default();
                let txt = if drives.is_empty() {
                    tr("Inside it: “My Drive” and “Shared drives” (no shared drives on this account).").to_string()
                } else {
                    format!("{} {}", tr("Inside it: “My Drive” and “Shared drives”:"), drives.join(", "))
                };
                ui.label(RichText::new(txt).small().color(p.on_variant));
            }
        });
        bento(ui, &p, |ui| {
            setting_title(ui, tr("Shared drives"));
            let drives = self.engine.shared.lock().unwrap().drive_list.get(&st.remote).cloned();
            match (st.drive_mode, drives) {
                (_, None) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(tr("Loading shared drives")).color(p.on_variant));
                    });
                }
                (_, Some(list)) if list.is_empty() => {
                    ui.label(RichText::new(tr("This account has no shared drives.")).color(p.on_variant));
                }
                (DriveMode::Stream, Some(list)) => {
                    ui.label(
                        RichText::new(format!("{} {}", tr("All shared drives are streamed inside “Shared drives”:"), list.iter().map(|d| d.1.clone()).collect::<Vec<_>>().join(", ")))
                            .color(p.on_variant),
                    );
                }
                (DriveMode::Mirror, Some(list)) => {
                    setting_title(ui, tr("Shared drive syncing"));
                    ui.label(RichText::new(format!("{} {}", tr("Shared drives switched on are mirrored to"), st.shared_drives_root(&st.remote).to_string_lossy())).small().color(p.on_variant));
                    let on: Vec<String> = st.synced_shared_drives.get(&st.remote).cloned().unwrap_or_default();
                    let mut changed: Option<(String, bool)> = None;
                    for (id, name) in &list {
                        let mut v = on.contains(id);
                        if ui.checkbox(&mut v, name).on_hover_text(tr("Toggle shared drive")).changed() {
                            changed = Some((id.clone(), v));
                        }
                    }
                    if let Some((id, v)) = changed {
                        let acc = st.remote.clone();
                        self.update_settings(|s| {
                            let e = s.synced_shared_drives.entry(acc).or_default();
                            e.retain(|x| *x != id);
                            if v {
                                e.push(id);
                            }
                        });
                        let msg = if v { tr("Shared drive syncing enabled") } else { tr("Shared drive syncing disabled") };
                        self.engine.shared.lock().unwrap().notify(tr("Shared drive sync settings updated"), msg);
                        self.engine.send(Cmd::SyncNow);
                    }
                }
            }
        });
        bento(ui, &p, |ui| {
            setting_title(ui, tr("Local cached files directory"));
            ui.label(RichText::new(&st.cache_folder).color(p.on_variant));
            if ui.add(outlined(tr("Change"), &p)).clicked() {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    let np = dir.to_string_lossy().to_string();
                    self.update_settings(|s| s.cache_folder = np);
                    self.needs_restart = true;
                }
            }
            ui.add_space(8.0);
            setting_title(ui, tr("Cache settings"));
            ui.label(
                RichText::new(tr("Limit the amount of storage on your computer used by cached files. Limit won’t apply to files that need to sync or files you make available offline."))
                    .color(p.on_variant),
            );
            let mut on = st.cache_limit_on;
            if ui.checkbox(&mut on, tr("Limit cache size")).changed() {
                self.update_settings(|s| s.cache_limit_on = on);
                self.needs_restart = true;
            }
            if on {
                if let Some(v) = number_field(ui, &p, &mut self.cache_text, "GB", CACHE_MIN, CACHE_MAX) {
                    if v != st.cache_limit_gb {
                        self.update_settings(|s| s.cache_limit_gb = v);
                        self.needs_restart = true;
                    }
                }
            }
        });
        bento(ui, &p, |ui| {
            setting_title(ui, tr("Google Account"));
            match &s.account {
                Some(a) => {
                    ui.label(format!("{} {a}", tr("Signed in as")));
                    if ui.add(outlined(tr("Disconnect account"), &p)).clicked() {
                        self.confirm_disconnect = Some(a.clone());
                    }
                }
                None => {
                    ui.label(RichText::new(tr("You are signed out")).color(p.on_variant));
                    if ui.add(filled(tr("Sign in with browser"), &p)).clicked() {
                        self.engine.send(Cmd::Connect);
                    }
                }
            }
        });
        section_title(ui, &p, tr("USB and External Devices"));
        bento(ui, &p, |ui| {
            let mut prompt = st.prompt_devices;
            if ui.checkbox(&mut prompt, tr("Prompt me to back up devices")).changed() {
                self.update_settings(|s| s.prompt_devices = prompt);
            }
            ui.add_space(6.0);
            setting_title(ui, tr("Ignored USB Devices"));
            ui.label(RichText::new(tr("You won’t be prompted to sync or back up these devices")).small().color(p.on_variant));
            if st.ignored_media.is_empty() {
                ui.label(RichText::new(tr("Ignored USB devices will show up here")).color(p.on_variant));
            }
            let mut remove = None;
            for d in &st.ignored_media {
                ui.horizontal(|ui| {
                    ui.add(icons::image(Icon::Computer, 18.0, p.on_variant));
                    ui.label(&d.name).on_hover_text(&d.last_mount_point);
                    if ui.add(text_button(tr("Remove ignored device"), &p)).clicked() {
                        remove = Some(d.media_id.clone());
                    }
                });
            }
            if let Some(r) = remove {
                self.update_settings(|s| s.ignored_media.retain(|x| x.media_id != r));
            }
            if !st.ignored_media.is_empty() && ui.add(outlined(tr("Stop ignoring all"), &p)).clicked() {
                self.update_settings(|s| s.ignored_media.clear());
            }
        });
        section_title(ui, &p, "rclone");
        bento(ui, &p, |ui| {
            let (ver, busy) = {
                let sh = self.engine.shared.lock().unwrap();
                (sh.rclone_version.clone(), sh.updating_rclone)
            };
            ui.label(RichText::new(if is_vi() { format!("Đang dùng: {ver}") } else { format!("In use: {ver}") }).color(p.on_variant));
            ui.label(RichText::new(tr("A newer rclone is needed for Google Photos (Google changed its access in March 2025). It is downloaded from rclone.org, checked against its published checksum, and kept in this app's own folder.")).small().color(p.on_variant));
            if busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr("Downloading…"));
                });
            } else if ui.add(outlined(tr("Get the latest rclone"), &p)).clicked() {
                self.engine.update_rclone();
            }
        });
        section_title(ui, &p, if is_vi() { "Mức tương thích với bản gốc" } else { "Compatibility with the original" });
        bento(ui, &p, |ui| compat_list(ui, &p));
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let p = self.pal;
        if let Some(acc) = self.confirm_disconnect.clone() {
            let label = self.settings().account_label(&acc);
            let r = egui::Modal::new(egui::Id::unique("disconnect")).show(ctx, |ui| {
                ui.set_width(400.0);
                ui.label(RichText::new(tr("Disconnect account?")).size(20.0));
                ui.label(RichText::new(&label).color(p.on_variant));
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(filled(tr("Disconnect"), &p)).clicked() {
                        self.engine.send(Cmd::Disconnect(acc.clone()));
                        self.confirm_disconnect = None;
                    }
                    if ui.add(text_button(tr("Cancel"), &p)).clicked() {
                        self.confirm_disconnect = None;
                    }
                });
            });
            if r.should_close() {
                self.confirm_disconnect = None;
            }
        }
        // A USB device was plugged in: offer to back it up (the original's "Add device" prompt).
        let new_device = self.engine.shared.lock().unwrap().new_device.clone();
        if let Some(dev) = new_device {
            let st = self.settings();
            if !st.prompt_devices {
                self.engine.shared.lock().unwrap().new_device = None;
            } else {
                let name = std::path::Path::new(&dev).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| dev.clone());
                let mut done = false;
                egui::Modal::new(egui::Id::unique("device")).show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui.label(RichText::new(format!("{} “{name}”", tr("Add"))).size(20.0));
                    ui.label(RichText::new(tr("Choose to upload files in this device to Google Drive or Google Photos")).color(p.on_variant));
                    ui.add_space(6.0);
                    ui.checkbox(&mut self.remember_device, tr("Remember my choice for this device"));
                    ui.add_space(8.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(filled(tr("Add device"), &p)).clicked() {
                            let path = dev.clone();
                            self.update_settings(|s| {
                                if !s.folders.iter().any(|f| f.path == path) {
                                    let account = s.remote.clone();
                                    let id = media_id(&path);
                                    s.folders.push(LocalFolder { path, to_drive: true, to_photos: false, account, device: true, media_id: id });
                                }
                            });
                            self.engine.send(Cmd::SyncNow);
                            self.go(View::Folders);
                            done = true;
                        }
                        if ui.add(text_button(tr("Not now"), &p)).clicked() {
                            if self.remember_device {
                                let path = dev.clone();
                                let id = media_id(&path);
                                let name = name.clone();
                                self.update_settings(|s| {
                                    if !s.ignored_media.iter().any(|m| m.media_id == id) {
                                        s.ignored_media.push(IgnoredMedia { media_id: id, name, last_mount_point: path });
                                    }
                                });
                            }
                            done = true;
                        }
                    });
                });
                if done {
                    self.remember_device = false;
                    self.engine.shared.lock().unwrap().new_device = None;
                }
            }
        }
        if self.confirm_quit {
            let mut answer = None;
            let r = egui::Modal::new(egui::Id::unique("quit")).show(ctx, |ui| {
                ui.set_width(420.0);
                ui.label(RichText::new(tr("Quit Google Drive?")).size(20.0));
                ui.add_space(4.0);
                ui.label(RichText::new(tr("Files are still syncing. Your changes won’t sync unless Google Drive is running.")).color(p.on_variant));
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(outlined(tr("Quit now"), &p)).clicked() {
                        answer = Some(true);
                    }
                    if ui.add(filled(tr("Finish syncing, then quit"), &p)).clicked() {
                        // A second request is ignored, like the original.
                        if self.quit_after_sync.is_none() {
                            self.quit_after_sync = Some(std::time::Instant::now());
                            self.engine.send(Cmd::SyncNow);
                        }
                        self.confirm_quit = false;
                    }
                    if ui.add(text_button(tr("Cancel"), &p)).clicked() {
                        answer = Some(false);
                    }
                });
            });
            if r.should_close() && answer.is_none() {
                answer = Some(false);
            }
            match answer {
                Some(true) => {
                    self.allow_close = true;
                    self.confirm_quit = false;
                    self.engine.send(Cmd::Quit);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Some(false) => self.confirm_quit = false,
                None => {}
            }
        }
        self.ransomware_dialogs(ctx);
        self.diagnostics_dialog(ctx);
        if self.show_about {
            let r = egui::Modal::new(egui::Id::unique("about")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.horizontal(|ui| {
                    ui.add(theme::logo(40.0));
                    ui.label(RichText::new(crate::APP_NAME).size(20.0));
                });
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label(RichText::new(tr("Independent Linux app. Connects to Google Drive through rclone; not made by Google.")).color(p.on_variant));
                ui.add_space(8.0);
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| compat_list(ui, &p));
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(text_button(tr("Close"), &p)).clicked() {
                        self.show_about = false;
                    }
                });
            });
            if r.should_close() {
                self.show_about = false;
            }
        }
    }

    /// "Ransomware detected" (the original's ransomware-dialog-content) and the "I recognize it"
    /// confirmation. The original's steps send the person to Drive's web restore tool; here the
    /// changes never reached Drive, so its copies can simply be put back.
    fn ransomware_dialogs(&mut self, ctx: &egui::Context) {
        let p = self.pal;
        if let Some(acc) = self.ransom_allow.clone() {
            let who = self.settings().account_label(&acc);
            let mut close = false;
            let r = egui::Modal::new(egui::Id::unique("ransom-allow")).show(ctx, |ui| {
                ui.set_width(440.0);
                ui.label(RichText::new(tr("I recognize it")).size(20.0));
                ui.add_space(4.0);
                ui.label(
                    RichText::new(if is_vi() {
                        format!("Các thay đổi trên máy tính này sẽ được đồng bộ hoá lên {who} như hiện có.")
                    } else {
                        format!("The changes on this computer will be synced to {who} as they are.")
                    })
                    .color(p.on_variant),
                );
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(filled(tr("Allow sync"), &p)).clicked() {
                        self.engine.send(Cmd::RansomwareAllow(acc.clone()));
                        close = true;
                    }
                    if ui.add(text_button(tr("Cancel"), &p)).clicked() {
                        close = true;
                    }
                });
            });
            if close || r.should_close() {
                self.ransom_allow = None;
            }
        }
        let Some(acc) = self.ransom_dialog.clone() else { return };
        let alert = self.engine.shared.lock().unwrap().ransomware.get(&acc).cloned();
        let Some(alert) = alert else {
            self.ransom_dialog = None;
            return;
        };
        let vi = is_vi();
        let t = |en: &'static str, v: &'static str| if vi { v } else { en };
        let mut close = false;
        let r = egui::Modal::new(egui::Id::unique("ransom")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.label(RichText::new(t("Ransomware detected", "Đã phát hiện phần mềm tống tiền")).size(22.0));
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    icons::show(ui, Icon::Error, 22.0, p.red);
                    let head = if vi {
                        format!("Đã phát hiện phần mềm tống tiền: Tính năng đồng bộ hoá tệp đã bị tạm dừng lúc {} ngày {}", alert.time, alert.date)
                    } else {
                        format!("Ransomware detected: File syncing paused on {} at {}", alert.date, alert.time)
                    };
                    ui.label(RichText::new(head).strong());
                });
                ui.add_space(6.0);
                ui.label(RichText::new(t("What is ransomware?", "Phần mềm tống tiền là gì?")).strong());
                ui.label(RichText::new(t(
                    "Harmful software that prevents access to a computer system until an amount of money is paid.",
                    "Phần mềm độc hại ngăn chặn quyền truy cập vào hệ thống máy tính cho đến khi người dùng thanh toán một khoản tiền.",
                )).color(p.on_variant));
                ui.add_space(6.0);
                ui.label(t(
                    "Your files are safely stored in Drive, but you need to remove the ransomware from your computer. You should also make sure that you have effective and up-to-date antivirus software installed.",
                    "Các tệp của bạn được lưu trữ an toàn trong Drive, nhưng bạn cần xoá phần mềm tống tiền khỏi máy tính. Bạn cũng nên đảm bảo đã cài đặt phần mềm diệt virus hiệu quả và mới nhất.",
                ));
                ui.add_space(4.0);
                let n = alert.file_count();
                ui.label(RichText::new(if vi {
                    format!("{n} tệp đã đồng bộ bị sửa đổi hoặc xoá trên máy tính này. Các thay đổi đã được giữ lại nên bản trên Drive không bị ảnh hưởng.")
                } else {
                    format!("{n} synced files were modified or removed on this computer. The changes were held back, so the copies on Drive weren’t changed.")
                }).color(p.on_variant));
                ui.add_space(4.0);
                ui.label(t(
                    "Drive keeps old file versions for 25 days, so you should initiate a restore in less than 25 days.",
                    "Drive lưu giữ các phiên bản tệp cũ trong 25 ngày, vì vậy, bạn nên tiến hành khôi phục trong vòng dưới 25 ngày.",
                ));
                ui.add_space(10.0);
                // Step 1 of the original.
                ui.label(RichText::new(t("Disconnect account and restore files", "Ngắt kết nối tài khoản và khôi phục tệp")).size(16.0).strong());
                ui.label(RichText::new(t(
                    "1. Disconnect your Google Account from Drive for desktop.",
                    "1. Ngắt kết nối Tài khoản Google khỏi ứng dụng Drive cho máy tính.",
                )).color(p.on_variant));
                ui.label(RichText::new(t(
                    "2. Use the Drive restoration tool to undo file changes made by ransomware.",
                    "2. Sử dụng công cụ khôi phục của Drive để huỷ các thay đổi đối với tệp do phần mềm tống tiền thực hiện.",
                )).color(p.on_variant));
                ui.horizontal(|ui| {
                    if ui.add(outlined(tr("Disconnect account"), &p)).clicked() {
                        self.confirm_disconnect = Some(acc.clone());
                        close = true;
                    }
                    if ui.add(text_button(tr("Open Drive web"), &p)).clicked() {
                        open("https://drive.google.com/drive/my-drive");
                    }
                });
                ui.add_space(10.0);
                // Step 2 of the original.
                ui.label(RichText::new(t("Clear files and resume syncing", "Xoá các tệp và tiếp tục đồng bộ hoá")).size(16.0).strong());
                ui.label(RichText::new(t(
                    "If mirroring My Drive or a local folder, delete the corrupted files or move them to a new folder on your computer.",
                    "Nếu bạn đang đồng bộ hoá hai chiều Drive của tôi hoặc thư mục trên thiết bị, hãy xoá các tệp bị hỏng hoặc di chuyển các tệp đó sang thư mục mới trên máy tính.",
                )).color(p.on_variant));
                ui.label(RichText::new(t(
                    "This app can do it for you: the changed files are moved to a new folder in “Lost and Found”, Drive’s copies are put back and syncing resumes.",
                    "Ứng dụng có thể làm việc này cho bạn: các tệp đã thay đổi được chuyển sang một thư mục mới trong “Bị thất lạc và đã tìm thấy”, bản trên Drive được đưa trở lại và tiếp tục đồng bộ hoá.",
                )).color(p.on_variant));
                if ui.add(filled(tr("Clear files and resume syncing"), &p)).clicked() {
                    self.engine.send(Cmd::RansomwareRestore(acc.clone()));
                    close = true;
                }
                ui.add_space(8.0);
                if ui.add(text_button(t("Learn more about ransomware recovery", "Tìm hiểu thêm về cách khôi phục sau khi bị tấn công bằng phần mềm tống tiền"), &p)).clicked() {
                    open("https://support.google.com/drive/search?q=ransomware%20recovery");
                }
            });
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(text_button(tr("Close"), &p)).clicked() {
                    close = true;
                }
            });
        });
        if close || r.should_close() {
            self.ransom_dialog = None;
        }
    }

    /// A newer copy of the app replaced this one on disk: once nothing is syncing, quit cleanly
    /// (streaming folders released, history saved) and start the new copy in this process, so
    /// new parts such as the file manager badges are installed without the person restarting.
    fn restart_if_updated(&mut self, ctx: &egui::Context) {
        if self.screenshot.is_some() {
            return;
        }
        if let Some(t) = self.restart_at {
            if t.elapsed() < std::time::Duration::from_millis(1500) {
                ctx.request_repaint_after(std::time::Duration::from_millis(300));
                return;
            }
            if let Some(exe) = updated_executable() {
                use std::os::unix::process::CommandExt;
                let mut c = std::process::Command::new(exe);
                if self.hidden {
                    c.arg("--background");
                }
                let err = c.exec(); // only returns on failure
                crate::diag::log_event("error", &format!("Couldn't start the updated app: {err}"));
            }
            self.restart_at = None;
            return;
        }
        if self.last_update_check.elapsed() < std::time::Duration::from_secs(UPDATE_CHECK_SECS) {
            return;
        }
        self.last_update_check = std::time::Instant::now();
        let busy = {
            let sh = self.engine.shared.lock().unwrap();
            sh.job.is_some() || !sh.transfers.is_empty() || sh.pending > 0 || sh.connecting || sh.connecting_photos
        };
        let dialog_open = self.confirm_disconnect.is_some() || self.confirm_quit || self.ransom_dialog.is_some() || self.ransom_allow.is_some() || self.diag.lock().unwrap().is_some();
        if busy || dialog_open || self.quit_after_sync.is_some() || updated_executable().is_none() {
            return;
        }
        crate::diag::log_event("notice", "A newer version of the app is installed; restarting into it");
        self.engine.send(Cmd::Quit);
        self.restart_at = Some(std::time::Instant::now());
        ctx.request_repaint_after(std::time::Duration::from_millis(300));
    }

    /// Builds the report on its own thread, one at a time (the original's
    /// diagnostic_info_generator_thread, function 0x100512ce0).
    fn generate_diagnostics(&mut self, ctx: &egui::Context) {
        let mut g = self.diag.lock().unwrap();
        if matches!(*g, Some(None)) {
            return;
        }
        *g = Some(None);
        drop(g);
        let state = self.diag.clone();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("diagnostic_info_generator_thread".into())
            .spawn(move || {
                let r = crate::diag::generate(&crate::engine::rclone_program());
                *state.lock().unwrap() = Some(Some(r));
                ctx.request_repaint();
            })
            .ok();
    }

    fn diagnostics_dialog(&mut self, ctx: &egui::Context) {
        let p = self.pal;
        let cur = self.diag.lock().unwrap().clone();
        let Some(cur) = cur else { return };
        let mut close = false;
        egui::Modal::new(egui::Id::unique("diag")).show(ctx, |ui| {
            ui.set_width(440.0);
            match &cur {
                None => {
                    ui.label(RichText::new(tr("Generating diagnostic info")).size(20.0));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(tr("Diagnostic info is being generated. This may take a few minutes. Please wait…")).color(p.on_variant));
                    });
                }
                Some(Ok(path)) => {
                    ui.label(RichText::new(tr("Diagnostic info created")).size(20.0));
                    ui.add_space(4.0);
                    let text = if is_vi() {
                        format!("Thông tin chẩn đoán của bạn đã được tạo tại \"{}\"", path.display())
                    } else {
                        format!("Your diagnostic has been created at “{}”", path.display())
                    };
                    ui.label(RichText::new(text).color(p.on_variant));
                    ui.add_space(10.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(filled(tr("OK"), &p)).clicked() {
                            close = true;
                        }
                        if ui.add(text_button(tr("Open folder"), &p)).clicked() {
                            if let Some(dir) = path.parent() {
                                open(&dir.to_string_lossy());
                            }
                        }
                    });
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(tr("Diagnostic info could not be created")).size(20.0));
                    ui.label(RichText::new(e).color(p.on_variant));
                    ui.add_space(10.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(filled(tr("OK"), &p)).clicked() {
                            close = true;
                        }
                    });
                }
            }
        });
        if close {
            *self.diag.lock().unwrap() = None;
        }
    }

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        self.frames += 1;
        ctx.request_repaint();
        if self.frames >= 120 && !self.shot_requested {
            self.shot_requested = true;
            let c = ctx.clone();
            ctx.request_screenshot(move |img| {
                let [w, h] = img.size;
                let mut buf = Vec::with_capacity(w * h * 4);
                for px in &img.pixels {
                    buf.extend_from_slice(&px.to_srgba_unmultiplied());
                }
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                if let Some(im) = image::RgbaImage::from_raw(w as u32, h as u32, buf) {
                    if let Err(e) = im.save(&path) {
                        eprintln!("could not save screenshot: {e}");
                    }
                }
                c.send_viewport_cmd(egui::ViewportCommand::Close);
            });
        }
        if self.frames > 900 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for DriveApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // If naming the window failed while it was created, try again on the first frames.
        if !CLASS_SET.load(std::sync::atomic::Ordering::Relaxed) && CLASS_TRIES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 5 {
            CLASS_SET.store(set_window_class(frame), std::sync::atomic::Ordering::Relaxed);
        }
        let ctx = ui.ctx().clone();
        let dark = self.settings().dark;
        if self.applied_dark != Some(dark) {
            self.pal = if dark { DARK } else { LIGHT };
            theme::apply(&ctx, &self.pal, dark);
            self.applied_dark = Some(dark);
        }
        let p = self.pal;
        let s = self.snap();

        egui::Panel::left("nav")
            .exact_size(NAV_WIDTH)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(p.container).inner_margin(egui::Margin { left: 12, right: 12, top: 0, bottom: 0 }))
            .show(ui, |ui| self.nav(ui, &s));
        egui::Panel::top("top").exact_size(TOP_BAR).resizable(false).show_separator_line(false).frame(egui::Frame::new().fill(p.container)).show(ui, |ui| self.top_bar(ui, &s));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.container).inner_margin(egui::Margin { left: 0, right: 16, top: 0, bottom: 16 })).show(ui, |ui| {
            egui::Frame::new().fill(p.background).corner_radius(16.0).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    egui::Frame::new().inner_margin(egui::Margin { left: 24, right: 24, top: 20, bottom: 24 }).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        match self.view {
                            View::Home => self.home(ui, &s),
                            View::Activity => self.activity_view(ui),
                            View::Notifications => self.notifications_view(ui),
                            View::Folders => self.folders_view(ui, &s),
                            View::Offline => self.offline_view(ui),
                            View::Shared => self.shared_view(ui, &s),
                            View::Settings => self.settings_view(ui, &s),
                        }
                    });
                });
            });
        });

        if self.quit_after_sync.is_some() {
            egui::Area::new(egui::Id::unique("quitting")).anchor(egui::Align2::CENTER_BOTTOM, vec2(NAV_WIDTH / 2.0, -28.0)).show(&ctx, |ui| {
                egui::Frame::new().fill(Color32::from_rgb(0x30, 0x30, 0x30)).corner_radius(8.0).inner_margin(egui::Margin::symmetric(16, 10)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(tr("Finishing syncing, then Google Drive will quit…")).color(Color32::from_rgb(0xf2, 0xf2, 0xf2)));
                        if ui.add(egui::Button::new(RichText::new(tr("Cancel")).color(Color32::from_rgb(0xa8, 0xc7, 0xfa))).frame(false)).clicked() {
                            self.quit_after_sync = None;
                        }
                    });
                });
            });
        }
        if self.needs_restart {
            egui::Area::new(egui::Id::unique("restart")).anchor(egui::Align2::CENTER_BOTTOM, vec2(NAV_WIDTH / 2.0, -28.0)).show(&ctx, |ui| {
                egui::Frame::new().fill(Color32::from_rgb(0x30, 0x30, 0x30)).corner_radius(8.0).inner_margin(egui::Margin::symmetric(16, 10)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tr("Some changes require Google Drive to restart")).color(Color32::from_rgb(0xf2, 0xf2, 0xf2)));
                        if ui.add(egui::Button::new(RichText::new(tr("Restart now")).color(Color32::from_rgb(0xa8, 0xc7, 0xfa))).frame(false)).clicked() {
                            self.engine.send(Cmd::Reconfigure);
                            self.needs_restart = false;
                        }
                    });
                });
            });
        }
        self.dialogs(&ctx);
        self.screenshot_step(&ctx);
        if ctx.input(|i| i.viewport().close_requested()) {
            let busy = {
                let sh = self.engine.shared.lock().unwrap();
                sh.job.is_some() || !sh.transfers.is_empty() || sh.pending > 0
            };
            if self.allow_close || self.screenshot.is_some() {
                self.engine.send(Cmd::Quit);
            } else if self.tray.is_some() && self.settings().close_keeps_running {
                // Chosen in Settings: keep running in the tray, like the original's menu-bar app.
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.hidden = true;
            } else if busy {
                // Close quits; ask first when files are still syncing ("Quit Google Drive?").
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.confirm_quit = true;
            } else {
                // Close quits the app.
                self.allow_close = true;
                self.engine.send(Cmd::Quit);
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let msgs: Vec<crate::tray::TrayMsg> = self.tray_rx.as_ref().map(|r| r.try_iter().collect()).unwrap_or_default();
        for m in msgs {
            use crate::tray::TrayMsg::*;
            match m {
                Action(name, path) => self.file_action(ctx, &name, &path),
                Open | Preferences => {
                    if m == Preferences {
                        self.go(View::Settings);
                    }
                    self.hidden = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TogglePause => {
                    let paused = self.settings().paused;
                    self.engine.set_paused(!paused);
                }
                Quit => self.quit(ctx),
            }
        }
        // "Copy link to clipboard": the clipboard is set during a frame, so ask for one even while hidden.
        if let Some(url) = self.link_to_copy.lock().unwrap().take() {
            ctx.copy_text(url);
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
            ctx.request_repaint();
            crate::engine::desktop_notify(tr("Link copied to the clipboard."), "");
        }
        // Safe quit: once uploads are done ("Quitting because there are no pending uploads.").
        if let Some(t) = self.quit_after_sync {
            let busy = {
                let sh = self.engine.shared.lock().unwrap();
                sh.job.is_some() || !sh.transfers.is_empty() || sh.pending > 0
            };
            if !busy && t.elapsed() > std::time::Duration::from_secs(4) {
                self.quit_after_sync = None;
                self.allow_close = true;
                self.engine.send(Cmd::Quit);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        self.restart_if_updated(ctx);
        // Keep the tray menu's pause item and tooltip current.
        let paused = self.settings().paused;
        if self.tray_paused != Some(paused) {
            if let Some(t) = &self.tray {
                t.update(|tr| tr.paused = paused);
            }
            self.tray_paused = Some(paused);
        }
        if self.hidden {
            // While hidden, wake up now and then so tray clicks are handled.
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.restart_at.is_some() {
            return;
        }
        self.engine.send(Cmd::Quit);
        std::thread::sleep(std::time::Duration::from_millis(800));
    }
}

// ---------------------------------------------------------------------- texts

/// How often the app looks for a newer copy of itself (estimated).
const UPDATE_CHECK_SECS: u64 = 30;

/// The new program file when the running one was replaced on disk.
fn updated_executable() -> Option<PathBuf> {
    let link = std::fs::read_link("/proc/self/exe").ok()?;
    let text = link.to_string_lossy();
    let path = PathBuf::from(text.strip_suffix(" (deleted)")?);
    path.is_file().then_some(path)
}

fn start_view() -> View {
    let args: Vec<String> = std::env::args().collect();
    match args.iter().position(|a| a == "--view").and_then(|i| args.get(i + 1)).map(|s| s.as_str()) {
        Some("activity") => View::Activity,
        Some("notifications") => View::Notifications,
        Some("folders") => View::Folders,
        Some("offline") => View::Offline,
        Some("shared") => View::Shared,
        Some("settings") => View::Settings,
        _ => View::Home,
    }
}

/// Height of one notification row on Home (the original's data-table rows are 77px).
const NOTICE_ROW_H: f32 = 77.0;

/// Text that always fits the space it's given: it wraps anywhere (long paths have no spaces to
/// break at), and past `rows` lines it ends with "…". It can never widen the card it sits in.
fn fit_text(ui: &mut egui::Ui, text: &str, size: f32, color: Color32, rows: usize) -> egui::Response {
    let width = ui.available_width().max(10.0);
    let font = egui::FontId::proportional(size);
    // Break inside a word only when a word (a path, an address) is wider than the space.
    let too_wide = text.split_whitespace().any(|w| ui.painter().layout_no_wrap(w.to_string(), font.clone(), color).size().x > width);
    let mut job = egui::text::LayoutJob::single_section(text.to_string(), egui::TextFormat::simple(font, color));
    job.wrap = egui::text::TextWrapping { max_width: width, max_rows: rows, break_anywhere: too_wide, overflow_character: Some('…') };
    let galley = ui.painter().layout_job(job);
    ui.add(egui::Label::new(galley).selectable(false))
}

fn ransom_title(who: &str) -> String {
    if is_vi() {
        format!("Tính năng đồng bộ hoá đã tạm dừng: Đã phát hiện phần mềm tống tiền trên {who}")
    } else {
        format!("Syncing paused: Ransomware detected on {who}")
    }
}

fn ransom_body(date: &str, time: &str) -> String {
    if is_vi() {
        format!("Nhiều tệp đã bị sửa đổi và tính năng đồng bộ hoá đã bị tạm dừng vào {time} ngày {date}. Nếu không nhận ra hoạt động này thì bạn cần phải tiến hành xử lý để tránh tệp bị hư hỏng nghiêm trọng hơn.")
    } else {
        format!("Multiple files were modified, and syncing was paused on {date} at {time}. If you don‘t recognize this activity, action needs to be taken to prevent further corruption.")
    }
}

fn status_texts(s: &Snap, st: &Settings, p: &Palette) -> (Icon, Color32, &'static str, String) {
    if s.account.is_none() {
        return if s.checked { (Icon::CloudOff, p.on_variant, tr("Offline"), tr("You are signed out").into()) } else { (Icon::Sync, p.primary, tr("Syncing items"), tr("Loading your files").into()) };
    }
    if s.ransomware {
        return (Icon::Error, p.red, tr("Paused"), if is_vi() { "Đã tạm dừng đồng bộ hóa: phát hiện phần mềm tống tiền".into() } else { "Syncing paused: Ransomware detected".into() });
    }
    if st.paused {
        let sub = match (is_vi(), s.pending) {
            (false, 0) => "Syncing Paused".to_string(),
            (false, 1) => "Syncing Paused. 1 file to sync.".to_string(),
            (false, n) => format!("Syncing Paused. {n} files to sync."),
            (true, 0) => "Đã tạm dừng đồng bộ hóa".to_string(),
            (true, n) => format!("Đã tạm dừng đồng bộ hóa. Còn {n} tệp cần đồng bộ hóa."),
        };
        return (Icon::Pause, p.on_variant, tr("Paused"), sub);
    }
    if s.transfers > 0 || s.job.is_some() {
        let n = s.pending.max(s.transfers as u64);
        let sub = match (is_vi(), n) {
            (false, 0) => tr("Fetching new changes").to_string(),
            (false, 1) => "Syncing 1 file".to_string(),
            (false, n) => format!("Syncing {n} files"),
            (true, 0) => tr("Fetching new changes").to_string(),
            (true, n) => format!("Đang đồng bộ hóa {n} tệp"),
        };
        return (Icon::Sync, p.primary, tr("Syncing items"), sub);
    }
    (Icon::Check, p.green, tr("Up to date"), tr("Everything is up to date").into())
}

/// Percentage of the cache limit in use, rounded down like the original (0x100e65c20).
pub fn cache_percent(used: u64, capacity: u64) -> u64 {
    used.saturating_mul(100).checked_div(capacity).unwrap_or(0)
}

fn cache_used_text(used: u64, capacity: Option<u64>) -> String {
    let vi = is_vi();
    match capacity {
        Some(c) => {
            let pct = cache_percent(used, c);
            if vi {
                format!("Bộ nhớ đệm đang dùng: {} ({}% giới hạn {})", theme::human_bytes(used), pct, theme::human_bytes(c))
            } else {
                format!("Content cache used: {} ({}% of {} limit)", theme::human_bytes(used), pct, theme::human_bytes(c))
            }
        }
        None => {
            if vi {
                format!("Bộ nhớ đệm đang dùng: {}", theme::human_bytes(used))
            } else {
                format!("Content cache used: {}", theme::human_bytes(used))
            }
        }
    }
}

fn action_row(ui: &mut egui::Ui, p: &Palette, icon: Icon, label: &str, enabled: bool) -> bool {
    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && resp.hovered() {
        ui.painter().rect_filled(row, 12.0, p.container_high);
    }
    let col = if enabled { p.on_variant } else { p.outline };
    icons::image(icon, 20.0, col).paint_at(ui, egui::Rect::from_center_size(row.left_center() + vec2(28.0, 0.0), vec2(20.0, 20.0)));
    ui.painter().text(row.left_center() + vec2(56.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(14.0), if enabled { p.on_surface } else { p.outline });
    enabled && resp.clicked()
}

/// A steady avatar colour per account.
fn account_color(label: &str) -> Color32 {
    const C: [Color32; 6] = [
        Color32::from_rgb(0x7b, 0x1f, 0xa2),
        Color32::from_rgb(0x0b, 0x57, 0xd0),
        Color32::from_rgb(0x14, 0x6c, 0x2e),
        Color32::from_rgb(0xb3, 0x26, 0x1e),
        Color32::from_rgb(0xe3, 0x74, 0x00),
        Color32::from_rgb(0x00, 0x79, 0x6b),
    ];
    let h = label.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    C[(h % 6) as usize]
}

fn storage_text(used: u64, total: u64) -> String {
    if total == 0 {
        return theme::human_bytes(used);
    }
    if is_vi() {
        format!("Đã dùng {} trong tổng số {}", theme::human_bytes(used), theme::human_bytes(total))
    } else {
        format!("{} of {} used", theme::human_bytes(used), theme::human_bytes(total))
    }
}

fn folders_text(n: usize) -> String {
    match (is_vi(), n) {
        (false, 0) => "No folders will sync with Google Drive".into(),
        (false, 1) => "1 folder".into(),
        (false, n) => format!("{n} folders"),
        (true, 0) => "Sẽ không có thư mục nào đồng bộ hóa với Google Drive".into(),
        (true, n) => format!("{n} thư mục"),
    }
}

fn pct_text(upload: bool, pct: f32) -> String {
    match (is_vi(), upload) {
        (false, true) => format!("Uploading {:.0}%", pct),
        (false, false) => format!("Downloading {:.0}%", pct),
        (true, true) => format!("Đang tải lên {:.0}%", pct),
        (true, false) => format!("Đang tải xuống {:.0}%", pct),
    }
}

fn ago(t: SystemTime) -> String {
    let s = t.elapsed().map(|d| d.as_secs()).unwrap_or(0);
    let vi = is_vi();
    match s {
        0..=59 => tr("Just now").into(),
        60..=3599 => {
            if vi {
                format!("{} phút trước", s / 60)
            } else {
                format!("{} min ago", s / 60)
            }
        }
        3600..=86399 => {
            if vi {
                format!("{} giờ trước", s / 3600)
            } else {
                format!("{} hr ago", s / 3600)
            }
        }
        _ => {
            if vi {
                format!("{} ngày trước", s / 86400)
            } else {
                format!("{} days ago", s / 86400)
            }
        }
    }
}

// ---------------------------------------------------------------------- compatibility

/// What this Linux version shares with the original (Drive for desktop 133) and what it doesn't.
/// Shown in Settings and About so nobody has to guess.
const COMPAT_SAME: &[(&str, &str)] = &[
    ("Same Google Drive accounts and files: everything you change shows up in Drive on the web and on other computers.", "Cùng tài khoản và tệp Google Drive: mọi thay đổi đều hiện trên Drive web và các máy khác."),
    ("Home window: Home, Sync activity, Notifications, Connected folders, Offline files, Shared with me, Settings — same layout, wording and colours (English, optional Vietnamese, light/dark).", "Cửa sổ Home: Trang chủ, Hoạt động đồng bộ hóa, Thông báo, Thư mục đã kết nối, Tệp ngoại tuyến, Được chia sẻ với tôi, Cài đặt — cùng bố cục, câu chữ và màu (tiếng Anh, có tiếng Việt, sáng/tối)."),
    ("Up to 4 accounts, account card with profile picture, storage, add/disconnect account.", "Tối đa 4 tài khoản, thẻ tài khoản có ảnh đại diện, dung lượng, thêm/ngắt kết nối tài khoản."),
    ("Stream files (“My Drive” + “Shared drives”) and Mirror files, with the original's folder checks, 10-second Drive change checks and “Lost and Found” for conflicts.", "Truy cập trực tuyến (“My Drive” + “Shared drives”) và Phản chiếu tệp, cùng các kiểm tra thư mục, kiểm tra thay đổi Drive mỗi 10 giây và “Lost and Found” khi xung đột như bản gốc."),
    ("Folders from this computer, USB devices (asked when plugged in, remembered by device ID) and network storage (every 6 hours); the original's ignored-file list and photo/video types.", "Thư mục trên máy, ổ USB (hỏi khi cắm, ghi nhớ theo mã ổ) và ổ mạng (6 giờ/lần); cùng danh sách tệp bị bỏ qua và loại ảnh/video như bản gốc."),
    ("Pause/resume, speed limits, cache limit, launch on login, proxy, low-disk-space stop (500 MB).", "Tạm dừng/tiếp tục, giới hạn tốc độ, giới hạn bộ nhớ đệm, chạy khi đăng nhập, proxy, dừng khi đĩa còn dưới 500 MB."),
    ("Tray icon and running in the background, one running copy, “Quit Google Drive?” with finish-syncing-then-quit, notifications with “Don’t show this again”, history kept between runs.", "Biểu tượng khay và chạy nền, chỉ một bản chạy, hỏi “Quit Google Drive?” với tùy chọn đồng bộ xong rồi thoát, thông báo có “Không hiện lại”, lịch sử được giữ khi mở lại."),
    ("Search box over all accounts (instead of macOS Spotlight).", "Ô tìm kiếm trên mọi tài khoản (thay cho Spotlight của macOS)."),
    ("Shared drive syncing: choose shared drives to mirror in Mirror mode (all are streamed in Stream mode).", "Đồng bộ hóa bộ nhớ dùng chung: chọn bộ nhớ dùng chung để phản chiếu ở chế độ Phản chiếu (ở chế độ trực tuyến, tất cả đều truy cập được)."),
    ("Help menu: “Generate diagnostic info” (a zip on the desktop with logs and settings, never sign-in keys) and “Enable verbose logging”; logs kept as 30 files of 5 MB.", "Menu Trợ giúp: “Tạo thông tin chẩn đoán” (tệp zip trên Màn hình nền gồm nhật ký và cài đặt, không bao giờ có khoá đăng nhập) và “Bật tính năng ghi nhật ký chi tiết”; nhật ký giữ 30 tệp, mỗi tệp 5 MB."),
    ("Sync status badges on files, as in Finder: synced, syncing, can't sync (ignored files), the Drive folder itself, and for streamed files available offline or online only.", "Biểu tượng trạng thái đồng bộ trên tệp như trong Finder: đã đồng bộ, đang đồng bộ, không đồng bộ được (tệp bị bỏ qua), chính thư mục Drive, và với tệp truy cập trực tuyến: có thể dùng khi không có mạng hoặc chỉ trực tuyến."),
    ("“Add shortcut to Google Drive” for items in Shared with me: pick a folder inside your Drive folder and the shortcut is created there.", "“Thêm lối tắt vào Google Drive” cho mục trong Được chia sẻ với tôi: chọn một thư mục trong thư mục Drive và lối tắt được tạo ở đó."),
    ("Ransomware protection: syncing pauses with the original's warning, “Take action” and “I recognize it” → “Allow sync”; a clear made within an hour wins over a new alert.", "Bảo vệ khỏi mã độc tống tiền: tạm dừng đồng bộ với cảnh báo như bản gốc, “Tiến hành xử lý” và “Tôi nhận ra hoạt động này” → “Cho phép đồng bộ hoá”; đã xử lý trong vòng một giờ thì cảnh báo mới bị bỏ qua."),
    ("File manager menu (Dolphin, GNOME Files): Copy link, Share, Manage versions, Open with Google Drive, Available offline / Online only.", "Menu chuột phải trong trình quản lý tệp (Dolphin, GNOME Files): Sao chép liên kết, Chia sẻ, Quản lý phiên bản, Mở bằng Google Drive, Dùng khi không có mạng / Chỉ trực tuyến."),
];
const COMPAT_DIFF: &[(&str, &str)] = &[
    ("Not the original program: its code is Mac-only and signs in with Google's private app key, so this is a new Linux app using rclone (open source) for sign-in and transfers.", "Không phải chương trình gốc: mã gốc chỉ chạy trên Mac và đăng nhập bằng khóa riêng của Google, nên đây là ứng dụng Linux mới dùng rclone (mã nguồn mở) để đăng nhập và truyền tệp."),
    ("Not in this version — “Add shortcut to Google Drive” in the file manager menu. Sync status badges show in Dolphin after signing out and back in once; GNOME Files shows them only with the nautilus-python package; other file managers don't show them.", "Chưa có — “Thêm lối tắt vào Google Drive” trong menu chuột phải. Biểu tượng trạng thái đồng bộ hiện trong Dolphin sau khi đăng xuất và đăng nhập lại một lần; GNOME Files chỉ hiện khi có gói nautilus-python; các trình quản lý tệp khác chưa hiện."),
    ("Not in this version — Google Photos “Upload size” (Storage saver), search hotkey, Gemini features. “Send feedback” opens Google's help site instead of sending a report with logs.", "Chưa có — “Kích thước tải lên” của Google Photos (Tiết kiệm bộ nhớ), phím tắt tìm kiếm, tính năng Gemini. “Gửi phản hồi” mở trang trợ giúp của Google thay vì gửi báo cáo kèm nhật ký."),
    ("Ransomware: the original is warned by Google's servers; this app decides itself when 50 or more synced files (at least 10% of a folder, or 500 at once) are changed or removed on this computer in Mirror mode, and holds the changes back. Streamed files and connected folders aren't checked.", "Mã độc tống tiền: bản gốc được máy chủ Google cảnh báo; ứng dụng này tự phát hiện khi từ 50 tệp đã đồng bộ trở lên (ít nhất 10% thư mục, hoặc 500 tệp cùng lúc) bị sửa hoặc xoá trên máy ở chế độ Phản chiếu, và giữ lại các thay đổi. Tệp truy cập trực tuyến và thư mục đã kết nối chưa được kiểm tra."),
    ("“Available offline” keeps files by reading them into the cache (checked every 5 minutes, at most 50 files per check, each again after 4 hours, like the original), not a true pin: with a small cache limit they can still be dropped.", "“Có thể dùng khi không có mạng” giữ tệp bằng cách đọc chúng vào bộ nhớ đệm (kiểm tra mỗi 5 phút, tối đa 50 tệp mỗi lần, mỗi tệp đọc lại sau 4 giờ, như bản gốc), không phải ghim thật: nếu giới hạn bộ nhớ đệm nhỏ, tệp vẫn có thể bị xóa khỏi bộ nhớ đệm."),
    ("Google Photos backup needs “Get the latest rclone” (Settings); whether Google accepts the sign-in is only known by trying.", "Sao lưu Google Photos cần bấm “Tải rclone mới nhất” (Cài đặt); Google có chấp nhận đăng nhập hay không chỉ biết khi thử."),
    ("Folders from your computer go to “Linux computers” in My Drive: Drive's separate “Computers” area can only be created by Google's own app.", "Thư mục trên máy được đưa vào “Linux computers” trong Drive của tôi: khu vực “Máy tính” riêng chỉ ứng dụng của Google tạo được."),
    ("Mac/Windows-only parts aren't possible on Linux: Microsoft Office real-time presence, Outlook and Meet add-ins, Apple Photos library, Finder/Spotlight integration.", "Phần chỉ có trên Mac/Windows không làm được trên Linux: Hiện diện thời gian thực của Microsoft Office, tiện ích Outlook và Meet, thư viện Apple Photos, tích hợp Finder/Spotlight."),
    ("Not in this version — the file manager menu (Copy link, Share, versions…) doesn't recognise files inside mirrored shared drives yet; it works in My Drive, streamed folders and connected folders.", "Chưa có — menu chuột phải (Sao chép liên kết, Chia sẻ, phiên bản…) chưa nhận ra tệp trong bộ nhớ dùng chung đang phản chiếu; vẫn dùng được trong Drive của tôi, thư mục truy cập trực tuyến và thư mục đã kết nối."),
    ("Google answers this app through rclone's shared sign-in key, which many people use, so Drive can briefly refuse requests (“Quota exceeded”); the original uses Google's own key.", "Google trả lời ứng dụng này qua khóa đăng nhập dùng chung của rclone (nhiều người cùng dùng), nên Drive đôi khi tạm từ chối yêu cầu (“Quota exceeded”); bản gốc dùng khóa riêng của Google."),
    ("Its own name and logo (Drive For Linux: a cloud carrying a terminal prompt) instead of Google's.", "Tên và logo riêng (Drive For Linux: đám mây mang dấu nhắc lệnh) thay cho của Google."),
];

fn compat_list(ui: &mut egui::Ui, p: &Palette) {
    let vi = is_vi();
    ui.label(
        RichText::new(if vi {
            "Chưa tương thích 100%. Dùng chung tài khoản và tệp, có các tính năng chính của bản gốc, nhưng một số tính năng bên dưới vẫn còn thiếu."
        } else {
            "Not 100% compatible. It uses the same accounts and files and covers the original's main features, but some features below are still missing."
        })
        .strong(),
    );
    ui.add_space(6.0);
    ui.label(RichText::new(if vi { "Giống bản gốc" } else { "Works the same" }).color(p.green).strong());
    for (en, v) in COMPAT_SAME {
        ui.horizontal_wrapped(|ui| {
            icons::show(ui, Icon::Check, 16.0, p.green);
            ui.label(if vi { *v } else { *en });
        });
    }
    ui.add_space(6.0);
    ui.label(RichText::new(if vi { "Khác bản gốc" } else { "Different or missing" }).color(p.red).strong());
    for (en, v) in COMPAT_DIFF {
        ui.horizontal_wrapped(|ui| {
            icons::show(ui, Icon::Error, 16.0, p.red);
            ui.label(if vi { *v } else { *en });
        });
    }
}

// ---------------------------------------------------------------------- widgets

fn nav_item(ui: &mut egui::Ui, p: &Palette, selected: bool, icon: Icon, label: &str, badge: usize) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 24.0, p.selected);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 24.0, p.container_high);
    }
    let col = if selected { p.on_selected } else { p.on_variant };
    icons::image(icon, 22.0, col).paint_at(ui, egui::Rect::from_center_size(rect.left_center() + vec2(28.0, 0.0), vec2(22.0, 22.0)));
    let font = egui::FontId::proportional(14.5);
    ui.painter().text(rect.left_center() + vec2(56.0, 0.0), egui::Align2::LEFT_CENTER, label, font.clone(), if selected { p.on_selected } else { p.on_surface });
    if badge > 0 {
        ui.painter().text(rect.right_center() - vec2(20.0, 0.0), egui::Align2::RIGHT_CENTER, badge.to_string(), font, col);
    }
    resp.clicked()
}

fn bento(ui: &mut egui::Ui, p: &Palette, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(p.container_low).stroke(Stroke::new(1.0, p.container_high)).corner_radius(16.0).inner_margin(20).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
    ui.add_space(12.0);
}

fn card_title(ui: &mut egui::Ui, p: &Palette, t: &str) {
    ui.label(RichText::new(t).size(14.0).strong().color(p.on_variant));
    ui.add_space(6.0);
}

fn page_title(ui: &mut egui::Ui, p: &Palette, t: &str) {
    ui.label(RichText::new(t).size(22.0).color(p.on_surface));
    ui.add_space(12.0);
}

fn section_title(ui: &mut egui::Ui, p: &Palette, t: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(t).size(16.0).strong().color(p.on_surface));
    ui.add_space(8.0);
}

fn setting_title(ui: &mut egui::Ui, t: &str) {
    ui.label(RichText::new(t).size(15.0).strong());
    ui.add_space(2.0);
}

fn sub_text(ui: &mut egui::Ui, p: &Palette, t: &str) {
    egui::Frame::new().inner_margin(egui::Margin { left: 26, right: 0, top: 0, bottom: 4 }).show(ui, |ui| {
        ui.label(RichText::new(t).small().color(p.on_variant));
    });
}

fn link_row(ui: &mut egui::Ui, p: &Palette, icon: Icon, label: &str, enabled: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), if enabled { Sense::click() } else { Sense::hover() });
    if resp.hovered() && enabled {
        ui.painter().rect_filled(rect, 8.0, p.container_high);
    }
    let col = if enabled { p.primary } else { p.outline };
    icons::image(icon, 20.0, col).paint_at(ui, egui::Rect::from_center_size(rect.left_center() + vec2(18.0, 0.0), vec2(20.0, 20.0)));
    ui.painter().text(rect.left_center() + vec2(42.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(14.0), if enabled { p.on_surface } else { p.outline });
    resp.clicked()
}

fn filled<'a>(t: &'a str, p: &Palette) -> egui::Button<'a> {
    egui::Button::new(RichText::new(t).color(p.on_primary)).fill(p.primary).stroke(Stroke::NONE).corner_radius(20.0)
}

fn outlined<'a>(t: &'a str, p: &Palette) -> egui::Button<'a> {
    egui::Button::new(RichText::new(t).color(p.primary)).fill(Color32::TRANSPARENT).stroke(Stroke::new(1.0, p.outline)).corner_radius(20.0)
}

fn text_button<'a>(t: &'a str, p: &Palette) -> egui::Button<'a> {
    egui::Button::new(RichText::new(t).color(p.primary)).frame(false)
}

fn mode_option(ui: &mut egui::Ui, p: &Palette, mode: &mut DriveMode, me: DriveMode, title: &str, bullets: &[&str]) {
    let sel = *mode == me;
    egui::Frame::new().stroke(Stroke::new(if sel { 2.0 } else { 1.0 }, if sel { p.primary } else { p.outline })).corner_radius(12.0).inner_margin(14).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.radio_value(mode, me, RichText::new(title).size(15.0).strong());
        egui::Frame::new().inner_margin(egui::Margin { left: 26, right: 0, top: 0, bottom: 0 }).show(ui, |ui| {
            for b in bullets {
                ui.label(RichText::new(*b).color(p.on_variant));
            }
        });
    });
}

// Column grid shared by a table's header and rows: an icon column, a name column that takes
// the remaining width, then fixed-width columns.
const ICON_COL: f32 = 48.0;
const ROW_HEIGHT: f32 = 48.0;
const ACTIVITY_COLS: [f32; 2] = [110.0, 220.0];
const OFFLINE_COLS: [f32; 1] = [110.0];
const SHARED_COLS: [f32; 3] = [120.0, 110.0, 88.0];

fn column_rects(row: egui::Rect, fixed: &[f32]) -> Vec<egui::Rect> {
    let name_w = (row.width() - ICON_COL - fixed.iter().sum::<f32>()).max(80.0);
    let mut x = row.left();
    let mut out = Vec::new();
    for w in [ICON_COL, name_w].into_iter().chain(fixed.iter().copied()) {
        out.push(egui::Rect::from_min_size(egui::pos2(x, row.top()), vec2(w, row.height())));
        x += w;
    }
    out
}

fn cell(ui: &mut egui::Ui, r: egui::Rect, add: impl FnOnce(&mut egui::Ui)) {
    let inner = r.shrink2(vec2(8.0, 0.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)), |ui| {
        ui.set_clip_rect(inner.intersect(ui.clip_rect()));
        add(ui);
    });
}

/// Header labels for the name column and each fixed column.
fn table_header(ui: &mut egui::Ui, p: &Palette, names: &[&str], fixed: &[f32]) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
    let rects = column_rects(row, fixed);
    for (i, name) in names.iter().enumerate() {
        if let Some(r) = rects.get(i + 1) {
            cell(ui, *r, |ui| {
                ui.add(egui::Label::new(RichText::new(*name).strong().color(p.on_variant)).truncate());
            });
        }
    }
    ui.painter().hline(row.x_range(), row.bottom(), Stroke::new(1.0, p.outline));
}

fn table_row(ui: &mut egui::Ui, p: &Palette, fixed: &[f32], mut add: impl FnMut(usize, &mut egui::Ui)) {
    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    if resp.hovered() {
        ui.painter().rect_filled(row, 0.0, p.container_low);
    }
    for (i, r) in column_rects(row, fixed).into_iter().enumerate() {
        cell(ui, r, |ui| add(i, ui));
    }
    ui.painter().hline(row.x_range(), row.bottom(), Stroke::new(1.0, p.container_high));
}

fn list_row(ui: &mut egui::Ui, p: &Palette, add: impl FnOnce(&mut egui::Ui)) {
    let w = ui.available_width();
    let resp = egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 8)).show(ui, |ui| {
        ui.set_width(w);
        ui.horizontal(add);
    });
    let r = resp.response.rect;
    ui.painter().hline(r.x_range(), r.bottom(), Stroke::new(1.0, p.container_high));
}

fn empty(ui: &mut egui::Ui, p: &Palette, icon: Icon, title: &str, body: &str) {
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        icons::show(ui, icon, 56.0, p.outline);
        ui.add_space(8.0);
        ui.label(RichText::new(title).size(18.0));
        if !body.is_empty() {
            ui.label(RichText::new(body).color(p.on_variant));
        }
    });
}

fn number_field(ui: &mut egui::Ui, p: &Palette, text: &mut String, unit: &str, min: u64, max: u64) -> Option<u64> {
    ui.horizontal(|ui| {
        ui.add_space(26.0);
        ui.add(egui::TextEdit::singleline(text).desired_width(120.0));
        ui.label(unit);
    });
    match text.trim().parse::<u64>() {
        Ok(v) if (min..=max).contains(&v) => Some(v),
        _ => {
            let msg = if is_vi() { format!("Nhập một giá trị nằm trong khoảng từ {min} đến {max}.") } else { format!("Enter a value between {min} and {max}.") };
            ui.horizontal(|ui| {
                ui.add_space(26.0);
                ui.label(RichText::new(msg).small().color(p.red));
            });
            None
        }
    }
}

fn file_icon(name: &str, p: &Palette) -> (Icon, Color32) {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "bmp" => (Icon::File, Color32::from_rgb(0xd9, 0x30, 0x25)),
        "mp4" | "mov" | "mkv" | "avi" | "webm" => (Icon::File, Color32::from_rgb(0xd9, 0x30, 0x25)),
        "pdf" => (Icon::File, Color32::from_rgb(0xea, 0x43, 0x35)),
        "doc" | "docx" | "odt" | "txt" | "md" => (Icon::File, Color32::from_rgb(0x42, 0x85, 0xf4)),
        "xls" | "xlsx" | "ods" | "csv" => (Icon::File, Color32::from_rgb(0x0f, 0x9d, 0x58)),
        "ppt" | "pptx" | "odp" => (Icon::File, Color32::from_rgb(0xf4, 0xb4, 0x00)),
        _ if !name.contains('.') => (Icon::Folder, p.on_variant),
        _ => (Icon::File, p.on_variant),
    }
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{:02X}", b),
        })
        .collect()
}

/// X11 window class ("drive-linux", "drive-linux"), set while the window is still hidden (eframe
/// shows it after its first frame). egui writes the class as ("", app id); with the first part
/// empty Plasma's taskbar can't tie the window to drive-linux.desktop and shows the icon theme's
/// generic "drive" picture instead of the logo.
static CLASS_SET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CLASS_TRIES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Returns true once the class is in place (or there is nothing to do, as on Wayland).
fn set_window_class(win: &impl raw_window_handle::HasWindowHandle) -> bool {
    use raw_window_handle::RawWindowHandle;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
    use x11rb::wrapper::ConnectionExt as _;
    const CLASS: &[u8] = b"drive-linux\0drive-linux\0";
    let Ok(handle) = win.window_handle() else { return false };
    let window = match handle.as_raw() {
        RawWindowHandle::Xlib(h) => h.window as u32,
        RawWindowHandle::Xcb(h) => h.window.get(),
        _ => return true, // Wayland: the app id already names the app
    };
    let conn = match x11rb::connect(None) {
        Ok((conn, _)) => conn,
        Err(e) => {
            crate::diag::log_event("warning", &format!("Couldn't name the window for the taskbar: {e}"));
            return false;
        }
    };
    let _ = conn.change_property8(PropMode::REPLACE, window, AtomEnum::WM_CLASS, AtomEnum::STRING, CLASS);
    // Read it back: this also waits until the X server has applied the change.
    let ok = conn.get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 64).ok().and_then(|c| c.reply().ok()).is_some_and(|r| r.value == CLASS);
    if !ok {
        crate::diag::log_event("warning", &format!("The window's taskbar name didn't take (window {window:#x})"));
    }
    ok
}

fn open(target: &str) {
    let _ = std::process::Command::new("xdg-open").arg(target).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_use_is_rounded_down_like_the_original() {
        assert_eq!(cache_percent(0, 0), 0, "no limit known");
        assert_eq!(cache_percent(999, 1000), 99);
        assert_eq!(cache_percent(1000, 1000), 100);
        assert_eq!(cache_percent(u64::MAX, 10), u64::MAX / 10, "never overflows");
    }

    #[test]
    fn ransomware_texts_use_the_originals_wording() {
        assert_eq!(ransom_title("me@example.com"), "Syncing paused: Ransomware detected on me@example.com");
        assert!(ransom_body("10 Nov 2026", "14:05").starts_with("Multiple files were modified, and syncing was paused on 10 Nov 2026 at 14:05."));
    }

    /// Long paths and titles must never make a card wider than its column (what broke Home
    /// when notifications arrived), and a row limit cuts the text with "…".
    #[test]
    fn notification_text_never_widens_its_card() {
        let ctx = egui::Context::default();
        let long = "Shortcut added at /mnt/DATA2/testdrive/Projects/2026/Very-long-folder-name-without-any-spaces-at-all/report_final_v12.xlsx";
        let mut sizes = Vec::new();
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            ui.allocate_ui(vec2(200.0, 400.0), |ui| {
                ui.set_max_width(200.0);
                sizes.push(fit_text(ui, long, 12.0, Color32::BLACK, usize::MAX).rect.size());
                sizes.push(fit_text(ui, long, 12.0, Color32::BLACK, 2).rect.size());
                sizes.push(fit_text(ui, "Created a shortcut", 14.0, Color32::BLACK, 1).rect.size());
            });
        });
        out.textures_delta.clear(); // nothing draws in a test
        assert!(sizes.iter().all(|s| s.x <= 200.5), "too wide: {sizes:?}");
        assert!(sizes[1].y < sizes[0].y, "two lines at most when limited");
        assert!(sizes[2].y < 25.0, "a short title stays on one line");
    }

    #[test]
    fn this_build_is_not_reported_as_replaced() {
        assert!(updated_executable().is_none());
    }
}
