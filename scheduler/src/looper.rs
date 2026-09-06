use std::{fs, io, path::Path, sync::Arc, thread, time::Duration};

use anyhow::{Context, Result};
use config::{AtomicConfig, Config};
use tracing::{error, info, warn};
use utils::{BccParams, BccParamsReader, mask_val};

const BATTERY_STATUS_PATH: &str =
    "/sys/devices/platform/soc/soc:oplus,mms_gauge/oplus_mms/gauge/battery/status";
const UFCS_FORCE_VAL_PATH: &str = "/proc/oplus-votable/UFCS_CURR/force_val";
const UFCS_FORCE_ACTIVE_PATH: &str = "/proc/oplus-votable/UFCS_CURR/force_active";

pub struct Looper {
    was_charging: Option<bool>,
    current_vote: Option<i32>,
    locked_step_ma: Option<u32>,
    cut_off: bool,
    constant_current: bool,
    constant_voltage: bool,
}

impl Looper {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            was_charging: None,
            current_vote: None,
            locked_step_ma: None,
            cut_off: false,
            constant_current: false,
            constant_voltage: false,
        }
    }

    pub fn enter_loop(&mut self, config_manager: &Arc<AtomicConfig>) -> Result<()> {
        let mut reader = BccParamsReader::new()?;
        loop {
            match Self::get_battery_status() {
                Ok(charging) => self.handle_battery_status(
                    &config_manager.get(),
                    || reader.read(),
                    Self::apply_ufcs_vote,
                    charging,
                ),
                Err(error) => error!("读取电池状态失败: {error}"),
            }
            thread::sleep(Duration::from_secs(1));
        }
    }

    fn handle_battery_status(
        &mut self,
        config: &Config,
        read_params: impl FnOnce() -> io::Result<BccParams>,
        apply_vote: impl FnMut(i32) -> Result<()>,
        charging: bool,
    ) {
        let previous = self.was_charging.replace(charging);
        if previous.is_some_and(|was| was != charging) {
            info!(
                "{}",
                if charging {
                    "进入充电"
                } else {
                    "退出充电"
                }
            );
        }
        if charging && previous != Some(true) {
            self.reset_session();
        }
        if charging {
            let _ = self
                .handle_charge_data(config, read_params, apply_vote)
                .inspect_err(|error| error!("处理充电数据失败: {error:#}"));
        }
        info!("{}", if charging { "充电中" } else { "未充电" });
    }

    fn handle_charge_data(
        &mut self,
        config: &Config,
        read_params: impl FnOnce() -> io::Result<BccParams>,
        mut apply_vote: impl FnMut(i32) -> Result<()>,
    ) -> Result<()> {
        let (current, step, first_sample) = self.start_session(config, &mut apply_vote)?;
        let params = read_params().context("读取充电数据失败")?;
        info!(
            cell_voltage_1_mv = params.cell_voltage_1_mv,
            cell_voltage_2_mv = params.cell_voltage_2_mv,
            current_ma = params.current_ma,
            "充电数据"
        );
        self.update_voltage_state(config, &params);
        let next = self.next_vote(
            current,
            step,
            config.ufcs_max_vote,
            first_sample,
            Self::over_constant_voltage(config, &params),
        );
        if next != current {
            apply_vote(next)?;
            self.current_vote = Some(next);
            if self.cut_off {
                warn!("电芯电压达到截止阈值，UFCS 电流已置 0");
            }
        }
        Ok(())
    }

    fn start_session(
        &mut self,
        config: &Config,
        apply_vote: &mut impl FnMut(i32) -> Result<()>,
    ) -> Result<(i32, u32, bool)> {
        if let (Some(vote), Some(step)) = (self.current_vote, self.locked_step_ma) {
            return Ok((vote, step, false));
        }
        let step = config.ufcs_step_ma;
        let vote = match i32::try_from(step) {
            Ok(step) => step.min(config.ufcs_max_vote),
            Err(_) => config.ufcs_max_vote,
        };
        apply_vote(vote)?;
        self.current_vote = Some(vote);
        self.locked_step_ma = Some(step);
        Ok((vote, step, true))
    }

    fn update_voltage_state(&mut self, config: &Config, params: &BccParams) {
        self.cut_off |= params.cell_voltage_1_mv >= f64::from(config.charge_cutoff_mv);
        if Self::over_constant_voltage(config, params) && !self.constant_voltage {
            self.constant_voltage = true;
            info!(
                constant_voltage_mv = config.constant_voltage_mv,
                "电芯电压达到恒压阈值，进入恒压降流阶段"
            );
        }
    }

    fn over_constant_voltage(config: &Config, params: &BccParams) -> bool {
        let threshold = f64::from(config.constant_voltage_mv);
        params.cell_voltage_1_mv >= threshold || params.cell_voltage_2_mv >= threshold
    }

    const fn reset_session(&mut self) {
        self.cut_off = false;
        self.current_vote = None;
        self.locked_step_ma = None;
        self.constant_current = false;
        self.constant_voltage = false;
    }

    fn next_vote(
        &mut self,
        current: i32,
        step: u32,
        max: i32,
        first_sample: bool,
        over_voltage: bool,
    ) -> i32 {
        if self.cut_off {
            return 0;
        }
        if over_voltage {
            return current.saturating_sub_unsigned(step).max(0);
        }
        if first_sample || self.constant_current || self.constant_voltage {
            return current;
        }
        let next = current.saturating_add_unsigned(step);
        if next > max {
            self.constant_current = true;
            info!(current_vote_ma = current, "升流已达上限，进入恒流充电阶段");
            current
        } else {
            next
        }
    }

    fn get_battery_status() -> Result<bool> {
        Ok(fs::read_to_string(BATTERY_STATUS_PATH)?.trim() == "Charging")
    }

    fn apply_ufcs_vote(vote: i32) -> Result<()> {
        mask_val(&vote.to_string(), Path::new(UFCS_FORCE_VAL_PATH))
            .context("设置 UFCS 电流失败")?;
        mask_val("1", Path::new(UFCS_FORCE_ACTIVE_PATH)).context("启用 UFCS 强制投票失败")?;
        info!(current_vote_ma = vote, "UFCS 电流已锁定");
        Ok(())
    }
}

impl Default for Looper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
