//! Keeps the machine awake while local Agent turns are running.

use anyhow::{Context as _, Result};

pub(super) struct SleepPreventer {
    #[cfg(target_os = "macos")]
    child: std::process::Child,
}

impl SleepPreventer {
    pub(super) fn start() -> Result<Self> {
        #[cfg(target_os = "macos")]
        {
            let child = std::process::Command::new("/usr/bin/caffeinate")
                .arg("-i")
                .spawn()
                .context("无法启动 macOS 防休眠服务")?;
            Ok(Self { child })
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err(anyhow::anyhow!("当前平台暂不支持任务运行时防休眠"))
        }
    }

    pub(super) const fn supported() -> bool {
        cfg!(target_os = "macos")
    }
}

impl Drop for SleepPreventer {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
