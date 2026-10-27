//! Look and feel shared by Z and Z Setup: theme, fonts, logo and the custom widgets.
//! One corner radius, one text colour and one violet for every control in both programs.
#![allow(dead_code)]

use eframe::egui::{self, vec2, Align2, Color32, FontFamily, FontId, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, TextStyle};

/// Logo violet (icons, folder glyphs, focus rings).
pub const ACCENT: Color32 = Color32::from_rgb(108, 71, 255);
/// Deeper violet for filled controls, so light text on them keeps a readable contrast.
pub const ACCENT_DEEP: Color32 = Color32::from_rgb(76, 48, 206);
/// #c1c5ce: the one text colour used across the whole interface.
pub const TXT: Color32 = Color32::from_rgb(0xc1, 0xc5, 0xce);
/// Background gradient, top-left to bottom-right.
pub const GRADIENT: [Color32; 3] = [Color32::from_rgb(44, 28, 112), Color32::from_rgb(28, 20, 72), Color32::from_rgb(14, 11, 34)];
pub const MENU_BG: Color32 = Color32::from_rgb(24, 19, 58);
/// The single corner radius used by every button, field, menu, checkbox and window.
pub const RADIUS: f32 = 6.0;

/// #c1c5ce at a given opacity (hairlines, hover washes, field fills).
pub fn tint(a: u8) -> Color32 { Color32::from_rgba_unmultiplied(0xc1, 0xc5, 0xce, a) }

// ------------------------------------------------------------ theme and fonts

