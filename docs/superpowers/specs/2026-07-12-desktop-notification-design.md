# 新着メールのデスクトップ通知

IMAP IDLE（2026-07-12-imap-idle-design.md）の新着検知イベントに載せる形で、
新着メール受信時に OS のデスクトップ通知を表示する。

**2026-07-13 追記**: バックログ項目6「通知の強化」により、件名プレビュー
（デフォルト OFF）を追加した。通知クリックでのアプリ前面化は技術的制約により
スコープアウトした（「v2: 通知の強化」節を参照）。

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
- **設定 UI は実装済み**。サイドバー下部の `NotificationToggle`
  （`src/components/sidebar/NotificationToggle.tsx`）に
  「新着メールのデスクトップ通知」の 1 行トグルを置く
  - ON はキー削除（デフォルト ON のため）、OFF は `"false"` を書き込む
  - 既存の設定ダイアログは LLM / クラウド送信に特化しているため、
    汎用設定画面は新設しない

## テスト

- `notifyNewMail`（プラグインは vi.mock）:
  - 権限ありで sendNotification が呼ばれる
  - 権限なし→requestPermission が granted なら通知する
  - 拒否時は静かにスキップ（sendNotification を呼ばない）
  - localStorage で OFF のとき権限確認すらしない
- mailStore: 同期結果 count > 0 で notifyNewMail が呼ばれ、0 では呼ばれない
- `NotificationToggle`: 初期状態が localStorage を反映すること、
  切替で localStorage が更新されること（OFF→`"false"`、ON→キー削除）
- 実際の OS 通知表示は統合境界として自動テスト対象外

## v2: 通知の強化（2026-07-13、バックログ項目6）

### 件名プレビュー

新着通知に件名を表示するオプション。プライバシー配慮のため既定は
**表示しない**（現行の件数のみ表示を維持）。

- `isSubjectPreviewEnabled()`: localStorage キー `pigeon.notifySubjectPreview`
  が `"true"` のときのみ有効（**デフォルト OFF**。通知本体トグルと逆の既定値）。
  保存先を既存の通知トグルと統一するため localStorage のまま
  （settings テーブルへの移行はバックログ #16 で別途行う想定であり、
  本タスクではスコープを合わせない）
- `buildNotificationBody(count, subjects, previewEnabled)`（純関数・新規）:
  - `previewEnabled` が false、または `subjects` が空なら
    `"${count}件の新着メールを受信しました"`（従来と同じ）
  - true かつ `subjects` があれば、先頭 3 件の件名を改行区切りで表示し、
    `count` が 3 件を超える場合は末尾に `"他N件"` を追加する
    （N = count − 表示した件名数）
- `notifyNewMail(count, subjects = [])`: 第2引数 `subjects` を追加
  （**省略可能・デフォルト空配列**）。`buildNotificationBody` で本文を
  組み立てて `sendNotification` に渡す

**呼び出し元の制約と設計判断**: 件名プレビューには「実際に新規取り込みされた
メールの件名」が必要だが、これを持っているのは `mailStore.ts` の
`initNewMailListener`（`syncAccount` の解決コールバック）のみである。
本タスクでは `mailStore.ts` を変更しない制約があったため、
`notifyNewMail` の第2引数を **省略可能** にすることで、既存の
`notifyNewMail(count)` 呼び出し（`mailStore.ts` 側）を無改修のまま
互換に保った。その結果、**v2 の時点では件名プレビューは有効化しても
実際には件数のみの通知になる**（`subjects` が渡されないため）。
`mailStore.ts` の担当エージェントの作業が完了し次第、当該コールバックで
`notifyNewMail(count, event.payload.account_id)` のように呼び出し元へ
実際の件名一覧（またはそれを引ける account_id）を渡す変更を別途行う必要がある。
件名の取得元は新着として取り込まれた最新メール（`get_threads` 等の
既存経路、または新規に軽量なコマンドを追加）を想定。

設定 UI: `NotificationToggle` に「通知に件名を表示」の1行トグルを追加。
通知本体トグルが OFF のときは非表示にする（無関係な設定を見せない）。

### 通知クリックでのアプリ前面化（スコープアウト）

**実装しない。** `tauri-plugin-notification` 2.3.3 のデスクトップ実装
（`desktop.rs`）は `notify-rust` クレートで `notification.show()` を
fire-and-forget しているのみで、クリック時のコールバック・イベント発火の
仕組みが存在しない。JS 側に公開されている `onAction` / `onNotificationReceived`
は `addPluginListener('notification', 'actionPerformed' | 'notification', cb)`
で Rust 側の emit を購読する仕組みだが、該当の emit は **iOS/Android
（`mobile.rs`）にのみ実装されており、desktop.rs には存在しない**。
そのため macOS/Windows/Linux では通知をクリックしても何のイベントも
発火せず、ウィンドウ前面化のトリガーを取得できない。

将来対応するとすれば、`notify-rust` の代わりにプラットフォームネイティブ
API（macOS の `UNUserNotificationCenterDelegate` 等）を直接実装するか、
プラグイン側のアップデートを待つ必要がある。バックログには追加しない
（頻度・重要度が低いため、必要になった時点で再検討）。
