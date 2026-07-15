# Pigeon セキュリティ監査レポート

- **対象**: `/Users/h.aiso/Projects/pigeon`（Tauri 2 + Rust + React 19/TypeScript のデスクトップメールクライアント）
- **実施日**: 2026-07-14
- **手法**: 静的コード解析（読み取り専用）。4領域を並列監査（Rust機密情報/DB、Tauri commands入力検証/LLM送信境界、フロントエンドXSS、Tauri設定/依存関係）
- **前提**: 本アプリは外部から届く**悪意あるHTMLメールの表示**が前提であり、メール本文は最も敵対的な入力である

---

## エグゼクティブサマリ

| # | 深刻度 | 概要 | 主なファイル |
|---|--------|------|--------------|
| **1** | 🔴 Critical | `.env` に本番 Google OAuth クライアントシークレットが平文で存在 | `.env:3` |
| **2** | 🟠 High | SecureStore（Stronghold）の暗号鍵がソース公開の固定文字列 SHA256 でハードコード | `src-tauri/src/lib.rs:45` |
| **3** | 🟠 High | 分類時に案件コンテキストが `allow_cloud_context` を無視してクラウド LLM へ送信 | `classifier/service.rs:257` |
| **4** | 🟠 High | クラウド判定が `"claude"` のみ＋起動時 `cloud=false` 固定で、Vertex 系/起動時に送信可否ポリシーが無効化 | `directory_commands.rs:70`, `lib.rs:156-161` |
| **5** | 🟠 High | DOMPurify 3.4.0 に既知脆弱性（XSS バイパス含む）。HTML メール表示の要 | `package.json:23` |
| **6** | 🟠 High | メール HTML サニタイズが DOMPurify デフォルト依存（ホワイトリスト/iframe 隔離なし） | `MailBody.tsx:41` |
| **7** | 🟠 High | メール内リンクのクリック挙動が未制御（外部ブラウザ強制・noopener・スキーム制限なし） | `MailBody.tsx:37-49` |
| **8** | 🟡 Medium | メール本文のプロンプトインジェクションで自動案件割り当てを誘導可能 | `classifier/prompt.rs:30-37` |
| **9** | 🟡 Medium | `stat_file`/`read_attachments` が任意絶対パスを読み取り・送信可能 | `send_commands.rs:44,68` |
| **10** | 🟡 Medium | 受信 1 通あたりのサイズ上限がなく巨大メールで DoS（メモリ/DB 肥大） | `imap_client.rs:268-290` |
| **11** | 🟡 Medium | deep-link OAuth コールバックの URL 検証が `includes` のみで緩い | `accountStore.ts:112-123` |
| **12** | 🟡 Medium | 実鍵形式のダミー秘密鍵がリポジトリにコミット済み | `classifier/test_sa.json:5` |
| **13** | 🟡 Medium | 外部画像ブロックが CSP 単独依存でフロント多層防御なし | `tauri.conf.json:31` |
| **14** | 🔵 Low | CI に依存スキャン（pnpm audit / cargo audit / dependabot）が無い | `.github/` |
| **15** | 🔵 Low | Risk ゲート・監査が未配線のスケルトン（将来 MCP/Agent 公開時のリスク） | `usecase/gate.rs` |
| **16** | 🔵 Low | cid 画像の data URI に MIME 検証がない（フロント側） | `inlineImages.ts:11-28` |

**良好な設計（問題なし）**: CSP 明示設定・capabilities 最小権限（fs/shell/http 不使用）・withGlobalTauri 無効・`pull_request_target` 不使用・dist/secrets 未コミット・SQL 完全パラメータ化・FTS5/LIKE エスケープ・添付ファイル名サニタイズ・保存先パス検証・TLS 証明書検証（平文フォールバックなし）・返信引用のプレーンテキスト化・本文冒頭 1000 文字制限。

---

## 1. 🔴 Critical: `.env` に本番 OAuth クライアントシークレットが平文

**ファイル**: `.env:2-3`

```
PIGEON_GOOGLE_CLIENT_ID_DESKTOP=1071668072405-...apps.googleusercontent.com
PIGEON_GOOGLE_CLIENT_SECRET_DESKTOP=GOCSPX-...
```

