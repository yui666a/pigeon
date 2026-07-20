use pigeon_lib::cli::{runtime::CliRuntime, tty};

#[tokio::main]
async fn main() {
    let driver = tty::current_driver();

    // --driver-probe は DB / SecureStore を開かずに driver 判定だけを出す。
    // 実ランタイムの起動は OS キーチェーンへのアクセスを伴い、署名前の
    // バイナリでは確認ダイアログでブロックするため、TTY 判定の確認を
    // それと切り離せるようにしておく。
    if std::env::args().any(|a| a == "--driver-probe") {
        println!("driver={}", driver.as_str());
        return;
    }

    let runtime = match CliRuntime::open(driver) {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    // Task 10 でサブコマンド処理に差し替える
    println!(
        "driver={} usecases={}",
        driver.as_str(),
        runtime.registry().names().len()
    );
}
