//! Where the meter's numbers come from (spec 0023 FR-4): adoken's own
//! process only. macOS reads one `proc_pid_rusage(RUSAGE_INFO_V4)` and
//! falls back to `task_info` + `getrusage`; other platforms have no source
//! yet, so the meter and its setting are hidden there.

use super::meter::Reading;

/// A reader of the process's footprint, resident size, peak and CPU time.
/// Called only on the background executor (FR-5).
pub trait ResourceSource: Send + 'static {
    fn read(&mut self) -> Option<Reading>;
}

/// The platform has a source (the meter and its setting show).
pub const SUPPORTED: bool = cfg!(target_os = "macos");

/// This platform's source, if any.
pub fn platform() -> Option<Box<dyn ResourceSource>> {
    #[cfg(target_os = "macos")]
    {
        Some(Box::new(mac::ProcSource::new()))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
pub use mac::ProcSource;

#[cfg(target_os = "macos")]
mod mac {
    use mach2::kern_return::KERN_SUCCESS;
    use mach2::mach_time::{mach_timebase_info, mach_timebase_info_data_t};
    use mach2::task::task_info;
    use mach2::task_info::{
        MACH_TASK_BASIC_INFO, MACH_TASK_BASIC_INFO_COUNT, TASK_VM_INFO, mach_task_basic_info,
        task_vm_info,
    };
    use mach2::traps::mach_task_self;

    use super::super::meter::{Reading, ticks_to_ns};
    use super::ResourceSource;

    /// `proc_pid_rusage` with the Mach timebase read once.
    pub struct ProcSource {
        numer: u32,
        denom: u32,
    }

    impl Default for ProcSource {
        fn default() -> Self {
            Self::new()
        }
    }

    impl ProcSource {
        pub fn new() -> Self {
            let mut info = mach_timebase_info_data_t { numer: 1, denom: 1 };
            // SAFETY: `info` is a valid out-pointer for the call.
            let status = unsafe { mach_timebase_info(&mut info) };
            if status != KERN_SUCCESS || info.denom == 0 {
                info = mach_timebase_info_data_t { numer: 1, denom: 1 };
            }
            Self {
                numer: info.numer,
                denom: info.denom,
            }
        }

        /// One `proc_pid_rusage(getpid(), RUSAGE_INFO_V4)`; CPU times are
        /// Mach ticks, converted with the timebase.
        pub fn primary(&self) -> Option<Reading> {
            // SAFETY: `rusage_info_v4` is plain old data; zero is valid.
            let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
            // SAFETY: the buffer is a `rusage_info_v4`, matching the flavor,
            // passed as the `rusage_info_t *` the call expects.
            let status = unsafe {
                libc::proc_pid_rusage(
                    libc::getpid(),
                    libc::RUSAGE_INFO_V4,
                    (&mut info as *mut libc::rusage_info_v4).cast::<libc::rusage_info_t>(),
                )
            };
            (status == 0).then(|| Reading {
                footprint: info.ri_phys_footprint,
                resident: info.ri_resident_size,
                peak: Some(info.ri_lifetime_max_phys_footprint),
                cpu_ns: ticks_to_ns(
                    info.ri_user_time + info.ri_system_time,
                    self.numer,
                    self.denom,
                ),
            })
        }

        /// `task_info` for footprint and resident size, `getrusage` for CPU
        /// (microseconds, no conversion); no peak.
        pub fn fallback(&self) -> Option<Reading> {
            let mut vm = task_vm_info::default();
            let mut count =
                (std::mem::size_of::<task_vm_info>() / std::mem::size_of::<u32>()) as u32;
            // SAFETY: `vm` is `count` natural words long; the kernel writes
            // at most that many (older kernels fewer, up to their revision).
            let status = unsafe {
                task_info(
                    mach_task_self(),
                    TASK_VM_INFO,
                    (&mut vm as *mut task_vm_info).cast(),
                    &mut count,
                )
            };
            if status != KERN_SUCCESS {
                return None;
            }
            let mut basic = mach_task_basic_info::default();
            let mut count = MACH_TASK_BASIC_INFO_COUNT;
            // SAFETY: as above, with the flavor's own struct and count.
            let status = unsafe {
                task_info(
                    mach_task_self(),
                    MACH_TASK_BASIC_INFO,
                    (&mut basic as *mut mach_task_basic_info).cast(),
                    &mut count,
                )
            };
            if status != KERN_SUCCESS {
                return None;
            }
            // SAFETY: plain old data; zero is valid.
            let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
            // SAFETY: `usage` is a valid out-pointer.
            if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
                return None;
            }
            let micros = |t: libc::timeval| t.tv_sec as u64 * 1_000_000 + t.tv_usec as u64;
            Some(Reading {
                footprint: vm.phys_footprint,
                resident: basic.resident_size,
                peak: None,
                cpu_ns: (micros(usage.ru_utime) + micros(usage.ru_stime)) * 1_000,
            })
        }
    }

    impl ResourceSource for ProcSource {
        fn read(&mut self) -> Option<Reading> {
            self.primary().or_else(|| self.fallback())
        }
    }
}
