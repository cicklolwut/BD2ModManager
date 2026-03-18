use bd2modmanager_lib::utils::path::is_dir_empty;
use pelite::pe32::Pe;
use pelite::FileMap;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tauri::Manager;
#[cfg(target_os = "windows")]
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

use crate::AppState;

fn cleanup_empty_dirs(game_path: &PathBuf, file_list: &[String]) {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for f in file_list {
        let full = game_path.join(f);
        let mut current = full.parent().map(|p| p.to_path_buf());
        while let Some(dir) = current {
            if !dir.starts_with(game_path) || dir == *game_path {
                break;
            }
            dirs.push(dir.clone());
            current = dir.parent().map(|p| p.to_path_buf());
        }
    }
    dirs.sort();
    dirs.dedup();
    dirs.sort_by(|a, b| b.components().count().cmp(&a.components().count()));

    for dir in &dirs {
        if dir.exists() && dir.is_dir() {
            let _ = fs::remove_dir(dir); // only succeeds if empty
        }
    }
}

#[tauri::command]
pub fn locate_game() -> Option<Vec<String>> {
    #[allow(unused_mut)]
    let mut path_founds = Vec::new();

    // Windows: check registry for Neowiz launcher install path
    #[cfg(target_os = "windows")]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let launcher_key_path = r"Software\Neowiz\Browndust2Starter\10000001";

        match hkcu.open_subkey(launcher_key_path) {
            Ok(key) => {
                let result: Result<String, _> = key.get_value("path");
                if let Ok(path) = result {
                    path_founds.push(path);
                }
            }
            Err(_) => {}
        }
    }

    // Linux: auto-detection not yet supported.
    // Users must set the game directory manually via the file picker.
    // TODO: scan Wine/Proton prefixes and Steam library folders.

    if path_founds.is_empty() {
        None
    } else {
        Some(path_founds)
    }
}

#[tauri::command]
pub fn validate_game_path(path: String) -> bool {
    let exe_path = PathBuf::from(&path).join("BrownDust II.exe");
    let data_path = PathBuf::from(&path).join("BrownDust II_Data");
    exe_path.exists() && data_path.exists()
}

// #[derive(Serialize, Deserialize)]
// enum BDXStatus {
//     Installed,
//     NotInstalled,
//     BepinexMissing,
//     GameDirectoryNotSet,
// }

// #[derive(Serialize, Deserialize)]
// struct BDXVersion {
//     status: BDXStatus,
//     version: Option<String>,
// }

#[derive(Serialize, Deserialize)]
pub struct BDXVersionResult {
    status: String,
    version: Option<String>,
    can_remove: Option<bool>,
    can_configure: Option<bool>,
}

fn get_dll_version(dll_path: &PathBuf) -> Option<String> {
    if !dll_path.exists() {
        return None;
    }

    let file_map = FileMap::open(dll_path).ok()?;
    let pe = pelite::pe32::PeFile::from_bytes(&file_map).ok()?;
    let resources = pe.resources().ok()?;
    let version_info = resources.version_info().ok()?;
    let file_info = version_info.file_info();

    for (_lang, strings) in file_info.strings {
        for (key, value) in strings {
            if key == "FileVersion" {
                return Some(value.to_string());
            }
        }
    }

    None
}

// #[derive(Serialize, Deserialize, Clone)]
// pub struct LogMessage {
//     level: String,
//     scope: String,
//     message: String,
//     timestamp: String,
// }

