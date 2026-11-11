use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=assets/app.ico");

    // 非 Windows 目标跳过资源编译
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if !Path::new("assets/app.ico").exists() {
        println!("cargo:warning=assets/app.ico 缺失，exe 将没有自定义图标");
        return;
    }

    // 资源只影响外观（图标 / 版本信息），没有 manifest 也能正常链接
    if let Err(e) = embed_resource::compile("app.rc", embed_resource::NONE).manifest_optional() {
        println!("cargo:warning=资源编译失败（{e}），exe 将使用默认图标");
    }
}
