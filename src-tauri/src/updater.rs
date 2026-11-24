use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

const REPO_API_LATEST: &str = "https://api.github.com/repos/yanm299-bit/kurodog/releases/latest";
const REPO_RELEASES_PAGE: &str = "https://github.com/yanm299-bit/kurodog/releases/latest";

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub release_url: String,
    pub asset_name: Option<String>,
    pub download_url: Option<String>,
    pub asset_sha256: Option<String>,
    pub asset_size: Option<u64>,
    pub install_kind: String,
    pub notes_summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgressEvent {
    pub stage: String,
    pub message: String,
    pub info: Option<UpdateInfo>,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: Option<String>,
    body: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: Option<u64>,
    digest: Option<String>,
}

pub fn parse_semver(raw: &str) -> (u64, u64, u64) {
    let clean = raw.trim().trim_start_matches(['v', 'V']);
    let main = clean.split('-').next().unwrap_or(clean);
    let mut parts = main.split('.');
    let major = parts.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    (major, minor, patch)
}

pub fn is_newer_version(latest: &str, current: &str) -> bool {
    parse_semver(latest) > parse_semver(current)
}

fn extract_sha256(digest: &Option<String>) -> Option<String> {
    let raw = digest.as_deref()?.trim();
    let hex = raw.strip_prefix("sha256:").unwrap_or(raw).trim().to_ascii_lowercase();
    if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(hex)
    } else {
        None
    }
}

fn detect_install_kind() -> &'static str {
    #[cfg(windows)]
    {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                if dir.join("uninstall.exe").exists() {
                    return "windows-nsis";
                }
            }
        }
        return "windows-portable";
    }
    #[cfg(target_os = "macos")]
    {
        return "macos-app";
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        return "manual";
    }
}

fn select_asset<'a>(assets: &'a [GhAsset], install_kind: &str) -> Option<&'a GhAsset> {
    let want_108 = cfg!(feature = "layout-108");
    match install_kind {
        "windows-nsis" => assets
            .iter()
            .find(|a| {
                let n = a.name.to_ascii_lowercase();
                n.ends_with("-setup.exe") && n.contains("x64") && (n.contains("108key") == want_108)
            })
            .or_else(|| {
                assets.iter().find(|a| {
                    let n = a.name.to_ascii_lowercase();
                    n.ends_with("-setup.exe") && n.contains("x64")
                })
            })
            .or_else(|| {
                assets
                    .iter()
                    .find(|a| a.name.to_ascii_lowercase().ends_with("-windows-x64.exe"))
            }),
        "windows-portable" => assets
            .iter()
            .find(|a| {
                let n = a.name.to_ascii_lowercase();
                n.ends_with("-windows-x64.exe") && (n.contains("108key") == want_108)
            })
            .or_else(|| {
                assets
                    .iter()
                    .find(|a| a.name.to_ascii_lowercase().ends_with("-windows-x64.exe"))
            })
            .or_else(|| {
                assets
                    .iter()
                    .find(|a| a.name.to_ascii_lowercase().ends_with("-setup.exe"))
            }),
        "macos-app" => assets
            .iter()
            .find(|a| a.name.to_ascii_lowercase().ends_with("-macos-universal.app.tar.gz"))
            .or_else(|| {
                assets
                    .iter()
                    .find(|a| a.name.to_ascii_lowercase().ends_with("-macos-universal.dmg"))
            }),
        _ => None,
    }
}

fn summarize_notes(body: Option<&str>) -> String {
    let Some(raw) = body else {
        return String::new();
    };
    raw.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(180)
        .collect()
}

#[cfg(windows)]
fn fetch_latest_release_json() -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let script = format!(
        "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
         [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; \
         $ProgressPreference = 'SilentlyContinue'; \
         $resp = Invoke-WebRequest -UseBasicParsing -Uri '{url}' \
           -Headers @{{'User-Agent'='KuroDog/{ver}'; 'Accept'='application/vnd.github+json'}} -TimeoutSec 15; \
         Write-Output $resp.Content",
        url = REPO_API_LATEST,
        ver = CURRENT_VERSION
    );

    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("无法启动网络检测: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "连接 GitHub 检查更新失败，请检查网络或代理".to_string()
        } else {
            format!("检查更新失败: {}", stderr.lines().next().unwrap_or(&stderr))
        });
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        return Err("GitHub 返回空响应".to_string());
    }
    Ok(text)
}

