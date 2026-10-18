//! The machine a Kev flavour and device would run on (022, flavours and
//! devices).
//!
//! Total RAM comes from `sysctl hw.memsize` on macOS and `/proc/meminfo` on
//! Linux. The GPU with the largest VRAM comes from `nvidia-smi`; a missing or
//! failed probe means no GPU. `ai_permissions_hardware` replaces the whole
//! probe in tests, the way `ai_permissions_installer` replaces the install.

use std::ffi::OsStr;
use std::path::PathBuf;

use ariadne_core::probe;

/// One GPU, the largest VRAM one where a machine has several.
#[derive(Debug, Clone, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub vram_bytes: u64,
}

/// The machine a Kev flavour and device would run on.
#[derive(Debug, Clone, PartialEq)]
pub struct Hardware {
    pub os: String,
    pub arch: String,
    pub memory_bytes: u64,
    pub gpu: Option<Gpu>,
}

/// `ai_permissions_hardware`: a whole [`Hardware`] given in GB rather than
/// bytes, since it is written by hand in a test.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HardwareOverride {
    pub os: String,
    pub arch: String,
    pub memory_gb: u64,
    pub gpu_name: Option<String>,
    pub vram_gb: Option<u64>,
}

impl HardwareOverride {
    fn into_hardware(self) -> Hardware {
        Hardware {
            os: self.os,
            arch: self.arch,
            memory_bytes: self.memory_gb * GIB,
            gpu: self.gpu_name.map(|name| Gpu {
                name,
                vram_bytes: self.vram_gb.unwrap_or(0) * GIB,
            }),
        }
    }
}

/// 1 GB, read as GiB (022, the memory rule): `1024³` bytes.
pub const GIB: u64 = 1024 * 1024 * 1024;

/// The real machine, or `override_` where the suite replaces the probe whole.
pub async fn hardware(
    override_: Option<&HardwareOverride>,
    nvidia_smi_bin: Option<&str>,
    path: Option<&OsStr>,
) -> Hardware {
    match override_ {
        Some(over) => over.clone().into_hardware(),
        None => host(nvidia_smi_bin, path).await,
    }
}

/// The real host: its OS, architecture, total RAM and largest GPU.
async fn host(nvidia_smi_bin: Option<&str>, path: Option<&OsStr>) -> Hardware {
    Hardware {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        memory_bytes: total_memory().await.unwrap_or(0),
        gpu: gpu(nvidia_smi_bin, path).await,
    }
}

async fn total_memory() -> Option<u64> {
    if std::env::consts::OS == "macos" {
        let output = tokio::process::Command::new("sysctl")
            .arg("hw.memsize")
            .kill_on_drop(true)
            .output()
            .await
            .ok()?;
        parse_sysctl_memsize(&String::from_utf8_lossy(&output.stdout))
    } else {
        let content = tokio::fs::read_to_string("/proc/meminfo").await.ok()?;
        parse_meminfo(&content)
    }
}

/// `hw.memsize: 17179869184` -> bytes.
fn parse_sysctl_memsize(text: &str) -> Option<u64> {
    text.split(':').nth(1)?.trim().parse().ok()
}

