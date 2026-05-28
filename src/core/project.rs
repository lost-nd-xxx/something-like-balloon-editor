use std::fs;
use std::path::{Path, PathBuf};

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
            fs::copy(&path, project_dir.join(file_name))?;
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
