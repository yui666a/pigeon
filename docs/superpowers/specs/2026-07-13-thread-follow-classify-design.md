# スレッド追従の自動分類 設計書

- 作成日: 2026-07-13
- ステータス: 実装完了
- 関連: `docs/BACKLOG.md` 項目10, `2026-07-12-sequential-classification-design.md`

## 1. 目的

未分類スレッドの部分分類問題を解消する。

現状: スレッドD&Dは「その時点でスレッドに含まれる全メール」を分類する。分類後に同じスレッドへ
返信が届くと、その返信は新規メールとして再び未分類一覧に入る。ユーザーは同じスレッドに対して
何度も分類操作を行う羽目になる。

## 2. 要件

未分類メールのうち、**同一スレッドの既存メールが単一の案件に割り当て済み**のものは、その案件へ
自動的に追従割り当てする。

- スレッド仲間が複数の異なる案件に割り当てられている場合は追従しない（曖昧なので未分類のまま）
- スレッド仲間の割り当てが1件もなければ通常どおり未分類のまま（AI分類 or 手動分類を待つ）
- UI変更は不要。未分類一覧から自動的に消え、案件ビューに現れるだけ

## 3. スレッド判定

`db::mails::build_threads` と同じロジック（In-Reply-To / References によるグラフ結合、
リンクが無いメールのみ正規化件名でのフォールバック結合）を**再利用**する。判定ロジックの
重複実装はしない。

`build_threads` はスレッド分割に使う `&[Mail]` の中身しか見ない純関数のため、
スレッド追従を成立させるには「未分類メール」だけでなく「既に割り当て済みのメール」も
同じ集合に含めて渡す必要がある。取得範囲は **INBOX に限らずアカウント全フォルダ**とする
（自分の返信や過去にアーカイブされたメールもスレッドの手がかりになるため）。
既存の `mails::get_mails_by_account` はフォルダ単位のため、全フォルダを取得する
`mails::get_all_mails_by_account(conn, account_id)` を新設する。

## 4. 実装方式

### 4.1 呼び出しタイミング

`classify_commands::get_unclassified_threads` の**先頭**で
`assignments::auto_follow_threads(conn, account_id)` を呼ぶ。同期経路（`mail_commands.rs` /
`imap_client.rs`）には一切触れない。未分類一覧を開くたびに追従が効く（呼び出し側は
D&D後の再取得や定期ポーリングを経て自然にこの経路を通る）。

理由: 同期直後に判定するのが理想的ではあるが、同期経路は他エージェントが Critical 修正中で
競合が避けられない制約がある。一覧取得時の判定は「表示のたびに毎回再計算」という多少のコストは
あるが、アカウント単位のメール数は数千件オーダーであり許容範囲。実利用上、未分類一覧を開く
タイミング＝ユーザーが分類状況を確認するタイミングなので体験上の遅延も無い。

### 4.2 アルゴリズム（`db::assignments::auto_follow_threads`）

```
auto_follow_threads(conn, account_id) -> Result<usize, AppError>:
  all_mails = mails::get_all_mails_by_account(conn, account_id)
  threads = mails::build_threads(&all_mails)
  followed = 0
  for thread in threads:
      assigned_projects = thread.mails
          .filter_map(|m| get_assignment_info(conn, m.id).project_id)
          .unique()
      if assigned_projects.len() != 1:
          continue  # 0件（未分類のみ） or 複数（曖昧）は対象外
      target_project = assigned_projects[0]
      for mail in thread.mails where mail is unclassified:
          assign_mail(conn, mail.id, target_project, "ai", None)
          followed += 1
  return followed
```

- `assigned_projects` が **ちょうど1件** のスレッドのみ対象（要件どおり複数案件へは追従しない）
- スレッド内の未割り当てメール全件を対象の案件へ割り当てる（部分的にしか追従しないと、
  また別の未分類メールが残ってしまい問題が再発するため）

### 4.3 `assigned_by` の扱い

