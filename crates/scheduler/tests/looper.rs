use std::io;

use config::Config;
use scheduler::looper::{BatteryDisplayAction, Looper};
use utils::BccParams;

const UFCS_CHARGE_TYPE: u32 = 15;

const fn config() -> Config {
    Config {
        ufcs_max_vote: 5000,
        ufcs_ramp_step_ma: 100,
        ufcs_taper_step_ma: 100,
        constant_voltage_mv: 4500,
        charge_cutoff_mv: 4570,
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

struct TickResult {
    votes: Vec<i32>,
    actions: Vec<BatteryDisplayAction>,
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

fn tick_actions(
    looper: &mut Looper,
    config: &Config,
    charging: bool,
    p: BccParams,
    capacity: u8,
) -> Vec<BatteryDisplayAction> {
    tick_with_capacity(looper, config, charging, UFCS_CHARGE_TYPE, p, capacity).actions
}

fn fail_next_vote(looper: &mut Looper, config: &Config) {
    let mut vote_failed = true;
    let previous_charging = looper.handle_battery_display(true, || Ok(65), |_| Ok(()));
    looper.handle_battery_status(
        config,
        || Ok(normal_params()),
        || Ok(UFCS_CHARGE_TYPE),
        |_| {
            if vote_failed {
                vote_failed = false;
                anyhow::bail!("failed");
            }
            Ok(())
        },
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
    let mut votes = Vec::new();
    let mut actions = Vec::new();
    let previous_charging = looper.handle_battery_display(
        charging,
        || Ok(capacity),
        |action| {
            actions.push(action);
            Ok(())
        },
    );
    looper.handle_battery_status(
        config,
        || Ok(p),
        || Ok(charge_type),
        |vote| {
            votes.push(vote);
            Ok(())
        },
        previous_charging,
        charging,
    );
    TickResult { votes, actions }
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
    assert!(tick_charging(&mut looper, &config, normal_params()).is_empty());
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
    assert!(tick_charging(&mut looper, &config, params(4570.0, 4390.0)).is_empty());
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
    assert!(
        tick_charging(
            &mut looper,
            &config,
            params_with_current(4400.0, 4570.0, -180.0)
        )
        .is_empty()
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
    assert!(tick_charging(&mut looper, &config, normal_params()).is_empty());
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
    assert!(
        tick_charging(
            &mut looper,
            &config,
            params_with_current(4500.0, 4390.0, 30.0)
        )
        .is_empty()
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
    assert!(tick_not_charging(&mut looper, &config, normal_params()).is_empty());
    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [100]);
}

#[test]
fn repeated_not_charging_status_is_skipped() {
    let config = config();
    let mut looper = Looper::new();
    let params = normal_params();

    assert!(tick_not_charging(&mut looper, &config, params).is_empty());
    assert!(tick_not_charging(&mut looper, &config, params).is_empty());
}

#[test]
fn non_ufcs_charger_skips_control() {
    let config = config();
    let mut looper = Looper::new();
    assert!(tick(&mut looper, &config, true, 14, normal_params()).is_empty());
    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [100]);
    assert_eq!(tick_charging(&mut looper, &config, normal_params()), [200]);
}

#[test]
fn charge_type_read_failure_skips_control() {
    let config = config();
    let mut looper = Looper::new();
    let mut votes = Vec::new();
    let previous_charging = looper.handle_battery_display(true, || Ok(65), |_| Ok(()));
    looper.handle_battery_status(
        &config,
        || Ok(normal_params()),
        || Err(io::Error::new(io::ErrorKind::InvalidData, "读取失败")),
        |vote| {
            votes.push(vote);
            Ok(())
        },
        previous_charging,
        true,
    );
    assert!(votes.is_empty());
}

#[test]
fn low_battery_display_is_locked_once_and_reset_when_charging_starts() {
    let config = config();
    let mut looper = Looper::new();
    let params = normal_params();

    assert!(tick_actions(&mut looper, &config, false, params, 65).is_empty());
    assert_eq!(
        tick_actions(&mut looper, &config, false, params, 2),
        [BatteryDisplayAction::LockLowLevel]
    );
    assert!(tick_actions(&mut looper, &config, false, params, 2).is_empty());
    assert_eq!(
        tick_actions(&mut looper, &config, true, params, 2),
        [BatteryDisplayAction::Reset]
    );
    assert!(tick_actions(&mut looper, &config, true, params, 2).is_empty());
}

#[test]
fn charging_transition_resets_battery_display_without_low_level_lock() {
    let config = config();
    let mut looper = Looper::new();
    let params = normal_params();

    assert!(tick_actions(&mut looper, &config, false, params, 65).is_empty());
    assert_eq!(
        tick_actions(&mut looper, &config, true, params, 65),
        [BatteryDisplayAction::Reset]
    );
}

#[test]
fn battery_capacity_read_failure_does_not_lock_display() {
    let mut looper = Looper::new();
    let mut actions = Vec::new();

    looper.handle_battery_display(
        false,
        || Err(io::Error::new(io::ErrorKind::InvalidData, "读取失败")),
        |action| {
            actions.push(action);
            Ok(())
        },
    );

    assert!(actions.is_empty());
}

#[test]
fn failed_display_reset_is_retried_while_charging() {
    let mut looper = Looper::new();
    let mut actions = Vec::new();

    looper.handle_battery_display(
        false,
        || Ok(65),
        |action| {
            actions.push(action);
            Ok(())
        },
    );
    looper.handle_battery_display(
        true,
        || Ok(2),
        |action| {
            actions.push(action);
            anyhow::bail!("failed")
        },
    );
    looper.handle_battery_display(
        true,
        || Ok(2),
        |action| {
            actions.push(action);
            Ok(())
        },
    );
    looper.handle_battery_display(
        true,
        || Ok(2),
        |action| {
            actions.push(action);
            Ok(())
        },
    );

    assert_eq!(
        actions,
        [BatteryDisplayAction::Reset, BatteryDisplayAction::Reset]
    );
}

fn count_charge_type_reads(values: &[u32], previous_charging: Option<bool>) -> usize {
    let mut values = values.iter().copied();
    let mut reads = 0;
    let mut looper = Looper::new();

    looper.handle_battery_status(
        &config(),
        || Ok(normal_params()),
        || {
            reads += 1;
            values
                .next()
                .ok_or_else(|| io::Error::other("unexpected charge type read"))
        },
        |_| Ok(()),
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
