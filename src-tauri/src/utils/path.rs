use std::{env, path::{Path, PathBuf}};

use tauri::{path::BaseDirectory, AppHandle, Manager};

use crate::config::BD2Config;

pub fn ensure_dir_exists(path: &PathBuf) -> Result<(), std::io::Error> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

fn get_executable_dir() -> PathBuf {
    let exe_dir = env::current_exe()
        .expect("Failed to get the current exe path")
        .parent()
        .unwrap()
        .to_path_buf();

    // On Linux, the executable directory may be read-only (e.g. AppImage).
    // Fall back to a writable location in the user's home directory.
    #[cfg(not(target_os = "windows"))]
    {
        use std::fs;
        // Quick writability check
        let test_path = exe_dir.join(".write_test");
        match fs::File::create(&test_path) {
            Ok(_) => { let _ = fs::remove_file(&test_path); }
            Err(_) => {
                // Use XDG data dir or ~/.local/share fallback
                let data_dir = env::var("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| {
                        PathBuf::from(env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
                            .join(".local/share")
                    })
                    .join("BD2ModManager");
                let _ = fs::create_dir_all(&data_dir);
                return data_dir;
            }
        }
    }

    exe_dir
}

pub fn get_default_staging_dir() -> PathBuf {
    println!("returning default");
    let exec_dir = get_executable_dir();
    exec_dir.join("mods").to_path_buf()
}

pub fn get_default_profiles_dir(app: &AppHandle, on_executable_dir: bool) -> PathBuf {
    // receives (on_executable_dir: bool), if true then return executable_dir/profiles, if not appdata/profiles

    if on_executable_dir {
        let exec_dir = get_executable_dir();
        exec_dir
    } else {
        match app
            .app_handle()
            .path()
            .resolve("profiles", BaseDirectory::AppData)
        {
            Ok(path) => path,
            Err(_error) => {
                let exec_dir = get_executable_dir();
                exec_dir
            }
        }
    }
}

pub fn get_config_file_path() -> PathBuf {
    let exec_dir = get_executable_dir();
    exec_dir.join("config.json").to_path_buf()
}

pub fn get_staging_dir(config: &BD2Config) -> PathBuf {
    match &config.staging_directory {
        Some(path) => return PathBuf::from(path),
        None => return get_default_staging_dir(),
    }
}

pub fn get_mod_preview_path(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(path) = app
        .app_handle()
        .path()
        .resolve("tools", BaseDirectory::AppData)
    {
        #[cfg(target_os = "windows")]
        { Some(path.join("BD2ModPreview.exe").to_path_buf()) }

        #[cfg(not(target_os = "windows"))]
        { Some(path.join("bd2modpreview").to_path_buf()) }
    } else {
        None
    }
}

pub fn get_7zip_path(app: &AppHandle) -> Option<PathBuf> {
    // 7z extraction is handled by the sevenz-rust2 crate at the Rust level.
    // The external 7z.exe binary is only used on Windows.
    #[cfg(not(target_os = "windows"))]
    { let _ = app; return None; }

    #[cfg(target_os = "windows")]
    {
        if let Ok(path) = app
            .app_handle()
            .path()
            .resolve("tools", BaseDirectory::AppData)
        {
            Some(path.join("7z.exe").to_path_buf())
        } else {
            None
        }
    }
}

pub fn get_temp_dir() -> PathBuf {
    let exec_dir = get_executable_dir();
    exec_dir.join("temp").to_path_buf()
}

pub fn is_dir_empty(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => true, 
    }
}