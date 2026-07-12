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

DB 反映は `upsert_sent_mail` で行う。

### uid 確定フラグ（uid_confirmed）と watermark 分離（C1）

**当初 v1 では「uid 確定フラグは将来」としていたが、下記の watermark 汚染（C1）と
uid 衝突（C2）を成立させないためには送信時の推定 uid とサーバー実 uid を区別する状態が
必須であり、v1 に昇格した。**

`mails` に `uid_confirmed INTEGER NOT NULL DEFAULT 1`（マイグレーション v10）を追加する。

- **書き込み経路**: サーバーから取得した行（INBOX/Sent の同期挿入、`parse_mime` 由来）は
  uid がサーバー実 UID なので `uid_confirmed = 1`。送信時にローカル保存する Sent 行
  （`build_sent_record`、uid は `get_max_uid+1` の推定値）は `uid_confirmed = 0`。
  `upsert_sent_mail` の message_id 一致更新（`confirm_uid`）は uid をサーバー値に置き換えると
  同時に `uid_confirmed = 1` にする。
- **既存行の埋め戻し**: 本マイグレーション以前は Sent 同期が存在せず、全 Sent 行が送信時の
  推定 uid だったため、`folder='Sent'` の既存行は 0 で埋め戻す。INBOX 等は 1 のまま。

#### C1: watermark 汚染の防止

Sent 差分同期の起点（since_uid）を素朴に `get_max_uid(account, "Sent")` で計算すると、
送信時の**推定 uid（サーバー実 uid より大きくなりがち）**が混入する。実 uid が推定 max
以下のサーバー Sent 行が全て fetch からスキップされ、本設計の主目的である
「message_id マージによる uid 後追い確定」の対象行がそもそも取得されない。

そこで Sent の watermark は **`uid_confirmed = 1` の行のみの max uid**
（`get_max_confirmed_uid`）で計算する。確定行が皆無なら 0（初回 Sent 同期として全件対象）。
INBOX 経路（`get_max_uid`）の挙動は変えない。

#### C2: uid 確定時の UNIQUE 衝突をバッチ中断させない

`confirm_uid` が uid をサーバー値へ書き換える際、別行が同じ
`(account_id, 'Sent', uid)` を既に占有していると UNIQUE 制約に違反する（推定 uid と
サーバー実 uid が同値を取り合うケース）。無防備な UPDATE はここで Err を返し、`?` 伝播で
**バッチ途中から以降の Sent 取り込みが沈黙中断**する。

`upsert_sent_mail` は確定前に占有行を検出し、次のように解消してバッチを継続させる:

- **占有行が同一 message_id**（同一メールの重複行）: 案件割り当てを持つ側を残して他方を
  DELETE し統合（`merge_duplicate_sent_rows`）。割り当ての CASCADE 消失を避けるため
  「割り当てを持つ側を残す」ことを厳守する。
- **占有行が異なる message_id**: 別メールの uid を奪うのは危険なため、この行の取り込みを
  スキップして警告ログを出し、`Ok` を返して次の行へ進む。

## モジュール構成

### mail_sync/imap_client.rs

- `has_sent_attribute(attrs) -> bool`: フォルダ属性に `\Sent` (RFC 6154) を含むか（純関数、テスト対象）
- `find_sent_folder(session) -> Result<Option<String>>`: LIST から `\Sent` 属性のフォルダを探す
  （`find_trash_folder` と同型）

### db/mails.rs

- `get_mail_id_by_message_id(conn, account_id, folder, message_id) -> Result<Option<String>>`:
  マージ先の既存行を引く
- `get_max_confirmed_uid(conn, account_id, folder) -> Result<u32>`: `uid_confirmed=1` の行のみの
  max uid（Sent watermark 用。C1）
- `confirm_uid(conn, mail_id, uid) -> Result<()>`: 行を uid=サーバー値・`uid_confirmed=1` に確定
  （案件割り当てを保持）
- `upsert_sent_mail(conn, mail) -> Result<bool>`: message_id で既存行があれば `confirm_uid`、
  無ければ insert。uid スロットの UNIQUE 衝突を検出して統合/スキップする（C2）。戻り値は
  「新規取り込みが起きたか」（件数集計用。確定・統合・スキップは false）

### commands/mail_commands.rs

- `sync_folder_into`（INBOX 同期ロジックを共通化した内部関数）を用意し、INBOX と Sent の
  両方から呼ぶ。Sent は `upsert_sent_mail` 経由・論理フォルダ `"Sent"` で挿入する点が異なる
- Sent 同期（`sync_sent_folder`）は INBOX 同期成功後に実行し、watermark は
  `get_max_confirmed_uid`（C1）。失敗は警告ログのみ

## LocalOnly 判定の見直し

`plan_delete` / `plan_archive` は `folder == "Sent"` を無条件で `LocalOnly` にしている。
本 PR で `uid_confirmed` が入り、`uid_confirmed=1` の Sent 行はサーバー反映が理論上
安全になったが、**本 PR では Sent の削除・アーカイブは引き続き `LocalOnly` を維持する**:

