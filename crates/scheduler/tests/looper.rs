use std::io;

use config::Config;
use scheduler::looper::{BatterySample, Looper, UfcsVoter};
use utils::BccParams;

const UFCS_CHARGE_TYPE: u32 = 15;
/// 低于阈值 440 的常温，供不关心温度的既有用例使用。
const NORMAL_TEMP: i32 = 430;

const fn config() -> Config {
    Config {
        ufcs_max_vote: 5000,
        ufcs_ramp_step_ma: 100,
        ufcs_taper_step_ma: 100,
        constant_voltage_mv: 4500,
        charge_cutoff_mv: 4570,
        ufcs_release_capacity: 90,
        shell_back_emul_temp_enabled: true,
        temp_taper_start: 440,
    }
}

const fn params(v1: f64, v2: f64) -> BccParams {
    params_with_current(v1, v2, -100.0)
}

const fn params_with_current(v1: f64, v2: f64, current_ma: f64) -> BccParams {
    BccParams {
        cell_voltage_1_mv: v1,
        cell_voltage_2_mv: v2,
        current_ma,
    }
}

const fn normal_params() -> BccParams {
    params(4400.0, 4390.0)
}

#[derive(Default)]
struct Recorder {
    votes: Vec<i32>,
    releases: usize,
}

impl UfcsVoter for Recorder {
    fn apply(&mut self, vote: i32) -> anyhow::Result<()> {
        self.votes.push(vote);
        Ok(())
    }

    fn release(&mut self) -> anyhow::Result<()> {
        self.releases += 1;
        Ok(())
    }
}

struct FailingVoter {
    failed: bool,
}

impl UfcsVoter for FailingVoter {
    fn apply(&mut self, _vote: i32) -> anyhow::Result<()> {
        if self.failed {
            return Ok(());
        }
        self.failed = true;
        anyhow::bail!("failed");
    }

    fn release(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

struct TickResult {
    votes: Vec<i32>,
    releases: usize,
    resets: usize,
}

fn tick(
    looper: &mut Looper,
    config: &Config,
    charging: bool,
    charge_type: u32,
    p: BccParams,
) -> Vec<i32> {
    tick_with_capacity(looper, config, charging, charge_type, p, 65).votes
}

fn tick_charging(looper: &mut Looper, config: &Config, p: BccParams) -> Vec<i32> {
    tick(looper, config, true, UFCS_CHARGE_TYPE, p)
}

fn tick_not_charging(looper: &mut Looper, config: &Config, p: BccParams) -> Vec<i32> {
    tick(looper, config, false, UFCS_CHARGE_TYPE, p)
}

fn fail_next_vote(looper: &mut Looper, config: &Config) {
    let previous_charging = looper.handle_battery_display(true, || Ok(()));
    looper.handle_battery_status(
        config,
        || {
            Ok(BatterySample {
                params: normal_params(),
                capacity: 65,
                temp: NORMAL_TEMP,
            })
        },
        || Ok(UFCS_CHARGE_TYPE),
        &mut FailingVoter { failed: false },
        previous_charging,
        true,
    );
}

fn tick_with_capacity(
    looper: &mut Looper,
    config: &Config,
    charging: bool,
    charge_type: u32,
    p: BccParams,
    capacity: u8,
) -> TickResult {
    tick_with_sample(
        looper,
        config,
        charging,
        charge_type,
        p,
        capacity,
        NORMAL_TEMP,
    )
}

fn tick_charging_with_temp(
    looper: &mut Looper,
    config: &Config,
    p: BccParams,
    temp: i32,
) -> Vec<i32> {
    tick_with_sample(looper, config, true, UFCS_CHARGE_TYPE, p, 65, temp).votes
}

fn tick_with_sample(
    looper: &mut Looper,
    config: &Config,
    charging: bool,
    charge_type: u32,
    p: BccParams,
    capacity: u8,
    temp: i32,
) -> TickResult {
    let mut recorder = Recorder::default();
    let mut resets = 0;
    let previous_charging = looper.handle_battery_display(charging, || {
        resets += 1;
        Ok(())
    });
    looper.handle_battery_status(
        config,
        || {
            Ok(BatterySample {
                params: p,
                capacity,
                temp,
            })
        },
        || Ok(charge_type),
        &mut recorder,
        previous_charging,
        charging,
    );
    TickResult {
        votes: recorder.votes,
        releases: recorder.releases,
        resets,
    }
}

#[test]
fn ramps_by_one_locked_step_and_enters_constant_current_at_cap() {
    let config = Config {
        ufcs_max_vote: 250,
        ..config()
    };
    let mut looper = Looper::new();
    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [100]);
    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [200]);
    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [250]);
    assert_eq!(
        tick_charging(&mut looper, &config, normal_params()),
        [] as [i32; 0]
    );
}

