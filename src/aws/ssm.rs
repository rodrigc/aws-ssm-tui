use anyhow::{bail, Context, Result};
use aws_config::BehaviorVersion;
use aws_sdk_ssm::{types::InstanceInformationStringFilter, Client};
use serde_json;
use std::process::{Command, Stdio};
use which::which;

#[derive(Debug, Clone)]
pub struct InstanceInfo {
    pub instance_id: String,
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

    // Call describe_instance_information with the filter
    let response = client
        .describe_instance_information()
        .filters(filter)
        .send()
        .await?;

    let mut instances = Vec::new();

    for instance in response.instance_information_list() {
        instances.push(InstanceInfo {
            instance_id: instance.instance_id().unwrap_or("N/A").to_string(),
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
        .document_name(document_name.unwrap_or("AWS-StartInteractiveCommand"))
        .send()
        .await?;

    Ok(output)
}

/// Starts an interactive SSM session using the session-manager-plugin.
///
/// The region is taken directly from the client's config (which comes from the selected profile).
pub async fn start_interactive_ssm(
    client: &Client,
    instance_id: &str,
    document_name: &str, // e.g. "AWS-StartInteractiveCommand"
) -> Result<()> {
    // 1. Call StartSession API using the existing client (already configured with profile)
    let output = client
        .start_session()
        .target(instance_id)
        .document_name(document_name)
        .send()
        .await
        .context("Failed to start SSM session via SDK")?;

    let session_id = output.session_id().context("Missing session ID")?;
    let stream_url = output.stream_url().context("Missing stream URL")?;
    let token_value = output.token_value().context("Missing token value")?;

    // 2. Build JSON payload for the plugin
    let plugin_input = serde_json::json!({
        "Target":     instance_id,
        "SessionId":  session_id,
        "StreamUrl":  stream_url,
        "TokenValue": token_value,
    });

    let plugin_input_str = plugin_input.to_string();

    // 3. Find session-manager-plugin in PATH
    let plugin_exe = which("session-manager-plugin")
        .context(
            "session-manager-plugin not found in PATH. \
             Please install it: https://docs.aws.amazon.com/systems-manager/latest/userguide/session-manager-working-with-install-plugin.html"
        )?;

    // 4. Get the region directly from the client's config
    let region = client
        .config()
        .region()
        .context("No region found in the current SDK config (profile missing region?)")?
        .as_ref()
        .to_string();

    // Optional: you can log this for debugging (file or TUI status)
    // eprintln!("Using region from profile: {}", region);

    // 5. Spawn the plugin with the profile's region
    let mut child = Command::new(plugin_exe)
        .arg(&plugin_input_str) // JSON payload
        .arg(&region) // region from active profile
        .arg("StartSession") // mode
        .arg("") // profile name (empty = use current credential chain)
        .arg("{}") // extra params (empty object)
        .arg("") // SSM endpoint override (empty = default)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Failed to spawn session-manager-plugin")?;

    // 6. Wait for the user to exit the interactive session
    let status = child.wait().context("Session process error")?;

    if !status.success() {
        bail!("SSM session exited with code: {:?}", status.code());
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
