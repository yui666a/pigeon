fn main() {
    // OAuth 定数（ADR 0003 のビルド時定数）を option_env! でバイナリへ焼き込む。
    // CI のビルドキャッシュ（rust-cache 等）はロックファイル基準でキーを作るため、
    // これらの環境変数が変わってもキャッシュヒットで再コンパイルされず、値が
    // 焼き込まれないことがある。明示的に rerun-if-env-changed を宣言して、
    // 環境変数の変化でクレートを再コンパイルさせる。
    for key in [
        "PIGEON_GOOGLE_CLIENT_ID_DESKTOP",
        "PIGEON_GOOGLE_CLIENT_SECRET_DESKTOP",
        "PIGEON_GOOGLE_CLIENT_ID_IOS",
        "PIGEON_GOOGLE_CLIENT_SECRET_IOS",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }

    tauri_build::build()
}
