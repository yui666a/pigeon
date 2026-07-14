# ADR 0005: メール同期の方向性・UID信頼性・破壊的操作の反映規約

## ステータス

確定（2026-07-14）。

同期ルールの規約自体は確定である。判定ロジックの実装統合は概ね完了しているが、
最終的なトラッキングと将来拡張（`uid_confirmed` を見た Sent 反映解禁）は残タスクとして
`docs/BACKLOG.md` を参照すること。

## コンテキスト

メール同期の中核ルールが、機能ごとに追加された設計書6本に分散していた。

- `2026-04-12-pigeon-design.md`（双方向同期表）
- `2026-07-12-read-unread-design.md`（既読/未読、BODY.PEEK 必須）
- `2026-07-12-mail-delete-archive-design.md`（削除・アーカイブのサーバー反映順序）
- `2026-07-12-sent-sync-uidplus-design.md`（Sent 同期、`uid_confirmed`、APPENDUID/COPYUID 断念）
- `2026-07-12-star-flag-unread-design.md`（スター/フラグ、未読に戻す）
- `2026-07-13-bulk-actions-design.md`（一括操作、`plan_*` の意図的複製という負債メモ）

これらは個別には整合していたが、「サーバー UID を信頼できるか」「どの操作を同期反映し、
どれをベストエフォートにするか」「Sent をどう扱うか」という同期の背骨となる判断が
1本にまとまっておらず、新しいメール操作を追加するたびに複数の設計書を横断して読む必要があった。

さらに実害として、`plan_delete` / `plan_archive` / `is_local_only_folder` という
同じ判定ロジックが機能ブランチごとに別実装で複製されていた。並行開発中に
`mail_commands.rs` を触れない制約から、`bulk_commands.rs`（一括操作）と
`flag_commands.rs`（スター/未読戻し）がそれぞれ独自に「Sent は LocalOnly」判定を
書き起こしていた。同じルールが3箇所に散り、Sent 同期対応で LocalOnly 判定が変わったときに
複製側が追随できず乖離する構造的リスクを抱えていた。

本 ADR は、これら分散したルールを1本の規約として集約し、複製解消のトラッキング根拠とする。

## 決定

### 1. 同期の方向性は操作の可逆性で非対称に決める

- **破壊的操作（削除）は同期反映する。サーバー成功 → ローカル反映の順序を厳守する。**
  楽観更新（先にローカルを消してからサーバーへ投げる）は行わない。サーバー処理が失敗したら
  エラーを返し、ローカルは一切変更しない。
- **可逆操作（既読 / 未読戻し / スター）は DB 即時更新 + サーバーはベストエフォート。**
  UI は DB 更新で確定させ、IMAP `STORE` はバックグラウンドタスクで投げる。サーバー反映の
  失敗（オフライン等）はログのみでエラーにせず、正しい状態は次回同期のフラグ再同期で収束する。
- **アーカイブは削除と可逆操作の中間**として扱う。サーバー反映はするが（サーバー成功 →
  ローカルの `mails.folder` を `'Archive'` に更新）、ローカル行は消さない。案件割り当て・
  スレッド・検索性を維持することがアーカイブの価値だからである。
- **アーカイブ解除（unarchive）はローカルのみ**。サーバー反映しない（理由は後述）。

### 2. サーバー UID の信頼性は `uid_confirmed` フラグで管理し、APPENDUID/COPYUID には依存しない

- `mails` テーブルに `uid_confirmed`（`INTEGER NOT NULL DEFAULT 1`、マイグレーション v10）を持つ。
- サーバーから FETCH で取得した行（INBOX / Sent の同期挿入）は実 UID を持つので
  `uid_confirmed = 1`。送信時にローカル保存する Sent 行は `uid = max(uid)+1` の**推定値**であり
  `uid_confirmed = 0`。
- 送信メールの UID は **Sent フォルダの差分同期 + `message_id` による後追い確定**で
  正しい値に置き換える（置き換え時に `uid_confirmed = 1` へ昇格）。APPENDUID を送信時に
  読むことには依存しない。
- Sent 差分同期の watermark（since_uid）は `uid_confirmed = 1` の行のみの max uid
  （`get_max_confirmed_uid`）で計算する。推定 uid で汚染しないことで、後追い確定対象の
  サーバー行を取りこぼさない。