/// `MemTotal:       16384000 kB` -> bytes.
fn parse_meminfo(text: &str) -> Option<u64> {
    let line = text.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

/// The GPU with the largest VRAM `nvidia_smi_bin` reports, or `None` where it
/// is not configured, not found on `path`, or fails.
async fn gpu(nvidia_smi_bin: Option<&str>, path: Option<&OsStr>) -> Option<Gpu> {
    let binary = match nvidia_smi_bin {
        Some(bin) if bin.contains('/') => {
            let candidate = PathBuf::from(bin);
            probe::is_executable(&candidate).then_some(candidate)
        }
        Some(bin) => path.and_then(|path| probe::which(path, bin)),
        None => path.and_then(|path| probe::which(path, "nvidia-smi")),
    }?;
    let output = tokio::process::Command::new(&binary)
        .args([
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .kill_on_drop(true)
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    largest_gpu(&String::from_utf8_lossy(&output.stdout))
}

/// `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits`:
/// one `<name>, <MiB>` line per GPU. The value is MiB whichever unit
/// `nounits` strips off the line.
fn largest_gpu(csv: &str) -> Option<Gpu> {
    csv.lines()
        .filter_map(|line| {
            let (name, mib) = line.rsplit_once(',')?;
            let mib: u64 = mib.trim().parse().ok()?;
            Some(Gpu {
                name: name.trim().to_string(),
                vram_bytes: mib * 1024 * 1024,
            })
        })
        .max_by_key(|gpu| gpu.vram_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn sysctl_and_meminfo_are_read_as_bytes() {
        assert_eq!(
            parse_sysctl_memsize("hw.memsize: 17179869184\n"),
            Some(17179869184)
        );
        assert_eq!(parse_sysctl_memsize("nonsense"), None);

        let meminfo = "MemTotal:       16384000 kB\nMemFree:         1234 kB\n";
        assert_eq!(parse_meminfo(meminfo), Some(16384000 * 1024));
        assert_eq!(parse_meminfo("nothing here"), None);
    }

    /// Two GPUs, the larger one taken; a name that itself holds a comma
    /// still splits on the trailing memory field.
    #[test]
    fn the_largest_of_several_gpus_is_taken() {
        let csv = "NVIDIA GeForce RTX 4090, 24564\nNVIDIA A100-SXM4-80GB, 81920\n";
        let gpu = largest_gpu(csv).unwrap();
        assert_eq!(gpu.name, "NVIDIA A100-SXM4-80GB");
        assert_eq!(gpu.vram_bytes, 81920 * 1024 * 1024);

        assert_eq!(largest_gpu(""), None);
        assert_eq!(largest_gpu("not a csv line"), None);
    }

    fn stub(dir: &std::path::Path, body: &str) -> PathBuf {
        let path = dir.join("nvidia-smi");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// A stub that prints two GPUs is parsed and the larger one taken; a
    /// stub that fails, and one that is not there at all, both report no GPU.
    #[tokio::test]
    async fn a_stub_probe_is_parsed_and_a_failing_one_reports_no_gpu() {
        let dir = tempfile::tempdir().unwrap();
        let ok = stub(
            dir.path(),
            "echo 'NVIDIA GeForce RTX 4090, 24564'\necho 'NVIDIA A100-SXM4-80GB, 81920'",
        );
        let found = gpu(Some(&ok.display().to_string()), None).await;
        assert_eq!(
            found,
            Some(Gpu {
                name: "NVIDIA A100-SXM4-80GB".into(),
                vram_bytes: 81920 * 1024 * 1024,
            })
        );

        let failing = stub(dir.path(), "echo 'no GPU' >&2\nexit 1");
        assert_eq!(gpu(Some(&failing.display().to_string()), None).await, None);

        assert_eq!(gpu(Some("/nonexistent/nvidia-smi"), None).await, None);
        assert_eq!(gpu(None, None).await, None);
    }

    /// The override stands in for the whole probe, converting GB to bytes.
    #[tokio::test]
    async fn the_override_replaces_the_probe_and_converts_gb_to_bytes() {
        let over = HardwareOverride {
            os: "linux".into(),
            arch: "x86_64".into(),
            memory_gb: 64,
            gpu_name: Some("NVIDIA A100".into()),
            vram_gb: Some(24),
        };
        let found = hardware(Some(&over), None, None).await;
        assert_eq!(found.os, "linux");
        assert_eq!(found.arch, "x86_64");
        assert_eq!(found.memory_bytes, 64 * GIB);
        assert_eq!(
            found.gpu,
            Some(Gpu {
                name: "NVIDIA A100".into(),
                vram_bytes: 24 * GIB,
            })
        );
    }
}
