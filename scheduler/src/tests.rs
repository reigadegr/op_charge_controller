use super::*;

fn config() -> Config {
    Config {
        ufcs_max_vote: 5000,
        ufcs_step_ma: 100,
        constant_voltage_mv: 4500,
        charge_cutoff_mv: 4570,
    }
}

fn params(v1: f64, v2: f64) -> BccParams {
    BccParams {
        cell_voltage_1_mv: v1,
        cell_voltage_2_mv: v2,
        current_ma: -100.0,
    }
}

fn tick(looper: &mut Looper, config: &Config, charging: bool, p: BccParams) -> Vec<i32> {
    let mut votes = Vec::new();
    looper.handle_battery_status(
        config,
        || Ok(p),
        |vote| {
            votes.push(vote);
            Ok(())
        },
        charging,
    );
    votes
}

#[test]
fn ramps_by_one_locked_step_and_holds_at_cap() {
    let config = Config {
        ufcs_max_vote: 250,
        ..config()
    };
    let mut looper = Looper::new();
    assert_eq!(
        tick(&mut looper, &config, true, params(4400.0, 4390.0)),
        [100]
    );
    assert_eq!(
        tick(&mut looper, &config, true, params(4400.0, 4390.0)),
        [200]
    );
    assert!(tick(&mut looper, &config, true, params(4400.0, 4390.0)).is_empty());
}

#[test]
fn cutoff_and_constant_voltage_reduce_current() {
    let config = config();
    let mut looper = Looper::new();
    assert_eq!(
        tick(&mut looper, &config, true, params(4400.0, 4390.0)),
        [100]
    );
    assert_eq!(
        tick(&mut looper, &config, true, params(4400.0, 4500.0)),
        [0]
    );
    assert!(tick(&mut looper, &config, true, params(4570.0, 4390.0)).is_empty());
}

#[test]
fn failed_vote_is_retried_without_advancing_state() {
    let config = config();
    let mut looper = Looper::new();
    looper.handle_battery_status(
        &config,
        || Ok(params(4400.0, 4390.0)),
        |_| anyhow::bail!("failed"),
        true,
    );
    assert_eq!(
        tick(&mut looper, &config, true, params(4400.0, 4390.0)),
        [100]
    );
}

#[test]
fn a_new_session_restarts_from_initial_step() {
    let config = config();
    let mut looper = Looper::new();
    assert_eq!(
        tick(&mut looper, &config, true, params(4400.0, 4390.0)),
        [100]
    );
    assert!(tick(&mut looper, &config, false, params(4400.0, 4390.0)).is_empty());
    assert_eq!(
        tick(&mut looper, &config, true, params(4400.0, 4390.0)),
        [100]
    );
}
