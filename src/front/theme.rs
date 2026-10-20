//! The interface's look, a frontier field manual: warm charcoal panels in
//! olive and steel frames, cream text, amber/teal/red/green accents,
//! condensed uppercase headings and windows with chamfered corners.
//!
//! `install` sets egui's fonts and style once; `FieldWindow` and `plate`
//! draw the chamfered windows and panels; the colour constants are what the
//! rest of the interface picks its accents from.

use bevy_egui::egui::{self, Align2, Color32, FontFamily, FontId, Pos2, Rect, RichText, Shape, Stroke, Vec2};

// Panels, darkest to lightest.
pub const PANEL_DEEP: Color32 = Color32::from_rgb(24, 22, 19);
pub const PANEL: Color32 = Color32::from_rgb(37, 34, 29);
pub const RAISED: Color32 = Color32::from_rgb(56, 51, 42);
pub const RAISED_HOVER: Color32 = Color32::from_rgb(72, 65, 50);
// Frames.
pub const OLIVE: Color32 = Color32::from_rgb(124, 122, 78);
pub const OLIVE_DIM: Color32 = Color32::from_rgb(76, 76, 52);
pub const STEEL: Color32 = Color32::from_rgb(140, 152, 156);
// Text.
pub const CREAM: Color32 = Color32::from_rgb(234, 225, 201);
pub const CREAM_BRIGHT: Color32 = Color32::from_rgb(255, 247, 224);
pub const CREAM_DIM: Color32 = Color32::from_rgb(166, 157, 136);
// Accents: attention, information, danger, good.
pub const AMBER: Color32 = Color32::from_rgb(228, 164, 62);
pub const TEAL: Color32 = Color32::from_rgb(86, 184, 170);
pub const RED: Color32 = Color32::from_rgb(218, 88, 66);
pub const GREEN: Color32 = Color32::from_rgb(142, 184, 94);
/// Something worth knowing that isn't wrong: a softer amber.
pub const WARN: Color32 = Color32::from_rgb(236, 190, 110);

/// How far window and panel corners are cut back.
const CUT: f32 = 9.0;
const HEADER_H: f32 = 26.0;
const MARGIN: f32 = 10.0;

/// The heading face: Barlow Condensed SemiBold.
pub fn heading_family() -> FontFamily {
    FontFamily::Name("heading".into())
}

pub fn heading_font(size: f32) -> FontId {
    FontId::new(size, heading_family())
}

/// Condensed uppercase text, for headings, tabs and titles.
pub fn caps(text: impl AsRef<str>, size: f32) -> RichText {
    RichText::new(text.as_ref().to_uppercase()).font(heading_font(size))
}

/// A heading inside a window: an amber tick and the text in condensed capitals.
pub fn section(ui: &mut egui::Ui, text: impl AsRef<str>) -> egui::Response {
    section_colored(ui, text, AMBER)
}

pub fn section_colored(ui: &mut egui::Ui, text: impl AsRef<str>, color: Color32) -> egui::Response {
    ui.add_space(2.0);
    let r = ui
        .horizontal(|ui| {
            let (tick, _) = ui.allocate_exact_size(egui::vec2(3.0, 12.0), egui::Sense::hover());
            ui.painter().rect_filled(tick, 0.0, color);
            ui.label(caps(text, 15.0).color(color))
        })
        .inner;
    ui.add_space(1.0);
    r
}

/// A tab or topic button in condensed capitals.
pub fn tab(ui: &mut egui::Ui, selected: bool, text: impl AsRef<str>) -> egui::Response {
    let color = if selected { CREAM_BRIGHT } else { CREAM_DIM };
    ui.add(egui::Button::selectable(selected, caps(text, 14.5).color(color)))
}

/// A tab that sets `current` to `value` when clicked (`selectable_value` in condensed capitals).
pub fn tab_value<V: PartialEq>(ui: &mut egui::Ui, current: &mut V, value: V, text: impl AsRef<str>) -> egui::Response {
    let r = tab(ui, *current == value, text);
    if r.clicked() {
        *current = value;
    }
    r
}

/// A progress bar with square ends.
pub fn bar(fraction: f32) -> egui::ProgressBar {
    egui::ProgressBar::new(fraction).corner_radius(0)
}

/// An entry in a list of topics or tabs down the side: a number, a title in
/// condensed capitals, an amber bar when selected.
pub fn list_entry(ui: &mut egui::Ui, selected: bool, number: &str, title: &str, width: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 22.0), egui::Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 0.0, ui.visuals().selection.bg_fill);
        p.rect_filled(Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())), 0.0, AMBER);
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0, RAISED);
    }
    let text = if selected || resp.hovered() { CREAM_BRIGHT } else { CREAM_DIM };
    let y = rect.center().y;
    p.text(egui::pos2(rect.left() + 9.0, y), Align2::LEFT_CENTER, number, heading_font(13.0), if selected { AMBER } else { OLIVE });
    p.text(egui::pos2(rect.left() + 30.0, y), Align2::LEFT_CENTER, title.to_uppercase(), heading_font(15.0), text);
    resp
}