#[tauri::command]
pub fn get_browndustx_version(state: tauri::State<AppState>) -> BDXVersionResult {
    let config = state.config.lock().unwrap();
    let game_path = match &config.game_directory {
        Some(p) => p,
        None => {
            return BDXVersionResult {
                status: "GAME_PATH_NOT_SET".to_string(),
                version: None,
                can_configure: None,
                can_remove: None,
            };
        }
    };

    let bepinex_path = PathBuf::from(game_path).join("BepInEx");
    let bepinex_dll_path = bepinex_path.join("core").join("BepInEx.dll");
    let bepinex_winhttp_dll_path = PathBuf::from(game_path).join("winhttp.dll");

    if !bepinex_path.exists() || !bepinex_dll_path.exists() || !bepinex_winhttp_dll_path.exists() {
        return BDXVersionResult {
            status: "BEPINEX_MISSING".to_string(),
            version: None,
            can_configure: None,
            can_remove: None,
        };
    }

    let bdx_dll_path = PathBuf::from(game_path)
        .join("BepInEx")
        .join("plugins")
        .join("BrownDustX")
        .join("lynesth.bd2.browndustx.dll");

    let bdx_version = get_dll_version(&bdx_dll_path);

    let can_remove = (|| -> bool {
        if !bdx_dll_path.exists() {
            return false;
        }

        let manifest_path = PathBuf::from(game_path)
            .join("BepInEx")
            .join("plugins")
            .join("BrownDustX")
            .join(".bd2mm_bdx_manifest.json");

        if !manifest_path.exists() {
            return false;
        };

        let Ok(manifest_content) = fs::read_to_string(&manifest_path) else {
            return false;
        };

        let Ok(manifest_json) = serde_json::from_str::<serde_json::Value>(&manifest_content) else {
            return false;
        };

        let Some(version_str) = manifest_json.get("version").and_then(|v| v.as_str()) else {
            return false;
        };

        let Some(bdx_ver) = bdx_version.as_ref() else {
            return false;
        };

        version_str == bdx_ver
    })();

    let can_configure = if bdx_dll_path.exists() {
        let config_path = PathBuf::from(game_path)
            .join("BepInEx")
            .join("config")
            .join("BrownDustX.cfg");
        config_path.exists()
    } else {
        false
    };

    match bdx_version {
        Some(v) => BDXVersionResult {
            status: "INSTALLED".to_string(),
            version: Some(v),
            can_remove: Some(can_remove),
            can_configure: Some(can_configure),
        },
        None => BDXVersionResult {
            status: "NOT_INSTALLED".to_string(),
            version: None,
            can_remove: None,
            can_configure: None,
        },
    }
}

// #[derive(Serialize, Deserialize)]
// enum BepInExStatus {
//     INSTALLED,
//     FolderMissing,
//     CoreDllMissing,
//     WinhttpDllMISSING,
//     MultipleFilesMissing,
//     GameDirectoryNotSet,
// }

#[derive(Serialize, Deserialize)]
pub struct BepInExVersionResult {
    status: String,
    version: Option<String>,
    can_remove: Option<bool>,
    can_configure: Option<bool>,
}

#[tauri::command]
pub fn get_bepinex_version(state: tauri::State<AppState>) -> BepInExVersionResult {
    let config = state.config.lock().unwrap();
    let game_path = match &config.game_directory {
        Some(p) => p,
        None => {
            return BepInExVersionResult {
                status: "GAME_PATH_NOT_SET".to_string(),
                version: None,
                can_configure: None,
                can_remove: None,
            };
        }
    };

    let bepinex_dll_path = PathBuf::from(game_path)
        .join("BepInEx")
        .join("core")
        .join("BepInEx.dll");

    let bepinex_version = get_dll_version(&bepinex_dll_path);

    let mut can_remove = false;
    let mut can_configure = false;

    let manifest_path = PathBuf::from(game_path)
        .join("BepInEx")
        .join(".bd2mm_bepinex_manifest.json");

    if manifest_path.exists() {
        if let Ok(manifest_content) = fs::read_to_string(&manifest_path) {
            if let Ok(manifest_json) = serde_json::from_str::<serde_json::Value>(&manifest_content)
            {
                if let Some(manifest_version) = manifest_json.get("version") {
                    if let Some(version_str) = manifest_version.as_str() {
                        if let Some(bepinex_ver) = &bepinex_version {
                            if version_str == bepinex_ver {
                                can_remove = true;
                            }
                        }
                    }
                }
            }
        }
    }

    // Block removal if plugins are still installed
    if can_remove {
        let bdx_dll = PathBuf::from(game_path)
            .join("BepInEx")
            .join("plugins")
            .join("BrownDustX")
            .join("lynesth.bd2.browndustx.dll");
        let cm_dll = PathBuf::from(game_path)
            .join("BepInEx")
            .join("plugins")
            .join("ConfigurationManager")
            .join("ConfigurationManager.dll");
        if bdx_dll.exists() || cm_dll.exists() {
            can_remove = false;
        }
    }

    let config_path = PathBuf::from(game_path)
        .join("BepInEx")
        .join("config")
        .join("BepInEx.cfg");

    if config_path.exists() {
        can_configure = true;
    }

    match bepinex_version {
        Some(v) => BepInExVersionResult {
            status: "INSTALLED".to_string(),
            version: Some(v),
            can_configure: Some(can_configure),
            can_remove: Some(can_remove),
        },
        None => BepInExVersionResult {
            status: "NOT_INSTALLED".to_string(),
            version: None,
            can_configure: None,
            can_remove: None,
        },
    }
}

#[derive(Serialize, Deserialize)]
pub struct ConfigManagerVersionResult {
    status: String,
    version: Option<String>,
    can_configure: Option<bool>,
    can_remove: Option<bool>,
}

