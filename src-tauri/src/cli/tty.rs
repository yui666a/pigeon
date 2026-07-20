use crate::usecase::Driver;

/// TTY 判定の結果から driver を決める純関数。
///
/// stdin で判定するのは、利用者が出力をパイプしても
/// （`pigeon-cli search ... | jq`）stdin は端末のまま残るため。
/// stdout で判定するとこのケースを誤って非対話と見なす。
pub fn driver_for(is_tty: bool) -> Driver {
    if is_tty {
        Driver::CliInteractive
    } else {
        Driver::CliAutomated
    }
}

/// 実環境の stdin が端末に接続されているかを返す。
pub fn detect_stdin_tty() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

/// 実環境から driver を決める。
pub fn current_driver() -> Driver {
    driver_for(detect_stdin_tty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tty_maps_to_interactive() {
        assert_eq!(driver_for(true), Driver::CliInteractive);
    }

    #[test]
    fn test_non_tty_maps_to_automated() {
        // エージェント経由の起動（Claude Code の Bash ツール等）はここに落ちる
        assert_eq!(driver_for(false), Driver::CliAutomated);
    }
}
