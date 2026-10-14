//! GRBL `$n=value` settings: parsing what the controller reports, building the lines that change
//! them, and a table of what each one means.
//!
//! Source for the table: the GRBL v1.1 configuration documentation,
//! <https://github.com/gnea/grbl/wiki/Grbl-v1.1-Configuration>.

use crate::LinkError;

/// Settings remembered from the controller; a hostile device cannot grow this without bound.
pub const MAX_SETTINGS: usize = 256;
/// Longest setting value accepted from the controller, in bytes.
const MAX_VALUE_BYTES: usize = 24;
/// Highest setting number we will read or write.
pub const MAX_SETTING_ID: u32 = 999;
/// Largest magnitude accepted when writing a setting.
const MAX_SETTING_VALUE: f64 = 1.0e7;

/// How a setting's value is meant to be edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Number,
    /// 0 or 1.
    Flag,
    /// A bit mask, one bit per axis.
    Mask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingInfo {
    pub id: u32,
    pub name: &'static str,
    pub unit: &'static str,
    pub kind: Kind,
}

const fn info(id: u32, name: &'static str, unit: &'static str, kind: Kind) -> SettingInfo {
    SettingInfo { id, name, unit, kind }
}

const TABLE: &[SettingInfo] = &[
    info(0, "Step pulse", "µs", Kind::Number),
    info(1, "Step idle delay", "ms", Kind::Number),
    info(2, "Step port invert", "mask", Kind::Mask),
    info(3, "Direction port invert", "mask", Kind::Mask),
    info(4, "Step enable invert", "", Kind::Flag),
    info(5, "Limit pins invert", "", Kind::Flag),
    info(6, "Probe pin invert", "", Kind::Flag),
    info(10, "Status report mask", "mask", Kind::Mask),
    info(11, "Junction deviation", "mm", Kind::Number),
    info(12, "Arc tolerance", "mm", Kind::Number),
    info(13, "Report in inches", "", Kind::Flag),
    info(20, "Soft limits", "", Kind::Flag),
    info(21, "Hard limits", "", Kind::Flag),
    info(22, "Homing cycle", "", Kind::Flag),
    info(23, "Homing direction invert", "mask", Kind::Mask),
    info(24, "Homing feed", "mm/min", Kind::Number),
    info(25, "Homing seek", "mm/min", Kind::Number),
    info(26, "Homing debounce", "ms", Kind::Number),
    info(27, "Homing pull-off", "mm", Kind::Number),
    info(30, "Max spindle speed (S max)", "", Kind::Number),
    info(31, "Min spindle speed", "", Kind::Number),
    info(32, "Laser mode", "", Kind::Flag),
    info(100, "X steps", "steps/mm", Kind::Number),
    info(101, "Y steps", "steps/mm", Kind::Number),
    info(102, "Z steps", "steps/mm", Kind::Number),
    info(110, "X max rate", "mm/min", Kind::Number),
    info(111, "Y max rate", "mm/min", Kind::Number),
    info(112, "Z max rate", "mm/min", Kind::Number),
    info(120, "X acceleration", "mm/s²", Kind::Number),
    info(121, "Y acceleration", "mm/s²", Kind::Number),
    info(122, "Z acceleration", "mm/s²", Kind::Number),
    info(130, "X max travel", "mm", Kind::Number),
    info(131, "Y max travel", "mm", Kind::Number),
    info(132, "Z max travel", "mm", Kind::Number),
];

/// What setting `id` means, when it is one GRBL 1.1 documents.
pub fn describe(id: u32) -> Option<&'static SettingInfo> {
    TABLE.iter().find(|s| s.id == id)
}

/// Reads a `$n=value` report line. Anything else (including jog lines and non-numeric values)
/// is not a setting.
pub fn parse_setting(line: &str) -> Option<(u32, String)> {
    let rest = line.trim().strip_prefix('$')?;
    let (id, value) = rest.split_once('=')?;
    if id.is_empty() || id.len() > 3 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let id: u32 = id.parse().ok()?;
    // Some firmwares append a description after the value ("$0=10 (step pulse, usec)").
    let value = value.split_whitespace().next()?;
    if value.len() > MAX_VALUE_BYTES || value.parse::<f64>().ok().filter(|v| v.is_finite()).is_none() {
        return None;
    }
    Some((id, value.to_string()))
}

/// The line that sets `$id` to `value`. Refuses unknown-looking numbers and non-finite values.
pub fn setting_line(id: u32, value: f64) -> Result<String, LinkError> {
    if id > MAX_SETTING_ID {
        return Err(LinkError::Rejected(format!("setting ${id} is out of range")));
    }
    if !value.is_finite() || value.abs() > MAX_SETTING_VALUE {
        return Err(LinkError::Rejected("a setting must be a finite number of reasonable size".into()));
    }
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    let text = if text.is_empty() || text == "-0" { "0" } else { text };
    Ok(format!("${id}={text}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_lines_are_parsed() {
        assert_eq!(parse_setting("$130=410.000"), Some((130, "410.000".into())));
        assert_eq!(parse_setting(" $0=10 "), Some((0, "10".into())));
        assert_eq!(parse_setting("$32=1 (laser mode)"), Some((32, "1".into())));
        assert_eq!(parse_setting("$-5=1"), None);
        assert_eq!(parse_setting("$J=G91 X1"), None, "a jog is not a setting");
        assert_eq!(parse_setting("$N0=G54"), None, "startup blocks are not numeric settings");
        assert_eq!(parse_setting("$1000000=1"), None);
        assert_eq!(parse_setting("$5=abc"), None);
        assert_eq!(parse_setting("$5=NaN"), None);
        assert_eq!(parse_setting("$5=inf"), None);
        assert_eq!(parse_setting("$5="), None);
        assert_eq!(parse_setting("$="), None);
        assert_eq!(parse_setting(&format!("$5={}", "9".repeat(40))), None);
        assert_eq!(parse_setting("ok"), None);
    }

    #[test]
    fn write_lines_are_validated_and_tidy() {
        assert_eq!(setting_line(32, 1.0).unwrap(), "$32=1");
        assert_eq!(setting_line(130, 410.5).unwrap(), "$130=410.5");
        assert_eq!(setting_line(11, 0.01).unwrap(), "$11=0.01");
        assert_eq!(setting_line(5, -0.00001).unwrap(), "$5=0");
        for bad in [f64::NAN, f64::INFINITY, -f64::INFINITY, 1e12] {
            assert!(setting_line(30, bad).is_err(), "{bad}");
        }
        assert!(setting_line(MAX_SETTING_ID + 1, 1.0).is_err());
    }

    #[test]
    fn the_table_has_unique_ids_and_the_settings_laser_work_needs() {
        for (i, a) in TABLE.iter().enumerate() {
            assert!(TABLE.iter().skip(i + 1).all(|b| b.id != a.id), "duplicate ${}", a.id);
        }
        assert_eq!(describe(32).map(|s| s.kind), Some(Kind::Flag));
        assert_eq!(describe(30).map(|s| s.name), Some("Max spindle speed (S max)"));
        assert!(describe(999).is_none());
    }
}
