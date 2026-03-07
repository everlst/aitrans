use std::{env, fs, path::Path};

fn patch_stale_tauri_permission_paths() {
    // Workaround for stale tauri permission path entries like:
    // ".../aitrans-false/src-tauri/target/...".
    const NEEDLES: [&str; 2] = ["-false/src-tauri/target/", "-true/src-tauri/target/"];

    for (key, value) in env::vars() {
        if !key.starts_with("DEP_") || !key.ends_with("_PERMISSION_FILES_PATH") {
            continue;
        }

        let permission_list_file = Path::new(&value);
        let Ok(raw) = fs::read_to_string(permission_list_file) else {
            continue;
        };
        let Ok(mut paths) = serde_json::from_str::<Vec<String>>(&raw) else {
            continue;
        };

        let mut changed = false;
        let mut fixed_count = 0usize;
        for p in &mut paths {
            if Path::new(p).exists() {
                continue;
            }
            for needle in NEEDLES {
                if p.contains(needle) {
                    let candidate = p.replacen(needle, "/src-tauri/target/", 1);
                    if Path::new(&candidate).exists() {
                        *p = candidate;
                        changed = true;
                        fixed_count += 1;
                        break;
                    }
                }
            }
        }

        if changed {
            if let Ok(serialized) = serde_json::to_string(&paths) {
                if let Err(e) = fs::write(permission_list_file, serialized) {
                    println!(
                        "cargo:warning=无法写入修复后的权限路径文件 {}: {}",
                        permission_list_file.display(),
                        e
                    );
                } else {
                    println!(
                        "cargo:warning=修复了 {} 条 tauri 权限路径 ({})",
                        fixed_count,
                        permission_list_file.display()
                    );
                }
            }
        }
    }
}

fn main() {
    patch_stale_tauri_permission_paths();
    tauri_build::build();

    // macOS: 提醒开发者在 dev 模式下需要手动签名以启用 HAL Tap
    #[cfg(target_os = "macos")]
    {
        println!("cargo:warning=📋 macOS 开发提示: 构建完成后请运行 ./src-tauri/dev-codesign.sh 签名二进制文件，以启用应用音频捕获 (HAL Tap)");
    }
}