#[tauri::command]
pub fn get_configmanager_version(state: tauri::State<AppState>) -> ConfigManagerVersionResult {
    let config = state.config.lock().unwrap();
    let game_path = match &config.game_directory {
        Some(p) => p,
        None => {
            return ConfigManagerVersionResult {
                status: "GAME_PATH_NOT_SET".to_string(),
                version: None,
                can_configure: None,
                can_remove: None,
            };
        }
    };

    let configmanager_dll_path = PathBuf::from(game_path)
        .join("BepInEx")
        .join("plugins")
        .join("ConfigurationManager")
        .join("ConfigurationManager.dll");

    let bepinex_path = PathBuf::from(game_path).join("BepInEx");
    let bepinex_dll_path = bepinex_path.join("core").join("BepInEx.dll");
    let bepinex_winhttp_dll_path = PathBuf::from(game_path).join("winhttp.dll");

    if !bepinex_path.exists() || !bepinex_dll_path.exists() || !bepinex_winhttp_dll_path.exists() {
        return ConfigManagerVersionResult {
            status: "BEPINEX_MISSING".to_string(),
            version: None,
            can_configure: None,
            can_remove: None,
        };
    }

    let configmanager_version = get_dll_version(&configmanager_dll_path);

    let can_configure = if configmanager_dll_path.exists() {
        let config_path = PathBuf::from(game_path)
            .join("BepInEx")
            .join("config")
            .join("ConfigurationManager.cfg");
        config_path.exists()
    } else {
        false
    };

    let can_remove = (|| -> bool {
        if !configmanager_dll_path.exists() {
            return false;
        }

        let manifest_path = PathBuf::from(game_path)
            .join("BepInEx")
            .join("plugins")
            .join("ConfigurationManager")
            .join(".bd2mm_configmanager_manifest.json");

        if !manifest_path.exists() {
            return false;
        }

        let Ok(manifest_content) = fs::read_to_string(&manifest_path) else {
            return false;
        };
        let Ok(manifest_json) = serde_json::from_str::<serde_json::Value>(&manifest_content) else {
            return false;
        };
        let Some(version_str) = manifest_json.get("version").and_then(|v| v.as_str()) else {
            return false;
        };
        let Some(configmanager_ver) = configmanager_version.as_ref() else {
            return false;
        };

        version_str == configmanager_ver
    })();

    match configmanager_version {
        Some(v) => ConfigManagerVersionResult {
            status: "INSTALLED".to_string(),
            version: Some(v),
            can_configure: Some(can_configure),
            can_remove: Some(can_remove),
        },
        None => ConfigManagerVersionResult {
            status: "NOT_INSTALLED".to_string(),
            version: None,
            can_configure: None,
            can_remove: None,
        },
    }
}

const BEPINEX_COMMON_FILES: [&str; 3] = [
    "BepInEx/core/BepInEx.dll",
    "doorstop_config.ini",
    "winhttp.dll",
];

const BDX_COMMON_FILES: [&str; 3] = [
    "BepInEx/plugins/BrownDustX/lynesth.bd2.browndustx.dll",
    "BepInEx/plugins/BrownDustX/K4os.Compression.LZ4.dll",
    "BepInEx/plugins/BrownDustX/resources/",
];

const CONFIGMANAGER_COMMON_FILES: [&str; 1] = [
    "BepInEx/plugins/ConfigurationManager/ConfigurationManager.dll",
];

#[derive(Serialize, Deserialize)]
pub enum InstallBepInExError {
    GamePathNotSet,
    InvalidBepInExArchive,
    BepInExAlreadyInstalled,
    DownloadFailed(String),
    ExtractionFailed(String),
    ArchiveNotFound(String),
}

#[derive(Serialize, Deserialize)]
pub enum InstallBrownDustXError {
    GamePathNotSet,
    InvalidBrownDustXArchive,
    BepInExNotInstalled,
    BrownDustXAlreadyInstalled,
    ExtractionFailed(String),
    ArchiveNotFound(String),
}

#[derive(Serialize, Deserialize)]
pub enum InstallConfigManagerError {
    GamePathNotSet,
    InvalidConfigManagerArchive,
    BepInExNotInstalled,
    ConfigManagerAlreadyInstalled,
    ExtractionFailed(String),
    ArchiveNotFound(String),
}

fn is_bepinex_archive<R: std::io::Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>) -> bool {
    BEPINEX_COMMON_FILES
        .iter()
        .all(|file| archive.by_name(file).is_ok())
}

fn is_bdx_archive<R: std::io::Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>) -> bool {
    BDX_COMMON_FILES
        .iter()
        .all(|file| archive.by_name(file).is_ok())
}