/// A point in a list: a small amber square, then `add` beside it (wrapping under itself).
pub fn bullet(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, ui.text_style_height(&egui::TextStyle::Body)), egui::Sense::hover());
        ui.painter().rect_filled(Rect::from_center_size(egui::pos2(rect.left() + 4.0, rect.center().y), egui::vec2(4.0, 4.0)), 0.0, AMBER);
        add(ui);
    });
}

/// A keyboard key, drawn as a little steel-framed cap.
pub fn key_cap(ui: &mut egui::Ui, key: &str) -> egui::Response {
    egui::Frame::new()
        .fill(PANEL_DEEP)
        .stroke(Stroke::new(1.0, STEEL))
        .inner_margin(egui::Margin::symmetric(5, 0))
        .show(ui, |ui| ui.label(RichText::new(key).font(heading_font(14.0)).color(CREAM_BRIGHT)))
        .response
}

/// Fonts and style; called every frame before egui's pass, does its work once per context.
pub fn install(ctx: &egui::Context) {
    let done = egui::Id::new("field_manual_theme");
    if ctx.data(|d| d.get_temp::<bool>(done)).unwrap_or(false) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(done, true));

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "barlow".into(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/BarlowSemiCondensed-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        "barlow_condensed".into(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/BarlowCondensed-SemiBold.ttf")).into(),
    );
    // egui's own fonts stay behind them, for symbols Barlow doesn't have.
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut body = vec!["barlow".to_string()];
    body.extend(fallback.iter().cloned());
    let mut heading = vec!["barlow_condensed".to_string(), "barlow".to_string()];
    heading.extend(fallback);
    fonts.families.insert(FontFamily::Proportional, body);
    fonts.families.insert(heading_family(), heading);
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        use egui::TextStyle;
        style.text_styles = [
            (TextStyle::Heading, heading_font(20.0)),
            (TextStyle::Body, FontId::proportional(14.0)),
            (TextStyle::Button, FontId::proportional(14.0)),
            (TextStyle::Small, FontId::proportional(12.0)),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
        let s = &mut style.spacing;
        s.item_spacing = egui::vec2(6.0, 4.0);
        s.button_padding = egui::vec2(6.0, 2.0);
        s.window_margin = egui::Margin::same(MARGIN as i8);
        s.interact_size.y = 20.0;

        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.weak_text_color = Some(CREAM_DIM);
        v.hyperlink_color = TEAL;
        v.warn_fg_color = AMBER;
        v.error_fg_color = RED;
        v.faint_bg_color = Color32::from_rgb(44, 40, 34);
        v.extreme_bg_color = PANEL_DEEP;
        v.code_bg_color = PANEL_DEEP;
        v.window_fill = PANEL;
        v.panel_fill = PANEL;
        v.window_stroke = Stroke::new(1.0, OLIVE);
        v.window_corner_radius = egui::CornerRadius::ZERO;
        v.menu_corner_radius = egui::CornerRadius::ZERO;
        v.window_shadow = egui::Shadow { offset: [0, 3], blur: 8, spread: 0, color: Color32::from_black_alpha(110) };
        v.popup_shadow = egui::Shadow { offset: [0, 2], blur: 6, spread: 0, color: Color32::from_black_alpha(110) };
        v.selection.bg_fill = Color32::from_rgb(40, 92, 85);
        v.selection.stroke = Stroke::new(1.0, CREAM_BRIGHT);
        v.text_cursor.stroke = Stroke::new(2.0, AMBER);
        v.slider_trailing_fill = true;

        let w = &mut v.widgets;
        let square = egui::CornerRadius::same(1);
        w.noninteractive.bg_fill = PANEL;
        w.noninteractive.weak_bg_fill = PANEL;
        w.noninteractive.bg_stroke = Stroke::new(1.0, OLIVE_DIM);
        w.noninteractive.fg_stroke = Stroke::new(1.0, CREAM);
        w.noninteractive.corner_radius = square;
        w.inactive.bg_fill = RAISED;
        w.inactive.weak_bg_fill = RAISED;
        w.inactive.bg_stroke = Stroke::new(1.0, OLIVE_DIM);
        w.inactive.fg_stroke = Stroke::new(1.0, CREAM);
        w.inactive.corner_radius = square;
        w.hovered.bg_fill = RAISED_HOVER;
        w.hovered.weak_bg_fill = RAISED_HOVER;
        w.hovered.bg_stroke = Stroke::new(1.0, AMBER);
        w.hovered.fg_stroke = Stroke::new(1.5, CREAM_BRIGHT);
        w.hovered.corner_radius = square;
        w.hovered.expansion = 0.0;
        // Strong text takes the active colour, so it stays light.
        w.active.bg_fill = Color32::from_rgb(120, 86, 34);
        w.active.weak_bg_fill = Color32::from_rgb(120, 86, 34);
        w.active.bg_stroke = Stroke::new(1.0, AMBER);
        w.active.fg_stroke = Stroke::new(1.5, CREAM_BRIGHT);
        w.active.corner_radius = square;
        w.active.expansion = 0.0;
        w.open.bg_fill = RAISED_HOVER;
        w.open.weak_bg_fill = RAISED;
        w.open.bg_stroke = Stroke::new(1.0, OLIVE);
        w.open.fg_stroke = Stroke::new(1.0, CREAM_BRIGHT);
        w.open.corner_radius = square;
    });
}