pub fn apply_theme(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    let r = egui::Rounding::same(RADIUS);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.rounding = r;
        w.fg_stroke = Stroke::new(1.0_f32, TXT);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, tint(30));
    v.widgets.inactive.bg_fill = tint(16);  v.widgets.inactive.weak_bg_fill = tint(16);  v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, tint(34));
    v.widgets.hovered.bg_fill = tint(30);   v.widgets.hovered.weak_bg_fill = tint(30);   v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, tint(70));
    v.widgets.active.bg_fill = tint(44);    v.widgets.active.weak_bg_fill = tint(44);    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.open.bg_fill = tint(30);      v.widgets.open.weak_bg_fill = tint(30);

    v.override_text_color = Some(TXT);
    v.hyperlink_color = TXT;
    v.selection.bg_fill = ACCENT_DEEP;
    v.selection.stroke = Stroke::new(1.0_f32, TXT);

    v.window_rounding = egui::Rounding::same(RADIUS);
    v.menu_rounding = egui::Rounding::same(RADIUS);
    v.window_fill = MENU_BG;
    v.window_stroke = Stroke::new(1.0_f32, tint(40));
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;

    v.panel_fill = Color32::TRANSPARENT;           // the gradient is painted behind the panels
    v.extreme_bg_color = Color32::from_rgb(15, 12, 34);
    v.faint_bg_color = tint(9);
    let mut s = (*ctx.style()).clone();
    s.visuals = v;
    s.spacing.item_spacing = vec2(8.0, 6.0);
    s.spacing.button_padding = vec2(12.0, 5.0);
    // Thin scroll bars in the interface colour.
    s.spacing.scroll = egui::style::ScrollStyle::thin();
    s.spacing.scroll.bar_width = 7.0;
    s.spacing.scroll.floating_width = 4.0;
    s.text_styles = [
        (TextStyle::Heading, FontId::new(18.0, FontFamily::Name("medium".into()))),
        (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(13.5, FontFamily::Name("medium".into()))),
        (TextStyle::Small, FontId::new(11.5, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
    ].into();
    ctx.set_style(s);
}

/// Z Sans (22 KB per weight) everywhere. Wide-script fallbacks are read from the OS only when an
/// archive actually contains such names, so a normal session stays small in memory.
pub fn install_fonts(ctx: &egui::Context, with_fallbacks: bool) {
    let mut fonts = egui::FontDefinitions::empty();
    fonts.font_data.insert("zsans".into(), egui::FontData::from_static(include_bytes!("../../../assets/font/ZSans-Regular.ttf")));
    fonts.font_data.insert("zsans-medium".into(), egui::FontData::from_static(include_bytes!("../../../assets/font/ZSans-Medium.ttf")));
    let mut fallback: Vec<String> = Vec::new();
    if with_fallbacks {
        let candidates = [
            r"C:\Windows\Fonts\segoeui.ttf", r"C:\Windows\Fonts\msyh.ttc", r"C:\Windows\Fonts\YuGothM.ttc", r"C:\Windows\Fonts\malgun.ttf",
            r"C:\Windows\Fonts\leelawui.ttf", "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ];
        for (i, p) in candidates.iter().enumerate() {
            if let Ok(bytes) = std::fs::read(p) { let k = format!("sys{i}"); fonts.font_data.insert(k.clone(), egui::FontData::from_owned(bytes)); fallback.push(k); }
        }
    }
    let mut mono_key = "zsans".to_string();
    for (i, p) in [r"C:\Windows\Fonts\consola.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"].iter().enumerate() {
        if let Ok(bytes) = std::fs::read(p) { let k = format!("mono{i}"); fonts.font_data.insert(k.clone(), egui::FontData::from_owned(bytes)); mono_key = k; break; }
    }
    let fam = |first: &str| { let mut v = vec![first.to_string()]; v.extend(fallback.iter().cloned()); v };
    fonts.families.insert(FontFamily::Proportional, fam("zsans"));
    fonts.families.insert(FontFamily::Name("medium".into()), fam("zsans-medium"));
    fonts.families.insert(FontFamily::Monospace, { let mut v = fam(&mono_key); if mono_key != "zsans" { v.push("zsans".into()); } v });
    ctx.set_fonts(fonts);
}

// ------------------------------------------------------------ background and logo

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

fn gradient_at(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0) * 2.0;
    if t < 1.0 { lerp_color(GRADIENT[0], GRADIENT[1], t) } else { lerp_color(GRADIENT[1], GRADIENT[2], t - 1.0) }
}

/// Window-wide brand gradient, painted behind every panel.
pub fn paint_background(ctx: &egui::Context) {
    const N: usize = 12;
    let rect = ctx.screen_rect();
    let mut mesh = egui::Mesh::default();
    for j in 0..=N {
        for i in 0..=N {
            let (u, v) = (i as f32 / N as f32, j as f32 / N as f32);
            mesh.colored_vertex(rect.min + vec2(u * rect.width(), v * rect.height()), gradient_at(u * 0.45 + v * 0.55));
        }
    }
    let row = N as u32 + 1;
    for j in 0..N as u32 {
        for i in 0..N as u32 {
            let (a, b, c, d) = (j * row + i, j * row + i + 1, (j + 1) * row + i, (j + 1) * row + i + 1);
            mesh.add_triangle(a, b, c); mesh.add_triangle(b, d, c);
        }
    }
    ctx.layer_painter(egui::LayerId::background()).add(Shape::mesh(mesh));
}

pub fn paint_logo(painter: &egui::Painter, rect: Rect) {
    let s = rect.width();
    painter.rect_filled(rect, s * 0.225, ACCENT);
    let p = |x: f32, y: f32| -> Pos2 { rect.min + vec2(x * s, y * s) };
    let (l, t, r, b, th, k) = (0.27, 0.27, 0.73, 0.73, 0.118, 0.17);
    painter.rect_filled(Rect::from_min_max(p(l, t), p(r, t + th)), s * 0.012, TXT);
    painter.rect_filled(Rect::from_min_max(p(l, b - th), p(r, b)), s * 0.012, TXT);
    painter.add(Shape::convex_polygon(vec![p(r - k, t + th), p(r, t + th), p(l + k, b - th), p(l, b - th)], TXT, Stroke::NONE));
}

// ------------------------------------------------------------ buttons

/// Filled violet call-to-action button.
pub fn primary(label: &str) -> impl egui::Widget {
    egui::Button::new(RichText::new(label).color(TXT)).fill(ACCENT_DEEP)
}

/// Left-arrow button drawn with the painter (no image file, nothing loaded from anywhere).
pub fn icon_back(ui: &mut egui::Ui, enabled: bool) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(34.0, 28.0), sense);
    let resp = if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp };
    let lit = enabled && resp.hovered();
    let fill = if !enabled { tint(8) } else if resp.is_pointer_button_down_on() { tint(44) } else if lit { tint(30) } else { tint(16) };
    ui.painter().rect(rect, RADIUS, fill, Stroke::new(1.0_f32, if lit { tint(70) } else { tint(34) }));
    let st = Stroke::new(1.8_f32, if enabled { TXT } else { tint(90) });
    let c = rect.center();
    ui.painter().line_segment([c + vec2(6.0, 0.0), c + vec2(-6.0, 0.0)], st);
    ui.painter().line_segment([c + vec2(-6.0, 0.0), c + vec2(-1.0, -5.0)], st);
    ui.painter().line_segment([c + vec2(-6.0, 0.0), c + vec2(-1.0, 5.0)], st);
    resp
}

