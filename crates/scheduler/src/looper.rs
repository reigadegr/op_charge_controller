use std::{io, path::Path, sync::Arc, thread, time::Duration};

use anyhow::{Context, Result};
use config::{AtomicConfig, Config};
use dumpsys_rs::Dumpsys;
use tracing::{error, info, warn};
use utils::{
    BatteryCapacityReader, BccParams, BccParamsReader, ChargeTypeReader, SysfsReader, write_val,
};

use crate::{ramp_up, taper};

#[path = "battery_display.rs"]
mod battery_display;

pub use battery_display::BatteryDisplayAction;
use battery_display::{BatteryDisplay, apply_battery_display_action};

const BATTERY_STATUS_PATH: &str =
    "/sys/devices/platform/soc/soc:oplus,mms_gauge/oplus_mms/gauge/battery/status";
const UFCS_FORCE_VAL_PATH: &str = "/proc/oplus-votable/UFCS_CURR/force_val";
const UFCS_FORCE_ACTIVE_PATH: &str = "/proc/oplus-votable/UFCS_CURR/force_active";
const UFCS_CHARGE_TYPE: u32 = 15;
const UNSET_CHARGE_TYPE: u32 = 0;
const CHARGE_TYPE_RETRY_COUNT: usize = 3;
const CHARGE_TYPE_RETRY_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Eq, PartialEq)]
enum ChargePhase {
    RampUp,
    ConstantCurrent,
    ConstantVoltage,
}

#[derive(Clone, Copy)]
struct Session {
    current_vote: i32,
    ramp_step_ma: u32,
    phase: ChargePhase,
    cut_off: bool,
}

pub struct Looper {
    battery_display: BatteryDisplay,
    session: Option<Session>,
}

