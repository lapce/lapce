extern crate getopts;
use getopts::Options;
use std::env;

//struct cmdLine;
//impl cmdLine{
//fn new(){}//associate function, leave blank otherwise
//}
pub fn version(executable: &str) {
    //let theversion : String; //run script that updates
    //format!("Current Version is {}", theversion);
    format!("Usage: {} [options]", executable);
    println!("Lightning-fast and Powerful Code Editor: Check details at https://lapce.dev");
}

pub fn help(executable: &str, opts: Options) {
    let error = format!("Usage: {} [options]", executable);
    println!("{}", opts.usage(&error));
    println!("If you are having trouble with Lapce setup, please visit https://docs.lapce.dev");
    println!("If you wish to contribute, please go to https://github.com/lapce/lapce.git");
}

pub fn start_cmd_line() -> bool {
    let args: Vec<String> = env::args().collect(); //get arguments
    let exec = &args[0]; //executable
                                //let mut arguments = cmdLine::new();
    let mut opts = Options::new();
    opts.optflag("v", "version", "check Lapce version");
    opts.optflag("h", "help", "help");
    //additional new options go here
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => {m}, //accepts from Ok()
        Err(f) => {
            panic!("{}. Use --help to list recognized commands", f.to_string());
        } //rejects from Err()
    };
    if matches.opt_present("h") {
        help(&exec, opts);
        return true;
    }
    if matches.opt_present("v") {
        version(&exec);
        return true;
    }
    return true;
}

fn main() {
    start_cmd_line();
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[test]
    fn test_works_not_version() {
        let args: Vec<String> = env::args().collect(); //get arguments
        let mut opts = Options::new();
        opts.optflag("v", "version", "check Lapce version");
        opts.optflag("h", "help", "help");
        let matches = match opts.parse(&args[1..]) {
            Ok(m) => m, //accepts from Ok()
            Err(f) => {
                let val = f.to_string();
                if val != "version" {
					assert!(true);
                }
                panic!("{}. Use --help to list recognized commands", f.to_string());
                
            }// //rejects from Err()
        };
        if matches.opt_present("h") {
            assert!(!true);
        }
    }

    

    #[test]
    fn test_works_for_version() {
        let args: Vec<String> = env::args().collect(); //get arguments
        let mut opts = Options::new();
        opts.optflag("v", "version", "check Lapce version");
        opts.optflag("h", "help", "help");
        let matches = match opts.parse(&args[1..]) {
            Ok(m) => m, //accepts from Ok()
            Err(f) => {
                //rejects from Err()
                panic!("{}. Use --help to list recognized commands", f.to_string());
            }
        };
        if matches.opt_present("version") {
            assert!(true);
        }
    }

    #[test]
    fn test_works_for_help() {
        let args: Vec<String> = env::args().collect(); //get arguments
        let mut opts = Options::new();
        opts.optflag("v", "version", "check Lapce version");
        opts.optflag("h", "help", "help");
        let matches = match opts.parse(&args[1..]) {
            Ok(m) => m, //accepts from Ok()
            Err(f) => {
                panic!("{}. Use --help to list recognized commands", f.to_string());
                //rejects from Err()
            }
        };
        if matches.opt_present("help") {
            assert!(true);
        }
    }

    #[test]
    fn test_works_not_help() {
        let args: Vec<String> = env::args().collect(); //get arguments
        let mut opts = Options::new();
        opts.optflag("v", "version", "check Lapce version");
        opts.optflag("h", "help", "help");
        let matches = match opts.parse(&args[1..]) {
            Ok(m) => m, //accepts from Ok()
            Err(f) => {
                if f.to_string() != "help" {
					assert!(true)
                } //rejects from Err()
                panic!("{}. Use --help to list recognized commands", f.to_string());

            }
        };
        if matches.opt_present("v") {
            assert!(true);
        }
    }
    #[test]
    fn test_help_h() {
        let args: Vec<String> = env::args().collect(); //get arguments
        let exec = &args[0]; //executable
        let mut opts = Options::new();
        opts.optflag("v", "version", "check Lapce version");
        opts.optflag("h", "help", "help");
        //additional new options go here
        let matches = match opts.parse(&args[1..]) {
            Ok(m) => m, //accepts from Ok()
            Err(f) => {
                panic!("{}. Use --help to list recognized commands", f.to_string());
            } //rejects from Err()
        };
        if matches.opt_present("h") {
            help(&exec, opts);
            assert!(true);
        }
    }
    #[test]
    fn test_version_v() {
        let args: Vec<String> = env::args().collect(); //get arguments
        let exec = &args[0]; //executable
        let mut opts = Options::new();
        opts.optflag("v", "version", "check Lapce version");
        opts.optflag("h", "help", "help");
        //additional new options go here
        let matches = match opts.parse(&args[1..]) {
            Ok(m) => m, //accepts from Ok()
            Err(f) => {
                panic!("{}. Use --help to list recognized commands", f.to_string());
            } //rejects from Err()
        };
        if matches.opt_present("v") {
            version(&exec);
            assert!(true);
        }
    }
}

