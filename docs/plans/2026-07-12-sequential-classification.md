# 逐次分類（新規提案の1件ずつ承認）実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 未分類メールの分類を「フロント主導の逐次処理」に変え、新規プロジェクト提案(create)を1件ずつ承認させ、承認したプロジェクトを即座に一覧反映して以降のメールに反映する。

**Architecture:** バックエンドの一括ループ `classify_unassigned` を削除し、フロント(`classifyStore`)が `get_unclassified_mails` で対象一覧を取り、1件ずつ `classify_mail` を呼ぶ。`classify_mail` は毎回 `build_project_summaries` を読むため、承認直後の新プロジェクトが自動で候補に入る。

**Tech Stack:** Rust / Tauri 2、React 19 + TypeScript + Zustand、Vitest + RTL / cargo test。

## Global Constraints

- `unwrap()`/`expect()` はテストコード以外で使用しない。
- Tauri commands は `Result<T, AppError>` を返す。
- TypeScript で `any` を使わない。invoke レスポンスに型を付ける。
- create 提案のときだけ停止する。assign / unclassified は自動で次へ。却下したメールは未分類のまま。
- Conventional Commits（scope: classifier / ui）。

## File Structure

- `src-tauri/src/commands/classify_commands.rs` — `classify_unassigned` / `cancel_classification` / `ClassifyCancelFlag` と関連テスト削除
- `src-tauri/src/lib.rs` — command 登録と `ClassifyCancelFlag` の manage 削除
- `src/stores/projectStore.ts` — `addProject` 追加
- `src/stores/classifyStore.ts` — 逐次制御へ全面変更
- `src/types/classifier.ts` — `ClassifyProgress` / `ClassifySummary` の整理
- `src/components/thread-list/UnclassifiedList.tsx` — 1件表示、summary/listener 参照除去
- `src/components/thread-list/ClassifyButton.tsx` — store 変更への追随（大きな変更なし）
- テスト: `src/__tests__/stores/classifyStore.test.ts`, `src/__tests__/NewProjectProposal.test.tsx`, `src/__tests__/Sidebar.test.tsx`（影響あれば）

---

## Task 1: バックエンドの一括ループ削除

**Files:**
- Modify: `src-tauri/src/commands/classify_commands.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Removed: `classify_unassigned`, `cancel_classification`, `ClassifyCancelFlag`
- Kept: `classify_mail`, `approve_new_project`, `reject_classification`, `get_unclassified_mails`, `PendingClassifications`

- [ ] **Step 1: classify_commands.rs から削除**

`src-tauri/src/commands/classify_commands.rs` から次を削除する:
- `ClassifyCancelFlag` 構造体とその `impl`（28-35行付近: `pub struct ClassifyCancelFlag(pub Arc<AtomicBool>);` と `impl ClassifyCancelFlag { pub fn new() ... }`）
- `classify_unassigned` 関数全体（`#[tauri::command] pub async fn classify_unassigned(...) { ... }`、95-217行付近）
- `cancel_classification` 関数全体（219-224行付近）
- `#[cfg(test)]` 内の `test_cancel_flag_toggle`（509-行付近、`ClassifyCancelFlag` を使うテスト）
- 使わなくなる import: `std::sync::atomic::{AtomicBool, Ordering}`、`std::sync::Arc`、`tauri::{AppHandle, Emitter}`（`State` は他で使うなら残す）。コンパイラの unused 警告に従って正確に削除する。

- [ ] **Step 2: lib.rs から登録削除**

`src-tauri/src/lib.rs`:
- `.manage(commands::classify_commands::ClassifyCancelFlag::new())` の行を削除。
- `invoke_handler` の `commands::classify_commands::classify_unassigned,` と `commands::classify_commands::cancel_classification,` の2行を削除。

- [ ] **Step 3: ビルドとテスト**

Run: `cd src-tauri && cargo build 2>&1 | grep -E "^error|unused" | head`
Expected: エラーなし（unused 警告が出たら該当 import を削除して再ビルド）。

Run: `cd src-tauri && cargo test classify`
Expected: PASS（残った classify_mail / approve_new_project / reject / get_unclassified_mails のテストが緑。削除したテストは消えている）。

- [ ] **Step 4: コミット**

