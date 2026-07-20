use clap::{Parser, Subcommand};
use pigeon_lib::cli::{output, progress, runtime::CliRuntime, tty};
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
    /// アカウントのメールを同期する
    Sync { account_id: String },
    /// メールを全文検索する
    Search {
        account_id: String,
        query: String,
        /// 案件で絞り込む
        #[arg(long)]
        project_id: Option<String>,
    },
    /// アカウント一覧を表示する（他コマンドに渡す account_id はここで調べる）
    Accounts,
    /// 案件一覧を表示する
    Projects { account_id: String },
    /// スレッド一覧を表示する
    Threads {
        account_id: String,
        #[arg(default_value = "INBOX")]
        folder: String,
    },
    /// 未読件数を表示する
    Unread { account_id: String },
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

    // ランタイム（DB / SecureStore）を必要としないコマンドを先に片付ける。
    // 実ランタイムの起動は OS キーチェーンへのアクセスを伴うため、
    // 不要なコマンドで開かないようにする。
    match &cli.command {
        // driver 判定は stdin の TTY だけで決まる。
        Commands::Driver => {
            println!("driver={}", driver.as_str());
            return Ok(());
        }
        // 一覧は Registry を見るだけ。
        Commands::Call { list: true, .. } => {
            let infos = registry_only().describe();
            if cli.json {
                let value = serde_json::to_value(&infos).map_err(|e| e.to_string())?;
                println!("{}", output::render(&value, true));
            } else {
                for info in infos {
                    println!("{}", info.name);
                }
            }
            return Ok(());
        }
        // MCP は stdout を JSON-RPC が占有する。ここで返り、下の共通処理
        // （結果の println）を通さない。driver も TTY 判定ではなく
        // Driver::Mcp を使う（監査ログに MCP 経由と残すため）。
        // ランタイムは tools/call で初めて必要になるのでサーバー内で遅延して開く。
        Commands::Mcp => {
            return pigeon_lib::mcp::server::serve_stdio()
                .await
                .map_err(|e| e.to_string())
        }
        _ => {}
    }

    // 名前付きサブコマンドは call と同じく「UseCase 名 + 入力 JSON」に落ちる。
    // 特権的な裏口を作らず、全 driver が同じ dispatch を通る（ADR 0004）。
    let (name, input) = match cli.command {
        Commands::Call { name, input, .. } => {
            let name =
                name.ok_or_else(|| "UseCase 名を指定してください（一覧は --list）".to_string())?;
            let input: serde_json::Value =
                serde_json::from_str(&input).map_err(|e| format!("入力 JSON が不正です: {e}"))?;
            (name, input)
        }
        Commands::Sync { account_id } => (
            "sync_account".to_string(),
            serde_json::json!({ "account_id": account_id }),
        ),
        Commands::Search {
            account_id,
            query,
            project_id,
        } => (
            "search_mails".to_string(),
            serde_json::json!({
                "account_id": account_id,
                "query": query,
                "project_id": project_id,
            }),
        ),
        Commands::Accounts => ("get_accounts".to_string(), serde_json::json!({})),
        Commands::Projects { account_id } => (
            "get_projects".to_string(),
            serde_json::json!({ "account_id": account_id }),
        ),
        Commands::Threads { account_id, folder } => (
            "get_threads".to_string(),
            serde_json::json!({ "account_id": account_id, "folder": folder }),
        ),
        Commands::Unread { account_id } => (
            "get_unread_counts".to_string(),
            serde_json::json!({ "account_id": account_id }),
        ),
        // 上の match で return 済み。到達したらそこの分岐漏れなので
        // panic させずエラーとして返す。
        Commands::Driver | Commands::Mcp => {
            return Err("内部エラー: ランタイム不要のコマンドが処理されませんでした".to_string())
        }
    };

    let runtime = CliRuntime::open(driver).map_err(|e| e.to_string())?;
    // 進捗は stderr へ出し stdout を結果専用に保つ。
    // ライフタイムの都合で sink は ctx より長生きする必要があり、ここで持つ。
    let sink = progress::StderrProgressSink;
    let ctx = runtime.ctx().with_progress(&sink);
    let out = dispatch(runtime.registry(), &name, input, &ctx)
        .await
        .map_err(|e| e.to_string())?;
    println!("{}", output::render(&out, cli.json));
    Ok(())
}