impl Looper {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            battery_display: BatteryDisplay::new(),
            session: None,
        }
    }

    pub fn enter_loop(&mut self, config_manager: &Arc<AtomicConfig>) -> Result<()> {
        let battery_dumper = loop {
            match Dumpsys::new("battery") {
                Some(battery_dumper) => break battery_dumper,
                None => thread::sleep(Duration::from_secs(1)),
            }
        };
        let mut reader = BccParamsReader::new()?;
        let mut charge_type_reader = ChargeTypeReader::new()?;
        let mut capacity_reader = BatteryCapacityReader::new()?;
        let mut status_reader = SysfsReader::new(BATTERY_STATUS_PATH, 16)?;
        loop {
            match status_reader.read() {
                Ok(content) => {
                    let charging = content.trim() == "Charging";
                    let previous_charging = self.handle_battery_display(
                        charging,
                        || capacity_reader.read(),
                        |action| apply_battery_display_action(&battery_dumper, action),
                    );
                    self.handle_battery_status(
                        &config_manager.get(),
                        || reader.read(),
                        || charge_type_reader.read(),
                        Self::apply_ufcs_vote,
                        previous_charging,
                        charging,
                    );
                }
                Err(error) => error!("读取电池状态失败: {error}"),
            }
            thread::sleep(Duration::from_secs(1));
        }
    }

    pub fn handle_battery_display(
        &mut self,
        charging: bool,
        read_capacity: impl FnOnce() -> io::Result<u8>,
        apply_action: impl FnMut(BatteryDisplayAction) -> Result<()>,
    ) -> Option<bool> {
        self.battery_display
            .handle(charging, read_capacity, apply_action)
    }

    pub fn handle_battery_status(
        &mut self,
        config: &Config,
        read_params: impl FnOnce() -> io::Result<BccParams>,
        read_charge_type: impl FnMut() -> io::Result<u32>,
        apply_vote: impl FnMut(i32) -> Result<()>,
        previous: Option<bool>,
        charging: bool,
    ) {
        if !charging && previous == Some(false) {
            return;
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
            && match Self::read_charge_type(read_charge_type, previous != Some(true)) {
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
            self.session = None;
        }
        if ufcs {
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
        let (mut session, first_sample) = self.start_session(config, &mut apply_vote)?;
        let current = session.current_vote;
        let params = read_params().context("读取充电数据失败")?;
        info!(
            cell_voltage_1_mv = params.cell_voltage_1_mv,
            cell_voltage_2_mv = params.cell_voltage_2_mv,
            current_ma = params.current_ma,
            "充电数据"
        );
        let over_voltage = Self::over_constant_voltage(config, &params);
        Self::update_voltage_state(&mut session, config, &params, over_voltage);
        let next = Self::next_vote(
            &mut session,
            config,
            first_sample,
            params.current_ma,
            over_voltage,
        );
        if next != current {
            apply_vote(next)?;
            session.current_vote = next;
            if session.cut_off {
                warn!("电芯电压达到截止阈值，UFCS 电流已置 0");
            }
        }
        self.session = Some(session);

        Ok(())
    }

    fn start_session(
        &mut self,
        config: &Config,
        apply_vote: &mut impl FnMut(i32) -> Result<()>,
    ) -> Result<(Session, bool)> {
        if let Some(session) = self.session {
            return Ok((session, false));
        }
        let step = config.ufcs_ramp_step_ma;
        let vote = match i32::try_from(step) {
            Ok(step) => step.min(config.ufcs_max_vote),
            Err(_) => config.ufcs_max_vote,
        };
        apply_vote(vote)?;
        let session = Session {
            current_vote: vote,
            ramp_step_ma: step,
            phase: ChargePhase::RampUp,
            cut_off: false,
        };
        self.session = Some(session);
        Ok((session, true))
    }

    fn update_voltage_state(
        session: &mut Session,
        config: &Config,
        params: &BccParams,
        over_voltage: bool,
    ) {
        session.cut_off |= params.cell_voltage_1_mv >= f64::from(config.charge_cutoff_mv)
            || params.cell_voltage_2_mv >= f64::from(config.charge_cutoff_mv);
        if over_voltage && session.phase != ChargePhase::ConstantVoltage {
            session.phase = ChargePhase::ConstantVoltage;
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

    fn next_vote(
        session: &mut Session,
        config: &Config,
        first_sample: bool,
        measured_current_ma: f64,
        over_voltage: bool,
    ) -> i32 {
        if session.cut_off {
            return 0;
        }
        match session.phase {
            ChargePhase::ConstantVoltage if over_voltage => {
                taper::next(measured_current_ma, config)
            }
            ChargePhase::ConstantVoltage | ChargePhase::ConstantCurrent => session.current_vote,
            ChargePhase::RampUp => {
                if first_sample {
                    return session.current_vote;
                }
                let (next, reached) =
                    ramp_up::next(session.current_vote, session.ramp_step_ma, config);
                if reached {
                    session.phase = ChargePhase::ConstantCurrent;
                }
                next
            }
        }
    }

    fn apply_ufcs_vote(vote: i32) -> Result<()> {
        write_val(&vote.to_string(), Path::new(UFCS_FORCE_VAL_PATH))
            .context("设置 UFCS 电流失败")?;
        write_val("1", Path::new(UFCS_FORCE_ACTIVE_PATH)).context("启用 UFCS 强制投票失败")?;
        info!(current_vote_ma = vote, "UFCS 电流已锁定");
        Ok(())
    }

    fn read_charge_type(
        read_charge_type: impl FnMut() -> io::Result<u32>,
        retry_on_unset_charge_type: bool,
    ) -> io::Result<u32> {
        Self::read_charge_type_with_retry(
            read_charge_type,
            thread::sleep,
            retry_on_unset_charge_type,
        )
    }

    fn read_charge_type_with_retry(
        mut read_charge_type: impl FnMut() -> io::Result<u32>,
        mut sleep: impl FnMut(Duration),
        retry_on_unset_charge_type: bool,
    ) -> io::Result<u32> {
        if !retry_on_unset_charge_type {
            return read_charge_type();
        }

        let mut charge_type = read_charge_type()?;
        for _ in 0..CHARGE_TYPE_RETRY_COUNT {
            if charge_type != UNSET_CHARGE_TYPE {
                return Ok(charge_type);
            }
            sleep(CHARGE_TYPE_RETRY_INTERVAL);
            charge_type = read_charge_type()?;
        }

        Ok(charge_type)
    }
}

impl Default for Looper {
    fn default() -> Self {
        Self::new()
    }
}
