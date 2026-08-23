use std::{fs, path::Path, sync::Arc, thread, time::Duration};

use anyhow::Result;
use config::AtomicConfig;
use tracing::{error, info, warn};
use utils::{BccParamsReader, mask_val};

const BATTERY_STATUS_PATH: &str =
    "/sys/devices/platform/soc/soc:oplus,mms_gauge/oplus_mms/gauge/battery/status";
const UFCS_FORCE_VAL_PATH: &str = "/proc/oplus-votable/UFCS_CURR/force_val";
const UFCS_FORCE_ACTIVE_PATH: &str = "/proc/oplus-votable/UFCS_CURR/force_active";

pub struct Looper;

impl Looper {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    pub fn enter_loop(&mut self, config_manager: &Arc<AtomicConfig>) -> Result<()> {
        let mut was_charging = None;
        let mut cut_off = false;
        let mut bcc_params_reader = BccParamsReader::new()?;

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
                        cut_off = false;
                        Self::apply_ufcs_vote(config_manager);
                    }

                    if is_charging {
                        match bcc_params_reader.read() {
                            Ok(params) => {
                                info!(
                                    cell_voltage_1_mv = params.cell_voltage_1_mv,
                                    cell_voltage_2_mv = params.cell_voltage_2_mv,
                                    current_ma = params.current_ma,
                                    "充电数据"
                                );
                                let charge_cutoff_mv =
                                    f64::from(config_manager.get().charge_cutoff_mv);
                                if !cut_off && params.cell_voltage_1_mv >= charge_cutoff_mv {
                                    Self::cut_off_ufcs();
                                    cut_off = true;
                                }
                            }
                            Err(error) => error!("读取充电数据失败: {error}"),
                        }
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

        if let Err(error) = mask_val(&ufcs_max_vote, Path::new(UFCS_FORCE_VAL_PATH)) {
            error!("设置 UFCS 最大电流失败: {error}");
        }

        if let Err(error) = mask_val("1", Path::new(UFCS_FORCE_ACTIVE_PATH)) {
            error!("启用 UFCS 强制投票失败: {error}");
        }
    }

    fn cut_off_ufcs() {
        if let Err(error) = mask_val("0", Path::new(UFCS_FORCE_VAL_PATH)) {
            error!("截止 UFCS 充电失败: {error}");
        } else {
            warn!("电芯电压达到截止阈值，UFCS 电流已置 0");
        }

        if let Err(error) = mask_val("1", Path::new(UFCS_FORCE_ACTIVE_PATH)) {
            error!("启用 UFCS 强制投票失败: {error}");
        }
    }
}

impl Default for Looper {
    fn default() -> Self {
        Self::new()
    }
}
