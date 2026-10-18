//! Which Kev flavours and devices this machine could run (022, flavours and
//! devices).
//!
//! A flavour is a Kev size; a device is where it runs. Both are chosen
//! together, but only a combination the machine can actually run: the memory
//! rule below is what decides that. Installing and serving the chosen pair is
//! the next task's; this only says what fits.

use ariadne_api::permissions::{Device, Flavour};

use super::hardware::{GIB, Hardware};

/// The Hugging Face Hub adapter and base a flavour installs, at the Kev
/// commit `f1535963cea021439370c23127bc970b6788e730` every flavour shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pins {
    pub adapter: &'static str,
    pub adapter_revision: &'static str,
    pub base: &'static str,
    pub base_revision: &'static str,
}

/// The adapter and base a flavour installs.
pub fn pins(flavour: Flavour) -> Pins {
    match flavour {
        Flavour::Kev08B => Pins {
            adapter: "jaredpalmer/kev-0.8b",
            adapter_revision: "9a45d25eb2ab761841196625383fa1dff0e56c1e",
            base: "Qwen/Qwen3.5-0.8B-Base",
            base_revision: "dc7cdfe2ee4154fa7e30f5b51ca41bfa40174e68",
        },
        Flavour::Kev4B => Pins {
            adapter: "jaredpalmer/kev-4b",
            adapter_revision: "139fdd94f1b6a6ad80cc15e08fcb99cac885a101",
            base: "Qwen/Qwen3.5-4B-Base",
            base_revision: "1001bb4d826a52d1f399e183466143f4da7b741b",
        },
        Flavour::Kev9B => Pins {
            adapter: "jaredpalmer/kev-9b",
            adapter_revision: "2629c06a5aeb0feb3b9783bafed17ed8f39ecf5c",
            base: "Qwen/Qwen3.5-9B-Base",
            base_revision: "68c46c4b3498877f3ef123c856ecfde50c39f404",
        },
        Flavour::Kev27B => Pins {
            adapter: "jaredpalmer/kev-27b",
            adapter_revision: "01b81998019be550f0ae858727df49bac9511195",
            base: "Qwen/Qwen3.8-27B",
            base_revision: "1d4bf0f2ff6012fd82039f2fa52739d0dd7c60c0",
        },
    }
}

/// Best device first: a machine that can run a flavour on more than one
/// device prefers CUDA, then MLX, then the CPU.
const BEST_ORDER: [Device; 3] = [Device::Cuda, Device::Mlx, Device::Cpu];

/// The memory a flavour needs on a device, in GiB. `None` on MLX for 27b: it
/// never runs there, whatever the RAM.
fn memory_bound_gib(flavour: Flavour, device: Device) -> Option<u64> {
    use Device::{Cpu, Cuda, Mlx};
    use Flavour::{Kev4B, Kev08B, Kev9B, Kev27B};
    match (flavour, device) {
        (Kev08B, Mlx) => Some(8),
        (Kev08B, Cuda) => Some(4),
        (Kev08B, Cpu) => Some(8),
        (Kev4B, Mlx) => Some(24),
        (Kev4B, Cuda) => Some(12),
        (Kev4B, Cpu) => Some(32),
        (Kev9B, Mlx) => Some(32),
        (Kev9B, Cuda) => Some(24),
        (Kev9B, Cpu) => Some(64),
        (Kev27B, Mlx) => None,
        (Kev27B, Cuda) => Some(80),
        (Kev27B, Cpu) => Some(128),
    }
}

/// A note only: it blocks nothing. Every flavour but 0.8b is slow on the CPU.
fn slow(flavour: Flavour, device: Device) -> bool {
    device == Device::Cpu && flavour != Flavour::Kev08B
}

/// Why a device is not there at all on this machine, before memory is even
/// asked about: MLX needs Apple Silicon macOS, and CUDA needs a GPU the probe
/// found. The CPU is always there.
fn platform_unavailable(hardware: &Hardware, device: Device) -> Option<&'static str> {
    match device {
        Device::Mlx if !(hardware.os == "macos" && hardware.arch == "aarch64") => {
            Some("MLX needs macOS on Apple Silicon")
        }
        Device::Cuda if hardware.gpu.is_none() => Some("CUDA needs a GPU"),
        _ => None,
    }
}

