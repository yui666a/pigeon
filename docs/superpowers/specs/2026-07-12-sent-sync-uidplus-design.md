# Sentフォルダ同期 + UIDPLUS（APPENDUID / COPYUID）設計書

日付: 2026-07-12
ステータス: 実装対象
関連: `2026-07-12-mail-send-design.md` / `2026-07-12-mail-delete-archive-design.md` / `docs/BACKLOG.md` 項目1

## 目的

送信メールのローカル `uid` がサーバー UID と一致しない構造的負債を解消する。現状:

- 送信時のローカル Sent 行は `uid = get_max_uid("Sent") + 1` の**推定値**で、サーバー UID と一致しない
- そのため Sent メールの削除・アーカイブ・既読はサーバー反映できず `LocalOnly` 扱い（`2026-07-12-mail-delete-archive-design.md`「v1 の制限」）
- 他クライアント（Gmail Web / 標準メール等）から送ったメールは Pigeon に取り込まれず、スレッドに現れない

## 採用方式の結論（重要な設計判断）

**APPENDUID を送信時に読むのではなく、Sent フォルダの差分同期を正とし、`message_id` で
ローカル送信行のサーバー UID を後追い確定する。**

### なぜ APPENDUID を送信時に読まないのか

依存クレート `async-imap 0.11.2` の API 制約により、**APPENDUID / COPYUID を読む
公開経路が存在しない**ことを確認した:

- `Session::append()` の戻り値は `Result<()>`。応答の `OK [APPENDUID <uidvalidity> <uid>]`
  はタグ付き完了行の `code` フィールドに入るが、`check_status_ok` がステータス判定に
  使うだけで**破棄される**（`src/client.rs:1115` append 実装、`check_done_ok_from`）。
- APPENDUID はタグ付き行に乗るため `UnsolicitedResponse` チャネルにも流れない
  （untagged 行のみが `handle_unilateral` を通る）。
- `uid_copy()` も同様に `Result<()>` で、COPYUID を読めない。

APPENDUID を読むにはクレートのフォーク、または `append()` を使わず生 IMAP コマンドを
手書きして応答をパースする必要がある。いずれも保守コストが高く、下記のより本質的な理由で
不要になる。

### なぜ Sent 同期の方が本質的か

- **Gmail はそもそも APPEND しない**。SMTP 送信時に Gmail が自動で「送信済み」へ保存する
  ため、Pigeon 側は APPEND を行わない（`2026-07-12-mail-send-design.md`）。したがって
  Gmail については APPENDUID が原理的に取得不能で、送信時 UID 確定はできない。主要
  プロバイダで使えない手段を土台にはできない。
- **他クライアント送信の取り込み**（BACKLOG の3つ目の穴）は APPENDUID では解決できない。
  Pigeon 以外から送ったメールには Pigeon の送信フローが介在しないため、Sent フォルダを
  同期する以外に取り込む方法がない。
- Sent 同期を実装すれば、サーバー Sent 行が**正しいサーバー UID を持って**降ってくる。
  送信時ローカル行と `message_id` で突き合わせて UID を確定でき、APPENDUID は不要になる。

結論として、**Sent 同期が「他クライアント取り込み」と「送信メールの UID 確定」の両方を
一手に解決する**。APPENDUID/COPYUID 対応は本 PR ではスコープアウトし、その理由と将来の
再検討条件を本書に明記する。

## アーキテクチャ

### 同期対象フォルダの拡張

`sync_account_inner`（`commands/mail_commands.rs`）は現状 INBOX 決め打ち。これを

1. INBOX を従来どおり同期（差分・フラグ再同期）
2. **Sent フォルダを続けて同期**（フォルダ名は下記「Sent フォルダの解決」で決定）

の2段構成にする。Sent 同期は失敗しても INBOX 同期の成功は覆さない（ベストエフォート、
`eprintln!` 警告のみ）。理由: Sent 同期は付加価値であり、INBOX が取れていれば日常利用は
成立するため。

### Sent フォルダの解決

`mails` テーブルの送信行は `folder = 'Sent'`（`mail-send-design`）で固定。一方サーバー上の
Sent の実フォルダ名はプロバイダで異なる（Gmail: `[Gmail]/Sent Mail`、一般 IMAP: `Sent`）。
そこで:

- サーバー側フォルダ名は **SPECIAL-USE (RFC 6154) の `\Sent` 属性**で探す
  （`find_trash_folder` と同型の `find_sent_folder`）。見つからなければ settings の
  `sent_folder`（デフォルト `"Sent"`）にフォールバック。
- ローカル DB 上は従来どおり `folder = 'Sent'` に正規化して保存する（UI・分類・
  重複判定は `'Sent'` 一語で扱えるほうが単純で、既存コードと整合する）。

つまり「サーバーの実フォルダ名（可変）」→「ローカルの論理フォルダ名 `'Sent'`（固定）」の
写像を同期時に行う。

### message_id によるマージ（二重行の防止）

`mails` の UNIQUE は `(account_id, folder, uid)`。送信時ローカル行（推定 uid）と
サーバー同期行（正しい uid）は uid が異なるため、素朴な `INSERT OR IGNORE` では
**同じメールが2行**できてしまう。これを防ぐため Sent 同期時は次の判定を行う:

- サーバー Sent 行の `message_id` と同じ `message_id` を持つローカル Sent 行が既にあれば、
  その行の **`uid` をサーバー値へ更新**する（新規挿入しない。案件割り当てを保持するため
  DELETE/REPLACE は使わない）。
