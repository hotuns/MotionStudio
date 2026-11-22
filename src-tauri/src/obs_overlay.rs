//! Read-only OBS browser source, independent of the desktop overlay window.
use crate::{client::Snapshot, controller_overlay, text, AppState};
use http_body_util::Full;
use hyper::{
    body::Bytes, header, server::conn::http1, service::service_fn, Method, Request, Response,
    StatusCode,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use motion_core::storage::{ControllerOverlaySettings, Language, OverlayContent};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    convert::Infallible,
    io,
    net::{Ipv4Addr, SocketAddr, TcpListener},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tokio::{sync::oneshot, task::JoinSet};

const PORT: u16 = 39741;
const VIEW: &str = "/overlay";
const FRAME: &str = "/overlay/frame";
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' blob:; img-src 'self' data: blob:; font-src 'self'; worker-src 'self' blob:; frame-ancestors 'none'; base-uri 'none'";
type Reply = Response<Full<Bytes>>;

fn response(code: StatusCode, mime: &str, bytes: Vec<u8>, immutable: bool) -> Reply {
    Response::builder()
        .status(code)
        .header(header::CONTENT_TYPE, mime)
        .header(
            header::CACHE_CONTROL,
            if immutable {
                "private, max-age=31536000, immutable"
            } else {
                "no-store"
            },
        )
        .header(header::CONTENT_SECURITY_POLICY, CSP)
        .header("X-Content-Type-Options", "nosniff")
        .header("Cross-Origin-Resource-Policy", "same-origin")
        .body(Full::new(Bytes::from(bytes)))
        .unwrap()
}

fn empty(code: StatusCode) -> Reply {
    response(code, "text/plain", vec![], false)
}

/// The browser receives only the controls drawn by the selected views.
fn frame(settings: &ControllerOverlaySettings, language: Language, snapshot: &Snapshot) -> Value {
    let age = snapshot
        .status_seen
        .map(|at| at.elapsed().as_millis() as u64);
    let status = &snapshot.status;
    let live = controller_overlay::live(
        status,
        snapshot.engine_ready,
        age.is_some_and(|ms| ms < 600),
    );
    let controller = settings.content != OverlayContent::KeyMouse;
    let keys = settings.content != OverlayContent::Controller;
    let device = status.device.as_ref().filter(|_| live && controller).map(|d| json!({
        "id": d.id, "name": d.name, "vendor": d.vendor, "product": d.product, "gyro": d.gyro, "buttons": d.buttons
    }));
    let status = live.then(|| {
        let preview = status.output_preview.as_ref()
            .filter(|_| keys && status.emitting && !status.binding_capture && !status.ui_navigation)
            .map(|p| json!({ "key_mouse": p.key_mouse, "mouse": p.mouse }));
        json!({
            "state": status.state, "emitting": status.emitting, "output_preview": preview, "reconnects": status.reconnects,
            "buttons": if controller { &status.buttons[..] } else { &[] },
            "axes": if controller { &status.axes[..] } else { &[] },
            "touches": controller.then_some(&status.touches),
            "motion": { "orientation_quat": if controller && settings.follow_motion { status.motion.orientation_quat } else { None } }
        })
    });
    json!({
        "content": settings.content, "opacity": settings.opacity, "follow_motion": settings.follow_motion,
        "size": controller_overlay::logical_size(settings), "language": language, "device": device,
        "status": status, "status_age_ms": age, "restart_count": snapshot.restart_count
    })
}

fn asset_path(path: &str) -> Option<(&str, &'static str)> {
    if path == VIEW {
        return Some(("obs.html", "text/html; charset=utf-8"));
    }
    let name = path.strip_prefix("/assets/")?;
    let (stem, extension) = name.rsplit_once('.')?;
    if stem.is_empty()
        || stem.starts_with('.')
        || stem.contains("..")
        || !stem
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
    {
        return None;
    }
    let mime = match extension {
        "js" => "application/javascript",
        "css" => "text/css",
        "glb" => "model/gltf-binary",
        "woff2" => "font/woff2",
        _ => return None,
    };
    Some((&path[1..], mime))
}

fn content<R: tauri::Runtime>(app: &AppHandle<R>, path: &str) -> Reply {
    if path == FRAME {
        let state = app.state::<AppState>();
        let (overlay, language) = {
            let Ok(settings) = state.settings.lock() else {
                return empty(StatusCode::SERVICE_UNAVAILABLE);
            };
            (settings.controller_overlay.clone(), settings.language)
        };
        if !overlay.obs_enabled {
            return empty(StatusCode::NO_CONTENT);
        }
        let Ok(snapshot) = state.client.snapshot.lock() else {
            return empty(StatusCode::SERVICE_UNAVAILABLE);
        };
        let view = frame(&overlay, language, &snapshot);
        drop(snapshot);
        return response(
            StatusCode::OK,
            "application/json",
            view.to_string().into_bytes(),
            false,
        );
    }
    static_content(path, |key| {
        app.asset_resolver()
            .get_for_scheme(key.to_owned(), false)
            .map(|asset| asset.bytes)
    })
}

fn static_content(path: &str, load: impl FnOnce(&str) -> Option<Vec<u8>>) -> Reply {
    let Some((key, mime)) = asset_path(path) else {
        return empty(StatusCode::NOT_FOUND);
    };
    let Some(bytes) = load(key) else {
        return empty(StatusCode::NOT_FOUND);
    };
    // Packaged apps can fall back to index.html for a missing asset. Detect the
    // actual HTML bytes since Tauri can also label a valid binary GLB as HTML.
    if key == "obs.html" {
        if !String::from_utf8_lossy(&bytes).contains("id=\"obs-root\"") {
            return empty(StatusCode::NOT_FOUND);
        }
    } else if bytes.trim_ascii_start().starts_with(b"<") {
        return empty(StatusCode::NOT_FOUND);
    }
    response(StatusCode::OK, mime, bytes, path != VIEW)
}

fn route<B>(req: &Request<B>, host: &str, serve: &impl Fn(&str) -> Reply) -> Reply {
    if req.method() != Method::GET {
        return empty(StatusCode::METHOD_NOT_ALLOWED);
    }
    let headers = req.headers();
    if headers.get(header::HOST).and_then(|v| v.to_str().ok()) != Some(host)
        || headers
            .get(header::ORIGIN)
            .is_some_and(|v| v.as_bytes() != format!("http://{host}").as_bytes())
        || headers
            .get("sec-fetch-site")
            .is_some_and(|v| v == "cross-site")
    {
        return empty(StatusCode::FORBIDDEN);
    }
    let path = req.uri().path();
    if path == FRAME && !headers.get("x-motion-overlay").is_some_and(|v| v == "1") {
        return empty(StatusCode::FORBIDDEN);
    }
    serve(path)
}

struct Listener {
    address: SocketAddr,
    stop: oneshot::Sender<()>,
    worker: thread::JoinHandle<()>,
}

impl Listener {
    fn start(port: u16, serve: impl Fn(&str) -> Reply + Send + Sync + 'static) -> io::Result<Self> {
        let socket = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
        socket.set_nonblocking(true)?;
        let address = socket.local_addr()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()?;
        let listener = {
            let _guard = runtime.enter();
            tokio::net::TcpListener::from_std(socket)?
        };
        let (stop, mut stopped) = oneshot::channel();
        let run = async move {
            let host = Arc::new(address.to_string());
            let serve = Arc::new(serve);
            let mut clients = JoinSet::new();
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    Some(_) = clients.join_next(), if !clients.is_empty() => {},
                    accepted = listener.accept(), if clients.len() < 16 => {
                        let Ok((stream, _)) = accepted else {
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            continue;
                        };
                        let _ = stream.set_nodelay(true);
                        let (host, serve) = (host.clone(), serve.clone());
                        clients.spawn(async move {
                            let service = service_fn(move |req| {
                                let reply = route(&req, &host, &*serve);
                                std::future::ready(Ok::<_, Infallible>(reply))
                            });
                            let _ = http1::Builder::new().timer(TokioTimer::new())
                                .header_read_timeout(Duration::from_secs(8)).max_buf_size(16 * 1024)
                                .serve_connection(TokioIo::new(stream), service).await;
                        });
                    }
                }
            }
            clients.shutdown().await;
        };
        let worker = thread::Builder::new()
            .name("obs-overlay".into())
            .spawn(move || runtime.block_on(run))?;
        Ok(Self {
            address,
            stop,
            worker,
        })
    }

    fn close(self) {
        let _ = self.stop.send(());
        let _ = self.worker.join();
    }
}

