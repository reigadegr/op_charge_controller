use std::{
    env, fs, io,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
    thread,
    time::Duration,
};

use anyhow::Result;
use config::AtomicConfig;
use tracing::{error, info};
use utils::mask_val;

const BATTERY_STATUS_PATH: &str =
    "/sys/devices/platform/soc/soc:oplus,mms_gauge/oplus_mms/gauge/battery/status";
const UFCS_FORCE_VAL_PATH: &str = "/proc/oplus-votable/UFCS_CURR/force_val";
const UFCS_FORCE_ACTIVE_PATH: &str = "/proc/oplus-votable/UFCS_CURR/force_active";
static MASKS_DIR: LazyLock<io::Result<PathBuf>> = LazyLock::new(|| masks_dir(&env::current_exe()?));

fn masks_dir(executable: &Path) -> io::Result<PathBuf> {
    executable
        .parent()
        .map(|directory| directory.join("masks"))
        .ok_or_else(|| io::Error::other("无法获取当前 ELF 所在目录"))
}

pub struct Looper;

impl Looper {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    pub fn enter_loop(&mut self, config_manager: &Arc<AtomicConfig>) -> Result<()> {
        let mut was_charging = None;

        loop {
            match Self::get_battery_status() {
                Ok(is_charging) => {
                    let previously_charging = was_charging.replace(is_charging);
                    if let Some(previously_charging) = previously_charging
                        && previously_charging != is_charging
                    {
                        let message = if is_charging {
                            "进入充电"
                        } else {
                            "退出充电"
                        };
                        info!("{message}");
                    }

                    if is_charging && previously_charging != Some(true) {
                        Self::apply_ufcs_vote(config_manager);
                    }

                    let message = if is_charging {
                        "充电中"
                    } else {
                        "未充电"
                    };
                    info!("{message}");
                }
                Err(error) => error!("读取电池状态失败: {error}"),
            }

            thread::sleep(Duration::from_secs(1));
        }
    }

    fn get_battery_status() -> Result<bool> {
        let status = fs::read_to_string(BATTERY_STATUS_PATH)?;
        Ok(status.trim() == "Charging")
    }

    fn apply_ufcs_vote(config_manager: &Arc<AtomicConfig>) {
        let ufcs_max_vote = config_manager.get().ufcs_max_vote.to_string();
        let masks_dir = match &*MASKS_DIR {
            Ok(masks_dir) => masks_dir,
            Err(error) => {
                error!("获取 masks 目录失败: {error}");
                return;
            }
        };

        if let Err(error) = mask_val(
            &ufcs_max_vote,
            std::path::Path::new(UFCS_FORCE_VAL_PATH),
            masks_dir,
        ) {
            error!("设置 UFCS 最大电流失败: {error}");
        }

        if let Err(error) = mask_val("1", std::path::Path::new(UFCS_FORCE_ACTIVE_PATH), masks_dir) {
            error!("启用 UFCS 强制投票失败: {error}");
        }
    }
}

impl Default for Looper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::{io, path::PathBuf};

    use super::masks_dir;

    #[test]
    fn masks_directory_is_next_to_executable() -> io::Result<()> {
        let executable = PathBuf::from("/opt/op_charge_controller/op_charge_controller");

        assert_eq!(
            masks_dir(&executable)?,
            PathBuf::from("/opt/op_charge_controller/masks")
        );

        Ok(())
    }
}