`mail_project_assignments.assigned_by` は現状 `CHECK(assigned_by IN ('ai', 'user'))`。

スレッド追従は「AIによる意味的な分類判断」ではなく「スレッドという構造的事実からの機械的な
推論」であり、本来は両者と区別できる方が正確である。しかし:

- `assigned_by` はフロントエンドから一切参照されていない（表示・分岐なし）
- 3値目を追加するには CHECK 制約変更のためテーブル再構築マイグレーションが必要で、
  このスレッド追従機能の価値に対してリスク・複雑度が見合わない

以上より **v1 では `assigned_by = "ai"`, `confidence = None` を使う**。既存の「AI分類」の
意味を流用しつつ、`confidence` が `None`（AI分類は必ずスコアを持つ）であることで実質的に
区別可能にしておく。将来 `assigned_by` の3値化が必要になったタイミング（UIで割り当て根拠を
表示する要望が出た場合等）でマイグレーションを追加する。

### 4.4 `correction_log` への記録

**記録しない。**

`correction_log` は「AIの提案 → ユーザーが訂正した」という学習信号であり、LLMプロンプトの
few-shot例として使われる（`get_recent_corrections`）。スレッド追従はユーザーの訂正判断を
一切経由しない機械的な追従であり、ここに書くと「ユーザーがこの分類を選んだ」という誤った
学習信号になる。`move_mail_to_project` が新規割り当て時にも記録するのは、そこがユーザーの
明示的なD&D操作だからであり、本機能とは性質が異なる。

### 4.5 却下（reject）の尊重 — 除外トゥームストーン

**（レビュー指摘対応。2026-07-13 追記）**

初版では `reject_classification` が `mail_project_assignments` を DELETE するだけで却下の
痕跡を残さなかった。`auto_follow_threads` は一覧を開くたびに走るため、次のような**却下の
サイレント復活**が起きる:

1. ユーザーが m2 を却下（未分類へ戻す）
2. 一覧を開く → スレッド仲間 m1 が単一案件に割り当て済み
3. m2 がその案件へ自動追従で**再割り当て**される

ユーザーの明示的な却下が自動処理で黙って取り消されるのは製品原則に反する。よって
**却下を尊重するトゥームストーン方式**を採用する。

- **`follow_exclusions(mail_id TEXT PRIMARY KEY REFERENCES mails(id) ON DELETE CASCADE)`**
  を **migration v14** で追加（既存 `correction_log` は「訂正」用で「却下」を表現できず、
  流用は誤った学習信号になるため専用テーブルにする。4.4 と同じ理由）
- `reject_classification` は割り当て削除後に `follow_exclusions` へ INSERT（冪等）
- `auto_follow_threads` は `follow_exclusions` にあるメールを追従対象から除外する
- **手動割り当て（`move_mail_to_project`）成功時は除外行を DELETE** する。ユーザーが自ら
  同じ案件へ入れ直す＝「却下の意思を撤回した」とみなし、以後はそのメールも追従対象に戻す。
  手動割り当て自体は却下後も常に可能（除外は自動追従にのみ効く）
- メール削除時は `ON DELETE CASCADE` で除外行も自動的に消える

### 4.6 既知の挙動: pending な AI 提案との競合

**（レビュー Minor。実装変更なし・記録のみ）**

逐次分類（`PendingClassifications`）で m2 に対する AI 提案が保留中のときに、一覧取得経由で
`auto_follow_threads` が m2 を先に追従割り当てすることがある。この場合、後から確定する AI 提案は
`approve_classification` 経由で既存割り当てを上書きしうる（＝追従先と AI 提案先が異なれば AI 提案が
勝つ）。スレッド構造による追従は「同一スレッド＝同一案件」という強い手がかりであり、AI 提案が
それを上書きするのは望ましくない可能性はあるが、実利用では両者が同じ案件を指すことがほとんどで
実害は小さい。v1 では現状の挙動（後勝ち）のまま許容し、必要になれば追従済みメールを AI 提案の
対象から外す等の調整を将来行う。

