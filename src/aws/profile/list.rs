use anyhow::Result;
use aws_runtime::env_config::file::EnvConfigFiles;
use aws_types::os_shim_internal::{Env, Fs};

#[derive(Debug, Clone)]
pub struct ProfileInfo {
    pub name: String,
    pub region: Option<String>,
    pub sso_start_url: Option<String>,
    pub sso_account_id: Option<String>,
    pub sso_role_name: Option<String>,
}

pub async fn load_profiles() -> Result<Vec<ProfileInfo>> {
    let fs = Fs::real();
    let env = Env::real();
    let profile_files = EnvConfigFiles::default();

    let profiles_set = aws_config::profile::load(&fs, &env, &profile_files, None).await?;

    let mut profiles = Vec::new();

    for profile_name in profiles_set.profiles() {
        let profile = profiles_set.get_profile(profile_name);

        let region = profile.and_then(|p| p.get("region")).map(|s| s.to_string());
        let sso_start_url = profile
            .and_then(|p| p.get("sso_start_url"))
            .map(String::from);
        let sso_account_id = profile
            .and_then(|p| p.get("sso_account_id"))
            .map(|s| s.to_string());
        let sso_role_name = profile
            .and_then(|p| p.get("sso_role_name"))
            .map(|s| s.to_string());

        profiles.push(ProfileInfo {
            name: profile_name.to_string(),
            region,
            sso_start_url,
            sso_account_id,
            sso_role_name,
        });
    }

    profiles.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(profiles)
}
