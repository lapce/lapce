#![windows_subsystem = "windows"]
use lapce_ui::app;

pub fn main() {
    app::launch();
}

#[cfg(windows)]
use windows_sys::{core::PWSTR, Win32::Foundation::HINSTANCE};

#[no_mangle]
#[cfg(windows)]
#[allow(non_snake_case)]
pub extern "system" fn WinMain(_: HINSTANCE, _: HINSTANCE, _: PWSTR, _: i32) -> i32 {
    // use std::{ffi::OsString, os::windows::prelude::OsStringExt, slice::from_raw_parts};

    // use windows_sys::Win32::{System::Environment::GetCommandLineW, UI::Shell::CommandLineToArgvW, System::WindowsProgramming::uaw_wcslen};

    // let lpcmdline = unsafe { GetCommandLineW() };

    // let pnumargs = std::ptr::null_mut();
    // let szArglist = unsafe { CommandLineToArgvW(lpcmdline, pnumargs) };
    // if szArglist.is_null() {
    //     return 1;
    // }

    // let args = unsafe {
    //     from_raw_parts(szArglist, dbg!(pnumargs as usize))
    // };
    // for arg in dbg!(args) {
    //     let arg = arg.clone();
    //     let len = unsafe { uaw_wcslen(arg.clone() as _) };
    //     let arg = unsafe { from_raw_parts(arg, len) };
    //     _ = dbg!(OsString::from_wide(arg));
    // }

    main();

    return 0;
}
