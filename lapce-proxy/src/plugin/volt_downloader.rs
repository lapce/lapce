use std::{fs, path::PathBuf, io::Write};

use lapce_rpc::{core::CoreRpcHandler, plugin::{VoltMetadata, VoltInfo}};

use crate::{anyhow, Result};

pub struct VoltDownloader {
    rpc_handler: CoreRpcHandler,
    meta_str: String,
    meta: VoltMetadata
}

impl VoltDownloader {
    pub fn new(rpc_handler: CoreRpcHandler, volt: VoltInfo) -> Self {
        let meta_str = reqwest::blocking::get(&volt.meta).unwrap().text().unwrap();
        let meta: VoltMetadata = toml_edit::easy::from_str(&meta_str).unwrap();
        Self {
            rpc_handler,
            meta_str,
            meta
        }
    }

    pub fn init(&self, volt_id: &str, wasm: bool) -> Result<PathBuf>{

        // Call the progress function with 0%
        self.rpc_handler.volt_installing(self.meta.clone(), 0.0);
    
        if self.meta.wasm.is_some() != wasm {
            return Err(anyhow!("plugin type not fit"));
        }
    
        let path = crate::directory::Directory::plugins_directory()
            .ok_or_else(|| anyhow!("can't get plugin directory"))?
            .join(&volt_id);
        let _ = std::fs::remove_dir_all(&path);

        // return plugins_directory_path
        Ok(path)
    }

    pub fn write_volt_file(&self, path: PathBuf) -> Result<PathBuf> {
        let thread_test = std::thread::spawn(move || -> Result<PathBuf> {
            fs::create_dir_all(&path)?;
            let meta_path = path.join("volt.toml");
            {
                let mut file = fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(&meta_path)?;
                file.write_all(self.meta_str.as_bytes())?;
            }
            Ok(meta_path)
        }); 
        self.rpc_handler.volt_installing(self.meta.clone(), 20.0);
        let return_val = thread_test.join().unwrap();
        return return_val;
    }

    pub fn write_wasm(&self, volt_meta: &str, path: PathBuf) -> Result<lsp_types::Url>{
        let url = url::Url::parse(volt_meta)?;
        if let Some(wasm) = self.meta.wasm.as_ref() {
            let url = url.join(wasm)?;
            {
                let mut resp = reqwest::blocking::get(url)?;
                if let Some(path) = path.join(&wasm).parent() {
                    if !path.exists() {
                        fs::DirBuilder::new().recursive(true).create(path)?;
                    }
                }
                let mut file = fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(path.join(&wasm))?;
                std::io::copy(&mut resp, &mut file)?;
            }
        }
        self.rpc_handler.volt_installing(self.meta.clone(), 40.0);

        Ok(url)
    }

    pub fn write_themes(&self, path: PathBuf, url: lsp_types::Url) -> Result<()> {
        if let Some(themes) = self.meta.themes.as_ref() {
            for theme in themes {
                let url = url.join(theme)?;
                {
                    let mut resp = reqwest::blocking::get(url)?;
                    let mut file = std::fs::OpenOptions::new()
                        .create(true)
                        .truncate(true)
                        .write(true)
                        .open(path.join(&theme))?;
                    std::io::copy(&mut resp, &mut file)?;
                }
            }
        }
        self.rpc_handler.volt_installing(self.meta.clone(), 60.0);
        Ok(())
    }

    pub fn load_volt(&self, meta_path: PathBuf) -> Result<VoltMetadata> {
        self.rpc_handler.volt_installing(self.meta.clone(), 80.0);
        super::wasi::load_volt(&meta_path)
        
    }
}