### 3. Sent フォルダは LocalOnly 扱い

- 削除・アーカイブ・スター・未読戻しにおいて、`folder == "Sent"` はサーバー反映せず
  ローカル更新のみとする（`plan_delete` / `plan_archive` = `LocalOnly`、
  `is_local_only_folder("Sent") == true`）。
- 理由は、送信直後・同期前の Sent 行は `uid_confirmed = 0`（推定値）のままであり、
  サーバー反映すると別メールを操作する危険があるため。現行の判定はフォルダ名しか見ておらず
  `uid_confirmed` を見ていないので、安全側に倒して Sent 全体を LocalOnly とする。

### 4. 本文取得の FETCH は必ず BODY.PEEK[] を使う

- 同期・添付取得・バックフィルすべての本文取得 FETCH は `BODY.PEEK[]` を使う。
  PEEK なしの `RFC822` / `BODY[]` は RFC 3501 の仕様でサーバー側に `\Seen` を付けるため、
  同期しただけで Gmail 本体ごと全メールが既読化される不具合を引き起こす。
- この規約は `imap_client.rs` の `FETCH_ITEMS_*` 定数と回帰テストで固定する。

### 5. 判定ロジックは1箇所に集約する

- 「サーバー反映するか / どの方式か」および「サーバー UID を信頼できるか（LocalOnly か）」の
  判定は、単一のポリシーモジュール（`commands/mail_policy.rs`）に集約する。削除・アーカイブ・
  一括操作・スター・未読戻しの各コマンドは、この1箇所を経由して判定する。機能ごとに同じ判定を
  再実装しない。

## 理由

### なぜ破壊的操作と可逆操作で非対称にするのか

削除は失敗の黙殺が危険な破壊的操作である。ローカルを先に消してサーバー反映に失敗すると、
「ローカルには無いがサーバーには残る」不整合が生じ、しかも復元不能な方向に倒れる。だから
サーバー成功を待ってからローカルへ反映する。一方、既読・スターは可逆かつ冪等であり、失敗しても
次回のフラグ再同期で収束する。ここで同期的な成功待ちを課すと、UI の即応性を犠牲にするだけで
得るものが無い。誤爆時の損害の非対称性が、そのまま反映方式の非対称性に対応する。

### なぜ APPENDUID / COPYUID を使わないのか

依存クレート `async-imap 0.11.2` の API 制約により、APPENDUID / COPYUID を読む公開経路が
存在しないことを確認した。`Session::append()` / `uid_copy()` の戻り値は `Result<()>` で、
`OK [APPENDUID …]` / `[COPYUID …]` はタグ付き完了行の `code` に入るがステータス判定に
使われるだけで破棄される。タグ付き行なので `UnsolicitedResponse` チャネルにも流れない。
読むにはクレートのフォークか生 IMAP コマンドの手書きが必要で、保守コストが高い。

加えて、より本質的な理由がある。**Gmail はそもそも APPEND しない**（SMTP 送信時に自動で
送信済みへ保存する）ため、Gmail については APPENDUID が原理的に取得不能である。主要
プロバイダで使えない手段を UID 確定の土台にはできない。代わりに Sent フォルダ同期を正とすれば、
サーバー Sent 行が正しい UID を持って降ってくるので `message_id` で後追い確定でき、同時に
「他クライアントから送ったメールの取り込み」も一手に解決する。

### なぜ unarchive をローカルのみにするのか

アーカイブ時に COPY 先の新 UID を取得・保存していない（COPYUID を読めないため）。
Gmail ではアーカイブ済みメールの実体は All Mail にあり、ローカルに記録された `uid` は
INBOX 時代のもので All Mail 上では別 UID になる。UID が特定できないままサーバーを操作すると
別メールを操作する危険があるため、v1 では unarchive をローカルの folder 更新のみとする。
サーバー側はアーカイブ済みのまま残る。COPYUID を保存できる改修とセットで将来対応する。

### なぜ判定を1箇所に集約するのか

