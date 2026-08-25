use super::appearance::AppIconMode;

#[cfg(target_os = "macos")]
pub(super) fn apply(mode: AppIconMode, appearance_is_dark: bool) -> anyhow::Result<()> {
    const LIGHT_ICON: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/brand/corbit-app-icon-1024.png"
    ));
    const DARK_ICON: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/brand/corbit-app-icon-dark-1024.png"
    ));

    let bytes = if mode.resolves_to_dark(appearance_is_dark) {
        DARK_ICON
    } else {
        LIGHT_ICON
    };
    corbit_macos_interop::set_application_icon(bytes).map_err(anyhow::Error::msg)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn apply(_mode: AppIconMode, _appearance_is_dark: bool) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundled_png_icons_have_png_signatures() {
        for bytes in [
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/brand/corbit-app-icon-1024.png"
            )) as &[u8],
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/brand/corbit-app-icon-dark-1024.png"
            )) as &[u8],
        ] {
            assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        }
    }
}