```bash
git add src-tauri/src/commands/classify_commands.rs src-tauri/src/lib.rs
git commit -m "refactor(classifier): 一括分類ループ classify_unassigned を削除"
```

---

## Task 2: projectStore に addProject を追加

**Files:**
- Modify: `src/stores/projectStore.ts`
- Test: `src/__tests__/stores/projectStore.test.ts`（無ければ作成）

**Interfaces:**
- Produces: `addProject: (project: Project) => void`（既存 `projects` 配列に追加。重複ID は追加しない）

- [ ] **Step 1: 失敗するテストを書く**

`src/__tests__/stores/projectStore.test.ts` に追加（ファイルが無ければ新規作成し、既存の projectStore テストの import 形式に合わせる）:

```ts
import { describe, it, expect, beforeEach } from "vitest";
import { useProjectStore } from "../../stores/projectStore";
import type { Project } from "../../types/project";

const makeProject = (id: string, name: string): Project =>
  ({ id, name, account_id: "acc1", description: null, color: null } as Project);

describe("projectStore.addProject", () => {
  beforeEach(() => {
    useProjectStore.setState({ projects: [] });
  });

  it("既存配列にプロジェクトを追加する", () => {
    useProjectStore.getState().addProject(makeProject("p1", "Alpha"));
    expect(useProjectStore.getState().projects.map((p) => p.id)).toEqual(["p1"]);
  });

  it("同じIDは重複追加しない", () => {
    useProjectStore.getState().addProject(makeProject("p1", "Alpha"));
    useProjectStore.getState().addProject(makeProject("p1", "Alpha dup"));
    expect(useProjectStore.getState().projects).toHaveLength(1);
  });
});
```

（`Project` 型の必須フィールドは `src/types/project.ts` を確認して `makeProject` を合わせること。上記は最小例。）

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm vitest run src/__tests__/stores/projectStore.test.ts`
Expected: FAIL（`addProject` 未定義）

- [ ] **Step 3: addProject を実装**

`src/stores/projectStore.ts` の interface（`ProjectState`）に追加:
```ts
  addProject: (project: Project) => void;
```
store 実装に追加（`fetchProjects` の近くに）:
```ts
  addProject: (project) => {
    const exists = get().projects.some((p) => p.id === project.id);
    if (exists) return;
    set({ projects: [...get().projects, project] });
  },
```

- [ ] **Step 4: テストが通ることを確認**

Run: `pnpm vitest run src/__tests__/stores/projectStore.test.ts`
Expected: PASS

- [ ] **Step 5: コミット**

```bash
git add src/stores/projectStore.ts src/__tests__/stores/projectStore.test.ts
git commit -m "feat(ui): projectStore に作成済みプロジェクトを追加する addProject を追加"
```

---

## Task 3: classifyStore を逐次制御に書き換え

**Files:**
- Modify: `src/stores/classifyStore.ts`
- Modify: `src/types/classifier.ts`（`ClassifyProgress` は不要になる。`ClassifySummary` の扱いは下記）
- Test: `src/__tests__/stores/classifyStore.test.ts`

**Interfaces:**
- Consumes: `invoke("get_unclassified_mails", {accountId})`, `invoke("classify_mail", {mailId})`, `invoke("approve_new_project", ...)`, `invoke("reject_classification", {mailId})`, `useProjectStore.getState().addProject`
- Produces: `classifyStore` の新 state/メソッド:
  - state: `classifying: boolean`, `progress: {current, total} | null`, `pendingProposal: ClassifyResponse | null`, `error: string | null`
  - methods: `classifyAll(accountId)`, `approveNewProject(mailId, name, desc?)`, `rejectClassification(mailId)`, `cancelClassification()`, `classifyMail(mailId)`（単発は維持）

- [ ] **Step 1: 失敗するテストを書く**

`src/__tests__/stores/classifyStore.test.ts` を新挙動に置き換える（既存テストは旧イベント方式なので全面更新）:

```ts
import { describe, it, expect, vi, beforeEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));
// listen はもう使わないが、他モジュールが読む可能性に備えてスタブ
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

import { useClassifyStore } from "../../stores/classifyStore";
import { useProjectStore } from "../../stores/projectStore";

