use std::{fs, io, sync::Arc};

use anyhow::Result;
use config::AtomicConfig;
use netlink_sys::{Socket, SocketAddr, protocols::NETLINK_KOBJECT_UEVENT};

const BATTERY_STATUS_PATH: &str =
    "/sys/devices/platform/soc/soc:oplus,mms_gauge/oplus_mms/gauge/battery/status";
const BATTERY_DEVICE_PATH: &[u8] =
    b"/devices/platform/soc/soc:oplus,mms_gauge/oplus_mms/gauge/battery";

struct BatteryStatusWatcher {
    socket: Socket,
}

impl BatteryStatusWatcher {
    fn new() -> io::Result<Self> {
        let mut socket = Socket::new(NETLINK_KOBJECT_UEVENT)?;
        socket.bind(&SocketAddr::new(0, 1))?;

        Ok(Self { socket })
    }

    fn wait_for_change(&self) -> io::Result<()> {
        loop {
            let message = match self.socket.recv_from_full() {
                Ok((message, _)) => message,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            if is_battery_change_event(&message) {
                return Ok(());
            }
        }
    }
}

fn is_battery_change_event(message: &[u8]) -> bool {
    let mut is_change = false;
    let mut is_battery = false;

    for field in message.split(|byte| *byte == 0) {
        if field == b"ACTION=change" {
            is_change = true;
        } else if let Some(device_path) = field.strip_prefix(b"DEVPATH=") {
            is_battery = device_path == BATTERY_DEVICE_PATH;
        }
    }

    is_change && is_battery
}

pub struct Looper;

impl Looper {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    pub fn enter_loop(&mut self, _config_manager: &Arc<AtomicConfig>) -> Result<()> {
        let watcher = BatteryStatusWatcher::new()?;
        let mut was_charging = None;

        loop {
            match Self::read_battery_status() {
                Ok(status) => {
                    let is_charging = status == "Charging";

                    if let Some(previously_charging) = was_charging
                        && previously_charging != is_charging
                    {
                        if is_charging {
                            println!("进入充电");
                        } else {
                            println!("退出充电");
                        }
                    }

                    if is_charging {
                        println!("充电中");
                    } else {
                        println!("未充电");
                    }
                    was_charging = Some(is_charging);
                }
                Err(error) => eprintln!("读取电池状态失败: {error}"),
            }

            watcher.wait_for_change()?;
        }
    }

    fn read_battery_status() -> Result<String> {
        let status = fs::read_to_string(BATTERY_STATUS_PATH)?;
        Ok(status.trim().to_owned())
    }
}

impl Default for Looper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::is_battery_change_event;

    #[test]
    fn recognizes_battery_change_event() {
        let message = b"change@/devices/platform/soc/soc:oplus,mms_gauge/oplus_mms/gauge/battery\0ACTION=change\0DEVPATH=/devices/platform/soc/soc:oplus,mms_gauge/oplus_mms/gauge/battery\0SUBSYSTEM=power_supply\0";

        assert!(is_battery_change_event(message));
    }

    #[test]
    fn ignores_unrelated_and_non_change_events() {
        let unrelated = b"change@/devices/virtual/power_supply/usb\0ACTION=change\0DEVPATH=/devices/virtual/power_supply/usb\0";
        let battery_add = b"add@/devices/platform/soc/soc:oplus,mms_gauge/oplus_mms/gauge/battery\0ACTION=add\0DEVPATH=/devices/platform/soc/soc:oplus,mms_gauge/oplus_mms/gauge/battery\0";

        assert!(!is_battery_change_event(unrelated));
        assert!(!is_battery_change_event(battery_add));
    }
}
