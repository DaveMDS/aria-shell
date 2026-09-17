//! What `/sys` knows: cpu frequency and temperature (hwmon), GPUs
//! (amdgpu through sysfs, nvidia through `nvidia-smi`).

use std::fs;
use std::path::{Path, PathBuf};

use super::Gpu;

fn read_num(path: &Path) -> Option<f64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn read_str(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// Mean of the cores' current frequency, MHz (`scaling_cur_freq` is
/// in kHz), when the cpufreq driver exposes it.
pub fn cpu_freq_mhz() -> Option<f32> {
    let dir = fs::read_dir("/sys/devices/system/cpu").ok()?;
    let freqs: Vec<f64> = dir
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name();
            let name = name.to_str()?;
            if !name.starts_with("cpu") || !name[3..].chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            read_num(&e.path().join("cpufreq/scaling_cur_freq"))
        })
        .collect();
    if freqs.is_empty() {
        return None;
    }
    Some((freqs.iter().sum::<f64>() / freqs.len() as f64 / 1000.0) as f32)
}

/// The hwmon directories with their `name`.
fn hwmons() -> Vec<(String, PathBuf)> {
    let Ok(dir) = fs::read_dir("/sys/class/hwmon") else {
        return Vec::new();
    };
    let mut list: Vec<(String, PathBuf)> = dir
        .filter_map(|e| {
            let path = e.ok()?.path();
            Some((read_str(&path.join("name"))?, path))
        })
        .collect();
    list.sort();
    list
}

/// The cpu's temperature sensor: the hwmon named `preferred` if given,
/// else the first of the usual cpu drivers.
pub fn cpu_temp_sensor(preferred: &str) -> Option<PathBuf> {
    let all = hwmons();
    let by_name = |name: &str| all.iter().find(|(n, _)| n == name).map(|(_, p)| p.clone());
    if !preferred.is_empty() {
        return by_name(preferred);
    }
    ["k10temp", "zenpower", "coretemp", "cpu_thermal", "acpitz"]
        .iter()
        .find_map(|n| by_name(n))
}

/// The temperature of a sensor directory, °C: the input labelled
/// `Tctl`, `Tdie` or `Package id 0` when there is one, else `temp1`.
pub fn temp_c(sensor: &Path) -> Option<f32> {
    let Ok(dir) = fs::read_dir(sensor) else {
        return None;
    };
    let mut labelled = None;
    let mut first = None;
    let mut entries: Vec<PathBuf> = dir.filter_map(|e| Some(e.ok()?.path())).collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let Some(stem) = name
            .strip_suffix("_input")
            .filter(|s| s.starts_with("temp"))
        else {
            continue;
        };
        let label = read_str(&sensor.join(format!("{stem}_label"))).unwrap_or_default();
        if matches!(label.as_str(), "Tctl" | "Tdie" | "Package id 0") {
            labelled = Some(path);
            break;
        }
        if first.is_none() {
            first = Some(path);
        }
    }
    read_num(&labelled.or(first)?).map(|milli| (milli / 1000.0) as f32)
}

/// The amdgpu cards: busy percent, VRAM and temperature from sysfs.
pub fn amd_gpus() -> Vec<Gpu> {
    let Ok(dir) = fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut cards: Vec<PathBuf> = dir
        .filter_map(|e| {
            let path = e.ok()?.path();
            let name = path.file_name()?.to_str()?;
            // `card1`, not `card1-DP-1`.
            (name.starts_with("card") && !name.contains('-')).then_some(path)
        })
        .collect();
    cards.sort();
    cards
        .into_iter()
        .filter_map(|card| {
            let device = card.join("device");
            let busy = read_num(&device.join("gpu_busy_percent"))?;
            let temp = fs::read_dir(device.join("hwmon"))
                .ok()
                .and_then(|d| d.filter_map(|e| e.ok()).next())
                .and_then(|h| temp_c(&h.path()));
            Some(Gpu {
                name: read_str(&device.join("product_name"))
                    .unwrap_or_else(|| "AMD GPU".to_owned()),
                busy: Some(busy as f32),
                vram_used: read_num(&device.join("mem_info_vram_used")).map(|v| v as u64),
                vram_total: read_num(&device.join("mem_info_vram_total")).map(|v| v as u64),
                temp_c: temp,
            })
        })
        .collect()
}

/// The `nvidia-smi` query for [`nvidia_gpus`].
pub const NVIDIA_SMI_ARGS: &[&str] = &[
    "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu",
    "--format=csv,noheader,nounits",
];

/// One [`Gpu`] per line of `nvidia-smi`'s output: `name, busy, used
/// MiB, total MiB, temp`.
pub fn nvidia_gpus(output: &str) -> Vec<Gpu> {
    output
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').map(str::trim).collect();
            if f.len() < 5 {
                return None;
            }
            let mib = |s: &str| s.parse::<f64>().ok().map(|m| (m * 1024.0 * 1024.0) as u64);
            Some(Gpu {
                name: f[0].to_owned(),
                busy: f[1].parse().ok(),
                vram_used: mib(f[2]),
                vram_total: mib(f[3]),
                temp_c: f[4].parse().ok(),
            })
        })
        .collect()
}

/// Whether `nvidia-smi` is on the PATH.
pub fn has_nvidia_smi() -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| dir.join("nvidia-smi").is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_csv() {
        let g = nvidia_gpus("NVIDIA GeForce RTX 3060, 12, 1024, 12288, 45\n");
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].name, "NVIDIA GeForce RTX 3060");
        assert_eq!(g[0].busy, Some(12.0));
        assert_eq!(g[0].vram_used, Some(1024 * 1024 * 1024));
        assert_eq!(g[0].temp_c, Some(45.0));
        assert!(nvidia_gpus("garbage\n").is_empty());
    }

    #[test]
    fn temperature_from_a_fake_hwmon() {
        let dir = std::env::temp_dir().join(format!("aria-hwmon-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("temp1_input"), "41000\n").unwrap();
        fs::write(dir.join("temp1_label"), "Core 0\n").unwrap();
        fs::write(dir.join("temp2_input"), "55000\n").unwrap();
        fs::write(dir.join("temp2_label"), "Package id 0\n").unwrap();
        assert_eq!(temp_c(&dir), Some(55.0));
        fs::remove_file(dir.join("temp2_label")).unwrap();
        assert_eq!(temp_c(&dir), Some(41.0));
        let _ = fs::remove_dir_all(&dir);
    }
}
