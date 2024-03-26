use crate::command_to_string;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NotaryToolResponse {
    notarization_upload: Option<NotarizationUpload>,
    notarization_info: Option<NotarizationInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NotarizationUpload {
    request_uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NotarizationInfo {
    status: String,
}

pub fn notarize(
    file: PathBuf,
    asc_provider: String,
    username: String,
    password: String,
) -> Result<()> {
    let request_id = submit_notarization_request(file, &asc_provider, &username, &password)?;

    println!("Notarization request: {request_id}");

    let request = get_notarization_info(&request_id, &username, &password)?;
    dbg!(request);

    Ok(())
}

fn submit_notarization_request(
    file: PathBuf,
    asc_provider: &str,
    username: &str,
    password: &str,
) -> Result<String> {
    let file = file.to_str();
    let output = command_to_string(
        "xcrun",
        &[
            "notarytool",
            "--output-format",
            "json",
            "submit",
            "--team-id",
            asc_provider,
            "--apple-id",
            username,
            "--password",
            password,
            file.unwrap(),
            "--wait",
        ],
        None,
    )?;

    let result = serde_json::from_str::<NotaryToolResponse>(&output.stdout)?;

    let request_id = result.notarization_upload.unwrap().request_uuid;

    Ok(request_id)
}

fn get_notarization_info(
    request_id: &str,
    username: &str,
    password: &str,
) -> Result<String> {
    let status = match command_to_string(
        "xcrun",
        &[
            "notarytool",
            "--output-format",
            "json",
            "info",
            request_id,
            "--apple-id",
            username,
            "--password",
            password,
        ],
        None,
    ) {
        Ok(output) => {
            let response = match serde_json::from_str::<NotaryToolResponse>(
                &output.stdout.as_str(),
            ) {
                Ok(v) => v,
                Err(error) => {
                    anyhow::bail!(
                        "Failed to deserialize notarytool response: {error}"
                    );
                }
            };

            response.notarization_info
        }
        Err(error) => {
            anyhow::bail!(format!("Failed to execute notarytool: {error}"));
        }
    };

    Ok(format!("{status:#?}"))
}