#[derive(Default)]
pub struct RecordingSource(Mutex<Option<Listener>>);

impl RecordingSource {
    pub fn configure(&self, app: &AppHandle, enabled: bool) -> Result<(), String> {
        let mut current = self.0.lock().map_err(text)?;
        if enabled {
            if current.as_ref().is_some_and(|s| !s.worker.is_finished()) {
                return Ok(());
            }
            if content(app, VIEW).status() != StatusCode::OK {
                return Err("缺少 OBS 录制页面，请重新构建或安装 Motion Studio。".into());
            }
            let app = app.clone();
            *current = Some(
                Listener::start(PORT, move |path| content(&app, path)).map_err(|error| {
                    format!("无法启动 OBS 录制源，请检查本机端口 {PORT}。{error}")
                })?,
            );
        } else if let Some(listener) = current.take() {
            listener.close();
        }
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(listener) = self.0.lock().unwrap().take() {
            listener.close();
        }
    }
}

#[derive(Serialize)]
pub struct SourceInfo {
    url: String,
    size: [u32; 2],
}

#[tauri::command]
pub fn obs_source_info(
    window: WebviewWindow,
    state: State<'_, AppState>,
    source: State<'_, RecordingSource>,
    copy: bool,
) -> Result<SourceInfo, String> {
    if window.label() != "main" {
        return Err("请在 Studio 查看录制地址".into());
    }
    let size =
        controller_overlay::logical_size(&state.settings.lock().map_err(text)?.controller_overlay);
    let active = source.0.lock().map_err(text)?;
    let listener = active
        .as_ref()
        .filter(|s| !s.worker.is_finished())
        .ok_or_else(|| format!("OBS 录制源未就绪。请检查本机端口 {PORT}，再重新启用录制模式。"))?;
    let url = format!("http://{}{VIEW}", listener.address);
    if copy {
        clipboard_win::set_clipboard_string(&url).map_err(|_| "无法写入剪贴板")?;
    }
    Ok(SourceInfo {
        url,
        size: size.map(|v| v.ceil() as u32),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpStream,
        time::Instant,
    };

    fn fixture() -> Snapshot {
        let mut snapshot = Snapshot {
            engine_ready: true,
            status_seen: Some(Instant::now()),
            ..Default::default()
        };
        let status = &mut snapshot.status;
        status.state = "running".into();
        status.emitting = true;
        status.profile_name = Some("private profile".into());
        status.device = Some(
            serde_json::from_value(json!({
                "id":7, "name":"Synthetic", "vendor":0, "product":0, "path":"private device path",
                "gamepad":true, "gyro":true, "accelerometer":true, "buttons":["south"]
            }))
            .unwrap(),
        );
        status.buttons = vec![true];
        status.output_preview = Some(
            serde_json::from_value(json!({
                "generation":1, "mouse":[], "pad":null, "key_mouse":{"held":["key:W"],"recent":[]}
            }))
            .unwrap(),
        );
        snapshot
    }

    #[test]
    fn recording_only_exposes_requested_controls_and_clears_inactive_output() {
        let mut snapshot = fixture();
        let mut config = ControllerOverlaySettings {
            obs_enabled: true,
            ..Default::default()
        };
        assert!(!config.enabled);
        let keys = frame(&config, Language::En, &snapshot);
        assert_eq!(
            keys["status"]["output_preview"]["key_mouse"]["held"][0],
            "key:W"
        );
        assert_eq!(keys["status"]["buttons"], json!([]));
        assert!(keys["device"].is_null());
        config.content = OverlayContent::Both;
        let both = frame(&config, Language::En, &snapshot);
        assert_eq!(both["status"]["buttons"], json!([true]));
        assert_eq!(both["device"]["id"], 7);
        assert!(!both.to_string().contains("private"));
        config.enabled = true;
        assert_eq!(
            frame(&config, Language::En, &snapshot)["status"],
            both["status"]
        );
        config.content = OverlayContent::Controller;
        assert!(frame(&config, Language::En, &snapshot)["status"]["output_preview"].is_null());
        config.content = OverlayContent::Both;
        snapshot.status.binding_capture = true;
        assert!(frame(&config, Language::En, &snapshot)["status"]["output_preview"].is_null());
        snapshot.status.binding_capture = false;
        snapshot.status.ui_navigation = true;
        assert!(frame(&config, Language::En, &snapshot)["status"]["output_preview"].is_null());
        snapshot.status.ui_navigation = false;
        snapshot.status.emitting = false;
        assert!(frame(&config, Language::En, &snapshot)["status"]["output_preview"].is_null());
        snapshot.status_seen = Some(Instant::now() - Duration::from_secs(1));
        assert!(frame(&config, Language::En, &snapshot)["status"].is_null());
    }

    #[test]
    fn only_viewer_and_built_assets_are_routable() {
        assert_eq!(asset_path(VIEW).unwrap().0, "obs.html");
        let model = static_content("/assets/dualsense-123.glb", |_| Some(b"glTF".to_vec()));
        assert_eq!(model.status(), StatusCode::OK);
        assert_eq!(model.headers()[header::CONTENT_TYPE], "model/gltf-binary");
        let fallback = static_content("/assets/missing.js", |_| Some(b" <!doctype html>".to_vec()));
        assert_eq!(fallback.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            asset_path("/assets/dualsense-123.glb").unwrap().1,
            "model/gltf-binary"
        );
        assert_eq!(
            asset_path("/assets/DevicePage.module-ABC.js").unwrap().1,
            "application/javascript"
        );
        for path in [
            "/",
            "/index.html",
            "/assets/../settings.json",
            "/assets/%2fkey.js",
            "/assets/a.js.map",
        ] {
            assert!(asset_path(path).is_none());
        }
    }

    #[test]
    #[ignore = "requires the built OBS viewer from npm run build:web"]
    fn built_obs_page_and_model_are_served_by_tauri_assets() {
        use http_body_util::BodyExt;
        let app = tauri::test::mock_builder()
            .build(tauri::generate_context!())
            .unwrap();
        let read = |path| {
            let reply = content(app.handle(), path);
            assert_eq!(reply.status(), StatusCode::OK, "{path}");
            tauri::async_runtime::block_on(reply.into_body().collect())
                .unwrap()
                .to_bytes()
        };
        let page = read(VIEW);
        let html = std::str::from_utf8(&page).unwrap();
        assert!(html.contains("id=\"obs-root\""));
        let imports: Vec<_> = html
            .split('"')
            .filter(|s| s.starts_with("/assets/"))
            .collect();
        assert!(!imports.is_empty());
        for path in imports {
            assert!(!read(path).is_empty());
        }
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../dist/assets");
        let model = std::fs::read_dir(directory)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .find(|name| name.starts_with("dualsense-") && name.ends_with(".glb"))
            .unwrap();
        assert!(read(&format!("/assets/{model}")).starts_with(b"glTF"));
        assert_eq!(
            content(app.handle(), "/assets/missing.js").status(),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn obs_reads_loopback_then_disable_releases_port_and_open_connections() {
        let serve = |path: &str| {
            if path == FRAME {
                let config = ControllerOverlaySettings {
                    obs_enabled: true,
                    ..Default::default()
                };
                response(
                    StatusCode::OK,
                    "application/json",
                    frame(&config, Language::En, &fixture())
                        .to_string()
                        .into_bytes(),
                    false,
                )
            } else {
                empty(StatusCode::NOT_FOUND)
            }
        };
        let source = Listener::start(0, serve).unwrap();
        let address = source.address;
        assert!(address.ip().is_loopback());
        assert!(Listener::start(address.port(), serve).is_err());
        let get = |authority: &str, extra: &str| {
            let mut socket = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            write!(
                socket,
                "GET {FRAME} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n{extra}\r\n"
            )
            .unwrap();
            let mut reply = String::new();
            socket.read_to_string(&mut reply).unwrap();
            reply
        };
        let header = "X-Motion-Overlay: 1\r\n";
        let reply = get(&address.to_string(), header);
        assert!(reply.starts_with("HTTP/1.1 200") && reply.contains("key:W"));
        assert!(get(&address.to_string(), "").starts_with("HTTP/1.1 403"));
        assert!(get("example.test", header).starts_with("HTTP/1.1 403"));
        assert!(get(
            &address.to_string(),
            &format!("{header}Origin: https://example.test\r\n")
        )
        .starts_with("HTTP/1.1 403"));
        let _idle = TcpStream::connect(address).unwrap();
        source.close();
        Listener::start(address.port(), serve).unwrap().close();
    }
}