#[test]
fn cutoff_and_constant_voltage_reduce_current() {
    let config = config();
    let mut looper = Looper::new();
    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [100]);
    assert_eq!(
        tick_charging(
            &mut looper,
            &config,
            params_with_current(4400.0, 4500.0, -180.0)
        ),
        [80]
    );
    assert_eq!(
        tick_charging(&mut looper, &config, params(4570.0, 4390.0)),
        [0]
    );
    assert_eq!(
        tick_charging(&mut looper, &config, params(4570.0, 4390.0)),
        [] as [i32; 0]
    );
}

#[test]
fn cutoff_when_either_cell_reaches_threshold() {
    let config = config();
    let mut looper = Looper::new();
    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [100]);
    assert_eq!(
        tick_charging(
            &mut looper,
            &config,
            params_with_current(4400.0, 4570.0, -180.0)
        ),
        [0]
    );
    assert_eq!(
        tick_charging(
            &mut looper,
            &config,
            params_with_current(4400.0, 4570.0, -180.0)
        ),
        [] as [i32; 0]
    );
}

#[test]
fn failed_vote_is_retried_without_advancing_state() {
    let config = config();
    let mut looper = Looper::new();
    fail_next_vote(&mut looper, &config);

    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [100]);
}

#[test]
fn failed_cap_vote_does_not_advance_to_constant_current() {
    let config = Config {
        ufcs_max_vote: 250,
        ..config()
    };
    let mut looper = Looper::new();
    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [100]);
    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [200]);

    fail_next_vote(&mut looper, &config);

    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [250]);
}

#[test]
fn ramp_and_taper_use_their_own_steps() {
    let config = Config {
        ufcs_ramp_step_ma: 200,
        ufcs_taper_step_ma: 50,
        ..config()
    };
    let mut looper = Looper::new();
    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [200]);
    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [400]);
    assert_eq!(
        tick_charging(
            &mut looper,
            &config,
            params_with_current(4500.0, 4390.0, -380.0)
        ),
        [330]
    );
    assert_eq!(
        tick_charging(
            &mut looper,
            &config,
            params_with_current(4500.0, 4390.0, -300.0)
        ),
        [250]
    );
    assert_eq!(
        tick_charging(&mut looper, &config, normal_params()),
        [] as [i32; 0]
    );
}

#[test]
fn taper_uses_last_recorded_current_and_floors_at_zero() {
    let config = config();
    let mut looper = Looper::new();
    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [100]);
    assert_eq!(
        tick_charging(
            &mut looper,
            &config,
            params_with_current(4500.0, 4390.0, 320.0)
        ),
        [220]
    );
    assert_eq!(
        tick_charging(
            &mut looper,
            &config,
            params_with_current(4500.0, 4390.0, -180.0)
        ),
        [80]
    );
    assert_eq!(
        tick_charging(
            &mut looper,
            &config,
            params_with_current(4500.0, 4390.0, 30.0)
        ),
        [0]
    );
    assert_eq!(
        tick_charging(
            &mut looper,
            &config,
            params_with_current(4500.0, 4390.0, 30.0)
        ),
        [] as [i32; 0]
    );
}

#[test]
fn hot_reload_updates_taper_but_not_locked_ramp_step() {
    let initial_config = config();
    let reloaded_config = Config {
        ufcs_ramp_step_ma: 500,
        ufcs_taper_step_ma: 50,
        ..config()
    };
    let mut looper = Looper::new();
    assert_eq!(
        tick_charging(&mut looper, &initial_config, normal_params()),
        [100]
    );
    assert_eq!(
        tick_charging(&mut looper, &reloaded_config, normal_params()),
        [200]
    );
    assert_eq!(
        tick_charging(
            &mut looper,
            &reloaded_config,
            params_with_current(4500.0, 4390.0, -180.0)
        ),
        [130]
    );
}

#[test]
fn a_new_session_restarts_from_initial_step() {
    let config = config();
    let mut looper = Looper::new();
    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [100]);
    assert_eq!(
        tick_not_charging(&mut looper, &config, normal_params()),
        [] as [i32; 0]
    );
    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [100]);
}

#[test]
fn repeated_not_charging_status_is_skipped() {
    let config = config();
    let mut looper = Looper::new();
    let params = normal_params();

    assert_eq!(
        tick_not_charging(&mut looper, &config, params),
        [] as [i32; 0]
    );
    assert_eq!(
        tick_not_charging(&mut looper, &config, params),
        [] as [i32; 0]
    );
}

#[test]
fn non_ufcs_charger_skips_control() {
    let config = config();
    let mut looper = Looper::new();
    assert_eq!(
        tick(&mut looper, &config, true, 14, normal_params()),
        [] as [i32; 0]
    );
    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [100]);
    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [200]);
}

