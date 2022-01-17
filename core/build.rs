use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

fn collect_tree_sitter_dirs(ignore: &[String]) -> Result<Vec<String>> {
    let mut dirs = Vec::new();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("languages");

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();

        if !entry.file_type()?.is_dir() {
            continue;
        }

        let dir = path.file_name().unwrap().to_str().unwrap().to_string();

        // filter ignores
        if ignore.contains(&dir) {
            continue;
        }
        dirs.push(dir)
    }

    Ok(dirs)
}

fn build_library(src_dir: &Path, language: &str) -> Result<()> {
    let mut config = cc::Build::new();
    config.include(&src_dir);
    config
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .flag_if_supported("-Wno-trigraphs");

    let parser_path = src_dir.join("parser.c");
    println!("cargo:rerun-if-changed={}", parser_path.to_str().unwrap());
    config.file(&parser_path);

    let scanner_path = src_dir.join("scanner.c");
    if scanner_path.exists() {
        println!("cargo:rerun-if-changed={}", scanner_path.to_str().unwrap());
        config.file(&scanner_path);
    }

    let scanner_path = src_dir.join("scanner.cc");
    if scanner_path.exists() {
        println!("cargo:rerun-if-changed={}", scanner_path.to_str().unwrap());
        config.file(&scanner_path);
        config.cpp(true);
    }

    config.out_dir(src_dir);
    config.compile(language);
    Ok(())
}

fn build_dir(dir: &str, language: &str) {
    println!("Build language {}", language);
    if PathBuf::from("languages")
        .join(dir)
        .read_dir()
        .unwrap()
        .next()
        .is_none()
    {
        eprintln!(
            "The directory {} is empty, you probably need to use 'git submodule update --init --recursive'?",
            dir
        );
        std::process::exit(1);
    }

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("languages")
        .join(dir)
        .join("src");

    build_library(&path, language).unwrap();
}

fn main() {
    let ignore = vec![
        "tree-sitter-typescript".to_string(),
        "tree-sitter-ocaml".to_string(),
    ];
    let dirs = collect_tree_sitter_dirs(&ignore).unwrap();

    for dir in dirs {
        let language = &dir.strip_prefix("tree-sitter-").unwrap();
        build_dir(&dir, language);
    }
}
