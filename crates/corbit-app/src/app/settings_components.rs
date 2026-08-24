use super::*;
use gpui::ElementId;
use gpui_component::Side;
use std::rc::Rc;

type SettingsSwitchHandler = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub(super) struct SettingsSwitch {
    id: ElementId,
    checked: bool,
    disabled: bool,
    on_click: Option<SettingsSwitchHandler>,
}

impl SettingsSwitch {
    pub(super) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub(super) fn on_click<F>(mut self, handler: F) -> Self
    where
        F: Fn(&bool, &mut Window, &mut App) + 'static,
    {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for SettingsSwitch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        const TRACK_WIDTH: f32 = 32.;
        const TRACK_HEIGHT: f32 = 20.;
        const THUMB_SIZE: f32 = 16.;
        const INSET: f32 = 2.;

        let checked = self.checked;
        let disabled = self.disabled;
        let on_click = self.on_click.clone();
        let toggle_state = window.use_keyed_state(self.id.clone(), cx, |_, _| checked);
        let (track, hover, active) = if checked {
            (0x2a_86ff, 0x3b_92ff, 0x23_79e8)
        } else if is_dark_mode() {
            (0x2c_2c2c, 0x34_3436, 0x3a_3a3c)
        } else {
            (0xd1_d1d6, 0xc7_c7cc, 0xb9_b9be)
        };

        div()
            .id(self.id.clone())
            .relative()
            .flex()
            .items_center()
            .w(px(TRACK_WIDTH))
            .h(px(TRACK_HEIGHT))
            .border(px(INSET))
            .border_color(gpui::transparent_black())
            .rounded(px(TRACK_HEIGHT / 2.))
            .bg(fixed_rgb(track))
            .when(disabled, |this| this.opacity(0.48))
            .when(!disabled, |this| {
                this.cursor_pointer()
                    .hover(move |this| this.bg(fixed_rgb(hover)))
                    .active(move |this| this.bg(fixed_rgb(active)))
            })
            .child(
                div()
                    .relative()
                    .size(px(THUMB_SIZE))
                    .rounded(px(THUMB_SIZE / 2.))
                    .bg(fixed_rgb(0xff_ffff))
                    .shadow_sm()
                    .map(|thumb| {
                        let previous = *toggle_state.read(cx);
                        let max_x = px(TRACK_WIDTH - THUMB_SIZE - INSET * 2.);
                        if !disabled && previous != checked {
                            let duration = Duration::from_secs_f64(0.15);
                            cx.spawn({
                                let toggle_state = toggle_state.clone();
                                async move |cx| {
                                    cx.background_executor().timer(duration).await;
                                    _ = toggle_state.update(cx, |state, _| *state = checked);
                                }
                            })
                            .detach();

                            thumb
                                .with_animation(
                                    ElementId::NamedInteger(
                                        "settings-switch-move".into(),
                                        u64::from(checked),
                                    ),
                                    Animation::new(duration),
                                    move |thumb, delta| {
                                        let x = if checked {
                                            max_x * delta
                                        } else {
                                            max_x - max_x * delta
                                        };
                                        thumb.left(x)
                                    },
                                )
                                .into_any_element()
                        } else {
                            thumb
                                .left(if checked { max_x } else { px(0.) })
                                .into_any_element()
                        }
                    }),
            )
            .when_some(on_click.filter(|_| !disabled), |this, on_click| {
                let toggle_state = toggle_state.clone();
                this.on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    toggle_state.update(cx, |state, _| *state = checked);
                    on_click(&!checked, window, cx);
                })
            })
    }
}

pub(super) fn settings_switch(id: impl Into<ElementId>, checked: bool) -> SettingsSwitch {
    SettingsSwitch {
        id: id.into(),
        checked,
        disabled: false,
        on_click: None,
    }
}

#[derive(IntoElement)]
pub(super) struct SettingsCard {
    title: SharedString,
    children: Vec<AnyElement>,
}

