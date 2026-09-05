use std::{fs, io, path::Path, sync::Arc, thread, time::Duration};

use anyhow::{Context, Result};
use config::{AtomicConfig, Config};
use tracing::{error, info, warn};
use utils::{BccParams, BccParamsReader, mask_val};

const BATTERY_STATUS_PATH: &str =
    "/sys/devices/platform/soc/soc:oplus,mms_gauge/oplus_mms/gauge/battery/status";
const UFCS_FORCE_VAL_PATH: &str = "/proc/oplus-votable/UFCS_CURR/force_val";
const UFCS_FORCE_ACTIVE_PATH: &str = "/proc/oplus-votable/UFCS_CURR/force_active";
const UFCS_INITIAL_VOTE_MA: i32 = 1500;

pub struct Looper {
    was_charging: Option<bool>,
    current_vote: Option<i32>,
    cut_off: bool,
}

impl Looper {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            was_charging: None,
            current_vote: None,
            cut_off: false,
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
            self.cut_off = false;
            self.current_vote = None;
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
        let just_started = self.current_vote.is_none();
        let current_vote = if let Some(vote) = self.current_vote {
            vote
        } else {
            let vote = UFCS_INITIAL_VOTE_MA.min(config.ufcs_max_vote);
            apply_vote(vote)?;
            self.current_vote = Some(vote);
            vote
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

        let next_vote = if self.cut_off {
            0
        } else if just_started {
            current_vote
        } else {
            current_vote
                .saturating_add_unsigned(config.ufcs_step_ma)
                .min(config.ufcs_max_vote)
        };
        if next_vote != current_vote {
            apply_vote(next_vote)?;
            self.current_vote = Some(next_vote);
            if self.cut_off {
                warn!("电芯电压达到截止阈值，UFCS 电流已置 0");
            }
        }

        Ok(())
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
            charge_cutoff_mv: 4570,
        }
    }

    const fn params(voltage: f64) -> BccParams {
        BccParams {
            cell_voltage_1_mv: voltage,
            cell_voltage_2_mv: 4390.0,
            current_ma: -1500.0,
        }
    }

    fn tick(looper: &mut Looper, config: &Config, charging: bool, voltage: f64) -> Vec<i32> {
        let mut votes = Vec::new();
        looper.handle_battery_status(
            config,
            || Ok(params(voltage)),
            |vote| {
                votes.push(vote);
                Ok(())
            },
            charging,
        );
        votes
    }

    #[test]
    fn ramps_one_step_per_sample_and_keeps_checking_at_the_cap() {
        #[derive(Debug, PartialEq)]
        enum Event {
            Vote(i32),
            Read,
        }

        let config = Config {
            ufcs_max_vote: 1750,
            ..config()
        };
        let mut looper = Looper::new();
        let events = RefCell::new(Vec::new());
        for voltage in [4400.0, 4400.0, 4400.0, 4400.0, 4400.0, 4570.0, 4400.0] {
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
                Event::Vote(1500),
                Event::Read,
                Event::Read,
                Event::Vote(1600),
                Event::Read,
                Event::Vote(1700),
                Event::Read,
                Event::Vote(1750),
                Event::Read,
                Event::Read,
                Event::Vote(0),
                Event::Read,
            ]
        );
    }

    #[test]
    fn restarts_from_1500_after_a_new_charging_session() {
        let config = config();
        let mut looper = Looper::new();
        looper.handle_battery_status(
            &config,
            || panic!("must not read charge data while idle"),
            |_| panic!("must not apply a vote while idle"),
            false,
        );

        assert_eq!(tick(&mut looper, &config, true, 4400.0), [1500]);
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [1600]);
        assert!(tick(&mut looper, &config, false, 4400.0).is_empty());
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [1500]);
        assert_eq!(tick(&mut looper, &config, true, 4571.0), [0]);
        assert!(tick(&mut looper, &config, true, 4400.0).is_empty());
        assert!(tick(&mut looper, &config, false, 4400.0).is_empty());
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [1500]);
    }

    #[test]
    fn initial_vote_respects_a_lower_cap_and_checks_voltage_immediately() {
        let config = Config {
            ufcs_max_vote: 1000,
            ..config()
        };
        let mut looper = Looper::new();

        assert_eq!(tick(&mut looper, &config, true, 4400.0), [1000]);
        assert!(tick(&mut looper, &config, true, 4400.0).is_empty());
        assert_eq!(tick(&mut Looper::new(), &config, true, 4570.0), [1000, 0]);
    }

    #[test]
    fn uses_updated_step_cap_and_cutoff_on_each_tick() {
        let mut config = config();
        let mut looper = Looper::new();

        assert_eq!(tick(&mut looper, &config, true, 4400.0), [1500]);
        config.ufcs_step_ma = 250;
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [1750]);
        config.ufcs_max_vote = 1600;
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [1600]);
        config.ufcs_step_ma = u32::MAX;
        config.ufcs_max_vote = 5000;
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [5000]);
        config.charge_cutoff_mv = 4400;
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [0]);
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

        assert_eq!(votes, [1500]);
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [1600]);
    }

    #[test]
    fn retries_failed_initial_and_incremental_votes_without_skipping_steps() {
        let config = config();
        let mut looper = Looper::new();
        for expected_vote in [1500, 1600, 1700] {
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
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [1500]);

        looper.handle_battery_status(
            &config,
            || Ok(params(4570.0)),
            |vote| {
                assert_eq!(vote, 0);
                anyhow::bail!("write failed")
            },
            true,
        );

        assert_eq!(looper.current_vote, Some(1500));
        assert_eq!(tick(&mut looper, &config, true, 4400.0), [0]);
        assert!(tick(&mut looper, &config, true, 4400.0).is_empty());
    }
}
