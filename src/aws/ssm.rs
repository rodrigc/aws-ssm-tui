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
    pub last_ping_date_time: String,
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
                last_ping_date_time: instance
                    .last_ping_date_time()
                    .map(|dt| dt.to_string())
                    .unwrap_or_default(),
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

/// A running background port-forward managed by the TUI (no terminal takeover).
pub struct SpawnedPortForward {
    pub instance_id: String,
    pub remote_port: u16,
    pub local_port: u16,
    pub session_id: String,
    pub child: std::process::Child,
    /// Captured plugin stdout/stderr (continues to grow while the process runs).
    pub output: std::sync::Arc<std::sync::Mutex<String>>,
}

/// Failure starting a port-forward; `detail` is meant for a TUI console popup.
#[derive(Debug)]
pub struct PortForwardError {
    pub instance_id: String,
    pub detail: String,
}

impl std::fmt::Display for PortForwardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.detail)
    }
}

impl std::error::Error for PortForwardError {}

/// Starts an SSM port-forwarding session using the AWS Rust SDK +
/// session-manager-plugin, **without** leaving the TUI or writing to stdout.
///
/// On success the plugin process keeps running in the background and is owned
/// by the returned [`SpawnedPortForward`]. On failure, plugin/API output is
/// collected into [`PortForwardError::detail`].
pub async fn start_port_forwarding(
    client: &Client,
    instance_id: &str,
    remote_port: u16,
    local_port: u16,
) -> std::result::Result<SpawnedPortForward, PortForwardError> {
    let fail = |detail: String| PortForwardError {
        instance_id: instance_id.to_string(),
        detail,
    };

    let remote_port_str = remote_port.to_string();
    let local_port_str = local_port.to_string();

    let output = client
        .start_session()
        .target(instance_id)
        .document_name("AWS-StartPortForwardingSession")
        .parameters("portNumber", vec![remote_port_str.clone()])
        .parameters("localPortNumber", vec![local_port_str.clone()])
        .send()
        .await
        .map_err(|e| fail(format!("StartSession API failed:\n{e:#}")))?;

    let session_id = output
        .session_id()
        .ok_or_else(|| fail("StartSession response missing session ID".into()))?
        .to_string();
    let stream_url = output
        .stream_url()
        .ok_or_else(|| fail("StartSession response missing stream URL".into()))?
        .to_string();
    let token_value = output
        .token_value()
        .ok_or_else(|| fail("StartSession response missing token value".into()))?
        .to_string();

    let plugin_input = serde_json::json!({
        "Target":     instance_id,
        "SessionId":  session_id,
        "StreamUrl":  stream_url,
        "TokenValue": token_value,
    });

    let parameters = serde_json::json!({
        "Target": instance_id,
        "DocumentName": "AWS-StartPortForwardingSession",
        "Parameters": {
            "portNumber": [remote_port_str],
            "localPortNumber": [local_port_str],
        }
    });

    let plugin_exe = which("session-manager-plugin").map_err(|_| {
        fail(
            "session-manager-plugin not found on PATH.\n\
             Install the AWS Session Manager plugin and ensure it is available."
                .into(),
        )
    })?;

    let region = client
        .config()
        .region()
        .map(|r| r.as_ref().to_string())
        .unwrap_or_else(|| "us-east-1".to_string());

    let profile = std::env::var(env::AWS_PROFILE_ENV_VAR).unwrap_or_default();

    let mut child = Command::new(&plugin_exe)
        .arg(plugin_input.to_string())
        .arg(&region)
        .arg("StartSession")
        .arg(&profile)
        .arg(parameters.to_string())
        .arg("")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| fail(format!("Failed to spawn session-manager-plugin:\n{e}")))?;

    let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    {
        let buf = captured.clone();
        if let Some(stdout) = child.stdout.take() {
            std::thread::spawn(move || drain_pipe(stdout, buf));
        }
    }
    {
        let buf = captured.clone();
        if let Some(stderr) = child.stderr.take() {
            std::thread::spawn(move || drain_pipe(stderr, buf));
        }
    }

    // Wait briefly for the plugin to bind the local port or exit with an error.
    let ready_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Let reader threads flush
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                let plugin_out = captured
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                let mut detail = format!(
                    "session-manager-plugin exited early (status: {status})\n\
                     Instance: {instance_id}\n\
                     Forward: localhost:{local_port} → remote:{remote_port}\n\
                     SessionId: {session_id}\n"
                );
                if plugin_out.trim().is_empty() {
                    detail.push_str("\n(no plugin output captured)");
                } else {
                    detail.push_str("\n--- plugin output ---\n");
                    detail.push_str(&plugin_out);
                }
                // Best-effort session cleanup
                let _ = client
                    .terminate_session()
                    .session_id(&session_id)
                    .send()
                    .await;
                return Err(fail(detail));
            }
            Ok(None) => {}
            Err(e) => {
                return Err(fail(format!("Failed to poll session-manager-plugin: {e}")));
            }
        }

        let plugin_out = captured
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let lower = plugin_out.to_lowercase();
        let looks_ready = (lower.contains("port") && lower.contains("opened"))
            || local_port_is_listening(local_port);

        if looks_ready || std::time::Instant::now() >= ready_deadline {
            // Still running after timeout → treat as success (plugin often stays quiet).
            return Ok(SpawnedPortForward {
                instance_id: instance_id.to_string(),
                remote_port,
                local_port,
                session_id,
                child,
                output: captured,
            });
        }

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// Stop a background port-forward process and best-effort terminate the SSM session.
pub async fn stop_port_forwarding(
    client: &Client,
    mut session: SpawnedPortForward,
) -> String {
    let _ = session.child.kill();
    let status = session.child.wait();
    let plugin_out = session
        .output
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();

    let _ = client
        .terminate_session()
        .session_id(&session.session_id)
        .send()
        .await;

    let mut detail = format!(
        "Stopped port forward for {}\n\
         Forward was: localhost:{} → remote:{}\n\
         SessionId: {}\n\
         Process status: {:?}\n",
        session.instance_id,
        session.local_port,
        session.remote_port,
        session.session_id,
        status
    );
    if !plugin_out.trim().is_empty() {
        detail.push_str("\n--- plugin output ---\n");
        detail.push_str(&plugin_out);
    }
    detail
}

fn drain_pipe(mut reader: impl std::io::Read, buf: std::sync::Arc<std::sync::Mutex<String>>) {
    let mut chunk = [0u8; 1024];
    loop {
        match std::io::Read::read(&mut reader, &mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if let Ok(mut guard) = buf.lock() {
                    guard.push_str(&String::from_utf8_lossy(&chunk[..n]));
                }
            }
            Err(_) => break,
        }
    }
}

fn local_port_is_listening(port: u16) -> bool {
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(50)).is_ok()
}
