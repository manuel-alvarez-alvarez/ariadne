//! Choosing a Kev flavour and device that fit the machine (022, flavours and
//! devices).
//!
//! `ai_permissions_hardware` replaces the whole hardware probe, so these
//! tests prove the daemon's own wiring — the wire shape, the refusal, the
//! startup fill — on hardware the test picks, not the machine running it.

use axum::http::StatusCode;
use serde_json::json;

use ariadne_api::error::ErrorBody;
use ariadne_api::permissions::{AiPermissionsStatusDto, Device, Flavour};
use ariadne_daemon::ai_permissions::hardware::HardwareOverride;

use crate::common::{Harness, harness, put_json, shared_script};

async fn status(h: &Harness) -> AiPermissionsStatusDto {
    h.get("/v1/permissions/ai").await
}

fn update(body: serde_json::Value) -> axum::http::Request<axum::body::Body> {
    put_json("/v1/permissions/ai", body)
}

fn mac(memory_gb: u64) -> HardwareOverride {
    HardwareOverride {
        os: "macos".into(),
        arch: "aarch64".into(),
        memory_gb,
        gpu_name: None,
        vram_gb: None,
    }
}

fn linux(memory_gb: u64, vram_gb: Option<u64>) -> HardwareOverride {
    HardwareOverride {
        os: "linux".into(),
        arch: "x86_64".into(),
        memory_gb,
        gpu_name: vram_gb.map(|_| "test-gpu".to_string()),
        vram_gb,
    }
}

fn can_run(status: &AiPermissionsStatusDto, flavour: Flavour, device: Device) -> bool {
    status
        .flavours
        .iter()
        .find(|f| f.flavour == flavour)
        .and_then(|f| f.devices.iter().find(|d| d.device == device))
        .expect("every flavour and device is listed")
        .can_run
}

/// A 64 GB Mac offers 0.8b, 4b and 9b on MLX and on the CPU, never on CUDA —
/// there is no GPU — and never 27b on MLX at all.
#[tokio::test]
async fn a_64gb_mac_offers_08b_4b_9b_on_mlx_and_cpu_never_cuda_or_27b_on_mlx() {
    let h = harness().ai_permissions_hardware(mac(64)).await;
    let status = status(&h).await;
    assert_eq!(status.hardware.os, "macos");
    assert_eq!(status.hardware.gpu, None);
    for flavour in [Flavour::Kev08B, Flavour::Kev4B, Flavour::Kev9B] {
        assert!(can_run(&status, flavour, Device::Mlx), "{flavour:?} mlx");
        assert!(can_run(&status, flavour, Device::Cpu), "{flavour:?} cpu");
        assert!(!can_run(&status, flavour, Device::Cuda), "{flavour:?} cuda");
    }
    assert!(!can_run(&status, Flavour::Kev27B, Device::Mlx));
}

/// A Linux box with no GPU offers the CPU only.
#[tokio::test]
async fn a_linux_box_with_no_gpu_offers_cpu_only() {
    let h = harness().ai_permissions_hardware(linux(256, None)).await;
    let status = status(&h).await;
    for flavour in Flavour::ALL {
        assert!(can_run(&status, flavour, Device::Cpu), "{flavour:?} cpu");
        assert!(!can_run(&status, flavour, Device::Mlx), "{flavour:?} mlx");
        assert!(!can_run(&status, flavour, Device::Cuda), "{flavour:?} cuda");
    }
}

/// A Linux box with a 24 GB GPU offers CUDA up to 9b, and not 27b.
#[tokio::test]
async fn a_linux_box_with_a_24gb_gpu_offers_cuda_up_to_9b() {
    let h = harness().ai_permissions_hardware(linux(16, Some(24))).await;
    let status = status(&h).await;
    assert!(can_run(&status, Flavour::Kev08B, Device::Cuda));
    assert!(can_run(&status, Flavour::Kev4B, Device::Cuda));
    assert!(can_run(&status, Flavour::Kev9B, Device::Cuda));
    assert!(!can_run(&status, Flavour::Kev27B, Device::Cuda));
}

