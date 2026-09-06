use std::{
    fs::File,
    io::{self, Read, Seek},
    path::Path,
    sync::Arc,
    thread,
    time::Duration,
};

use crate::{constant_current, ramp_up, taper};
use anyhow::{Context, Result};
use config::{AtomicConfig, Config};
use tracing::{error, info, warn};
use utils::{BccParams, BccParamsReader, ChargeTypeReader, mask_val};

const BATTERY_STATUS_PATH: &str =
    "/sys/devices/platform/soc/soc:oplus,mms_gauge/oplus_mms/gauge/battery/status";
const UFCS_FORCE_VAL_PATH: &str = "/proc/oplus-votable/UFCS_CURR/force_val";
const UFCS_FORCE_ACTIVE_PATH: &str = "/proc/oplus-votable/UFCS_CURR/force_active";
const UFCS_CHARGE_TYPE: u32 = 15;

pub struct Looper {
    was_charging: Option<bool>,
    current_vote: Option<i32>,
    locked_ramp_step_ma: Option<u32>,
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
            locked_ramp_step_ma: None,
            cut_off: false,
            constant_current: false,
            constant_voltage: false,
        }
    }

    pub fn enter_loop(&mut self, config_manager: &Arc<AtomicConfig>) -> Result<()> {
        let mut reader = BccParamsReader::new()?;
        let mut charge_type_reader = ChargeTypeReader::new()?;
        let mut status_file = None;
        let mut status_content = String::with_capacity(16);
        loop {
            match Self::get_battery_status(&mut status_file, &mut status_content) {
                Ok(charging) => {
                    self.handle_battery_status(
                        &config_manager.get(),
                        || reader.read(),
                        || charge_type_reader.read(),
                        Self::apply_ufcs_vote,
                        charging,
                    );
                }
                Err(error) => error!("读取电池状态失败: {error}"),
            }
            thread::sleep(Duration::from_secs(1));
        }
    }

    fn handle_battery_status(
        &mut self,
        config: &Config,
        read_params: impl FnOnce() -> io::Result<BccParams>,
        read_charge_type: impl FnOnce() -> io::Result<u32>,
        apply_vote: impl FnMut(i32) -> Result<()>,
        charging: bool,
    ) -> bool {
        let previous = self.was_charging.replace(charging);
        if !charging && previous == Some(false) {
            return false;
        }
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
        let ufcs = charging
            && match read_charge_type() {
                Ok(charge_type) if charge_type == UFCS_CHARGE_TYPE => true,
                Ok(charge_type) => {
                    if previous != Some(true) {
                        info!(charge_type, "充电器类型非 UFCS，跳过充电控制");
                    }
                    false
                }
                Err(error) => {
                    error!("读取充电器类型失败: {error}");
                    false
                }
            };
        if ufcs && previous != Some(true) {
            self.reset_session();
        }
        if ufcs {
            let _ = self
                .handle_charge_data(config, read_params, apply_vote)
                .inspect_err(|error| error!("处理充电数据失败: {error:#}"));
        }
        info!("{}", if charging { "充电中" } else { "未充电" });
        true
    }

    fn handle_charge_data(
        &mut self,
        config: &Config,
        read_params: impl FnOnce() -> io::Result<BccParams>,
        mut apply_vote: impl FnMut(i32) -> Result<()>,
    ) -> Result<()> {
        let (current, ramp_step, first_sample) = self.start_session(config, &mut apply_vote)?;
        let params = read_params().context("读取充电数据失败")?;
        info!(
            cell_voltage_1_mv = params.cell_voltage_1_mv,
            cell_voltage_2_mv = params.cell_voltage_2_mv,
            current_ma = params.current_ma,
            "充电数据"
        );
        self.update_voltage_state(config, &params);
        let next = self.next_vote(
            config,
            current,
            ramp_step,
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
        if let (Some(vote), Some(step)) = (self.current_vote, self.locked_ramp_step_ma) {
            return Ok((vote, step, false));
        }
        let step = config.ufcs_ramp_step_ma;
        let vote = match i32::try_from(step) {
            Ok(step) => step.min(config.ufcs_max_vote),
            Err(_) => config.ufcs_max_vote,
        };
        apply_vote(vote)?;
        self.current_vote = Some(vote);
        self.locked_ramp_step_ma = Some(step);
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
        self.locked_ramp_step_ma = None;
        self.constant_current = false;
        self.constant_voltage = false;
    }

    fn next_vote(
        &mut self,
        config: &Config,
        current: i32,
        ramp_step: u32,
        first_sample: bool,
        over_voltage: bool,
    ) -> i32 {
        if self.cut_off {
            return 0;
        }
        if over_voltage {
            return taper::next(current, config);
        }
        if first_sample || self.constant_voltage {
            return current;
        }
        if self.constant_current {
            return constant_current::next(current);
        }
        let (next, reached) = ramp_up::next(current, ramp_step, config);
        self.constant_current |= reached;
        next
    }

    fn get_battery_status(file: &mut Option<File>, content: &mut String) -> io::Result<bool> {
        if file.is_none() {
            *file = Some(File::open(BATTERY_STATUS_PATH)?);
        }

        let result = match file.as_mut() {
            Some(file) => file
                .rewind()
                .and_then(|()| {
                    content.clear();
                    file.read_to_string(content)
                })
                .map(|_| content.trim() == "Charging"),
            None => unreachable!("battery status file is initialized above"),
        };

        if result.is_err() {
            *file = None;
        }

        result
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