// ------------------------------------------------------------ checkbox, radio, progress

pub fn checkbox(ui: &mut egui::Ui, checked: &mut bool, label: &str) -> Response {
    let font = FontId::new(13.5, FontFamily::Proportional);
    let enabled = ui.is_enabled();
    let text_w = ui.painter().layout_no_wrap(label.to_string(), font.clone(), TXT).size().x;
    let (rect, mut resp) = ui.allocate_exact_size(vec2(26.0 + text_w, 24.0), Sense::click());
    if resp.clicked() { *checked = !*checked; resp.mark_changed(); }
    let resp = if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp };
    let b = Rect::from_min_size(rect.left_center() - vec2(0.0, 9.0), vec2(18.0, 18.0));
    let lit = enabled && resp.hovered();
    if *checked {
        ui.painter().rect(b, RADIUS, if enabled { ACCENT_DEEP } else { tint(30) }, Stroke::new(1.0_f32, if enabled { ACCENT } else { tint(34) }));
        let st = Stroke::new(1.9_f32, if enabled { TXT } else { tint(110) });
        ui.painter().line_segment([b.center() + vec2(-4.5, 0.5), b.center() + vec2(-1.5, 3.5)], st);
        ui.painter().line_segment([b.center() + vec2(-1.5, 3.5), b.center() + vec2(4.5, -3.5)], st);
    } else {
        ui.painter().rect(b, RADIUS, if lit { tint(30) } else { tint(16) }, Stroke::new(1.0_f32, if lit { tint(90) } else { tint(60) }));
    }
    ui.painter().text(rect.left_center() + vec2(26.0, 0.0), Align2::LEFT_CENTER, label, font, if enabled { TXT } else { tint(110) });
    resp
}

/// Round radio button. `selected` is read only; the caller decides what a click means.
pub fn radio(ui: &mut egui::Ui, selected: bool, label: &str) -> Response {
    let font = FontId::new(13.5, FontFamily::Proportional);
    let text_w = ui.painter().layout_no_wrap(label.to_string(), font.clone(), TXT).size().x;
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0 + text_w, 24.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let c = rect.left_center() + vec2(9.0, 0.0);
    let lit = resp.hovered();
    ui.painter().circle(c, 9.0, if selected { ACCENT_DEEP } else if lit { tint(30) } else { tint(16) }, Stroke::new(1.0_f32, if selected { ACCENT } else if lit { tint(90) } else { tint(60) }));
    if selected { ui.painter().circle_filled(c, 3.5, TXT); }
    ui.painter().text(rect.left_center() + vec2(26.0, 0.0), Align2::LEFT_CENTER, label, font, TXT);
    resp
}

pub fn progress_bar(ui: &mut egui::Ui, fraction: f32, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 12.0), Sense::hover());
    ui.painter().rect_filled(rect, RADIUS, tint(22));
    let w = rect.width() * fraction.clamp(0.0, 1.0);
    if w > 1.0 {
        ui.painter().rect_filled(Rect::from_min_size(rect.min, vec2(w.max(12.0).min(rect.width()), rect.height())), RADIUS, ACCENT);
    }
}

// ------------------------------------------------------------ pop-up lists (drop-down, menu)

fn popup_slot() -> egui::Id { egui::Id::new("z_open_popup") }

