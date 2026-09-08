use std::io;

use anyhow::{Context, Result};
use dumpsys_rs::BoundDumpsys;
use tracing::{error, info, warn};

const BATTERY_LEVEL_LOCK_THRESHOLD: u8 = 3;
const BATTERY_LOCKED_LEVEL: &str = "2";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BatteryDisplayAction {
    Reset,
    LockLowLevel,
}

#[derive(Clone, Copy)]
pub(super) struct BatteryDisplay {
    was_charging: Option<bool>,
    low_level_locked: bool,
}

impl BatteryDisplay {
    pub(super) const fn new() -> Self {
        Self {
            was_charging: None,
            low_level_locked: false,
        }
    }

    pub(super) fn handle(
        &mut self,
        charging: bool,
        read_capacity: impl FnOnce() -> io::Result<u8>,
        mut apply_action: impl FnMut(BatteryDisplayAction) -> Result<()>,
    ) -> Option<bool> {
        let previous = self.was_charging.replace(charging);
        let entered_charging = previous == Some(false) && charging;
        if entered_charging {
            self.low_level_locked = false;
            if let Err(error) = apply_action(BatteryDisplayAction::Reset) {
                error!("恢复电池显示失败: {error:#}");
                self.was_charging = Some(false);
            }
            return previous;
        }

        let action = if charging || self.low_level_locked {
            None
        } else {
            match read_capacity() {
                Ok(level) if level < BATTERY_LEVEL_LOCK_THRESHOLD => {
                    Some(BatteryDisplayAction::LockLowLevel)
                }
                Ok(_) => None,
                Err(error) => {
                    error!("读取电池电量失败: {error}");
                    None
                }
            }
        };
        if let Some(action) = action {
            match apply_action(action) {
                Ok(()) => self.low_level_locked = true,
                Err(error) => error!("锁定电池显示失败: {error:#}"),
            }
        }

        previous
    }
}

pub(super) fn apply_battery_display_action(
    battery_dumper: &BoundDumpsys,
    action: BatteryDisplayAction,
) -> Result<()> {
    let args: &[&str] = match action {
        BatteryDisplayAction::Reset => &["reset"],
        BatteryDisplayAction::LockLowLevel => &["set", "level", BATTERY_LOCKED_LEVEL],
    };
    battery_dumper
        .dump_only(args)
        .with_context(|| format!("执行 dumpsys battery {} 失败", args.join(" ")))?;

    match action {
        BatteryDisplayAction::Reset => info!("进入充电，电池显示已恢复真实值"),
        BatteryDisplayAction::LockLowLevel => warn!(
            locked_level = BATTERY_LOCKED_LEVEL,
            "电池电量低于3%，电池显示已锁定"
        ),
    }
    Ok(())
}
