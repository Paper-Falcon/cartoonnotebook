use crate::{anilist, bangumi, db};
use axum::{
    body::Body,
    extract::{ConnectInfo, Multipart, Path, Query, State},
    http::{header, HeaderMap, HeaderName, Request, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, patch, post},
    Json, Router,
};
use rand::Rng;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;
use tower_http::{cors::CorsLayer, services::ServeFile};

pub const INDEX_JSON: &[u8] = include_bytes!("../assets/index.min.json");
pub const DEFAULT_PORT: u16 = 8787;

/// 前端构建产物直接嵌进二进制：打包版和手机访问都用这一份
#[derive(RustEmbed)]
#[folder = "../dist"]
struct Assets;

/// 全局状态：一个 SQLite 连接 + 数据目录 + 复用的 HTTP 客户端
pub struct AppState {
    pub db: Mutex<rusqlite::Connection>,
    pub data_dir: PathBuf,
    pub http: reqwest::Client,
    /// 手机端登录用的长期口令（存数据库，重启不变）
    pub phone_token: String,
    /// 一次性配对码 + 生成时间
    pub pair: Mutex<Option<(String, Instant)>>,
    /// 扫到的 Wallpaper Engine 壁纸（首次访问时扫描，可手动刷新）
    pub wallpapers: Mutex<Vec<Wallpaper>>,
    pub port: u16,
}

impl AppState {
    pub fn new(data_dir: PathBuf, port: u16) -> anyhow::Result<Self> {
        std::fs::create_dir_all(data_dir.join("covers"))?;
        std::fs::create_dir_all(data_dir.join("media"))?;
        let conn = db::open(&data_dir)?;

        let phone_token = match db::get_setting(&conn, "phone_token")? {
            Some(t) if !t.is_empty() => t,
            _ => {
                let t: String = rand::thread_rng()
                    .sample_iter(&rand::distributions::Alphanumeric)
                    .take(40)
                    .map(char::from)
                    .collect();
                db::set_setting(&conn, "phone_token", &t)?;
                t
            }
        };

        // 跟随 Windows 系统代理：用户开了代理（系统代理模式）时，
        // AniList / Bangumi 这类被墙的在线源才能通
        let mut builder =
            reqwest::Client::builder().timeout(Duration::from_secs(20));
        #[cfg(windows)]
        if let Some(proxy) = system_proxy() {
            println!("[net] 使用系统代理 {proxy}");
            match reqwest::Proxy::all(&proxy) {
                Ok(p) => builder = builder.proxy(p),
                Err(e) => eprintln!("[net] 系统代理解析失败: {e}"),
            }
        }
        let http = builder.build()?;
        Ok(Self {
            db: Mutex::new(conn),
            data_dir,
            http,
            phone_token,
            pair: Mutex::new(None),
            wallpapers: Mutex::new(Vec::new()),
            port,
        })
    }

    /// 生成（或复用 5 分钟内的）配对码
    pub fn fresh_pair_code(&self) -> String {
        let mut slot = self.pair.lock().expect("pair mutex poisoned");
        if let Some((code, at)) = slot.as_ref() {
            if at.elapsed() < Duration::from_secs(300) {
                return code.clone();
            }
        }
        let code = format!("{:06}", rand::thread_rng().gen_range(0..1_000_000));
        *slot = Some((code.clone(), Instant::now()));
        code
    }

    /// 校验配对码；成功即作废（一次性）
    pub fn consume_pair_code(&self, code: &str) -> bool {
        let mut slot = self.pair.lock().expect("pair mutex poisoned");
        let ok = matches!(
            slot.as_ref(),
            Some((c, at)) if at.elapsed() < Duration::from_secs(300) && c == code
        );
        if ok {
            *slot = None;
        }
        ok
    }
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{:#}", err.into()),
        )
    }
}

pub fn resolve_data_dir() -> PathBuf {
    if let Ok(v) = std::env::var("CARTOON_DATA_DIR") {
        if !v.trim().is_empty() {
            return PathBuf::from(v);
        }
    }
    let mut cur = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    loop {
        if cur.join("package.json").exists() && cur.join("src-tauri").exists() {
            return cur.join("data");
        }
        if !cur.pop() {
            break;
        }
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata).join("CartoonNotebook").join("data");
    }
    PathBuf::from("data")
}

pub fn resolve_port() -> u16 {
    std::env::var("CARTOON_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/index", get(index_json))
        .route("/api/bgm/search", get(bgm_search))
        .route("/api/works", get(list_works).post(create_work))
        .route(
            "/api/works/:id",
            get(get_work).patch(patch_work).delete(delete_work),
        )
        .route("/api/cover/:name", get(cover))
        .route("/api/works/:id/media", get(list_media))
        .route("/api/works/:id/media/upload", post(upload_media))
        .route("/api/works/:id/media/import", post(import_media))
        .route(
            "/api/media/:id",
            get(get_media).patch(patch_media).delete(delete_media),
        )
        .route("/api/media/:id/file", get(media_file))
        .route("/api/media/:id/thumb", get(media_thumb))
        .route("/api/tags", get(list_tags).post(create_tag))
        .route("/api/tags/:id", patch(rename_tag).delete(delete_tag))
        .route("/api/works/:id/tags", post(add_work_tag))
        .route("/api/works/:id/tags/:tag_id", delete(remove_work_tag))
        .route("/api/export", get(export_library))
        .route("/api/export/download", get(export_download))
        .route("/api/restore", post(restore_backup))
        .route("/api/wallpapers", get(list_wallpapers))
        .route("/api/wallpapers/:id/preview", get(wallpaper_preview))
        .route("/api/wallpapers/:id/video", get(wallpaper_video))
        .route("/api/wallpapers/:id/entry", get(wallpaper_entry))
        .route("/api/wallpapers/:id/entry/", get(wallpaper_entry))
        .route(
            "/api/wallpapers/:id/entry/*rest",
            get(wallpaper_entry_asset),
        )
        .route("/api/wallpapers/:id/file/*rest", get(wallpaper_entry_asset))
        .route("/api/wallpaper", get(get_wallpaper).patch(set_wallpaper))
        .route("/api/appearance", get(get_appearance).put(set_appearance))
        .route("/api/pair", get(pair_info))
        .route("/pair", get(pair_landing))
        .route("/diag", get(diag_page))
        .fallback(spa)
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

pub async fn serve(state: Arc<AppState>, host: &str, port: u16) -> anyhow::Result<()> {
    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("[server] http://{addr}");
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// 本机（桌面应用）直接放行；局域网设备必须带 cookie
async fn guard(
    State(st): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if !req.uri().path().starts_with("/api") {
        return next.run(req).await;
    }
    if addr.ip().is_loopback() || has_valid_token(&st, req.headers()) {
        return next.run(req).await;
    }
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        "这台设备还没配对，请用电脑上的二维码打开一次",
    )
    .into_response()
}

fn has_valid_token(st: &AppState, headers: &HeaderMap) -> bool {
    let Some(raw) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    raw.split(';')
        .filter_map(|part| part.trim().split_once('='))
        .any(|(k, v)| k == "cn_token" && v == st.phone_token)
}

fn mime_of(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// 静态资源 + history 回退到 index.html
async fn spa(uri: Uri) -> Response {
    let raw = uri.path().trim_start_matches('/');
    // 未知接口返回 JSON 404，不能让 SPA 兜底接管成一段 HTML
    if raw == "api" || raw.starts_with("api/") {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "接口不存在" })),
        )
            .into_response();
    }
    let path = if raw.is_empty() { "index.html" } else { raw };

    // 带内容哈希的产物放心永久缓存；index.html 必须每次校验，
    // 否则手机浏览器会拿旧 HTML 引用已不存在的资源 → 白屏
    if let Some(file) = Assets::get(path) {
        let cache = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return (
            [
                (header::CONTENT_TYPE, mime_of(path)),
                (header::CACHE_CONTROL, cache),
            ],
            file.data.into_owned(),
        )
            .into_response();
    }
    match Assets::get("index.html") {
        Some(file) => {
            // 手机端排障：注入加载诊断条（可见地暴露 UA/资源错误/启动状态）
            let html = String::from_utf8_lossy(&file.data).to_string();
            let diag = r#"<script>
(function(){
  function boot(){
    var s=document.getElementById('__diag');
    if(!s){
      s=document.createElement('div');
      s.id='__diag';
      s.style.cssText='position:fixed;bottom:0;left:0;right:0;z-index:999999;background:#fffbe8;color:#8a2b1d;font:10px/1.5 monospace;padding:4px 6px;max-height:38%;overflow:auto;border-top:1px solid #e0d0b0;text-align:left';
      (document.body||document.documentElement).appendChild(s);
    }
    return s;
  }
  function log(m){
    var s=boot();
    var d=document.createElement('div');
    d.textContent='• '+m;
    s.appendChild(d);
  }
  log('UA: '+navigator.userAgent.slice(0,80));
  log('HTML: '+document.readyState);
  window.addEventListener('error',function(e){
    if(e.target&&e.target.src){log('资源加载失败: '+e.target.src.slice(-60));}
    else{log('脚本错误: '+(e.message||'未知'));}
  },true);
  window.addEventListener('unhandledrejection',function(e){
    log('Promise拒绝: '+e.reason);
  });
  var n=0;
  var t=setInterval(function(){
    n++;
    if(window.__APP_BOOTED){log('应用启动成功 ✓');clearInterval(t);setTimeout(function(){var x=document.getElementById('__diag');if(x)x.remove();},4000);}
    else if(n>40){log('应用 12 秒仍未启动——JS 加载或执行失败');clearInterval(t);}
  },300);
  window.__DIAG_LOG=log;
})();
</script>"#;
            let html = if html.contains("__TAURI_INTERNALS__") || html.contains("</body>") {
                html.replace("</body>", &format!("{diag}</body>"))
            } else {
                html + &diag
            };
            (
                [
                    (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                    (header::CACHE_CONTROL, "no-cache"),
                ],
                html.into_bytes(),
            )
                .into_response()
        }
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "前端资源没打进包里，先跑一次 npm run build",
        )
            .into_response(),
    }
}