/// Whether one flavour can run on one device, and why not: the platform is
/// missing, the flavour never runs there, or the machine's memory falls
/// short of the bound.
pub struct DeviceOption {
    pub device: Device,
    pub can_run: bool,
    pub reason: Option<String>,
    pub slow: bool,
}

fn device_option(hardware: &Hardware, flavour: Flavour, device: Device) -> DeviceOption {
    let slow = slow(flavour, device);
    if let Some(reason) = platform_unavailable(hardware, device) {
        return DeviceOption {
            device,
            can_run: false,
            reason: Some(reason.to_string()),
            slow,
        };
    }
    let Some(bound_gib) = memory_bound_gib(flavour, device) else {
        return DeviceOption {
            device,
            can_run: false,
            reason: Some(format!(
                "{} does not run on {}",
                flavour.as_str(),
                device.as_str()
            )),
            slow,
        };
    };
    let available_bytes = match device {
        Device::Cuda => hardware.gpu.as_ref().map_or(0, |gpu| gpu.vram_bytes),
        Device::Mlx | Device::Cpu => hardware.memory_bytes,
    };
    if available_bytes >= bound_gib * GIB {
        return DeviceOption {
            device,
            can_run: true,
            reason: None,
            slow,
        };
    }
    let unit = if device == Device::Cuda {
        "VRAM"
    } else {
        "RAM"
    };
    DeviceOption {
        device,
        can_run: false,
        reason: Some(format!(
            "needs {bound_gib} GB {unit}, found {} GB",
            available_bytes / GIB
        )),
        slow,
    }
}

/// One flavour with every device it might run on, in wire order.
pub struct FlavourOptions {
    pub flavour: Flavour,
    pub devices: Vec<DeviceOption>,
}

/// Every flavour with every device, in the order the wire always lists them:
/// 0.8b, 4b, 9b, 27b, and mlx, cuda, cpu within each.
pub fn options(hardware: &Hardware) -> Vec<FlavourOptions> {
    Flavour::ALL
        .into_iter()
        .map(|flavour| FlavourOptions {
            flavour,
            devices: Device::ALL
                .into_iter()
                .map(|device| device_option(hardware, flavour, device))
                .collect(),
        })
        .collect()
}

/// The best device that runs `flavour` on this machine, `cuda` then `mlx`
/// then `cpu`, or `None` where nothing does.
pub fn best_device(hardware: &Hardware, flavour: Flavour) -> Option<Device> {
    BEST_ORDER
        .into_iter()
        .find(|&device| device_option(hardware, flavour, device).can_run)
}

/// `4b` where some device on this machine can run it, else `0.8b`.
pub fn default_flavour(hardware: &Hardware) -> Flavour {
    match best_device(hardware, Flavour::Kev4B) {
        Some(_) => Flavour::Kev4B,
        None => Flavour::Kev08B,
    }
}

#[cfg(test)]
mod tests {
    use super::super::hardware::Gpu;
    use super::*;

    fn mac(memory_gb: u64) -> Hardware {
        Hardware {
            os: "macos".into(),
            arch: "aarch64".into(),
            memory_bytes: memory_gb * GIB,
            gpu: None,
        }
    }

    fn linux(memory_gb: u64, vram_gb: Option<u64>) -> Hardware {
        Hardware {
            os: "linux".into(),
            arch: "x86_64".into(),
            memory_bytes: memory_gb * GIB,
            gpu: vram_gb.map(|vram_gb| Gpu {
                name: "test-gpu".into(),
                vram_bytes: vram_gb * GIB,
            }),
        }
    }

    fn can_run(hardware: &Hardware, flavour: Flavour, device: Device) -> bool {
        device_option(hardware, flavour, device).can_run
    }

