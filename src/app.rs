use std::f32::consts::TAU;
use std::time::{Duration, Instant};

use egui::{
    Align, Align2, Color32, CursorIcon, FontData, FontDefinitions, FontFamily, FontId, Frame, Id,
    Key, Layout, Margin, Order, Pos2, Rect, RichText, Rounding, ScrollArea, Sense, Shape, Stroke,
    TextureHandle, TextureOptions, Ui, Vec2, ViewportCommand, WindowLevel,
};

use crate::config::AppConfig;
use crate::display::{
    apply_resolution, enumerate_monitors, query_current_mode, restore_native, DisplayMode,
    MonitorInfo,
};
use crate::hotkeys::{GlobalHotkeyListener, HotkeyAction};
use crate::integration::{is_in_start_menu, register_in_start_menu, remove_from_start_menu};
use crate::presets::{builtin_presets, format_aspect_ratio, PresetCategory, ResolutionPreset};
use crate::toast::ScreenToast;
use crate::updater::{apply_downloaded_update, ReleaseInfo, UpdateEvent, UpdateManager};
use crate::window_stretch::{
    apply_borderless_window, find_foreground_window, get_window_center, scan_candidate_windows,
    CandidateWindow,
};

// Paleta minimalista en tonos grafito, pizarra, índigo suave y verde salvia
const BG_DARK: Color32 = Color32::from_rgb(18, 19, 22);
const BG_PANEL: Color32 = Color32::from_rgb(23, 25, 30);
const BG_CARD: Color32 = Color32::from_rgb(28, 31, 38);
const BG_CARD_HOVER: Color32 = Color32::from_rgb(38, 42, 53);
const BG_CARD_ACTIVE: Color32 = Color32::from_rgb(29, 38, 55);

const ACCENT: Color32 = Color32::from_rgb(110, 142, 247);
const ACCENT_HOVER: Color32 = Color32::from_rgb(128, 157, 252);
const ACCENT_SOFT: Color32 = Color32::from_rgb(142, 168, 250);
const STATUS_OK: Color32 = Color32::from_rgb(86, 182, 159);
const STATUS_WARN: Color32 = Color32::from_rgb(218, 165, 85);

const TEXT_PRIMARY: Color32 = Color32::from_rgb(236, 238, 242);
const TEXT_MUTED: Color32 = Color32::from_rgb(148, 155, 166);
const TEXT_DIM: Color32 = Color32::from_rgb(99, 106, 117);
const BORDER: Color32 = Color32::from_rgb(40, 44, 53);
const BORDER_HOVER: Color32 = Color32::from_rgb(68, 76, 94);

const SAFE_CONFIRM_SECONDS: f32 = 12.0;

/// Interpola suavemente entre dos colores para animaciones fluidas.
fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0_f32, 1.0_f32);
    let mix = |x: u8, y: u8| -> u8 { (x as f32 + (y as f32 - x as f32) * t).round() as u8 };
    Color32::from_rgba_unmultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

/// Dibuja una estrella vectorial de 5 puntas sin depender de fuentes externas.
fn draw_star(painter: &egui::Painter, center: Pos2, radius: f32, filled: bool, color: Color32) {
    let inner = radius * 0.44_f32;
    let mut pts = Vec::with_capacity(10);
    for i in 0..10 {
        let angle = -TAU * 0.25_f32 + (i as f32) * (TAU / 10.0_f32);
        let r = if i % 2 == 0 { radius } else { inner };
        pts.push(Pos2::new(
            center.x + r * angle.cos(),
            center.y + r * angle.sin(),
        ));
    }

    if filled {
        painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
    } else {
        pts.push(pts[0]);
        painter.add(Shape::line(pts, Stroke::new(1.1_f32, color)));
    }
}