fn default_route_ip() -> Option<String> {
    // 不会真的发包，只是让系统选出默认路由所在的网卡地址
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

/// 默认路由被 VPN 虚拟网卡抢走时，退回到 ipconfig 里找内网地址
fn private_ip_from_ipconfig() -> Option<String> {
    let out = std::process::Command::new("ipconfig").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut fallback: Option<String> = None;
    for line in text.lines() {
        let Some(pos) = line.find("IPv4") else { continue };
        let Some(colon) = line[pos..].find(':') else { continue };
        let raw = line[pos + colon + 1..].trim();
        let ip = raw.trim_end_matches(|c: char| !c.is_ascii_digit() && c != '.');
        if ip.starts_with("192.168.") {
            return Some(ip.to_string());
        }
        if fallback.is_none() && (ip.starts_with("10.") || ip.starts_with("172.")) {
            fallback = Some(ip.to_string());
        }
    }
    fallback
}

fn lan_ip() -> Option<String> {
    lan_ips().into_iter().next()
}

/// 公网地址：排除私网/回环/链路本地/虚拟 VPN（Radmin 26.x）网段。
/// 机器网卡上直接配了公网 IP 时，手机用流量也能直连。
fn is_public_ipv4(ip: &str) -> bool {
    let Ok(v4) = ip.parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    let o = v4.octets();
    !((o[0] == 10)
        || (o[0] == 127)
        || (o[0] == 172 && (16..=31).contains(&o[1]))
        || (o[0] == 192 && o[1] == 168)
        || (o[0] == 169 && o[1] == 254)
        || (o[0] == 100 && (64..=127).contains(&o[1]))
        || (o[0] == 26))
}

/// 多网卡机器（VMware/VPN/多网络）上，默认路由选的 IP 手机多半连不上。
/// 列出所有可能的局域网地址：默认路由 IP 打头，其余来自 ipconfig，
/// 跳过回环/链路本地/常见虚拟网卡网段。
fn lan_ips() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(ip) = default_route_ip() {
        if !ip.starts_with("127.") && !ip.starts_with("169.254.") {
            out.push(ip);
        }
    }
    for (ip, adapter) in ipconfig_ips() {
        if out.contains(&ip) {
            continue;
        }
        let virtual_adapter = ["vmware", "virtual", "vpn", "hyper-v", "loopback", "bluetooth", "miniport"]
            .iter()
            .any(|k| adapter.to_lowercase().contains(k));
        // 虚拟网卡仍列出（用户可能就在那个网段），但排到真实网卡后面
        if virtual_adapter {
            out.push(ip);
        } else if ip.starts_with("169.254.") || ip.starts_with("127.") {
            continue;
        } else {
            out.insert(1, ip);
        }
    }
    out
}

/// ipconfig 解析：返回 (IPv4, 适配器名) 列表
fn ipconfig_ips() -> Vec<(String, String)> {
    let out = match std::process::Command::new("ipconfig").output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut result: Vec<(String, String)> = Vec::new();
    let mut adapter = String::new();
    for line in text.lines() {
        let line = line.trim_end();
        if let Some(idx) = line.find(':') {
            let (head, tail) = (line[..idx].trim(), line[idx + 1..].trim());
            if head.contains("适配器") || head.to_lowercase().contains("adapter") {
                // 「以太网适配器 VMware ...:」或「adapter VMware ...:」
                let name = head
                    .replace("以太网适配器", "")
                    .replace("无线局域网适配器", "")
                    .replace("本地链接", "")
                    .replace("Ethernet adapter", "")
                    .replace("Wireless LAN adapter", "")
                    .trim()
                    .trim_end_matches(':')
                    .trim()
                    .to_string();
                adapter = name;
            }
            if tail.starts_with("192.168.")
                || tail.starts_with("10.")
                || tail.starts_with("172.")
                || tail.starts_with("26.")
                || tail.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
            {
                let ip = tail
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches(|c: char| !c.is_ascii_digit() && c != '.')
                    .to_string();
                if ip.parse::<std::net::Ipv4Addr>().is_ok() && adapter.len() > 0 {
                    result.push((ip, adapter.clone()));
                }
            }
        }
    }
    result
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairInfo {
    pub url: String,
    pub code: String,
    pub lan_ip: Option<String>,
    pub port: u16,
    pub qr: String,
    /// 多网卡时给备选地址，每个都带二维码
    pub alternates: Vec<AlternatePair>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlternatePair {
    pub ip: String,
    pub adapter: String,
    pub url: String,
    pub qr: String,
}

fn qr_svg(content: &str) -> String {
    qrcode::QrCode::new(content.as_bytes())
        .map(|code| {
            code.render::<qrcode::render::svg::Color>()
                .min_dimensions(160, 160)
                .quiet_zone(true)
                .build()
        })
        .unwrap_or_default()
}

async fn pair_info(State(st): State<Arc<AppState>>) -> Result<Json<PairInfo>, ApiError> {
    let code = st.fresh_pair_code();
    let ips = lan_ips();
    let primary = ips.first().cloned().unwrap_or_else(|| "127.0.0.1".to_string());
    let url = format!("http://{primary}:{}/pair?c={code}", st.port);
    let qr = qr_svg(&url);

    let mut alternates = Vec::new();
    for ip in ips.iter().skip(1).take(4) {
        let adapter = ipconfig_ips()
            .into_iter()
            .find(|(a_ip, _)| a_ip == ip)
            .map(|(_, name)| name)
            .unwrap_or_else(|| "未知网卡".to_string());
        let a_url = format!("http://{ip}:{}/pair?c={code}", st.port);
        let a_qr = qr_svg(&a_url);
        alternates.push(AlternatePair {
            ip: ip.clone(),
            adapter,
            url: a_url,
            qr: a_qr,
        });
    }
    // 默认路由是公网 IP 时，补一个「手机流量直连」的候选
    if let Some(first) = ips.first() {
        if is_public_ipv4(first) && !alternates.iter().any(|a| a.ip == *first) {
            let a_url = format!("http://{first}:{}/pair?c={code}", st.port);
            let a_qr = qr_svg(&a_url);
            alternates.push(AlternatePair {
                ip: first.clone(),
                adapter: "公网（手机流量也能连）".to_string(),
                url: a_url,
                qr: a_qr,
            });
        }
    }

    Ok(Json(PairInfo {
        url,
        code,
        lan_ip: Some(primary),
        port: st.port,
        qr,
        alternates,
    }))
}

#[derive(Deserialize)]
pub struct PairQuery {
    pub c: Option<String>,
}

async fn pair_landing(
    State(st): State<Arc<AppState>>,
    Query(q): Query<PairQuery>,
) -> Response {
    let code = q.c.unwrap_or_default();
    if !st.consume_pair_code(&code) {
        return (
            StatusCode::FORBIDDEN,
            "配对码无效或已过期，请在电脑上重新生成",
        )
            .into_response();
    }
    let cookie = format!(
        "cn_token={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=31536000",
        st.phone_token
    );
    // 顺手清掉这个源站的旧缓存：老版本页面曾无缓存头，
    // 手机可能存着引用已失效资源的 index.html（白屏元凶）
    let mut headers = HeaderMap::new();
    headers.insert(header::SET_COOKIE, cookie.parse().unwrap());
    headers.insert(
        HeaderName::from_static("clear-site-data"),
        "\"cache\"".parse().unwrap(),
    );
    (headers, Redirect::to("/")).into_response()
}

/// 手机端排障页：无缓存、纯内联 ES5，直接暴露每一步的真实状态
async fn diag_page() -> Response {
    let html = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>加载诊断</title></head>
<body style="font:13px/1.7 monospace;padding:12px;background:#fffbe8;color:#333">
<div style="font-weight:bold;margin-bottom:8px">加载诊断（截图发给电脑）</div>
<div id="out"></div>
<script>
function log(m){var d=document.createElement('div');d.textContent='- '+m;document.getElementById('out').appendChild(d);}
log('页面加载成功（服务器可达，网络没问题）');
log('UA: '+navigator.userAgent.slice(0,90));
fetch('/api/health').then(function(r){return r.json()}).then(function(j){log('health 接口: '+JSON.stringify(j))}).catch(function(e){log('health 请求失败: '+e)});
fetch('/api/works').then(function(r){log('works 接口: HTTP '+r.status+(r.status===200?'（已配对）':'（未配对——需要扫新的配对码）'))}).catch(function(e){log('works 请求失败: '+e)});
</script>
<script type="module">window.__MOD_OK=true;</script>
<script>
setTimeout(function(){log('ES Module 支持: '+(window.__MOD_OK?'是':'否——老内核，会走降级包'));},250);
setTimeout(function(){log('诊断完成。全部正常的话，请清一次浏览器缓存（或用无痕）再打开主页');},900);
</script>
</body></html>"#;
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        html,
    )
        .into_response()
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") }))
}

async fn index_json() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        INDEX_JSON,
    )
}

