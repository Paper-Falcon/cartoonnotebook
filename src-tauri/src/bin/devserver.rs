use std::sync::Arc;

/// 只起 HTTP 服务，不开窗口 —— 用来在命令行里验证接口与数据
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let dir = cartoonnotebook_lib::server::resolve_data_dir();
    let port = cartoonnotebook_lib::server::resolve_port();
    println!("[devserver] data dir = {}", dir.display());
    let state = Arc::new(cartoonnotebook_lib::server::AppState::new(dir, port)?);
    cartoonnotebook_lib::server::serve(state, "0.0.0.0", port).await
}