`plan_delete` / `plan_archive` / `is_local_only_folder` が機能ブランチごとに複製されていた
（`bulk_commands.rs` は並行編集中の `mail_commands.rs` を触れない制約から意図的に複製、
`flag_commands.rs` は「サーバー反映方式」ではなく「UID を信頼できるか」という別意味だが
同じ `folder == "Sent"` 判定を独自定義）。この状態では、Sent 同期対応で LocalOnly 判定を
`uid_confirmed` ベースに拡張したとき、複製側が追随せず乖離する。破壊的操作の安全性に直結する
判定が食い違うのは許容できないため、単一の真実の源に統合する。

## 却下した代替案

- **全操作を同期反映する**: 既読・スターまで成功待ちにすると、オフライン時に UI が固まり、
  冪等な操作にまで失敗ハンドリングを課すことになる。可逆操作は次回同期で収束するので不要。
- **APPENDUID / COPYUID に依存して送信時に UID を確定する**: `async-imap 0.11.2` に公開経路が
  無く、Gmail はそもそも APPEND しないため主要プロバイダで成立しない。Sent 同期 + `message_id`
  後追いで代替できるため採らない。
- **双方向の完全同期（unarchive も含めすべてサーバーへ反映）**: COPYUID を読めない現状では
  アーカイブ済みメールの UID を追跡できず、別メール誤操作のリスクがある。安全側に倒し、
  UID を確実に追跡できる操作のみ同期する。
- **判定ロジックを機能ごとに複製したまま運用する**: 短期的には並行開発の衝突を避けられるが、
  LocalOnly 判定の乖離という構造的リスクを恒久化する。集約により解消する。

## 影響

- **新しいメール操作を追加するときは本規約に従う。** 破壊的操作はサーバー成功 → ローカル反映の
  順序厳守、可逆操作は DB 即時 + ベストエフォート、Sent は LocalOnly、本文取得は BODY.PEEK[]、
  UID 信頼性は `uid_confirmed` で判断する。
- **判定は `commands/mail_policy.rs`（`plan_delete` / `plan_archive` / `is_local_only_folder`）
  1箇所を経由する。** 個別コマンドで同じ判定を再実装しない。`bulk_commands.rs` は
  `mail_commands` の共通内部関数（`delete_mail_inner` / `archive_mail_inner`）へ委譲し、
  `flag_commands.rs` は `mail_policy::is_local_only_folder` を直接参照する形へ統合済みである。
  以後、複製を再導入しないことを本 ADR で規約化する。残る統合・トラッキングは
  `docs/BACKLOG.md` を参照すること。
- **将来拡張の土台**: `plan_delete(folder, uid_confirmed)` /
  `plan_archive(provider, folder, archive_folder, uid_confirmed)` へシグネチャを拡張し、
  `folder == "Sent" && uid_confirmed` のときは通常のサーバー反映経路へ、
  `!uid_confirmed` のときのみ LocalOnly とすれば、Sent の削除・アーカイブのサーバー反映を
  安全に解禁できる。土台となる `uid_confirmed` は導入済みである。
- **unarchive のサーバー反映**は、`async-imap` の更新または生コマンド実装で COPYUID を
  読めるようになった時点で再検討する。

## 参照

- `docs/design/2026-04-12-pigeon-design.md`（Pigeon 全体設計 / 双方向同期表）
- `docs/archive/specs/2026-07-12-read-unread-design.md`（既読/未読と IMAP フラグ同期）
- `docs/archive/specs/2026-07-12-mail-delete-archive-design.md`（メール削除・アーカイブのサーバー反映）
- `docs/archive/specs/2026-07-12-sent-sync-uidplus-design.md`（Sent 同期 + UIDPLUS / APPENDUID / COPYUID）
- `docs/archive/specs/2026-07-12-star-flag-unread-design.md`（スター/フラグ・手動で未読に戻す）
- `docs/archive/specs/2026-07-13-bulk-actions-design.md`（複数選択・一括操作 / `plan_*` 複製メモ）
- 実装: `src-tauri/src/commands/mail_policy.rs`、`mail_commands.rs`、`bulk_commands.rs`、
  `flag_commands.rs`、`src-tauri/src/mail_sync/`（`imap_client.rs` / `sync_service.rs`）、
  `src-tauri/src/db/`（`mails.rs` / `sent_sync.rs`）