/// The outline of a panel: top-left and bottom-right corners cut back.
fn chamfer(r: Rect, cut: f32) -> Vec<Pos2> {
    vec![
        egui::pos2(r.left() + cut, r.top()),
        egui::pos2(r.right(), r.top()),
        egui::pos2(r.right(), r.bottom() - cut),
        egui::pos2(r.right() - cut, r.bottom()),
        egui::pos2(r.left(), r.bottom()),
        egui::pos2(r.left(), r.top() + cut),
    ]
}

/// How a plate looks: fill, frame and corner cut.
#[derive(Clone, Copy)]
pub struct Plate {
    pub fill: Color32,
    pub stroke: Stroke,
    pub cut: f32,
    pub margin: egui::Margin,
    pub shadow: bool,
}

impl Plate {
    /// A window or a free-standing panel over the map.
    pub fn panel() -> Plate {
        Plate { fill: PANEL, stroke: Stroke::new(1.0, OLIVE), cut: CUT, margin: egui::Margin::same(MARGIN as i8), shadow: true }
    }

    /// A smaller plate inside a panel (cards, entries).
    pub fn inset() -> Plate {
        Plate { fill: PANEL_DEEP, stroke: Stroke::new(1.0, OLIVE_DIM), cut: 5.0, margin: egui::Margin::same(6), shadow: false }
    }

    pub fn stroke(mut self, stroke: Stroke) -> Plate {
        self.stroke = stroke;
        self
    }

    pub fn fill(mut self, fill: Color32) -> Plate {
        self.fill = fill;
        self
    }

    pub fn margin(mut self, margin: impl Into<egui::Margin>) -> Plate {
        self.margin = margin.into();
        self
    }

    /// Lays out `add` inside the plate, drawn behind it once its size is known.
    pub fn show<R>(self, ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
        let behind = ui.painter().add(Shape::Noop);
        let inner = egui::Frame::NONE.inner_margin(self.margin).show(ui, add);
        ui.painter().set(behind, self.shape(inner.response.rect));
        inner
    }

    fn shape(&self, r: Rect) -> Shape {
        let mut shapes = Vec::new();
        if self.shadow {
            let shadow = chamfer(r.translate(egui::vec2(0.0, 3.0)), self.cut);
            shapes.push(Shape::convex_polygon(shadow, Color32::from_black_alpha(90), Stroke::NONE));
        }
        shapes.push(Shape::convex_polygon(chamfer(r, self.cut), self.fill, self.stroke));
        // A short steel line along the lower cut, like a plate's bevel.
        if self.cut >= 6.0 {
            let k = self.cut * 0.5;
            shapes.push(Shape::line_segment(
                [egui::pos2(r.right() - 2.0, r.bottom() - self.cut - k + 2.0), egui::pos2(r.right() - self.cut - k + 2.0, r.bottom() - 2.0)],
                Stroke::new(1.0, STEEL.gamma_multiply(0.6)),
            ));
        }
        Shape::Vec(shapes)
    }
}

/// `Plate::panel().show(...)`.
pub fn plate<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    Plate::panel().show(ui, add)
}

/// A window in the field manual's style: chamfered plate, a dark header band
/// with the title in condensed capitals and a close button. Wraps
/// `egui::Window` (its id still comes from the title unless `id` is given).
pub struct FieldWindow<'a> {
    window: egui::Window<'static>,
    title: String,
    accent: Color32,
    open: Option<&'a mut bool>,
}

impl<'a> FieldWindow<'a> {
    pub fn new(title: impl Into<String>) -> Self {
        let title = title.into();
        let window = egui::Window::new(title.clone()).title_bar(false).collapsible(false).resizable(false).frame(egui::Frame::NONE);
        FieldWindow { window, title, accent: AMBER, open: None }
    }