async fn list_works(State(st): State<Arc<AppState>>) -> Result<Json<Vec<db::Work>>, ApiError> {
    let works = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::list(&conn)?
    };
    Ok(Json(works))
}

async fn get_work(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<db::Work>, ApiError> {
    let work = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::get(&conn, id)?
    };
    work.map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "作品不存在"))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddWork {
    pub anilist_id: Option<i64>,
    pub bangumi_id: Option<i64>,
    pub title_cn: Option<String>,
    pub title_jp: Option<String>,
    pub year: Option<i64>,
}

#[derive(Deserialize)]
pub struct BgmQuery {
    pub q: String,
}

/// Bangumi 在线搜索（国漫、日漫都能搜）
async fn bgm_search(
    State(st): State<Arc<AppState>>,
    Query(q): Query<BgmQuery>,
) -> Result<Json<Vec<bangumi::SubjectHit>>, ApiError> {
    let kw = q.q.trim();
    if kw.is_empty() {
        return Ok(Json(vec![]));
    }
    let hits = bangumi::search(&st.http, kw)
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, format!("Bangumi 搜索失败：{e:#}")))?;
    Ok(Json(hits))
}

async fn create_work(
    State(st): State<Arc<AppState>>,
    Json(body): Json<AddWork>,
) -> Result<Json<db::Work>, ApiError> {
    if let Some(aid) = body.anilist_id {
        let existing = {
            let conn = st.db.lock().expect("db mutex poisoned");
            db::find_by_anilist(&conn, aid)?
        };
        if let Some(w) = existing {
            return Ok(Json(w));
        }
    }
    if let Some(bid) = body.bangumi_id {
        let existing = {
            let conn = st.db.lock().expect("db mutex poisoned");
            db::find_by_bangumi(&conn, bid)?
        };
        if let Some(w) = existing {
            return Ok(Json(w));
        }
    }

    // 联网补全失败不阻断入库：有 anilistId 走 AniList，只有 bangumiId 走 Bangumi
    let mut meta = anilist::AnimeMeta::default();
    // 离线优先：全库 31312 条动画都有烘焙详情（含日漫），秒回不等网络。
    // 网络源（AniList / Bangumi 在线）只在离线详情缺失时兜底。
    if let Some(bid) = body.bangumi_id {
        match offline_bgm_meta(bid) {
            Some(m) => meta = m.into(),
            None => match bangumi::subject_meta(&st.http, bid).await {
                Ok(m) => meta = m.into(),
                Err(e) => eprintln!("[bangumi] 补全失败 #{bid}: {e:#}"),
            },
        }
    }
    let offline_used = !meta.title_cn.is_some() && !meta.title_native.is_some();
    let _ = offline_used;
    if meta.title_cn.is_none()
        && meta.title_native.is_none()
        && meta.description.is_none()
    {
        // 离线详情也没给足信息：有 AniList 编号就试一次（被墙时 4 秒即放弃）
        if let Some(aid) = body.anilist_id {
            match anilist::fetch(&st.http, aid).await {
                Ok(m) => meta = m,
                Err(e) => eprintln!("[anilist] 补全失败 #{aid}: {e:#}"),
            }
        }
        // AniList 也不行再试 Bangumi 在线
        if meta.title_cn.is_none()
            && meta.title_native.is_none()
            && meta.description.is_none()
        {
            if let Some(bid) = body.bangumi_id {
                match bangumi::subject_meta(&st.http, bid).await {
                    Ok(m) => meta = m.into(),
                    Err(e) => eprintln!("[bangumi] 补全失败 #{bid}: {e:#}"),
                }
            }
        }
    }

    let title_cn = body
        .title_cn
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| meta.title_cn.clone())
        .or_else(|| meta.title_native.clone())
        .or_else(|| json_opt(&body.title_jp))
        .or_else(|| meta.title_romaji.clone())
        .unwrap_or_else(|| "未命名".to_string());

    let new = db::NewWork {
        anilist_id: body.anilist_id,
        bangumi_id: body.bangumi_id,
        title_cn,
        title_jp: body.title_jp.or_else(|| meta.title_native.clone()),
        title_en: meta.title_english.clone(),
        year: body.year.or(meta.year),
        format: meta.format.clone(),
        episodes_total: meta.episodes,
        cover_path: None,
        synopsis: meta.description.clone(),
        genres: if meta.genres.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&meta.genres).unwrap_or_default())
        },
    };

    let work = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::insert(&conn, &new)?
    };

    // 封面在后台补：添加立即完成，不等下载（下不来启动时 heal 还会兜底）
    {
        let st2 = st.clone();
        let work_id = work.id;
        let bid = body.bangumi_id;
        let aid = body.anilist_id;
        let t_native = meta.title_native.clone();
        let t_cn = meta.title_cn.clone();
        tauri::async_runtime::spawn(async move {
            fetch_and_set_cover(st2, work_id, bid, aid, t_native, t_cn).await;
        });
    }

    Ok(Json(work))
}

/// 后台取封面：离线详情 / Kitsu 搜索 / AniList，谁先给出可达地址用谁，
/// 下载成功后写回 cover_path（前端几秒后刷新列表即可见）
async fn fetch_and_set_cover(
    st: Arc<AppState>,
    work_id: i64,
    bid: Option<i64>,
    aid: Option<i64>,
    title_native: Option<String>,
    title_cn: Option<String>,
) {
    let mut url: Option<String> = None;
    // 1) 离线详情里的 Kitsu 地址（国漫）
    if let Some(b) = bid {
        if let Some(d) = bgm_details().get(&b) {
            if !d.cover.is_empty() && !d.cover.contains("bgm.tv") {
                url = Some(d.cover.clone());
            }
        }
    }
    // 2) Kitsu 按标题搜（日文标题优先）
    if url.is_none() {
        let q = title_native
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| title_cn.as_deref().filter(|s| !s.trim().is_empty()))
            .map(str::to_string);
        if let Some(q) = q {
            url = kitsu_cover_for(&st, &q).await;
        }
    }
    // 3) AniList（要代理）
    if url.is_none() {
        if let Some(a) = aid {
            match anilist::fetch(&st.http, a).await {
                Ok(m) => {
                    if let Some(u) = m.cover_url.filter(|u| !u.is_empty()) {
                        url = Some(u);
                    }
                }
                Err(e) => eprintln!("[cover-bg] AniList 补封面失败 #{a}: {e:#}"),
            }
        }
    }
    let Some(url) = url else {
        return;
    };
    if url.contains("bgm.tv") {
        return; // 被墙源，留给 heal 兜底
    }
    let file = if let Some(b) = bid {
        format!("bgm-{b}.jpg")
    } else if let Some(a) = aid {
        format!("{a}.jpg")
    } else {
        format!("k-{:x}.jpg", md5_of(&url))
    };
    let dest = st.data_dir.join("covers").join(&file);
    let dl = if url.contains("kitsu") || url.contains("hdslb") {
        bangumi::download_cover(&st.http, &url, &dest).await
    } else {
        anilist::download_cover(&st.http, &url, &dest).await
    };
    match dl {
        Ok(()) => {
            let conn = st.db.lock().expect("db mutex poisoned");
            let _ = conn.execute(
                "UPDATE works SET cover_path = ?2 WHERE id = ?1",
                rusqlite::params![work_id, file],
            );
            println!("[cover-bg] 作品 #{work_id} 封面已就位: {file}");
        }
        Err(e) => eprintln!("[cover-bg] 作品 #{work_id} 封面下载失败: {e:#}"),
    }
}

async fn patch_work(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(patch): Json<db::WorkPatch>,
) -> Result<Json<db::Work>, ApiError> {
    let work = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::update(&conn, id, &patch)?
    };
    work.map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "作品不存在"))
}

async fn delete_work(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ok = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::delete(&conn, id)?
    };
    if ok {
        Ok(Json(json!({ "ok": true })))
    } else {
        Err(ApiError::new(StatusCode::NOT_FOUND, "作品不存在"))
    }
}

async fn cover(
    State(st): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Response, ApiError> {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "非法文件名"));
    }
    let path = st.data_dir.join("covers").join(&name);
    let bytes = tokio::fs::read(&path).await.map_err(|_| {
        ApiError::new(StatusCode::NOT_FOUND, "封面不存在")
    })?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        bytes,
    )
        .into_response())
}

fn json_opt(v: &Option<String>) -> Option<String> {
    v.as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

// ---------------- 素材 ----------------

const VIDEO_EXT: [&str; 7] = ["mp4", "webm", "mov", "mkv", "avi", "m4v", "wmv"];

fn kind_of(ext: &str) -> &'static str {
    if VIDEO_EXT.contains(&ext) {
        "video"
    } else {
        "image"
    }
}

fn media_abs(st: &AppState, m: &db::Media) -> PathBuf {
    // 归档模式存相对路径，引用模式存绝对路径
    if m.storage == "ref" {
        PathBuf::from(&m.path)
    } else {
        st.data_dir.join(&m.path)
    }
}

fn stamp_name() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

fn ext_of(file_name: &str, content_type: Option<&str>) -> String {
    if let Some(ext) = file_name.rsplit('.').next() {
        let ok = !ext.is_empty()
            && ext.len() <= 5
            && ext != file_name
            && ext.chars().all(|c| c.is_ascii_alphanumeric());
        if ok {
            return ext.to_lowercase();
        }
    }
    let ext = match content_type.unwrap_or("") {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/jpeg" => "jpg",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "video/x-matroska" => "mkv",
        "video/x-msvideo" => "avi",
        _ => "bin",
    };
    ext.to_string()
}

