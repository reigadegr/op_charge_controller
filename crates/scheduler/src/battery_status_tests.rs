use super::*;

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
    let mut votes = Vec::new();
    let first_previous = looper.battery_display.handle(false, || Ok(65), |_| Ok(()));

    looper.handle_battery_status(
        &config,
        || Ok(params(4400.0, 4390.0)),
        || Ok(UFCS_CHARGE_TYPE),
        |vote| {
            votes.push(vote);
            Ok(())
        },
        first_previous,
        false,
    );
    let second_previous = looper.battery_display.handle(false, || Ok(65), |_| Ok(()));
    looper.handle_battery_status(
        &config,
        || Ok(params(4400.0, 4390.0)),
        || Ok(UFCS_CHARGE_TYPE),
        |vote| {
            votes.push(vote);
            Ok(())
        },
        second_previous,
        false,
    );

    assert!(votes.is_empty());
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
