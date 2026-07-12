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

`mails::get_all_mails_by_account`:
- 複数フォルダ（INBOX/Sent/Archive）のメールを全て返す
- 他アカウントのメールを含まない

`classify_commands::get_unclassified_threads`:
- 呼び出し時に追従対象があれば未分類一覧から消えていること（統合的な確認）