`GOCSPX-` プレフィックスは Google OAuth クライアントシークレットの本物の形式であり、実在の値と推測される。**Git 追跡はされておらず**（`.gitignore` で除外・履歴にも無し）、この点は救い。しかし作業ディレクトリに平文で存在するため、バックアップ・画面共有・誤コミット・ログ等での漏洩リスクがある。

デスクトップアプリの OAuth クライアントシークレットは公開クライアントとして「厳密な秘密」ではない（本来の防御は PKCE）が、Google のポリシー上ローテーション対象であり、公開されればレート悪用・なりすまし同意画面のリスクがある。

**推奨対応**:
1. **このクライアントシークレットを即時ローテーション**（Google Cloud Console で再発行）
2. 今後は当該値をコミット可能物・共有物に置かない運用を徹底
3. Desktop OAuth は PKCE + シークレットレス構成を検討

---

## 2. 🟠 High: SecureStore 暗号鍵のハードコード

**ファイル**: `src-tauri/src/lib.rs:43-45`

```rust
// In production, this would use OS keychain. For now, derive from app identifier.
let key = Sha256::digest(b"com.haiso666.pigeon-secure-store-key");
```

Stronghold 暗号化スナップショット（IMAP/SMTP パスワード、OAuth トークン、Claude API キー、GCP サービスアカウント JSON を保管）の**マスター鍵が、ソース公開の固定文字列の SHA256**。ソースを読める攻撃者は誰でも同じ鍵を再現でき、`pigeon.stronghold` ファイルを窃取すれば全秘密情報を復号できる。暗号化の実効性がほぼ失われている。コメント自身が暫定実装であることを認めている技術的負債。

なお `secure_store.rs` 自体は `zeroize::Zeroizing` でパスワードをメモリからゼロ化しており、鍵の受け渡し以外の実装は適切。

**推奨対応**: OS キーチェーン（macOS Keychain / Windows Credential Manager / libsecret）でデバイス固有のランダム鍵を生成・保管し、それを Stronghold のパスフレーズに使う。CLAUDE.md のセキュリティルール「パスワード等は OS キーチェーンに保存する」に整合させる。

---

## 3. 🟠 High: 分類時に案件コンテキストがクラウド送信ポリシーを無視して漏洩

**ファイル**: `src-tauri/src/classifier/service.rs:257`

```rust
let project_summaries = projects::build_project_summaries(&conn, &mail.account_id, false)?;
```

第 3 引数 `for_cloud` に**常に `false`** を渡している。`db/projects.rs:122-125` では `for_cloud=false` のとき `allow_cloud_context` 設定を無視して全案件の `cached_context`（案件フォルダのファイル内容から生成した要約）をプロンプトへ注入する。この分類器のプロバイダが `claude` / `claude_vertex` / `gemini_vertex` の場合、案件コンテキストがそのまま**クラウド API へ送信される**。

設計書（`docs/design/2026-07-09-project-directory-context-design.md`）の不変条件「クラウドには `allow_cloud_context` 許可済み案件のみ context 注入」が分類経路で完全にバイパスされる。ダイジェスト生成側にはテストで守られた分岐があるが、**その生成物を分類で送る側で漏れている**。

**推奨対応**: `classify_one`/`classify_batch` にプロバイダのクラウド判定を渡し、`build_project_summaries(conn, account_id, is_cloud)` を呼ぶ。判定は下記 #4 と共通化する。

---

## 4. 🟠 High: クラウド判定が `"claude"` のみ＋起動時スキャンが `cloud=false` 固定

**ファイル**: `src-tauri/src/commands/directory_commands.rs:70-72`, `src-tauri/src/lib.rs:156-161`

```rust
// directory_commands.rs:70
let cloud = ... get_or_default(conn, "llm_provider", "ollama")? == "claude";
```

`rescan_project` の `cloud` フラグは、未許可ファイルをダイジェスト入力から除外する（`cloud_policy::is_cloud_allowed` フィルタ）ための唯一のスイッチ。しかし:

- `claude_vertex` / `gemini_vertex` はクラウド送信にもかかわらず `== "claude"` の判定で `cloud=false` となり、**未許可ファイルを含む全ファイル内容がダイジェスト入力に入り Vertex（クラウド）へ送信される**
- `lib.rs:156-161` の起動時バックグラウンドスキャンは `rescan_project(..., false)` と**無条件 `cloud=false`**。アプリ起動のたびに、クラウドプロバイダ設定でも送信可否ポリシーを一切適用せずにダイジェスト生成（＝クラウド送信）が走る

