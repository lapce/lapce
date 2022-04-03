extern crate getopts;
use getopts::Options;
use std::env;

pub fn version(executable: &str){
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
    let args: Vec<String> = env::args().collect();//get arguments
    let exec = args[0].clone();//executable
    let mut opts = Options::new();
    opts.optflag("v", "version", "check Lapce version");
    opts.optflag("h", "help", "help");
    //additional new options go here
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => { m }//accepts from Ok()
        Err(f) => {panic!("{}. Use --help to list recognized commands", f.to_string()); return false;}//rejects from Err()
    };
    if matches.opt_present("h") {
        help(&exec, opts);
        return true;
    }
    if matches.opt_present("v"){
        version(&exec);
        return true;
    }
    return true;
}


#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[test]
    fn test_stub(){}
    fn test_general() {
		let mut a = cmdLine::new();
        let mut b = cmdLine::new();
        let mut c = cmdLine::new();
        a=b;
        b=c;
        c=a;
        assert_eq!(a, b);
        assert_eq!(a,c);
        assert_eq!(b,c);     
   }
    fn test_works_not_version(){
    	let args: Vec<String> = env::args().collect();//get arguments
    	let exec = args[0].clone();//executable
    	//let mut arguments = cmdLine::new();
    	let mut opts = Options::new();
	    opts.optflag("v", "version", "check Lapce version");
    	opts.optflag("h", "help", "help");
	    let matches = match opts.parse(&args[1..]) {
        Ok(m) => { m }//accepts from Ok()
        Err(f) => {
		if(f.to_string() != "version".to_string){
			panic!("{}. Use --help to list recognized commands", f.to_string());}//rejects from Err()
			assert!(true)
		}
		
        };
    }
	
	fn test_works_for_version(){
    	let args: Vec<String> = env::args().collect();//get arguments
    	let exec = args[0].clone();//executable
    	//let mut arguments = cmdLine::new();
    	let mut opts = Options::new();
	    opts.optflag("v", "version", "check Lapce version");
    	opts.optflag("h", "help", "help");
	    let matches = match opts.parse(&args[1..]) {
        Ok(m) => { m }//accepts from Ok()
        Err(f) => {
		if(f.to_string() != "version".to_string){
			panic!("{}. Use --help to list recognized commands", f.to_string());}//rejects from Err()
		}
        };
		assert!(true)
    }
	
		fn test_works_for_help(){
    	let args: Vec<String> = env::args().collect();//get arguments
    	let exec = args[0].clone();//executable
    	//let mut arguments = cmdLine::new();
    	let mut opts = Options::new();
	    opts.optflag("v", "version", "check Lapce version");
    	opts.optflag("h", "help", "help");
	    let matches = match opts.parse(&args[1..]) {
        Ok(m) => { m }//accepts from Ok()
        Err(f) => {
		if(f.to_string() != "help".to_string){
			panic!("{}. Use --help to list recognized commands", f.to_string());}//rejects from Err()
		}
        };
		assert!(true)
    }
	
	
	    fn test_works_not_help(){
    	let args: Vec<String> = env::args().collect();//get arguments
    	let exec = args[0].clone();//executable
    	//let mut arguments = cmdLine::new();
    	let mut opts = Options::new();
	    opts.optflag("v", "version", "check Lapce version");
    	opts.optflag("h", "help", "help");
	    let matches = match opts.parse(&args[1..]) {
        Ok(m) => { m }//accepts from Ok()
        Err(f) => {
		if(f.to_string() != "help".to_string){
			panic!("{}. Use --help to list recognized commands", f.to_string());}//rejects from Err()
			assert!(true)
		}
		
        };
    }
	
	fn test_help(){
		let args: Vec<String> = env::args().collect();//get arguments
		let exec = args[0].clone();//executable
		//let mut arguments = cmdLine::new();
		let mut opts = Options::new();
		opts.optflag("v", "version", "check Lapce version");
		opts.optflag("h", "help", "help");
		//additional new options go here
		let matches = match opts.parse(&args[1..]) {
        Ok(m) => { m }//accepts from Ok()
        Err(f) => {panic!("{}. Use --help to list recognized commands", f.to_string());}//rejects from Err()
        };
		if matches.opt_present("h") {
        arguments.help(&exec, opts);
        assert!(true);
		}
	}
	
	fn test_version(){
		let args: Vec<String> = env::args().collect();//get arguments
		let exec = args[0].clone();//executable
		//let mut arguments = cmdLine::new();
		let mut opts = Options::new();
		opts.optflag("v", "version", "check Lapce version");
		opts.optflag("h", "help", "help");
		//additional new options go here
		let matches = match opts.parse(&args[1..]) {
        Ok(m) => { m }//accepts from Ok()
        Err(f) => {panic!("{}. Use --help to list recognized commands", f.to_string());}//rejects from Err()
        };
		if matches.opt_present("v") {
        arguments.version(&exec, opts);
		assert!(true);
		}
	}
}

