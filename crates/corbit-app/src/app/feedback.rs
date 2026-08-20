use super::*;
use gpui_component::{
    IconName,
    notification::{Notification, NotificationType},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FeedbackKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub(super) struct AppFeedback {
    generation: u64,
    kind: FeedbackKind,
    message: String,
}

struct DialogFeedback;

impl FeedbackKind {
    fn title(self) -> &'static str {
        match self {
            Self::Info => "提示",
            Self::Success => "操作成功",
            Self::Warning => "请注意",
            Self::Error => "操作失败",
        }
    }

    fn notification_type(self) -> NotificationType {
        match self {
            Self::Info => NotificationType::Info,
            Self::Success => NotificationType::Success,
            Self::Warning => NotificationType::Warning,
            Self::Error => NotificationType::Error,
        }
    }

    fn autohide(self) -> bool {
        self != Self::Error
    }

    fn timeout(self) -> Duration {
        match self {
            Self::Warning => Duration::from_secs(7),
            Self::Info | Self::Success => Duration::from_secs(5),
            Self::Error => Duration::ZERO,
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Info => IconName::Info,
            Self::Success => IconName::CircleCheck,
            Self::Warning => IconName::TriangleAlert,
            Self::Error => IconName::CircleX,
        }
    }

    fn color(self) -> u32 {
        match self {
            Self::Info => COLOR_TEXT_SECONDARY,
            Self::Success => COLOR_SUCCESS,
            Self::Warning => COLOR_WARNING,
            Self::Error => COLOR_ERROR,
        }
    }
}

pub(super) fn app_notification(
    kind: FeedbackKind,
    message: impl Into<SharedString>,
) -> Notification {
    Notification::new()
        .id::<DialogFeedback>()
        .with_type(kind.notification_type())
        .title(kind.title())
        .message(message)
        .autohide(kind.autohide())
        .w(px(420.))
}

pub(super) fn push_app_notification(
    kind: FeedbackKind,
    message: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
) {
    window.push_notification(app_notification(kind, message), cx);
}

impl ConnectionView {
    pub(super) fn show_feedback(
        &mut self,
        kind: FeedbackKind,
        message: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let message = message.into();
        self.detail.clone_from(&message);
        self.feedback_generation = self.feedback_generation.wrapping_add(1);
        let generation = self.feedback_generation;
        self.feedback = Some(AppFeedback {
            generation,
            kind,
            message,
        });

        if kind.autohide() {
            cx.spawn(async move |view, cx| {
                Timer::after(kind.timeout()).await;
                let Some(view) = view.upgrade() else {
                    return;
                };
                let _ = view.update(cx, |view, cx| {
                    if view.feedback.as_ref().map(|feedback| feedback.generation)
                        == Some(generation)
                    {
                        view.feedback = None;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    fn dismiss_feedback(&mut self, generation: u64, cx: &mut Context<Self>) {
        if self.feedback.as_ref().map(|feedback| feedback.generation) == Some(generation) {
            self.feedback = None;
            cx.notify();
        }
    }

    pub(super) fn render_feedback(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let feedback = self.feedback.as_ref()?;
        let generation = feedback.generation;
        let kind = feedback.kind;
        let color = rgb(kind.color());

        Some(
            div()
                .absolute()
                .top(px(64.))
                .right(px(20.))
                .w(px(420.))
                .child(
                    div()
                        .id(("app-feedback", generation))
                        .h_flex()
                        .relative()
                        .items_start()
                        .overflow_hidden()
                        .occlude()
                        .rounded(px(12.))
                        .border_1()
                        .border_color(rgb(COLOR_BORDER_HEAVY))
                        .bg(rgb(COLOR_SURFACE_SECONDARY))
                        .shadow_md()
                        .pl_4()
                        .pr_2()
                        .py_3()
                        .gap_3()
                        .child(Icon::new(kind.icon()).size(px(18.)).text_color(color))
                        .child(
                            div()
                                .v_flex()
                                .flex_1()
                                .min_w(px(0.))
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_SM))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(COLOR_TEXT))
                                        .child(kind.title()),
                                )
                                .child(
                                    div()
                                        .text_size(font_px(FONT_SIZE_SM))
                                        .text_color(rgb(COLOR_TEXT_SECONDARY))
                                        .child(feedback.message.clone()),
                                ),
                        )
                        .child(
                            Button::new(("dismiss-app-feedback", generation))
                                .ghost()
                                .xsmall()
                                .icon(AppIcon::Close)
                                .tooltip("关闭提示")
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.dismiss_feedback(generation, cx);
                                })),
                        ),
                )
                .into_any_element(),
        )
    }

    pub(super) fn show_info(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.show_feedback(FeedbackKind::Info, message, cx);
    }

    pub(super) fn show_success(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.show_feedback(FeedbackKind::Success, message, cx);
    }

    pub(super) fn show_warning(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.show_feedback(FeedbackKind::Warning, message, cx);
    }

    pub(super) fn show_error(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.show_feedback(FeedbackKind::Error, message, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_errors_require_manual_dismissal() {
        assert!(!FeedbackKind::Error.autohide());
        assert!(FeedbackKind::Warning.autohide());
        assert!(FeedbackKind::Success.autohide());
        assert!(FeedbackKind::Info.autohide());
        assert_eq!(FeedbackKind::Error.timeout(), Duration::ZERO);
        assert!(FeedbackKind::Warning.timeout() > FeedbackKind::Success.timeout());
    }

    #[test]
    fn feedback_titles_are_action_oriented() {
        assert_eq!(FeedbackKind::Error.title(), "操作失败");
        assert_eq!(FeedbackKind::Success.title(), "操作成功");
        assert_eq!(FeedbackKind::Warning.title(), "请注意");
        assert_eq!(FeedbackKind::Info.title(), "提示");
    }
}