/// 边收边写，几百 MB 的视频也不会进内存
async fn write_field(
    field: &mut axum::extract::multipart::Field<'_>,
    path: &std::path::Path,
) -> anyhow::Result<i64> {
    let mut file = tokio::fs::File::create(path).await?;
    let mut total = 0i64;
    while let Some(chunk) = field.chunk().await? {
        file.write_all(&chunk).await?;
        total += chunk.len() as i64;
    }
    file.flush().await?;
    Ok(total)
}

async fn list_media(
    State(st): State<Arc<AppState>>,
    Path(work_id): Path<i64>,
) -> Result<Json<Vec<db::Media>>, ApiError> {
    let items = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::list_media(&conn, work_id)?
    };
    Ok(Json(items))
}

async fn get_media(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<db::Media>, ApiError> {
    let media = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::get_media(&conn, id)?
    };
    media
        .map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "素材不存在"))
}

async fn upload_media(
    State(st): State<Arc<AppState>>,
    Path(work_id): Path<i64>,
    mut mp: Multipart,
) -> Result<Json<db::Media>, ApiError> {
    {
        let conn = st.db.lock().expect("db mutex poisoned");
        if db::get(&conn, work_id)?.is_none() {
            return Err(ApiError::new(StatusCode::NOT_FOUND, "作品不存在"));
        }
    }

    let dir = st.data_dir.join("media").join(work_id.to_string());
    tokio::fs::create_dir_all(&dir).await?;
    let base = stamp_name();

    let mut kind = String::new();
    let mut storage = "archive".to_string();
    let mut stamp: Option<f64> = None;
    let mut duration: Option<f64> = None;
    let mut main: Option<(String, i64)> = None;
    let mut thumb: Option<String> = None;
    let mut poster: Option<String> = None;
    let mut ext = String::new();

    while let Some(mut field) = mp
        .next_field()
        .await
        .map_err(|e| anyhow::anyhow!("解析上传失败: {e}"))?
    {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "kind" => kind = field.text().await.unwrap_or_default(),
            "storage" => {
                let v = field.text().await.unwrap_or_default();
                if v == "ref" || v == "archive" {
                    storage = v;
                }
            }
            "stamp" => stamp = field.text().await.ok().and_then(|v| v.trim().parse().ok()),
            "duration" => duration = field.text().await.ok().and_then(|v| v.trim().parse().ok()),
            "file" => {
                let fname = field.file_name().map(str::to_string).unwrap_or_default();
                let ctype = field.content_type().map(str::to_string);
                ext = ext_of(&fname, ctype.as_deref());
                let abs = dir.join(format!("{base}.{ext}"));
                let bytes = write_field(&mut field, &abs).await?;
                main = Some((format!("media/{work_id}/{base}.{ext}"), bytes));
            }
            "thumb" => {
                let abs = dir.join(format!("{base}.thumb.jpg"));
                write_field(&mut field, &abs).await?;
                thumb = Some(format!("media/{work_id}/{base}.thumb.jpg"));
            }
            "poster" => {
                let abs = dir.join(format!("{base}.poster.jpg"));
                write_field(&mut field, &abs).await?;
                poster = Some(format!("media/{work_id}/{base}.poster.jpg"));
            }
            _ => {
                let _ = field.bytes().await;
            }
        }
    }

    let Some((path, bytes)) = main else {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "没有收到文件"));
    };
    if kind.is_empty() {
        kind = kind_of(&ext).to_string();
    }

    let media = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::insert_media(
            &conn,
            &db::NewMedia {
                work_id,
                kind,
                storage,
                path,
                thumb,
                poster,
                bytes: Some(bytes),
                duration,
                stamp,
            },
        )?
    };
    Ok(Json(media))
}

#[derive(Debug, Deserialize)]
pub struct ImportMediaBody {
    pub path: String,
    /// ref = 只登记路径；archive = 复制进库。缺省跟随作品设置
    pub mode: Option<String>,
}

/// 按本机路径导入：拖拽文件进来时走这里。
/// 归档模式由服务端直接复制，不用把文件内容经浏览器传一遍。
async fn import_media(
    State(st): State<Arc<AppState>>,
    Path(work_id): Path<i64>,
    Json(body): Json<ImportMediaBody>,
) -> Result<Json<db::Media>, ApiError> {
    let work = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::get(&conn, work_id)?
    };
    let Some(work) = work else {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "作品不存在"));
    };

    let abs = PathBuf::from(body.path.trim());
    if !abs.is_file() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "这个路径不是文件，或者已经不存在",
        ));
    }
    let ext = abs
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let kind = kind_of(&ext).to_string();
    let mode = body.mode.unwrap_or_else(|| work.media_mode.clone());
    let bytes = std::fs::metadata(&abs).ok().map(|m| m.len() as i64);

    let (storage, path) = if mode == "ref" {
        ("ref".to_string(), abs.to_string_lossy().to_string())
    } else {
        let dir = st.data_dir.join("media").join(work_id.to_string());
        tokio::fs::create_dir_all(&dir).await?;
        let name = format!("{}.{}", stamp_name(), if ext.is_empty() { "bin" } else { &ext });
        tokio::fs::copy(&abs, dir.join(&name)).await?;
        (
            "archive".to_string(),
            format!("media/{work_id}/{name}"),
        )
    };

    let media = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::insert_media(
            &conn,
            &db::NewMedia {
                work_id,
                kind,
                storage,
                path,
                thumb: None,
                poster: None,
                bytes,
                duration: None,
                stamp: None,
            },
        )?
    };
    Ok(Json(media))
}

/// 带 Range 的文件流，视频拖动进度条靠它
async fn media_file(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
    req: Request<Body>,
) -> Result<Response, ApiError> {
    let media = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::get_media(&conn, id)?
    };
    let Some(m) = media else {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "素材不存在"));
    };

    let abs = media_abs(&st, &m);
    if !abs.is_file() {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::set_media_missing(&conn, id, true)?;
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "原文件不在了，可能被移动或删除",
        ));
    }
    if m.missing != 0 {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::set_media_missing(&conn, id, false)?;
    }

    let res = ServeFile::new(abs)
        .oneshot(req)
        .await
        .expect("ServeFile 不会失败");
    Ok(res.into_response())
}

async fn media_thumb(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let media = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::get_media(&conn, id)?
    };
    let Some(m) = media else {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "素材不存在"));
    };
    let Some(rel) = m.thumb.clone().or_else(|| m.poster.clone()) else {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "没有缩略图"));
    };
    let abs = st.data_dir.join(rel);
    let bytes = tokio::fs::read(&abs)
        .await
        .map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "缩略图不存在"))?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::CACHE_CONTROL, "public, max-age=604800"),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
pub struct MediaPatch {
    pub caption: Option<String>,
    /// 0 表示清空集号
    pub episode: Option<i64>,
    /// 播放进度（秒），续播用
    pub position: Option<f64>,
    /// 引用模式改路径（文件被移动后重新指回去）
    pub path: Option<String>,
}

async fn patch_media(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(patch): Json<MediaPatch>,
) -> Result<Json<db::Media>, ApiError> {
    let conn = st.db.lock().expect("db mutex poisoned");
    let Some(cur) = db::get_media(&conn, id)? else {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "素材不存在"));
    };

    if let Some(raw) = patch
        .path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if cur.storage != "ref" {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "只有引用模式的素材能改路径，归档素材请重新导入",
            ));
        }
        let abs = PathBuf::from(raw);
        if !abs.is_file() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "这个路径不是文件，或者已经不存在",
            ));
        }
        let ext = abs
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let size = std::fs::metadata(&abs).ok().map(|m| m.len() as i64);
        db::set_media_path(&conn, id, &abs.to_string_lossy(), size, kind_of(&ext))?;
    }

    if let Some(ep) = patch.episode {
        // 0 或负数视为"清空"
        db::set_media_episode(&conn, id, if ep <= 0 { None } else { Some(ep) })?;
    }
    if let Some(pos) = patch.position {
        db::set_media_position(&conn, id, pos.max(0.0))?;
    }
    if let Some(caption) = patch.caption.clone() {
        db::set_media_caption(&conn, id, Some(caption))?;
    }

    let media = db::get_media(&conn, id)?;
    drop(conn);
    media
        .map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "素材不存在"))
}

// ---------------- 标签 ----------------

async fn list_tags(State(st): State<Arc<AppState>>) -> Result<Json<Vec<db::Tag>>, ApiError> {
    let tags = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::list_tags(&conn)?
    };
    Ok(Json(tags))
}

#[derive(Debug, Deserialize)]
pub struct TagBody {
    pub name: String,
}

async fn create_tag(
    State(st): State<Arc<AppState>>,
    Json(body): Json<TagBody>,
) -> Result<Json<db::Tag>, ApiError> {
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "标签名不能为空"));
    }
    let conn = st.db.lock().expect("db mutex poisoned");
    conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [&name])?;
    let tag = conn.query_row(
        "SELECT t.id, t.name, (SELECT COUNT(*) FROM work_tags wt WHERE wt.tag_id = t.id)
         FROM tags t WHERE t.name = ?1",
        [&name],
        |r| {
            Ok(db::Tag {
                id: r.get(0)?,
                name: r.get(1)?,
                count: r.get(2)?,
            })
        },
    )?;
    Ok(Json(tag))
}