- 送信直後・同期前の行は `uid_confirmed=0`（推定値）のままで、サーバー反映すると別メールを
  操作する危険がある。`plan_delete`/`plan_archive` はフォルダ名しか受け取らず、対象行の
  `uid_confirmed` を見ていない。ここで安全に解禁するには plan 関数へ `uid_confirmed` を渡す
  シグネチャ変更と分岐追加が必要で、C1/C2 修正と別関心のため本 PR ではスコープ外にする。
- 破壊的操作（削除）で誤爆すると復元不能。安全側に倒す。

**将来対応（本 PR で土台は整った）**: `plan_delete(folder, uid_confirmed)` /
`plan_archive(provider, folder, archive_folder, uid_confirmed)` へ拡張し、
`folder=="Sent" && uid_confirmed` のときは通常のサーバー反映経路（実 UID が確定済みなので安全）、
`folder=="Sent" && !uid_confirmed` のときのみ `LocalOnly`、とすれば Sent の削除・アーカイブの
サーバー反映を解禁できる。本 PR では `plan_*` のテストは現状の `LocalOnly` を維持する。

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
- `migrate_v10`: `uid_confirmed` の既定は 1 / 既存 `folder='Sent'` 行は 0 に埋め戻す
- `db::mails::get_mail_id_by_message_id`: account/folder/message_id での引き当て
- `db::mails::get_max_confirmed_uid`（C1）: `uid_confirmed=0` の推定 uid を watermark に含めない。
  確定行が皆無なら 0
- `db::mails::confirm_uid`: uid 更新 + `uid_confirmed=1` 化、案件割り当ては維持
- `db::mails::upsert_sent_mail`:
  - 送信時ローカル行（推定 uid・未確定）+ 同 message_id のサーバー行（正 uid・衝突なし）
    → 1行のまま uid が正値に確定し `uid_confirmed=1`、案件割り当てを保持
  - 新規 message_id → 挿入される（他クライアント送信の取り込み）
  - C2 統合: 確定先 uid を同一 message_id の重複行が占有 → 割り当てを持つ側を残して統合
  - C2 スキップ: 確定先 uid を別 message_id が占有 → スキップして `Ok`（バッチ継続）、
    既存行の uid は奪わない

### テスト対象外（PR に明記）

- 実 IMAP セッションでの LIST / SELECT / UID FETCH（統合境界）
- APPENDUID/COPYUID の実応答パース（クレート制約でそもそも未実装）

## スコープ

| 含む | 含まない（将来・理由） |
|------|----------------------|
| Sent フォルダの差分同期（INBOX と同じバッチ取得） | APPENDUID の送信時取得（async-imap 0.11.2 に公開経路なし。Gmail は APPEND 自体しない） |
| `\Sent` SPECIAL-USE によるフォルダ発見 | COPYUID の取得（同上）とそれに依存するアーカイブ解除のサーバー反映 |
| message_id による送信行と同期行のマージ（二重行防止） | Sent メールの削除・アーカイブのサーバー反映（plan 関数への uid_confirmed 伝播は別関心。安全側で LocalOnly 維持。土台は本 PR で用意） |
| `uid_confirmed` フラグ + watermark 分離（C1）+ uid 衝突解消（C2） | Drafts / Trash など他フォルダの同期（別項目 A5） |
| 他クライアント送信メールの取り込み | |

### I1: Message-ID を持たないメールのマージ

`mime_parser` は Message-ID ヘッダが欠落したメールに対し `<generated-{uuid}@pigeon>` を
**毎回新規生成**する。このため送信時ローカル行とサーバー同期行で message_id が一致せず、
message_id マージ（`upsert_sent_mail` の突き合わせ）が効かない。ただし挙動は
UNIQUE(account_id, folder, uid) による二重行防止で守られる（サーバー同期行は
`uid_confirmed=1` で挿入され、以後の watermark にも正しく反映される）ため、実害は
「送信時ローカル行が別行として残りうる」ことに留まる。Pigeon 送信メールは
`generate_message_id()` で必ず Message-ID を付与するため、この経路は他クライアント由来の
Message-ID 欠落メールに限られる。現状は現行挙動を維持する（v1 制限）。

## v1 の制限

- **Sent の削除・アーカイブはローカルのみ**（`plan_* = LocalOnly` を維持）。`plan_*` が対象行の
  `uid_confirmed` を見ないため。`uid_confirmed=1` 前提のサーバー反映解禁は「LocalOnly 判定の
  見直し」の将来対応で行う（土台となる `uid_confirmed` は本 PR で導入済み）
- **アーカイブ解除はローカルのみ**（COPYUID 未取得。既存制限を踏襲）
- **APPENDUID/COPYUID 非対応**: async-imap 0.11.2 の API 制約による。Sent 同期で UID は
  後追い確定するため実害はないが、送信直後に同期が走るまでの間は Sent 行の uid は推定値
  （`uid_confirmed=0`）
- **Message-ID 欠落メールのマージ不成立**（I1 参照）
- Sent 同期は INBOX の初回同期範囲（`initial_sync_limit`、既定5000）と同じ上限に従う
