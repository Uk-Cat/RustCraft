//! Vectored Exception Handler (VEH) for Windows SEH exceptions.
//!
//! ## Why this exists
//!
//! Rust's `panic::catch_unwind` only catches Rust panics. Windows raises
//! Structured Exceptions (SEH) for things like:
//!   - `STATUS_ACCESS_VIOLATION` (0xC0000005) — null/wild pointer deref
//!   - `STATUS_STACK_OVERFLOW`   (0xC00000FD) — exhausted thread stack
//!   - `STATUS_ILLEGAL_INSTRUCTION` (0xC000001D)
//!   - `STATUS_INTEGER_DIVIDE_BY_ZERO` (0xC0000094)
//!   - ... and many more
//!
//! A Rust panic hook DOES NOT fire for SEH exceptions (with the partial
//! exception of `STATUS_STACK_OVERFLOW`, which `std` converts to a panic
//! only when there is enough stack left to do so — often there isn't).
//! When an SEH exception fires, the OS default handler kills the
//! process immediately with no Rust-side logging.
//!
//! A Vectored Exception Handler is the Windows-supported way to intercept
//! SEH exceptions BEFORE the OS default handler runs. We register one
//! with priority 1 (first in the chain) so we get the first crack at
//! every exception.
//!
//! ## What the handler does
//!
//! When an SEH exception fires:
//!   1. Map the exception code to a human-readable name.
//!   2. Write a `crash-veh-<timestamp>.log` file in the same directory
//!      as the main `client.log`, containing:
//!         - exception code & name
//!         - exception address
//!         - thread id
//!         - a best-effort backtrace (using `Backtrace::force_capture`)
//!         - the build hash (so we know which binary crashed)
//!         - the last 8 KB of the main log (so we have packet context)
//!   3. Append a one-line marker to the main `client.log` so the user
//!      sees the crash referenced from the regular log too.
//!   4. Print the same info to stderr (visible if launched from a
//!      terminal; also captured by Windows' WerFault crash dialog).
//!   5. Return `EXCEPTION_CONTINUE_SEARCH` so the OS default handler
//!      still runs (the process will still be killed — we are NOT
//!      trying to recover, just to leave a forensic trail).
//!
//! ## What the handler deliberately does NOT do
//!
//! - Does NOT attempt recovery. SEH recovery is extremely fragile
//!   (the program state may be corrupted) and we'd rather crash cleanly
//!   with a log than limp on with subtle corruption.
//! - Does NOT acquire any non-reentrant locks from inside the handler.
//!   The handler runs in arbitrary thread context with a possibly
//!   blown stack, so we only use try-lock-style primitives and
//!   fall through silently on contention.
//! - Does NOT allocate large buffers. We use a fixed 8 KB tail buffer.
//!
//! ## Non-Windows behaviour
//!
//! On non-Windows targets all of this compiles to no-ops, so the same
//! source tree can still build for Linux / macOS without `#ifdef`
//! noise at call sites.
//!
//! ## Why raw FFI instead of `windows-sys`
//!
//! The `windows-sys` crate has had several breaking API reorganisations
//! between 0.48, 0.52, and 0.59 (e.g. `EXCEPTION_RECORD` moved from
//! `Win32::Foundation` to `Win32::System::Diagnostics::Debug`, and
//! `AddVectoredExceptionHandler` has disappeared from some feature
//! combinations). To avoid coupling our build to whatever
//! `windows-sys` version our transitive deps (winit, glutin, …)
//! resolve to, we declare the small handful of FFI signatures we
//! need directly. The Windows ABI is stable, so this is safe.

#![allow(clippy::missing_safety_doc)]

use std::backtrace::Backtrace;
use std::sync::atomic::{AtomicBool, Ordering};

// Imports only needed on Windows — gate them so non-Windows builds
// don't get unused-import warnings.
#[cfg(target_os = "windows")]
use std::fs;
#[cfg(target_os = "windows")]
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(target_os = "windows")]
use std::time::{SystemTime, UNIX_EPOCH};

/// Tracks whether `init()` has already been called so we don't double-
/// register the handler. Vectored handlers DO stack (calling
/// `AddVectoredExceptionHandler` twice registers two handlers), so this
/// is mostly a defensive measure.
static VEH_INSTALLED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Raw FFI declarations (Windows only)
// ---------------------------------------------------------------------------

/// The OS passes us a pointer to one of these as the argument to the
/// vectored handler. We only need to read two fields from it.
#[cfg(target_os = "windows")]
#[repr(C)]
struct EXCEPTION_POINTERS {
    exception_record: *mut EXCEPTION_RECORD,
    context_record: *mut u8, // we don't actually read the CONTEXT — opaque
}