#[cfg(not(windows))]
fn fetch_latest_release_json() -> Result<String, String> {
    use std::process::Command;

    let output = Command::new("/usr/bin/curl")
        .args([
            "-fsSL",
            "--connect-timeout",
            "10",
            "--max-time",
            "20",
            "-H",
            &format!("User-Agent: KuroDog/{}", CURRENT_VERSION),
            "-H",
            "Accept: application/vnd.github+json",
            REPO_API_LATEST,
        ])
        .output()
        .map_err(|e| format!("无法调用 curl 检查更新: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "连接 GitHub 检查更新失败，请检查网络或代理".to_string()
        } else {
            format!("检查更新失败: {}", stderr)
        });
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        return Err("GitHub 返回空响应".to_string());
    }
    Ok(text)
}

pub fn check_latest_update() -> Result<UpdateInfo, String> {
    let raw_json = fetch_latest_release_json()?;
    let rel: GhRelease = serde_json::from_str(&raw_json)
        .map_err(|e| format!("解析最新版本信息失败: {}", e))?;

    let latest_clean = rel
        .tag_name
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_string();
    let has_update = is_newer_version(&latest_clean, CURRENT_VERSION);
    let install_kind = detect_install_kind();
    let asset = select_asset(&rel.assets, install_kind);

    Ok(UpdateInfo {
        current_version: CURRENT_VERSION.to_string(),
        latest_version: latest_clean,
        has_update,
        release_url: rel
            .html_url
            .unwrap_or_else(|| REPO_RELEASES_PAGE.to_string()),
        asset_name: asset.map(|a| a.name.clone()),
        download_url: asset.map(|a| a.browser_download_url.clone()),
        asset_sha256: asset.and_then(|a| extract_sha256(&a.digest)),
        asset_size: asset.and_then(|a| a.size),
        install_kind: install_kind.to_string(),
        notes_summary: summarize_notes(rel.body.as_deref()),
    })
}

fn emit_progress(app: &AppHandle, stage: &str, message: impl Into<String>, info: Option<UpdateInfo>) {
    let _ = app.emit(
        "kuro-update-status",
        UpdateProgressEvent {
            stage: stage.to_string(),
            message: message.into(),
            info,
        },
    );
}

pub fn open_url_in_browser(url: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("cmd.exe")
            .args(["/C", "start", "", url])
            .creation_flags(0x08000000)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("/usr/bin/open").arg(url).spawn();
    }
}

#[cfg(windows)]
fn download_and_verify_windows(url: &str, dest: &PathBuf, expected_sha256: Option<&str>) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let url_esc = url.replace('\'', "''");
    let dst_esc = dest.to_string_lossy().replace('\'', "''");
    let sha_esc = expected_sha256.unwrap_or("").replace('\'', "''");

    let script = format!(
        "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
         [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; \
         $ProgressPreference = 'SilentlyContinue'; \
         $dst = '{dst}'; \
         Invoke-WebRequest -UseBasicParsing -Uri '{url}' -Headers @{{'User-Agent'='KuroDog/{ver}'}} -OutFile $dst -TimeoutSec 180; \
         if (-not (Test-Path $dst) -or (Get-Item $dst).Length -lt 500000) {{ throw '下载文件不完整' }}; \
         $exp = '{sha}'; \
         if ($exp -ne '') {{ \
           $actual = (Get-FileHash -Path $dst -Algorithm SHA256).Hash.ToLowerInvariant(); \
           if ($actual -ne $exp.ToLowerInvariant()) {{ Remove-Item $dst -Force -ErrorAction SilentlyContinue; throw \"SHA-256 校验失败: $actual\" }} \
         }}",
        dst = dst_esc,
        url = url_esc,
        ver = CURRENT_VERSION,
        sha = sha_esc,
    );

    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("启动下载失败: {}", e))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if err.is_empty() {
            "下载安装包失败".to_string()
        } else {
            err.lines().next().unwrap_or(&err).to_string()
        });
    }
    Ok(())
}

#[cfg(windows)]
fn install_windows_update(app: &AppHandle, info: &UpdateInfo) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let url = info
        .download_url
        .as_deref()
        .ok_or_else(|| "未找到匹配的 Windows 更新包".to_string())?;
    let asset_name = info
        .asset_name
        .as_deref()
        .unwrap_or("KuroDog-update.exe");

    emit_progress(
        app,
        "downloading",
        format!("正在下载 v{}...", info.latest_version),
        Some(info.clone()),
    );

    let temp_dir = std::env::temp_dir().join("kurodog-updates");
    let _ = std::fs::create_dir_all(&temp_dir);
    let safe_name = asset_name.replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|'], "_");
    let target_file = temp_dir.join(safe_name);

    download_and_verify_windows(url, &target_file, info.asset_sha256.as_deref())?;

    emit_progress(
        app,
        "installing",
        format!("校验通过，正在安装 v{} 并重启...", info.latest_version),
        Some(info.clone()),
    );

    let is_setup_installer = asset_name.to_ascii_lowercase().ends_with("-setup.exe");

    if is_setup_installer {
        // Launch Tauri NSIS installer in passive auto-relaunch update mode (/P /R /UPDATE)
        Command::new(&target_file)
            .args(["/P", "/R", "/UPDATE"])
            .spawn()
            .map_err(|e| format!("启动安装程序失败: {}", e))?;
    } else {
        // Portable single .exe atomic swap + relaunch after this process exits
        let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let pid = std::process::id();
        let src_esc = target_file.to_string_lossy().replace('\'', "''");
        let dst_esc = current_exe.to_string_lossy().replace('\'', "''");
        let swap_script = format!(
            "for ($i = 0; $i -lt 40; $i++) {{ \
               if (-not (Get-Process -Id {pid} -ErrorAction SilentlyContinue)) {{ break }} \
               Start-Sleep -Milliseconds 200 \
             }}; \
             Start-Sleep -Milliseconds 250; \
             Copy-Item -LiteralPath '{src}' -Destination '{dst}' -Force; \
             Remove-Item -LiteralPath '{src}' -Force -ErrorAction SilentlyContinue; \
             Start-Process -FilePath '{dst}'",
            pid = pid,
            src = src_esc,
            dst = dst_esc,
        );

        Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &swap_script,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("启动替换脚本失败: {}", e))?;
    }

    let app_clone = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(450));
        app_clone.exit(0);
    });

    Ok(())
}

