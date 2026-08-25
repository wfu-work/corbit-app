//! Pure composer policy, labels, and attachment loading.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gpui::{ClipboardEntry, ClipboardItem, Image, ImageFormat};
use std::{path::PathBuf, sync::Arc};

use super::ComposerAttachment;

pub(in crate::app) const MAX_PROMPT_ATTACHMENTS: usize = 3;
const MAX_PROMPT_ATTACHMENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_PROMPT_ATTACHMENTS_TOTAL_BYTES: usize = 5 * 1024 * 1024;

pub(super) fn context_window_percent(used_tokens: u64, context_window: u64) -> u8 {
    if context_window == 0 {
        return 0;
    }

    let used_tokens = u128::from(used_tokens);
    let context_window = u128::from(context_window);
    let rounded = (used_tokens * 100 + context_window / 2) / context_window;
    rounded.min(100) as u8
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PermissionModeCopy {
    pub(super) title: &'static str,
    pub(super) description: &'static str,
}

pub(super) fn permission_mode_copy(mode: corbit_client::AgentPermissionMode) -> PermissionModeCopy {
    match mode {
        corbit_client::AgentPermissionMode::ReadOnly => PermissionModeCopy {
            title: "请求批准",
            description: "编辑外部文件和使用互联网时始终询问",
        },
        corbit_client::AgentPermissionMode::WorkspaceWrite => PermissionModeCopy {
            title: "帮我批准",
            description: "自动审查低风险权限请求，高风险操作仍会询问",
        },
        corbit_client::AgentPermissionMode::FullAccess => PermissionModeCopy {
            title: "完全访问权限",
            description: "可不受限制地访问互联网和你电脑上的任何文件",
        },
    }
}

pub(super) fn attachment_size_label(size: usize) -> String {
    format!("{} KB", size.div_ceil(1024))
}

pub(in crate::app) fn load_prompt_attachments(
    paths: Vec<PathBuf>,
    available_slots: usize,
    existing_bytes: usize,
) -> Result<Vec<ComposerAttachment>, String> {
    if paths.len() > available_slots {
        return Err(format!(
            "每条消息最多可添加 {MAX_PROMPT_ATTACHMENTS} 个附件"
        ));
    }

    let mut total_bytes = existing_bytes;
    paths
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .ok_or_else(|| format!("附件名称无效：{}", path.display()))?
                .to_owned();
            let bytes =
                std::fs::read(&path).map_err(|error| format!("无法读取附件 {name}：{error}"))?;
            validate_attachment_size(&name, bytes.len(), total_bytes)?;
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let mime_type = match extension.as_str() {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "webp" => "image/webp",
                _ => {
                    std::str::from_utf8(&bytes)
                        .map_err(|_| format!("附件 {name} 不是支持的图片或 UTF-8 文本文件"))?;
                    "text/plain"
                }
            };
            let preview = ImageFormat::from_mime_type(mime_type)
                .map(|format| Arc::new(Image::from_bytes(format, bytes.clone())));
            let attachment = encode_attachment(name, mime_type, &bytes);
            total_bytes += attachment.size_bytes;
            Ok(ComposerAttachment {
                preview,
                ..attachment
            })
        })
        .collect()
}

pub(in crate::app) fn load_clipboard_image(
    image: &Image,
    sequence: usize,
    existing_count: usize,
    existing_bytes: usize,
) -> Result<ComposerAttachment, String> {
    if existing_count >= MAX_PROMPT_ATTACHMENTS {
        return Err(format!(
            "每条消息最多可添加 {MAX_PROMPT_ATTACHMENTS} 个附件"
        ));
    }

    let (mime_type, extension) = match image.format() {
        ImageFormat::Png => ("image/png", "png"),
        ImageFormat::Jpeg => ("image/jpeg", "jpg"),
        ImageFormat::Webp => ("image/webp", "webp"),
        ImageFormat::Gif => ("image/gif", "gif"),
        _ => {
            return Err("暂不支持粘贴 SVG、BMP 或 TIFF 图片，请使用 PNG、JPEG、WebP 或 GIF".into());
        }
    };
    let name = format!("clipboard-image-{sequence}.{extension}");
    let bytes = image.bytes();
    let attachment = build_attachment(name, mime_type, bytes, existing_bytes)?;
    Ok(ComposerAttachment {
        preview: Some(Arc::new(image.clone())),
        ..attachment
    })
}

