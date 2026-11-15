use eframe::egui::{self, *};

pub const BG: Color32 = Color32::from_rgb(18, 23, 25);
pub const PANEL: Color32 = Color32::from_rgb(27, 34, 37);
pub const BORDER: Color32 = Color32::from_rgb(49, 61, 65);
pub const TEXT: Color32 = Color32::from_rgb(232, 238, 234);
pub const MUTED: Color32 = Color32::from_rgb(142, 160, 161);
pub const ACCENT: Color32 = Color32::from_rgb(159, 227, 193);

pub fn configure(ctx: &Context) {
    let mut style = (*ctx.style_of(Theme::Dark)).clone();
    style.visuals = Visuals::dark();
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = PANEL;
    style.visuals.extreme_bg_color = BG;
    style.visuals.faint_bg_color = PANEL;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.selection.bg_fill = Color32::from_rgb(48, 81, 69);
    style.visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    style.visuals.hyperlink_color = ACCENT;
    style.visuals.window_corner_radius = CornerRadius::same(12);
    style.visuals.window_stroke = Stroke::new(1.0, BORDER);
    for widgets in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
    ] {
        widgets.corner_radius = CornerRadius::same(7);
        widgets.bg_stroke = Stroke::new(1.0, BORDER);
    }
    style.visuals.widgets.inactive.bg_fill = PANEL;
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(43, 57, 58);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(49, 74, 64);
    style.spacing.item_spacing = vec2(10.0, 10.0);
    style.spacing.button_padding = vec2(12.0, 8.0);
    style.spacing.interact_size.y = 32.0;
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(13.0));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(11.0));
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(24.0));
    ctx.set_style_of(Theme::Dark, style);
    ctx.set_theme(Theme::Dark);
}

#[derive(Clone, Copy)]
pub enum Icon {
    Plus,
    Paste,
    Copy,
    Trash,
    Edit,
    Expand,
    Collapse,
    Back,
    Upload,
    Server,
    Close,
    Clock,
    Check,
}

// Draw a shared set of 24px line icons; no font or image runtime is needed.
pub fn icon(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let point = |x: f32, y: f32| rect.min + vec2(x / 24.0 * rect.width(), y / 24.0 * rect.height());
    let stroke = Stroke::new(1.6, color);
    let line = |points: &[[f32; 2]]| {
        painter.add(Shape::line(
            points.iter().map(|p| point(p[0], p[1])).collect(),
            stroke,
        ));
    };
    let box_at = |x: f32, y: f32, w: f32, h: f32| {
        painter.rect_stroke(
            Rect::from_min_max(point(x, y), point(x + w, y + h)),
            2.0,
            stroke,
            StrokeKind::Inside,
        );
    };
    match icon {
        Icon::Plus => {
            line(&[[12., 5.], [12., 19.]]);
            line(&[[5., 12.], [19., 12.]]);
        }
        Icon::Close => {
            line(&[[6., 6.], [18., 18.]]);
            line(&[[18., 6.], [6., 18.]]);
        }
        Icon::Back => {
            line(&[[11., 5.], [4., 12.], [11., 19.]]);
            line(&[[4., 12.], [21., 12.]]);
        }
        Icon::Check => line(&[[4., 12.], [9., 17.], [20., 6.]]),
        Icon::Copy => {
            box_at(8., 8., 12., 13.);
            line(&[[5., 16.], [3., 16.], [3., 3.], [15., 3.], [15., 5.]]);
        }
        Icon::Paste => {
            box_at(5., 5., 14., 16.);
            box_at(9., 2., 6., 5.);
            line(&[[9., 12.], [15., 12.]]);
            line(&[[9., 16.], [15., 16.]]);
        }
        Icon::Trash => {
            line(&[[4., 6.], [20., 6.]]);
            line(&[[6., 6.], [7., 21.], [17., 21.], [18., 6.]]);
            line(&[[9., 6.], [9., 3.], [15., 3.], [15., 6.]]);
            line(&[[10., 10.], [10., 17.]]);
            line(&[[14., 10.], [14., 17.]]);
        }
        Icon::Edit => {
            line(&[
                [4., 16.],
                [16., 4.],
                [20., 8.],
                [8., 20.],
                [3., 21.],
                [4., 16.],
                [8., 20.],
            ]);
            line(&[[13., 7.], [17., 11.]]);
        }
        Icon::Expand => {
            line(&[[9., 3.], [3., 3.], [3., 9.]]);
            line(&[[15., 3.], [21., 3.], [21., 9.]]);
            line(&[[3., 15.], [3., 21.], [9., 21.]]);
            line(&[[15., 21.], [21., 21.], [21., 15.]]);
        }
        Icon::Collapse => {
            line(&[[3., 9.], [9., 9.], [9., 3.]]);
            line(&[[15., 3.], [15., 9.], [21., 9.]]);
            line(&[[3., 15.], [9., 15.], [9., 21.]]);
            line(&[[15., 21.], [15., 15.], [21., 15.]]);
        }
        Icon::Upload => {
            line(&[[12., 16.], [12., 3.]]);
            line(&[[7., 8.], [12., 3.], [17., 8.]]);
            line(&[[4., 14.], [4., 21.], [20., 21.], [20., 14.]]);
        }
        Icon::Server => {
            box_at(3., 3., 18., 14.);
            line(&[[8., 21.], [16., 21.]]);
            line(&[[12., 17.], [12., 21.]]);
            line(&[[7., 7.], [10., 10.], [7., 13.]]);
            line(&[[13., 13.], [17., 13.]]);
        }
        Icon::Clock => {
            painter.circle_stroke(rect.center(), rect.width() * 0.38, stroke);
            line(&[[12., 6.], [12., 12.], [16., 14.]]);
        }
    }
}

pub fn icon_button(ui: &mut Ui, glyph: Icon, label: &str) -> Response {
    let response = ui.add(Button::new("").min_size(vec2(32.0, 32.0)).frame(false));
    response.widget_info(|| WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    icon(
        ui.painter(),
        Rect::from_center_size(response.rect.center(), vec2(18., 18.)),
        glyph,
        if response.hovered() { ACCENT } else { MUTED },
    );
    response.on_hover_text(label)
}

pub fn left_label(ui: &mut Ui, rect: Rect, label: Label) -> Response {
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| ui.add(label),
    )
    .inner
}