**推奨対応**:
1. クラウド判定を共通ヘルパ化し `matches!(provider, "claude" | "claude_vertex" | "gemini_vertex")` に統一
2. `lib.rs` の起動時スキャンでハードコードの `false` を実プロバイダ由来の `cloud` フラグへ置換

> #3 と #4 は「クラウドプロバイダ判定の共通化」と「分類・起動時スキャンへの正しい `cloud` フラグ伝播」という一連の修正でまとめて解消できる。**最優先領域**。

---

## 5. 🟠 High: DOMPurify 3.4.0 に既知脆弱性

**ファイル**: `package.json:23`（`"dompurify": "^3.4.0"`、lock 実体 `3.4.0`）

`pnpm audit` で 8 件（moderate 5 / low 3）。主なもの:
- IN_PLACE モードのサニタイズバイパスによる XSS（≤3.4.5）
- Hook mutation によるデフォルト許可リスト汚染（<3.4.7）
- Shadow Root 経由のバイパス（≤3.4.6 等）

本アプリは悪意ある HTML メールを DOMPurify でサニタイズして `dangerouslySetInnerHTML` で表示する（#6）ため、XSS バイパスが実際の攻撃経路になる。

**推奨対応**: `pnpm up dompurify@latest`（≥3.4.10）。deprecated な `@types/dompurify`（`package.json:33`）も削除。

---

## 6. 🟠 High: メール HTML サニタイズが DOMPurify デフォルト依存

**ファイル**: `src/components/mail-view/MailBody.tsx:41`

```tsx
dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(resolvedHtml) }}
```

メール本文はメイン Webview 内の React ツリーに直接描画されており、**iframe/`srcdoc`+`sandbox` による隔離がない**。唯一の XSS 防御境界は「オプション無しの DOMPurify 1 回」と Tauri の CSP のみ。デフォルトでもスクリプト実行 XSS は概ね防げるが、ホワイトリスト（`ALLOWED_TAGS`/`ALLOWED_ATTR`）未指定のため、mXSS 攻撃面が広く、`<form>`/`<input>` によるフィッシング UI、`<style>` による CSS 注入・UI redressing が通り得る。

参考: `SearchResults.tsx:10` は `ALLOWED_TAGS: ["b"]` と厳格で、これが良い手本。

**推奨対応**:
- 明示的ホワイトリスト化（表示に必要なタグ/属性のみ、`FORBID_TAGS: ['style','form','input','button']`、`FORBID_ATTR: ['style']`、`USE_PROFILES: { html: true }`）
- 可能なら本文を `srcdoc` + `sandbox` iframe で隔離する多層防御

---

## 7. 🟠 High: メール内リンクのクリック挙動が未制御

**ファイル**: `src/components/mail-view/MailBody.tsx:37-49`（クリック処理が存在しない）

メール HTML 内の `<a href>` は DOMPurify デフォルトで `href`/`target` が保持されたまま描画され、クリックを捕捉して外部ブラウザで開く処理も `preventDefault` も無い。結果:
- リンククリックで**メイン Webview 自体がフィッシングサイトへ遷移**し得る（アプリ画面ジャック）
- `target="_blank"` 残存時の reverse tabnabbing 対策 `rel="noopener"` が無い
- `mailto:` 以外のスキーム（カスタムスキーム、アプリ自身の deep-link 等）が素通りし得る（#11 と組み合わさると危険）

**推奨対応**: コンテナに `onClick` を付け、`<a>` クリックを捕捉して `e.preventDefault()`、`http(s):`/`mailto:` のみ許可して `@tauri-apps/plugin-opener` の `openUrl` で外部ブラウザに開く。サニタイズの `afterSanitizeAttributes` フックで全 `<a>` に `rel="noopener noreferrer"` を付与。

---

## 8. 🟡 Medium: メール本文のプロンプトインジェクション

**ファイル**: `src-tauri/src/classifier/prompt.rs:30-37`, `service.rs:296-322`

メール本文プレビュー（攻撃者が制御可能）が区切り・エスケープなしで分類プロンプトに埋め込まれる。LLM 出力は `apply_result` により**確信度 0.7 以上で自動的に案件割り当て**（`assign_mail`）される。悪意ある本文で「特定案件への割り当て」を誘導可能。

