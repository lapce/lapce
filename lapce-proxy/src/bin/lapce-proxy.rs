use lapce_proxy::{mainloop, VERSION};

fn main() {
    let mut args = std::env::args();
    // TODO(panekj): implement opening files via proxy (IPC support) (#795)
    if args.len() > 1 {
        args.next();
        for arg in args {
            match arg.as_str() {
                "-v" | "--version" => {
                    println!("lapce-proxy {VERSION}");
                    return;
                }
                "-h" | "--help" => {
                    println!("lapce [-h|--help] [-v|--version]");
                    return;
                }
                v => {
                    eprintln!("lapce: unrecognized option: {v}");
                    std::process::exit(1)
                }
            }
        }
    }

    mainloop();
}
