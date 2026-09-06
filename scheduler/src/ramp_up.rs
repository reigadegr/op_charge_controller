use config::Config;
use tracing::info;

pub fn next(current: i32, step: u32, config: &Config) -> (i32, bool) {
    let next = current.saturating_add_unsigned(step);
    if next > config.ufcs_max_vote {
        info!(
            current_vote_ma = config.ufcs_max_vote,
            "升流已达上限，进入恒流充电阶段"
        );
        (config.ufcs_max_vote, true)
    } else {
        (next, false)
    }
}
