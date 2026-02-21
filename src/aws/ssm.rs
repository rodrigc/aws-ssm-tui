use anyhow::Result;
use aws_config::BehaviorVersion;
use aws_sdk_ssm::{types::InstanceInformationStringFilter, Client};

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
