/// GRBL machine states from a `?` status report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineState {
    Idle,
    Run,
    Hold,
    Jog,
    Alarm,
    Door,
    Check,
    Home,
    Sleep,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub state: MachineState,
    /// Machine position in mm, when reported (`MPos`, or `WPos` as a fallback).
    pub position: Option<(f64, f64, f64)>,
    /// The position is in work coordinates (`WPos`), not machine coordinates: it shifts with the
    /// work offset, so it cannot be mapped onto the bed.
    pub position_is_work: bool,
}

/// Parses `<Idle|MPos:1.000,2.000,0.000|FS:0,0>`. Returns `None` for anything else.
pub fn parse_status(line: &str) -> Option<Status> {
    let inner = line.trim().strip_prefix('<')?.strip_suffix('>')?;
    let mut fields = inner.split('|');
    let state = match fields.next()?.split(':').next()? {
        "Idle" => MachineState::Idle,
        "Run" => MachineState::Run,
        "Hold" => MachineState::Hold,
        "Jog" => MachineState::Jog,
        "Alarm" => MachineState::Alarm,
        "Door" => MachineState::Door,
        "Check" => MachineState::Check,
        "Home" => MachineState::Home,
        "Sleep" => MachineState::Sleep,
        _ => MachineState::Unknown,
    };
    let mut position_is_work = false;
    let position = fields
        .find_map(|f| {
            f.strip_prefix("MPos:").or_else(|| {
                let w = f.strip_prefix("WPos:");
                position_is_work = w.is_some();
                w
            })
        })
        .and_then(|v| {
            let mut n = v.split(',').map(|s| s.trim().parse::<f64>().ok().filter(|x| x.is_finite()));
            Some((n.next()??, n.next()??, n.next().flatten().unwrap_or(0.0)))
        });
    Some(Status { state, position, position_is_work })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_positions_are_flagged_because_they_cannot_be_placed_on_the_bed() {
        let w = parse_status("<Idle|WPos:1.000,2.000,0.000|FS:0,0>").unwrap();
        assert!(w.position_is_work && w.position == Some((1.0, 2.0, 0.0)));
        let m = parse_status("<Idle|MPos:1.000,2.000,0.000|WPos:9,9,9>").unwrap();
        assert!(!m.position_is_work && m.position == Some((1.0, 2.0, 0.0)), "machine position wins when both are reported");
        assert!(!parse_status("<Idle>").unwrap().position_is_work);
    }

    #[test]
    fn parses_state_and_position() {
        let s = parse_status("<Run|MPos:10.500,-2.000,0.000|FS:1000,500>").unwrap();
        assert_eq!(s.state, MachineState::Run);
        assert_eq!(s.position, Some((10.5, -2.0, 0.0)));
        assert_eq!(parse_status("<Hold:0|WPos:1,2,3>").unwrap().state, MachineState::Hold);
    }

    #[test]
    fn rejects_non_status_lines() {
        assert_eq!(parse_status("ok"), None);
        assert_eq!(parse_status("<Idle"), None);
        assert_eq!(parse_status(""), None);
    }

    #[test]
    fn bad_numbers_leave_position_empty() {
        assert_eq!(parse_status("<Idle|MPos:a,b,c>").unwrap().position, None);
        assert_eq!(parse_status("<Idle|MPos:NaN,1,2>").unwrap().position, None);
    }
}