/// Dibuja una equis limpia para eliminar presets personalizados.
fn draw_cross(painter: &egui::Painter, center: Pos2, half: f32, color: Color32) {
    let stroke = Stroke::new(1.3_f32, color);
    painter.line_segment(
        [
            Pos2::new(center.x - half, center.y - half),
            Pos2::new(center.x + half, center.y + half),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(center.x + half, center.y - half),
            Pos2::new(center.x - half, center.y + half),
        ],
        stroke,
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusKind {
    Ready,
    Success,
    Warning,
    Error,
}

struct PendingConfirmation {
    device_name: String,
    previous_mode: DisplayMode,
    started_at: Instant,
}

/// Estado principal de la interfaz de AspectShift.
pub struct AspectShiftApp {
    config: AppConfig,
    monitors: Vec<MonitorInfo>,
    selected_monitor: usize,
    selected_hz: Option<u32>,

    selected_category: PresetCategory,
    category_changed_at: Instant,
    builtin: Vec<ResolutionPreset>,

    custom_w_input: String,
    custom_h_input: String,

    candidate_windows: Vec<CandidateWindow>,
    // 0 significa ninguna ventana seleccionada; 1.. corresponde a candidate_windows[idx - 1]
    selected_window_idx: usize,

    status_text: String,
    status_kind: StatusKind,
    show_options_panel: bool,
    show_guide_modal: bool,
    last_mode_poll: Instant,

    pending_confirm: Option<PendingConfirmation>,
    screen_toast: ScreenToast,

    update_manager: UpdateManager,
    available_update: Option<ReleaseInfo>,
    update_dismissed: bool,
    update_checking: bool,
    update_downloading: bool,
    in_start_menu: bool,

    logo_texture: Option<TextureHandle>,
    hotkey_listener: GlobalHotkeyListener,
}

impl AspectShiftApp {
    pub fn new(cc: &eframe::CreationContext<'_>, icon_rgba_64: &[u8]) -> Self {
        Self::configure_fonts(&cc.egui_ctx);
        Self::configure_theme(&cc.egui_ctx);

        let config = AppConfig::load();
        let monitors = enumerate_monitors();
        let candidate_windows = scan_candidate_windows();

        let logo_texture = if icon_rgba_64.len() == 64 * 64 * 4 {
            let image = egui::ColorImage::from_rgba_unmultiplied([64, 64], icon_rgba_64);
            Some(cc.egui_ctx.load_texture(
                "aspectshift_logo",
                image,
                TextureOptions::LINEAR,
            ))
        } else {
            None
        };

        let hotkey_listener = GlobalHotkeyListener::start(cc.egui_ctx.clone());
        let (status_text, status_kind) = if !hotkey_listener.conflicts.is_empty() {
            (
                format!(
                    "Atajo ocupado por otra app: {}",
                    hotkey_listener.conflicts.join(", ")
                ),
                StatusKind::Warning,
            )
        } else if let Some(m) = monitors.first() {
            (
                format!(
                    "{} × {} @ {} Hz",
                    m.current_mode.width, m.current_mode.height, m.current_mode.hz
                ),
                StatusKind::Ready,
            )
        } else {
            ("Listo".to_string(), StatusKind::Ready)
        };

        let screen_toast = ScreenToast::new();
        let update_manager = UpdateManager::new();
        update_manager.check_for_updates(cc.egui_ctx.clone(), false);

        Self {
            custom_w_input: config.last_stretch_res.0.to_string(),
            custom_h_input: config.last_stretch_res.1.to_string(),
            config,
            monitors,
            selected_monitor: 0,
            selected_hz: None,
            selected_category: PresetCategory::Featured,
            category_changed_at: Instant::now() - Duration::from_secs(1),
            builtin: builtin_presets(),
            candidate_windows,
            selected_window_idx: 0,
            status_text,
            status_kind,
            show_options_panel: false,
            show_guide_modal: false,
            last_mode_poll: Instant::now(),
            pending_confirm: None,
            screen_toast,
            update_manager,
            available_update: None,
            update_dismissed: false,
            update_checking: true,
            update_downloading: false,
            in_start_menu: is_in_start_menu(),
            logo_texture,
            hotkey_listener,
        }
    }

    /// Carga únicamente Segoe UI desde Windows (con respaldo a Arial) para ahorrar memoria y espacio.
    fn configure_fonts(ctx: &egui::Context) {
        let mut fonts = FontDefinitions::default();
        let font_bytes = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf")
            .or_else(|_| std::fs::read(r"C:\Windows\Fonts\arial.ttf"));

        if let Ok(bytes) = font_bytes {
            fonts
                .font_data
                .insert("ui_sans".to_owned(), FontData::from_owned(bytes));
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "ui_sans".to_owned());
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .insert(0, "ui_sans".to_owned());
        }

        ctx.set_fonts(fonts);
    }

    fn configure_theme(ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        style.animation_time = 0.22_f32;
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = BG_DARK;
        style.visuals.window_fill = BG_PANEL;
        style.visuals.extreme_bg_color = Color32::from_rgb(14, 15, 18);
        style.visuals.faint_bg_color = BG_CARD;
        style.visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(110, 142, 247, 56);
        style.visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);

        style.visuals.widgets.noninteractive.bg_fill = BG_CARD;
        style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
        style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);

        style.visuals.widgets.inactive.bg_fill = BG_CARD;
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
        style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
        style.visuals.widgets.inactive.rounding = Rounding::same(6.0_f32);

        style.visuals.widgets.hovered.bg_fill = BG_CARD_HOVER;
        style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, BORDER_HOVER);
        style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
        style.visuals.widgets.hovered.rounding = Rounding::same(6.0_f32);

        style.visuals.widgets.active.bg_fill = ACCENT;
        style.visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT_SOFT);
        style.visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
        style.visuals.widgets.active.rounding = Rounding::same(6.0_f32);

        style.spacing.item_spacing = Vec2::new(6.0_f32, 6.0_f32);
        style.spacing.button_padding = Vec2::new(9.0_f32, 4.0_f32);
        ctx.set_style(style);
    }

    /// Dibuja un botón minimalista con transiciones suaves de color, borde y presión.
    fn animated_button(
        ui: &mut Ui,
        id_salt: impl std::hash::Hash,
        label: &str,
        font_size: f32,
        primary: bool,
        active: bool,
        enabled: bool,
    ) -> egui::Response {
        let btn_id = Id::new(id_salt);
        let galley = ui.painter().layout_no_wrap(
            label.to_string(),
            FontId::new(font_size, FontFamily::Proportional),
            TEXT_PRIMARY,
        );
        let size = Vec2::new(
            (galley.size().x + 18.0_f32).max(44.0_f32),
            (galley.size().y + 10.0_f32).max(24.0_f32),
        );
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(size, sense);

        let hover_t = ui.ctx().animate_bool_with_time(
            btn_id.with("hover"),
            enabled && response.hovered(),
            0.18_f32,
        );
        let active_t =
            ui.ctx()
                .animate_bool_with_time(btn_id.with("active"), active, 0.20_f32);
        let press_t = ui.ctx().animate_bool_with_time(
            btn_id.with("press"),
            enabled && response.is_pointer_button_down_on(),
            0.09_f32,
        );

        if enabled && response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }

        let visual_rect = rect.translate(Vec2::new(0.0_f32, 0.8_f32 * press_t));
        let (base_fill, hover_fill, base_stroke, hover_stroke, base_text, hover_text) = if primary
        {
            (
                ACCENT,
                ACCENT_HOVER,
                ACCENT,
                ACCENT_SOFT,
                Color32::WHITE,
                Color32::WHITE,
            )
        } else {
            (
                BG_CARD,
                BG_CARD_HOVER,
                BORDER,
                BORDER_HOVER,
                TEXT_MUTED,
                TEXT_PRIMARY,
            )
        };

        let mut fill = lerp_color(base_fill, hover_fill, hover_t);
        let mut stroke_col = lerp_color(base_stroke, hover_stroke, hover_t);
        let mut text_col = lerp_color(base_text, hover_text, hover_t);

        if !primary {
            fill = lerp_color(fill, BG_CARD_ACTIVE, active_t);
            stroke_col = lerp_color(stroke_col, ACCENT, active_t);
            text_col = lerp_color(text_col, TEXT_PRIMARY, active_t);
        }

        if !enabled {
            fill = BG_PANEL;
            stroke_col = BORDER;
            text_col = TEXT_DIM;
        } else if press_t > 0.01_f32 {
            fill = lerp_color(fill, BG_DARK, 0.14_f32 * press_t);
        }

        let painter = ui.painter_at(visual_rect.expand(1.0_f32));
        painter.rect(
            visual_rect,
            Rounding::same(6.0_f32),
            fill,
            Stroke::new(1.0_f32, stroke_col),
        );
        painter.text(
            visual_rect.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::new(font_size, FontFamily::Proportional),
            text_col,
        );

        response
    }

    fn current_monitor(&self) -> Option<&MonitorInfo> {
        self.monitors.get(self.selected_monitor)
    }

    fn refresh_live_mode(&mut self) {
        if let Some(mon) = self.monitors.get_mut(self.selected_monitor) {
            if let Some(live) = query_current_mode(&mon.device_name) {
                mon.current_mode = live;
            }
        }
    }

    fn full_refresh_devices_and_windows(&mut self) {
        let prev_dev = self
            .current_monitor()
            .map(|m| m.device_name.clone())
            .unwrap_or_default();
        self.monitors = enumerate_monitors();
        if let Some(pos) = self.monitors.iter().position(|m| m.device_name == prev_dev) {
            self.selected_monitor = pos;
        } else {
            self.selected_monitor = 0;
        }
        self.candidate_windows = scan_candidate_windows();
        if self.selected_window_idx > self.candidate_windows.len() {
            self.selected_window_idx = 0;
        }
    }

    fn notify_resolution_changed(&self, mode: DisplayMode, is_native: bool) {
        let label = if is_native {
            format!("Nativa · {} × {} · {} Hz", mode.width, mode.height, mode.hz)
        } else {
            format!("{} × {} · {} Hz", mode.width, mode.height, mode.hz)
        };
        self.screen_toast
            .show(mode.pos_x, mode.pos_y, mode.width, label, is_native);
    }

    /// Cambia la resolución únicamente del monitor elegido sin alterar ni mover ventanas abiertas.
    fn trigger_apply_resolution(&mut self, width: u32, height: u32) {
        let Some(mon) = self.current_monitor().cloned() else {
            self.status_text = "No se encontró ningún monitor activo.".to_string();
            self.status_kind = StatusKind::Error;
            return;
        };

        let previous_mode = mon.current_mode;
        let target_hz = self.selected_hz.or(Some(mon.current_mode.hz));
        match apply_resolution(
            &mon.device_name,
            width,
            height,
            target_hz,
            self.config.force_gpu_stretch_flag,
        ) {
            Ok(res) => {
                if let Some(m) = self.monitors.get_mut(self.selected_monitor) {
                    m.current_mode = res.applied_mode;
                }
                let ratio = width as f32 / height.max(1) as f32;
                let is_native_ratio = (ratio - 16.0_f32 / 9.0_f32).abs() <= 0.03_f32;
                if !is_native_ratio {
                    self.config.last_stretch_res = (width, height);
                    self.config.save();
                }

                if self.config.confirm_resolution_change
                    && (previous_mode.width != res.applied_mode.width
                        || previous_mode.height != res.applied_mode.height)
                {
                    self.pending_confirm = Some(PendingConfirmation {
                        device_name: mon.device_name,
                        previous_mode,
                        started_at: Instant::now(),
                    });
                } else {
                    self.pending_confirm = None;
                }

                self.notify_resolution_changed(res.applied_mode, is_native_ratio);
                self.status_text = res.message;
                self.status_kind = StatusKind::Success;
            }
            Err(err_msg) => {
                self.status_text = err_msg;
                self.status_kind = StatusKind::Warning;
            }
        }
    }

    fn trigger_restore_native(&mut self) {
        self.pending_confirm = None;
        let Some(mon) = self.current_monitor().cloned() else {
            return;
        };

        match restore_native(&mon.device_name, Some(mon.native_mode)) {
            Ok(res) => {
                if let Some(m) = self.monitors.get_mut(self.selected_monitor) {
                    m.current_mode = res.applied_mode;
                }
                self.notify_resolution_changed(res.applied_mode, true);
                self.status_text = res.message;
                self.status_kind = StatusKind::Success;
            }
            Err(err_msg) => {
                self.status_text = err_msg;
                self.status_kind = StatusKind::Error;
            }
        }
    }

    fn revert_pending_confirmation(&mut self) {
        let Some(pending) = self.pending_confirm.take() else {
            return;
        };
        let prev = pending.previous_mode;
        match apply_resolution(
            &pending.device_name,
            prev.width,
            prev.height,
            Some(prev.hz),
            false,
        ) {
            Ok(res) => {
                if let Some(m) = self
                    .monitors
                    .iter_mut()
                    .find(|m| m.device_name == pending.device_name)
                {
                    m.current_mode = res.applied_mode;
                }
                self.notify_resolution_changed(res.applied_mode, false);
                self.status_text = format!(
                    "Revertida automáticamente a {} × {} @ {} Hz",
                    res.applied_mode.width, res.applied_mode.height, res.applied_mode.hz
                );
                self.status_kind = StatusKind::Warning;
            }
            Err(err) => {
                self.status_text = err;
                self.status_kind = StatusKind::Error;
            }
        }
    }

    /// Aplica el modo sin bordes conservando la ventana en el mismo monitor donde ya se encuentra.
    fn trigger_adjust_window(&mut self, from_hotkey: bool) {
        self.refresh_live_mode();
        let target = if from_hotkey && self.selected_window_idx == 0 {
            find_foreground_window()
        } else if self.selected_window_idx > 0 {
            self.candidate_windows
                .get(self.selected_window_idx - 1)
                .cloned()
        } else {
            None
        };

        let Some(win) = target else {
            self.status_text = if from_hotkey {
                "Enfoca la ventana que deseas ajustar antes de usar Ctrl+Alt+B.".to_string()
            } else {
                "Elige una ventana en la lista desplegable para quitarle los bordes.".to_string()
            };
            self.status_kind = StatusKind::Warning;
            return;
        };

        // Determina el monitor actual de esa ventana para no moverla a otra pantalla
        let win_monitor = get_window_center(win.hwnd)
            .and_then(|(cx, cy)| self.monitors.iter().find(|m| m.contains_point(cx, cy)))
            .or_else(|| self.current_monitor());

        let mode = win_monitor.map(|m| m.current_mode).unwrap_or_default();

        match apply_borderless_window(
            win.hwnd,
            &win.title,
            mode.pos_x,
            mode.pos_y,
            mode.width,
            mode.height,
        ) {
            Ok(msg) => {
                self.status_text = msg;
                self.status_kind = StatusKind::Success;
            }
            Err(err) => {
                self.status_text = err;
                self.status_kind = StatusKind::Warning;
            }
        }
    }

    fn visible_presets(&self) -> Vec<ResolutionPreset> {
        let mut all = self.builtin.clone();
        for custom in self.config.custom_as_presets() {
            if !all
                .iter()
                .any(|p| p.width == custom.width && p.height == custom.height)
            {
                all.push(custom);
            }
        }

        match self.selected_category {
            PresetCategory::Featured => {
                let mut favs: Vec<ResolutionPreset> = all
                    .iter()
                    .filter(|p| self.config.is_favorite(p.width, p.height))
                    .cloned()
                    .collect();
                if favs.is_empty() {
                    favs = all.into_iter().take(9).collect();
                }
                favs
            }
            PresetCategory::Custom => {
                let mut customs = self.config.custom_as_presets();
                for p in &self.builtin {
                    if self.config.is_favorite(p.width, p.height)
                        && !customs
                            .iter()
                            .any(|c| c.width == p.width && c.height == p.height)
                    {
                        customs.push(p.clone());
                    }
                }
                customs
            }
            cat => all.into_iter().filter(|p| p.category == cat).collect(),
        }
    }

    fn render_header(&mut self, ui: &mut Ui) {
        let compact = ui.available_width() < 410.0_f32;

        Frame::none()
            .fill(BG_PANEL)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .inner_margin(Margin::symmetric(12.0_f32, 8.0_f32))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0_f32;

                    if let Some(tex) = &self.logo_texture {
                        ui.add(
                            egui::Image::new(tex)
                                .fit_to_exact_size(Vec2::new(20.0_f32, 20.0_f32)),
                        );
                    }

                    ui.label(
                        RichText::new("AspectShift")
                            .font(FontId::new(14.5_f32, FontFamily::Proportional))
                            .strong()
                            .color(TEXT_PRIMARY),
                    );

                    if let Some(mon) = self.current_monitor() {
                        let m = mon.current_mode;
                        let pill_text = if compact {
                            format!("{}×{}", m.width, m.height)
                        } else {
                            format!("{}×{} · {}Hz", m.width, m.height, m.hz)
                        };
                        Frame::none()
                            .fill(BG_CARD)
                            .stroke(Stroke::new(1.0_f32, BORDER))
                            .rounding(Rounding::same(5.0_f32))
                            .inner_margin(Margin::symmetric(7.0_f32, 2.0_f32))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(pill_text).size(11.0_f32).color(TEXT_MUTED),
                                );
                            });
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0_f32;

                        let opts_label = if compact { "Ajustes" } else { "Opciones" };
                        if Self::animated_button(
                            ui,
                            "hdr_opts_btn",
                            opts_label,
                            11.5_f32,
                            false,
                            self.show_options_panel,
                            true,
                        )
                        .on_hover_text("Mostrar u ocultar opciones avanzadas")
                        .clicked()
                        {
                            self.show_options_panel = !self.show_options_panel;
                        }

                        if Self::animated_button(
                            ui,
                            "hdr_native_btn",
                            "Nativa",
                            11.5_f32,
                            false,
                            false,
                            true,
                        )
                        .on_hover_text("Restaurar resolución nativa (Ctrl+Alt+N)")
                        .clicked()
                        {
                            self.trigger_restore_native();
                        }
                    });
                });
            });
    }

    fn render_category_bar(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(5.0_f32, 5.0_f32);

            for (idx, cat) in PresetCategory::ALL.into_iter().enumerate() {
                let is_sel = self.selected_category == cat;
                if Self::animated_button(
                    ui,
                    ("cat_tab", idx),
                    cat.label(),
                    11.5_f32,
                    false,
                    is_sel,
                    true,
                )
                .clicked()
                    && !is_sel
                {
                    self.selected_category = cat;
                    self.category_changed_at = Instant::now();
                }
            }
        });
    }

    fn render_options_drawer(&mut self, ui: &mut Ui) {
        let open_t = ui.ctx().animate_bool_with_time(
            Id::new("options_drawer_anim"),
            self.show_options_panel,
            0.24_f32,
        );
        if open_t <= 0.01_f32 {
            return;
        }

        let ease = 1.0_f32 - (1.0_f32 - open_t).powi(3);
        let bg = lerp_color(BG_DARK, BG_PANEL, ease);
        let stroke_col = lerp_color(BG_DARK, BORDER, ease);

        ui.scope(|ui| {
            ui.multiply_opacity(ease);

            Frame::none()
                .fill(bg)
                .stroke(Stroke::new(1.0_f32, stroke_col))
                .rounding(Rounding::same(7.0_f32))
                .inner_margin(Margin::symmetric(10.0_f32, 8.0_f32 * ease))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(6.0_f32, 6.0_f32);

                    // Monitor y tasa de refresco
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Pantalla:").size(11.0_f32).color(TEXT_MUTED));

                        if self.monitors.len() > 1 {
                            let current_label = format!("Pantalla {}", self.selected_monitor + 1);
                            egui::ComboBox::from_id_salt("monitor_selector")
                                .width(105.0_f32)
                                .selected_text(RichText::new(current_label).size(11.0_f32))
                                .show_ui(ui, |ui| {
                                    for idx in 0..self.monitors.len() {
                                        let label = self.monitors[idx].display_label(idx);
                                        if ui
                                            .selectable_label(self.selected_monitor == idx, label)
                                            .clicked()
                                        {
                                            self.selected_monitor = idx;
                                        }
                                    }
                                });
                        }

                        let hz_list = self
                            .current_monitor()
                            .map(|m| m.all_supported_hz.clone())
                            .unwrap_or_else(|| vec![60]);
                        let selected_hz_text = match self.selected_hz {
                            None => "Hz: Auto".to_string(),
                            Some(hz) => format!("{} Hz", hz),
                        };
                        egui::ComboBox::from_id_salt("hz_selector")
                            .width(78.0_f32)
                            .selected_text(RichText::new(selected_hz_text).size(11.0_f32))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.selected_hz, None, "Auto");
                                for hz in hz_list {
                                    ui.selectable_value(
                                        &mut self.selected_hz,
                                        Some(hz),
                                        format!("{} Hz", hz),
                                    );
                                }
                            });

                        if Self::animated_button(
                            ui,
                            "opt_refresh_btn",
                            "Actualizar",
                            11.0_f32,
                            false,
                            false,
                            true,
                        )
                        .on_hover_text("Recargar monitores y ventanas")
                        .clicked()
                        {
                            self.full_refresh_devices_and_windows();
                        }
                    });

                    // Resolución personalizada (soporta Enter para aplicar rápido)
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new("Personalizada:")
                                .size(11.0_f32)
                                .color(TEXT_MUTED),
                        );
                        let w_resp = ui.add(
                            egui::TextEdit::singleline(&mut self.custom_w_input)
                                .desired_width(48.0_f32)
                                .hint_text("1440"),
                        );
                        ui.label(RichText::new("×").size(11.5_f32).color(TEXT_DIM));
                        let h_resp = ui.add(
                            egui::TextEdit::singleline(&mut self.custom_h_input)
                                .desired_width(48.0_f32)
                                .hint_text("1080"),
                        );

                        let parsed_pair = match (
                            self.custom_w_input.trim().parse::<u32>(),
                            self.custom_h_input.trim().parse::<u32>(),
                        ) {
                            (Ok(w), Ok(h))
                                if (640..=7680).contains(&w) && (480..=4320).contains(&h) =>
                            {
                                Some((w, h))
                            }
                            _ => None,
                        };

                        let enter_pressed = (w_resp.lost_focus() || h_resp.lost_focus())
                            && ui.input(|i| i.key_pressed(Key::Enter));

                        if (enter_pressed && parsed_pair.is_some())
                            || Self::animated_button(
                                ui,
                                "opt_apply_custom",
                                "Aplicar",
                                11.0_f32,
                                true,
                               false,
                                parsed_pair.is_some(),
                            )
                            .clicked()
                        {
                            if let Some((w, h)) = parsed_pair {
                                self.trigger_apply_resolution(w, h);
                            }
                        }

                        if Self::animated_button(
                            ui,
                            "opt_save_custom",
                            "Guardar",
                            11.0_f32,
                            false,
                            false,
                            parsed_pair.is_some(),
                        )
                        .clicked()
                        {
                            if let Some((w, h)) = parsed_pair {
                                let tag =
                                    Some(format!("{} Personalizada", format_aspect_ratio(w, h)));
                                self.config.add_custom_preset(w, h, tag);
                                self.status_text = format!("Guardada {}×{}", w, h);
                                self.status_kind = StatusKind::Success;
                            }
                        }
                    });

                    // Ajuste manual de ventana sin bordes
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new("Ventana:")
                                .size(11.0_f32)
                                .color(TEXT_MUTED),
                        );

                        let selected_win_label = if self.selected_window_idx == 0 {
                            "Seleccionar ventana...".to_string()
                        } else if let Some(w) =
                            self.candidate_windows.get(self.selected_window_idx - 1)
                        {
                            let mut s: String = w.title.chars().take(20).collect();
                            if w.title.chars().count() > 20 {
                                s.push('…');
                            }
                            s
                        } else {
                            "Seleccionar ventana...".to_string()
                        };

                        egui::ComboBox::from_id_salt("window_selector")
                            .width(148.0_f32)
                            .selected_text(RichText::new(selected_win_label).size(11.0_f32))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.selected_window_idx,
                                    0,
                                    "Ninguna (no modificar ventanas)",
                                );
                                for (i, w) in self.candidate_windows.iter().enumerate() {
                                    ui.selectable_value(
                                        &mut self.selected_window_idx,
                                        i + 1,
                                        &w.title,
                                    );
                                }
                            });

                        if Self::animated_button(
                            ui,
                            "opt_borderless_btn",
                            "Quitar bordes",
                            11.0_f32,
                            false,
                            false,
                            self.selected_window_idx > 0,
                        )
                        .on_hover_text(
                            "Adapta únicamente la ventana seleccionada a su monitor actual",
                        )
                        .clicked()
                        {
                            self.trigger_adjust_window(false);
                        }
                    });

                    // Preferencias y guía
                    ui.horizontal_wrapped(|ui| {
                        let mut force_stretch = self.config.force_gpu_stretch_flag;
                        if ui
                            .checkbox(
                                &mut force_stretch,
                                RichText::new("Escalado GPU")
                                    .size(11.0_f32)
                                    .color(TEXT_MUTED),
                            )
                            .changed()
                        {
                            self.config.force_gpu_stretch_flag = force_stretch;
                            self.config.save();
                        }

                        let mut restore_exit = self.config.restore_native_on_exit;
                        if ui
                            .checkbox(
                                &mut restore_exit,
                                RichText::new("Restaurar al salir")
                                    .size(11.0_f32)
                                    .color(TEXT_MUTED),
                            )
                            .changed()
                        {
                            self.config.restore_native_on_exit = restore_exit;
                            self.config.save();
                        }

                        let mut always_top = self.config.always_on_top;
                        if ui
                            .checkbox(
                                &mut always_top,
                                RichText::new("Siempre visible")
                                    .size(11.0_f32)
                                    .color(TEXT_MUTED),
                            )
                            .changed()
                        {
                            self.config.always_on_top = always_top;
                            self.config.save();
                            let level = if always_top {
                                WindowLevel::AlwaysOnTop
                            } else {
                                WindowLevel::Normal
                            };
                            ui.ctx().send_viewport_cmd(ViewportCommand::WindowLevel(level));
                        }

                        let mut confirm_safe = self.config.confirm_resolution_change;
                        if ui
                            .checkbox(
                                &mut confirm_safe,
                                RichText::new("Confirmar cambio")
                                    .size(11.0_f32)
                                    .color(TEXT_MUTED),
                            )
                            .on_hover_text(
                                "Si la pantalla no se ve bien tras cambiar, revierte automáticamente en 12 segundos",
                            )
                            .changed()
                        {
                            self.config.confirm_resolution_change = confirm_safe;
                            self.config.save();
                        }

                        if Self::animated_button(
                            ui,
                            "opt_guide_btn",
                            "Guía",
                            11.0_f32,
                            false,
                            self.show_guide_modal,
                            true,
                        )
                        .clicked()
                        {
                            self.show_guide_modal = !self.show_guide_modal;
                        }

                        let check_label = if self.update_checking {
                            "Buscando..."
                        } else {
                            "Buscar actualización"
                        };
                        if Self::animated_button(
                            ui,
                            "opt_check_update_btn",
                            check_label,
                            11.0_f32,
                            false,
                            false,
                            !self.update_checking && !self.update_downloading,
                        )
                        .clicked()
                        {
                            self.update_checking = true;
                            self.update_manager
                                .check_for_updates(ui.ctx().clone(), true);
                        }

                        let start_menu_label = if self.in_start_menu {
                            "Quitar de Inicio"
                        } else {
                            "Añadir a Inicio"
                        };
                        if Self::animated_button(
                            ui,
                            "opt_start_menu_btn",
                            start_menu_label,
                            11.0_f32,
                            false,
                            self.in_start_menu,
                            true,
                        )
                        .on_hover_text(
                            "Registra AspectShift en el Menú Inicio para que aparezca como Aplicación al buscar en Windows",
                        )
                        .clicked()
                        {
                            let res = if self.in_start_menu {
                                remove_from_start_menu()
                            } else {
                                register_in_start_menu()
                            };
                            match res {
                                Ok(msg) => {
                                    self.in_start_menu = is_in_start_menu();
                                    self.status_text = msg;
                                    self.status_kind = StatusKind::Success;
                                }
                                Err(err) => {
                                    self.status_text = err;
                                    self.status_kind = StatusKind::Warning;
                                }
                            }
                        }

                        ui.label(
                            RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                .size(10.5_f32)
                                .color(TEXT_DIM),
                        );
                    });
                });
        });

        ui.add_space(6.0_f32 * ease);
    }

    fn render_update_banner(&mut self, ui: &mut Ui) {
        let Some(info) = self.available_update.clone() else {
            return;
        };
        if self.update_dismissed {
            return;
        }

        let mut start_download = false;
        let mut dismiss = false;

        Frame::none()
            .fill(BG_CARD_ACTIVE)
            .stroke(Stroke::new(1.0_f32, ACCENT))
            .rounding(Rounding::same(6.0_f32))
            .inner_margin(Margin::symmetric(10.0_f32, 6.0_f32))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (dot_rect, _) =
                        ui.allocate_exact_size(Vec2::new(8.0_f32, 8.0_f32), Sense::hover());
                    ui.painter()
                        .circle_filled(dot_rect.center(), 3.2_f32, ACCENT_SOFT);

                    ui.label(
                        RichText::new(format!("Nueva versión {} disponible", info.version_tag))
                            .size(11.5_f32)
                            .color(TEXT_PRIMARY),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (x_rect, x_resp) = ui.allocate_exact_size(
                            Vec2::new(18.0_f32, 18.0_f32),
                            Sense::click(),
                        );
                        if x_resp.hovered() {
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        }
                        let x_col = if x_resp.hovered() {
                            TEXT_PRIMARY
                        } else {
                            TEXT_MUTED
                        };
                        draw_cross(ui.painter(), x_rect.center(), 3.5_f32, x_col);
                        if x_resp.clicked() && !self.update_downloading {
                            dismiss = true;
                        }

                        let btn_label = if self.update_downloading {
                            "Descargando..."
                        } else {
                            "Actualizar"
                        };
                        if Self::animated_button(
                            ui,
                            "banner_update_btn",
                            btn_label,
                            11.0_f32,
                            true,
                            false,
                            !self.update_downloading,
                        )
                        .clicked()
                        {
                            start_download = true;
                        }
                    });
                });
            });

        ui.add_space(6.0_f32);

        if dismiss {
            self.update_dismissed = true;
        } else if start_download {
            self.update_downloading = true;
            self.status_text = format!("Descargando {}...", info.version_tag);
            self.status_kind = StatusKind::Ready;
            self.update_manager.download_release(ui.ctx().clone(), info);
        }
    }

    fn render_resolution_grid(&mut self, ui: &mut Ui) {
        let presets = self.visible_presets();
        let active_mode = self
            .current_monitor()
            .map(|m| (m.current_mode.width, m.current_mode.height))
            .unwrap_or((0, 0));

        // Suaviza la entrada de las tarjetas al cambiar de categoría
        let elapsed_cat = self.category_changed_at.elapsed().as_secs_f32();
        let cat_fade = if elapsed_cat < 0.20_f32 {
            ui.ctx().request_repaint();
            let t = (elapsed_cat / 0.20_f32).clamp(0.0_f32, 1.0_f32);
            0.35_f32 + 0.65_f32 * (1.0_f32 - (1.0_f32 - t).powi(2))
        } else {
            1.0_f32
        };

        let spacing = 8.0_f32;
        let avail_w = ui.available_width().max(120.0_f32);
        let min_card_w = 138.0_f32;
        let cols = ((avail_w + spacing) / (min_card_w + spacing))
            .floor()
            .clamp(1.0_f32, 4.0_f32) as usize;
        let card_w = ((avail_w - spacing * (cols as f32 - 1.0_f32)) / cols as f32)
            .floor()
            .max(100.0_f32);
        let card_h = 62.0_f32;

        let mut action_apply: Option<(u32, u32)> = None;
        let mut action_toggle_fav: Option<(u32, u32)> = None;
        let mut action_delete_custom: Option<(u32, u32)> = None;

        ui.scope(|ui| {
            ui.multiply_opacity(cat_fade);

            for row in presets.chunks(cols) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = spacing;
                    for preset in row {
                        let is_active = active_mode == (preset.width, preset.height);
                        let is_fav = self.config.is_favorite(preset.width, preset.height);
                        let is_driver_ready = self
                            .current_monitor()
                            .map(|m| {
                                m.supported_resolutions
                                    .contains(&(preset.width, preset.height))
                            })
                            .unwrap_or(true);
                        let card_id = Id::new(("res_card", preset.width, preset.height));

                        let (rect, response) =
                            ui.allocate_exact_size(Vec2::new(card_w, card_h), Sense::click());

                        let hover_t = ui.ctx().animate_bool_with_time(
                            card_id.with("hover"),
                            response.hovered(),
                            0.20_f32,
                        );
                        let active_t = ui.ctx().animate_bool_with_time(
                            card_id.with("active"),
                            is_active,
                            0.26_f32,
                        );
                        let press_t = ui.ctx().animate_bool_with_time(
                            card_id.with("press"),
                            response.is_pointer_button_down_on(),
                            0.10_f32,
                        );

                        // Elevación en hover y asentamiento táctil al pulsar
                        let y_offset = -1.6_f32 * hover_t + 1.4_f32 * press_t;
                        let visual_rect = rect.translate(Vec2::new(0.0_f32, y_offset));

                        let star_rect = Rect::from_min_size(
                            Pos2::new(visual_rect.right() - 25.0_f32, visual_rect.top() + 6.0_f32),
                            Vec2::new(18.0_f32, 18.0_f32),
                        );
                        let del_rect = if preset.is_custom {
                            Some(Rect::from_min_size(
                                Pos2::new(
                                    visual_rect.right() - 45.0_f32,
                                    visual_rect.top() + 6.0_f32,
                                ),
                                Vec2::new(18.0_f32, 18.0_f32),
                            ))
                        } else {
                            None
                        };

                        let pointer_pos = response
                            .interact_pointer_pos()
                            .or_else(|| ui.ctx().pointer_hover_pos());
                        let star_hovered =
                            pointer_pos.map(|p| star_rect.contains(p)).unwrap_or(false);
                        let del_hovered = del_rect
                            .and_then(|r| pointer_pos.map(|p| r.contains(p)))
                            .unwrap_or(false);

                        if response.hovered() {
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        }

                        if response.clicked() {
                            if star_hovered {
                                action_toggle_fav = Some((preset.width, preset.height));
                            } else if del_hovered {
                                action_delete_custom = Some((preset.width, preset.height));
                            } else {
                                action_apply = Some((preset.width, preset.height));
                            }
                        }

                        let painter = ui.painter_at(visual_rect.expand(3.0_f32));
                        let bg_hover = lerp_color(BG_CARD, BG_CARD_HOVER, hover_t);
                        let mut bg_fill = lerp_color(bg_hover, BG_CARD_ACTIVE, active_t);
                        if press_t > 0.01_f32 {
                            bg_fill = lerp_color(bg_fill, BG_PANEL, 0.20_f32 * press_t);
                        }

                        let border_hover = lerp_color(BORDER, BORDER_HOVER, hover_t);
                        let border_col = lerp_color(border_hover, ACCENT, active_t);
                        let border_w = 1.0_f32 + 0.5_f32 * active_t;

                        painter.rect(
                            visual_rect,
                            Rounding::same(7.0_f32),
                            bg_fill,
                            Stroke::new(border_w, border_col),
                        );

                        let res_label = format!("{} × {}", preset.width, preset.height);
                        let title_size = if card_w < 135.0_f32 { 13.5_f32 } else { 15.0_f32 };
                        painter.text(
                            Pos2::new(visual_rect.left() + 10.0_f32, visual_rect.top() + 17.0_f32),
                            Align2::LEFT_CENTER,
                            res_label,
                            FontId::new(title_size, FontFamily::Proportional),
                            TEXT_PRIMARY,
                        );

                        let star_col = if is_fav {
                            ACCENT_SOFT
                        } else if star_hovered {
                            TEXT_PRIMARY
                        } else {
                            lerp_color(Color32::TRANSPARENT, TEXT_DIM, hover_t)
                        };
                        if is_fav || hover_t > 0.05_f32 {
                            draw_star(&painter, star_rect.center(), 5.0_f32, is_fav, star_col);
                        }

                        if let Some(drect) = del_rect {
                            let dcol = if del_hovered {
                                STATUS_WARN
                            } else {
                                lerp_color(TEXT_DIM, TEXT_MUTED, hover_t)
                            };
                            draw_cross(&painter, drect.center(), 3.6_f32, dcol);
                        }

                        painter.text(
                            Pos2::new(visual_rect.left() + 10.0_f32, visual_rect.top() + 37.0_f32),
                            Align2::LEFT_CENTER,
                            &preset.tag,
                            FontId::new(10.5_f32, FontFamily::Proportional),
                            TEXT_DIM,
                        );

                        let aspect_col = lerp_color(TEXT_MUTED, ACCENT_SOFT, active_t);
                        painter.text(
                            Pos2::new(
                                visual_rect.right() - 10.0_f32,
                                visual_rect.top() + 37.0_f32,
                            ),
                            Align2::RIGHT_CENTER,
                            preset.aspect_label(),
                            FontId::new(10.5_f32, FontFamily::Proportional),
                            aspect_col,
                        );

                        // Barra inferior de proporción respecto a 16:9
                        let bar_outer = Rect::from_min_max(
                            Pos2::new(
                                visual_rect.left() + 10.0_f32,
                                visual_rect.bottom() - 9.0_f32,
                            ),
                            Pos2::new(
                                visual_rect.right() - 10.0_f32,
                                visual_rect.bottom() - 6.5_f32,
                            ),
                        );
                        painter.rect_filled(
                            bar_outer,
                            Rounding::same(1.5_f32),
                            Color32::from_rgb(20, 22, 28),
                        );

                        let base_ratio = preset.visual_bar_ratio();
                        let anim_ratio =
                            base_ratio + (1.0_f32 - base_ratio) * 0.05_f32 * hover_t;
                        let inner_w = bar_outer.width() * anim_ratio;
                        let bar_inner = Rect::from_min_size(
                            Pos2::new(bar_outer.center().x - inner_w * 0.5_f32, bar_outer.top()),
                            Vec2::new(inner_w, bar_outer.height()),
                        );
                        let bar_idle = lerp_color(TEXT_DIM, ACCENT_SOFT, hover_t);
                        let bar_col = lerp_color(bar_idle, STATUS_OK, active_t);
                        painter.rect_filled(bar_inner, Rounding::same(1.5_f32), bar_col);

                        let tip = if is_driver_ready {
                            "Lista en el controlador gráfico — clic para aplicar"
                        } else {
                            "No registrada de fábrica — puede requerir crearla en el panel GPU"
                        };
                        response.on_hover_text(tip);
                    }
                });
                ui.add_space(2.0_f32);
            }
        });

        if let Some((w, h)) = action_toggle_fav {
            let added = self.config.toggle_favorite(w, h);
            self.status_text = if added {
                format!("Añadida {}×{} a Destacadas", w, h)
            } else {
                format!("Quitada {}×{} de Destacadas", w, h)
            };
            self.status_kind = StatusKind::Ready;
        }

        if let Some((w, h)) = action_delete_custom {
            self.config.remove_custom_preset(w, h);
            self.status_text = format!("Eliminada {}×{}", w, h);
            self.status_kind = StatusKind::Ready;
        }

        if let Some((w, h)) = action_apply {
            self.trigger_apply_resolution(w, h);
        }
    }

    fn render_footer(&self, ui: &mut Ui) {
        let dot_col = match self.status_kind {
            StatusKind::Ready => TEXT_DIM,
            StatusKind::Success => STATUS_OK,
            StatusKind::Warning => STATUS_WARN,
            StatusKind::Error => ACCENT_SOFT,
        };
        let show_hotkeys = ui.available_width() > 440.0_f32;

        Frame::none()
            .fill(BG_PANEL)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .inner_margin(Margin::symmetric(12.0_f32, 6.0_f32))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (dot_rect, _) =
                        ui.allocate_exact_size(Vec2::new(8.0_f32, 8.0_f32), Sense::hover());
                    ui.painter().circle_filled(dot_rect.center(), 3.2_f32, dot_col);

                    ui.add(
                        egui::Label::new(
                            RichText::new(&self.status_text)
                                .size(11.0_f32)
                                .color(TEXT_MUTED),
                        )
                        .truncate(),
                    );

                    if show_hotkeys {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new("Ctrl+Alt+S / N / B")
                                    .size(10.5_f32)
                                    .color(TEXT_DIM),
                            );
                        });
                    }
                });
            });
    }

    fn render_confirm_modal(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_confirm else {
            return;
        };

        let elapsed = pending.started_at.elapsed().as_secs_f32();
        if elapsed >= SAFE_CONFIRM_SECONDS {
            self.revert_pending_confirmation();
            return;
        }

        ctx.request_repaint();
        let remaining = (SAFE_CONFIRM_SECONDS - elapsed).ceil() as u32;

        egui::Area::new(Id::new("confirm_modal_backdrop"))
            .order(Order::Foreground)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                let screen = ctx.screen_rect();
                ui.painter().rect_filled(
                    screen,
                    Rounding::ZERO,
                    Color32::from_rgba_unmultiplied(0, 0, 0, 175),
                );
            });

        let modal_w = (ctx.screen_rect().width() - 32.0_f32).clamp(260.0_f32, 340.0_f32);
        let mut keep_clicked = false;
        let mut revert_clicked = false;

        egui::Window::new("Confirmar cambio")
            .order(Order::Foreground)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(modal_w, 118.0_f32))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0_f32;
                ui.label(
                    RichText::new("¿Deseas conservar la resolución actual?")
                        .size(12.5_f32)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                ui.label(
                    RichText::new(format!(
                        "Se restaurará la anterior automáticamente en {} s.",
                        remaining
                    ))
                    .size(11.5_f32)
                    .color(TEXT_MUTED),
                );

                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if Self::animated_button(
                            ui,
                            "confirm_keep_btn",
                            "Conservar",
                            11.5_f32,
                            true,
                            false,
                            true,
                        )
                        .clicked()
                        {
                            keep_clicked = true;
                        }

                        if Self::animated_button(
                            ui,
                            "confirm_revert_btn",
                            "Revertir",
                            11.5_f32,
                            false,
                            false,
                            true,
                        )
                        .clicked()
                        {
                            revert_clicked = true;
                        }
                    });
                });
            });

        if keep_clicked {
            self.pending_confirm = None;
        } else if revert_clicked {
            self.revert_pending_confirmation();
        }
    }

    fn render_guide_modal(&mut self, ctx: &egui::Context) {
        if !self.show_guide_modal {
            return;
        }

        egui::Area::new(Id::new("guide_modal_backdrop"))
            .order(Order::Foreground)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                let screen = ctx.screen_rect();
                ui.painter().rect_filled(
                    screen,
                    Rounding::ZERO,
                    Color32::from_rgba_unmultiplied(0, 0, 0, 175),
                );
            });

        let modal_w = (ctx.screen_rect().width() - 32.0_f32).clamp(280.0_f32, 430.0_f32);
        let modal_h = (ctx.screen_rect().height() - 32.0_f32).clamp(240.0_f32, 310.0_f32);

        egui::Window::new("Guía rápida")
            .order(Order::Foreground)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(modal_w, modal_h))
            .show(ctx, |ui| {
                ScrollArea::vertical().show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0_f32;

                    ui.label(
                        RichText::new("1. Escalado a pantalla completa (GPU)")
                            .size(12.5_f32)
                            .strong()
                            .color(TEXT_PRIMARY),
                    );
                    ui.label(
                        RichText::new(
                            "• NVIDIA: Panel NVIDIA → Ajustar tamaño y posición del escritorio → Pantalla completa → Realizar escalado en: GPU.\n• AMD: Adrenalin → Pantalla → Activar Escalado de GPU y Modo: Panel completo.",
                        )
                        .size(11.5_f32)
                        .color(TEXT_MUTED),
                    );

                    ui.separator();

                    ui.label(
                        RichText::new("2. Resoluciones personalizadas")
                            .size(12.5_f32)
                            .strong()
                            .color(TEXT_PRIMARY),
                    );
                    ui.label(
                        RichText::new(
                            "Si Windows rechaza una resolución (código -2), créala una vez como resolución personalizada en el panel de tu tarjeta gráfica para registrarla en el controlador.",
                        )
                        .size(11.5_f32)
                        .color(TEXT_MUTED),
                    );

                    ui.separator();

                    ui.label(
                        RichText::new("3. Atajos globales")
                            .size(12.5_f32)
                            .strong()
                            .color(TEXT_PRIMARY),
                    );
                    ui.label(
                        RichText::new(
                            "• Ctrl + Alt + S: Aplicar última resolución estirada.\n• Ctrl + Alt + N: Restaurar resolución nativa.\n• Ctrl + Alt + B: Quitar bordes a la ventana enfocada.",
                        )
                        .size(11.5_f32)
                        .color(TEXT_MUTED),
                    );
                });

                ui.add_space(6.0_f32);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if Self::animated_button(
                        ui,
                        "modal_close_btn",
                        "Cerrar",
                        11.5_f32,
                        true,
                        false,
                        true,
                    )
                    .clicked()
                    {
                        self.show_guide_modal = false;
                    }
                });
            });
    }
}

