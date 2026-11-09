//! In-app text-file picker; reachable with the same gamepad focus controls.
use serde::Serialize;
use std::path::PathBuf;
#[derive(Serialize)]
pub struct Place {
    name: String,
    path: PathBuf,
}
#[derive(Serialize)]
pub struct FileItem {
    name: String,
    path: PathBuf,
    directory: bool,
}
#[derive(Serialize)]
pub struct Listing {
    directory: PathBuf,
    parent: Option<PathBuf>,
    places: Vec<Place>,
    items: Vec<FileItem>,
    truncated: bool,
}
#[derive(Serialize)]
pub struct TextFile {
    name: String,
    bytes: Vec<u8>,
}
#[tauri::command]
pub async fn browse_import_files(directory: Option<PathBuf>) -> Result<Listing, String> {
    super::background(move || {
        let user = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .ok_or("无法找到用户目录")?;
        let mut places: Vec<Place> = [
            user.join("Desktop"),
            user.join("Documents"),
            user.join("Downloads"),
            user.clone(),
        ]
        .into_iter()
        .zip(["桌面", "文档", "下载", "用户目录"])
        .filter(|(p, _)| p.is_dir())
        .map(|(path, name)| Place {
            name: name.into(),
            path,
        })
        .collect();
        for drive in b'A'..=b'Z' {
            let path = PathBuf::from(format!("{}:\\", drive as char));
            if path.is_dir() {
                places.push(Place {
                    name: format!("{} 盘", drive as char),
                    path,
                });
            }
        }
        let directory = directory
            .unwrap_or(user)
            .canonicalize()
            .map_err(super::text)?;
        if !directory.is_dir() {
            return Err("请选择文件夹".into());
        }
        let mut items = vec![];
        let mut truncated = false;
        for entry in std::fs::read_dir(&directory).map_err(super::text)? {
            let Ok(entry) = entry else { continue };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let supported = path.extension().and_then(|v| v.to_str()).is_some_and(|v| {
                matches!(
                    v.to_ascii_lowercase().as_str(),
                    "txt" | "jsm" | "cfg" | "conf" | "toml"
                )
            });
            if kind.is_dir() || kind.is_file() && supported {
                items.push(FileItem {
                    name,
                    path,
                    directory: kind.is_dir(),
                });
            }
            if items.len() >= 1000 {
                truncated = true;
                break;
            }
        }
        items.sort_by(|a, b| {
            b.directory
                .cmp(&a.directory)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(Listing {
            parent: directory.parent().map(|p| p.to_path_buf()),
            directory,
            places,
            items,
            truncated,
        })
    })
    .await
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
