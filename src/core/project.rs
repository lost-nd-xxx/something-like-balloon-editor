use std::fs;
use std::path::{Path, PathBuf};
use encoding_rs::Encoding;

/// プロジェクトが配置されるベースディレクトリ（実行ファイルと同じディレクトリの `projects` フォルダ）を返します。
pub fn get_projects_base_dir() -> anyhow::Result<PathBuf> {
    let exe_path = std::env::current_exe()?;
    let exe_dir = exe_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("実行ファイルの親ディレクトリの取得に失敗しました"))?;
    // Windows では current_exe() が \\?\ プレフィックス付きパスを返すことがある。
    // std::fs 操作には問題ないが、以降の join や is_dir が予期しない挙動をすることがあるため
    // 通常パスに正規化する。
    let exe_dir = strip_unc_prefix(exe_dir);
    Ok(exe_dir.join("projects"))
}

/// Windows の \\?\ (UNC long-path) プレフィックスを除去して通常パスを返す。
/// 他プラットフォームではそのまま返す。
fn strip_unc_prefix(path: &std::path::Path) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with(r"\\?\") {
        PathBuf::from(&s[4..])
    } else {
        path.to_path_buf()
    }
}

/// 指定されたプロジェクト名に対応するフォルダパスを返します。
pub fn get_project_dir(name: &str) -> anyhow::Result<PathBuf> {
    let base = get_projects_base_dir()?;
    Ok(base.join(name))
}

/// projects/ 配下のプロジェクト名一覧をソートして返します。
pub fn list_projects() -> Vec<String> {
    let Ok(base) = get_projects_base_dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&base) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}

/// 指定された名前のプロジェクトフォルダが既に存在するか判定します。
pub fn project_exists(name: &str) -> bool {
    if let Ok(dir) = get_project_dir(name) {
        dir.exists()
    } else {
        false
    }
}

/// 新規プロジェクトを物理的に作成します。
/// `projects/<name>/` フォルダを作成し、空の `descript.txt` と `install.txt` を配置します。
pub fn create_project_raw(name: &str) -> anyhow::Result<PathBuf> {
    let project_dir = get_project_dir(name)?;
    
    // projects ベースディレクトリと、個別プロジェクトフォルダを作成
    fs::create_dir_all(&project_dir)?;

    // 空の descript.txt と install.txt を生成
    let descript_path = project_dir.join("descript.txt");
    if !descript_path.exists() {
        fs::write(&descript_path, "")?;
    }

    let install_path = project_dir.join("install.txt");
    if !install_path.exists() {
        fs::write(&install_path, "")?;
    }

    Ok(project_dir)
}

/// コピー対象の拡張子
const IMPORT_EXTENSIONS: &[&str] = &["png", "pna", "pnr", "txt"];

/// 既存フォルダの内容を新規プロジェクトとしてコピーして作成します。
/// `src_dir` 内の *.png / *.pna / *.pnr / *.txt を `projects/<name>/` にコピーします。
/// 既に同名プロジェクトが存在する場合は `_backup` に退避し、失敗時はロールバックします。
pub fn create_project_from_folder(src_dir: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let project_dir = get_project_dir(name)?;
    let backup_dir = project_dir.with_file_name(format!("{}_backup", name));

    let exists = project_dir.exists();

    // 1. 既存フォルダがある場合は一時退避
    if exists {
        if backup_dir.exists() {
            fs::remove_dir_all(&backup_dir)?;
        }
        fs::rename(&project_dir, &backup_dir)?;
    }

    // 2. プロジェクトフォルダを作成してファイルをコピー
    let result = (|| -> anyhow::Result<()> {
        fs::create_dir_all(&project_dir)?;

        let entries = fs::read_dir(src_dir)
            .map_err(|e| anyhow::anyhow!("フォルダの読み取りに失敗しました: {}", e))?;

        for entry in entries.flatten() {
            let path = entry.path();

            // profile/ サブフォルダは中身ごとコピー（slbe_files.txt / slbe_profile.json 等）
            if path.is_dir() {
                let dir_name = path.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.to_lowercase())
                    .unwrap_or_default();
                if dir_name == "profile" {
                    let dest_profile = project_dir.join("profile");
                    fs::create_dir_all(&dest_profile)?;
                    for sub in fs::read_dir(&path).into_iter().flatten().flatten() {
                        let sub_path = sub.path();
                        if !sub_path.is_file() { continue; }
                        let sub_name = sub_path.file_name().unwrap();
                        let sub_name_lower = sub_name.to_string_lossy().to_lowercase();
                        let dest = dest_profile.join(sub_name);
                        if sub_name_lower.ends_with(".txt") {
                            convert_txt_to_utf8(&sub_path, &dest)?;
                        } else {
                            fs::copy(&sub_path, &dest)?;
                        }
                    }
                }
                continue;
            }

            if !path.is_file() {
                continue;
            }
            let ext = path.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase());
            let Some(ext) = ext else { continue };
            if !IMPORT_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }
            // updates.txt はネットワーク更新用ファイルのためコピー対象外
            let file_name_lower = path.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.to_lowercase())
                .unwrap_or_default();
            if file_name_lower == "updates.txt" {
                continue;
            }
            let file_name = path.file_name().unwrap();
            // files.txt は profile/slbe_files.txt として取り込む（改名）
            let dest = if file_name_lower == "files.txt" {
                let profile_dir = project_dir.join("profile");
                fs::create_dir_all(&profile_dir)?;
                profile_dir.join("slbe_files.txt")
            } else {
                project_dir.join(file_name)
            };
            if ext == "txt" {
                // txt ファイルは UTF-8 に変換してコピーする
                convert_txt_to_utf8(&path, &dest)?;
            } else {
                fs::copy(&path, &dest)?;
            }
        }
        Ok(())
    })();

    match result {
        Ok(()) => {
            // 成功: バックアップを削除
            if exists && backup_dir.exists() {
                if let Err(e) = fs::remove_dir_all(&backup_dir) {
                    eprintln!("警告: 一時バックアップの削除に失敗しました: {}", e);
                }
            }
            Ok(project_dir)
        }
        Err(e) => {
            // 失敗: ロールバック
            if project_dir.exists() {
                let _ = fs::remove_dir_all(&project_dir);
            }
            if exists && backup_dir.exists() {
                let _ = fs::rename(&backup_dir, &project_dir);
            }
            Err(anyhow::anyhow!("プロジェクトの作成に失敗しました。ロールバックを実行しました。エラー: {}", e))
        }
    }
}