#[cfg(target_os = "macos")]
fn find_current_macos_app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    for ancestor in exe.ancestors() {
        if ancestor.extension().and_then(|s| s.to_str()) == Some("app") {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn install_macos_update(app: &AppHandle, info: &UpdateInfo) -> Result<(), String> {
    use std::process::Command;

    let url = info
        .download_url
        .as_deref()
        .ok_or_else(|| "未找到匹配的 macOS 更新包".to_string())?;
    let asset_name = info
        .asset_name
        .as_deref()
        .unwrap_or("KuroDog-macos-universal.app.tar.gz");

    if !asset_name.to_ascii_lowercase().ends_with(".tar.gz") {
        open_url_in_browser(url);
        return Ok(());
    }

    let app_bundle = find_current_macos_app_bundle()
        .unwrap_or_else(|| PathBuf::from("/Applications/KuroDog.app"));

    emit_progress(
        app,
        "downloading",
        format!("正在下载 macOS v{}...", info.latest_version),
        Some(info.clone()),
    );

    let temp_dir = std::env::temp_dir().join(format!("kurodog-update-{}", info.latest_version));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;
    let archive_path = temp_dir.join("KuroDog-update.app.tar.gz");

    let dl = Command::new("/usr/bin/curl")
        .args([
            "-fsSL",
            "--connect-timeout",
            "15",
            "--max-time",
            "240",
            "-H",
            &format!("User-Agent: KuroDog/{}", CURRENT_VERSION),
            "-o",
            archive_path.to_str().unwrap_or("/tmp/KuroDog-update.app.tar.gz"),
            url,
        ])
        .output()
        .map_err(|e| format!("下载失败: {}", e))?;

    if !dl.status.success() {
        return Err("下载 macOS 更新包失败".to_string());
    }

    // Verify SHA-256 if GitHub Release provided a digest
    if let Some(expected_sha) = info.asset_sha256.as_deref() {
        emit_progress(
            app,
            "verifying",
            "正在校验安装包完整性...",
            Some(info.clone()),
        );
        let shasum = Command::new("/usr/bin/shasum")
            .args(["-a", "256", archive_path.to_str().unwrap_or("")])
            .output()
            .map_err(|e| format!("校验失败: {}", e))?;
        let out_str = String::from_utf8_lossy(&shasum.stdout);
        let actual = out_str.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
        if actual != expected_sha.to_ascii_lowercase() {
            return Err("安装包 SHA-256 校验未通过".to_string());
        }
    }

    let extract_dir = temp_dir.join("extracted");
    std::fs::create_dir_all(&extract_dir).map_err(|e| e.to_string())?;
    let untar = Command::new("/usr/bin/tar")
        .args([
            "-xzf",
            archive_path.to_str().unwrap_or(""),
            "-C",
            extract_dir.to_str().unwrap_or(""),
        ])
        .output()
        .map_err(|e| format!("解压失败: {}", e))?;
    if !untar.status.success() {
        return Err("解压更新包失败".to_string());
    }

    // Find extracted .app (KuroDog.app or KuroDogPet.app)
    let new_app = std::fs::read_dir(&extract_dir)
        .ok()
        .and_then(|mut it| {
            it.find_map(|entry| {
                let p = entry.ok()?.path();
                if p.extension().and_then(|s| s.to_str()) == Some("app") {
                    Some(p)
                } else {
                    None
                }
            })
        })
        .ok_or_else(|| "更新包内未找到 .app 程序".to_string())?;

    // Clear quarantine flag so macOS Gatekeeper won't block the freshly updated open-source app
    let _ = Command::new("/usr/bin/xattr")
        .args(["-dr", "com.apple.quarantine", new_app.to_str().unwrap_or("")])
        .status();

    emit_progress(
        app,
        "installing",
        format!("正在安装 v{} 并重启...", info.latest_version),
        Some(info.clone()),
    );

    let pid = std::process::id();
    let dst_str = app_bundle.to_string_lossy().replace('\'', "'\\''");
    let src_str = new_app.to_string_lossy().replace('\'', "'\\''");
    let tmp_str = temp_dir.to_string_lossy().replace('\'', "'\\''");
    let swap_sh = format!(
        "for i in $(seq 1 40); do kill -0 {pid} 2>/dev/null || break; sleep 0.2; done; \
         sleep 0.25; \
         rm -rf '{dst}.bak'; \
         if [ -d '{dst}' ]; then mv '{dst}' '{dst}.bak'; fi; \
         if mv '{src}' '{dst}'; then \
           rm -rf '{dst}.bak' '{tmp}'; \
           /usr/bin/xattr -dr com.apple.quarantine '{dst}' 2>/dev/null || true; \
           /usr/bin/open -n '{dst}'; \
         else \
           if [ -d '{dst}.bak' ]; then mv '{dst}.bak' '{dst}'; fi; \
           /usr/bin/open '{url}'; \
         fi",
        pid = pid,
        dst = dst_str,
        src = src_str,
        tmp = tmp_str,
        url = info.release_url.replace('\'', "'\\''"),
    );

    Command::new("/bin/sh")
        .args(["-c", &swap_sh])
        .spawn()
        .map_err(|e| format!("启动更新脚本失败: {}", e))?;

    let app_clone = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(450));
        app_clone.exit(0);
    });

    Ok(())
}