    pub fn id(mut self, id: egui::Id) -> Self {
        self.window = self.window.id(id);
        self
    }

    pub fn anchor(mut self, align: Align2, offset: impl Into<Vec2>) -> Self {
        self.window = self.window.anchor(align, offset);
        self
    }

    pub fn default_width(mut self, width: f32) -> Self {
        self.window = self.window.default_width(width);
        self
    }

    /// The colour of the title's tick: amber, or red for danger.
    pub fn accent(mut self, color: Color32) -> Self {
        self.accent = color;
        self
    }

    /// Adds a close button, which sets `open` to false.
    pub fn open(mut self, open: &'a mut bool) -> Self {
        self.open = Some(open);
        self
    }

    pub fn show<R>(self, ctx: &egui::Context, add: impl FnOnce(&mut egui::Ui) -> R) -> Option<R> {
        let FieldWindow { window, title, accent, open } = self;
        if open.as_deref() == Some(&false) {
            return None;
        }
        let closable = open.is_some();
        let mut close = false;
        let shown = window.show(ctx, |ui| {
            let plate = Plate::panel().margin(egui::Margin { left: MARGIN as i8, right: MARGIN as i8, top: 0, bottom: MARGIN as i8 });
            plate
                .show(ui, |ui| {
                    let font = heading_font(17.0);
                    let galley = ui.painter().layout_no_wrap(title.to_uppercase(), font, CREAM_BRIGHT);
                    // Reserve the header: tick, title, close button.
                    let need = 10.0 + galley.size().x + if closable { 30.0 } else { 0.0 };
                    let (header, _) = ui.allocate_exact_size(egui::vec2(need, HEADER_H), egui::Sense::hover());
                    ui.add_space(4.0);
                    let r = add(ui);
                    // Now the content's width is known: the band spans it all.
                    let full = ui.min_rect();
                    let band = Rect::from_min_max(
                        egui::pos2(full.left() - MARGIN, header.top()),
                        egui::pos2(full.right() + MARGIN, header.bottom()),
                    );
                    let p = ui.painter();
                    p.add(Shape::convex_polygon(chamfer_top(band, CUT), PANEL_DEEP, Stroke::NONE));
                    p.line_segment([band.left_bottom(), band.right_bottom()], Stroke::new(1.0, OLIVE));
                    let tick = Rect::from_min_size(egui::pos2(full.left(), band.center().y - 6.0), egui::vec2(4.0, 12.0));
                    p.rect_filled(tick, 0.0, accent);
                    p.galley(egui::pos2(full.left() + 10.0, band.center().y - galley.size().y / 2.0), galley, CREAM_BRIGHT);
                    if closable {
                        let at = Rect::from_center_size(egui::pos2(band.right() - MARGIN - 8.0, band.center().y), egui::vec2(18.0, 18.0));
                        let resp = ui.interact(at, ui.id().with("field_window_close"), egui::Sense::click());
                        let color = if resp.hovered() { AMBER } else { CREAM_DIM };
                        let k = 4.5;
                        let c = at.center();
                        let p = ui.painter();
                        if resp.hovered() {
                            p.rect_stroke(at, 0.0, Stroke::new(1.0, AMBER), egui::StrokeKind::Inside);
                        }
                        p.line_segment([c + egui::vec2(-k, -k), c + egui::vec2(k, k)], Stroke::new(1.5, color));
                        p.line_segment([c + egui::vec2(-k, k), c + egui::vec2(k, -k)], Stroke::new(1.5, color));
                        close = resp.on_hover_text("Close").clicked();
                    }
                    r
                })
                .inner
        });
        if close {
            if let Some(open) = open {
                *open = false;
            }
        }
        shown.and_then(|s| s.inner)
    }
}

/// A header band: only the top-left corner cut.
fn chamfer_top(r: Rect, cut: f32) -> Vec<Pos2> {
    vec![
        egui::pos2(r.left() + cut, r.top()),
        egui::pos2(r.right(), r.top()),
        egui::pos2(r.right(), r.bottom()),
        egui::pos2(r.left(), r.bottom()),
        egui::pos2(r.left(), r.top() + cut),
    ]
}

/// A thin meter: the field manual's bar for needs, health and progress.
pub fn meter(ui: &mut egui::Ui, fraction: f32, width: f32, height: f32, color: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, PANEL_DEEP);
    let f = fraction.clamp(0.0, 1.0);
    if f > 0.0 {
        p.rect_filled(Rect::from_min_size(rect.min, egui::vec2(rect.width() * f, rect.height())), 0.0, color);
    }
    p.rect_stroke(rect, 0.0, Stroke::new(1.0, OLIVE_DIM), egui::StrokeKind::Outside);
    resp
}