#[test]
fn charge_type_read_failure_skips_control() {
    let config = config();
    let mut looper = Looper::new();
    let mut recorder = Recorder::default();
    let previous_charging = looper.handle_battery_display(true, || Ok(()));
    looper.handle_battery_status(
        &config,
        || {
            Ok(BatterySample {
                params: normal_params(),
                capacity: 65,
                temp: NORMAL_TEMP,
            })
        },
        || Err(io::Error::new(io::ErrorKind::InvalidData, "读取失败")),
        &mut recorder,
        previous_charging,
        true,
    );
    assert_eq!(recorder.votes, [] as [i32; 0]);
}

#[test]
fn capacity_above_threshold_releases_control_and_stops_voting() {
    let config = config();
    let mut looper = Looper::new();

    let released = tick_with_capacity(
        &mut looper,
        &config,
        true,
        UFCS_CHARGE_TYPE,
        normal_params(),
        91,
    );
    assert_eq!(released.votes, [] as [i32; 0]);
    assert_eq!(released.releases, 1);

    let released_again = tick_with_capacity(
        &mut looper,
        &config,
        true,
        UFCS_CHARGE_TYPE,
        normal_params(),
        91,
    );
    assert_eq!(released_again.votes, [] as [i32; 0]);
    assert_eq!(released_again.releases, 0);
}

#[test]
fn capacity_at_threshold_keeps_control() {
    let config = config();
    let mut looper = Looper::new();

    let kept = tick_with_capacity(
        &mut looper,
        &config,
        true,
        UFCS_CHARGE_TYPE,
        normal_params(),
        90,
    );
    assert_eq!(kept.votes, [100]);
    assert_eq!(kept.releases, 0);
}

#[test]
fn released_control_is_not_resumed_within_the_same_session() {
    let config = config();
    let mut looper = Looper::new();

    assert_eq!(
        tick_with_capacity(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            normal_params(),
            91,
        )
        .releases,
        1
    );

    let dropped = tick_with_capacity(
        &mut looper,
        &config,
        true,
        UFCS_CHARGE_TYPE,
        normal_params(),
        80,
    );
    assert_eq!(dropped.votes, [] as [i32; 0]);
    assert_eq!(dropped.releases, 0);
}

#[test]
fn a_new_charging_session_takes_control_again_after_release() {
    let config = config();
    let mut looper = Looper::new();

    assert_eq!(
        tick_with_capacity(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            normal_params(),
            91,
        )
        .releases,
        1
    );
    assert_eq!(
        tick_with_capacity(
            &mut looper,
            &config,
            false,
            UFCS_CHARGE_TYPE,
            normal_params(),
            91,
        )
        .votes,
        [] as [i32; 0]
    );

    let resumed = tick_with_capacity(
        &mut looper,
        &config,
        true,
        UFCS_CHARGE_TYPE,
        normal_params(),
        60,
    );
    assert_eq!(resumed.votes, [100]);
    assert_eq!(resumed.releases, 0);
}

#[test]
fn battery_display_is_reset_when_charging_starts() {
    let config = config();
    let mut looper = Looper::new();
    let params = normal_params();

    assert_eq!(
        tick_with_capacity(&mut looper, &config, false, UFCS_CHARGE_TYPE, params, 2).resets,
        0
    );
    assert_eq!(
        tick_with_capacity(&mut looper, &config, true, UFCS_CHARGE_TYPE, params, 2).resets,
        1
    );
    assert_eq!(
        tick_with_capacity(&mut looper, &config, true, UFCS_CHARGE_TYPE, params, 2).resets,
        0
    );
}

#[test]
fn failed_display_reset_is_retried_while_charging() {
    let mut looper = Looper::new();
    let mut resets = 0;

    looper.handle_battery_display(false, || Ok(()));
    looper.handle_battery_display(true, || {
        resets += 1;
        anyhow::bail!("failed")
    });
    looper.handle_battery_display(true, || {
        resets += 1;
        Ok(())
    });
    looper.handle_battery_display(true, || {
        resets += 1;
        Ok(())
    });

    assert_eq!(resets, 2);
}

fn count_charge_type_reads(values: &[u32], previous_charging: Option<bool>) -> usize {
    let mut values = values.iter().copied();
    let mut reads = 0;
    let mut looper = Looper::new();

    looper.handle_battery_status(
        &config(),
        || {
            Ok(BatterySample {
                params: normal_params(),
                capacity: 65,
                temp: NORMAL_TEMP,
            })
        },
        || {
            reads += 1;
            values
                .next()
                .ok_or_else(|| io::Error::other("unexpected charge type read"))
        },
        &mut Recorder::default(),
        previous_charging,
        true,
    );

    reads
}

