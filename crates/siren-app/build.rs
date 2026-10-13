//! 构建脚本：Windows 下把应用图标嵌入 exe 资源。

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=../../assets/icons/siren.ico");
    let _ = embed_resource::compile("app.rc", embed_resource::NONE);
}