fn is_configmanager_archive<R: std::io::Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>) -> bool {
    CONFIGMANAGER_COMMON_FILES
        .iter()
        .all(|file| archive.by_name(file).is_ok())
}

fn extract_bepinex_files<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    game_path: PathBuf,
) -> Result<Vec<String>, InstallBepInExError> {
    if !is_bepinex_archive(archive) {
        return Err(InstallBepInExError::InvalidBepInExArchive);
    }

    let mut extracted_files: Vec<String> = Vec::new();

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| InstallBepInExError::ExtractionFailed(e.to_string()))?;

        let Some(path) = file.enclosed_name() else {
            continue;
        };

        extracted_files.push(path.clone().to_string_lossy().to_string());

        let outpath = game_path.join(path);

        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)
                .map_err(|e| InstallBepInExError::ExtractionFailed(e.to_string()))?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| InstallBepInExError::ExtractionFailed(e.to_string()))?;
            }
            let mut outfile = fs::File::create(&outpath)
                .map_err(|e| InstallBepInExError::ExtractionFailed(e.to_string()))?;
            std::io::copy(&mut file, &mut outfile)
                .map_err(|e| InstallBepInExError::ExtractionFailed(e.to_string()))?;
        }
    }

    Ok(extracted_files)
}

fn extract_bepinex_from_bytes(
    bytes: Vec<u8>,
    game_path: PathBuf,
) -> Result<Vec<String>, InstallBepInExError> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|_| InstallBepInExError::InvalidBepInExArchive)?;

    extract_bepinex_files(&mut archive, game_path)
}

fn extract_bepinex_from_path(
    archive_path: PathBuf,
    game_path: PathBuf,
) -> Result<Vec<String>, InstallBepInExError> {
    let file = fs::File::open(&archive_path)
        .map_err(|e| InstallBepInExError::ArchiveNotFound(e.to_string()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|_| InstallBepInExError::InvalidBepInExArchive)?;

    extract_bepinex_files(&mut archive, game_path)
}

#[tauri::command]
pub async fn install_bepinex_from_archive(
    state: tauri::State<'_, AppState>,
    archive_path: String,
) -> Result<(), InstallBepInExError> {
    let game_path = {
        let config = state.config.lock().unwrap();
        config
            .game_directory
            .as_ref()
            .map(PathBuf::from)
            .ok_or(InstallBepInExError::GamePathNotSet)?
    };

    let archive_path = PathBuf::from(archive_path);
    if !archive_path.exists() {
        return Err(InstallBepInExError::ArchiveNotFound(
            archive_path.to_string_lossy().to_string(),
        ));
    }

    let bepinex_dir = game_path.join("BepInEx");
    if bepinex_dir.exists() && !is_dir_empty(&bepinex_dir) {
        return Err(InstallBepInExError::BepInExAlreadyInstalled);
    }

    let archive_path_clone = archive_path.clone();
    let game_path_clone = game_path.clone();

    let extracted_files: Vec<_> = tokio::task::spawn_blocking(move || {
        extract_bepinex_from_path(archive_path_clone, game_path_clone)
    })
    .await
    .map_err(|e| InstallBepInExError::ExtractionFailed(e.to_string()))??;

    let file_path = game_path
        .join("BepInEx")
        .join(".bd2mm_bepinex_manifest.json");

    let bepinex_dll_path = PathBuf::from(game_path)
        .join("BepInEx")
        .join("core")
        .join("BepInEx.dll");

    let bepinex_version = get_dll_version(&bepinex_dll_path);

    let manifest = serde_json::json!({
        "version": bepinex_version,
        "installed_files": extracted_files,
    });

    fs::write(file_path, serde_json::to_string_pretty(&manifest).unwrap())
        .map_err(|e| InstallBepInExError::ExtractionFailed(e.to_string()))?;

    Ok(())
}

#[tauri::command]
pub async fn install_bepinex_from_url(
    state: tauri::State<'_, AppState>,
    url: String,
) -> Result<(), InstallBepInExError> {
    let game_path = {
        let config = state.config.lock().unwrap();
        config
            .game_directory
            .as_ref()
            .map(PathBuf::from)
            .ok_or(InstallBepInExError::GamePathNotSet)?
    };

    let bepinex_dir = game_path.join("BepInEx");
    if bepinex_dir.exists() && !is_dir_empty(&bepinex_dir) {
        return Err(InstallBepInExError::BepInExAlreadyInstalled);
    }

    let response = reqwest::get(&url)
        .await
        .map_err(|e| InstallBepInExError::DownloadFailed(e.to_string()))?;

    if !response.status().is_success() {
        return Err(InstallBepInExError::DownloadFailed(format!(
            "HTTP {}",
            response.status()
        )));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| InstallBepInExError::DownloadFailed(e.to_string()))?
        .to_vec();

    let game_path_clone = game_path.clone();

    let extracted_files: Vec<String> =
        tokio::task::spawn_blocking(move || extract_bepinex_from_bytes(bytes, game_path_clone))
            .await
            .map_err(|e| InstallBepInExError::ExtractionFailed(e.to_string()))??;

    let file_path = game_path
        .join("BepInEx")
        .join(".bd2mm_bepinex_manifest.json");
    let bepinex_dll_path = PathBuf::from(game_path)
        .join("BepInEx")
        .join("core")
        .join("BepInEx.dll");
    let bepinex_version = get_dll_version(&bepinex_dll_path);
    let manifest = serde_json::json!({
        "version": bepinex_version,
        "installed_files": extracted_files,
    });
    fs::write(file_path, serde_json::to_string_pretty(&manifest).unwrap())
        .map_err(|e| InstallBepInExError::ExtractionFailed(e.to_string()))?;

    Ok(())
}

fn extract_browndustx_files<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    game_path: PathBuf,
) -> Result<Vec<String>, InstallBrownDustXError> {
    if !is_bdx_archive(archive) {
        return Err(InstallBrownDustXError::InvalidBrownDustXArchive);
    }

    let mut extracted_files: Vec<String> = Vec::new();

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| InstallBrownDustXError::ExtractionFailed(e.to_string()))?;

        let Some(path) = file.enclosed_name() else {
            continue;
        };

        extracted_files.push(path.clone().to_string_lossy().to_string());

        let outpath = game_path.join(path);

        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)
                .map_err(|e| InstallBrownDustXError::ExtractionFailed(e.to_string()))?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| InstallBrownDustXError::ExtractionFailed(e.to_string()))?;
            }
            let mut outfile = fs::File::create(&outpath)
                .map_err(|e| InstallBrownDustXError::ExtractionFailed(e.to_string()))?;
            std::io::copy(&mut file, &mut outfile)
                .map_err(|e| InstallBrownDustXError::ExtractionFailed(e.to_string()))?;
        }
    }

    Ok(extracted_files)
}