async fn rename_tag(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<TagBody>,
) -> Result<Json<db::Tag>, ApiError> {
    let tag = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::rename_tag(&conn, id, &body.name)?
    };
    tag.map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "标签名不能为空"))
}

async fn delete_tag(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ok = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::delete_tag(&conn, id)?
    };
    if !ok {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "标签不存在"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// 直接给作品加标签，标签不存在就建
async fn add_work_tag(
    State(st): State<Arc<AppState>>,
    Path(work_id): Path<i64>,
    Json(body): Json<TagBody>,
) -> Result<Json<Vec<String>>, ApiError> {
    let tags = {
        let conn = st.db.lock().expect("db mutex poisoned");
        if db::get(&conn, work_id)?.is_none() {
            return Err(ApiError::new(StatusCode::NOT_FOUND, "作品不存在"));
        }
        db::add_work_tag(&conn, work_id, &body.name)?
    };
    Ok(Json(tags))
}

async fn remove_work_tag(
    State(st): State<Arc<AppState>>,
    Path((work_id, tag_id)): Path<(i64, i64)>,
) -> Result<Json<Vec<String>>, ApiError> {
    let tags = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::remove_work_tag(&conn, work_id, tag_id)?
    };
    Ok(Json(tags))
}

async fn delete_media(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let media = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::get_media(&conn, id)?
    };
    let Some(m) = media else {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "素材不存在"));
    };

    // 归档文件先移进 .trash，不直接销毁；引用模式不碰用户的原文件
    if m.storage == "archive" {
        let trash = st.data_dir.join(".trash");
        tokio::fs::create_dir_all(&trash).await?;
        let rels = [Some(m.path.clone()), m.thumb.clone(), m.poster.clone()];
        for rel in rels.into_iter().flatten() {
            let abs = st.data_dir.join(&rel);
            if abs.is_file() {
                if let Some(name) = abs.file_name().map(|s| s.to_string_lossy().to_string()) {
                    let dest = trash.join(format!("{}-{name}", stamp_name()));
                    let _ = tokio::fs::rename(&abs, dest).await;
                }
            }
        }
    }

    {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::delete_media(&conn, id)?;
    }
    Ok(Json(json!({ "ok": true })))
}

// ---------------- 导出 / 备份 ----------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportInfo {
    pub name: String,
    pub bytes: i64,
    pub works: i64,
    pub media: i64,
    pub created_at: String,
}

fn prune_exports(dir: &std::path::Path, keep: usize) {
    let mut zips: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "zip").unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    zips.sort();
    while zips.len() > keep {
        let old = zips.remove(0);
        let _ = std::fs::remove_file(old);
    }
}

fn add_zip_dir(
    zw: &mut zip::ZipWriter<std::fs::File>,
    dir: &std::path::Path,
    prefix: &str,
    opts: zip::write::SimpleFileOptions,
) -> anyhow::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        zw.start_file(format!("{prefix}/{name}"), opts)?;
        let mut f = std::fs::File::open(entry.path())?;
        std::io::copy(&mut f, zw)?;
    }
    Ok(())
}

fn build_zip(
    data_dir: &std::path::Path,
    snapshot: &std::path::Path,
    out: &std::path::Path,
    manifest: &str,
) -> anyhow::Result<i64> {
    use zip::write::SimpleFileOptions;

    let file = std::fs::File::create(out)?;
    let mut zw = zip::ZipWriter::new(file);
    let deflate = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    // 封面和视频本身已压缩，再 deflate 纯属浪费 CPU
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    zw.start_file("manifest.json", deflate)?;
    std::io::Write::write_all(&mut zw, manifest.as_bytes())?;

    zw.start_file("library.db", deflate)?;
    let mut db_file = std::fs::File::open(snapshot)?;
    std::io::copy(&mut db_file, &mut zw)?;

    add_zip_dir(&mut zw, &data_dir.join("covers"), "covers", stored)?;

    let media_root = data_dir.join("media");
    if media_root.is_dir() {
        for entry in std::fs::read_dir(&media_root)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                add_zip_dir(&mut zw, &entry.path(), &format!("media/{name}"), stored)?;
            }
        }
    }

    zw.finish()?;
    Ok(std::fs::metadata(out)?.len() as i64)
}

async fn export_library(State(st): State<Arc<AppState>>) -> Result<Json<ExportInfo>, ApiError> {
    let dir = st.data_dir.join(".exports");
    tokio::fs::create_dir_all(&dir).await?;

    let (stamp, works, refs) = {
        let conn = st.db.lock().expect("db mutex poisoned");
        let stamp: String = conn.query_row(
            "SELECT strftime('%Y%m%d-%H%M','now','localtime')",
            [],
            |r| r.get(0),
        )?;
        (stamp, db::count(&conn)?, db::list_media_refs(&conn)?)
    };

    // VACUUM INTO 在 WAL 模式下也能拿到一致快照，比直接拷 .db 稳
    let snapshot = dir.join(format!("library-{stamp}.db"));
    {
        let conn = st.db.lock().expect("db mutex poisoned");
        let _ = std::fs::remove_file(&snapshot);
        conn.execute(
            "VACUUM INTO ?1",
            rusqlite::params![snapshot.to_string_lossy().to_string()],
        )?;
    }

    let manifest = json!({
        "app": "动漫档案馆",
        "version": env!("CARGO_PKG_VERSION"),
        "exportedAt": stamp,
        "works": works,
        "mediaCount": refs.len(),
        "layout": {
            "library.db": "SQLite 快照，含作品/标签/素材索引",
            "covers/": "封面图",
            "media/": "归档模式下复制进来的文件",
            "manifest.json": "本文件"
        },
        "mediaRefs": refs,
        "note": "引用模式的素材没有复制文件，mediaRefs 里保留了原始绝对路径，换机后可据此重连",
    });

    let out = dir.join(format!("library-{stamp}.zip"));
    let data_dir = st.data_dir.clone();
    let snap = snapshot.clone();
    let out2 = out.clone();
    let manifest_str = serde_json::to_string_pretty(&manifest)?;

    let bytes = tokio::task::spawn_blocking(move || {
        build_zip(&data_dir, &snap, &out2, &manifest_str)
    })
    .await??;

    let _ = std::fs::remove_file(&snapshot);
    prune_exports(&dir, 3);

    Ok(Json(ExportInfo {
        name: format!("library-{stamp}.zip"),
        bytes,
        works,
        media: refs.len() as i64,
        created_at: stamp,
    }))
}

/// 下载最近一次导出的 zip
async fn export_download(
    State(st): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response, ApiError> {
    let dir = st.data_dir.join(".exports");
    let mut zips: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "zip").unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    zips.sort();
    let Some(path) = zips.pop() else {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "还没有导出过"));
    };
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "library.zip".to_string());

    let mut res = ServeFile::new(path)
        .oneshot(req)
        .await
        .expect("ServeFile 不会失败")
        .into_response();
    res.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{name}\"").parse().expect("header 合法"),
    );
    Ok(res)
}

// ---------------- Wallpaper Engine 壁纸 ----------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Wallpaper {
    /// 创意工坊 id，同时当作对外的不透明标识
    pub id: String,
    pub title: String,
    /// video / scene / web
    pub kind: String,
    /// 编码可靠的视频才有值
    pub video: Option<String>,
    /// 网页壁纸的入口文件（相对路径，如 index.html）
    pub entry: Option<String>,
    pub preview: Option<String>,
    pub tags: Vec<String>,
    pub bytes: i64,
    /// 本地目录，仅供服务端解析文件用，不对外暴露
    #[serde(skip)]
    pub dir: Option<std::path::PathBuf>,
}

/// 背景的显示参数，和"选了哪个壁纸"一起存进 settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WallpaperConfig {
    pub id: Option<String>,
    /// 临时开关：关掉但保留选择
    pub enabled: bool,
    /// 薄纱浓度 0..1
    pub veil: f64,
    /// 模糊像素 0..24
    pub blur: f64,
    /// cover / contain
    pub fit: String,
    /// 这台机器上确认播不出来、已降级为静态的壁纸 id
    #[serde(default)]
    pub unplayable: Vec<String>,
}

