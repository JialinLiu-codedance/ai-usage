use serde::{de::DeserializeOwned, Serialize};
use std::{fs, path::PathBuf};
use tauri::{AppHandle, Manager};

pub fn app_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法定位应用目录: {error}"))?;
    fs::create_dir_all(&path).map_err(|error| format!("无法创建应用目录: {error}"))?;
    Ok(path)
}

pub fn read_json<T: DeserializeOwned>(
    app: &AppHandle,
    filename: &str,
) -> Result<Option<T>, String> {
    let path = app_dir(app)?.join(filename);
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path).map_err(|error| format!("读取文件失败: {error}"))?;
    let value =
        serde_json::from_str::<T>(&content).map_err(|error| format!("解析文件失败: {error}"))?;
    Ok(Some(value))
}

pub fn write_json<T: Serialize>(app: &AppHandle, filename: &str, value: &T) -> Result<(), String> {
    let path = app_dir(app)?.join(filename);
    let content =
        serde_json::to_string_pretty(value).map_err(|error| format!("序列化文件失败: {error}"))?;
    fs::write(path, content).map_err(|error| format!("写入文件失败: {error}"))
}
