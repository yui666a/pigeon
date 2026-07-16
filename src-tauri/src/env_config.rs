//! 開発時の `.env`（OAuth クライアント ID/シークレット等のビルド時定数）の読み込み。
//!
//! `dotenvy::dotenv()` は cwd を起点に `.env` を探すため、`open` 経由で起動された
//! `.app` バンドル（cwd が `/` になる）では開発時の `.env` を見つけられない。
//! 実行ファイルの位置を起点に上方探索するフォールバックを持たせ、
//! `cargo run` / `tauri dev` / debug/release バンドルの `open` 起動のいずれでも、
//! プロジェクト直下の `.env` を拾えるようにする（開発体験の改善）。
//!
//! **これは開発時専用の便宜である。** 配布ビルドの `.app` には `.env` が同梱されず
//! （`.gitignore` 済み）、`/Applications` 配下を上方探索しても見つからないため、
//! `load_dotenv()` は何もしない。配布アプリで OAuth 定数を供給するには
//! ビルド時埋め込み（`env!` / `option_env!`）が必要で、それは OAuthConfig 側で
//! 環境変数のフォールバックとして扱う（ADR 0003: ビルド時定数はバイナリ埋め込み）。

use std::path::{Path, PathBuf};

/// `start` から親方向へ辿り、最初に見つかった `.env` のパスを返す。
/// リポジトリ配置（`.app` は `target/**/` 配下、dev バイナリは `target/` 配下）でも
/// プロジェクト直下の `.env` に到達できる。
pub fn find_dotenv_upwards(start: &Path) -> Option<PathBuf> {
    for dir in start.ancestors() {
        let candidate = dir.join(".env");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// `.env` を読み込む。cwd 起点（従来動作）で見つからなければ、実行ファイルの
/// ディレクトリを起点に上方探索する。どちらでも見つからなければ何もしない。
pub fn load_dotenv() {
    if dotenvy::dotenv().is_ok() {
        return;
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    if let Some(dir) = exe_dir {
        if let Some(env_path) = find_dotenv_upwards(&dir) {
            let _ = dotenvy::from_path(&env_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_finds_dotenv_in_start_dir() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".env"), "K=V").unwrap();
        let found = find_dotenv_upwards(dir.path()).unwrap();
        assert_eq!(found, dir.path().join(".env"));
    }

    #[test]
    fn test_finds_dotenv_in_ancestor() {
        // 実行ファイルが target/debug/bundle/macos/Pigeon.app/... の深い位置にあり、
        // .env はプロジェクト直下、という配置を模す
        let root = TempDir::new().unwrap();
        fs::write(root.path().join(".env"), "K=V").unwrap();
        let deep = root
            .path()
            .join("target/debug/bundle/macos/Pigeon.app/Contents/MacOS");
        fs::create_dir_all(&deep).unwrap();

        let found = find_dotenv_upwards(&deep).unwrap();
        assert_eq!(found, root.path().join(".env"));
    }

    #[test]
    fn test_returns_none_when_no_dotenv() {
        let dir = TempDir::new().unwrap();
        assert!(find_dotenv_upwards(dir.path()).is_none());
    }

    #[test]
    fn test_picks_nearest_dotenv() {
        // 途中ディレクトリにも .env があれば、より近い方（子側）を優先する
        let root = TempDir::new().unwrap();
        fs::write(root.path().join(".env"), "ROOT=1").unwrap();
        let mid = root.path().join("a/b");
        fs::create_dir_all(&mid).unwrap();
        fs::write(mid.join(".env"), "MID=1").unwrap();
        let start = mid.join("c");
        fs::create_dir_all(&start).unwrap();

        assert_eq!(find_dotenv_upwards(&start).unwrap(), mid.join(".env"));
    }
}
