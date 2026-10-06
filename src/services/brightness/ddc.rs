//! External monitors over DDC/CI, through `ddcutil` (its terse output:
//! `detect --terse`, `getvcp 10 --terse`, `setvcp 10 <n> --noverify`).
//! VCP feature 0x10 is the brightness. The monitor's DRM connector
//! (`card1-HDMI-A-1`) is how it's tied to a Wayland output, whose name
//! is the connector's (`HDMI-A-1`). Without `ddcutil` on the PATH there
//! are no monitors, only the laptop's panel.

use std::process::Stdio;

use super::{Display, Kind, Level};

const PROGRAM: &str = "ddcutil";

/// Whether `ddcutil` is there to ask.
pub fn available() -> bool {
    crate::process::first_on_path(&[PROGRAM]).is_some()
}

/// `ddcutil detect --terse`: the monitors that answer.
pub async fn detect() -> Vec<Display> {
    match run(&["detect", "--terse"]).await {
        Ok(out) => parse_detect(&out),
        Err(e) => {
            log::warn!("brightness: {PROGRAM} detect: {e}");
            Vec::new()
        }
    }
}

/// The brightness of the monitor on i2c bus `bus`.
pub async fn get(bus: &str) -> Option<Level> {
    match run(&["getvcp", "10", "--bus", bus, "--terse"]).await {
        Ok(out) => {
            let level = parse_getvcp(&out);
            if level.is_none() {
                log::warn!("brightness: ddc bus {bus}: unexpected {:?}", out.trim());
            }
            level
        }
        Err(e) => {
            log::warn!("brightness: ddc bus {bus}: {e}");
            None
        }
    }
}

pub async fn set(bus: &str, value: u32) -> Result<(), String> {
    let value = value.to_string();
    run(&["setvcp", "10", &value, "--bus", bus, "--noverify"])
        .await
        .map(drop)
}

/// The i2c bus number of a display id (`ddc:3`).
pub fn bus(id: &str) -> Option<&str> {
    id.strip_prefix("ddc:")
}

async fn run(args: &[&str]) -> Result<String, String> {
    let output = tokio::process::Command::new(PROGRAM)
        .args(args)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("{PROGRAM}: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let said = stderr.trim();
        let said = if said.is_empty() { stdout.trim() } else { said };
        Err(format!(
            "{} ({})",
            said.lines().last().unwrap_or(""),
            output.status
        ))
    }
}

/// The `Display N` blocks (an `Invalid display` one is a monitor that
/// doesn't answer: a laptop's panel, DDC off in the monitor's menu).
fn parse_detect(text: &str) -> Vec<Display> {
    let mut displays = Vec::new();
    let mut current: Option<(Option<String>, Option<String>, String)> = None;
    let mut flush = |current: &mut Option<(Option<String>, Option<String>, String)>| {
        if let Some((Some(bus), output, model)) = current.take() {
            displays.push(Display {
                id: format!("ddc:{bus}"),
                kind: Kind::Ddc,
                output,
                model,
                level: None,
            });
        }
    };
    for line in text.lines() {
        if !line.starts_with(char::is_whitespace) {
            flush(&mut current);
            if line.starts_with("Display ") {
                current = Some((None, None, String::new()));
            }
            continue;
        }
        let Some((bus, output, model)) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "I2C bus" => *bus = value.strip_prefix("/dev/i2c-").map(str::to_owned),
            // card1-HDMI-A-1 -> HDMI-A-1
            "DRM connector" => *output = value.split_once('-').map(|(_, c)| c.to_owned()),
            // MFG:Model:Serial
            "Monitor" => *model = value.split(':').nth(1).unwrap_or("").trim().to_owned(),
            _ => {}
        }
    }
    flush(&mut current);
    displays
}

/// `VCP 10 C 75 100`: continuous, current, maximum.
fn parse_getvcp(text: &str) -> Option<Level> {
    let words: Vec<&str> = text.split_whitespace().collect();
    match words.as_slice() {
        ["VCP", "10", "C", value, max, ..] => {
            let level = Level {
                value: value.parse().ok()?,
                max: max.parse().ok()?,
            };
            (level.max > 0).then_some(level)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_output() {
        let text = "\
Display 1
   I2C bus:          /dev/i2c-0
   DRM connector:    card1-HDMI-A-1
   drm_connector_id: 508
   Monitor:          DEL:DELL P2314H:X87P04CP25NB

Invalid display
   I2C bus:          /dev/i2c-4
   DRM connector:    card0-eDP-1
   Monitor:          BOE::

Display 2
   I2C bus:          /dev/i2c-1
   DRM connector:    card1-HDMI-A-2
   Monitor:          DEL:DELL P2314H:X87P04CP25CB
";
        let displays = parse_detect(text);
        assert_eq!(displays.len(), 2);
        assert_eq!(displays[0].id, "ddc:0");
        assert_eq!(displays[0].output.as_deref(), Some("HDMI-A-1"));
        assert_eq!(displays[0].model, "DELL P2314H");
        assert_eq!(displays[1].id, "ddc:1");
        assert_eq!(displays[1].output.as_deref(), Some("HDMI-A-2"));
        assert!(parse_detect("No displays found.\n").is_empty());
    }

    #[test]
    fn getvcp_output() {
        assert_eq!(
            parse_getvcp("VCP 10 C 75 100\n"),
            Some(Level {
                value: 75,
                max: 100
            })
        );
        assert_eq!(parse_getvcp("VCP 10 ERR\n"), None);
        assert_eq!(parse_getvcp("VCP 10 C 0 0\n"), None);
        assert_eq!(bus("ddc:3"), Some("3"));
        assert_eq!(bus("backlight:x"), None);
    }
}
