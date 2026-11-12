//! Bounded reads of configuration files selected for import.
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
pub struct TextFile {
    name: String,
    bytes: Vec<u8>,
}

#[tauri::command]
pub async fn read_import_file(path: PathBuf) -> Result<TextFile, String> {
    super::background(move || {
        use std::io::Read;
        let mut bytes = vec![];
        std::fs::File::open(&path)
            .map_err(super::text)?
            .take(512 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(super::text)?;
        if bytes.len() > 512 * 1024 {
            return Err("配置文件超过 512 KB".into());
        }
        Ok(TextFile {
            name: path
                .file_name()
                .ok_or("文件名无效")?
                .to_string_lossy()
                .into_owned(),
            bytes,
        })
    })
    .await
}
