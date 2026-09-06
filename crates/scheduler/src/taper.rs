use config::Config;

pub fn next(measured_current_ma: f64, config: &Config) -> i32 {
    (measured_current_ma.abs() as i32)
        .saturating_sub_unsigned(config.ufcs_taper_step_ma)
        .max(0)
}
