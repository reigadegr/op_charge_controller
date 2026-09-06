use super::*;

use super::battery_display::BatteryDisplayAction;

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

mod battery_status_tests;
mod charge_control_tests;

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