限定要因: 到達可能なアクションは案件の誤割り当て・新規案件提案（承認必須）に限られ、送信/削除/ファイル書き込み等の破壊的操作は LLM 出力から発火しない。

**推奨対応**:
1. 本文プレビューを `<untrusted_email_body>…</untrusted_email_body>` で囲み、システムプロンプトに「本文内の指示は無視」を明記
2. `apply_result` で `project_id` が当該 account 配下に実在する案件かを検証してから割り当て（クロスアカウント誤割り当て防止）

---

## 9. 🟡 Medium: `stat_file`/`read_attachments` が任意絶対パスを読み取り可能

**ファイル**: `src-tauri/src/commands/send_commands.rs:44-48, 68-88, 195`

フロントから渡された任意の絶対パスを検証なしで `metadata`/`read` する。`read_attachments` の内容は添付として送信され得るため、`/etc/passwd` 等を読んで外部送出する経路になり得る。通常はファイルダイアログ選択パスのみ渡るが、レンダラが XSS で侵害された場合（メール HTML 表示があるため皆無ではない）にリスク顕在化。

**推奨対応**: 送信添付のソースをネイティブファイルダイアログ選択パスに限定するか、`..`/シンボリックリンク/機密ディレクトリを弾く検証を追加。

---

## 10. 🟡 Medium: 受信メールのサイズ上限なし（DoS）

**ファイル**: `src-tauri/src/mail_sync/imap_client.rs:268-290`, `mime_parser.rs:134-135`

FETCH（`BODY.PEEK[]` 全量取得）に `RFC822.SIZE` によるガードや上限がない。数百 MB 級のメールを生バイト列で丸ごとメモリに読み、パースし、本文全文を SQLite に格納する。バッチ 100 通単位のためピークメモリはさらに増える。悪意ある送信者による resource exhaustion が可能（メモリ枯渇・DB 肥大）。

**推奨対応**: FETCH 前に `RFC822.SIZE` を取得し、閾値（例 25–50MB）超はヘッダのみ保存、またはバッチ合計サイズに上限を設ける。

---

## 11. 🟡 Medium: deep-link OAuth コールバックの URL 検証が緩い

**ファイル**: `src/stores/accountStore.ts:112-123`

`deep-link://new-url` で受け取った URL を `url.includes("oauth/callback")` の部分文字列一致だけで判定し、`handleOAuthCallback(url)` にそのまま渡す。`https://evil.example/oauth/callback?...` のような偽装 URL も通過する。deep-link は悪意あるリンク（#7）からも起動され得る。

**推奨対応**: `new URL(url)` でパースし、`protocol` が自アプリスキーム（`com.haiso666.pigeon:`）、`pathname` が厳密に `/oauth/callback` であることを検証。バックエンド側での `state` パラメータ検証も確認。

---

## 12. 🟡 Medium: 実鍵形式のダミー秘密鍵がコミット済み

**ファイル**: `src-tauri/src/classifier/test_sa.json:5`（Git 追跡あり）

`project_id: test-project` のダミーだが、有効な PEM 形式の RSA 秘密鍵ブロックが実体として含まれる。テスト用の使い捨て鍵と推測されるが、リポジトリに秘密鍵形式データを置くのはシークレットスキャナ誤検知・悪しき前例化の観点で非推奨。

**推奨対応**: テスト時にコード内で動的生成するか、鍵ブロックをダミー文字列に置換。実在 GCP プロジェクトに紐づかないことを確認。

---

## 13. 🟡 Medium: 外部画像ブロックが CSP 単独依存

**ファイル**: `src-tauri/tauri.conf.json:31`, `MailBody.tsx:41`

CSP が `img-src 'self' data: blob:` で外部 `http(s)` 画像をブロックしており、トラッキングピクセル対策として有効（良い設計）。ただしこれは副作用的保護で、フロントのサニタイズ側で外部画像を積極除去していない。将来 CSP に外部を追加した瞬間に全メールでトラッキング・IP リークが発生する。`style-src 'unsafe-inline'` があるため `<style>`/`style=` 許可時は CSS 経由漏洩の余地。

**推奨対応**: 画像表示を明示的ユーザーオプトインにする設計を検討。防御を CSP 単独に依存させない。#6 と併せて `style` 属性/`<style>` を禁止。

---

## 14. 🔵 Low: CI に依存スキャンが無い

