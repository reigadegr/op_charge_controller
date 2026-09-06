use config::Config;

pub fn next(current: i32, config: &Config) -> i32 {
    current
        .saturating_sub_unsigned(config.ufcs_taper_step_ma)
        .max(0)
}
