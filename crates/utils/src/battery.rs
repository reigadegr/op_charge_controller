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
        Self::from_path(BCC_PARMS_PATH)
    }

    pub fn from_path(path: impl Into<PathBuf>) -> io::Result<Self> {
        Ok(Self {
            reader: SysfsReader::new(path, 128)?,
        })
    }

    pub fn read(&mut self) -> io::Result<BccParams> {
        parse_bcc_params(self.reader.read()?)
    }
}

fn parse_bcc_params(content: &str) -> io::Result<BccParams> {
    let content = content.trim();

    let params = BccParams {
        cell_voltage_1_mv: parse_field(content, 6, "第一电芯电压")?,
        current_ma: parse_field(content, 8, "电流")?,
        cell_voltage_2_mv: parse_field(content, 11, "第二电芯电压")?,
    };
    if !params.cell_voltage_1_mv.is_finite()
        || !params.cell_voltage_2_mv.is_finite()
        || !params.current_ma.is_finite()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "充电数据包含非有限数值",
        ));
    }

    Ok(params)
}

#[derive(Debug)]
pub struct ChargeTypeReader {
    reader: SysfsReader,
}

impl ChargeTypeReader {
    pub fn new() -> io::Result<Self> {
        Self::from_path(BATTERY_LOG_CONTENT_PATH)
    }

    pub fn from_path(path: impl Into<PathBuf>) -> io::Result<Self> {
        Ok(Self {
            reader: SysfsReader::new(path, 256)?,
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
        Self::from_path(BATTERY_CAPACITY_PATH)
    }

    pub fn from_path(path: impl Into<PathBuf>) -> io::Result<Self> {
        Ok(Self {
            reader: SysfsReader::new(path, 4)?,
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