/// EXCEPTION_RECORD layout, just the fields we touch. The real struct
/// has more fields (ExceptionInformation array, ExceptionAddress, etc.)
/// but we only need ExceptionCode and ExceptionAddress.
#[cfg(target_os = "windows")]
#[repr(C)]
struct EXCEPTION_RECORD {
    exception_code: i32,
    exception_flags: u32,
    exception_record: *mut EXCEPTION_RECORD, // nested exception, if any
    exception_address: *mut u8,
    number_parameters: u32,
    // exception_information: [usize; 15] follows but we don't declare it
    // — accessing it would require a fixed-size array and we don't use it.
}

#[cfg(target_os = "windows")]
extern "system" {
    fn AddVectoredExceptionHandler(
        first_handler: u32,
        handler: unsafe extern "system" fn(*mut EXCEPTION_POINTERS) -> i32,
    ) -> *mut u8;
    fn GetCurrentThreadId() -> u32;
    fn SetThreadStackGuarantee(reserve_size: *mut u32) -> u32;
}

/// Returned by the handler to mean "I didn't handle it, keep looking".
/// We always return this — we are NOT trying to recover, only to log.
#[cfg(target_os = "windows")]
const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

/// Exception code for stack overflow. Used to trigger
/// `SetThreadStackGuarantee` before doing logging work.
#[cfg(target_os = "windows")]
const EXCEPTION_STACK_OVERFLOW: i32 = 0xC000_00FDu32 as i32;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise the VEH. Safe to call multiple times — second and later
/// calls are no-ops.
///
/// On non-Windows targets this is a no-op.
#[cfg(target_os = "windows")]
pub fn init() {
    if VEH_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }

    // SAFETY: `AddVectoredExceptionHandler` is thread-safe and the
    // handler we register only touches thread-local / global state via
    // try-lock patterns. The `1` argument means "first in chain".
    unsafe {
        let handle = AddVectoredExceptionHandler(1, veh_handler);
        if handle.is_null() {
            // Best-effort: log to stderr that VEH setup failed. The
            // process can still run, we just won't catch SEH.
            eprintln!("veh: AddVectoredExceptionHandler returned NULL, SEH capture disabled");
        }
        // We deliberately DON'T log via the regular `info!()` macro
        // here because `init()` is called BEFORE the global logger
        // is fully wired up. The main.rs caller logs "VEH init
        // complete" itself once the logger is ready.
    }
}

#[cfg(not(target_os = "windows"))]
pub fn init() {
    // No-op on non-Windows targets. Mark installed so a subsequent
    // call on a port that DID add Windows support doesn't double-fire.
    let _ = VEH_INSTALLED.swap(true, Ordering::SeqCst);
}

// ---------------------------------------------------------------------------
// Windows implementation
// ---------------------------------------------------------------------------