- 無ければ通常どおり INSERT する（他クライアント送信の取り込み）。

この判定を純関数 `plan_sent_merge` に切り出し、DB 反映は `upsert_sent_mail` で行う。

## モジュール構成

### mail_sync/imap_client.rs

- `has_sent_attribute(attrs) -> bool`: フォルダ属性に `\Sent` (RFC 6154) を含むか（純関数、テスト対象）
- `find_sent_folder(session) -> Result<Option<String>>`: LIST から `\Sent` 属性のフォルダを探す
  （`find_trash_folder` と同型）

### db/mails.rs

- `get_mail_id_by_message_id(conn, account_id, folder, message_id) -> Result<Option<String>>`:
  マージ先の既存行を引く
- `update_uid(conn, mail_id, uid) -> Result<()>`: 行の uid を更新（案件割り当てを保持）
- `upsert_sent_mail(conn, mail) -> Result<bool>`: message_id で既存行があれば uid を更新、
  無ければ insert。戻り値は「新規取り込みが起きたか」（件数集計用）

### commands/mail_commands.rs

- `sync_folder_inner`（INBOX 同期ロジックを共通化した内部関数）を用意し、INBOX と Sent の
  両方から呼ぶ。Sent は `upsert_sent_mail` 経由で挿入する点が異なる
- Sent 同期は INBOX 同期成功後に実行し、失敗は警告ログのみ

## LocalOnly 判定の見直し

`plan_delete` / `plan_archive` は現状 `folder == "Sent"` を無条件で `LocalOnly` にしている。
Sent 同期で uid が確定した行はサーバー反映可能になるが、**次の理由で本 PR では Sent の
削除・アーカイブは引き続き `LocalOnly` を既定にする**:

- Sent 同期が一度も走っていない行（送信直後、同期前）は uid が推定値のままで、サーバー
  反映すると別メールを操作する危険がある。「同期済みか」をローカル行から確実に判定する
  フラグが現状ない。
- 破壊的操作（削除）で誤爆すると復元不能。安全側に倒す。

代わりに、Sent 同期によって **他クライアント送信も含めた Sent 行が正しい UID を持つ**
ようになるため、将来 uid 確定済みを示す状態（例: `uid_verified` フラグ、または「Sent 同期の
watermark UID 以下なら確定」判定）を導入すれば、Sent のサーバー反映を安全に解禁できる。
本書ではこれを将来対応として明記し、`plan_*` のテストは現状の `LocalOnly` を維持する。

## COPYUID（アーカイブ解除のサーバー反映）

`2026-07-12-mail-delete-archive-design.md` の「アーカイブ解除は v1 ローカルのみ」は COPYUID
未取得が理由だった。前述のとおり async-imap 0.11.2 では COPYUID も読めないため、**本 PR でも
アーカイブ解除のサーバー反映は実装しない**（スコープアウト）。将来 async-imap の更新または
生コマンド実装で COPYUID を読めるようになった時点で再検討する。

## テスト計画（TDD）

統合境界（実 IMAP 通信）はテスト対象外とし、判断ロジックを純関数に切り出して集中的に
テストする。この方針は既存の imap_client テスト（`plan_batches` 等）と同じ。

### 純関数・DB ロジック（ユニットテスト）

- `has_sent_attribute`: `\Sent` を検出し、`\Trash`/`\Junk` 等を誤検出しない
- `plan_sent_merge`: 既存 message_id あり → UpdateUid / 無し → Insert の分岐
- `db::mails::get_mail_id_by_message_id`: account/folder/message_id での引き当て
- `db::mails::update_uid`: uid 更新後も案件割り当てが残る（CASCADE で消えない）
- `db::mails::upsert_sent_mail`:
  - 送信時ローカル行（推定 uid）+ 同 message_id のサーバー行（正 uid）→ 1行のまま uid が正値に更新
  - 案件割り当てが保持される
  - 新規 message_id → 挿入される（他クライアント送信の取り込み）

### テスト対象外（PR に明記）

- 実 IMAP セッションでの LIST / SELECT / UID FETCH（統合境界）
- APPENDUID/COPYUID の実応答パース（クレート制約でそもそも未実装）

## スコープ

| 含む | 含まない（将来・理由） |
|------|----------------------|
| Sent フォルダの差分同期（INBOX と同じバッチ取得） | APPENDUID の送信時取得（async-imap 0.11.2 に公開経路なし。Gmail は APPEND 自体しない） |
| `\Sent` SPECIAL-USE によるフォルダ発見 | COPYUID の取得（同上）とそれに依存するアーカイブ解除のサーバー反映 |
| message_id による送信行と同期行のマージ（二重行防止） | Sent メールの削除・アーカイブのサーバー反映（uid 確定状態の判定フラグが未整備。安全側で LocalOnly 維持） |
| 他クライアント送信メールの取り込み | Drafts / Trash など他フォルダの同期（別項目 A5） |

## v1 の制限

- **Sent の削除・アーカイブはローカルのみ**（`plan_* = LocalOnly` を維持）。同期前の行の
  uid 推定値による誤爆を避けるため。uid 確定状態の判定を将来導入して解禁する
- **アーカイブ解除はローカルのみ**（COPYUID 未取得。既存制限を踏襲）
- **APPENDUID/COPYUID 非対応**: async-imap 0.11.2 の API 制約による。Sent 同期で UID は
  後追い確定するため実害はないが、送信直後に同期が走るまでの間は Sent 行の uid は推定値
- Sent 同期は INBOX の初回同期範囲（`initial_sync_limit`、既定5000）と同じ上限に従う