pub fn perform_update_install(app: &AppHandle, info: UpdateInfo) -> Result<(), String> {
    if !info.has_update {
        emit_progress(
            app,
            "up_to_date",
            format!("已是最新版本 v{}", info.current_version),
            Some(info),
        );
        return Ok(());
    }

    #[cfg(windows)]
    {
        return install_windows_update(app, &info);
    }

    #[cfg(target_os = "macos")]
    {
        return install_macos_update(app, &info);
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        open_url_in_browser(&info.release_url);
        Ok(())
    }
}

pub fn start_background_update_checker(app: AppHandle) {
    std::thread::spawn(move || {
        // Gentle initial delay after launch so startup stays instant & quiet
        std::thread::sleep(std::time::Duration::from_secs(8));
        loop {
            if let Ok(info) = check_latest_update() {
                if info.has_update {
                    emit_progress(
                        &app,
                        "available",
                        format!("发现新版本 v{}", info.latest_version),
                        Some(info),
                    );
                }
            }
            // Re-check every 6 hours while running
            std::thread::sleep(std::time::Duration::from_secs(6 * 3600));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison_works() {
        assert!(is_newer_version("4.4.0", "4.3.0"));
        assert!(is_newer_version("v4.10.0", "4.4.0"));
        assert!(is_newer_version("5.0.0", "4.9.9"));
        assert!(!is_newer_version("4.4.0", "4.4.0"));
        assert!(!is_newer_version("v4.3.9", "4.4.0"));
    }

    #[test]
    fn sha256_extraction_validates_hex() {
        let valid = Some("sha256:05c1be2c444f5303770f6d59e216bc0ed7b69321fc44368e4ec231405570f950".to_string());
        assert_eq!(
            extract_sha256(&valid).as_deref(),
            Some("05c1be2c444f5303770f6d59e216bc0ed7b69321fc44368e4ec231405570f950")
        );
        assert_eq!(extract_sha256(&Some("sha256:short".to_string())), None);
    }
}