impl eframe::App for AspectShiftApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Some(ev) = self.update_manager.poll() {
            match ev {
                UpdateEvent::CheckResult { manual, result } => {
                    self.update_checking = false;
                    match result {
                        Ok(Some(info)) => {
                            self.available_update = Some(info);
                            self.update_dismissed = false;
                        }
                        Ok(None) => {
                            if manual {
                                self.status_text = format!(
                                    "Ya tienes la última versión (v{})",
                                    env!("CARGO_PKG_VERSION")
                                );
                                self.status_kind = StatusKind::Success;
                            }
                        }
                        Err(err) => {
                            if manual {
                                self.status_text = err;
                                self.status_kind = StatusKind::Warning;
                            }
                        }
                    }
                }
                UpdateEvent::DownloadFinished(res) => {
                    self.update_downloading = false;
                    match res {
                        Ok(temp_exe) => match apply_downloaded_update(&temp_exe) {
                            Ok(()) => {
                                ctx.send_viewport_cmd(ViewportCommand::Close);
                            }
                            Err(err) => {
                                self.status_text = err;
                                self.status_kind = StatusKind::Error;
                            }
                        },
                        Err(err) => {
                            self.status_text = err;
                            self.status_kind = StatusKind::Warning;
                        }
                    }
                }
            }
        }

        while let Some(action) = self.hotkey_listener.poll() {
            match action {
                HotkeyAction::ApplyFavoriteStretch => {
                    let (w, h) = self.config.last_stretch_res;
                    self.trigger_apply_resolution(w, h);
                }
                HotkeyAction::RestoreNative => {
                    self.trigger_restore_native();
                }
                HotkeyAction::ApplyWindowBorderless => {
                    self.trigger_adjust_window(true);
                }
            }
        }

        if self.last_mode_poll.elapsed() > Duration::from_secs(3) {
            self.refresh_live_mode();
            self.last_mode_poll = Instant::now();
        }

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            if self.pending_confirm.is_some() {
                self.revert_pending_confirmation();
            } else if self.show_guide_modal {
                self.show_guide_modal = false;
            }
        }

        egui::TopBottomPanel::top("header_panel")
            .frame(Frame::none())
            .show(ctx, |ui| {
                self.render_header(ui);
            });

        egui::TopBottomPanel::bottom("footer_panel")
            .frame(Frame::none())
            .show(ctx, |ui| {
                self.render_footer(ui);
            });

        egui::CentralPanel::default()
            .frame(
                Frame::none()
                    .fill(BG_DARK)
                    .inner_margin(Margin::symmetric(12.0_f32, 8.0_f32)),
            )
            .show(ctx, |ui| {
                self.render_update_banner(ui);
                self.render_category_bar(ui);
                ui.add_space(6.0_f32);

                self.render_options_drawer(ui);

                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.render_resolution_grid(ui);
                    });
            });

        self.render_confirm_modal(ctx);
        self.render_guide_modal(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.config.restore_native_on_exit {
            let _ = self
                .current_monitor()
                .cloned()
                .map(|m| restore_native(&m.device_name, Some(m.native_mode)));
        }
    }
}
