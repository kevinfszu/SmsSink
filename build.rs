//! 把图标、manifest、版本信息编进 exe 资源。
//!
//! 版本号只有一处真相：`Cargo.toml` 的 `version`。`app.rc` 里是占位符，
//! 这里替换后生成到 `OUT_DIR` 再交给 embed-resource 编译，避免发布时
//! Cargo.toml 与资源版本各说一套。

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// 资源模板（项目根）。占位符：
/// `{ICON_PATH}` `{MANIFEST_PATH}` `{FILE_VERSION}` `{VERSION_STR}`
const RC_TEMPLATE: &str = "app.rc";

fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=assets/app.ico");

    // 非 Windows 目标跳过资源编译
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if !Path::new("assets/app.ico").exists() {
        println!("cargo:warning=assets/app.ico 缺失，exe 将没有自定义图标");
        return;
    }

    // 0.2.0 → FILEVERSION 0,2,0,0 / "0.2.0.0"
    // 预发布后缀（1.2.3-beta.1）只取数字部分，资源版本不支持语义化后缀
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());
    let mut nums: Vec<u64> = version
        .split('.')
        .map(|seg| {
            seg.split(['-', '+'])
                .next()
                .unwrap_or("0")
                .parse()
                .unwrap_or(0)
        })
        .collect();
    while nums.len() < 4 {
        nums.push(0);
    }
    let file_version = format!("{},{},{},{}", nums[0], nums[1], nums[2], nums[3]);
    let version_str = format!("{}.{}.{}.{}", nums[0], nums[1], nums[2], nums[3]);

    // 生成的 rc 落在 OUT_DIR，相对路径会失效，一律换成绝对路径；
    // RC 编译器把反斜杠当转义，统一用正斜杠
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string()));
    let icon = root.join("assets/app.ico");
    let manifest = root.join("app.manifest");
    let icon = icon.to_string_lossy().replace('\\', "/");
    let manifest = manifest.to_string_lossy().replace('\\', "/");

    let template = match fs::read_to_string(RC_TEMPLATE) {
        Ok(text) => text,
        Err(e) => {
            println!("cargo:warning=读取 {RC_TEMPLATE} 失败（{e}），exe 将没有自定义图标");
            return;
        }
    };
    let rc = template
        .replace("{ICON_PATH}", &icon)
        .replace("{MANIFEST_PATH}", &manifest)
        .replace("{FILE_VERSION}", &file_version)
        .replace("{VERSION_STR}", &version_str);

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR 由 cargo 提供"));
    let rc_path = out_dir.join("app.generated.rc");
    if let Err(e) = fs::write(&rc_path, rc) {
        println!("cargo:warning=写入生成的 rc 失败（{e}），exe 将没有自定义图标");
        return;
    }

    // 资源只影响外观（图标 / 版本信息），没有 manifest 也能正常链接
    if let Err(e) = embed_resource::compile(&rc_path, embed_resource::NONE).manifest_optional() {
        println!("cargo:warning=资源编译失败（{e}），exe 将使用默认图标");
    }
}