fn extract_browndustx_from_path(
    archive_path: PathBuf,
    game_path: PathBuf,
) -> Result<Vec<String>, InstallBrownDustXError> {
    let file = fs::File::open(&archive_path)
        .map_err(|e| InstallBrownDustXError::ArchiveNotFound(e.to_string()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|_| InstallBrownDustXError::InvalidBrownDustXArchive)?;

    extract_browndustx_files(&mut archive, game_path)
}

#[tauri::command]
pub async fn install_browndustx_from_archive(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<(), InstallBrownDustXError> {
    let game_path = {
        let config = state.config.lock().unwrap();
        match &config.game_directory {
            Some(p) => PathBuf::from(p),
            None => return Err(InstallBrownDustXError::GamePathNotSet),
        }
    };

    let archive_path = PathBuf::from(path);
    if !archive_path.exists() {
        return Err(InstallBrownDustXError::ArchiveNotFound(
            archive_path.to_string_lossy().to_string(),
        ));
    }

    if game_path
        .join("BepInEx")
        .join("plugins")
        .join("BrownDustX")
        .join("lynesth.bd2.browndustx.dll")
        .exists()
    {
        return Err(InstallBrownDustXError::BrownDustXAlreadyInstalled);
    }

    let bepinex_path = game_path.join("BepInEx");
    let bepinex_dll_path = bepinex_path.join("core").join("BepInEx.dll");
    let bepinex_winhttp_dll_path = game_path.join("winhttp.dll");

    if !bepinex_path.exists() || !bepinex_dll_path.exists() || !bepinex_winhttp_dll_path.exists() {
        return Err(InstallBrownDustXError::BepInExNotInstalled);
    }

    let archive_path_clone = archive_path.clone();
    let game_path_clone = game_path.clone();

    let extracted_files = tokio::task::spawn_blocking(move || {
        extract_browndustx_from_path(archive_path_clone, game_path_clone)
    })
    .await
    .map_err(|e| InstallBrownDustXError::ExtractionFailed(e.to_string()))??;

    let file_path = game_path
        .join("BepInEx")
        .join("plugins")
        .join("BrownDustX")
        .join(".bd2mm_bdx_manifest.json");

    let bdx_dll_path = PathBuf::from(&game_path)
        .join("BepInEx")
        .join("plugins")
        .join("BrownDustX")
        .join("lynesth.bd2.browndustx.dll");
    let bdx_version = get_dll_version(&bdx_dll_path);
    let manifest = serde_json::json!({
        "version": bdx_version,
        "installed_files": extracted_files,
    });
    fs::write(file_path, serde_json::to_string_pretty(&manifest).unwrap())
        .map_err(|e| InstallBrownDustXError::ExtractionFailed(e.to_string()))?;

    Ok(())
}

#[tauri::command]
pub fn determine_archive_type(path: String) -> Result<Option<String>, String> {
    let archive_path = PathBuf::from(path);
    if !archive_path.exists() {
        return Ok(None);
    }

    let file = fs::File::open(&archive_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    if is_bepinex_archive(&mut archive) {
        return Ok(Some("BEPINEX".to_string()));
    }

    if is_bdx_archive(&mut archive) {
        return Ok(Some("BROWNDUSTX".to_string()));
    }

    if is_configmanager_archive(&mut archive) {
        return Ok(Some("CONFIGMANAGER".to_string()));
    }

    Ok(None)
}

#[tauri::command]
pub async fn uninstall_bepinex(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let game_path = {
        let config = state.config.lock().unwrap();
        config
            .game_directory
            .as_ref()
            .map(PathBuf::from)
            .ok_or("Game path not set".to_string())?
    };

    let bepinex_path = game_path.join("BepInEx");
    let bepinex_dll_path = bepinex_path.join("core").join("BepInEx.dll");
    let bepinex_winhttp_dll_path = game_path.join("winhttp.dll");

    if !bepinex_path.exists() || !bepinex_dll_path.exists() || !bepinex_winhttp_dll_path.exists() {
        return Err("BepInEx is not installed".to_string());
    }

    // Block if plugins are still installed
    let bdx_dll = game_path
        .join("BepInEx")
        .join("plugins")
        .join("BrownDustX")
        .join("lynesth.bd2.browndustx.dll");
    let cm_dll = game_path
        .join("BepInEx")
        .join("plugins")
        .join("ConfigurationManager")
        .join("ConfigurationManager.dll");
    if bdx_dll.exists() || cm_dll.exists() {
        return Err("Cannot remove BepInEx while plugins are still installed. Remove plugins first.".to_string());
    }

    let manifest_path = bepinex_path.join(".bd2mm_bepinex_manifest.json");
    if !manifest_path.exists() {
        return Err("BepInEx manifest not found".to_string());
    }

    let manifest_content = fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let manifest_json: serde_json::Value =
        serde_json::from_str(&manifest_content).map_err(|e| e.to_string())?;

    let installed_files = manifest_json
        .get("installed_files")
        .and_then(|v| v.as_array())
        .ok_or("Invalid BepInEx manifest".to_string())?;

    let mut files_to_remove: Vec<String> = installed_files
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    files_to_remove.reverse();

    for file_str in files_to_remove {
        let file_path = game_path.join(&file_str);

        if !file_path.exists() {
            continue;
        }

        if file_path.is_file() {
            fs::remove_file(&file_path)
                .map_err(|e| format!("Failed to remove file {}: {}", file_str, e))?;
        } else if file_path.is_dir() {
            match fs::remove_dir(&file_path) {
                Ok(_) => {}
                Err(e)
                    if e.kind() == std::io::ErrorKind::Other
                        || e.raw_os_error() == Some(145)
                        || e.raw_os_error() == Some(39) =>
                {
                    continue;
                }
                Err(e) => {
                    return Err(format!("Failed to remove directory {}: {}", file_str, e));
                }
            }
        }
    }

    if manifest_path.exists() {
        fs::remove_file(&manifest_path).map_err(|e| e.to_string())?;
    }

    cleanup_empty_dirs(&game_path, &installed_files
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect::<Vec<_>>());

    Ok(())
}

#[tauri::command]
pub async fn uninstall_browndustx(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let game_path = {
        let config = state.config.lock().unwrap();
        config
            .game_directory
            .as_ref()
            .map(PathBuf::from)
            .ok_or("Game path not set".to_string())?
    };

    let bdx_dll_path = PathBuf::from(&game_path)
        .join("BepInEx")
        .join("plugins")
        .join("BrownDustX")
        .join("lynesth.bd2.browndustx.dll");

    if !bdx_dll_path.exists() {
        return Err("BrownDustX is not installed".to_string());
    }

    let manifest_path = PathBuf::from(&game_path)
        .join("BepInEx")
        .join("plugins")
        .join("BrownDustX")
        .join(".bd2mm_bdx_manifest.json");

    if !manifest_path.exists() {
        return Err("BrownDustX manifest not found".to_string());
    }

    let manifest_content = fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let manifest_json: serde_json::Value =
        serde_json::from_str(&manifest_content).map_err(|e| e.to_string())?;

    let installed_files = manifest_json
        .get("installed_files")
        .and_then(|v| v.as_array())
        .ok_or("Invalid BrownDustX manifest".to_string())?;

    let mut files_to_remove: Vec<String> = installed_files
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    files_to_remove.reverse();

    for file_str in files_to_remove {
        let file_path = game_path.join(&file_str);

        if !file_path.exists() {
            continue;
        }

        if file_path.is_file() {
            fs::remove_file(&file_path)
                .map_err(|e| format!("Failed to remove file {}: {}", file_str, e))?;
        } else if file_path.is_dir() {
            match fs::remove_dir(&file_path) {
                Ok(_) => {}
                Err(e)
                    if e.kind() == std::io::ErrorKind::Other
                        || e.raw_os_error() == Some(145)
                        || e.raw_os_error() == Some(39) =>
                {
                    continue;
                }
                Err(e) => {
                    return Err(format!("Failed to remove directory {}: {}", file_str, e));
                }
            }
        }
    }
    if manifest_path.exists() {
        fs::remove_file(&manifest_path).map_err(|e| e.to_string())?;
    }

    cleanup_empty_dirs(&game_path, &installed_files
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect::<Vec<_>>());

    Ok(())
}

fn extract_configmanager_files<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    game_path: PathBuf,
) -> Result<Vec<String>, InstallConfigManagerError> {
    if !is_configmanager_archive(archive) {
        return Err(InstallConfigManagerError::InvalidConfigManagerArchive);
    }

    let mut extracted_files: Vec<String> = Vec::new();

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| InstallConfigManagerError::ExtractionFailed(e.to_string()))?;

        let Some(path) = file.enclosed_name() else {
            continue;
        };

        extracted_files.push(path.clone().to_string_lossy().to_string());

        let outpath = game_path.join(path);

        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)
                .map_err(|e| InstallConfigManagerError::ExtractionFailed(e.to_string()))?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| InstallConfigManagerError::ExtractionFailed(e.to_string()))?;
            }
            let mut outfile = fs::File::create(&outpath)
                .map_err(|e| InstallConfigManagerError::ExtractionFailed(e.to_string()))?;
            std::io::copy(&mut file, &mut outfile)
                .map_err(|e| InstallConfigManagerError::ExtractionFailed(e.to_string()))?;
        }
    }

    Ok(extracted_files)
}

