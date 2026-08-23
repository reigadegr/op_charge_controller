use std::{
    fs::File,
    io::{self, Read, Seek},
};

const BCC_PARMS_PATH: &str = "/sys/class/oplus_chg/battery/bcc_parms";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BccParams {
    pub cell_voltage_1_mv: f64,
    pub cell_voltage_2_mv: f64,
    pub current_ma: f64,
}

#[derive(Debug)]
pub struct BccParamsReader {
    fd: File,
}

impl BccParamsReader {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            fd: File::open(BCC_PARMS_PATH)?,
        })
    }

    pub fn read(&mut self) -> io::Result<BccParams> {
        self.fd.rewind()?;

        let mut content = String::new();
        self.fd.read_to_string(&mut content)?;
        parse_bcc_params(&content)
    }
}

fn parse_bcc_params(content: &str) -> io::Result<BccParams> {
    let fields: Vec<_> = content.trim().split(',').collect();

    Ok(BccParams {
        cell_voltage_1_mv: parse_bcc_field(&fields, 6, "第一电芯电压")?,
        current_ma: parse_bcc_field(&fields, 8, "电流")?,
        cell_voltage_2_mv: parse_bcc_field(&fields, 11, "第二电芯电压")?,
    })
}

fn parse_bcc_field(fields: &[&str], index: usize, name: &str) -> io::Result<f64> {
    fields
        .get(index)
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
    fn rejects_invalid_bcc_params_fields() {
        assert!(matches!(
            parse_bcc_params("0,1,2,3,4,5,invalid,7,-5000,9,10,4390"),
            Err(error) if error.kind() == io::ErrorKind::InvalidData
        ));
    }
}
