use crate::error::AppError;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;

/// ロックファイル名。DB / Stronghold と同じデータディレクトリに置く。
const LOCK_FILE_NAME: &str = "pigeon.lock";

/// Pigeon のデータディレクトリに対するプロセス間の排他ロック。
///
/// GUI と CLI が同じ Stronghold スナップショットを同時に開くと、
/// 後から commit した側が先の書き込みを丸ごと上書きしてシークレットが
/// 無言で消える（2026-07-20 実測）。Stronghold 自身は排他ロックを取らない
/// ため、ここで明示的に排他する。
///
/// 判定には flock(2) のアドバイザリロックを使う。ロックファイルの
/// **存在有無では判定しない** — プロセスがクラッシュするとファイルが残り
/// 以後永久に起動できなくなるため。flock は OS がプロセス終了時に自動解放する。
///
/// ロックはこの値が drop されるまで保持される。取得後すぐスコープを
/// 抜けると解放されてしまうので、保持したい期間だけ生かすこと。
pub struct ProcessLock {
    file: File,
}

impl ProcessLock {
    /// `data_dir` に対する排他ロックを取る。他プロセスが保持中なら Err。
    ///
    /// `hint` には呼び出し元に応じた次の行動を渡す（GUI と CLI で案内が異なるため）。
    pub fn acquire_with_hint(data_dir: &Path, hint: &str) -> Result<Self, AppError> {
        Self::acquire_inner(data_dir, hint)
    }

    /// `data_dir` に対する排他ロックを取る。他プロセスが保持中なら Err。
    pub fn acquire(data_dir: &Path) -> Result<Self, AppError> {
        Self::acquire_inner(data_dir, "")
    }

    fn acquire_inner(data_dir: &Path, hint: &str) -> Result<Self, AppError> {
        let path = data_dir.join(LOCK_FILE_NAME);
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| AppError::FileIo(format!("failed to open {}: {e}", path.display())))?;

        // 事実（誰かが保持している）はここで述べ、次の行動（hint）は
        // 呼び出し元が渡す。GUI と CLI で案内が異なるため。
        file.try_lock_exclusive().map_err(|_| {
            let mut msg = "他の Pigeon プロセスが起動中です（GUI または pigeon-cli）。".to_string();
            if !hint.is_empty() {
                msg.push(' ');
                msg.push_str(hint);
            }
            AppError::Validation(msg)
        })?;

        Ok(Self { file })
    }
}

impl Drop for ProcessLock {
    fn drop(&mut self) {
        // 解放はベストエフォート。失敗してもプロセス終了時に OS が解放する。
        let _ = FileExt::unlock(&self.file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_second_lock_fails_while_first_is_held() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = ProcessLock::acquire(dir.path()).expect("1つ目は取得できる");
        assert!(
            ProcessLock::acquire(dir.path()).is_err(),
            "保持中は2つ目を取得できない"
        );
        drop(first);
        assert!(
            ProcessLock::acquire(dir.path()).is_ok(),
            "解放後は再取得できる"
        );
    }
}
