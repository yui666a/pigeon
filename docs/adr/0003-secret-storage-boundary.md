# ADR 0003: 機密情報の保管境界

## ステータス

確定（2026-07-14）

## コンテキスト

Pigeon は複数種類の秘密情報を扱う。

- IMAP/SMTP の PLAIN 認証パスワード
- Gmail の OAuth 2.0 トークン（アクセストークン / リフレッシュトークン）
- Claude API キー（Anthropic 直）
- GCP サービスアカウント JSON（Claude on Vertex AI 用）

これらと同時に、秘密ではない設定値も存在する。

- 使用する LLM プロバイダ名（`ollama` / `claude` / `claude_vertex` / `openai`）
- Ollama のエンドポイント URL とモデル名
- Claude / Vertex のモデル名、GCP プロジェクト ID、リージョン

これまで「どの情報をどこに置くか」の判断は、認証設計書（`2026-04-13-oauth-support-design.md`）と LLM プロバイダ選択設計書（`2026-07-10-llm-provider-selection-design.md`）にそれぞれ別々に書かれており、全体を貫く単一の規範として集約されていなかった。新しい秘密情報を扱う機能を追加するたびに配置判断を各設計書から拾い直す必要があり、平文 DB に秘密が混入する事故のリスクがあった。

本 ADR はこの保管境界を一箇所に集約し、以降の配置判断の基準とする。

## 決定

秘密情報と非機密設定を、保管先で明確に分離する。

### 秘密情報は SecureStore（Stronghold）に暗号化保管する

パスワード、OAuth トークン、API キー、サービスアカウント JSON などの秘密情報は、`iota_stronghold` を用いた暗号化ストア（コード上の名称は `SecureStore`、設計書上の呼称は SecureStore / Stronghold）に保管する。実体は `src-tauri/src/secure_store.rs` の `SecureStore` であり、`insert` / `get` / `delete` の key-value インタフェースを持つ暗号化スナップショットファイルである。

Stronghold のキー命名規約は次のとおりとする。

| キー | 内容 |
|---|---|
| `oauth_{account_id}` | OAuth トークン一式（access_token / refresh_token / expires_at / email）を JSON でまとめたもの |
| `password_{account_id}` | PLAIN 認証パスワード |
| `claude_api_key` | Claude API キー（Anthropic 直） |
| `vertex_sa_json` | GCP サービスアカウント JSON 全体 |
| `openai_api_key` | 将来用（現状は口だけ） |

アカウント単位の秘密（トークン・パスワード）は `{種別}_{account_id}` 形式でアカウントごとに分離する。アプリ全体で単一のプロバイダ資格情報（API キー・SA JSON）は固定キーとする。

### 非機密設定は SQLite の settings テーブルに平文で保管する

モデル名、エンドポイント、プロバイダ名、GCP プロジェクト ID、リージョンといった秘密でない設定値は、SQLite の `settings` テーブル（key-value, 平文）に保管する。

| キー | 説明 |
|---|---|
| `llm_provider` | 使用プロバイダ（`ollama` / `claude` / `claude_vertex` / `openai`） |
| `ollama_endpoint` / `ollama_model` | Ollama 接続設定 |
| `claude_model` | Claude モデル名 |
| `vertex_project_id` / `vertex_location` / `vertex_model` | Vertex 接続設定 |

### 平文の DB に秘密を置かない原則

SQLite（`settings` テーブルを含む）には、パスワード・トークン・API キー・サービスアカウント JSON を平文で保存しない。これは例外を設けない絶対の原則とする。UI へ秘密情報を返す場合も本体は返さず、「登録済みか」の真偽値（例: `claude_api_key_set`, `vertex_sa_json_set`）のみを返す。

### マスター鍵の導出と保管（2026-07-15 追記）

Stronghold スナップショットを暗号化する**マスター鍵**は、初回起動時に CSPRNG で生成するデバイス固有の 32byte 乱数とし、OS キーチェーンに保管する。

- 保管先: macOS Keychain / Windows Credential Manager / Linux secret-service（GNOME Keyring 等）。いずれも `keyring` クレート、サービス名 `com.haiso666.pigeon`、アカウント `secure-store-master-key`
- Linux（2026-07-15 実装）: secret-service へは zbus ベースの `async-secret-service` feature で接続する（pure Rust。libdbus 連携の `sync-secret-service` はビルドが壊れやすく、kernel keyutils の `linux-native` は再起動で鍵が消えるため不採用）。デーモン不在（ヘッドレス・CI 等）は実行時にデータディレクトリの `master.key`（権限 0600）へフォールバックする（`FallbackKeyBackend`）
  - 旧 `master.key` 運用からの移行: secret-service が空でファイルに鍵がある場合、鍵を secret-service へ複製する。**ファイルは削除せず残す**: デーモンが一時的に不在の起動でも同じ鍵でスナップショットを開けるようにする可用性優先の判断（消すとデーモン不在時に新しい鍵が生成され、スナップショット退避 + 再認証に至る）。ファイルを消して保護を完成させるのは Linux 正式配布時の課題
  - デーモンが不安定な環境（あるときと無いときが交互に来る）では鍵の不整合により再認証が発生しうる（データはスナップショット退避により失われない）
- キーチェーン系がない他の環境: `master.key`（0600）のみ
- テスト・CI: `MasterKeyBackend` トレイト経由でインメモリ鍵を注入し、実キーチェーンに触れない