    /// Every pin is a 40-character sha, and 4b's matches the install's own
    /// pin (022, Sources) so the two never drift apart.
    #[test]
    fn every_pin_is_a_full_sha_and_4b_matches_the_install() {
        for flavour in Flavour::ALL {
            let pin = pins(flavour);
            assert_eq!(pin.adapter_revision.len(), 40, "{flavour:?} adapter");
            assert_eq!(pin.base_revision.len(), 40, "{flavour:?} base");
        }
        let kev4b = pins(Flavour::Kev4B);
        assert_eq!(kev4b.adapter, "jaredpalmer/kev-4b");
        assert_eq!(
            kev4b.adapter_revision,
            "139fdd94f1b6a6ad80cc15e08fcb99cac885a101"
        );
        assert_eq!(kev4b.base, "Qwen/Qwen3.5-4B-Base");
        assert_eq!(
            kev4b.base_revision,
            "1001bb4d826a52d1f399e183466143f4da7b741b"
        );
    }

    /// Every cell of the memory rule table, at the boundary and one GB below
    /// it, mlx and cpu against RAM and cuda against VRAM.
    #[test]
    fn every_cell_of_the_memory_rule_holds_at_its_boundary() {
        let cases: [(Flavour, Device, u64); 12] = [
            (Flavour::Kev08B, Device::Mlx, 8),
            (Flavour::Kev08B, Device::Cuda, 4),
            (Flavour::Kev08B, Device::Cpu, 8),
            (Flavour::Kev4B, Device::Mlx, 24),
            (Flavour::Kev4B, Device::Cuda, 12),
            (Flavour::Kev4B, Device::Cpu, 32),
            (Flavour::Kev9B, Device::Mlx, 32),
            (Flavour::Kev9B, Device::Cuda, 24),
            (Flavour::Kev9B, Device::Cpu, 64),
            (Flavour::Kev27B, Device::Mlx, 0), // never: checked separately below
            (Flavour::Kev27B, Device::Cuda, 80),
            (Flavour::Kev27B, Device::Cpu, 128),
        ];
        for (flavour, device, bound) in cases {
            if device == Device::Mlx && flavour == Flavour::Kev27B {
                continue;
            }
            let (at, below) = match device {
                Device::Cuda => (linux(256, Some(bound)), linux(256, Some(bound - 1))),
                Device::Mlx => (mac(bound), mac(bound - 1)),
                Device::Cpu => (linux(bound, None), linux(bound - 1, None)),
            };
            assert!(
                can_run(&at, flavour, device),
                "{flavour:?} on {device:?} at {bound} GB should run"
            );
            assert!(
                !can_run(&below, flavour, device),
                "{flavour:?} on {device:?} one GB under {bound} should not run"
            );
        }

        // 27b never runs on MLX, whatever the RAM.
        let huge_mac = mac(1024);
        let option = device_option(&huge_mac, Flavour::Kev27B, Device::Mlx);
        assert!(!option.can_run);
        assert_eq!(option.reason.as_deref(), Some("27b does not run on mlx"));
    }

    /// A 64 GB Mac offers 0.8b, 4b and 9b on MLX and on the CPU, never on
    /// CUDA (no GPU) and never 27b on MLX.
    #[test]
    fn a_64gb_mac_offers_08b_4b_9b_on_mlx_and_cpu_never_cuda_or_27b_on_mlx() {
        let hardware = mac(64);
        for flavour in [Flavour::Kev08B, Flavour::Kev4B, Flavour::Kev9B] {
            assert!(can_run(&hardware, flavour, Device::Mlx), "{flavour:?} mlx");
            assert!(can_run(&hardware, flavour, Device::Cpu), "{flavour:?} cpu");
            let cuda = device_option(&hardware, flavour, Device::Cuda);
            assert!(!cuda.can_run, "{flavour:?} cuda");
            assert_eq!(cuda.reason.as_deref(), Some("CUDA needs a GPU"));
        }
        let big = device_option(&hardware, Flavour::Kev27B, Device::Mlx);
        assert!(!big.can_run);
    }