const resetStore = () =>
  useClassifyStore.setState({
    classifying: false,
    progress: null,
    pendingProposal: null,
    error: null,
  });

beforeEach(() => {
  invokeMock.mockReset();
  resetStore();
  useProjectStore.setState({ projects: [] });
});

// classify_mail は Rust の ClassifyResponse = { mail_id, result: {...} } を返す。
// テストのモックもこのネスト形に合わせる。
const resp = (mailId: string, action: string, extra: object = {}) => ({
  mail_id: mailId,
  result: { action, confidence: 0.9, reason: "r", ...extra },
});

describe("classifyStore sequential flow", () => {
  it("assign結果は自動で次のメールへ進む", async () => {
    invokeMock.mockImplementation((cmd: string, args: { mailId?: string }) => {
      if (cmd === "get_unclassified_mails")
        return Promise.resolve([{ id: "m1" }, { id: "m2" }]);
      if (cmd === "classify_mail")
        return Promise.resolve(resp(args.mailId as string, "assign", { project_id: "p1" }));
      return Promise.resolve();
    });
    await useClassifyStore.getState().classifyAll("acc1");
    // 2件とも classify_mail が呼ばれ、停止していない
    const calls = invokeMock.mock.calls.filter((c) => c[0] === "classify_mail");
    expect(calls).toHaveLength(2);
    expect(useClassifyStore.getState().pendingProposal).toBeNull();
    expect(useClassifyStore.getState().classifying).toBe(false);
  });

  it("create結果で停止し pendingProposal に1件セットする", async () => {
    invokeMock.mockImplementation((cmd: string, args: { mailId?: string }) => {
      if (cmd === "get_unclassified_mails")
        return Promise.resolve([{ id: "m1" }, { id: "m2" }]);
      if (cmd === "classify_mail")
        return Promise.resolve(
          resp(args.mailId as string, "create", { project_name: "New", description: "d" }),
        );
      return Promise.resolve();
    });
    await useClassifyStore.getState().classifyAll("acc1");
    // m1 で create → 停止。classify_mail は1回だけ。
    const calls = invokeMock.mock.calls.filter((c) => c[0] === "classify_mail");
    expect(calls).toHaveLength(1);
    expect(useClassifyStore.getState().pendingProposal?.mail_id).toBe("m1");
    expect(useClassifyStore.getState().classifying).toBe(true);
  });

  it("approveNewProject でプロジェクトを一覧追加し次へ進む", async () => {
    let step = 0;
    invokeMock.mockImplementation((cmd: string, args: { mailId?: string }) => {
      if (cmd === "get_unclassified_mails")
        return Promise.resolve([{ id: "m1" }, { id: "m2" }]);
      if (cmd === "classify_mail") {
        step++;
        // m1 は create、m2 は assign
        return Promise.resolve(
          step === 1
            ? resp("m1", "create", { project_name: "New" })
            : resp("m2", "assign", { project_id: "np" }),
        );
      }
      if (cmd === "approve_new_project")
        return Promise.resolve({ id: "np", name: "New", account_id: "acc1", description: null, color: null });
      return Promise.resolve();
    });
    await useClassifyStore.getState().classifyAll("acc1");
    expect(useClassifyStore.getState().pendingProposal?.mail_id).toBe("m1");

    await useClassifyStore.getState().approveNewProject("m1", "New");
    // 新プロジェクトが一覧に入った
    expect(useProjectStore.getState().projects.map((p) => p.id)).toContain("np");
    // pending クリア、m2 まで進んで完了
    expect(useClassifyStore.getState().pendingProposal).toBeNull();
    expect(useClassifyStore.getState().classifying).toBe(false);
  });

  it("rejectClassification で次へ進む（未分類のまま）", async () => {
    let step = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "get_unclassified_mails")
        return Promise.resolve([{ id: "m1" }, { id: "m2" }]);
      if (cmd === "classify_mail") {
        step++;
        return Promise.resolve(
          step === 1 ? resp("m1", "create", { project_name: "New" }) : resp("m2", "unclassified"),
        );
      }
      return Promise.resolve();
    });
    await useClassifyStore.getState().classifyAll("acc1");
    await useClassifyStore.getState().rejectClassification("m1");
    expect(invokeMock.mock.calls.some((c) => c[0] === "reject_classification")).toBe(true);
    expect(useClassifyStore.getState().pendingProposal).toBeNull();
    expect(useClassifyStore.getState().classifying).toBe(false);
  });

  it("cancelClassification 後は次を分類しない", async () => {
    invokeMock.mockImplementation((cmd: string, args: { mailId?: string }) => {
      if (cmd === "get_unclassified_mails")
        return Promise.resolve([{ id: "m1" }, { id: "m2" }, { id: "m3" }]);
      if (cmd === "classify_mail") {
        // m1 を create にして停止させ、その隙にキャンセル
        return Promise.resolve(resp(args.mailId as string, "create", { project_name: "N" }));
      }
      return Promise.resolve();
    });
    await useClassifyStore.getState().classifyAll("acc1");
    await useClassifyStore.getState().cancelClassification();
    const before = invokeMock.mock.calls.filter((c) => c[0] === "classify_mail").length;
    // reject して次へ進もうとしてもキャンセル済みなので進まない
    await useClassifyStore.getState().rejectClassification("m1");
    const after = invokeMock.mock.calls.filter((c) => c[0] === "classify_mail").length;
    expect(after).toBe(before);
    expect(useClassifyStore.getState().classifying).toBe(false);
  });
});
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm vitest run src/__tests__/stores/classifyStore.test.ts`
Expected: FAIL（旧 store には逐次挙動が無い）

- [ ] **Step 3: classifyStore を書き換える**

`src/types/classifier.ts` を確認し、`ClassifyProgress` を使わなくするなら削除、`ClassifySummary` は store から外すなら参照を消す（型自体は残してよい）。

`src/stores/classifyStore.ts` を全面的に次へ置き換える:

```ts
import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { useErrorStore } from "./errorStore";
import { useProjectStore } from "./projectStore";
import type { ClassifyResponse } from "../types/classifier";
import type { Project } from "../types/project";

