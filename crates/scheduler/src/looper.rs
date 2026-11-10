use std::{io, path::Path, sync::Arc, thread, time::Duration};

use anyhow::{Context, Result};
use config::{AtomicConfig, Config};
use dumpsys_rs::BoundDumpsys;
use tracing::{error, info, warn};
use utils::{
    BatteryCapacityReader, BatteryTempReader, BccParams, BccParamsReader, ChargeTypeReader,
    SysfsReader, lock_val, mask_val, unmask_val,
};

use crate::{ramp_up, taper, temp_taper};

#[path = "battery_display.rs"]
mod battery_display;

use battery_display::{BatteryDisplay, reset_battery_display};

const BATTERY_STATUS_PATH: &str =
    "/sys/devices/platform/soc/soc:oplus,mms_gauge/oplus_mms/gauge/battery/status";
const UFCS_FORCE_VAL_PATH: &str = "/proc/oplus-votable/UFCS_CURR/force_val";
const UFCS_FORCE_ACTIVE_PATH: &str = "/proc/oplus-votable/UFCS_CURR/force_active";
/// 充电类型映射（来源 `battery_log_content` 的 `charge_type` 字段）：
/// `1=SDP 2=DCP 3=CDP 4=ACA 5=C 6=PD 7=PD_DRP 8=PD_PPS 9=PD_SDP`
/// `10=APPLE_BKID 11=QC2 12=QC3 13=VOOC 14=SVOOC 15=UFCS`
const UFCS_CHARGE_TYPE: u32 = 15;
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
    /// 未做温度降额的基础投票值，用于升流与恒流状态推进。
    current_vote: i32,
    /// 上一轮实际写入节点的电流值，含温度降额。
    applied_vote: i32,
    ramp_step_ma: u32,
    phase: ChargePhase,
    cut_off: bool,
}

pub trait UfcsVoter {
    fn apply(&mut self, vote: i32) -> Result<()>;
    fn release(&mut self) -> Result<()>;
}

/// 一轮循环读到的电池数据：电芯参数、剩余电量与电池温度。
pub struct BatterySample {
    pub params: BccParams,
    pub capacity: u8,
    /// 温度节点的原始值（441 表示 44.1℃）。
    pub temp: i32,
}

struct SysfsVoter;

impl UfcsVoter for SysfsVoter {
    fn apply(&mut self, vote: i32) -> Result<()> {
        mask_val(&vote.to_string(), Path::new(UFCS_FORCE_VAL_PATH))
            .context("设置 UFCS 电流失败")?;
        mask_val("1", Path::new(UFCS_FORCE_ACTIVE_PATH)).context("启用 UFCS 强制投票失败")?;
        info!(current_vote_ma = vote, "UFCS 电流已锁定");
        Ok(())
    }

    fn release(&mut self) -> Result<()> {
        unmask_val(Path::new(UFCS_FORCE_VAL_PATH)).context("卸载 UFCS 电流遮罩失败")?;
        // 遮罩卸载后底层真实节点里仍是遮罩前直写的状态，需显式关闭强制投票。
        lock_val("0", Path::new(UFCS_FORCE_ACTIVE_PATH)).context("关闭 UFCS 强制投票失败")?;
        info!("已卸载 UFCS 遮罩并关闭强制投票，电流交还充电栈");
        Ok(())
    }
}

pub struct Looper {
    battery_display: BatteryDisplay,
    session: Option<Session>,
    released: bool,
}

