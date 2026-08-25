//! Small, auditable boundary around AppKit APIs whose generated bindings are
//! unsafe even when Corbit supplies a valid non-null image.

#[cfg(target_os = "macos")]
use objc2::{AllocAnyThread as _, MainThreadMarker, runtime::AnyObject};
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSApplication, NSBitmapImageFileType, NSBitmapImageRep, NSBitmapImageRepPropertyKey, NSImage,
    NSPasteboard, NSPasteboardTypePNG,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSData, NSDictionary};

/// Updates the running application's Dock and app-switcher icon from PNG data.
///
/// # Errors
///
/// Returns an error when called off the macOS main thread or when AppKit cannot
/// decode the supplied image.
#[cfg(target_os = "macos")]
pub fn set_application_icon(png: &[u8]) -> Result<(), &'static str> {
    let marker = MainThreadMarker::new().ok_or("应用图标只能在 macOS 主线程中更新")?;
    let data = NSData::with_bytes(png);
    let image = NSImage::initWithData(NSImage::alloc(), &data).ok_or("无法解析内置应用图标")?;
    let application = NSApplication::sharedApplication(marker);

    // SAFETY: `image` is a valid, non-null NSImage and AppKit retains it for
    // the application icon. MainThreadMarker proves this runs on the UI thread.
    unsafe { application.setApplicationIconImage(Some(&image)) };
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn set_application_icon(_png: &[u8]) -> Result<(), &'static str> {
    Err("当前平台不支持运行时应用图标切换")
}

/// Reads the macOS clipboard image as PNG, even when the same pasteboard item
/// also exposes a plain-text filename.
#[cfg(target_os = "macos")]
pub fn clipboard_png() -> Option<Vec<u8>> {
    clipboard_png_from(&NSPasteboard::generalPasteboard())
}

#[cfg(target_os = "macos")]
fn clipboard_png_from(pasteboard: &NSPasteboard) -> Option<Vec<u8>> {
    // SAFETY: `NSPasteboardTypePNG` is an immutable AppKit framework
    // constant whose lifetime covers the process.
    let png_type = unsafe { NSPasteboardTypePNG };
    if let Some(data) = pasteboard.dataForType(png_type) {
        return Some(data.to_vec());
    }

    let image = NSImage::initWithPasteboard(NSImage::alloc(), pasteboard)?;
    let properties: objc2::rc::Retained<NSDictionary<NSBitmapImageRepPropertyKey, AnyObject>> =
        NSDictionary::new();

    // SAFETY: AppKit owns the image representations and the property
    // dictionary has the exact key/value types required by this API.
    let png = unsafe {
        NSBitmapImageRep::representationOfImageRepsInArray_usingType_properties(
            &image.representations(),
            NSBitmapImageFileType::PNG,
            &properties,
        )
    }?;
    Some(png.to_vec())
}

/// Non-macOS builds do not have an AppKit clipboard.
#[cfg(not(target_os = "macos"))]
pub fn clipboard_png() -> Option<Vec<u8>> {
    None
}
