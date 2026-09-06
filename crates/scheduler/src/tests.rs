use super::*;

fn config() -> Config {
    Config {
        ufcs_max_vote: 5000,
        ufcs_ramp_step_ma: 100,
        ufcs_taper_step_ma: 100,
        constant_voltage_mv: 4500,
        charge_cutoff_mv: 4570,
    }
}

fn params(v1: f64, v2: f64) -> BccParams {
    params_with_current(v1, v2, -100.0)
}

fn params_with_current(v1: f64, v2: f64, current_ma: f64) -> BccParams {
    BccParams {
        cell_voltage_1_mv: v1,
        cell_voltage_2_mv: v2,
        current_ma,
    }
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
    let previous_charging = looper.battery_display.handle(
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
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params(4400.0, 4390.0)
        ),
        [100]
    );
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params(4400.0, 4390.0)
        ),
        [200]
    );
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params(4400.0, 4390.0)
        ),
        [250]
    );
    assert!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params(4400.0, 4390.0)
        )
        .is_empty()
    );
}

#[test]
fn cutoff_and_constant_voltage_reduce_current() {
    let config = config();
    let mut looper = Looper::new();
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params(4400.0, 4390.0)
        ),
        [100]
    );
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params_with_current(4400.0, 4500.0, -180.0)
        ),
        [80]
    );
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params(4570.0, 4390.0)
        ),
        [0]
    );
    assert!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params(4570.0, 4390.0)
        )
        .is_empty()
    );
}

#[test]
fn failed_vote_is_retried_without_advancing_state() {
    let config = config();
    let mut looper = Looper::new();
    let previous_charging = looper.battery_display.handle(true, || Ok(65), |_| Ok(()));
    looper.handle_battery_status(
        &config,
        || Ok(params(4400.0, 4390.0)),
        || Ok(UFCS_CHARGE_TYPE),
        |_| anyhow::bail!("failed"),
        previous_charging,
        true,
    );
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params(4400.0, 4390.0)
        ),
        [100]
    );
}

#[test]
fn ramp_and_taper_use_their_own_steps() {
    let config = Config {
        ufcs_ramp_step_ma: 200,
        ufcs_taper_step_ma: 50,
        ..config()
    };
    let mut looper = Looper::new();
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params(4400.0, 4390.0)
        ),
        [200]
    );
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params(4400.0, 4390.0)
        ),
        [400]
    );
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params_with_current(4500.0, 4390.0, -380.0)
        ),
        [330]
    );
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params_with_current(4500.0, 4390.0, -300.0)
        ),
        [250]
    );
    assert!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params(4400.0, 4390.0)
        )
        .is_empty()
    );
}

#[test]
fn taper_uses_last_recorded_current_and_floors_at_zero() {
    let config = config();
    let mut looper = Looper::new();
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params(4400.0, 4390.0)
        ),
        [100]
    );
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params_with_current(4500.0, 4390.0, 320.0)
        ),
        [220]
    );
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params_with_current(4500.0, 4390.0, -180.0)
        ),
        [80]
    );
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params_with_current(4500.0, 4390.0, 30.0)
        ),
        [0]
    );
    assert!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params_with_current(4500.0, 4390.0, 30.0)
        )
        .is_empty()
    );
}

#[test]
fn a_new_session_restarts_from_initial_step() {
    let config = config();
    let mut looper = Looper::new();
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params(4400.0, 4390.0)
        ),
        [100]
    );
    assert!(
        tick(
            &mut looper,
            &config,
            false,
            UFCS_CHARGE_TYPE,
            params(4400.0, 4390.0)
        )
        .is_empty()
    );
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params(4400.0, 4390.0)
        ),
        [100]
    );
}

#[test]
fn repeated_not_charging_status_is_skipped() {
    let config = config();
    let mut looper = Looper::new();
    let read_params = || Ok(params(4400.0, 4390.0));
    let apply_vote = |_| Ok(());
    let first_previous = looper.battery_display.handle(false, || Ok(65), |_| Ok(()));

    assert!(looper.handle_battery_status(
        &config,
        read_params,
        || Ok(UFCS_CHARGE_TYPE),
        apply_vote,
        first_previous,
        false,
    ));
    let second_previous = looper.battery_display.handle(false, || Ok(65), |_| Ok(()));
    assert!(!looper.handle_battery_status(
        &config,
        || Ok(params(4400.0, 4390.0)),
        || Ok(UFCS_CHARGE_TYPE),
        |_| Ok(()),
        second_previous,
        false,
    ));
}

#[test]
fn non_ufcs_charger_skips_control() {
    let config = config();
    let mut looper = Looper::new();
    assert!(tick(&mut looper, &config, true, 14, params(4400.0, 4390.0)).is_empty());
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params(4400.0, 4390.0)
        ),
        [100]
    );
    assert_eq!(
        tick(
            &mut looper,
            &config,
            true,
            UFCS_CHARGE_TYPE,
            params(4400.0, 4390.0)
        ),
        [200]
    );
}

#[test]
fn charge_type_read_failure_skips_control() {
    let config = config();
    let mut looper = Looper::new();
    let mut votes = Vec::new();
    let previous_charging = looper.battery_display.handle(true, || Ok(65), |_| Ok(()));
    looper.handle_battery_status(
        &config,
        || Ok(params(4400.0, 4390.0)),
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
    let params = params(4400.0, 4390.0);

    assert!(
        tick_with_capacity(&mut looper, &config, false, UFCS_CHARGE_TYPE, params, 65)
            .actions
            .is_empty()
    );
    assert_eq!(
        tick_with_capacity(&mut looper, &config, false, UFCS_CHARGE_TYPE, params, 2).actions,
        [BatteryDisplayAction::LockLowLevel]
    );
    assert!(
        tick_with_capacity(&mut looper, &config, false, UFCS_CHARGE_TYPE, params, 2)
            .actions
            .is_empty()
    );
    assert_eq!(
        tick_with_capacity(&mut looper, &config, true, UFCS_CHARGE_TYPE, params, 2).actions,
        [BatteryDisplayAction::Reset]
    );
    assert!(
        tick_with_capacity(&mut looper, &config, true, UFCS_CHARGE_TYPE, params, 2)
            .actions
            .is_empty()
    );
}

#[test]
fn charging_transition_resets_battery_display_without_low_level_lock() {
    let config = config();
    let mut looper = Looper::new();
    let params = params(4400.0, 4390.0);

    assert!(
        tick_with_capacity(&mut looper, &config, false, UFCS_CHARGE_TYPE, params, 65)
            .actions
            .is_empty()
    );
    assert_eq!(
        tick_with_capacity(&mut looper, &config, true, UFCS_CHARGE_TYPE, params, 65).actions,
        [BatteryDisplayAction::Reset]
    );
}

#[test]
fn battery_capacity_read_failure_does_not_lock_display() {
    let mut looper = Looper::new();
    let mut actions = Vec::new();

    looper.battery_display.handle(
        false,
        || Err(io::Error::new(io::ErrorKind::InvalidData, "读取失败")),
        |action| {
            actions.push(action);
            Ok(())
        },
    );

    assert!(actions.is_empty());
}