/// The actual VEH callback. Registered via `AddVectoredExceptionHandler`.
///
/// # Safety
///
/// This is an unsafe extern "system" callback as required by the Windows
/// API. Inside we only do:
///   - reads from the `EXCEPTION_POINTERS` passed to us by the OS
///   - try-lock style file I/O (no blocking)
///   - `Backtrace::force_capture` (which is signal-safe enough for our
///     purposes — it does allocate, but on Windows SEH we're already
///     off the raw signal path)
#[cfg(target_os = "windows")]
unsafe extern "system" fn veh_handler(
    exception_info: *mut EXCEPTION_POINTERS,
) -> i32 {
    // Defensive: never recursively re-enter. If our handler itself
    // faults, the OS will skip us on the second pass (because we
    // return EXCEPTION_CONTINUE_SEARCH), but we also add a thread-local
    // guard so we don't even try to log twice on the same thread.
    thread_local! {
        static IN_HANDLER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    let already_in = IN_HANDLER.with(|f| {
        let was = f.get();
        f.set(true);
        was
    });
    if already_in {
        return EXCEPTION_CONTINUE_SEARCH;
    }

    // Snapshot the exception info before doing anything else. If the
    // pointer is null (shouldn't happen but be defensive), bail.
    let (code, address) = if exception_info.is_null() {
        (0i32, 0usize)
    } else {
        // SAFETY: the OS guarantees ExceptionRecord points to a valid
        // EXCEPTION_RECORD for the duration of the callback.
        let rec = (*exception_info).exception_record;
        if rec.is_null() {
            (0i32, 0usize)
        } else {
            ((*rec).exception_code as i32, (*rec).exception_address as usize)
        }
    };

    // For stack-overflow: the stack is nearly exhausted. We must NOT
    // do anything that requires significant stack. Backtrace capture
    // and file I/O both need stack. Touch a guard page to reset the
    // stack guard (Windows pattern), then proceed cautiously. If
    // anything below faults, the recursive-guard above prevents us
    // from looping.
    if code == EXCEPTION_STACK_OVERFLOW {
        reset_stack_guard();
    }

    let thread_id = unsafe { GetCurrentThreadId() };
    let bt = Backtrace::force_capture();
    write_crash_artifacts(code as u32, address, thread_id, &bt);

    // Reset the thread-local so future exceptions on this thread can
    // be logged (e.g. if we get a follow-up EXCEPTION_CONTINUE_SEARCH
    // chain). Strictly optional.
    IN_HANDLER.with(|f| f.set(false));

    // Always let the OS default handler run. We are NOT trying to
    // recover — only to leave a forensic trail.
    EXCEPTION_CONTINUE_SEARCH
}

#[cfg(target_os = "windows")]
fn reset_stack_guard() {
    // Windows-specific: use SetThreadStackGuarantee to reserve a small
    // stack area so we have enough stack to do file I/O for the crash
    // log. This is the documented pattern for stack-overflow recovery.
    unsafe {
        let mut reserve: u32 = 64 * 1024; // 64 KB
        let _ = SetThreadStackGuarantee(&mut reserve);
    }
}

#[cfg(target_os = "windows")]
fn write_crash_artifacts(code: u32, address: usize, thread_id: u32, bt: &Backtrace) {
    let name = exception_name(code);
    let build_hash = env!("LEAFISH_BUILD_GIT_HASH");
    let build_time = env!("LEAFISH_BUILD_TIME");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let header = format!(
        "\n\n================ VEH EXCEPTION ================\n\
         time       : epoch+{} (utc)\n\
         build      : {} (built {})\n\
         exception  : 0x{:08X} ({})\n\
         address    : 0x{:016X}\n\
         thread id  : {}\n\
         main ticks : {} (main-thread liveness; 0 = crash before main loop)\n\
         ---------------- backtrace ----------------\n{}\n\
         =============================================\n",
        now, build_hash, build_time, code, name, address, thread_id,
        crate::main_tick_count(),
        bt
    );

    // Print to stderr first — this is the cheapest and works even if
    // the file system is unwritable.
    eprintln!("{}", header);

    // Write to the global log file (try_lock — if the logging thread
    // holds the mutex, we just skip; never block in an SEH handler).
    if let Some(global_mutex) = crate::console::global_log_mutex() {
        if let Some(mut gf) = global_mutex.try_lock() {
            let _ = gf.write_all(b"\n");
            let _ = gf.write_all(header.as_bytes());
            let _ = gf.sync_all();
        }
    }

    // Also write a dedicated crash-veh-*.log file next to the main log.
    if let Some(main_path) = crate::console::log_file_path() {
        let dir = main_path.parent().unwrap_or(std::path::Path::new("."));
        let crash_path = dir.join(format!("crash-veh-{}.log", now));
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&crash_path)
        {
            let _ = f.write_all(header.as_bytes());
            let _ = f.sync_all();

            // Append the last 8 KB of the main log so we have packet
            // context along with the backtrace. This is invaluable
            // when dissecting a crash that happened mid-packet.
            if let Some(global_mutex) = crate::console::global_log_mutex() {
                if let Some(mut gf) = global_mutex.try_lock() {
                    let file_len = gf.metadata().map(|m| m.len()).unwrap_or(0);
                    let tail_size = std::cmp::min(8192, file_len);
                    if tail_size > 0 {
                        let _ = gf.seek(SeekFrom::End(-(tail_size as i64)));
                        let mut tail = String::new();
                        let _ = gf.read_to_string(&mut tail);
                        let _ = f.write_all(b"\n--- last 8KB of main log ---\n");
                        let _ = f.write_all(tail.as_bytes());
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn exception_name(code: u32) -> &'static str {
    // Common SEH codes. Sourced from ntstatus.h.
    match code {
        0xC0000005 => "STATUS_ACCESS_VIOLATION (null/wild pointer deref or unmapped memory)",
        0xC00000FD => "STATUS_STACK_OVERFLOW (exhausted thread stack)",
        0xC000001D => "STATUS_ILLEGAL_INSTRUCTION (CPU decoded an invalid opcode)",
        0xC0000025 => "STATUS_NONCONTINUABLE_EXCEPTION (a non-continuable exception was continued)",
        0xC0000094 => "STATUS_INTEGER_DIVIDE_BY_ZERO",
        0xC0000095 => "STATUS_INTEGER_OVERFLOW",
        0xC0000096 => "STATUS_PRIVILEGED_INSTRUCTION (user-mode executed a kernel-only instruction)",
        0xC00000F6 => "STATUS_INVALID_WORKING_SET (working set limit issue)",
        0xC0000142 => "STATUS_DLL_INIT_FAILED (a DLL failed to initialise)",
        0xC0000374 => "STATUS_HEAP_CORRUPTION",
        0xC0000409 => "STATUS_STACK_BUFFER_OVERRUN (__fastfail / GS cookie failure)",
        0xC000041D => "STATUS_FATAL_USER_CALLBACK_EXCEPTION (exception during a user callback)",
        0xC0000420 => "STATUS_ASSERTION_FAILURE",
        0x80000003 => "STATUS_BREAKPOINT (debug breakpoint / __debugbreak())",
        0x80000004 => "STATUS_SINGLE_STEP (single-step trap)",
        _ => "UNKNOWN (see ntstatus.h)",
    }
}
