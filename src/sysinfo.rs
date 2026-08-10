//! Process resource usage helpers (memory and CPU) used by the F3 debug overlay.

use lazy_static::lazy_static;
use std::sync::Mutex;
use std::time::Instant;

#[cfg(target_os = "windows")]
mod imp {
    use std::ffi::c_void;
    use std::mem;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_nonpaged_pool_usage: usize,
        quota_nonpaged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }

    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut c_void,
            counters: *mut ProcessMemoryCounters,
            size: u32,
        ) -> i32;
    }

    extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
        fn GetProcessTimes(
            process: *mut c_void,
            creation_time: *mut u64,
            exit_time: *mut u64,
            kernel_time: *mut u64,
            user_time: *mut u64,
        ) -> i32;
    }

    /// Resident memory used by this process in bytes.
    pub fn memory_used() -> u64 {
        unsafe {
            let mut counters = ProcessMemoryCounters::default();
            counters.cb = mem::size_of::<ProcessMemoryCounters>() as u32;
            if GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) != 0 {
                counters.working_set_size as u64
            } else {
                0
            }
        }
    }

    /// Total physical memory installed in bytes.
    pub fn memory_total() -> u64 {
        unsafe {
            let mut status = MemoryStatusEx::default();
            status.length = mem::size_of::<MemoryStatusEx>() as u32;
            if GlobalMemoryStatusEx(&mut status) != 0 {
                status.total_phys
            } else {
                0
            }
        }
    }

    /// CPU time consumed by this process in seconds.
    pub fn process_cpu_seconds() -> f64 {
        unsafe {
            let mut creation = 0u64;
            let mut exit = 0u64;
            let mut kernel = 0u64;
            let mut user = 0u64;
            if GetProcessTimes(
                GetCurrentProcess(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            ) != 0
            {
                (kernel + user) as f64 / 10_000_000.0
            } else {
                0.0
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use std::fs;

    /// Resident memory used by this process in bytes (from /proc/self/statm).
    pub fn memory_used() -> u64 {
        fs::read_to_string("/proc/self/statm")
            .ok()
            .and_then(|contents| {
                contents
                    .split_whitespace()
                    .nth(1)
                    .and_then(|resident| resident.parse::<u64>().ok())
            })
            .map(|pages| pages * 4096)
            .unwrap_or(0)
    }

    /// Total physical memory installed in bytes (from /proc/meminfo).
    pub fn memory_total() -> u64 {
        fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|contents| {
                contents.lines().find_map(|line| {
                    line.strip_prefix("MemTotal:")
                        .and_then(|rest| rest.trim().split_whitespace().next())
                        .and_then(|kilobytes| kilobytes.parse::<u64>().ok())
                        .map(|kb| kb * 1024)
                })
            })
            .unwrap_or(0)
    }

    /// CPU time consumed by this process in seconds (from /proc/self/stat).
    pub fn process_cpu_seconds() -> f64 {
        fs::read_to_string("/proc/self/stat")
            .ok()
            .and_then(|contents| {
                // The command name (field 2) may contain spaces, so start from
                // the last ')' and skip the state field (field 3).
                let end = contents.rfind(')')?;
                let fields: Vec<&str> = contents[end + 2..].split_whitespace().collect();
                let utime: u64 = fields.get(11)?.parse().ok()?;
                let stime: u64 = fields.get(12)?.parse().ok()?;
                Some((utime + stime) as f64 / 100.0)
            })
            .unwrap_or(0.0)
    }
}

lazy_static! {
    static ref CPU_SAMPLE: Mutex<Option<(Instant, f64, f64)>> = Mutex::new(None);
}

/// Process CPU usage as a percentage of total system CPU, sampled over a
/// 0.5s window and cached in between (per-frame reads otherwise hit the
/// system timer granularity and report 0).
pub fn cpu_usage_percent() -> f64 {
    let now = Instant::now();
    let cpu = imp::process_cpu_seconds();
    let mut sample = CPU_SAMPLE.lock().unwrap();
    let usage = if let Some((last_now, last_cpu, last_usage)) = *sample {
        let elapsed = now.duration_since(last_now).as_secs_f64();
        if elapsed >= 0.5 {
            let cores = std::thread::available_parallelism()
                .map(|count| count.get())
                .unwrap_or(1)
                .max(1) as f64;
            let usage = if elapsed > 0.0 {
                ((cpu - last_cpu) / (elapsed * cores)).clamp(0.0, 1.0) * 100.0
            } else {
                0.0
            };
            *sample = Some((now, cpu, usage));
            usage
        } else {
            last_usage
        }
    } else {
        *sample = Some((now, cpu, 0.0));
        0.0
    };
    usage
}

/// Resident memory used by this process in bytes.
pub fn memory_used() -> u64 {
    imp::memory_used()
}

/// Total physical memory installed in bytes.
pub fn memory_total() -> u64 {
    imp::memory_total()
}