ソースコードに鍵素材（固定文字列やソルト）を置かない。鍵がデバイス固有であるため、スナップショットファイルを窃取されても他のデバイス・ソースコードの知識だけでは復号できない。

旧実装（2026-07 以前）は固定文字列の SHA256 をマスター鍵にしており、ソースを読める攻撃者は誰でもスナップショットを復号できた。既存ユーザーのスナップショットは、起動時に旧固定鍵で開けた場合のみ新しいランダム鍵で再暗号化して移行する（`SecureStore::open_with_migration`）。旧固定鍵の文字列はこの一方向の移行のためだけにコードへ残しており、新規の暗号化には使わない。どの鍵でも開けないスナップショットは `.unreadable.bak` に退避して新規作成し、ユーザーに再認証を促す（破壊はしない）。

## 理由

秘密と非機密を分けるのは、それぞれに求める性質が異なるためである。

- **秘密情報**には暗号化保管と漏洩耐性が要る。OS のキーチェーン相当の保護（macOS Keychain Services / Windows Credential Manager / Linux libsecret）を Pigeon では Stronghold で担保する。Stronghold は暗号化スナップショットとしてディスクに置かれるため、DB ファイルやバックアップが第三者に渡っても鍵なしには復号できない。
- **非機密設定**には可読性・可搬性・クエリ容易性が要る。モデル名やエンドポイントは頻繁に読み書きし、他のアプリケーションデータと結合して扱いたい。平文の SQLite が最も素直で、暗号化ストアに入れる利益がない。

平文 DB に秘密を置くことを避けるのは、SQLite ファイルはアプリのデータディレクトリに平置きされ、バックアップ・同期・誤共有で容易に外部へ出るためである。秘密が平文で載っていれば、ファイルの流出がそのまま資格情報の流出になる。この境界を守ることで、DB ファイルの取り扱いを「秘密を含まない」前提で設計でき、周辺（バックアップ・エクスポート・ログ）の安全性も担保しやすくなる。

GCP プロジェクト ID のようにセンシティブ度が中程度の値は、秘密ではないが「リポジトリに書きたくない」要件がある。これは settings（ユーザーローカル DB、git 管理外）に置くことで、ソース・設計書・コミットに含めずに満たせる。

## 却下した代替案

### 全部 SQLite に置く（秘密も含む）

実装が単純で読み書きも一元化できるが、DB ファイル流出＝資格情報流出となり、セキュリティルール（`agent.md`）に真っ向から反する。却下。

### 環境変数で秘密を渡す

`.env` やプロセス環境変数に秘密を置く案。デスクトップアプリでは複数アカウント・複数プロバイダの動的な資格情報を扱うため、環境変数の静的な性質と噛み合わない。プロセスリストや子プロセスへの漏洩リスクもある。OAuth クライアント ID/シークレットのようなビルド時定数（バイナリ埋め込み・公開情報扱い）にのみ環境変数を使い、実行時のユーザー秘密には使わない。

### 平文ファイル（JSON 等）に保存する

暗号化のない平文ファイルは SQLite 平文保存と同じ流出リスクを持ち、利点がない。却下。

### 全部 Stronghold に置く（非機密も含む）

秘密の保護は満たせるが、モデル名やエンドポイントのような頻繁に読む非機密値まで暗号化ストア経由になり、他データとの結合クエリもできず、可搬性・可読性を失う。過剰防御であり却下。

## 影響

### 新しい秘密情報を扱うときの配置判断ルール

新しい設定値を追加するときは、次の一問で配置を決める。

- **その値が漏れると、なりすまし・不正アクセス・課金の悪用が起きるか**
  - 起きる（パスワード・トークン・API キー・SA JSON・クレデンシャル全般）→ **SecureStore（Stronghold）**。アカウント単位なら `{種別}_{account_id}`、アプリ単位なら固定キー。
  - 起きない（モデル名・エンドポイント・プロバイダ名・リージョン・プロジェクト ID 等）→ **settings テーブル（平文）**。
- UI へ返すときは、秘密は本体を返さず登録有無の bool のみを返す。
- 秘密を SecureStore に保存した後に DB 保存が失敗した場合は、SecureStore 側のエントリを削除する補償処理を行い、孤立した秘密を残さない。

### 関連ファイル

- `src-tauri/src/secure_store.rs` — `SecureStore`（Stronghold ラッパ、insert/get/delete）
- `src-tauri/src/db/settings.rs` — settings テーブルの get/set
- `src-tauri/src/commands/auth_commands.rs` — OAuth トークン / パスワードの保存（`oauth_{id}` / `password_{id}`）
- `src-tauri/src/commands/settings_commands.rs` — LLM 設定の get/set（秘密は本体を返さない）
- `src-tauri/src/classifier/factory.rs` — SecureStore からの API キー / SA JSON 取得

## 参照

- `docs/design/2026-04-12-pigeon-design.md`（OS キーチェーン、`tauri-plugin-stronghold`、SQLite に平文保存しない方針）
- `docs/design/2026-04-13-oauth-support-design.md`（OAuth 2.0 / PKCE / deep-link、Stronghold キー体系 `oauth_{id}` / `password_{id}`）
- `docs/design/2026-07-10-llm-provider-selection-design.md`（SecureStore = Stronghold、API キー / サービスアカウント JSON の暗号化保管、非機密は settings テーブル）
- `agent.md` セキュリティルール（パスワード / OAuth トークン / API キーは OS キーチェーンに保存、SQLite に平文保存しない）
