use fern::Dispatch;
use log::LevelFilter;

fn apply(dispatch: Dispatch, module: Option<&str>, level: LevelFilter) -> Dispatch {
    if let Some(module) = module {
        dispatch.level_for(module.to_string(), level)
    } else {
        dispatch.level(level)
    }
}

fn set_level(dispatch: Dispatch, module: Option<&str>, level: &str) -> Dispatch {
    use LevelFilter::*;
    match level {
        "off" => apply(dispatch, module, Off),
        "error" => apply(dispatch, module, Error),
        "warn" => apply(dispatch, module, Warn),
        "info" => apply(dispatch, module, Info),
        "debug" => apply(dispatch, module, Debug),
        "trace" => apply(dispatch, module, Trace),
        val => {
            eprint!("RUST_LOG: ");
            if let Some(module) = module {
                eprint!("module '{module}' ");
            }
            eprintln!("ignored unknown log level: '{val}'");
            dispatch
        }
    }
}

fn parse_log_levels(value: &str, mut dispatch: fern::Dispatch) -> fern::Dispatch {
    println!("Parsing RUST_LOG");

    // To set the threshold at Error for all modules: RUST_LOG=error
    //
    // To set the threshold at Info for all but 'module1':
    // RUST_LOG=info,path::to::module1=off
    //
    // To set the threshold at Trace for 'module1' and keep the rest at Off:
    // RUST_LOG=path::to::module1=trace
    //
    // This would set the threshold at Info for all: RUST_LOG=error,info
    //
    // This sets the threshold at Error for all modules but 'module1' and
    // 'module2' which are at Info and Debug, respectively:
    // RUST_LOG="error,path::to::module1=info,path::to::module2=debug"
    for section in value.split(',').filter(|s| !s.is_empty()) {
        if let Some((module, level)) = section.split_once('=') {
            println!("module='{module}', level='{level}'");
            // "module=level"
            //
            // NOTE: The dash characters in crate names are converted into
            // underscores by the compiler.  For example, path to this
            // module will be "lapce_ui::loggings".
            dispatch = set_level(dispatch, Some(module), level);
        } else {
            println!("level='{section}' for all modules");
            // just "level"
            dispatch = set_level(dispatch, None, section);
        }
    }
    dispatch
}

pub(super) fn override_log_levels(dispatch: Dispatch) -> Dispatch {
    match std::env::var("RUST_LOG") {
        // Not an error if the env var does not exist.
        Err(std::env::VarError::NotPresent) => dispatch,
        Err(std::env::VarError::NotUnicode(val)) => {
            let val = val.to_string_lossy();
            eprintln!("RUST_LOG: ignored invalid unicode value: '{val}'");
            dispatch
        }
        Ok(val) => parse_log_levels(&val, dispatch),
    }
}