impl Default for WallpaperConfig {
    fn default() -> Self {
        Self {
            id: None,
            enabled: true,
            veil: 0.8,
            blur: 3.0,
            fit: "cover".to_string(),
            unplayable: Vec::new(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperState {
    #[serde(flatten)]
    pub config: WallpaperConfig,
    pub title: Option<String>,
    pub kind: Option<String>,
    /// 有没有预览图（场景壁纸只有这个能当静态背景）
    pub has_preview: bool,
    /// 是不是能动的视频
    pub has_video: bool,
    /// 是不是网页壁纸（iframe 动态渲染）
    pub has_web: bool,
}

const WALLPAPER_KEY: &str = "wallpaper_config";

fn load_config(st: &AppState) -> WallpaperConfig {
    let conn = st.db.lock().expect("db mutex poisoned");
    if let Ok(Some(raw)) = db::get_setting(&conn, WALLPAPER_KEY) {
        if let Ok(cfg) = serde_json::from_str::<WallpaperConfig>(&raw) {
            return cfg;
        }
    }
    // 兼容早期只存了 id 的格式
    if let Ok(Some(id)) = db::get_setting(&conn, "wallpaper") {
        return WallpaperConfig {
            id: Some(id),
            ..WallpaperConfig::default()
        };
    }
    WallpaperConfig::default()
}

fn save_config(st: &AppState, cfg: &WallpaperConfig) -> anyhow::Result<()> {
    let conn = st.db.lock().expect("db mutex poisoned");
    let raw = serde_json::to_string(cfg)?;
    db::set_setting(&conn, WALLPAPER_KEY, &raw)?;
    // 老字段清掉，避免两处状态打架
    let _ = conn.execute("DELETE FROM settings WHERE key = 'wallpaper'", []);
    Ok(())
}

fn steam_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    // 注册表里最准；cmd 输出是 GBK，这里只找 "SteamPath" 那行的路径片段
    if let Ok(out) = std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"])
        .output()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        if let Some(idx) = text.find("REG_SZ") {
            let p = text[idx + 6..].lines().next().unwrap_or("").trim();
            if !p.is_empty() {
                roots.push(PathBuf::from(p.replace('/', "\\")));
            }
        }
    }

    for guess in [
        r"C:/Program Files (x86)/Steam",
        r"C:/Program Files/Steam",
        r"D:/Steam",
        r"E:/Steam",
    ] {
        let p = PathBuf::from(guess);
        if p.is_dir() {
            roots.push(p);
        }
    }

    roots.sort();
    roots.dedup();
    roots
}

fn library_roots() -> Vec<PathBuf> {
    let mut libs = Vec::new();
    for root in steam_roots() {
        if root.is_dir() {
            libs.push(root.clone());
        }
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(&vdf) {
            for line in text.lines() {
                let t = line.trim();
                if !t.starts_with("\"path\"") {
                    continue;
                }
                let parts: Vec<&str> = t.split('"').collect();
                if parts.len() >= 4 {
                    let p = parts[3].replace("\\\\", "\\");
                    if !p.trim().is_empty() {
                        libs.push(PathBuf::from(p));
                    }
                }
            }
        }
    }
    libs.sort();
    libs.dedup();
    libs
}

/// WebView2 能否可靠播放这个视频；拿不准就退回静态预览。
/// H.264 的 10bit/422 档（动漫圈常见）和 HEVC 浏览器基本放不了。
fn video_playable(path: &std::path::Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = Vec::new();
    if f.take(6 * 1024 * 1024).read_to_end(&mut head).is_err() {
        return false;
    }

    let find = |needle: &[u8]| -> Option<usize> {
        if needle.len() <= head.len() {
            head.windows(needle.len()).position(|w| w == needle)
        } else {
            None
        }
    };

    // WebM/EBML：codec id 是明文字符串
    if head.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return find(b"V_VP9").is_some()
            || find(b"V_VP8").is_some()
            || find(b"V_AV1").is_some();
    }

    // HEVC：要装系统扩展，别赌
    if find(b"hev1").is_some() || find(b"hvc1").is_some() {
        return false;
    }
    // AV1 / VP9：Edge 内核原生支持
    if find(b"av01").is_some() || find(b"vp09").is_some() || find(b"vp08").is_some() {
        return true;
    }
    // H.264：看 avcC 里的 profile_idc
    if let Some(i) = find(b"avcC") {
        if i + 6 <= head.len() {
            let profile = head[i + 5];
            // 110=High10, 122=High422, 138/244=更冷门的档
            return !matches!(profile, 110 | 122 | 138 | 244);
        }
    }
    find(b"avc1").is_some() || find(b"avc3").is_some()
}

/// 目录里找一张能当预览的图：优先 preview.*，否则第一张图
fn fallback_preview(dir: &std::path::Path) -> Option<String> {
    let mut first: Option<String> = None;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "gif" | "webp") {
            continue;
        }
        let s = p.to_string_lossy().to_string();
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.starts_with("preview") {
            return Some(s);
        }
        if first.is_none() {
            first = Some(s);
        }
    }
    first
}

fn scan_wallpapers() -> Vec<Wallpaper> {
    let mut out: Vec<Wallpaper> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for lib in library_roots() {
        let ws = lib
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join("431960");
        let Ok(entries) = std::fs::read_dir(&ws) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            if !seen.insert(id.clone()) {
                continue;
            }

            let (title, kind, file, preview_name, tags) =
                match std::fs::read_to_string(dir.join("project.json")) {
                    Ok(text) => {
                        let text = text.trim_start_matches('\u{feff}');
                        match serde_json::from_str::<serde_json::Value>(text) {
                            Ok(v) => (
                                v.get("title")
                                    .and_then(|x| x.as_str())
                                    .map(str::to_string)
                                    .unwrap_or_else(|| id.clone()),
                                v.get("type")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("")
                                    .to_lowercase(),
                                v.get("file")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                v.get("preview")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                v.get("tags")
                                    .and_then(|x| x.as_array())
                                    .map(|a| {
                                        a.iter()
                                            .filter_map(|t| t.as_str().map(str::to_string))
                                            .collect()
                                    })
                                    .unwrap_or_default(),
                            ),
                            Err(_) => (id.clone(), String::new(), String::new(), String::new(), vec![]),
                        }
                    }
                    // 没有工程文件的旧壁纸：目录里翻一张图当静态背景
                    Err(_) => {
                        if let Some(img) = fallback_preview(&dir) {
                            out.push(Wallpaper {
                                id: id.clone(),
                                title: id.clone(),
                                kind: "static".to_string(),
                                video: None,
                                entry: None,
                                preview: Some(img),
                                tags: vec![],
                                bytes: 0,
                                dir: Some(dir),
                            });
                        }
                        continue;
                    }
                };

            // 只有编码可靠的才给视频模式，其余一律静态预览
            let video = if kind == "video" && !file.is_empty() {
                let p = dir.join(&file);
                if p.is_file() && video_playable(&p) {
                    Some(p.to_string_lossy().to_string())
                } else {
                    None
                }
            } else {
                None
            };
            // 网页壁纸的入口文件
            let entry = if kind == "web" && !file.is_empty() {
                let p = dir.join(&file);
                p.is_file().then(|| file.clone())
            } else {
                None
            };
            let bytes = video
                .as_ref()
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| m.len() as i64)
                .unwrap_or(0);
            let preview_abs = if preview_name.is_empty() {
                fallback_preview(&dir)
            } else {
                let p = dir.join(&preview_name);
                p.is_file().then(|| p.to_string_lossy().to_string())
            };

            out.push(Wallpaper {
                id,
                title,
                kind,
                video,
                entry,
                preview: preview_abs,
                tags,
                bytes,
                dir: Some(dir),
            });
        }
    }

    out.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    out
}

fn find_wallpaper(st: &AppState, id: &str) -> Option<Wallpaper> {
    let mut cache = st.wallpapers.lock().expect("wallpaper mutex poisoned");
    if cache.is_empty() {
        *cache = scan_wallpapers();
    }
    cache.iter().find(|w| w.id == id).cloned()
}

fn current_wallpaper(st: &AppState) -> WallpaperState {
    let config = load_config(st);
    let found = config.id.as_deref().and_then(|i| find_wallpaper(st, i));
    WallpaperState {
        title: found.as_ref().map(|w| w.title.clone()),
        kind: found.as_ref().map(|w| w.kind.clone()),
        has_preview: found.as_ref().map(|w| w.preview.is_some()).unwrap_or(false),
        has_video: found.as_ref().map(|w| w.video.is_some()).unwrap_or(false),
        has_web: found.as_ref().map(|w| w.entry.is_some()).unwrap_or(false),
        config,
    }
}

#[derive(Deserialize)]
pub struct RefreshQuery {
    pub refresh: Option<String>,
}

async fn list_wallpapers(
    State(st): State<Arc<AppState>>,
    Query(q): Query<RefreshQuery>,
) -> Result<Json<Vec<Wallpaper>>, ApiError> {
    let need_scan = {
        let cache = st.wallpapers.lock().expect("wallpaper mutex poisoned");
        cache.is_empty() || q.refresh.as_deref() == Some("1")
    };
    if need_scan {
        let list = tokio::task::spawn_blocking(scan_wallpapers).await?;
        let mut cache = st.wallpapers.lock().expect("wallpaper mutex poisoned");
        *cache = list;
    }
    let list = {
        let cache = st.wallpapers.lock().expect("wallpaper mutex poisoned");
        cache.clone()
    };
    Ok(Json(list))
}

async fn wallpaper_preview(
    State(st): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let w = find_wallpaper(&st, &id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "壁纸不存在"))?;
    let Some(preview) = w.preview else {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "这个壁纸没有预览图"));
    };
    let bytes = tokio::fs::read(&preview)
        .await
        .map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "预览图读不到"))?;
    let mime = if preview.to_lowercase().ends_with(".gif") {
        "image/gif"
    } else {
        "image/jpeg"
    };
    Ok((
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        bytes,
    )
        .into_response())
}

async fn wallpaper_video(
    State(st): State<Arc<AppState>>,
    Path(id): Path<String>,
    req: Request<Body>,
) -> Result<Response, ApiError> {
    let w = find_wallpaper(&st, &id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "壁纸不存在"))?;
    let Some(video) = w.video else {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "这个壁纸不是视频类型，没法当背景播放",
        ));
    };
    if !PathBuf::from(&video).is_file() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "视频文件不在了"));
    }
    let res = ServeFile::new(video)
        .oneshot(req)
        .await
        .expect("ServeFile 不会失败");
    Ok(res.into_response())
}

