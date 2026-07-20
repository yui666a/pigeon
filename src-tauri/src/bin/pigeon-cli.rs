use clap::{Parser, Subcommand};
use pigeon_lib::cli::{output, runtime::CliRuntime, tty};
use pigeon_lib::usecase::{cases, dispatch, Registry};

#[derive(Parser)]
#[command(name = "pigeon-cli", about = "Pigeon をコマンドラインから操作する")]
struct Cli {
    /// 結果を JSON で出力する
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// UseCase を名前で直接呼ぶ
    Call {
        /// 登録済み UseCase 名。--list で一覧
        name: Option<String>,
        /// 入力 JSON（例: '{"account_id":"a1"}'）
        #[arg(default_value = "{}")]
        input: String,
        /// 呼べる UseCase 名と入力スキーマを一覧する
        #[arg(long)]
        list: bool,
    },
    /// stdin の TTY 判定から決まる driver を表示する（DB / SecureStore を開かない）
    Driver,
    /// MCP サーバーを stdio で起動する
    Mcp,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// 登録済み UseCase だけを持つレジストリを作る。
/// 一覧表示に DB もキーチェーンも要らないため、ランタイムを開かずに済ませる。
fn registry_only() -> Registry {
    let mut reg = Registry::new();
    cases::register_all(&mut reg);
    reg
}

async fn run(cli: Cli) -> Result<(), String> {
    let driver = tty::current_driver();

    match cli.command {
        // driver 判定は TTY だけで決まるのでランタイムを開く前に処理する。
        // 実ランタイムの起動は OS キーチェーンへのアクセスを伴い、署名前の
        // バイナリでは確認ダイアログでブロックするため、切り離せる形にしておく。
        Commands::Driver => {
            println!("driver={}", driver.as_str());
            Ok(())
        }
        // 一覧は Registry を見るだけ。DB / SecureStore は開かない。
        Commands::Call { list: true, .. } => {
            let registry = registry_only();
            let infos = registry.describe();
            if cli.json {
                let value = serde_json::to_value(&infos).map_err(|e| e.to_string())?;
                println!("{}", output::render(&value, true));
            } else {
                for info in infos {
                    println!("{}", info.name);
                }
            }
            Ok(())
        }
        Commands::Call { name, input, .. } => {
            let name =
                name.ok_or_else(|| "UseCase 名を指定してください（一覧は --list）".to_string())?;
            let input: serde_json::Value =
                serde_json::from_str(&input).map_err(|e| format!("入力 JSON が不正です: {e}"))?;
            let runtime = CliRuntime::open(driver).map_err(|e| e.to_string())?;
            call_and_print(&runtime, &name, input, cli.json).await
        }
        Commands::Mcp => Err("MCP サーバーは未実装です".to_string()),
    }
}

/// UseCase を dispatch し、結果を stdout に出す。
async fn call_and_print(
    runtime: &CliRuntime,
    name: &str,
    input: serde_json::Value,
    as_json: bool,
) -> Result<(), String> {
    let ctx = runtime.ctx();
    let out = dispatch(runtime.registry(), name, input, &ctx)
        .await
        .map_err(|e| e.to_string())?;
    println!("{}", output::render(&out, as_json));
    Ok(())
}