interface UnclassifiedMailRef {
  id: string;
}

// classify_mail の戻り `result`（Rust ClassifyResult、#[serde(tag="action")]）のフラット形。
// action ごとに project_id（assign）/ project_name・description（create）が付く。
interface ClassifyResultRaw {
  action: "assign" | "create" | "unclassified";
  project_id?: string;
  project_name?: string;
  description?: string;
  confidence: number;
  reason: string;
}

interface ClassifyState {
  classifying: boolean;
  progress: { current: number; total: number } | null;
  pendingProposal: ClassifyResponse | null;
  error: string | null;
  // 内部: 逐次ループの状態
  _queue: UnclassifiedMailRef[];
  _index: number;
  _cancelled: boolean;

  classifyMail: (mailId: string) => Promise<void>;
  classifyAll: (accountId: string) => Promise<void>;
  cancelClassification: () => Promise<void>;
  approveNewProject: (
    mailId: string,
    projectName: string,
    description?: string,
  ) => Promise<void>;
  rejectClassification: (mailId: string) => Promise<void>;
}

export const useClassifyStore = create<ClassifyState>((set, get) => {
  // 次の1件を分類し、create でなければ自動で次へ進む
  const classifyNext = async (): Promise<void> => {
    const { _queue, _index, _cancelled } = get();
    if (_cancelled || _index >= _queue.length) {
      set({ classifying: false, progress: null, pendingProposal: null });
      return;
    }
    const mail = _queue[_index];
    let res: ClassifyResponse;
    try {
      // classify_mail は Rust の ClassifyResponse = { mail_id, result: ClassifyResult }
      // を返す（result の中に action/confidence/reason/project_id/project_name/description）。
      // フロントの ClassifyResponse はフラットなので、ここで平坦化する。
      const r = await invoke<{ mail_id: string; result: ClassifyResultRaw }>(
        "classify_mail",
        { mailId: mail.id },
      );
      res = { mail_id: r.mail_id, ...r.result };
    } catch (e) {
      useErrorStore.getState().addError(String(e));
      set({ classifying: false, progress: null });
      return;
    }
    set({
      _index: _index + 1,
      progress: { current: _index + 1, total: _queue.length },
    });
    if (res.action === "create") {
      set({ pendingProposal: res });
      return; // 停止：承認/却下を待つ
    }
    await classifyNext();
  };

  return {
    classifying: false,
    progress: null,
    pendingProposal: null,
    error: null,
    _queue: [],
    _index: 0,
    _cancelled: false,

    classifyMail: async (mailId) => {
      try {
        await invoke("classify_mail", { mailId });
      } catch (e) {
        set({ error: String(e) });
        useErrorStore.getState().addError(String(e));
      }
    },

    classifyAll: async (accountId) => {
      try {
        const mails = await invoke<UnclassifiedMailRef[]>(
          "get_unclassified_mails",
          { accountId },
        );
        set({
          classifying: true,
          _queue: mails,
          _index: 0,
          _cancelled: false,
          pendingProposal: null,
          progress: { current: 0, total: mails.length },
          error: null,
        });
        await classifyNext();
      } catch (e) {
        set({ error: String(e), classifying: false, progress: null });
        useErrorStore.getState().addError(String(e));
      }
    },

    cancelClassification: async () => {
      set({ _cancelled: true, classifying: false, progress: null, pendingProposal: null });
    },

    approveNewProject: async (mailId, projectName, description) => {
      try {
        const project = await invoke<Project>("approve_new_project", {
          mailId,
          projectName,
          description: description ?? null,
        });
        useProjectStore.getState().addProject(project);
        set({ pendingProposal: null });
        await classifyNext();
      } catch (e) {
        set({ error: String(e) });
        useErrorStore.getState().addError(String(e));
      }
    },

    rejectClassification: async (mailId) => {
      try {
        await invoke("reject_classification", { mailId });
      } catch (e) {
        useErrorStore.getState().addError(String(e));
      }
      set({ pendingProposal: null });
      await classifyNext();
    },
  };
});
```

注: `classify_mail` コマンドの戻り型は `ClassifyResponse`（`{ mail_id, result }` を包む型）。実際の Tauri 戻り値の形（`{ mail_id, result }`）に合わせて `r.result` を取り出している。`src/types/classifier.ts` の `ClassifyResponse` 定義と、`classify_mail` の実際の戻り JSON を確認し、取り出し方を一致させること（もし `classify_mail` が直接 `ClassifyResponse` 形（mail_id/action を持つフラット構造）を返すなら `res = r` にする）。

- [ ] **Step 4: テストが通ることを確認**

Run: `pnpm vitest run src/__tests__/stores/classifyStore.test.ts`
Expected: PASS

- [ ] **Step 5: 型チェック**

Run: `pnpm tsc --noEmit`
Expected: エラーなし（`UnclassifiedList`/`ClassifyButton` がまだ旧 API を参照していれば次タスクで直す。ここで tsc が通らない場合、それらは Task 4/5 で修正するので、このタスクのコミットは classifyStore とそのテストに限定してよい。ただし tsc は最終的に Task 5 完了時点で通ればよい）

- [ ] **Step 6: コミット**

```bash
git add src/stores/classifyStore.ts src/types/classifier.ts src/__tests__/stores/classifyStore.test.ts
git commit -m "feat(ui): classifyStore を逐次分類（create提案で停止）に変更"
```

---

## Task 4: UnclassifiedList を1件表示に更新

**Files:**
- Modify: `src/components/thread-list/UnclassifiedList.tsx`

**Interfaces:**
- Consumes: `classifyStore.pendingProposal`, `approveNewProject`, `rejectClassification`

- [ ] **Step 1: pendingProposal 1件表示に変更**

`src/components/thread-list/UnclassifiedList.tsx` を次のように変更:

- 削除する参照: `results`, `summary`, `initClassifyListeners`（およびそれらを使う `useEffect`（43-48行の listener 登録、50-55行の summary 監視）と `createResults`（64行））。
- 代わりに `const pendingProposal = useClassifyStore((s) => s.pendingProposal);` を追加。
- summary 表示ブロック（81-88行）を削除。
- `createResults.length > 0 && (...)`（90-104行）を、`pendingProposal` があるとき1件だけ表示する形に置き換え:

```tsx
      {pendingProposal && pendingProposal.action === "create" && (
        <div className="space-y-2 px-4 pb-2">
          <NewProjectProposal
            key={pendingProposal.mail_id}
            mailId={pendingProposal.mail_id}
            suggestedName={pendingProposal.project_name ?? ""}
            suggestedDescription={pendingProposal.description}
            reason={pendingProposal.reason}
            onApprove={handleApproveNewProject}
            onReject={rejectClassification}
          />
        </div>
      )}
