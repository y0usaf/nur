//! System information service — CPU, RAM, temperature, GPU.
//!
//! Uses the `sysinfo` crate for accurate CPU (two-sample delta), memory, and
//! temperature readings. GPU utilisation is read from AMD/Intel sysfs.
//!
//! A dedicated OS thread runs the blocking `sysinfo` refreshes every 2 s and
//! writes results to a shared slot. A GPUI async task wakes every 2 s, reads
//! the slot, and pushes updates to the GPUI entity.

use gpui::{App, AppContext, Entity};
use sysinfo::{Components, CpuRefreshKind, System};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct SysInfoState {
    /// Overall CPU usage, 0–100.
    pub cpu_percent: u32,
    /// RAM usage, 0–100.
    pub memory_percent: u32,
    /// Used RAM in GiB.
    pub memory_used_gb: f64,
    /// Total RAM in GiB.
    pub memory_total_gb: f64,
    /// CPU package / die temperature in °C, if a sensor is found.
    pub temperature: Option<i32>,
    /// GPU busy percentage, 0–100, if a supported GPU is found.
    pub gpu_percent: Option<u32>,
}

pub struct SysInfoService;

impl SysInfoService {
    pub fn start(cx: &mut App) -> Entity<SysInfoState> {
        let entity = cx.new(|_| SysInfoState::default());
        let weak = entity.downgrade();

        // Shared slot: OS thread writes, GPUI task reads.
        let slot: Arc<Mutex<Option<SysInfoState>>> = Arc::new(Mutex::new(None));
        let slot_writer = slot.clone();
        let slot_reader = slot.clone();

        // Dedicated OS thread — sysinfo I/O is blocking.
        std::thread::spawn(move || {
            // new_all() populates CPUs and memory so the first targeted
            // refresh produces correct values rather than zeros.
            let mut system = System::new_all();
            let mut components = Components::new_with_refreshed_list();

            // Establish CPU baseline; the next refresh (after sleep) gives
            // the accurate delta.
            system.refresh_cpu_specifics(CpuRefreshKind::everything());

            loop {
                std::thread::sleep(Duration::from_secs(2));

                system.refresh_memory();
                system.refresh_cpu_specifics(CpuRefreshKind::everything());
                components.refresh(true);

                let state = compute_state(&system, &components);
                if let Ok(mut guard) = slot_writer.lock() {
                    *guard = Some(state);
                }
            }
        });

        // GPUI task: pick up updates from the slot and notify the entity.
        cx.spawn(async move |cx| loop {
            cx.background_executor()
                .timer(Duration::from_secs(2))
                .await;

            let state = slot_reader.lock().ok().and_then(|mut g| g.take());
            if let Some(state) = state {
                cx.update(|cx| {
                    if let Some(e) = weak.upgrade() {
                        e.update(cx, |s, cx| {
                            *s = state;
                            cx.notify();
                        });
                    }
                });
            }
        })
        .detach();

        entity
    }
}

/// Compute memory usage percentage from raw byte counts.
/// Returns 0 when `total` is zero to avoid division by zero.
pub(crate) fn compute_memory_percent(used: f64, total: f64) -> u32 {
    if total > 0.0 {
        ((used / total) * 100.0) as u32
    } else {
        0
    }
}

fn compute_state(system: &System, components: &Components) -> SysInfoState {
    let cpu_percent = system.global_cpu_usage().round() as u32;

    let total = system.total_memory() as f64;
    let used = system.used_memory() as f64;
    let memory_percent = compute_memory_percent(used, total);
    let memory_total_gb = total / 1_073_741_824.0;
    let memory_used_gb = used / 1_073_741_824.0;

    // Find the first CPU/package temperature sensor.
    let temperature = components
        .iter()
        .find(|c| {
            let label = c.label().to_lowercase();
            label.contains("package")
                || label.contains("coretemp")
                || label.contains("k10temp")
                || label.contains("cpu")
        })
        .and_then(|c| c.temperature().map(|t| t as i32));

    let gpu_percent = read_gpu_percent();

    SysInfoState {
        cpu_percent,
        memory_percent,
        memory_total_gb,
        memory_used_gb,
        temperature,
        gpu_percent,
    }
}

/// Try AMD sysfs (amdgpu / radeon) across card0–card3.
/// Returns `None` if no supported GPU is found.
fn read_gpu_percent() -> Option<u32> {
    for i in 0..4 {
        let path = format!("/sys/class/drm/card{}/device/gpu_busy_percent", i);
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(v) = s.trim().parse::<u32>() {
                return Some(v);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- compute_memory_percent ---

    #[test]
    fn memory_percent_half() {
        assert_eq!(compute_memory_percent(512.0, 1024.0), 50);
    }

    #[test]
    fn memory_percent_full() {
        assert_eq!(compute_memory_percent(1024.0, 1024.0), 100);
    }

    #[test]
    fn memory_percent_zero_used() {
        assert_eq!(compute_memory_percent(0.0, 1024.0), 0);
    }

    #[test]
    fn memory_percent_zero_total_returns_zero() {
        // Must not divide by zero
        assert_eq!(compute_memory_percent(0.0, 0.0), 0);
    }

    #[test]
    fn memory_percent_rounds_down() {
        // 333 / 1000 = 33.3% → truncates to 33
        assert_eq!(compute_memory_percent(333.0, 1000.0), 33);
    }

    #[test]
    fn memory_percent_realistic_8gb() {
        // 3 GiB used out of 8 GiB = 37%
        let used = 3.0 * 1_073_741_824.0;
        let total = 8.0 * 1_073_741_824.0;
        assert_eq!(compute_memory_percent(used, total), 37);
    }

    // --- SysInfoState ---

    #[test]
    fn sysinfo_state_default() {
        let s = SysInfoState::default();
        assert_eq!(s.cpu_percent, 0);
        assert_eq!(s.memory_percent, 0);
        assert_eq!(s.temperature, None);
        assert_eq!(s.gpu_percent, None);
    }

    #[test]
    fn sysinfo_state_clone() {
        let a = SysInfoState {
            cpu_percent: 42,
            memory_percent: 70,
            memory_used_gb: 5.5,
            memory_total_gb: 16.0,
            temperature: Some(65),
            gpu_percent: Some(30),
        };
        let b = a.clone();
        assert_eq!(b.cpu_percent, 42);
        assert_eq!(b.temperature, Some(65));
        assert_eq!(b.gpu_percent, Some(30));
    }
}
