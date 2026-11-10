use config::Config;

/// 温度每超出阈值 1（0.1℃）就在最终电流上减去 100mA。
const TEMP_TAPER_STEP_MA: i32 = 100;

/// 在原本要输出的电流上按实时电池温度做减法，结果不低于 0。
pub fn apply(vote: i32, temp: i32, config: &Config) -> i32 {
    let over = temp.saturating_sub(config.temp_taper_start).max(0);
    vote.saturating_sub(over.saturating_mul(TEMP_TAPER_STEP_MA))
        .max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn config(temp_taper_start: i32) -> Config {
        Config {
            ufcs_max_vote: 6500,
            ufcs_ramp_step_ma: 300,
            ufcs_taper_step_ma: 100,
            constant_voltage_mv: 4500,
            charge_cutoff_mv: 4570,
            ufcs_release_capacity: 90,
            shell_back_emul_temp_enabled: true,
            temp_taper_start,
        }
    }

    #[test]
    fn no_reduction_at_or_below_threshold() {
        assert_eq!(apply(6500, 440, &config(440)), 6500);
        assert_eq!(apply(6500, 430, &config(440)), 6500);
    }

    #[test]
    fn reduces_100ma_per_tenth_of_a_degree_over_threshold() {
        assert_eq!(apply(6500, 441, &config(440)), 6400);
        assert_eq!(apply(6500, 454, &config(440)), 5100);
    }

    #[test]
    fn floors_at_zero() {
        assert_eq!(apply(300, 450, &config(440)), 0);
        assert_eq!(apply(0, 500, &config(440)), 0);
    }

    #[test]
    fn extreme_temperature_does_not_overflow() {
        assert_eq!(apply(100, i32::MIN, &config(440)), 100);
        assert_eq!(apply(100, i32::MAX, &config(440)), 0);
    }
}
