// Build as a GUI-subsystem app so launching the tray doesn't pop a console
// window. The CLI modes (--once/--diag) re-attach to the parent console below
// so their stdout still reaches the terminal.
#![windows_subsystem = "windows"]

use logitray::app;

/// Re-attach stdout/stderr to the launching terminal's console (if any) so the
/// CLI debug modes remain usable despite the "windows" subsystem. No-op when
/// launched without a parent console (e.g. from Explorer / autostart).
#[cfg(windows)]
fn attach_parent_console() {
    // kernel32 is always linked on Windows; declare the one call we need.
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(dw_process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF; // (DWORD)-1
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn should_attach_parent_console(
    cli_mode: bool,
    has_explicit_std_handles: bool,
    has_stdout_handle: bool,
) -> bool {
    cli_mode && !has_explicit_std_handles && !has_stdout_handle
}

#[cfg(windows)]
fn has_stdout_handle() -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(n_std_handle: u32) -> *mut std::ffi::c_void;
    }
    const STD_OUTPUT_HANDLE: u32 = (-11i32) as u32;
    const INVALID_HANDLE_VALUE: isize = -1;
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        !handle.is_null() && handle as isize != INVALID_HANDLE_VALUE
    }
}

#[cfg(windows)]
fn has_explicit_std_handles() -> bool {
    #[repr(C)]
    struct StartupInfoW {
        cb: u32,
        reserved: *mut u16,
        desktop: *mut u16,
        title: *mut u16,
        x: u32,
        y: u32,
        x_size: u32,
        y_size: u32,
        x_count_chars: u32,
        y_count_chars: u32,
        fill_attribute: u32,
        flags: u32,
        show_window: u16,
        reserved2_bytes: u16,
        reserved2: *mut u8,
        stdin: *mut std::ffi::c_void,
        stdout: *mut std::ffi::c_void,
        stderr: *mut std::ffi::c_void,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetStartupInfoW(startup_info: *mut StartupInfoW);
    }
    const STARTF_USESTDHANDLES: u32 = 0x0000_0100;
    unsafe {
        let mut info: StartupInfoW = std::mem::zeroed();
        info.cb = std::mem::size_of::<StartupInfoW>() as u32;
        GetStartupInfoW(&mut info);
        (info.flags & STARTF_USESTDHANDLES) != 0
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let cli_mode = args.iter().any(|a| a == "--once" || a == "--diag");

    #[cfg(windows)]
    if should_attach_parent_console(cli_mode, has_explicit_std_handles(), has_stdout_handle()) {
        attach_parent_console();
    }

    let result = if args.iter().any(|a| a == "--diag") {
        logitray::hid::diag::run_diag();
        Ok(())
    } else if args.iter().any(|a| a == "--once") {
        app::run_once()
    } else {
        app::run_tray()
    };

    if let Err(err) = result {
        eprintln!("logitray error: {err:#}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::should_attach_parent_console;

    #[test]
    fn redirected_cli_output_must_keep_the_existing_pipe() {
        assert!(!should_attach_parent_console(true, true, false));
    }

    #[test]
    fn explorer_cli_launch_may_attach_to_parent_console() {
        assert!(should_attach_parent_console(true, false, false));
    }

    #[test]
    fn tray_mode_never_attaches_a_console() {
        assert!(!should_attach_parent_console(false, false, false));
    }
}
