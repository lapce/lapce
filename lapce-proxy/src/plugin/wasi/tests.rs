use std::collections::HashMap;

use lapce_rpc::plugin::VoltMetadata;
use serde_json::{Value, json};

use super::{load_volt, unflatten_map};

#[test]
fn test_unflatten_map() {
    let map: HashMap<String, Value> = serde_json::from_value(json!({
        "a.b.c": "d",
        "a.d": ["e"],
    }))
    .unwrap();
    assert_eq!(
        unflatten_map(&map),
        json!({
            "a": {
                "b": {
                    "c": "d",
                },
                "d": ["e"],
            }
        })
    );
}

#[test]
fn test_load_volt() {
    let lapce_proxy_dir = std::env::current_dir()
        .expect("Can't get \"lapce-proxy\" directory")
        .join("src")
        .join("plugin")
        .join("wasi")
        .join("plugins");

    // Invalid path (file does not exist)
    let path = lapce_proxy_dir.join("some-path");
    match path.canonicalize() {
        Ok(path) => panic!("{path:?} file must not exast, but it is"),
        Err(err) => assert_eq!(err.kind(), std::io::ErrorKind::NotFound),
    };
    // This should return Err since the file does not exist
    if let Ok(volt_metadata) = load_volt(&lapce_proxy_dir) {
        panic!(
            "Unexpected result from `lapce_proxy::plugin::wasi::load_volt` function: {volt_metadata:?}"
        );
    }

    // Invalid file (not readable into a string)
    // Making sure the file exists
    let path = lapce_proxy_dir.join("smiley.png");
    let path = match path.canonicalize() {
        Ok(path) => path,
        Err(err) => panic!("{path:?} file must exast, but: {err:?}"),
    };
    // Making sure the data in the file is invalid utf-8
    match std::fs::read_to_string(path.clone()) {
        Ok(str) => panic!(
            "{path:?} file must be invalid utf-8, but it is valid utf-8: {str:?}",
        ),
        Err(err) => assert_eq!(err.kind(), std::io::ErrorKind::InvalidData),
    }
    // This should return Err since the `*.png` file cannot be read as a String
    if let Ok(volt_metadata) = load_volt(&path) {
        panic!(
            "Unexpected result from `lapce_proxy::plugin::wasi::load_volt` function: {volt_metadata:?}",
        );
    }

    // Invalid data in file (cannot be read as VoltMetadata)
    // Making sure the file exists
    let path = lapce_proxy_dir
        .join("some_author.test-plugin-one")
        .join("Light.svg");
    let path = match path.canonicalize() {
        Ok(path) => path,
        Err(err) => panic!("{path:?} file must exast, but: {err:?}"),
    };
    // Making sure the data in the file is valid utf-8 (*.svg file is must be a valid utf-8)
    match std::fs::read_to_string(path.clone()) {
        Ok(_) => {}
        Err(err) => panic!("{path:?} file must be valid utf-8, but {err:?}"),
    }
    // This should return Err since the data in the file cannot be interpreted as VoltMetadata
    if let Ok(volt_metadata) = load_volt(&path) {
        panic!(
            "Unexpected result from `lapce_proxy::plugin::wasi::load_volt` function: {volt_metadata:?}",
        );
    }

    let parent_path = lapce_proxy_dir.join("some_author.test-plugin-one");

    let volt_metadata = match load_volt(&parent_path) {
        Ok(volt_metadata) => volt_metadata,
        Err(error) => panic!("{}", error),
    };

    let wasm_path = parent_path
        .join("lapce.wasm")
        .canonicalize()
        .ok()
        .as_ref()
        .and_then(|path| path.to_str())
        .map(ToOwned::to_owned);

    let color_themes_pathes = ["Dark.toml", "Light.toml"]
        .into_iter()
        .filter_map(|theme| {
            parent_path
                .join(theme)
                .canonicalize()
                .ok()
                .as_ref()
                .and_then(|path| path.to_str())
                .map(ToOwned::to_owned)
        })
        .collect::<Vec<_>>();

    let icon_themes_pathes = ["Dark.svg", "Light.svg"]
        .into_iter()
        .filter_map(|theme| {
            parent_path
                .join(theme)
                .canonicalize()
                .ok()
                .as_ref()
                .and_then(|path| path.to_str())
                .map(ToOwned::to_owned)
        })
        .collect::<Vec<_>>();

    assert_eq!(
        volt_metadata,
        VoltMetadata {
            name: "some-useful-plugin".to_string(),
            version: "0.1.56".to_string(),
            display_name: "Some Useful Plugin Name".to_string(),
            author: "some_author".to_string(),
            description: "very useful plugin".to_string(),
            icon: Some("icon.svg".to_string()),
            repository: Some("https://github.com/lapce".to_string()),
            wasm: wasm_path,
            color_themes: Some(color_themes_pathes),
            icon_themes: Some(icon_themes_pathes),
            dir: parent_path.canonicalize().ok(),
            activation: None,
            config: None
        }
    );

    let parent_path = lapce_proxy_dir.join("some_author.test-plugin-two");

    let volt_metadata = match load_volt(&parent_path) {
        Ok(volt_metadata) => volt_metadata,
        Err(error) => panic!("{}", error),
    };

    let wasm_path = parent_path
        .join("lapce.wasm")
        .canonicalize()
        .ok()
        .as_ref()
        .and_then(|path| path.to_str())
        .map(ToOwned::to_owned);

    let color_themes_pathes = ["Light.toml"]
        .into_iter()
        .filter_map(|theme| {
            parent_path
                .join(theme)
                .canonicalize()
                .ok()
                .as_ref()
                .and_then(|path| path.to_str())
                .map(ToOwned::to_owned)
        })
        .collect::<Vec<_>>();

    let icon_themes_pathes = ["Light.svg"]
        .into_iter()
        .filter_map(|theme| {
            parent_path
                .join(theme)
                .canonicalize()
                .ok()
                .as_ref()
                .and_then(|path| path.to_str())
                .map(ToOwned::to_owned)
        })
        .collect::<Vec<_>>();

    assert_eq!(
        volt_metadata,
        VoltMetadata {
            name: "some-useful-plugin".to_string(),
            version: "0.1.56".to_string(),
            display_name: "Some Useful Plugin Name".to_string(),
            author: "some_author.".to_string(),
            description: "very useful plugin".to_string(),
            icon: Some("icon.svg".to_string()),
            repository: Some("https://github.com/lapce".to_string()),
            wasm: wasm_path,
            color_themes: Some(color_themes_pathes),
            icon_themes: Some(icon_themes_pathes),
            dir: parent_path.canonicalize().ok(),
            activation: None,
            config: None
        }
    );

    let parent_path = lapce_proxy_dir.join("some_author.test-plugin-three");

    let volt_metadata = match load_volt(&parent_path) {
        Ok(volt_metadata) => volt_metadata,
        Err(error) => panic!("{}", error),
    };

    assert_eq!(
        volt_metadata,
        VoltMetadata {
            name: "some-useful-plugin".to_string(),
            version: "0.1.56".to_string(),
            display_name: "Some Useful Plugin Name".to_string(),
            author: "some_author".to_string(),
            description: "very useful plugin".to_string(),
            icon: Some("icon.svg".to_string()),
            repository: Some("https://github.com/lapce".to_string()),
            wasm: None,
            color_themes: Some(Vec::new()),
            icon_themes: Some(Vec::new()),
            dir: parent_path.canonicalize().ok(),
            activation: None,
            config: None
        }
    );
}