## 5. スコープ外（v1）

- 同期直後のリアルタイム追従（同期経路の変更が必要なため。一覧取得時判定で代替）
- スレッド仲間が複数案件に割れている場合の解決UI（曖昧なまま未分類に残す。ユーザーが
  手動判断する）
- `assigned_by` の3値化・UIでの追従根拠表示
- 案件ビュー側で「スレッド追従で入った」ことを示すバッジ等の表示

## 6. テスト方針

### Rust（DB層・TDD）

`db::assignments::auto_follow_threads`:
- 未分類メールのスレッド仲間が単一案件に割り当て済み → 追従割り当てされる
- スレッド仲間が複数の異なる案件に割り当てられている → 追従しない（未分類のまま）
- スレッド仲間の割り当てが無い（全員未分類） → 何もしない
- 無関係なスレッド（他のスレッドの割り当て） → 影響なし
- 追従割り当て後、`correction_log` に記録が増えないこと
- 追従割り当ての `assigned_by` が `"ai"`、`confidence` が `None` であること
- **却下したメールは追従で復活しない**（reject → 再度 auto_follow で再割り当てされない）
- **却下後も手動割り当て（`move_mail_to_project`）は可能**
- **手動割り当てで除外トゥームストーンが解除される**（以後は追従の対象に戻る）

`db::migrations` v14:
- `follow_exclusions` テーブルが作成される
- メール削除で除外行が `ON DELETE CASCADE` で消える

`mails::get_all_mails_by_account`:
- 複数フォルダ（INBOX/Sent/Archive）のメールを全て返す
- 他アカウントのメールを含まない

`classify_commands::get_unclassified_threads`:
- 呼び出し時に追従対象があれば未分類一覧から消えていること（統合的な確認）

## 7. パフォーマンス改善（2026-07-13 追記）

**（コードベース調査レポート指摘 B-3 対応）**

`auto_follow_threads` は未分類一覧を開くたびに実行されるが、初版実装には
2つのコストがあった:

1. `get_all_mails_by_account` が body_text/body_html 込みで全メールをロードしていた
   （スレッド判定に本文は不要）
2. スレッド内のメール毎に `get_assignment_info` を個別発行していた
   （O(N)〜O(2N) クエリの N+1）

対応として 4.2 のアルゴリズムを次のとおり改める。要件・追従の判定結果は変えない:

- **軽量メタの専用クエリ**: 本文カラムを読まない `mails::ThreadMailMeta`
  （id / message_id / in_reply_to / references / subject / date）と
  `mails::get_thread_metas_by_account` を新設し、これを判定入力にする。
  取得範囲・順序（全フォルダ・date DESC）は `get_all_mails_by_account` と同一
- **スレッド判定の再利用**: `mails::group_mail_ids_into_threads` が軽量メタを
  `build_threads` に委譲してスレッドごとのメールID集合を返す（3章の
  「判定ロジックの重複実装はしない」を維持）
- **割り当ての先読み**: アカウント配下の `mail_project_assignments` を1クエリで
  `mail_id → project_id` の HashMap に読み出し、メール毎のクエリ発行を無くす
- **書き込みのトランザクション化**: 追従の `assign_mail` 群を1トランザクションに
  束ねる。本処理は一覧取得のたびに再実行される冪等な処理のため部分成功でも次回
  リカバリはされるが、単一コミットにより INSERT 毎の autocommit を避けられ、
  途中失敗時に「スレッドの一部だけ追従された」状態も残さない

これにより1回の実行あたりのクエリ数は「2 + メール数×最大2 + 追従数」から
「読み出し3 + 追従INSERT（単一コミット）」に固定される。

なお `mails::get_all_mails_by_account` の本番呼び出し元はこの改善で無くなった。
削除は別PRで行う（並行作業との競合回避のため本改善では `db/mails.rs` の
既存関数に触れない）。
