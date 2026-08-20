use anyhow::{Context as _, Result};
use async_channel::{Receiver, Sender};
use gpui::App;
use image::imageops::FilterType;
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
};

use super::show_main_window;

const SHOW_MENU_ID: &str = "corbit.tray.show";
const HIDE_MENU_ID: &str = "corbit.tray.hide";
const QUIT_MENU_ID: &str = "corbit.tray.quit";
const TEMPLATE_ICON_SIZE: u32 = 64;
const TEMPLATE_ICON_PADDING: u32 = 3;

const TEMPLATE_ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/brand/corbit-symbol-light-512.png"
));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TrayAction {
    Show,
    Hide,
    Quit,
}

pub(super) fn install(cx: &App) -> Result<()> {
    let (action_sender, action_receiver) = async_channel::unbounded();
    let tray_icon = build_tray_icon()?;
    install_event_handlers(action_sender);
    cx.spawn(async move |cx| {
        // The status item is removed when the final TrayIcon handle is dropped.
        // Keep it owned by this foreground task for the full application lifetime.
        let _tray_icon = tray_icon;
        process_actions(action_receiver, cx).await;
    })
    .detach();

    Ok(())
}

fn build_tray_icon() -> Result<TrayIcon> {
    let show_item = MenuItem::with_id(SHOW_MENU_ID, "显示 Corbit", true, None);
    let hide_item = MenuItem::with_id(HIDE_MENU_ID, "隐藏 Corbit", true, None);
    let separator = PredefinedMenuItem::separator();
    let quit_item = MenuItem::with_id(QUIT_MENU_ID, "退出 Corbit", true, None);
    let menu = Menu::with_items(&[&show_item, &hide_item, &separator, &quit_item])
        .context("failed to create the Corbit tray menu")?;

    TrayIconBuilder::new()
        .with_id("corbit.status-item")
        .with_tooltip("Corbit")
        .with_icon(load_template_icon()?)
        .with_icon_as_template(true)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_menu_on_right_click(true)
        .build()
        .context("failed to create the Corbit status item")
}

fn load_template_icon() -> Result<Icon> {
    let rgba = load_template_rgba()?;
    let (width, height) = rgba.dimensions();

    Icon::from_rgba(rgba.into_raw(), width, height)
        .context("failed to convert the Corbit tray icon to RGBA")
}

fn load_template_rgba() -> Result<image::RgbaImage> {
    let mut rgba = image::load_from_memory(TEMPLATE_ICON_PNG)
        .context("failed to decode the embedded Corbit tray icon")?
        .into_rgba8();

    // The exported light-surface symbol is flattened onto white. Convert its
    // distance from white into alpha so AppKit receives the monochrome mask it
    // expects for a template image and can adapt it to either menu-bar theme.
    for pixel in rgba.pixels_mut() {
        let source_alpha = u16::from(pixel.0[3]);
        let distance_from_white = u16::from(255 - pixel.0[0].min(pixel.0[1]).min(pixel.0[2]));
        let template_alpha =
            u8::try_from(source_alpha * distance_from_white / 255).unwrap_or(u8::MAX);
        pixel.0 = [0, 0, 0, template_alpha];
    }

    let (x, y, width, height) = visible_alpha_bounds(&rgba)
        .context("the embedded Corbit tray icon has no visible template pixels")?;
    let cropped = image::imageops::crop_imm(&rgba, x, y, width, height).to_image();
    let content_size = TEMPLATE_ICON_SIZE - TEMPLATE_ICON_PADDING * 2;
    let resized = image::DynamicImage::ImageRgba8(cropped)
        .resize(content_size, content_size, FilterType::Lanczos3)
        .into_rgba8();
    let mut template = image::RgbaImage::new(TEMPLATE_ICON_SIZE, TEMPLATE_ICON_SIZE);
    let offset_x = (TEMPLATE_ICON_SIZE - resized.width()) / 2;
    let offset_y = (TEMPLATE_ICON_SIZE - resized.height()) / 2;
    image::imageops::overlay(
        &mut template,
        &resized,
        i64::from(offset_x),
        i64::from(offset_y),
    );

    Ok(template)
}

fn visible_alpha_bounds(image: &image::RgbaImage) -> Option<(u32, u32, u32, u32)> {
    let mut min_x = image.width();
    let mut min_y = image.height();
    let mut max_x = 0;
    let mut max_y = 0;
    let mut has_visible_pixel = false;

    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel.0[3] == 0 {
            continue;
        }
        has_visible_pixel = true;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }

    has_visible_pixel.then_some((min_x, min_y, max_x - min_x + 1, max_y - min_y + 1))
}

fn install_event_handlers(action_sender: Sender<TrayAction>) {
    let menu_sender = action_sender.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let action = action_for_menu_id(event.id());
        if let Some(action) = action {
            let _ = menu_sender.try_send(action);
        }
    }));

    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if matches!(
            event,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
        ) {
            let _ = action_sender.try_send(TrayAction::Show);
        }
    }));
}

fn action_for_menu_id(id: &MenuId) -> Option<TrayAction> {
    match id.as_ref() {
        SHOW_MENU_ID => Some(TrayAction::Show),
        HIDE_MENU_ID => Some(TrayAction::Hide),
        QUIT_MENU_ID => Some(TrayAction::Quit),
        _ => None,
    }
}

async fn process_actions(action_receiver: Receiver<TrayAction>, cx: &mut gpui::AsyncApp) {
    while let Ok(action) = action_receiver.recv().await {
        let should_quit = action == TrayAction::Quit;
        let result = cx.update(|cx| match action {
            TrayAction::Show => show_main_window(cx),
            TrayAction::Hide => {
                cx.hide();
                Ok(())
            }
            TrayAction::Quit => {
                cx.quit();
                Ok(())
            }
        });

        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("Corbit tray action failed: {error:#}"),
            Err(_) => break,
        }

        if should_quit {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_template_icon_is_valid_rgba() {
        let rgba = load_template_rgba().expect("tray template icon should decode");
        assert_eq!(rgba.width(), TEMPLATE_ICON_SIZE);
        assert_eq!(rgba.height(), TEMPLATE_ICON_SIZE);
        assert!(rgba.pixels().any(|pixel| pixel.0[3] == 0));
        assert!(rgba.pixels().any(|pixel| pixel.0[3] > 0));
        assert!(rgba.pixels().all(|pixel| pixel.0[..3] == [0, 0, 0]));

        let (x, y, width, height) =
            visible_alpha_bounds(&rgba).expect("tray template should have visible bounds");
        assert!(x <= TEMPLATE_ICON_PADDING + 1);
        assert!(y <= TEMPLATE_ICON_PADDING + 1);
        assert!(width >= TEMPLATE_ICON_SIZE - (TEMPLATE_ICON_PADDING + 1) * 2);
        assert!(height >= TEMPLATE_ICON_SIZE - (TEMPLATE_ICON_PADDING + 1) * 2);
    }

    #[test]
    fn tray_menu_ids_map_to_expected_actions() {
        assert_eq!(
            action_for_menu_id(&MenuId::new(SHOW_MENU_ID)),
            Some(TrayAction::Show)
        );
        assert_eq!(
            action_for_menu_id(&MenuId::new(HIDE_MENU_ID)),
            Some(TrayAction::Hide)
        );
        assert_eq!(
            action_for_menu_id(&MenuId::new(QUIT_MENU_ID)),
            Some(TrayAction::Quit)
        );
        assert_eq!(action_for_menu_id(&MenuId::new("other")), None);
    }
}