fn extract_configmanager_from_path(
    archive_path: PathBuf,
    game_path: PathBuf,
) -> Result<Vec<String>, InstallConfigManagerError> {
    let file = fs::File::open(&archive_path)
        .map_err(|e| InstallConfigManagerError::ArchiveNotFound(e.to_string()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|_| InstallConfigManagerError::InvalidConfigManagerArchive)?;

    extract_configmanager_files(&mut archive, game_path)
}

#[tauri::command]
pub async fn install_configmanager_from_archive(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<(), InstallConfigManagerError> {
    let game_path = {
        let config = state.config.lock().unwrap();
        match &config.game_directory {
            Some(p) => PathBuf::from(p),
            None => return Err(InstallConfigManagerError::GamePathNotSet),
        }
    };

    let archive_path = PathBuf::from(path);
    if !archive_path.exists() {
        return Err(InstallConfigManagerError::ArchiveNotFound(
            archive_path.to_string_lossy().to_string(),
        ));
    }

    let configmanager_dll_path = game_path
        .join("BepInEx")
        .join("plugins")
        .join("ConfigurationManager")
        .join("ConfigurationManager.dll");

    if configmanager_dll_path.exists() {
        return Err(InstallConfigManagerError::ConfigManagerAlreadyInstalled);
    }

    let bepinex_path = game_path.join("BepInEx");
    let bepinex_dll_path = bepinex_path.join("core").join("BepInEx.dll");
    let bepinex_winhttp_dll_path = game_path.join("winhttp.dll");

    if !bepinex_path.exists() || !bepinex_dll_path.exists() || !bepinex_winhttp_dll_path.exists() {
        return Err(InstallConfigManagerError::BepInExNotInstalled);
    }

    let archive_path_clone = archive_path.clone();
    let game_path_clone = game_path.clone();

    let extracted_files = tokio::task::spawn_blocking(move || {
        extract_configmanager_from_path(archive_path_clone, game_path_clone)
    })
    .await
    .map_err(|e| InstallConfigManagerError::ExtractionFailed(e.to_string()))??;

    let file_path = game_path
        .join("BepInEx")
        .join("plugins")
        .join("ConfigurationManager")
        .join(".bd2mm_configmanager_manifest.json");

    let configmanager_dll_path = game_path
        .join("BepInEx")
        .join("plugins")
        .join("ConfigurationManager")
        .join("ConfigurationManager.dll");
    let configmanager_version = get_dll_version(&configmanager_dll_path);
    let manifest = serde_json::json!({
        "version": configmanager_version,
        "installed_files": extracted_files,
    });
    fs::write(file_path, serde_json::to_string_pretty(&manifest).unwrap())
        .map_err(|e| InstallConfigManagerError::ExtractionFailed(e.to_string()))?;

    Ok(())
}

#[tauri::command]
pub async fn uninstall_configmanager(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let game_path = {
        let config = state.config.lock().unwrap();
        config
            .game_directory
            .as_ref()
            .map(PathBuf::from)
            .ok_or("Game path not set".to_string())?
    };

    let configmanager_dll_path = game_path
        .join("BepInEx")
        .join("plugins")
        .join("ConfigurationManager")
        .join("ConfigurationManager.dll");

    if !configmanager_dll_path.exists() {
        return Err("Configuration Manager is not installed".to_string());
    }

    let manifest_path = game_path
        .join("BepInEx")
        .join("plugins")
        .join("ConfigurationManager")
        .join(".bd2mm_configmanager_manifest.json");

    if !manifest_path.exists() {
        return Err("Configuration Manager manifest not found".to_string());
    }

    let manifest_content = fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let manifest_json: serde_json::Value =
        serde_json::from_str(&manifest_content).map_err(|e| e.to_string())?;

    let installed_files = manifest_json
        .get("installed_files")
        .and_then(|v| v.as_array())
        .ok_or("Invalid Configuration Manager manifest".to_string())?;

    let mut files_to_remove: Vec<String> = installed_files
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    files_to_remove.reverse();

    for file_str in files_to_remove {
        let file_path = game_path.join(&file_str);

        if !file_path.exists() {
            continue;
        }

        if file_path.is_file() {
            fs::remove_file(&file_path)
                .map_err(|e| format!("Failed to remove file {}: {}", file_str, e))?;
        } else if file_path.is_dir() {
            match fs::remove_dir(&file_path) {
                Ok(_) => {}
                Err(e)
                    if e.kind() == std::io::ErrorKind::Other
                        || e.raw_os_error() == Some(145)
                        || e.raw_os_error() == Some(39) =>
                {
                    continue;
                }
                Err(e) => {
                    return Err(format!("Failed to remove directory {}: {}", file_str, e));
                }
            }
        }
    }

    if manifest_path.exists() {
        fs::remove_file(&manifest_path).map_err(|e| e.to_string())?;
    }

    cleanup_empty_dirs(&game_path, &installed_files
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect::<Vec<_>>());

    Ok(())
}

#[tauri::command]
pub fn get_game_version(state: tauri::State<AppState>) -> Option<String> {
    let config = state.config.lock().ok()?;
    let game_path = config.game_directory.as_ref()?;

    let path = PathBuf::from(game_path)
        .join("BrownDust II_Data")
        .join("globalgamemanagers");

    if !path.exists() {
        return None;
    }

    let data = fs::read(&path).ok()?;
    let content = String::from_utf8_lossy(&data);

    let re = Regex::new(r"\b\d{1,2}\.\d{1,2}\.\d{1,2}\b").ok()?;

    for cap in re.find_iter(&content) {
        let version = cap.as_str();

        if !version.starts_with("20") {
            return Some(version.to_string());
        }
    }

    None
}

#[tauri::command]
pub fn get_characters(app_handle: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let chars_path = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("characters.json");

    let content = std::fs::read_to_string(&chars_path).map_err(|e| e.to_string())?;

    serde_json::from_str(&content).map_err(|e| e.to_string())
}


#[tauri::command]
pub fn launch_game(state: tauri::State<AppState>) -> Result<(), String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    let game_path = config.game_directory.as_ref().ok_or("Game path not set".to_string())?;

    let exe_path = PathBuf::from(game_path).join("BrownDust II.exe");

    if !exe_path.exists() {
        return Err("Game executable not found".to_string());
    }

    // On Windows, launch the exe directly
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new(exe_path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    // On Linux, game launching is not yet supported.
    // Users should launch through their Wine/Proton prefix or Steam.
    // TODO: add configurable Wine/Proton prefix + binary path in settings.
    #[cfg(not(target_os = "windows"))]
    {
        return Err("Game launching is not supported on Linux yet. Please launch through your Wine/Proton prefix or Steam.".to_string());
    }

    #[allow(unreachable_code)]
    Ok(())
}