/// A combination that cannot run is refused with 422 `flavour_unsupported`
/// and leaves the row unchanged.
#[tokio::test]
async fn put_refuses_an_unsupported_combination_and_leaves_the_row_unchanged() {
    let h = harness().ai_permissions_hardware(linux(32, Some(8))).await;

    let refused: ErrorBody = h
        .json(
            update(json!({"flavour": "9b", "device": "cuda"})),
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
    assert_eq!(refused.error.code, "flavour_unsupported");
    assert!(refused.error.message.contains("24 GB"), "{refused:?}");

    let unchanged = status(&h).await;
    assert_eq!(
        unchanged.flavour,
        Flavour::Kev4B,
        "the default was untouched"
    );
    assert_eq!(unchanged.device, Device::Cpu, "and so was the device");
}

/// A request with a flavour and no device picks the best device that runs
/// it: CUDA over MLX over the CPU.
#[tokio::test]
async fn put_with_only_a_flavour_picks_the_best_device() {
    let h = harness()
        .ai_permissions_hardware(linux(256, Some(256)))
        .await;

    let chosen: AiPermissionsStatusDto = h
        .json(update(json!({"flavour": "9b"})), StatusCode::OK)
        .await;
    assert_eq!(chosen.flavour, Flavour::Kev9B);
    assert_eq!(chosen.device, Device::Cuda);
}

/// A request with a device and no flavour keeps the stored flavour.
#[tokio::test]
async fn put_with_only_a_device_keeps_the_stored_flavour() {
    let h = harness().ai_permissions_hardware(mac(64)).await;

    let chosen: AiPermissionsStatusDto = h
        .json(update(json!({"device": "cpu"})), StatusCode::OK)
        .await;
    assert_eq!(chosen.flavour, Flavour::Kev4B, "the default flavour");
    assert_eq!(chosen.device, Device::Cpu);
}

/// A device-only `PUT` reads the true stored flavour, not `status`'s own
/// fallback: on a host where nothing runs the stored `4b`, `status` reports
/// `0.8b`, but the request must still try `4b` — and refuse, since `4b`
/// cannot run on `cpu` here either — rather than silently keeping the
/// fallback flavour instead of the one actually stored.
#[tokio::test]
async fn put_with_only_a_device_keeps_the_stored_flavour_even_when_status_falls_back() {
    let h = harness().ai_permissions_hardware(linux(16, None)).await;
    assert_eq!(
        status(&h).await.flavour,
        Flavour::Kev08B,
        "status falls back since nothing runs the stored 4b"
    );

    let refused: ErrorBody = h
        .json(
            update(json!({"device": "cpu"})),
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
    assert_eq!(refused.error.code, "flavour_unsupported");

    let row = h.store.ai_permission_settings().await.unwrap();
    assert_eq!(
        row.flavour, "4b",
        "the stored flavour was not silently changed"
    );
    assert_eq!(row.device, None, "the refusal wrote nothing");
}

/// A fresh row keeps the default flavour, `4b`, and the daemon fills a
/// device at startup: the same fill an install from before flavours existed
/// gets, since its device also starts `NULL` after the migration.
#[tokio::test]
async fn a_fresh_row_keeps_4b_and_gets_a_device_at_startup() {
    let h = harness().ai_permissions_hardware(mac(64)).await;
    let status = status(&h).await;
    assert_eq!(status.flavour, Flavour::Kev4B);
    assert_eq!(status.device, Device::Mlx);

    let row = h.store.ai_permission_settings().await.unwrap();
    assert_eq!(row.flavour, "4b");
    assert_eq!(row.device.as_deref(), Some("mlx"), "filled, not left NULL");
}

/// A 16 GiB Linux box with no GPU cannot run `4b` on any device — `cpu`
/// alone needs 32 GB. The startup fill leaves its device `NULL` rather than
/// storing `cpu`, which the flavour and device table itself marks unable to
/// run `4b` here. `GET` never reports that unsupported pair either: it falls
/// back to `0.8b` on `cpu`, which this host can run.
#[tokio::test]
async fn a_16gb_linux_box_that_cannot_run_4b_keeps_its_device_unset() {
    let h = harness().ai_permissions_hardware(linux(16, None)).await;

    let row = h.store.ai_permission_settings().await.unwrap();
    assert_eq!(row.flavour, "4b", "the stored flavour is untouched");
    assert_eq!(row.device, None, "no device on this host runs 4b");

    let status = status(&h).await;
    assert!(
        !can_run(&status, Flavour::Kev4B, Device::Cpu),
        "cpu does not run 4b on this host"
    );
    assert_eq!(
        status.flavour,
        Flavour::Kev08B,
        "status never reports the unsupported 4b/cpu pair"
    );
    assert_eq!(status.device, Device::Cpu);
    assert!(
        can_run(&status, status.flavour, status.device),
        "the reported pair is one this host can actually run"
    );
}

/// The configured `nvidia_smi_bin` is what the hardware probe runs to find a
/// GPU, end to end through the daemon: a stub reporting one GPU is read as a
/// CUDA device, converted from the MiB `nvidia-smi` prints to bytes.
#[tokio::test]
async fn a_configured_nvidia_smi_bin_reaches_the_hardware_probe() {
    let stub = shared_script("#!/bin/sh\necho 'NVIDIA A100-SXM4-80GB, 81920'\n");
    let h = harness().nvidia_smi_bin(stub.display().to_string()).await;
    let status = status(&h).await;
    let gpu = status.hardware.gpu.expect("the stub reported a GPU");
    assert_eq!(gpu.name, "NVIDIA A100-SXM4-80GB");
    assert_eq!(gpu.vram_bytes, 81920 * 1024 * 1024);
}
