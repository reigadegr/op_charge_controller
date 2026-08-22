use std::{fs, sync::Arc, thread, time::Duration};

use anyhow::Result;
use config::AtomicConfig;
use tracing::{error, info};

const BATTERY_STATUS_PATH: &str =
    "/sys/devices/platform/soc/soc:oplus,mms_gauge/oplus_mms/gauge/battery/status";

pub struct Looper;

impl Looper {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    pub fn enter_loop(&mut self, _config_manager: &Arc<AtomicConfig>) -> Result<()> {
        let mut was_charging = None;

        loop {
            match Self::get_battery_status() {
                Ok(is_charging) => {
                    if let Some(previously_charging) = was_charging.replace(is_charging)
                        && previously_charging != is_charging
                    {
                        let message = if is_charging {
                            "进入充电"
                        } else {
                            "退出充电"
                        };
                        info!("{message}");
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
}

impl Default for Looper {
    fn default() -> Self {
        Self::new()
    }
}
