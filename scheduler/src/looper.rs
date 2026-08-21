use std::{fs, io, sync::Arc, thread, time::Duration};

use config::AtomicConfig;

const BATTERY_STATUS_PATH: &str =
    "/sys/devices/platform/soc/soc:oplus,mms_gauge/oplus_mms/gauge/battery/status";

pub struct Looper;

impl Looper {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    pub fn enter_loop(&mut self, _config_manager: &Arc<AtomicConfig>) {
        loop {
            match Self::read_battery_status() {
                Ok(status) if status == "Charging" => Self::enter_charging_loop(),
                Ok(_) => thread::sleep(Duration::from_secs(1)),
                Err(error) => {
                    eprintln!("读取电池状态失败: {error}");
                    thread::sleep(Duration::from_secs(1));
                }
            }
        }
    }

    fn read_battery_status() -> io::Result<String> {
        fs::read_to_string(BATTERY_STATUS_PATH).map(|status| status.trim().to_owned())
    }

    fn enter_charging_loop() {
        loop {
            println!("充电中");
            thread::sleep(Duration::from_secs(1));

            match Self::read_battery_status() {
                Ok(status) if status == "Discharging" => break,
                Ok(_) => {}
                Err(error) => eprintln!("读取电池状态失败: {error}"),
            }
        }
    }
}

impl Default for Looper {
    fn default() -> Self {
        Self::new()
    }
}
