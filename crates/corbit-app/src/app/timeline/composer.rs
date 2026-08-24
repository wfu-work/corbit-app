//! Pure composer policy, labels, and attachment loading.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::path::PathBuf;

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
            if bytes.len() > MAX_PROMPT_ATTACHMENT_BYTES {
                return Err(format!("附件 {name} 超过 2 MB 上限"));
            }
            total_bytes += bytes.len();
            if total_bytes > MAX_PROMPT_ATTACHMENTS_TOTAL_BYTES {
                return Err("附件总大小不能超过 5 MB".to_owned());
            }
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
            Ok(ComposerAttachment {
                upload: corbit_client::AgentPromptAttachment {
                    name,
                    mime_type: mime_type.to_owned(),
                    data_base64: STANDARD.encode(&bytes),
                },
                size_bytes: bytes.len(),
            })
        })
        .collect()
}
