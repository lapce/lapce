//! One-click setup — downloads GGUF models automatically.
//!
//! Called on first run or via `deepseek-carp setup`.
//! Downloads Qwen2.5-7B and Qwen2.5-Coder-7B from Hugging Face.
//! Qwen3.6-27B is offered as an optional larger model.

use std::path::{Path, PathBuf};

/// All models that can be auto-downloaded.
pub struct ModelInfo {
    pub name: &'static str,
    pub filename: &'static str,
    pub hf_repo: &'static str,
    pub size_gb: f64,
    pub required: bool,
    /// SHA256 hex digest for integrity verification (empty = skip check).
    pub sha256: &'static str,
}

/// Default models for setup.
pub fn default_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            name: "Qwen2.5-7B (Daily Assistant)",
            filename: "qwen2.5-7b-instruct-1m-q4_k_m.gguf",
            hf_repo: "Qwen/Qwen2.5-7B-Instruct-GGUF",
            size_gb: 4.5,
            required: true,
            sha256: "",
        },
        ModelInfo {
            name: "Qwen2.5-Coder-7B (Code Expert)",
            filename: "qwen2.5-coder-7b-instruct-q4_0.gguf",
            hf_repo: "Qwen/Qwen2.5-Coder-7B-Instruct-GGUF",
            size_gb: 3.8,
            required: true,
            sha256: "",
        },
        ModelInfo {
            name: "Qwen3.6-27B (Complex Reasoning)",
            filename: "Qwen3.6-27B-Q4_K_M.gguf",
            hf_repo: "Qwen/Qwen3-27B-Instruct-GGUF",
            size_gb: 15.7,
            required: false,
            sha256: "",
        },
    ]
}

/// Check if setup is needed (models directory exists with required models).
pub fn needs_setup(models_dir: &Path) -> bool {
    !models_dir.exists() || default_models().iter()
        .filter(|m| m.required)
        .any(|m| !models_dir.join(m.filename).exists())
}

/// Find or download llama-server.
async fn ensure_llama_server() -> anyhow::Result<()> {
    let name = if cfg!(target_os = "windows") { "llama-server.exe" } else { "llama-server" };

    // 1. Check current directory
    if std::path::Path::new(name).exists() {
        println!("✅ llama-server already present (current dir)");
        return Ok(());
    }

    // 2. Check PATH
    if let Ok(paths) = std::env::var("PATH") {
        for dir in std::env::split_paths(&paths) {
            if dir.join(name).exists() {
                println!("✅ llama-server already present (PATH: {})", dir.display());
                return Ok(());
            }
        }
    }

    // 3. Not found anywhere — skip download, warn user
    println!("⚠️  llama-server not found. Please place it next to deepseek-carp.exe:");
    println!("   Download from: https://github.com/ggml-org/llama.cpp/releases");
    println!("   Local models will be unavailable until installed.");
    Ok(())// Non-fatal: user can still use cloud APIs
}

/// Run the interactive setup wizard.
pub async fn run_setup(models_dir: &Path, force: bool) -> anyhow::Result<()> {
    std::fs::create_dir_all(models_dir)?;

    // ── Download llama-server first ──
    ensure_llama_server().await?;
    println!();

    let models = default_models();
    let required: Vec<_> = models.iter().filter(|m| m.required || force).collect();

    if required.is_empty() {
        println!("All models already present in {}", models_dir.display());
        return Ok(());
    }

    // Calculate total download size
    let total_gb: f64 = required.iter().map(|m| m.size_gb).sum();
    println!("╔══════════════════════════════════════════════╗");
    println!("║     DeepSeek Carp — One-Click Setup           ║");
    println!("╠══════════════════════════════════════════════╣");
    println!("║  Models to download:                         ║");
    for m in &required {
        println!("║    {} ({:.1}GB)   ║", m.name, m.size_gb);
    }
    println!("║  Total: ~{:.1}GB                               ║", total_gb);
    println!("╚══════════════════════════════════════════════╝");
    println!();

    for model in &required {
        let dest = models_dir.join(model.filename);
        if dest.exists() {
            println!("✅ {} — already downloaded", model.name);
            continue;
        }

        println!("📥 Downloading {} ({:.1}GB)...", model.name, model.size_gb);
        if let Err(e) = download_model(model, &dest).await {
            eprintln!("⚠️  Failed to download {}: {}", model.name, e);
            eprintln!("   You can manually download from:");
            eprintln!("   https://huggingface.co/{}", model.hf_repo);
            continue;
        }
        // Verify integrity if checksum is provided
        if !model.sha256.is_empty() {
            if let Err(e) = verify_checksum(&dest, model.sha256) {
                eprintln!("⚠️  Checksum FAILED for {}: {}", model.name, e);
                std::fs::remove_file(&dest).ok();
                eprintln!("   Corrupted download removed. Please re-run setup.");
                continue;
            }
            println!("✅ {} — download complete (SHA256 verified)", model.name);
        } else {
            println!("✅ {} — download complete", model.name);
        }
    }

    println!();
    println!("Setup complete! Start with: deepseek-carp chat");

    Ok(())
}

/// Download a single GGUF model from Hugging Face with progress display.
async fn download_model(model: &ModelInfo, dest: &PathBuf) -> anyhow::Result<()> {
    // Hugging Face direct download URL
    let url = format!(
        "https://huggingface.co/{}/resolve/main/{}?download=true",
        model.hf_repo, model.filename
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3600))
        .build()?;

    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} downloading {}", resp.status(), url);
    }

    let total_size = resp.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut file = std::fs::File::create(dest)?;
    let mut last_report = std::time::Instant::now();

    use futures::StreamExt;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk?;
        std::io::Write::write_all(&mut file, &bytes)?;
        downloaded += bytes.len() as u64;

        // Progress report every 2 seconds
        if last_report.elapsed().as_secs() >= 2 {
            if total_size > 0 {
                let pct = (downloaded as f64 / total_size as f64) * 100.0;
                let mb = downloaded as f64 / (1024.0 * 1024.0);
                print!("\r   {:.1}% ({:.0}MB / {:.0}MB)", pct, mb, total_size as f64 / (1024.0 * 1024.0));
            } else {
                let mb = downloaded as f64 / (1024.0 * 1024.0);
                print!("\r   {:.0}MB downloaded", mb);
            }
            use std::io::Write;
            let _ = std::io::stdout().flush();
            last_report = std::time::Instant::now();
        }
    }
    println!(); // Newline after download completes

    Ok(())
}

/// Verify file SHA256 against expected hex digest.
fn verify_checksum(path: &Path, expected: &str) -> anyhow::Result<()> {
    use sha2::Digest;
    let data = std::fs::read(path)?;
    let mut hasher = sha2::Sha256::new();
    sha2::Digest::update(&mut hasher, &data);
    let actual = hex::encode(hasher.finalize());
    if actual != expected {
        anyhow::bail!("checksum mismatch: expected {}, got {}", expected, actual);
    }
    Ok(())
}