/// Only one pop-up list is open at a time, so opening another one closes the previous one.
pub fn popup_is_open(ctx: &egui::Context, id: egui::Id) -> bool {
    ctx.data(|d| d.get_temp::<egui::Id>(popup_slot())) == Some(id)
}
pub fn popup_open(ctx: &egui::Context, id: egui::Id) { ctx.data_mut(|d| d.insert_temp(popup_slot(), id)); }
pub fn popup_close(ctx: &egui::Context) { ctx.data_mut(|d| d.remove::<egui::Id>(popup_slot())); }
pub fn popup_toggle(ctx: &egui::Context, id: egui::Id) {
    if popup_is_open(ctx, id) { popup_close(ctx); } else { popup_open(ctx, id); }
}

/// Floating list of options under `anchor`. Returns (picked index, the list should close).
pub fn option_list(ctx: &egui::Context, id: egui::Id, anchor: Rect, width: f32, labels: &[String], selected: Option<usize>) -> (Option<usize>, bool) {
    let mut picked: Option<usize> = None;
    let area = egui::Area::new(id.with("menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(anchor.left_bottom() + vec2(0.0, 4.0))
        .show(ctx, |ui| {
            egui::Frame::none().fill(MENU_BG).stroke(Stroke::new(1.0_f32, tint(44))).rounding(RADIUS).inner_margin(4.0).show(ui, |ui| {
                ui.set_width(width - 8.0);
                ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
                egui::ScrollArea::vertical().max_height(196.0).show(ui, |ui| {
                    for (i, label) in labels.iter().enumerate() {
                        let (r, rr) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
                        let is_sel = selected == Some(i);
                        if is_sel { ui.painter().rect_filled(r, RADIUS, ACCENT_DEEP); }
                        else if rr.hovered() { ui.painter().rect_filled(r, RADIUS, tint(26)); }
                        ui.painter().text(r.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, label, FontId::new(13.5, FontFamily::Proportional), TXT);
                        if is_sel {
                            let m = r.right_center() - vec2(16.0, 0.0);
                            let st = Stroke::new(1.6_f32, TXT);
                            ui.painter().line_segment([m + vec2(-4.0, 0.0), m + vec2(-1.0, 3.0)], st);
                            ui.painter().line_segment([m + vec2(-1.0, 3.0), m + vec2(4.5, -3.5)], st);
                        }
                        if rr.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { picked = Some(i); }
                    }
                });
            });
        });
    let menu_rect = area.response.rect;
    let outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().map_or(false, |p| !menu_rect.contains(p) && !anchor.contains(p)));
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    (picked, picked.is_some() || outside || esc)
}

/// Custom drop-down: a field-style trigger and a floating list. Returns the option the user picked.
pub fn dropdown<T: Clone + PartialEq>(ui: &mut egui::Ui, salt: &str, width: f32, current: &T, options: &[(T, String)]) -> Option<T> {
    let id = ui.make_persistent_id(salt);
    let ctx = ui.ctx().clone();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() { popup_toggle(&ctx, id); }
    let open = popup_is_open(&ctx, id);

    let label = options.iter().find(|(v, _)| v == current).map(|(_, l)| l.as_str()).unwrap_or("");
    let lit = open || resp.hovered();
    ui.painter().rect(rect, RADIUS, tint(if lit { 30 } else { 16 }), Stroke::new(1.0_f32, if open { ACCENT } else { tint(if lit { 70 } else { 34 }) }));
    ui.painter().text(rect.left_center() + vec2(11.0, 0.0), Align2::LEFT_CENTER, label, FontId::new(13.5, FontFamily::Proportional), TXT);
    let c = rect.right_center() - vec2(14.0, 0.0);
    let dir = if open { -1.0_f32 } else { 1.0_f32 };
    ui.painter().add(Shape::convex_polygon(vec![c + vec2(-4.5, -2.5 * dir), c + vec2(4.5, -2.5 * dir), c + vec2(0.0, 3.0 * dir)], TXT, Stroke::NONE));

    let mut picked: Option<T> = None;
    if open {
        let labels: Vec<String> = options.iter().map(|(_, l)| l.clone()).collect();
        let sel = options.iter().position(|(v, _)| v == current);
        let (idx, close) = option_list(&ctx, id, rect, width, &labels, sel);
        if let Some(i) = idx { picked = options.get(i).map(|(v, _)| v.clone()); }
        if close { popup_close(&ctx); }
    }
    picked
}
