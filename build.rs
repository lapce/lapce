fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=extra/windows/lapce.ico");

    if std::env::var("CARGO_CFG_TARGET_OS") == Ok("windows".into()) {
        winresource::WindowsResource::new()
            .set_icon("extra/windows/lapce.ico")
            .compile()?;
    }
    Ok(())
}
