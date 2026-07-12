# 新着メールのデスクトップ通知

IMAP IDLE（2026-07-12-imap-idle-design.md）の新着検知イベントに載せる形で、
新着メール受信時に OS のデスクトップ通知を表示する。

## 方針: 同期結果を通知のトリガーにする

`new-mail-detected` イベントそのものではなく、その後の自動同期の**取り込み結果**
（`syncAccount` の戻り値 count）を通知条件にする。

1. バックエンドが `new-mail-detected` を emit（既存）
2. フロントの `initNewMailListener` が `syncAccount(account_id)` を呼ぶ（既存）
3. **count > 0 のとき `notifyNewMail(count)` で通知を出す（本設計で追加）**

この設計の利点:

- IDLE の EXISTS 検知は誤検知（同期済みメールの再通知等）があり得るが、
  実際に新規取り込みされた件数を条件にすることで空通知を防げる
- 通知文言に正確な件数を載せられる

v1 ではアプリがフォアグラウンドでも通知する（出し分けしない）。

## バックエンド

- `tauri-plugin-notification` を Cargo.toml に追加
- `lib.rs` で `.plugin(tauri_plugin_notification::init())` を登録
- `capabilities/default.json` に `notification:default` を追加

通知の発火判断はフロントエンドが持つため、Rust 側はプラグイン登録のみ。

## フロントエンド

### `src/utils/notifyNewMail.ts`（新規）

`@tauri-apps/plugin-notification` を使うヘルパー:

- `isNotificationEnabled()`: localStorage キー `pigeon.notifyNewMail` が
  `"false"` でなければ有効（**デフォルト ON**）
- `notifyNewMail(count)`:
  1. 設定が OFF なら何もしない
  2. `isPermissionGranted()` で権限確認、なければ `requestPermission()` を要求
  3. 拒否されたら**静かにスキップ**（エラートーストは出さない。
     通知は補助機能であり、拒否はユーザーの意思のため）
  4. `sendNotification({ title: "Pigeon", body: "${count}件の新着メールを受信しました" })`
  - プラグイン呼び出しの失敗も console.error のみで握りつぶす

### mailStore

`initNewMailListener` 内で `syncAccount` の解決値を受け、
count > 0 のときだけ `notifyNewMail(count)` を呼ぶ。
count が 0 のとき（新着なし・同期中ガードで抑止・エラー時）は通知しない。

## オン/オフ設定

- localStorage キー `pigeon.notifyNewMail` を尊重する（`"false"` で無効）
- **設定 UI は将来対応**。現状の設定ダイアログは LLM / クラウド送信に
  特化しており、汎用設定画面の新設は本変更のスコープ外とする

## テスト

- `notifyNewMail`（プラグインは vi.mock）:
  - 権限ありで sendNotification が呼ばれる
  - 権限なし→requestPermission が granted なら通知する
  - 拒否時は静かにスキップ（sendNotification を呼ばない）
  - localStorage で OFF のとき権限確認すらしない
- mailStore: 同期結果 count > 0 で notifyNewMail が呼ばれ、0 では呼ばれない
- 実際の OS 通知表示は統合境界として自動テスト対象外
