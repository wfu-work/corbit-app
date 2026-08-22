//! Compile-time identity for the running desktop client.
//!
//! Development bundles deliberately use a different application name and
//! bundle identifier, but the UI should also make that distinction visible
//! when the app is launched from an IDE, Finder, or a copied artifact.

pub(crate) const CHANNEL: &str = env!("CORBIT_BUILD_CHANNEL");
pub(crate) const PROFILE: &str = env!("CORBIT_BUILD_PROFILE");
pub(crate) const TARGET: &str = env!("CORBIT_BUILD_TARGET");

pub(crate) fn is_development() -> bool {
    CHANNEL == "dev" || PROFILE == "debug"
}

pub(crate) fn channel_label() -> &'static str {
    if is_development() {
        "开发版"
    } else {
        "正式版"
    }
}

pub(crate) fn version_label() -> String {
    format!(
        "Desktop {} · {} · {}",
        env!("CARGO_PKG_VERSION"),
        channel_label(),
        PROFILE
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_identity_is_embedded_and_non_empty() {
        assert!(!CHANNEL.is_empty());
        assert!(!PROFILE.is_empty());
        assert!(!TARGET.is_empty());
    }

    #[test]
    fn version_label_contains_package_and_profile_identity() {
        let label = version_label();

        assert!(label.contains(env!("CARGO_PKG_VERSION")));
        assert!(label.contains(PROFILE));
        assert!(label.contains(channel_label()));
    }
}
