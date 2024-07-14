use std::env;

use directory::Directory;
use tracing::{
    appender::non_blocking::WorkerGuard,
    level_filters::LevelFilter,
    subscriber::{
        filter::Targets,
        fmt::{subscriber as fmt_subscriber, Subscriber as FmtSubscriber},
        registry,
        reload::{Handle as ReloadHandle, Subscriber as ReloadSubscriber},
        subscribe::CollectExt,
        util::SubscriberInitExt,
        Subscribe,
    },
    trace, TraceLevel,
};

#[inline(always)]
pub(super) fn logging() -> (ReloadHandle<Targets>, Option<WorkerGuard>) {
    let (log_file, guard) = match Directory::logs_directory()
        .and_then(|dir| {
            Ok(tracing::appender::rolling::Builder::new()
                .max_log_files(10)
                .rotation(tracing::appender::rolling::Rotation::DAILY)
                .filename_prefix("lapce")
                .filename_suffix("log")
                .build(dir)?)
        })
        .map(tracing::appender::non_blocking)
    {
        Ok((log_file, guard)) => (Some(log_file), Some(guard)),
        Err(e) => panic!("Failed to obtain logs directory: {e}"),
    };

    let log_file_filter_targets = Targets::new()
        .with_target("lapce_app", LevelFilter::DEBUG)
        .with_target("lapce_proxy", LevelFilter::DEBUG)
        .with_target("lapce_core", LevelFilter::DEBUG)
        .with_default(LevelFilter::from_level(TraceLevel::INFO));
    let (log_file_filter, reload_handle) =
        ReloadSubscriber::new(log_file_filter_targets.clone());

    let console_filter_targets = env::var("LAPCE_LOG")
        .unwrap_or_default()
        .parse::<Targets>()
        .unwrap_or(log_file_filter_targets);

    let registry = registry();
    if let Some(log_file) = log_file {
        let file_layer = fmt_subscriber()
            .with_ansi(false)
            .with_writer(log_file)
            .with_filter(log_file_filter);
        registry
            .with(file_layer)
            .with(
                FmtSubscriber::default()
                    .with_line_number(true)
                    .with_target(true)
                    .with_filter(console_filter_targets),
            )
            .init();
    } else {
        registry
            .with(FmtSubscriber::default().with_filter(console_filter_targets))
            .init();
    };

    (reload_handle, guard)
}

pub(super) fn panic_hook() {
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("main");
        let backtrace = backtrace::Backtrace::new();

        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .unwrap_or(&"<unknown>");

        match info.location() {
            Some(loc) => {
                trace!(
                    target: "lapce_app::panic_hook",
                    TraceLevel::ERROR,
                    "thread {thread} panicked at {} | file://./{}:{}:{}\n{:?}",
                    payload,
                    loc.file(), loc.line(), loc.column(),
                    backtrace,
                );
            }
            None => {
                trace!(
                    target: "lapce_app::panic_hook",
                    TraceLevel::ERROR,
                    "thread {thread} panicked at {}\n{:?}",
                    payload,
                    backtrace,
                );
            }
        }

        #[cfg(windows)]
        error_modal("Error", &info.to_string());
    }))
}

#[cfg(windows)]
pub(super) fn error_modal(title: &str, msg: &str) -> i32 {
    use std::{ffi::OsStr, iter::once, mem, os::windows::prelude::OsStrExt};

    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_SYSTEMMODAL,
    };

    let result: i32;

    let title = OsStr::new(title)
        .encode_wide()
        .chain(once(0u16))
        .collect::<Vec<u16>>();
    let msg = OsStr::new(msg)
        .encode_wide()
        .chain(once(0u16))
        .collect::<Vec<u16>>();
    unsafe {
        result = MessageBoxW(
            mem::zeroed(),
            msg.as_ptr(),
            title.as_ptr(),
            MB_ICONERROR | MB_SYSTEMMODAL,
        );
    }

    result
}
