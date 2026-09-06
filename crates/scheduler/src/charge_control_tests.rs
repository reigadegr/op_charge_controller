use super::*;

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
fn failed_cap_vote_does_not_advance_to_constant_current() {
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
        [250]
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
fn hot_reload_updates_taper_but_not_locked_ramp_step() {
    let initial_config = config();
    let reloaded_config = Config {
        ufcs_ramp_step_ma: 500,
        ufcs_taper_step_ma: 50,
        ..config()
    };
    let mut looper = Looper::new();
    assert_eq!(
        tick(
            &mut looper,
            &initial_config,
            true,
            UFCS_CHARGE_TYPE,
            params(4400.0, 4390.0)
        ),
        [100]
    );
    assert_eq!(
        tick(
            &mut looper,
            &reloaded_config,
            true,
            UFCS_CHARGE_TYPE,
            params(4400.0, 4390.0)
        ),
        [200]
    );
    assert_eq!(
        tick(
            &mut looper,
            &reloaded_config,
            true,
            UFCS_CHARGE_TYPE,
            params_with_current(4500.0, 4390.0, -180.0)
        ),
        [130]
    );
}
