use std::path::PathBuf;

use bd2modmanager_lib::utils::path::get_mod_preview_path;
#[cfg(target_os = "windows")]
use pelite::{FileMap, PeFile};
use semver::Version;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

const RELEASES_URL: &str = "https://api.github.com/repos/bruhnn/BD2ModPreview/releases/latest";

pub fn get_mod_preview_version(app_handle: tauri::AppHandle) -> Option<String> {
    let exe_path = get_mod_preview_path(&app_handle)?;

    let exe_path = PathBuf::from(exe_path);

    if !exe_path.exists() {
        println!("path doesnt exist");
        return None;
    }

    // On Windows, read version from PE headers
    #[cfg(target_os = "windows")]
    {
        let file_map = FileMap::open(&exe_path).ok()?;
        let pe = PeFile::from_bytes(&file_map).ok()?;
        let resources = pe.resources().ok()?;
        let version_info = resources.version_info().ok()?;
        let file_info = version_info.file_info();
        for (_lang, strings) in file_info.strings {
            for (key, value) in strings {
                if key == "FileVersion" || key == "ProductVersion" {
                    return Some(value.to_string());
                }
            }
        }
        return None;
    }

    // On Linux, try --version flag
    #[cfg(not(target_os = "windows"))]
    {
        let output = std::process::Command::new(&exe_path)
            .arg("--version")
            .output()
            .ok()?;
        let version_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if version_str.is_empty() { None } else { Some(version_str) }
    }
}

async fn get_latest_mod_preview_version() -> Result<(Version, String), String> {
    let client = reqwest::Client::new();

    let release: GitHubRelease = client
        .get(RELEASES_URL)
        .header("User-Agent", "BD2ModManager")
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Invalid JSON: {e}"))?;

    let latest_version = release.tag_name.trim_start_matches('v');

    #[cfg(target_os = "windows")]
    let asset_name = "BD2ModPreview.exe";
    #[cfg(not(target_os = "windows"))]
    let asset_name = "bd2modpreview";

    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or(format!("{} not found in release", asset_name))?;

    let version =
        Version::parse(latest_version).map_err(|e| format!("Invalid remote version: {e}"))?;

    Ok((version, asset.browser_download_url.clone()))
}

pub async fn check_for_update(
    app_handle: tauri::AppHandle,
) -> Result<(Option<String>, Option<String>), String> {
    let local_version = get_mod_preview_version(app_handle.clone());

    let (latest_version, download_url) = get_latest_mod_preview_version().await?;

    println!("Local: {:?}, Remote: {}", local_version, latest_version);

    if let Some(local) = local_version {
        let local_ver =
            Version::parse(&local).map_err(|e| format!("Invalid local version: {e}"))?;

        if latest_version <= local_ver {
            return Ok((None, None)); // Up-to-date
        }
    }

    Ok((Some(latest_version.to_string()), Some(download_url)))
}