/// 重複対策を統合した安全なプロジェクト作成処理。
/// 既に同名フォルダが存在する場合、一時的に `_backup` にリネーム退避し、
/// 新規作成の成功を確認した上でバックアップを削除します。
/// 万が一失敗した場合は、作成中だった中途半端なフォルダをクリーンアップし、バックアップから元の状態にロールバックします。
pub fn create_project_safe(name: &str) -> anyhow::Result<PathBuf> {
    let project_dir = get_project_dir(name)?;
    let backup_dir = project_dir.with_file_name(format!("{}_backup", name));

    let exists = project_dir.exists();

    // 1. 既存フォルダがある場合は一時退避
    if exists {
        // もし古いバックアップが残っていれば事前に削除
        if backup_dir.exists() {
            fs::remove_dir_all(&backup_dir)?;
        }
        fs::rename(&project_dir, &backup_dir)?;
    }

    // 2. 新規プロジェクトの作成を実行
    match create_project_raw(name) {
        Ok(dir) => {
            // 作成成功: バックアップがあれば完全に削除
            if exists && backup_dir.exists() {
                if let Err(e) = fs::remove_dir_all(&backup_dir) {
                    // バックアップの削除失敗は警告のみに留め、作成自体は成功とする
                    eprintln!("警告: 一時バックアップの削除に失敗しました: {}", e);
                }
            }
            Ok(dir)
        }
        Err(e) => {
            // 作成失敗: ロールバック処理
            if project_dir.exists() {
                let _ = fs::remove_dir_all(&project_dir);
            }
            if exists && backup_dir.exists() {
                let _ = fs::rename(&backup_dir, &project_dir);
            }
            Err(anyhow::anyhow!("プロジェクトの新規作成に失敗しました。ロールバックを実行しました。エラー: {}", e))
        }
    }
}

/// txt ファイルを読み込み、UTF-8 に変換して書き出す。
/// BOM 付き UTF-8 / 純粋 UTF-8 はそのままコピー。
/// Shift_JIS / EUC-JP 等は encoding_rs で変換する。
/// 変換できないバイトは U+FFFD で置換して続行する。
fn convert_txt_to_utf8(src: &Path, dest: &Path) -> anyhow::Result<()> {
    let bytes = fs::read(src)?;

    // BOM 付き UTF-8 または BOM なし UTF-8 はそのまま書き出す
    let check_bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    if std::str::from_utf8(check_bytes).is_ok() {
        fs::write(dest, &bytes)?;
        return Ok(());
    }

    // charset 行からエンコーディングを取得（なければ Shift_JIS を試みる）
    let charset = detect_charset_from_bytes(&bytes);
    let encoding = charset
        .as_deref()
        .and_then(|cs| Encoding::for_label(cs.as_bytes()))
        .unwrap_or(encoding_rs::SHIFT_JIS);

    let (cow, _, _had_errors) = encoding.decode(&bytes);
    fs::write(dest, cow.as_bytes())?;
    Ok(())
}

/// バイト列の先頭部分から `charset,xxx` 行を探して返す（見つからなければ None）
fn detect_charset_from_bytes(bytes: &[u8]) -> Option<String> {
    // 最初の 2KB だけ検査（エンコーディング不明なのでバイト列で検索）
    let head = &bytes[..bytes.len().min(2048)];
    // ASCII 互換として行単位で切る
    for line in head.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.starts_with(b"//") { continue; }
        if let Some(rest) = line.strip_prefix(b"charset,") {
            let cs = std::str::from_utf8(rest).ok()?.trim().to_string();
            if !cs.is_empty() { return Some(cs); }
        }
    }
    None
}

/// フォルダ内の descript.txt を読み込んで、copypen/none 以外の blendmethod 値を返す。
/// 問題がなければ空の Vec を返す。
pub fn check_blendmethod_warnings(project_dir: &Path) -> Vec<String> {
    let descript_path = project_dir.join("descript.txt");
    let Ok(text) = fs::read_to_string(&descript_path) else { return Vec::new() };

    let mut warnings = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("//") || line.is_empty() { continue; }
        if let Some(pos) = line.find(',') {
            let key = line[..pos].trim();
            let val = line[pos+1..].trim().to_lowercase();
            if (key == "cursor.blendmethod" || key == "anchor.blendmethod")
                && val != "none" && val != "copypen"
            {
                warnings.push(format!(
                    "{}={} (このアプリでは描画を再現できません)",
                    key, &line[pos+1..].trim()
                ));
            }
        }
    }
    warnings
}