impl ParentElement for SettingsCard {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for SettingsCard {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .v_flex()
            .w(px(720.))
            .max_w_full()
            .gap_3()
            .child(
                div()
                    .text_size(font_px(FONT_SIZE_SM))
                    .font_semibold()
                    .child(self.title),
            )
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_3()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(rgb(COLOR_BORDER_HEAVY))
                    .bg(rgb(COLOR_EDITOR))
                    .p_4()
                    .children(self.children),
            )
    }
}

pub(super) fn settings_card(title: impl Into<SharedString>) -> SettingsCard {
    SettingsCard {
        title: title.into(),
        children: Vec::new(),
    }
}

/// Standard single-line input used by settings pages.
///
/// The generic small input is only 24px tall, which makes it look undersized
/// beside the 30-32px buttons and selects used throughout settings.
pub(super) fn settings_input(state: &Entity<InputState>) -> Input {
    Input::new(state).with_size(Size::Medium).rounded(px(8.))
}

fn settings_action_button_base(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .small()
        .h(px(30.))
        .px_3()
        .rounded(px(8.))
        .font_medium()
}

/// Standard neutral action used inside settings cards.
///
/// The component owns its surface colors instead of using the generic outline
/// variant, whose page-background fill looks too dark when stretched inside a
/// settings card.
pub(super) fn settings_action_button(id: impl Into<ElementId>, cx: &App) -> Button {
    settings_action_button_base(id).custom(
        ButtonCustomVariant::new(cx)
            .color(rgb(COLOR_SURFACE_SECONDARY).into())
            .foreground(rgb(COLOR_TEXT).into())
            .border(rgb(COLOR_BORDER_HEAVY).into())
            .hover(rgb(COLOR_BORDER).into())
            .active(rgb(COLOR_BORDER_HEAVY).into()),
    )
}

pub(super) fn settings_primary_action_button(id: impl Into<ElementId>, cx: &App) -> Button {
    settings_action_button_base(id).custom(
        ButtonCustomVariant::new(cx)
            .color(rgb(COLOR_BORDER_HEAVY).into())
            .foreground(rgb(COLOR_TEXT).into())
            .border(rgb(COLOR_BORDER_HEAVY).into())
            .hover(sidebar_row_hover_rgb().into())
            .active(sidebar_row_active_rgb().into()),
    )
}

pub(super) fn settings_danger_action_button(id: impl Into<ElementId>, cx: &App) -> Button {
    settings_action_button_base(id).custom(
        ButtonCustomVariant::new(cx)
            .color(rgb(COLOR_SURFACE_SECONDARY).into())
            .foreground(rgb(COLOR_ERROR).into())
            .border(rgb(COLOR_BORDER_HEAVY).into())
            .hover(rgb(COLOR_BORDER).into())
            .active(rgb(COLOR_BORDER_HEAVY).into()),
    )
}

pub(super) fn settings_quiet_action_button(id: impl Into<ElementId>) -> Button {
    settings_action_button_base(id).ghost()
}

pub(super) fn settings_select_button(id: impl Into<ElementId>, cx: &App) -> Button {
    let (background, border, hover, active) = if is_dark_mode() {
        (0x38_383a, 0x4d_4d4f, 0x40_4042, 0x46_4648)
    } else {
        (
            COLOR_SURFACE_SECONDARY,
            COLOR_BORDER_HEAVY,
            COLOR_BORDER,
            COLOR_BORDER_HEAVY,
        )
    };

    Button::new(id)
        .small()
        .h(px(32.))
        .px_3()
        .rounded(px(10.))
        .font_medium()
        .custom(
            ButtonCustomVariant::new(cx)
                .color(fixed_rgb(background).into())
                .foreground(rgb(COLOR_TEXT).into())
                .border(fixed_rgb(border).into())
                .hover(fixed_rgb(hover).into())
                .active(fixed_rgb(active).into()),
        )
        .dropdown_caret(true)
}

pub(super) fn settings_select_menu(menu: PopupMenu) -> PopupMenu {
    menu.check_side(Side::Right)
}