impl Looper {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            battery_display: BatteryDisplay::new(),
            session: None,
            released: false,
        }
    }

    pub fn enter_loop(&mut self, config_manager: &Arc<AtomicConfig>) -> Result<()> {
        let battery_dumper = loop {
            if let Ok(battery_dumper) = BoundDumpsys::new("battery") {
                break battery_dumper;
            }
            thread::sleep(Duration::from_secs(1));
        };
        let mut reader = BccParamsReader::new()?;
        let mut charge_type_reader = ChargeTypeReader::new()?;
        let mut capacity_reader = BatteryCapacityReader::new()?;
        let mut temp_reader = BatteryTempReader::new()?;
        let mut status_reader = SysfsReader::new(BATTERY_STATUS_PATH, 16)?;
        let mut voter = SysfsVoter;
        loop {
            match status_reader.read() {
                Ok(content) => {
                    let charging = content.trim() == "Charging";
                    let previous_charging = self.handle_battery_display(charging, || {
                        reset_battery_display(&battery_dumper)
                    });
                    self.handle_battery_status(
                        &config_manager.get(),
                        || {
                            let params = reader.read()?;
                            let capacity = capacity_reader.read()?;
                            let temp = temp_reader.read()?;
                            Ok(BatterySample {
                                params,
                                capacity,
                                temp,
                            })
                        },
                        || charge_type_reader.read(),
                        &mut voter,
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
        apply_reset: impl FnMut() -> Result<()>,
    ) -> Option<bool> {
        self.battery_display.handle(charging, apply_reset)
    }

    pub fn handle_battery_status(
        &mut self,
        config: &Config,
        read_sample: impl FnOnce() -> io::Result<BatterySample>,
        read_charge_type: impl FnMut() -> io::Result<u32>,
        voter: &mut impl UfcsVoter,
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
        let charge_established = previous == Some(true);
        let ufcs = charging
            && match Self::read_charge_type(read_charge_type, !charge_established) {
                Ok(charge_type) if charge_type == UFCS_CHARGE_TYPE => true,
                Ok(charge_type) => {
                    if !charge_established {
                        info!(charge_type, "充电器类型非 UFCS，跳过充电控制");
                    }
                    false
                }
                Err(error) => {
                    error!("读取充电器类型失败: {error}");
                    false
                }
            };
        if ufcs && !charge_established {
            self.session = None;
            self.released = false;
        }
        if ufcs {
            let _ = self
                .handle_charge_data(config, read_sample, voter)
                .inspect_err(|error| error!("处理充电数据失败: {error:#}"));
        }
        info!("{}", if charging { "充电中" } else { "未充电" });
    }

    fn handle_charge_data(
        &mut self,
        config: &Config,
        read_sample: impl FnOnce() -> io::Result<BatterySample>,
        voter: &mut impl UfcsVoter,
    ) -> Result<()> {
        if self.released {
            return Ok(());
        }
        let BatterySample {
            params,
            capacity,
            temp,
        } = read_sample().context("读取电池数据失败")?;
        if capacity > config.ufcs_release_capacity {
            return self.release_control(config, capacity, voter);
        }

        let (mut session, first_sample) = self.start_session(config, temp, voter)?;
        info!(
            cell_voltage_1_mv = params.cell_voltage_1_mv,
            cell_voltage_2_mv = params.cell_voltage_2_mv,
            current_ma = params.current_ma,
            battery_temp = temp,
            "充电数据"
        );
        let over_voltage = Self::over_constant_voltage(config, &params);
        Self::update_voltage_state(&mut session, config, &params, over_voltage);
        let base = Self::next_vote(
            &mut session,
            config,
            first_sample,
            params.current_ma,
            over_voltage,
        );
        // 恒压阶段完全交给 taper 基于实测电流闭环降流，温度降额不介入：
        // 实测电流已包含上一轮写入的降额结果，若此处再减一次，降额量就会
        // 顺着实测电流重复进入下一轮 taper，形成正反馈直至把电流锁死在 0。
        let next = if session.phase == ChargePhase::ConstantVoltage {
            base
        } else {
            temp_taper::apply(base, temp, config)
        };
        if next != session.applied_vote {
            if next != base {
                info!(
                    battery_temp = temp,
                    temp_taper_start = config.temp_taper_start,
                    current_vote_ma = next,
                    "电池温度超过阈值，已按温度降低充电电流"
                );
            }
            voter.apply(next)?;
            session.applied_vote = next;
            if session.cut_off {
                warn!("电芯电压达到截止阈值，UFCS 电流已置 0");
            }
        }
        session.current_vote = base;
        self.session = Some(session);

        Ok(())
    }

    fn release_control(
        &mut self,
        config: &Config,
        capacity: u8,
        voter: &mut impl UfcsVoter,
    ) -> Result<()> {
        voter.release()?;
        self.session = None;
        self.released = true;
        info!(
            capacity,
            ufcs_release_capacity = config.ufcs_release_capacity,
            "电池电量已超过阈值，解除 UFCS 电流控制"
        );
        Ok(())
    }

    fn start_session(
        &self,
        config: &Config,
        temp: i32,
        voter: &mut impl UfcsVoter,
    ) -> Result<(Session, bool)> {
        if let Some(session) = self.session {
            return Ok((session, false));
        }
        let step = config.ufcs_ramp_step_ma;
        let vote = match i32::try_from(step) {
            Ok(step) => step.min(config.ufcs_max_vote),
            Err(_) => config.ufcs_max_vote,
        };
        let applied_vote = temp_taper::apply(vote, temp, config);
        voter.apply(applied_vote)?;
        let session = Session {
            current_vote: vote,
            applied_vote,
            ramp_step_ma: step,
            phase: ChargePhase::RampUp,
            cut_off: false,
        };
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

    fn read_charge_type(
        read_charge_type: impl FnMut() -> io::Result<u32>,
        retry: bool,
    ) -> io::Result<u32> {
        Self::read_charge_type_with_retry(read_charge_type, thread::sleep, retry)
    }

    fn read_charge_type_with_retry(
        mut read_charge_type: impl FnMut() -> io::Result<u32>,
        mut sleep: impl FnMut(Duration),
        retry: bool,
    ) -> io::Result<u32> {
        if !retry {
            return read_charge_type();
        }

        let mut charge_type = read_charge_type()?;
        for _ in 0..CHARGE_TYPE_RETRY_COUNT {
            if charge_type == UFCS_CHARGE_TYPE {
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
