use aws_runtime::env_config::file::EnvConfigFiles;
use aws_types::os_shim_internal::{Env, Fs};

/// Tries to see if we can load a default-ish config without fatal errors
/// Returns Some(profile_name) if usable, None if we need user to choose
pub async fn detect_default_profile() -> Option<String> {
    // First check env var (highest precedence)
    if let Ok(profile) = std::env::var("AWS_PROFILE") {
        return Some(profile);
    }

    // Try loading profiles safely
    let fs = aws_types::os_shim_internal::Fs::real();
    let env = aws_types::os_shim_internal::Env::real();
    let files = EnvConfigFiles::default();

    let profile_set = match aws_config::profile::load(&fs, &env, &files, None).await {
        Ok(set) => set,
        Err(_) => return None,
    };

    // If there's a "default" profile (even if empty-ish), consider it usable
    if profile_set.get_profile("default").is_some() {
        return Some("default".to_string());
    }

    // Or if only one profile exists, auto-select it (common case)
    let profiles: Vec<_> = profile_set.profiles().collect();
    if profiles.len() == 1 {
        return Some(profiles[0].to_string());
    }

    None
}