```

- 分類完了後の再取得: 旧コードは summary をトリガに `fetchProjects`/`fetchUnclassified` していた。逐次では `classifying` が false に戻ったときに `fetchUnclassified` するよう変更（プロジェクトは approveNewProject で addProject 済みなので `fetchProjects` は必須でないが、整合のため呼んでよい）:

```tsx
  useEffect(() => {
    if (!classifying && selectedAccountId) {
      fetchUnclassified(selectedAccountId);
    }
  }, [classifying, selectedAccountId, fetchUnclassified]);
```

（`initClassifyListeners` の useEffect は削除。`summary` の import と分割代入も削除。）

- [ ] **Step 2: 型チェック**

Run: `pnpm tsc --noEmit`
Expected: エラーなし（classifyStore に無くなった `results`/`summary`/`initClassifyListeners` を参照していないこと）

- [ ] **Step 3: フロントテスト**

Run: `pnpm vitest run`
Expected: PASS（`UnclassifiedList` を描画するテストがあれば新挙動で緑。壊れたら新 API に合わせて更新する）

- [ ] **Step 4: コミット**

```bash
git add src/components/thread-list/UnclassifiedList.tsx
git commit -m "feat(ui): 新規プロジェクト提案を常に1件だけ表示する"
```

---

## Task 5: ClassifyButton の追随と全体整合

**Files:**
- Modify: `src/components/thread-list/ClassifyButton.tsx`（必要なら）
- Modify: 影響する既存テスト

**Interfaces:**
- Consumes: `classifyStore.classifying`, `progress`, `classifyAll`, `cancelClassification`

- [ ] **Step 1: ClassifyButton の確認**

`src/components/thread-list/ClassifyButton.tsx` は `classifying` / `progress` / `classifyAll` / `cancelClassification` を参照している。これらは新 store にも存在するため、**基本的に変更不要**。`progress.total` が 0 のときの `width` 計算（0除算 → NaN）だけ確認し、必要なら次のようにガードする:

```tsx
                width: progress && progress.total > 0
                  ? `${(progress.current / progress.total) * 100}%`
                  : "0%",
