use crate::utils::constants::env;
use anyhow::{bail, Context, Result};
use aws_config::BehaviorVersion;
use aws_sdk_ssm::{types::InstanceInformationStringFilter, types::PingStatus, Client};
use serde_json;
use std::process::{Command, Stdio};
use which::which;

#[derive(Debug, Clone)]
pub struct InstanceInfo {
    pub instance_id: String,
    pub computer_name: String,
    pub ping_status: String,
    pub platform_type: String,
    pub platform_name: String,
    pub resource_type: String,
}

pub async fn get_instances() -> Result<Vec<InstanceInfo>> {
    // Load AWS configuration
    let config = aws_config::defaults(BehaviorVersion::latest()).load().await;
    let client = Client::new(&config);

    // Create the filter for AssociationStatus=Success
    let filter = InstanceInformationStringFilter::builder()
        .key("AssociationStatus")
        .values("Success")
        .build()?;

    let mut instances = Vec::new();
    let mut next_token: Option<String> = None;

    loop {
        // Call describe_instance_information with the filter
        let mut request = client
            .describe_instance_information()
            .filters(filter.clone())
            .max_results(50);

        if let Some(token) = next_token {
            request = request.next_token(token);
        }

        let response = request.send().await?;

        for instance in response.instance_information_list() {
            instances.push(InstanceInfo {
                instance_id: instance.instance_id().unwrap_or("N/A").to_string(),
                computer_name: instance.computer_name().unwrap_or("").to_string(),
                ping_status: instance
                    .ping_status()
                    .unwrap_or(&PingStatus::Inactive)
                    .to_string(),
                platform_type: instance
                    .platform_type()
                    .map(|p| p.as_str())
                    .unwrap_or("N/A")
                    .to_string(),
                platform_name: instance.platform_name().unwrap_or("N/A").to_string(),
                resource_type: instance
                    .resource_type()
                    .map(|r| r.as_str())
                    .unwrap_or("N/A")
                    .to_string(),
            });
        }

        next_token = response.next_token().map(|s| s.to_string());

        if next_token.is_none() {
            break;
        }
    }
    Ok(instances)
}

pub async fn start_ssm_session(
    client: &aws_sdk_ssm::Client,
    instance_id: &str,
    document_name: Option<&str>,
) -> Result<aws_sdk_ssm::operation::start_session::StartSessionOutput, aws_sdk_ssm::Error> {
    let output = client
        .start_session()
        .target(instance_id)
        .document_name(document_name.unwrap_or("AWS-StartSession"))
        .send()
        .await?;

    Ok(output)
}

/// Starts an interactive SSM session using the session-manager-plugin.
///
/// The region is taken directly from the client's config (which comes from the selected profile).
pub async fn start_interactive_ssm(client: &Client, instance_id: &str) -> Result<()> {
    println!("=== Starting SSM Session (SDK) ===");
    println!("Target Instance : {}", instance_id);

    let output = client
        .start_session()
        .target(instance_id)
        .send()
        .await
        .context("StartSession API failed")?;

    let session_id = output.session_id().context("No session ID")?;
    let stream_url = output.stream_url().context("No stream URL")?;
    let token_value = output.token_value().context("No token value")?;

    let plugin_input = serde_json::json!({
        "Target":     instance_id,
        "SessionId":  session_id,
        "StreamUrl":  stream_url,
        "TokenValue": token_value,
    });

    let parameters = serde_json::json!({
        "Target":     instance_id,
    });

    let plugin_input_str = plugin_input.to_string();
    let plugin_exe = which("session-manager-plugin").context("Plugin not found")?;

    let region = client
        .config()
        .region()
        .map(|r| r.as_ref().to_string())
        .unwrap_or_else(|| "us-east-1".to_string());

    let profile = std::env::var(env::AWS_PROFILE_ENV_VAR).unwrap_or_default();

    println!("Region          : {}", region);
    println!("Profile         : {}", profile);
    println!("Plugin Path     : {}", plugin_exe.display());
    println!(
        "Full Command    : {} \"{}\" {} StartSession {} {{}} \"\"",
        plugin_exe.display(),
        plugin_input_str,
        region,
        profile
    );

    let mut child = Command::new(plugin_exe)
        .arg(plugin_input.to_string()) // 1. JSON payload
        .arg(region) // 2. Region
        .arg("StartSession") // 3. Action
        .arg(profile) // 4. Profile name (important for SSO)
        .arg(parameters.to_string()) // 5. Parameters
        .arg("") // 6. Endpoint override
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Failed to spawn session-manager-plugin")?;

    let status = child.wait()?;

    if !status.success() {
        bail!("Plugin exited with code: {:?}", status.code());
    }

    Ok(())
}

/// Starts an SSM session by calling the AWS CLI directly.
/// This is the most reliable method when using AWS SSO profiles.
pub async fn start_interactive_ssm_with_aws_cli(instance_id: &str, profile: &str) -> Result<()> {
    println!(
        "Starting SSM session on {} using profile '{}'...",
        instance_id, profile
    );

    let status = std::process::Command::new("aws")
        .arg("ssm")
        .arg("start-session")
        .arg("--target")
        .arg(instance_id)
        .arg("--profile")
        .arg(profile)
        .status()
        .context("Failed to execute aws ssm start-session")?;

    if !status.success() {
        bail!(
            "aws ssm start-session exited with code: {:?}",
            status.code()
        );
    }

    Ok(())
}

pub fn print_instances_table(instances: &[InstanceInfo]) {
    println!("\n---------------------------------------------------------------");
    println!("|                DescribeInstanceInformation                  |");
    println!("+---------------------+----------------+--------------+---------------+");
    println!(
        "| {:19} | {:14} | {:12} | {:13} |",
        "InstanceId", "PlatformType", "PlatformName", "ResourceType"
    );
    println!("+---------------------+----------------+--------------+---------------+");

    for instance in instances {
        println!(
            "| {:19} | {:14} | {:12} | {:13} |",
            instance.instance_id,
            instance.platform_type,
            instance.platform_name,
            instance.resource_type
        );
    }

    println!("+---------------------+----------------+--------------+---------------+");
}
