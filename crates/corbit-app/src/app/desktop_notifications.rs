//! Small native desktop-notification bridge used by task lifecycle preferences.

#[cfg(target_os = "macos")]
#[allow(deprecated)]
pub(super) fn send(title: &str, body: &str, play_sound: bool) {
    use objc2::AnyThread as _;
    use objc2_foundation::{NSString, NSUserNotification, NSUserNotificationCenter};

    let notification = NSUserNotification::init(NSUserNotification::alloc());
    let title = NSString::from_str(title);
    let body = NSString::from_str(body);
    notification.setTitle(Some(&title));
    notification.setInformativeText(Some(&body));
    if play_sound {
        let sound = NSString::from_str("NSUserNotificationDefaultSoundName");
        notification.setSoundName(Some(&sound));
    }
    NSUserNotificationCenter::defaultUserNotificationCenter().deliverNotification(&notification);
}

#[cfg(not(target_os = "macos"))]
pub(super) fn send(_title: &str, _body: &str, _play_sound: bool) {}
