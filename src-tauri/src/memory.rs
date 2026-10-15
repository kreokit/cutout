//! How much memory the computer has and how much is available right now, so the Best
//! model can pick an input size that fits (or warn before it would run out).
//!
//! "Available" is what can be used without swapping: macOS free + inactive + speculative +
//! purgeable pages (like Activity Monitor), Windows `ullAvailPhys`, Linux `MemAvailable`.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Memory {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

/// `MemTotal` and `MemAvailable` from the text of `/proc/meminfo` (values in kB).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_meminfo(text: &str) -> Option<Memory> {
    let field = |name: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(name)?.strip_prefix(':'))
            .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
            .map(|kb| kb * 1024)
    };
    Some(Memory {
        total_bytes: field("MemTotal")?,
        available_bytes: field("MemAvailable")?,
    })
}

#[cfg(target_os = "linux")]
pub fn memory() -> Option<Memory> {
    parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

#[cfg(target_os = "macos")]
pub fn memory() -> Option<Memory> {
    // SAFETY: sysctlbyname and host_statistics64 only write into the buffers we pass, with
    // their sizes.
    unsafe {
        let mut total: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        if libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&raw mut total).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return None;
        }
        let mut stats: libc::vm_statistics64 = std::mem::zeroed();
        let mut count = libc::HOST_VM_INFO64_COUNT;
        #[allow(deprecated)] // libc points to the mach2 crate; the call is the same.
        let host = libc::mach_host_self();
        if libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            (&raw mut stats).cast(),
            &mut count,
        ) != libc::KERN_SUCCESS
        {
            return None;
        }
        #[allow(deprecated)]
        let page = libc::vm_page_size as u64;
        let pages = stats.free_count as u64
            + stats.inactive_count as u64
            + stats.speculative_count as u64
            + stats.purgeable_count as u64;
        Some(Memory {
            total_bytes: total,
            available_bytes: (pages * page).min(total),
        })
    }
}

#[cfg(windows)]
pub fn memory() -> Option<Memory> {
    #[repr(C)]
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
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }
    // SAFETY: the struct has the documented MEMORYSTATUSEX layout and its length set.
    unsafe {
        let mut status: MemoryStatusEx = std::mem::zeroed();
        status.length = std::mem::size_of::<MemoryStatusEx>() as u32;
        (GlobalMemoryStatusEx(&mut status) != 0).then_some(Memory {
            total_bytes: status.total_phys,
            available_bytes: status.avail_phys,
        })
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn memory() -> Option<Memory> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_proc_meminfo() {
        let text = "MemTotal:        8048576 kB\nMemFree:          123456 kB\nMemAvailable:    4024288 kB\n";
        assert_eq!(
            parse_meminfo(text),
            Some(Memory {
                total_bytes: 8_048_576 * 1024,
                available_bytes: 4_024_288 * 1024,
            })
        );
        assert_eq!(parse_meminfo("MemTotal: 10 kB\n"), None);
    }

    #[test]
    fn this_computer_has_memory() {
        let m = memory().expect("memory info on the dev and CI platforms");
        assert!(m.total_bytes >= 1 << 30, "{m:?}");
        assert!(
            m.available_bytes > 0 && m.available_bytes <= m.total_bytes,
            "{m:?}"
        );
    }
}
