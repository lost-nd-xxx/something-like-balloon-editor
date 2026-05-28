use std::fs;
use std::path::PathBuf;

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