pub(in crate::app) fn prompt_clipboard_image(clipboard: &ClipboardItem) -> Option<Image> {
    if let Some(image) = clipboard.entries().iter().find_map(|entry| match entry {
        ClipboardEntry::Image(image) => Some(image.clone()),
        ClipboardEntry::String(_) => None,
    }) {
        return Some(image);
    }

    // GPUI 0.2 checks macOS plain text before image types. Some applications
    // publish both a PNG and a filename, which otherwise pastes the filename
    // into the editor and hides the actual image from callers.
    #[cfg(target_os = "macos")]
    if let Some(bytes) = corbit_macos_interop::clipboard_png() {
        return Some(Image::from_bytes(ImageFormat::Png, bytes));
    }

    None
}

fn build_attachment(
    name: String,
    mime_type: &str,
    bytes: &[u8],
    existing_bytes: usize,
) -> Result<ComposerAttachment, String> {
    validate_attachment_size(&name, bytes.len(), existing_bytes)?;
    Ok(encode_attachment(name, mime_type, bytes))
}

fn validate_attachment_size(
    name: &str,
    size_bytes: usize,
    existing_bytes: usize,
) -> Result<(), String> {
    if size_bytes > MAX_PROMPT_ATTACHMENT_BYTES {
        return Err(format!("附件 {name} 超过 2 MB 上限"));
    }
    let total_bytes = existing_bytes
        .checked_add(size_bytes)
        .ok_or_else(|| "附件总大小不能超过 5 MB".to_owned())?;
    if total_bytes > MAX_PROMPT_ATTACHMENTS_TOTAL_BYTES {
        return Err("附件总大小不能超过 5 MB".to_owned());
    }
    Ok(())
}

fn encode_attachment(name: String, mime_type: &str, bytes: &[u8]) -> ComposerAttachment {
    ComposerAttachment {
        upload: corbit_client::AgentPromptAttachment {
            name,
            mime_type: mime_type.to_owned(),
            data_base64: STANDARD.encode(bytes),
        },
        size_bytes: bytes.len(),
        preview: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_png_becomes_an_image_attachment() {
        let image = Image::from_bytes(ImageFormat::Png, vec![1, 2, 3]);
        let attachment = load_clipboard_image(&image, 1, 0, 0).expect("png should load");

        assert_eq!(attachment.upload.name, "clipboard-image-1.png");
        assert_eq!(attachment.upload.mime_type, "image/png");
        assert_eq!(attachment.upload.data_base64, "AQID");
        assert_eq!(attachment.size_bytes, 3);
        assert!(attachment.preview.is_some());
    }

    #[test]
    fn clipboard_item_image_is_selected_for_prompt_preview() {
        let image = Image::from_bytes(ImageFormat::Png, vec![1, 2, 3]);
        let clipboard = ClipboardItem::new_image(&image);

        assert_eq!(prompt_clipboard_image(&clipboard), Some(image));
    }

    #[test]
    fn clipboard_supported_formats_get_stable_names_and_mime_types() {
        let cases = [
            (ImageFormat::Png, "png", "image/png"),
            (ImageFormat::Jpeg, "jpg", "image/jpeg"),
            (ImageFormat::Webp, "webp", "image/webp"),
            (ImageFormat::Gif, "gif", "image/gif"),
        ];

        for (index, (format, extension, mime_type)) in cases.into_iter().enumerate() {
            let sequence = index + 4;
            let image = Image::from_bytes(format, vec![u8::try_from(index).unwrap_or_default()]);
            let attachment =
                load_clipboard_image(&image, sequence, 0, 0).expect("format should load");
            assert_eq!(
                attachment.upload.name,
                format!("clipboard-image-{sequence}.{extension}")
            );
            assert_eq!(attachment.upload.mime_type, mime_type);
        }
    }

    #[test]
    fn clipboard_rejects_unsupported_formats_and_limits() {
        let bmp = Image::from_bytes(ImageFormat::Bmp, vec![1]);
        assert!(
            load_clipboard_image(&bmp, 1, 0, 0)
                .expect_err("bmp should be rejected")
                .contains("不支持")
        );

        let image = Image::from_bytes(ImageFormat::Png, vec![0; MAX_PROMPT_ATTACHMENT_BYTES + 1]);
        assert!(
            load_clipboard_image(&image, 1, 0, 0)
                .expect_err("large image should be rejected")
                .contains("2 MB")
        );
        assert!(
            load_clipboard_image(&Image::from_bytes(ImageFormat::Png, vec![1]), 1, 3, 0)
                .expect_err("fourth attachment should be rejected")
                .contains('3')
        );
        assert!(
            load_clipboard_image(
                &Image::from_bytes(ImageFormat::Png, vec![0; 2]),
                1,
                0,
                MAX_PROMPT_ATTACHMENTS_TOTAL_BYTES - 1,
            )
            .expect_err("total attachment size should be enforced")
            .contains("5 MB")
        );
    }
}
