#[cfg(all(target_os = "vita", debug_assertions))]
lazy_static::lazy_static! {
    pub static ref LOG_FILE: std::sync::Mutex<std::fs::File> = {
        let _ = std::fs::create_dir(crate::presenter::LOG_PATH);
        std::sync::Mutex::new(std::fs::File::create(crate::presenter::LOG_FILE).unwrap())
    };
}


#[cfg(target_os = "vita")]
const STABILITY_LOG_FILE: &str = "ux0:data/dsvita/log/stability.log";

/// Best-effort, low-frequency diagnostics for crashes and emergency exits.
#[cfg(target_os = "vita")]
pub fn stability_event(message: &str) {
    use std::io::Write;

    let _ = std::fs::create_dir_all(crate::presenter::LOG_PATH);
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(STABILITY_LOG_FILE) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let current = std::thread::current();
        let thread_name = current.name().unwrap_or("<unnamed>");
        let _ = writeln!(file, "[{stamp}] [{thread_name}] {message}");
        let _ = file.flush();
    }
}

/// Install a release-safe Vita panic hook. The release profile aborts, but Rust still
/// invokes the panic hook first, giving us a chance to persist the failure reason.
#[cfg(target_os = "vita")]
pub fn install_stability_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let message = if let Some(s) = info.payload().downcast_ref::<&'static str>() {
            *s
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.as_str()
        } else {
            "<non-string panic payload>"
        };
        stability_event(&format!("PANIC at {location}: {message}"));
        default_hook(info);
    }));
}

macro_rules! debug_println {
    ($($args:tt)*) => {
        if crate::DEBUG_LOG {
            let log = format!($($args)*);
            // Interleave into the binary instruction log (when enabled) so debug lines line up with
            // the per-instruction register snapshots; otherwise print.
            if crate::debug_inst_log::is_logging() {
                crate::debug_inst_log::log_text(&log);
            } else {
                let current_thread = std::thread::current();
                let thread_name = current_thread.name().unwrap();
                println!("[{}] {}", thread_name, log);
            }
        }
    };
}

pub(crate) use debug_println;

macro_rules! info_println {
    ($($args:tt)*) => {
        let log = format!($($args)*);
        let current_thread = std::thread::current();
        let thread_name = current_thread.name().unwrap();
        let value = format!("[{}] {}", thread_name, log);
        println!("{value}");
        #[cfg(all(target_os = "vita", debug_assertions))]
        {
            let mut log_file = crate::logging::LOG_FILE.lock().unwrap();
            std::io::Write::write(&mut *log_file, value.as_bytes()).unwrap();
            std::io::Write::write_all(&mut *log_file, "\n".as_bytes()).unwrap();
        }
    };
}
pub(crate) use info_println;

macro_rules! branch_println {
    ($($args:tt)*) => {
        if crate::BRANCH_LOG {
            let log = format!($($args)*);
            // Interleave branch decisions into the binary instruction log (when enabled) so control
            // flow can be diffed alongside the per-instruction register snapshots; otherwise print.
            if crate::debug_inst_log::is_logging() {
                crate::debug_inst_log::log_text(&log);
            } else {
                let current_thread = std::thread::current();
                let thread_name = current_thread.name().unwrap();
                println!("[{}] {}", thread_name, log);
            }
        }
    };
}
pub(crate) use branch_println;

macro_rules! block_asm_print {
    ($($args:tt)*) => {
        if crate::DEBUG_LOG {
            if crate::debug_inst_log::is_logging() {
                crate::debug_inst_log::log_text_no_newline(&format!($($args)*));
            } else {
                print!($($args)*);
            }
        }
    };
}
pub(crate) use block_asm_print;

macro_rules! block_asm_println {
    ($($args:tt)*) => {
        if crate::DEBUG_LOG {
            if crate::debug_inst_log::is_logging() {
                crate::debug_inst_log::log_text(&format!($($args)*));
            } else {
                println!($($args)*);
            }
        }
    };
}
pub(crate) use block_asm_println;

macro_rules! debug_panic {
    ($($args:tt)*) => {
        if crate::IS_DEBUG {
            panic!($($args)*)
        } else {
            unsafe { std::hint::unreachable_unchecked() }
        }
    };
}
pub(crate) use debug_panic;
