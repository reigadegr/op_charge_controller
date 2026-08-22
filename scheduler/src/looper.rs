use std::{fs, sync::Arc, thread, time::Duration};

use anyhow::Result;
use config::AtomicConfig;
use log::{error, info};

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
            match Self::read_battery_status() {
                Ok(status) => {
                    let is_charging = status == "Charging";

                    if let Some(previously_charging) = was_charging
                        && previously_charging != is_charging
                    {
                        if is_charging {
                            info!("进入充电");
                        } else {
                            info!("退出充电");
                        }
                    }

                    if is_charging {
                        info!("充电中");
                    } else {
                        info!("未充电");
                    }
                    was_charging = Some(is_charging);
                }
                Err(error) => error!("读取电池状态失败: {error}"),
            }

            thread::sleep(Duration::from_secs(1));
        }
    }

    fn read_battery_status() -> Result<String> {
        let status = fs::read_to_string(BATTERY_STATUS_PATH)?;
        Ok(status.trim().to_owned())
    }
}

impl Default for Looper {
    fn default() -> Self {
        Self::new()
    }
}
