fn main() {
    // 前端产物是编译期内嵌进二进制的（rust-embed），
    // 不声明这条依赖的话，改了前端 cargo 不会重编，端出来的还是旧页面。
    println!("cargo:rerun-if-changed=../dist");

    tauri_build::build()
}
