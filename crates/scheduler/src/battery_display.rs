use anyhow::{Context, Result};
use dumpsys_rs::BoundDumpsys;
use tracing::{error, info};

#[derive(Clone, Copy)]
pub(super) struct BatteryDisplay {
    was_charging: Option<bool>,
}

impl BatteryDisplay {
    pub(super) const fn new() -> Self {
        Self { was_charging: None }
    }

    pub(super) fn handle(
        &mut self,
        charging: bool,
        mut apply_reset: impl FnMut() -> Result<()>,
    ) -> Option<bool> {
        let previous = self.was_charging.replace(charging);
        if previous == Some(false)
            && charging
            && let Err(error) = apply_reset()
        {
            error!("恢复电池显示失败: {error:#}");
            self.was_charging = Some(false);
        }

        previous
    }
}

pub(super) fn reset_battery_display(battery_dumper: &BoundDumpsys) -> Result<()> {
    battery_dumper
        .dump_only(&["reset"])
        .context("执行 dumpsys battery reset 失败")?;
    info!("进入充电，电池显示已恢复真实值");
    Ok(())
}