```

- [ ] **Step 2: 影響テストの更新**

Run: `pnpm vitest run`
壊れているテスト（旧 `classifyStore` の `results`/`summary`/`classify-progress` を前提にしたもの）を新挙動へ更新する。特に:
- `src/__tests__/NewProjectProposal.test.tsx` — コンポーネント単体テストなら影響小。props が変わっていなければそのまま。
- `src/__tests__/Sidebar.test.tsx` — classifyStore をモックしているなら新 state 形に合わせる。

各テストを新 API（`pendingProposal` 中心）に合わせて修正する。

- [ ] **Step 3: 型チェックと全フロントテスト**

Run: `pnpm tsc --noEmit && pnpm vitest run`
Expected: 全緑。

- [ ] **Step 4: コミット**

```bash
git add src/components/thread-list/ClassifyButton.tsx src/__tests__/
git commit -m "feat(ui): 逐次分類に合わせて分類ボタンとテストを更新"
```

---

## Task 6: 全体検証と設計書ステータス更新

**Files:**
- Modify: `docs/archive/specs/2026-07-12-sequential-classification-design.md`

- [ ] **Step 1: Rust 全テスト**

Run: `cd src-tauri && cargo test`
Expected: 全緑（削除済み分を除き、既存が通る）。

- [ ] **Step 2: フロント検証**

Run: `pnpm tsc --noEmit && pnpm vitest run`
Expected: 全緑。（repo に lint スクリプトは無いので `pnpm lint` は実行しない。）

- [ ] **Step 3: 設計書ステータス更新**

`docs/archive/specs/2026-07-12-sequential-classification-design.md` のステータス行を `承認済み（実装前）` → `実装済み` に更新。

- [ ] **Step 4: コミット**

```bash
git add docs/archive/specs/2026-07-12-sequential-classification-design.md
git commit -m "docs(specs): 逐次分類の設計書ステータスを実装済みに更新"
```
