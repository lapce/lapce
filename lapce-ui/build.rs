#[cfg(windows)]
fn main() {
    let mut res = winres::WindowsResource::new();
    res.set("ProductName", "lapce");
    res.set("FileDescription", "lapce");
    res.set("LegalCopyright", "Copyright (C) 2022");
    res.set_icon("../extra/windows/lapce.ico");
    res.compile()
        .expect("Failed to run the Windows resource compiler (rc.exe)");
}

#[cfg(not(windows))]
fn main() {}
