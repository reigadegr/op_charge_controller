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
        let mut bcc_params_reader = BccParamsReader::new()?;

        loop {
            match Self::get_battery_status() {
                Ok(is_charging) => self.handle_battery_status(
                    &config_manager.get(),
                    || bcc_params_reader.read(),
                    Self::apply_ufcs_vote,
                    is_charging,
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
        is_charging: bool,
    ) {
        let previously_charging = self.was_charging.replace(is_charging);
        if previously_charging.is_some_and(|was_charging| was_charging != is_charging) {
            info!(
                "{}",
                if is_charging {
                    "进入充电"
                } else {
                    "退出充电"
                }
            );
        }

        if is_charging && previously_charging != Some(true) {
            self.reset_session();
        }

        if is_charging {
            let _ = self
                .handle_charge_data(config, read_params, apply_vote)
                .inspect_err(|error| error!("处理充电数据失败: {error:#}"));
        }

        let message = if is_charging {
            "充电中"
        } else {
            "未充电"
        };
        info!("{message}");
    }

    fn handle_charge_data(
        &mut self,
        config: &Config,
        read_params: impl FnOnce() -> io::Result<BccParams>,
        mut apply_vote: impl FnMut(i32) -> Result<()>,
    ) -> Result<()> {
        let ramp = self.current_vote.zip(self.locked_step_ma);
        let just_started = ramp.is_none();
        let (current_vote, step_ma) = if let Some(ramp) = ramp {
            ramp
        } else {
            // The step is locked for the whole session, so reloading the profile
            // midway cannot change this session's ramp rate.
            let step_ma = config.ufcs_step_ma;
            let vote = match i32::try_from(step_ma) {
                Ok(step) => step.min(config.ufcs_max_vote),
                // A step wider than i32 always exceeds the cap.
                Err(_) => config.ufcs_max_vote,
            };
            apply_vote(vote)?;
            self.current_vote = Some(vote);
            self.locked_step_ma = Some(step_ma);
            (vote, step_ma)
        };

        // After startup, each vote settles during the loop's sleep before sampling.
        let params = read_params().context("读取充电数据失败")?;
        info!(
            cell_voltage_1_mv = params.cell_voltage_1_mv,
            cell_voltage_2_mv = params.cell_voltage_2_mv,
            current_ma = params.current_ma,
            "充电数据"
        );
        if params.cell_voltage_1_mv >= f64::from(config.charge_cutoff_mv) {
            self.cut_off = true;
        }

        let constant_voltage_mv = f64::from(config.constant_voltage_mv);
        let over_constant_voltage = params.cell_voltage_1_mv >= constant_voltage_mv
            || params.cell_voltage_2_mv >= constant_voltage_mv;
        if over_constant_voltage && !self.constant_voltage {
            self.constant_voltage = true;
            info!(
                constant_voltage_mv = config.constant_voltage_mv,
                "电芯电压达到恒压阈值，进入恒压降流阶段"
            );
        }

        let next_vote = self.next_vote(
            current_vote,
            step_ma,
            config.ufcs_max_vote,
            just_started,
            over_constant_voltage,
        );
        if next_vote != current_vote {
            apply_vote(next_vote)?;
            self.current_vote = Some(next_vote);
            if self.cut_off {
                warn!("电芯电压达到截止阈值，UFCS 电流已置 0");
            }
        }

        Ok(())
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
        current_vote: i32,
        step_ma: u32,
        max_vote: i32,
        just_started: bool,
        over_constant_voltage: bool,
    ) -> i32 {
        if self.cut_off {
            return 0;
        }
        if over_constant_voltage {
            return current_vote.saturating_sub_unsigned(step_ma).max(0);
        }
        if just_started || self.constant_current || self.constant_voltage {
            return current_vote;
        }

        let stepped = current_vote.saturating_add_unsigned(step_ma);
        if stepped > max_vote {
            self.constant_current = true;
            info!(
                current_vote_ma = current_vote,
                "升流已达上限，进入恒流充电阶段"
            );
            current_vote
        } else {
            stepped
        }
    }

    fn get_battery_status() -> Result<bool> {
        let status = fs::read_to_string(BATTERY_STATUS_PATH)?;
        Ok(status.trim() == "Charging")
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
mod tests {
    use std::cell::RefCell;

    use super::*;

    const fn config() -> Config {
        Config {
            ufcs_max_vote: 5000,
            ufcs_step_ma: 100,
            constant_voltage_mv: 4500,
            charge_cutoff_mv: 4570,
        }
    }

    const fn params(voltage: f64) -> BccParams {
        cell_params(voltage, 4390.0)
    }

    const fn cell_params(cell_voltage_1_mv: f64, cell_voltage_2_mv: f64) -> BccParams {
        BccParams {
            cell_voltage_1_mv,
            cell_voltage_2_mv,
            current_ma: -100.0,
        }
    }

    fn tick_params(
        looper: &mut Looper,
        config: &Config,
        charging: bool,
        params: BccParams,
    ) -> Vec<i32> {
        let mut votes = Vec::new();
        looper.handle_battery_status(
            config,
            || Ok(params),
            |vote| {
                votes.push(vote);
                Ok(())
            },
            charging,
        );
        votes
    }

    fn tick(looper: &mut Looper, config: &Config, charging: bool, voltage: f64) -> Vec<i32> {
        tick_params(looper, config, charging, params(voltage))
    }

    #[test]
    fn ramps_one_locked_step_per_sample_and_stops_below_the_cap() {
        #[derive(Debug, PartialEq)]
        enum Event {
            Vote(i32),
            Read,
        }

        let config = Config {
            ufcs_max_vote: 350,
            ..config()
        };
        let mut looper = Looper::new();
        let events = RefCell::new(Vec::new());
        for voltage in [4400.0, 4400.0, 4400.0, 4400.0, 4570.0, 4400.0] {
            looper.handle_battery_status(
                &config,
                || {
                    events.borrow_mut().push(Event::Read);
                    Ok(params(voltage))
                },
                |vote| {
                    events.borrow_mut().push(Event::Vote(vote));
                    Ok(())
                },
                true,
            );
        }

        assert_eq!(
            events.into_inner(),
            [
                Event::Vote(100),
                Event::Read,
                Event::Read,
                Event::Vote(200),
                Event::Read,
                Event::Vote(300),
                Event::Read,
                Event::Read,
                Event::Vote(0),
                Event::Read,
            ]
        );
    }

    #[test]
    fn restarts_from_the_locked_step_after_a_new_charging_session() {
        let config = config();
        let mut looper = Looper::new();
        looper.handle_battery_status(
            &config,
            || panic!("must not read charge data while idle"),
            |_| panic!("must not apply a vote while idle"),
            false,
        );

        assert_eq!(tick(&mut looper, &config, true, 4400.0), [100]);
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [200]);
        assert!(tick(&mut looper, &config, false, 4400.0).is_empty());
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [100]);
        assert_eq!(tick(&mut looper, &config, true, 4571.0), [0]);
        assert!(tick(&mut looper, &config, true, 4400.0).is_empty());
        assert!(tick(&mut looper, &config, false, 4400.0).is_empty());
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [100]);
    }

    #[test]
    fn initial_vote_respects_a_lower_cap_and_checks_voltage_immediately() {
        let config = Config {
            ufcs_max_vote: 1000,
            ufcs_step_ma: 5000,
            ..config()
        };
        let mut looper = Looper::new();

        assert_eq!(tick(&mut looper, &config, true, 4400.0), [1000]);
        assert!(tick(&mut looper, &config, true, 4400.0).is_empty());
        assert_eq!(tick(&mut Looper::new(), &config, true, 4570.0), [1000, 0]);
    }

    #[test]
    fn locks_the_step_for_the_session_but_uses_the_updated_cutoff() {
        let mut config = config();
        let mut looper = Looper::new();

        assert_eq!(tick(&mut looper, &config, true, 4400.0), [100]);
        config.ufcs_step_ma = 250;
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [200]);
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [300]);
        config.charge_cutoff_mv = 4400;
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [0]);
    }

    #[test]
    fn holds_the_constant_current_stage_until_the_session_ends() {
        let config = Config {
            ufcs_max_vote: 150,
            ..config()
        };
        let mut looper = Looper::new();

        assert_eq!(tick(&mut looper, &config, true, 4400.0), [100]);
        for _ in 0..3 {
            assert!(tick(&mut looper, &config, true, 4400.0).is_empty());
        }
        assert!(tick(&mut looper, &config, false, 4400.0).is_empty());
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [100]);
    }

    #[test]
    fn steps_down_at_the_constant_voltage_and_then_holds_the_current() {
        let config = Config {
            ufcs_max_vote: 350,
            ..config()
        };
        let mut looper = Looper::new();

        for expected_vote in [100, 200, 300] {
            assert_eq!(tick(&mut looper, &config, true, 4400.0), [expected_vote]);
        }
        assert!(tick(&mut looper, &config, true, 4400.0).is_empty());

        assert_eq!(
            tick_params(&mut looper, &config, true, cell_params(4400.0, 4500.0)),
            [200]
        );
        assert_eq!(
            tick_params(&mut looper, &config, true, cell_params(4501.0, 4390.0)),
            [100]
        );

        for _ in 0..3 {
            assert!(tick(&mut looper, &config, true, 4400.0).is_empty());
        }
    }

    #[test]
    fn clamps_the_stepped_down_vote_at_zero() {
        let config = Config {
            ufcs_max_vote: 1000,
            ufcs_step_ma: 5000,
            ..config()
        };
        let mut looper = Looper::new();

        assert_eq!(
            tick_params(&mut looper, &config, true, cell_params(4400.0, 4500.0)),
            [1000, 0]
        );
        assert!(tick_params(&mut looper, &config, true, cell_params(4400.0, 4500.0)).is_empty());
    }

    #[test]
    fn ramps_again_after_a_new_session_that_follows_constant_voltage() {
        let config = config();
        let mut looper = Looper::new();

        assert_eq!(tick(&mut looper, &config, true, 4400.0), [100]);
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [200]);
        assert_eq!(
            tick_params(&mut looper, &config, true, cell_params(4400.0, 4500.0)),
            [100]
        );
        assert!(tick(&mut looper, &config, true, 4400.0).is_empty());

        assert!(tick(&mut looper, &config, false, 4400.0).is_empty());
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [100]);
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [200]);
    }

    #[test]
    fn holds_the_current_vote_when_reading_voltage_fails() {
        let config = config();
        let mut looper = Looper::new();
        let mut votes = Vec::new();

        for _ in 0..2 {
            looper.handle_battery_status(
                &config,
                || Err(io::Error::other("read failed")),
                |vote| {
                    votes.push(vote);
                    Ok(())
                },
                true,
            );
        }

        assert_eq!(votes, [100]);
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [200]);
    }

    #[test]
    fn retries_failed_initial_and_incremental_votes_without_skipping_steps() {
        let config = config();
        let mut looper = Looper::new();
        for expected_vote in [100, 200, 300] {
            let previous_vote = looper.current_vote;
            looper.handle_battery_status(
                &config,
                || Ok(params(4400.0)),
                |vote| {
                    assert_eq!(vote, expected_vote);
                    anyhow::bail!("write failed")
                },
                true,
            );
            assert_eq!(looper.current_vote, previous_vote);
            assert_eq!(tick(&mut looper, &config, true, 4400.0), [expected_vote]);
        }
    }

    #[test]
    fn retries_a_failed_cutoff_even_after_voltage_drops() {
        let config = config();
        let mut looper = Looper::new();
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [100]);

        looper.handle_battery_status(
            &config,
            || Ok(params(4570.0)),
            |vote| {
                assert_eq!(vote, 0);
                anyhow::bail!("write failed")
            },
            true,
        );

        assert_eq!(looper.current_vote, Some(100));
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [0]);
        assert!(tick(&mut looper, &config, true, 4400.0).is_empty());
    }
}
