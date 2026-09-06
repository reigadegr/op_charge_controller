use std::{
    fs::File,
    io::{self, Read, Seek},
};

const BCC_PARMS_PATH: &str = "/sys/class/oplus_chg/battery/bcc_parms";
const BATTERY_LOG_CONTENT_PATH: &str = "/sys/class/oplus_chg/battery/battery_log_content";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BccParams {
    pub cell_voltage_1_mv: f64,
    pub cell_voltage_2_mv: f64,
    pub current_ma: f64,
}

#[derive(Debug)]
pub struct BccParamsReader {
    fd: File,
    content: String,
}

impl BccParamsReader {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            fd: File::open(BCC_PARMS_PATH)?,
            content: String::with_capacity(128),
        })
    }

    pub fn read(&mut self) -> io::Result<BccParams> {
        self.fd.rewind()?;

        self.content.clear();
        self.fd.read_to_string(&mut self.content)?;
        parse_bcc_params(&self.content)
    }
}

fn parse_bcc_params(content: &str) -> io::Result<BccParams> {
    let content = content.trim();

    Ok(BccParams {
        cell_voltage_1_mv: parse_bcc_field(content, 6, "第一电芯电压")?,
        current_ma: parse_bcc_field(content, 8, "电流")?,
        cell_voltage_2_mv: parse_bcc_field(content, 11, "第二电芯电压")?,
    })
}

fn parse_charge_type(content: &str) -> io::Result<u32> {
    content
        .trim()
        .split(',')
        .nth(9)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "缺少充电器类型字段"))?
        .trim()
        .parse()
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("解析充电器类型字段失败: {error}"),
            )
        })
}

#[derive(Debug)]
pub struct ChargeTypeReader {
    fd: File,
    content: String,
}

impl ChargeTypeReader {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            fd: File::open(BATTERY_LOG_CONTENT_PATH)?,
            content: String::with_capacity(256),
        })
    }

    pub fn read(&mut self) -> io::Result<u32> {
        self.fd.rewind()?;

        self.content.clear();
        self.fd.read_to_string(&mut self.content)?;
        parse_charge_type(&self.content)
    }
}

fn parse_bcc_field(content: &str, index: usize, name: &str) -> io::Result<f64> {
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
        assert_eq!(parse_charge_type("0,1,2,3,4,5,6,7,8,15,10,11,5000\n")?, 15);
        assert_eq!(parse_charge_type(" 0,1,2,3,4,5,6,7,8, 14 ,10,11\n")?, 14);

        Ok(())
    }

    #[test]
    fn rejects_missing_charge_type_field() {
        assert!(matches!(
            parse_charge_type("0,1,2"),
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
}
