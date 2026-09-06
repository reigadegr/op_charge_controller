use std::{
    fs::File,
    io::{self, Read, Seek},
    path::PathBuf,
    str::FromStr,
};

const BCC_PARMS_PATH: &str = "/sys/class/oplus_chg/battery/bcc_parms";
const BATTERY_LOG_CONTENT_PATH: &str = "/sys/class/oplus_chg/battery/battery_log_content";
const BATTERY_CAPACITY_PATH: &str = "/sys/class/power_supply/battery/capacity";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BccParams {
    pub cell_voltage_1_mv: f64,
    pub cell_voltage_2_mv: f64,
    pub current_ma: f64,
}

#[derive(Debug)]
pub struct SysfsReader {
    path: PathBuf,
    fd: Option<File>,
    content: String,
}

impl SysfsReader {
    pub fn new(path: impl Into<PathBuf>, capacity: usize) -> io::Result<Self> {
        let path = path.into();
        let fd = File::open(&path)?;

        Ok(Self {
            path,
            fd: Some(fd),
            content: String::with_capacity(capacity),
        })
    }

    pub fn read(&mut self) -> io::Result<&str> {
        let result = if let Some(fd) = self.fd.as_mut() {
            Self::read_fd(fd, &mut self.content)
        } else {
            let mut fd = File::open(&self.path)?;
            let result = Self::read_fd(&mut fd, &mut self.content);
            if result.is_ok() {
                self.fd = Some(fd);
            }
            result
        };

        if result.is_err() {
            self.fd = None;
        }

        result.map(|()| self.content.as_str())
    }

    fn read_fd(fd: &mut File, content: &mut String) -> io::Result<()> {
        fd.rewind()?;
        content.clear();
        fd.read_to_string(content)?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct BccParamsReader {
    reader: SysfsReader,
}

impl BccParamsReader {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            reader: SysfsReader::new(BCC_PARMS_PATH, 128)?,
        })
    }

    pub fn read(&mut self) -> io::Result<BccParams> {
        parse_bcc_params(self.reader.read()?)
    }
}

fn parse_bcc_params(content: &str) -> io::Result<BccParams> {
    let content = content.trim();

    Ok(BccParams {
        cell_voltage_1_mv: parse_field(content, 6, "第一电芯电压")?,
        current_ma: parse_field(content, 8, "电流")?,
        cell_voltage_2_mv: parse_field(content, 11, "第二电芯电压")?,
    })
}

#[derive(Debug)]
pub struct ChargeTypeReader {
    reader: SysfsReader,
}

impl ChargeTypeReader {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            reader: SysfsReader::new(BATTERY_LOG_CONTENT_PATH, 256)?,
        })
    }

    pub fn read(&mut self) -> io::Result<u32> {
        parse_field(self.reader.read()?, 9, "充电器类型")
    }
}

#[derive(Debug)]
pub struct BatteryCapacityReader {
    reader: SysfsReader,
}

impl BatteryCapacityReader {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            reader: SysfsReader::new(BATTERY_CAPACITY_PATH, 4)?,
        })
    }

    pub fn read(&mut self) -> io::Result<u8> {
        parse_battery_capacity(self.reader.read()?)
    }
}

fn parse_battery_capacity(content: &str) -> io::Result<u8> {
    content.trim().parse().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("解析电池电量失败: {error}"),
        )
    })
}

fn parse_field<T>(content: &str, index: usize, name: &str) -> io::Result<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    content
        .split(',')
        .nth(index)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("缺少{name}字段")))?
        .trim()
        .parse()
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("解析{name}字段失败: {error}"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs, os::unix::fs::symlink, process};

    #[test]
    fn parses_bcc_params_battery_fields() -> io::Result<()> {
        let params = parse_bcc_params("0,1,2,3,4,5,4400,7,-5000,9,10,4390\n")?;

        assert_eq!(
            params,
            BccParams {
                cell_voltage_1_mv: 4400.0,
                cell_voltage_2_mv: 4390.0,
                current_ma: -5000.0,
            }
        );

        Ok(())
    }

    #[test]
    fn rejects_missing_bcc_params_fields() {
        assert!(matches!(
            parse_bcc_params("0,1,2"),
            Err(error) if error.kind() == io::ErrorKind::InvalidData
        ));
    }

    #[test]
    fn parses_charge_type_field() -> io::Result<()> {
        assert_eq!(
            parse_field::<u32>("0,1,2,3,4,5,6,7,8,15,10,11,5000\n", 9, "充电器类型")?,
            15
        );
        assert_eq!(
            parse_field::<u32>(" 0,1,2,3,4,5,6,7,8, 14 ,10,11\n", 9, "充电器类型")?,
            14
        );

        Ok(())
    }

    #[test]
    fn parses_battery_capacity() -> io::Result<()> {
        assert_eq!(parse_battery_capacity("65\n")?, 65);
        assert_eq!(parse_battery_capacity(" 2 ")?, 2);

        Ok(())
    }

    #[test]
    fn rejects_missing_charge_type_field() {
        assert!(matches!(
            parse_field::<u32>("0,1,2", 9, "充电器类型"),
            Err(error) if error.kind() == io::ErrorKind::InvalidData
        ));
    }

    #[test]
    fn rejects_invalid_bcc_params_fields() {
        assert!(matches!(
            parse_bcc_params("0,1,2,3,4,5,invalid,7,-5000,9,10,4390"),
            Err(error) if error.kind() == io::ErrorKind::InvalidData
        ));
    }

    #[test]
    fn sysfs_reader_reopens_after_read_failure() -> io::Result<()> {
        let path = env::temp_dir().join(format!(
            "op_charge_controller_{}_sysfs_reader",
            process::id()
        ));
        let _ = fs::remove_file(&path);
        symlink(env::temp_dir(), &path)?;
        let mut reader = SysfsReader::new(&path, 16)?;

        assert!(reader.read().is_err());
        fs::remove_file(&path)?;
        fs::write(&path, "first\n")?;
        assert_eq!(reader.read()?, "first\n");
        fs::write(&path, "second\n")?;
        assert_eq!(reader.read()?, "second\n");
        fs::remove_file(&path)
    }
}