**ファイル**: `.github/workflows/test.yml`, `.github/`（dependabot.yml 不在）

`pull_request` トリガーで `pull_request_target` 不使用・`secrets.*` 参照なし・`--frozen-lockfile` 使用と CI 自体は堅実。しかし `pnpm audit`/`cargo audit` ステップも dependabot も無く、#5 の DOMPurify 脆弱性が長期放置される構造的原因。

**推奨対応**: `.github/dependabot.yml`（npm + cargo + github-actions）を追加、または CI に `pnpm audit --prod` / `cargo audit` の non-blocking ステップを追加。

---

## 15. 🔵 Low: Risk ゲート・監査が未配線

**ファイル**: `src-tauri/src/usecase/gate.rs`, `context.rs:99-102`

`check` は `Read` のみ通過し `Reversible`/`Sensitive` は未実装エラー、監査シンクは `NoOpAuditSink` 固定。現状の Tauri コマンド（送信・削除・案件移動）はゲートを経由せず `db` を直接叩いており、実害はない。ただし将来 MCP/Agent driver を有効化する際、ゲート未配線のままだと承認・監査なしに Sensitive 操作が通る設計負債になる。

**推奨対応**: MCP/Agent 経路を公開する前に Sensitive/Reversible 操作を `dispatch`→`gate::check`→`audit` に配線。リグレッション監視対象として記録。

---

## 16. 🔵 Low: cid 画像 data URI の MIME 検証なし（フロント）

**ファイル**: `src/utils/inlineImages.ts:11-28`

cid 解決は `DOMParser` で `img[src^="cid:"]` の `src` のみを `data_uri` に差し替え、最終的に DOMPurify を通る安全な実装。ただし `data_uri` の中身（MIME）はバックエンド生成をフロントで検証していない。CSP の `img-src ... data:` により data 画像は許可されるため、バックエンドが MIME を画像に限定していることを確認すべき。

**推奨対応**: フロント側でも `dataUri` が `data:image/` で始まることを検証してから `setAttribute` する。

---

## 対応優先順位（推奨）

1. **#1 Critical** — `.env` の Google OAuth クライアントシークレットを即ローテーション
2. **#3 + #4 High** — クラウド送信境界の不変条件バイパス修正（判定共通化 + `cloud` フラグ伝播）※CLAUDE.md セキュリティルールの中核
3. **#2 High** — SecureStore 暗号鍵を OS キーチェーン由来のランダム鍵へ移行
4. **#5 High** — DOMPurify を ≥3.4.10 へ更新
5. **#6 + #7 High** — メール HTML の厳格サニタイズ + リンク外部ブラウザ強制（可能なら iframe 隔離）
6. **#8〜#13 Medium** — プロンプトインジェクション対策、任意パス読み取り制限、受信サイズ上限、deep-link URL 検証、test_sa.json ダミー化、画像オプトイン
7. **#14〜#16 Low** — CI 依存スキャン、Risk ゲート配線、cid MIME 検証

## 総評

Tauri のセキュリティ境界（CSP 明示・capabilities 最小権限・shell/fs/http プラグイン不使用・withGlobalTauri 無効・`pull_request_target` 不使用・dist/secrets 未コミット）と、SQL パラメータ化・添付ファイル名/保存先パスの検証・TLS 証明書検証・返信引用のプレーンテキスト化・本文 1000 文字制限は、いずれも堅実に実装されています。

最大の弱点は 2 点に集約されます。**(A) 秘密情報の取り扱い**（`.env` 平文シークレット + Stronghold 鍵ハードコード）、**(B) クラウド送信境界の不変条件**（分類・Vertex 系・起動時スキャンでの `allow_cloud_context` バイパス）。後者は本プロダクトの中核的なプライバシー保証（設計書 §5 の不変条件、CLAUDE.md のセキュリティルール）に直結するため、Critical に次ぐ最優先で修正すべきです。あわせて、悪意ある HTML メール描画の防御（DOMPurify 更新 + 厳格サニタイズ + リンク制御 + iframe 隔離）を多層化することを強く推奨します。

---

*本レポートは静的解析に基づく。#3/#4/#8 の一部は実際の LLM プロバイダ設定・DB 状態に依存するため、修正時は該当経路の統合テスト追加を推奨する（既存の `project_context` テストがダイジェスト生成側を守っている一方、分類送信側にはテストが無い）。*