fn mime_for_ext(ext: &str) -> &'static str {
    match ext {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "ogg" | "oga" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" | "otf" => "font/ttf",
        "wasm" => "application/wasm",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

/// 把 shim 注进 <head> 里面；找不到就放最前面
fn inject_after_head(html: &str, snippet: &str) -> String {
    let lower = html.to_lowercase();
    if let Some(idx) = lower.find("<head") {
        if let Some(end) = html[idx..].find('>') {
            let at = idx + end + 1;
            let mut out = String::with_capacity(html.len() + snippet.len());
            out.push_str(&html[..at]);
            out.push_str(snippet);
            out.push_str(&html[at..]);
            return out;
        }
    }
    format!("{snippet}{html}")
}

/// 网页壁纸入口：返回注入了 WE 私有接口 shim 的 index.html
async fn wallpaper_entry(
    State(st): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let w = find_wallpaper(&st, &id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "壁纸不存在"))?;
    let (Some(dir), Some(entry)) = (w.dir.as_ref(), w.entry.as_ref()) else {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "这个壁纸不是网页类型",
        ));
    };
    let bytes = tokio::fs::read(dir.join(entry))
        .await
        .map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "入口文件读不到"))?;
    let text = String::from_utf8_lossy(&bytes).to_string();

    // WE 的运行时会调用页面注册的 wallpaperPropertyListener.applyUserProperties；
    // 这里替它把 project.json 里的默认属性喂进去，不然一部分网页壁纸只画个黑底
    let props = std::fs::read_to_string(dir.join("project.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t.trim_start_matches('\u{feff}')).ok())
        .and_then(|v| {
            v.get("general")
                .and_then(|g| g.get("properties"))
                .cloned()
        })
        .unwrap_or_else(|| serde_json::json!({}));

    let shim = format!(
        r#"<script data-cn-shim>(function(){{var d={props};var applied=false;function apply(l){{if(!applied&&l&&typeof l.applyUserProperties==="function"){{try{{l.applyUserProperties(d);applied=true;}}catch(e){{}}}}}}var cur=null;try{{Object.defineProperty(window,"wallpaperPropertyListener",{{configurable:true,get:function(){{return cur;}},set:function(v2){{cur=v2;apply(v2);}}}});}}catch(e){{}}window.wallpaperRegisterAudioListener=function(){{}};window.wallpaperRegisterAudioPropertyListener=function(){{}};}})();</script>"#
    );
    let out = inject_after_head(&text, &shim);
    Ok((
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        out,
    )
        .into_response())
}

/// 网页壁纸的相对路径素材（index.js、assets/xxx、甚至 ../images/xxx）
/// 安全性靠 canonicalize 后的前缀校验，而不是简单拒绝 ".."
async fn wallpaper_entry_asset(
    State(st): State<Arc<AppState>>,
    Path((id, rel)): Path<(String, String)>,
    req: Request<Body>,
) -> Result<Response, ApiError> {
    let w = find_wallpaper(&st, &id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "壁纸不存在"))?;
    let Some(dir) = w.dir.as_ref() else {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "壁纸目录不存在"));
    };
    let rel = rel.trim_start_matches('/');
    if rel.is_empty() || rel.contains('\\') || rel.contains(':') {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "非法路径"));
    }
    let base = dir
        .canonicalize()
        .map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "壁纸目录不存在"))?;
    let file = dir.join(rel);
    let canon = file
        .canonicalize()
        .map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "素材不存在"))?;
    if !canon.starts_with(&base) || !canon.is_file() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "素材不存在"));
    }
    let ext = canon
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let res = ServeFile::new(canon)
        .oneshot(req)
        .await
        .expect("ServeFile 不会失败")
        .into_response();
    // ServeFile 按 Web 标准猜 mime，网页壁纸里的素材类型杂，按扩展名修正
    let mut res = res;
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        mime_for_ext(&ext).parse().expect("mime 合法"),
    );
    Ok(res)
}

async fn get_wallpaper(State(st): State<Arc<AppState>>) -> Json<WallpaperState> {
    let state = tokio::task::spawn_blocking(move || current_wallpaper(&st))
        .await
        .unwrap_or(WallpaperState {
            config: WallpaperConfig::default(),
            title: None,
            kind: None,
            has_preview: false,
            has_video: false,
            has_web: false,
        });
    Json(state)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperPatch {
    /// 传 id 选中；传空串清空；不传则保持不变
    pub id: Option<String>,
    pub enabled: Option<bool>,
    pub veil: Option<f64>,
    pub blur: Option<f64>,
    pub fit: Option<String>,
    /// 整表替换：这台机器上确认播不出来的 id 列表
    pub unplayable: Option<Vec<String>>,
}

async fn set_wallpaper(
    State(st): State<Arc<AppState>>,
    Json(patch): Json<WallpaperPatch>,
) -> Result<Json<WallpaperState>, ApiError> {
    let mut cfg = load_config(&st);

    if let Some(id) = patch.id {
        let id = id.trim().to_string();
        cfg.id = if id.is_empty() { None } else { Some(id) };
    }
    if let Some(v) = patch.enabled {
        cfg.enabled = v;
    }
    if let Some(v) = patch.veil {
        cfg.veil = v.clamp(0.0, 1.0);
    }
    if let Some(v) = patch.blur {
        cfg.blur = v.clamp(0.0, 24.0);
    }
    if let Some(v) = patch.fit {
        cfg.fit = if v == "contain" { "contain".into() } else { "cover".into() };
    }
    if let Some(list) = patch.unplayable {
        let mut uniq: Vec<String> = Vec::new();
        for x in list {
            let x = x.trim().to_string();
            if !x.is_empty() && !uniq.contains(&x) {
                uniq.push(x);
            }
        }
        cfg.unplayable = uniq;
    }

    save_config(&st, &cfg)?;
    let state = tokio::task::spawn_blocking(move || current_wallpaper(&st)).await?;
    Ok(Json(state))
}

// ---------------- 窗口状态记忆 ----------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WindowCfg {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub maximized: bool,
}

const WINDOW_KEY: &str = "window_state";

pub fn load_window_cfg(st: &AppState) -> Option<WindowCfg> {
    let conn = st.db.lock().expect("db mutex poisoned");
    let raw = db::get_setting(&conn, WINDOW_KEY).ok().flatten()?;
    serde_json::from_str(&raw).ok()
}

pub fn save_window_state(st: &AppState, window: &tauri::Window) {
    let pos = match window.outer_position() {
        Ok(p) => p,
        Err(_) => return,
    };
    let size = match window.inner_size() {
        Ok(s) => s,
        Err(_) => return,
    };
    let cfg = WindowCfg {
        x: pos.x,
        y: pos.y,
        w: size.width,
        h: size.height,
        maximized: window.is_maximized().unwrap_or(false),
    };
    if let Ok(raw) = serde_json::to_string(&cfg) {
        let conn = st.db.lock().expect("db mutex poisoned");
        let _ = db::set_setting(&conn, WINDOW_KEY, &raw);
    }
}

// ---------------- 从备份恢复 ----------------

/// 启动时（打开数据库之前）如果有待恢复的备份包，整包替换当前数据。
/// 当前数据会先挪进 .restore-backup-<时间戳>，删错了还能捞回来。
pub fn apply_pending_restore(data_dir: &std::path::Path) {
    let pending = data_dir.join(".restore-pending");
    if !pending.join("library.db").is_file() {
        return;
    }
    println!("[restore] 发现待恢复的备份，正在替换当前数据…");

    let backup = data_dir.join(format!(".restore-backup-{}", stamp_name()));
    let _ = std::fs::create_dir_all(&backup);
    for name in ["library.db", "library.db-wal", "library.db-shm", "covers", "media"] {
        let cur = data_dir.join(name);
        if cur.exists() {
            let _ = std::fs::rename(&cur, backup.join(name));
        }
    }

    for entry in std::fs::read_dir(&pending).expect("读恢复目录失败") {
        let entry = entry.expect("entry");
        let target = data_dir.join(entry.file_name());
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            let _ = std::fs::remove_dir_all(&target);
        }
        let _ = std::fs::rename(entry.path(), &target);
    }
    let _ = std::fs::remove_dir_all(&pending);
    println!("[restore] 完成，原数据备份在 {}", backup.display());
}

