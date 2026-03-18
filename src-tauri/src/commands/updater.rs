use crate::updater::{self};
use bd2modmanager_lib::utils::path::get_mod_preview_path;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Deserialize, Clone, Serialize)]
struct UpdateInfo {
    version: String,
    download_url: String,
}

#[tauri::command]
pub async fn check_for_app_update(app_handle: AppHandle) {
    app_handle.emit("update:app:checking", ()).ok();
    match updater::app::get_app_latest_version(&app_handle).await {
        Ok((latest_version, download_url)) => {
            let local_version = updater::app::get_app_version(&app_handle);

            let local_ver = match semver::Version::parse(&local_version) {
                Ok(v) => v,
                Err(e) => {
                    app_handle.emit("update:app:error", e.to_string()).ok();
                    log::error!("Invalid local version: {e}");
                    return;
                }
            };

            if latest_version <= local_ver {
                app_handle.emit("update:app:uptodate", ()).ok();
                return;
            }

            let update_info = UpdateInfo {
                version: latest_version.to_string(),
                download_url,
            };

            if let Err(e) = app_handle.emit("update:app:available", update_info) {
                app_handle.emit("update:app:error", e.to_string()).ok();
                log::error!("Emit failed: {e}");
            }
        }
        Err(e) => {
            log::error!("Failed to check update: {e}");
            app_handle.emit("update:app:error", e.to_string()).ok();
        }
    }
}

#[tauri::command]
pub fn get_mod_preview_version(app_handle: tauri::AppHandle) -> Option<String> {
    updater::mod_preview::get_mod_preview_version(app_handle)
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAvailablePayload {
    latest_version: String,
    download_url: String,
}

#[tauri::command]
pub async fn check_for_mod_preview_update(
    app_handle: AppHandle,
) -> Result<UpdateAvailablePayload, String> {
    let (latest_version, _download_url) =
        updater::mod_preview::check_for_update(app_handle).await?;
    if let Some(download_url) = _download_url {
        Ok(UpdateAvailablePayload {
            latest_version: latest_version.unwrap_or_default(),
            download_url,
        })
    } else {
        Err("No update available".to_string())
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct ProgressPayload {
    version: String,
    progress: u8,
}

#[tauri::command]
pub async fn update_mod_preview(app_handle: AppHandle) {
    app_handle.emit("update:modPreview:checking", ()).ok();

    let result = updater::mod_preview::check_for_update(app_handle.clone()).await;

    match result {
        Ok((Some(latest_version), Some(download_url))) => {
            app_handle
                .emit(
                    "update:modPreview:available",
                    (latest_version.clone(), download_url.clone()),
                )
                .ok();

            let dest_path = match get_mod_preview_path(&app_handle) {
                Some(p) => p,
                None => {
                    app_handle
                        .emit(
                            "update:modPreview:error",
                            "Failed to resolve mod preview path",
                        )
                        .ok();
                    return;
                }
            };

            if let Some(parent) = dest_path.parent() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    app_handle
                        .emit(
                            "update:modPreview:error",
                            format!("Failed to create directory: {e}"),
                        )
                        .ok();
                    return;
                }
            }

            let client = reqwest::Client::new();
            let response = match client
                .get(&download_url)
                .header("User-Agent", "BD2ModManager")
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    app_handle
                        .emit(
                            "update:modPreview:error",
                            format!("Download request failed: {e}"),
                        )
                        .ok();
                    return;
                }
            };

            if !response.status().is_success() {
                app_handle
                    .emit(
                        "update:modPreview:error",
                        format!("Download failed: HTTP {}", response.status()),
                    )
                    .ok();
                return;
            }

            let total_size = response.content_length().unwrap_or(0);
            let mut downloaded: u64 = 0;

            let mut file = match tokio::fs::File::create(&dest_path).await {
                Ok(f) => f,
                Err(e) => {
                    app_handle
                        .emit(
                            "update:modPreview:error",
                            format!("Failed to create file: {e}"),
                        )
                        .ok();
                    return;
                }
            };

            let mut stream = response.bytes_stream();
            use futures_util::StreamExt;

            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        app_handle
                            .emit("update:modPreview:error", format!("Download error: {e}"))
                            .ok();
                        return;
                    }
                };

                if let Err(e) = file.write_all(&chunk).await {
                    app_handle
                        .emit("update:modPreview:error", format!("Write error: {e}"))
                        .ok();
                    return;
                }

                downloaded += chunk.len() as u64;

                let progress = if total_size > 0 {
                    (downloaded as f64 / total_size as f64 * 100.0) as u8
                } else {
                    0
                };

                app_handle
                    .emit(
                        "update:modPreview:downloading",
                        ProgressPayload {
                            version: latest_version.clone(),
                            progress,
                        },
                    )
                    .ok();
            }

            // On Linux, make the downloaded binary executable
            #[cfg(not(target_os = "windows"))]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(metadata) = std::fs::metadata(&dest_path) {
                    let mut perms = metadata.permissions();
                    perms.set_mode(0o755);
                    let _ = std::fs::set_permissions(&dest_path, perms);
                }
            }

            app_handle
                .emit("update:modPreview:downloaded", latest_version.clone())
                .ok();
        }

        Ok((None, None)) => {
            app_handle.emit("update:modPreview:uptodate", ()).ok();
        }

        Ok(_) => {
            app_handle
                .emit("update:modPreview:error", "Invalid update state")
                .ok();
        }

        Err(err) => {
            app_handle.emit("update:modPreview:error", err).ok();
        }
    }
}

#[tauri::command]
pub async fn update_game_data(app_handle: AppHandle) -> Result<(), String> {
    updater::game_data::update_characters(app_handle).await
    // app_handle.emit("update:gameData:checking", ()).ok();
    // println!("Checking for game data updates...");

    // match updater::game_data::update_characters().await {
    //     Ok(_) => {
    //         app_handle.emit("update:gameData:updated", ()).ok();
    //         Ok(())
    //     }
    //     Err(e) => {
    //         log::error!("Game data update failed: {e}");
    //         app_handle.emit("update:gameData:error", e.clone()).ok();
    //         Err(e)
    //     }
    // }
}