    /// A Linux box with no GPU offers the CPU only.
    #[test]
    fn a_linux_box_with_no_gpu_offers_cpu_only() {
        let hardware = linux(256, None);
        for flavour in Flavour::ALL {
            assert!(can_run(&hardware, flavour, Device::Cpu), "{flavour:?} cpu");
            let mlx = device_option(&hardware, flavour, Device::Mlx);
            assert!(!mlx.can_run);
            assert_eq!(
                mlx.reason.as_deref(),
                Some("MLX needs macOS on Apple Silicon")
            );
            let cuda = device_option(&hardware, flavour, Device::Cuda);
            assert!(!cuda.can_run);
            assert_eq!(cuda.reason.as_deref(), Some("CUDA needs a GPU"));
        }
    }

    /// A Linux box with a 24 GB GPU offers CUDA up to 9b, and not 27b.
    #[test]
    fn a_linux_box_with_a_24gb_gpu_offers_cuda_up_to_9b() {
        let hardware = linux(16, Some(24));
        assert!(can_run(&hardware, Flavour::Kev08B, Device::Cuda));
        assert!(can_run(&hardware, Flavour::Kev4B, Device::Cuda));
        assert!(can_run(&hardware, Flavour::Kev9B, Device::Cuda));
        assert!(!can_run(&hardware, Flavour::Kev27B, Device::Cuda));
    }

    /// The reason names the shortfall in the units the device compares.
    #[test]
    fn the_reason_names_the_shortfall_in_gb() {
        let hardware = linux(16, Some(8));
        let cuda = device_option(&hardware, Flavour::Kev9B, Device::Cuda);
        assert_eq!(cuda.reason.as_deref(), Some("needs 24 GB VRAM, found 8 GB"));

        let hardware = mac(16);
        let mlx = device_option(&hardware, Flavour::Kev4B, Device::Mlx);
        assert_eq!(mlx.reason.as_deref(), Some("needs 24 GB RAM, found 16 GB"));
    }

    /// `slow` is a note only, on 4b, 9b and 27b on the CPU, and blocks
    /// nothing: a 0.8b on the CPU is not slow.
    #[test]
    fn slow_marks_4b_9b_27b_on_cpu_only() {
        let hardware = linux(256, Some(256));
        assert!(!device_option(&hardware, Flavour::Kev08B, Device::Cpu).slow);
        for flavour in [Flavour::Kev4B, Flavour::Kev9B, Flavour::Kev27B] {
            assert!(device_option(&hardware, flavour, Device::Cpu).slow);
            assert!(!device_option(&hardware, flavour, Device::Mlx).slow);
            assert!(!device_option(&hardware, flavour, Device::Cuda).slow);
        }
    }

    /// `options` lists every flavour and every device, unavailable ones
    /// included, in wire order.
    #[test]
    fn options_lists_every_flavour_and_device_in_order() {
        let listed = options(&linux(256, Some(256)));
        assert_eq!(
            listed.iter().map(|f| f.flavour).collect::<Vec<_>>(),
            Flavour::ALL
        );
        for flavour_options in &listed {
            assert_eq!(
                flavour_options
                    .devices
                    .iter()
                    .map(|d| d.device)
                    .collect::<Vec<_>>(),
                Device::ALL
            );
        }
    }

    /// The best device is CUDA, then MLX, then the CPU; the default flavour
    /// is 4b where some device runs it, else 0.8b.
    #[test]
    fn best_device_and_default_flavour_prefer_cuda_then_mlx_then_cpu() {
        let both = linux(256, Some(256));
        assert_eq!(best_device(&both, Flavour::Kev4B), Some(Device::Cuda));
        assert_eq!(default_flavour(&both), Flavour::Kev4B);

        let mac_only = mac(64);
        assert_eq!(best_device(&mac_only, Flavour::Kev4B), Some(Device::Mlx));

        let cpu_only = linux(64, None);
        assert_eq!(best_device(&cpu_only, Flavour::Kev4B), Some(Device::Cpu));

        let tiny = linux(2, None);
        assert_eq!(best_device(&tiny, Flavour::Kev4B), None);
        assert_eq!(default_flavour(&tiny), Flavour::Kev08B);
    }
}