#[test]
fn non_ufcs_charge_type_is_reread_until_valid() {
    assert_eq!(count_charge_type_reads(&[0, 0, 15], None), 3);
    assert_eq!(count_charge_type_reads(&[14, 15], None), 2);
}

#[test]
fn non_ufcs_charge_type_is_reread_at_most_three_times() {
    assert_eq!(count_charge_type_reads(&[0, 0, 0, 0], None), 4);
    assert_eq!(count_charge_type_reads(&[14, 14, 14, 14], None), 4);
}

#[test]
fn non_ufcs_charge_type_is_not_retried_after_charging_is_established() {
    assert_eq!(count_charge_type_reads(&[0], Some(true)), 1);
    assert_eq!(count_charge_type_reads(&[14], Some(true)), 1);
}

#[test]
fn temperature_derate_subtracts_100ma_per_step_from_the_final_vote() {
    let config = Config {
        ufcs_max_vote: 6500,
        ufcs_ramp_step_ma: 6500,
        ..config()
    };
    let mut looper = Looper::new();
    assert_eq!(
        tick_charging_with_temp(&mut looper, &config, normal_params(), 430),
        [6500]
    );
    assert_eq!(
        tick_charging_with_temp(&mut looper, &config, normal_params(), 454),
        [5100]
    );
}

#[test]
fn temperature_derate_is_recomputed_every_round_and_restores_original_current() {
    let config = Config {
        ufcs_max_vote: 1000,
        ufcs_ramp_step_ma: 1000,
        ..config()
    };
    let mut looper = Looper::new();
    assert_eq!(
        tick_charging_with_temp(&mut looper, &config, normal_params(), 440),
        [1000]
    );
    assert_eq!(
        tick_charging_with_temp(&mut looper, &config, normal_params(), 441),
        [900]
    );
    assert_eq!(
        tick_charging_with_temp(&mut looper, &config, normal_params(), 442),
        [800]
    );
    assert_eq!(
        tick_charging_with_temp(&mut looper, &config, normal_params(), 440),
        [1000]
    );
    assert_eq!(
        tick_charging_with_temp(&mut looper, &config, normal_params(), 430),
        Vec::<i32>::new()
    );
}

#[test]
fn the_first_vote_of_a_session_is_also_derated() {
    let config = Config {
        ufcs_ramp_step_ma: 300,
        ..config()
    };
    let mut looper = Looper::new();
    assert_eq!(
        tick_charging_with_temp(&mut looper, &config, normal_params(), 441),
        [200]
    );
}

/// 模拟恒压闭环：首轮建立会话，之后每轮电压达到恒压阈值，
/// 且把上一轮实际写入的电流作为本轮实测电流喂回状态机。
fn constant_voltage_votes(config: &Config, temps: &[i32]) -> Vec<i32> {
    let mut looper = Looper::new();
    let mut measured_current = 0.0;
    temps
        .iter()
        .enumerate()
        .map(|(round, &temp)| {
            let p = if round == 0 {
                normal_params()
            } else {
                params_with_current(4500.0, 4390.0, measured_current)
            };
            let votes = tick_charging_with_temp(&mut looper, config, p, temp);
            assert_eq!(votes.len(), 1, "恒压阶段每轮都应重新投票");
            let vote = votes[0];
            measured_current = -f64::from(vote);
            vote
        })
        .collect()
}

#[test]
fn constant_voltage_phase_is_not_derated_by_temperature() {
    let config = Config {
        ufcs_max_vote: 6500,
        ufcs_ramp_step_ma: 6500,
        ..config()
    };
    let normal = constant_voltage_votes(&config, &[430; 6]);
    // 温度在阈值上下剧烈抖动，恒压阶段仍只按 ufcs_taper_step_ma 每轮降 100mA。
    let hot = constant_voltage_votes(&config, &[450, 450, 430, 460, 430, 450]);

    assert_eq!(normal, [6500, 6400, 6300, 6200, 6100, 6000]);
    assert_eq!(hot, [5500, 5400, 5300, 5200, 5100, 5000]);
    // 进入恒压前的一次降额由实测电流自然继承，恒压阶段不再重复扣除。
    assert_eq!(
        hot,
        normal.iter().map(|&vote| vote - 1000).collect::<Vec<i32>>()
    );
}

#[test]
fn high_temperature_does_not_run_away_or_latch_current_at_zero_in_constant_voltage() {
    let config = Config {
        ufcs_max_vote: 6500,
        ufcs_ramp_step_ma: 6500,
        ..config()
    };
    let votes = constant_voltage_votes(&config, &[460; 12]);

    assert_eq!(
        votes,
        (0..12)
            .map(|round| 4500 - 100 * round)
            .collect::<Vec<i32>>()
    );
}