async fn restore_backup(
    State(st): State<Arc<AppState>>,
    mut mp: Multipart,
) -> Result<Json<serde_json::Value>, ApiError> {
    let pending = st.data_dir.join(".restore-pending");
    let incoming = st.data_dir.join(".restore-incoming.zip");
    tokio::fs::create_dir_all(&pending).await?;

    // 边收边写，几个 GB 的备份也不会撑爆内存
    let mut found = false;
    {
        let mut f = tokio::fs::File::create(&incoming).await?;
        while let Some(mut field) = mp
            .next_field()
            .await
            .map_err(|e| anyhow::anyhow!("解析上传失败: {e}"))?
        {
            if field.name() == Some("file") {
                found = true;
                while let Some(chunk) = field.chunk().await? {
                    f.write_all(&chunk).await?;
                }
            } else {
                let _ = field.bytes().await;
            }
        }
        f.flush().await?;
    }
    if !found {
        let _ = std::fs::remove_file(&incoming);
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "没有收到备份文件"));
    }

    let incoming2 = incoming.clone();
    let pending2 = pending.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let f = std::fs::File::open(&incoming2).map_err(|e| e.to_string())?;
        let mut z = zip::ZipArchive::new(f).map_err(|e| format!("不是有效的 zip：{e}"))?;

        let (mut has_db, mut has_manifest) = (false, false);
        for i in 0..z.len() {
            match z.name_for_index(i) {
                Some("library.db") => has_db = true,
                Some("manifest.json") => has_manifest = true,
                _ => {}
            }
        }
        if !has_db || !has_manifest {
            return Err("这不是动漫档案馆的备份包（缺 manifest.json 或 library.db）".into());
        }

        let _ = std::fs::remove_dir_all(&pending2);
        std::fs::create_dir_all(&pending2).map_err(|e| e.to_string())?;

        for i in 0..z.len() {
            let mut f = z.by_index(i).map_err(|e| e.to_string())?;
            let name = f.name().to_string();
            if name.ends_with('/') || name.contains("..") || name.starts_with('/') {
                continue;
            }
            // 白名单：只认认识的东西，别的直接跳过
            if !(name == "manifest.json"
                || name == "library.db"
                || name.starts_with("covers/")
                || name.starts_with("media/"))
            {
                continue;
            }
            let dest = pending2.join(&name);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
            std::io::copy(&mut f, &mut out).map_err(|e| e.to_string())?;
        }
        Ok(())
    })
    .await
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let _ = std::fs::remove_file(&incoming);
    if let Err(msg) = result {
        let _ = std::fs::remove_dir_all(&pending);
        return Err(ApiError::new(StatusCode::BAD_REQUEST, msg));
    }

    Ok(Json(json!({ "ok": true, "requiresRestart": true })))
}

/// 读 Windows 系统代理（IE/WinINET 设置）。未开启返回 None。
#[cfg(windows)]
fn system_proxy() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
        .ok()?;
    let enabled: u32 = key.get_value("ProxyEnable").ok()?;
    if enabled != 1 {
        return None;
    }
    let server: String = key.get_value("ProxyServer").ok()?;
    // ProxyServer 可能是 "host:port"，也可能是 "http=..;https=..;ftp=.." 的按协议形式
    let addr = if server.contains('=') {
        server.split(';').find_map(|kv| {
            let mut it = kv.splitn(2, '=');
            let k = it.next()?.trim().to_lowercase();
            let v = it.next()?.trim().to_string();
            if v.is_empty() {
                return None;
            }
            if k == "https" || k == "http" || k.is_empty() {
                Some(v)
            } else {
                None
            }
        })?
    } else {
        server.trim().to_string()
    };
    if addr.is_empty() {
        return None;
    }
    Some(format!("http://{addr}"))
}

// ---------- 国漫离线详情（bgm-details.json，构建期由 Archive dump 烘焙） ----------

#[derive(serde::Deserialize)]
pub struct BgmDetail {
    #[serde(default)]
    pub cn: String,
    #[serde(default)]
    pub jp: String,
    #[serde(default)]
    pub year: i64,
    #[serde(default)]
    pub eps: Option<i64>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub const BGM_DETAILS: &[u8] = include_bytes!("../assets/bgm-details.json");

fn bgm_details() -> &'static std::collections::HashMap<i64, BgmDetail> {
    static MAP: std::sync::OnceLock<std::collections::HashMap<i64, BgmDetail>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(|| {
        serde_json::from_slice(BGM_DETAILS)
            .unwrap_or_else(|e| {
                eprintln!("[bgm-details] 解析失败: {e}");
                std::collections::HashMap::new()
            })
    })
}


// ---------- Kitsu 运行时兜底（查一次缓存一次，避免重复搜索） ----------

fn kitsu_cache_path(state: &AppState) -> std::path::PathBuf {
    state.data_dir.join("kitsu-cache.json")
}

fn kitsu_cache_load(state: &AppState) -> std::collections::HashMap<String, String> {
    std::fs::read_to_string(kitsu_cache_path(state))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn kitsu_cache_store(state: &AppState, map: &std::collections::HashMap<String, String>) {
    if let Ok(s) = serde_json::to_string(map) {
        let _ = std::fs::write(kitsu_cache_path(state), s);
    }
}

/// Kitsu 搜海报：标题 → 封面地址。结果（含未命中）写缓存。
async fn kitsu_cover_for(state: &AppState, query: &str) -> Option<String> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    let mut cache = kitsu_cache_load(state);
    if let Some(hit) = cache.get(q) {
        return (!hit.is_empty()).then(|| hit.clone());
    }
    match crate::kitsu::find_cover(&state.http, q).await {
        Ok(Some(url)) => {
            cache.insert(q.to_string(), url.clone());
            kitsu_cache_store(state, &cache);
            Some(url)
        }
        Ok(None) => {
            cache.insert(q.to_string(), String::new());
            kitsu_cache_store(state, &cache);
            None
        }
        Err(e) => {
            eprintln!("[kitsu] 搜索「{q}」失败: {e:#}");
            None
        }
    }
}

fn offline_bgm_meta(bid: i64) -> Option<bangumi::SubjectMeta> {
    let d = bgm_details().get(&bid)?;
    let cn = (!d.cn.trim().is_empty()).then(|| d.cn.trim().to_string());
    let jp = (!d.jp.trim().is_empty()).then(|| d.jp.trim().to_string());
    let summary = (!d.summary.trim().is_empty()).then(|| d.summary.trim().to_string());
    let cover = (!d.cover.trim().is_empty()).then(|| d.cover.trim().to_string());
    let genres = d.tags.clone();
    Some(bangumi::SubjectMeta {
        name_cn: cn,
        name: jp,
        year: (d.year > 0).then_some(d.year),
        episodes: d.eps,
        summary,
        cover_url: cover,
        genres: d.tags.clone(),
    })
}

// ---------- 外观设置（字体 / 文字颜色） ----------


fn md5_of(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn sane_font(name: &str) -> String {
    // 字体名会拼进 CSS font-family 的引号里，剥掉引号/反斜杠/分号防注入
    name.chars()
        .filter(|c| !matches!(c, '"' | '\'' | '\\' | ';' | '{' | '}'))
        .collect::<String>()
        .trim()
        .to_string()
}

fn sane_color(v: &str) -> String {
    let v = v.trim();
    let ok = (v.len() == 4 || v.len() == 7)
        && v.starts_with('#')
        && v[1..].chars().all(|c| c.is_ascii_hexdigit());
    if ok { v.to_lowercase() } else { String::new() }
}

async fn get_appearance(State(st): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let v = {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::get_setting(&conn, "appearance").ok().flatten()
    };
    let parsed = v.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    Json(parsed.unwrap_or_else(|| serde_json::json!({})))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceBody {
    #[serde(default)]
    pub font: String,
    #[serde(default)]
    pub font_custom: String,
    #[serde(default)]
    pub color: String,
}

async fn set_appearance(
    State(st): State<Arc<AppState>>,
    Json(body): Json<AppearanceBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let obj = serde_json::json!({
        "font": sane_font(&body.font),
        "fontCustom": sane_font(&body.font_custom),
        "color": sane_color(&body.color),
    });
    {
        let conn = st.db.lock().expect("db mutex poisoned");
        db::set_setting(&conn, "appearance", &obj.to_string())?;
    }
    Ok(Json(obj))
}

/// 启动后补齐已有国漫作品的封面：库里 bangumi_id 有值但没封面的，
/// 从离线详情里取 Kitsu 图源地址下载（一次最多 60 部，间隔 400ms）
pub async fn heal_bgm_covers(state: Arc<AppState>) {
    let pending: Vec<(i64, Option<i64>, Option<i64>, Option<String>, Option<String>)> = {
        let conn = state.db.lock().expect("db mutex poisoned");
        let mut out = Vec::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT id, bangumi_id, anilist_id, title_cn, title_jp FROM works \
             WHERE (bangumi_id > 0 OR anilist_id > 0) AND (cover_path IS NULL OR cover_path = '')",
        ) {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            }) {
                out = rows.filter_map(Result::ok).collect();
            }
        }
        out
    };
    if pending.is_empty() {
        return;
    }
    println!("[cover-heal] 发现 {} 部作品缺封面，开始补齐", pending.len());
    let mut healed = 0;
    for (id, bid, aid, title_cn, title_jp) in pending.into_iter().take(60) {
        // 来源一：离线详情里烘焙的 Kitsu 地址（国漫）
        let mut url = bid
            .and_then(|b| bgm_details().get(&b))
            .map(|d| d.cover.clone())
            .filter(|c| !c.is_empty() && !c.contains("bgm.tv"));
        // 来源二：Kitsu 按标题搜（日文标题优先）
        if url.is_none() {
            let q = if title_jp.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false) {
                title_jp.clone().unwrap()
            } else {
                title_cn.clone().unwrap_or_default()
            };
            url = kitsu_cover_for(&state, &q).await;
        }
        let Some(url) = url else {
            continue;
        };
        let file = if let Some(b) = bid {
            format!("bgm-{b}.jpg")
        } else if let Some(a) = aid {
            format!("{a}.jpg")
        } else {
            format!("k-{:x}.jpg", md5_of(&url))
        };
        let dest = state.data_dir.join("covers").join(&file);
        match bangumi::download_cover(&state.http, &url, &dest).await {
            Ok(()) => {
                let conn = state.db.lock().expect("db mutex poisoned");
                let _ = conn.execute(
                    "UPDATE works SET cover_path = ?2 WHERE id = ?1",
                    rusqlite::params![id, file],
                );
                healed += 1;
            }
            Err(e) => eprintln!("[cover-heal] 封面下载失败 #{id}: {e:#}"),
        }
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }
    if healed > 0 {
        println!("[cover-heal] 已补齐 {healed} 张封面");
    }
}
