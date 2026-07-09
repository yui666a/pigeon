# 案件ディレクトリ連携（UI）実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 案件ディレクトリ連携のフロントエンド（紐付けUI・再スキャン・状態表示・クラウド送信設定ダイアログ）を実装する。

**Architecture:** バックエンド（PR #57 までの9コマンド）を Zustand の `projectStore` 拡張から invoke し、既存の3ペインUIへ最小追加。新ペインは作らない。クラウド送信可否の判定ロジック（最長マッチ）は Rust 側と同一セマンティクスの TS ユーティリティとして再実装し、チェックボックスツリーの表示状態を導出する。

**Tech Stack:** React 19 / TypeScript / Zustand 5 / Tailwind CSS v4 / Vitest + React Testing Library / tauri-plugin-dialog（新規導入）

**Spec:** `docs/superpowers/specs/2026-07-09-project-directory-context-design.md` §7（UI）が正。バックエンドのコマンド契約はバックエンドプランの Task 12 Interfaces 節。

## Global Constraints

- `any` は使用しない。invoke のレスポンスには必ず型を付ける（`src/types/directory.ts` に集約）
- 関数コンポーネントのみ。1ファイル1コンポーネント。Props は interface で定義
- グローバル状態は Zustand（`projectStore` を拡張。新ストアは作らない — スペック§7）
- テスト実行: `pnpm test`（Vitest、リポジトリルートで実行）
- 既存テストパターンに従う: store テストは `vi.mock("@tauri-apps/api/core")` + `mockInvoke`、コンポーネントテストは RTL
- クラウド送信UIのデフォルトは**すべて送信オフ**表示（ルール不在 = 不許可。TS 判定ユーティリティも Rust と同じく「マッチ無し→false」）
- invoke のコマンド名・引数はバックエンド実装が正（camelCase 引数: `projectId`, `directoryId`, `relativePath`）。レスポンスのフィールドは snake_case（serde デフォルト）
- コミットは Conventional Commits（scope: `ui`）
- 各タスク完了時に `pnpm test` 全パス
- PR 分割の目安: 本プラン全体で 1 PR（`feat/directory-context-ui`、親: feat/directory-context-commands）

---

### Task 1: tauri-plugin-dialog 導入 + 型定義

**Files:**
- Modify: `src-tauri/Cargo.toml`（dependencies に追加）
- Modify: `src-tauri/src/lib.rs`（plugin 登録）
- Modify: `src-tauri/capabilities/default.json`（permission 追加）
- Modify: `package.json`（`pnpm add @tauri-apps/plugin-dialog`）
- Create: `src/types/directory.ts`

**Interfaces:**
- Produces: フロント全体が使う型 `ProjectDirectory` / `ProjectFile` / `CloudRule` / `ProjectContext` / `RescanOutcome`、フォルダ選択 `open({ directory: true })`（@tauri-apps/plugin-dialog）

- [ ] **Step 1: Rust 側にプラグインを追加**

`src-tauri/Cargo.toml` の `[dependencies]` に追加:

```toml
tauri-plugin-dialog = "2"
```

`src-tauri/src/lib.rs` の `.plugin(tauri_plugin_deep_link::init())` の直後に追加:

```rust
        .plugin(tauri_plugin_dialog::init())
```

`src-tauri/capabilities/default.json` の permissions に `"dialog:default"` を追加:

```json
  "permissions": [
    "core:default",
    "opener:default",
    "deep-link:default",
    "dialog:default"
  ]
```

- [ ] **Step 2: フロント側の依存を追加**

Run: `pnpm add @tauri-apps/plugin-dialog`
Expected: package.json の dependencies に `"@tauri-apps/plugin-dialog": "^2..."` が追加される

- [ ] **Step 3: 型定義を作成**

`src/types/directory.ts`:

```typescript
export interface ProjectDirectory {
  id: string;
  project_id: string;
  path: string;
  is_primary: boolean;
  status: "ok" | "missing" | "inaccessible" | "error";
  last_scanned_at: string | null;
  created_at: string;
}

export interface ProjectFile {
  id: string;
  directory_id: string;
  relative_path: string;
  size_bytes: number;
  mtime: string;
  content_hash: string | null;
  content_kind: "none" | "text" | "pdf" | "office" | "other";
  extract_status: "ok" | "skipped_too_large" | "unsupported" | "error";
  indexed_at: string;
}

export interface CloudRule {
  id: string;
  directory_id: string;
  scope: "directory" | "file";
  relative_path: string;
  allow: boolean;
}

export interface ProjectContext {
  project_id: string;
  cached_context: string | null;
  context_hash: string | null;
  inventory_hash: string | null;
  allow_cloud_context: boolean;
  generated_at: string | null;
}

export interface RescanOutcome {
  status: string;
  regenerated: boolean;
  file_count: number;
}
```

- [ ] **Step 4: ビルド確認**

Run: `cd src-tauri && cargo build 2>&1 | tail -3 && cd .. && pnpm test`
Expected: cargo build 成功（dialog プラグインがリンクされる）、既存テスト全パス（新規型はまだ未使用なので影響なし）

- [ ] **Step 5: コミット**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs \
        src-tauri/capabilities/default.json package.json pnpm-lock.yaml src/types/directory.ts
git commit -m "feat(ui): tauri-plugin-dialogを導入しディレクトリ連携の型を定義"
```

---

### Task 2: クラウド送信判定の TS ユーティリティ

**Files:**
- Create: `src/utils/cloudPolicy.ts`
- Test: `src/__tests__/utils/cloudPolicy.test.ts`

**Interfaces:**
- Consumes: `CloudRule`（Task 1）
- Produces:
  - `effectiveAllow(rules: CloudRule[], relativePath: string): boolean` — Rust の `cloud_policy::is_cloud_allowed` と同一セマンティクス（マッチ無し→false / 最長マッチ / 同長は file 優先 / `..` セグメントは無条件 false / directory は `''`=全体・完全一致・`{rule}/` 前方一致）
  - `planToggle(rules: CloudRule[], scope: "directory" | "file", relativePath: string): ToggleAction` — チェックボックス切替時に「明示ルールを設定すべきか、自ルール削除で親の継承に戻すべきか」を返す
  - `type ToggleAction = { action: "set"; allow: boolean } | { action: "delete" }`

- [ ] **Step 1: 失敗するテストを書く**

`src/__tests__/utils/cloudPolicy.test.ts`:

```typescript
import { describe, it, expect } from "vitest";
import { effectiveAllow, planToggle } from "../../utils/cloudPolicy";
import type { CloudRule } from "../../types/directory";

function rule(scope: "directory" | "file", path: string, allow: boolean): CloudRule {
  return { id: `r-${scope}-${path}`, directory_id: "d1", scope, relative_path: path, allow };
}

describe("effectiveAllow", () => {
  it("returns false when no rules match (deny by default)", () => {
    expect(effectiveAllow([], "図面/平面図.pdf")).toBe(false);
  });

  it("directory allow covers children but not lookalike prefixes", () => {
    const rules = [rule("directory", "図面", true)];
    expect(effectiveAllow(rules, "図面/平面図.pdf")).toBe(true);
    expect(effectiveAllow(rules, "図面/sub/詳細.pdf")).toBe(true);
    expect(effectiveAllow(rules, "図面")).toBe(true);
    expect(effectiveAllow(rules, "契約/見積.pdf")).toBe(false);
    expect(effectiveAllow(rules, "図面外.txt")).toBe(false);
  });

  it("root directory rule ('') covers everything", () => {
    const rules = [rule("directory", "", true)];
    expect(effectiveAllow(rules, "anything.txt")).toBe(true);
    expect(effectiveAllow(rules, "a/b/c.txt")).toBe(true);
  });

  it("explicit file deny beats parent allow (longest match wins)", () => {
    const rules = [rule("directory", "", true), rule("file", "予算メモ.md", false)];
    expect(effectiveAllow(rules, "他.txt")).toBe(true);
    expect(effectiveAllow(rules, "予算メモ.md")).toBe(false);
  });

  it("file scope requires exact match", () => {
    const rules = [rule("file", "香盤表.md", true)];
    expect(effectiveAllow(rules, "香盤表.md")).toBe(true);
    expect(effectiveAllow(rules, "香盤表.md.bak")).toBe(false);
    expect(effectiveAllow(rules, "sub/香盤表.md")).toBe(false);
  });

  it("file scope beats directory scope at same length, regardless of order", () => {
    const a = [rule("directory", "a/b.txt", true), rule("file", "a/b.txt", false)];
    const b = [rule("file", "a/b.txt", false), rule("directory", "a/b.txt", true)];
    expect(effectiveAllow(a, "a/b.txt")).toBe(false);
    expect(effectiveAllow(b, "a/b.txt")).toBe(false);
  });

  it("paths containing .. segments are always denied", () => {
    const rules = [rule("directory", "", true)];
    expect(effectiveAllow(rules, "図面/../契約/x.pdf")).toBe(false);
    expect(effectiveAllow(rules, "..")).toBe(false);
  });
});

describe("planToggle", () => {
  it("sets an explicit allow rule when currently denied by default", () => {
    expect(planToggle([], "directory", "図面")).toEqual({ action: "set", allow: true });
  });

  it("deletes own rule when toggling back to the inherited state", () => {
    const rules = [rule("directory", "図面", true)];
    // 図面 は自ルールで true。トグルで false にしたいが、親（ルール無し）の継承も false → 自ルール削除でよい
    expect(planToggle(rules, "directory", "図面")).toEqual({ action: "delete" });
  });

  it("sets an explicit deny when parent allows", () => {
    const rules = [rule("directory", "", true)];
    // 予算メモ.md は親から true を継承。トグルで false → 明示 deny が必要
    expect(planToggle(rules, "file", "予算メモ.md")).toEqual({ action: "set", allow: false });
  });

  it("deletes explicit deny when toggling back on under an allowing parent", () => {
    const rules = [rule("directory", "", true), rule("file", "予算メモ.md", false)];
    // 現在 false → トグルで true。親の継承が true なので自ルール削除でよい
    expect(planToggle(rules, "file", "予算メモ.md")).toEqual({ action: "delete" });
  });
});
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm test cloudPolicy`
Expected: FAIL（モジュール未定義）

- [ ] **Step 3: 実装**

`src/utils/cloudPolicy.ts`:

```typescript
import type { CloudRule } from "../types/directory";

export type ToggleAction = { action: "set"; allow: boolean } | { action: "delete" };

function hasDotDotSegment(path: string): boolean {
  return path.split("/").some((seg) => seg === "..");
}

function ruleMatches(rule: CloudRule, relativePath: string): boolean {
  if (rule.scope === "file") {
    return rule.relative_path === relativePath;
  }
  return (
    rule.relative_path === "" ||
    relativePath === rule.relative_path ||
    relativePath.startsWith(`${rule.relative_path}/`)
  );
}

/**
 * Rust 側 cloud_policy::is_cloud_allowed と同一セマンティクスの表示用判定。
 * マッチするルールが無ければ常に false（危険側に倒れない）。
 * 最長 relative_path のルールが勝ち、同長なら file スコープが勝つ。
 */
export function effectiveAllow(rules: CloudRule[], relativePath: string): boolean {
  if (hasDotDotSegment(relativePath)) return false;
  let best: CloudRule | null = null;
  for (const rule of rules) {
    if (!ruleMatches(rule, relativePath)) continue;
    if (
      best === null ||
      rule.relative_path.length > best.relative_path.length ||
      (rule.relative_path.length === best.relative_path.length &&
        rule.scope === "file" &&
        best.scope !== "file")
    ) {
      best = rule;
    }
  }
  return best?.allow ?? false;
}

/**
 * チェックボックス切替時のルール操作を決める。
 * 望む状態が「自ルールを消したときの継承状態」と同じなら delete（ルールを増やさない）、
 * 違うなら明示ルールを set する。
 */
export function planToggle(
  rules: CloudRule[],
  scope: "directory" | "file",
  relativePath: string,
): ToggleAction {
  const desired = !effectiveAllow(rules, relativePath);
  const withoutOwn = rules.filter(
    (r) => !(r.scope === scope && r.relative_path === relativePath),
  );
  const inherited = effectiveAllow(withoutOwn, relativePath);
  if (desired === inherited) {
    return { action: "delete" };
  }
  return { action: "set", allow: desired };
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `pnpm test cloudPolicy`
Expected: 11件 PASS

- [ ] **Step 5: コミット**

```bash
git add src/utils/cloudPolicy.ts src/__tests__/utils/cloudPolicy.test.ts
git commit -m "feat(ui): クラウド送信判定のフロント用ユーティリティを追加"
```

---

### Task 3: projectStore の拡張

**Files:**
- Modify: `src/stores/projectStore.ts`
- Test: `src/__tests__/stores/projectStore.test.ts`（追記 + 既存テストの beforeEach 修正）

**Interfaces:**
- Consumes: バックエンドコマンド `get_project_directory` / `link_project_directory` / `unlink_project_directory` / `rescan_project_directory` / `get_project_context` / `set_allow_cloud_context`
- Produces（コンポーネントが使う状態とアクション）:
  - `directories: Record<string, ProjectDirectory | null>`（projectId → 紐付け。未取得は undefined、未紐付けは null）
  - `contexts: Record<string, ProjectContext | null>`
  - `scanningProjects: Record<string, boolean>`
  - `fetchDirectory(projectId)` / `linkDirectory(projectId, path)` / `unlinkDirectory(projectId)` / `rescanProject(projectId)` / `fetchProjectContext(projectId)` / `setAllowCloudContext(projectId, allow)`
  - `fetchProjects` は成功後に各案件の `fetchDirectory` を発火する

- [ ] **Step 1: 失敗するテストを書く**

`src/__tests__/stores/projectStore.test.ts` に追記（describe ブロックを末尾に追加）。あわせて既存の `beforeEach` の `setState` に新フィールドの初期化を追加する:

```typescript
    useProjectStore.setState({
      projects: [],
      selectedProjectId: null,
      loading: false,
      error: null,
      directories: {},
      contexts: {},
      scanningProjects: {},
    });
```

追加テスト:

```typescript
describe("directory linkage", () => {
  const dir = {
    id: "d1",
    project_id: "p1",
    path: "/tmp/stage-a",
    is_primary: true,
    status: "ok",
    last_scanned_at: null,
    created_at: "",
  };

  it("fetchDirectory stores the linked directory", async () => {
    mockInvoke.mockResolvedValue(dir);
    await useProjectStore.getState().fetchDirectory("p1");
    expect(mockInvoke).toHaveBeenCalledWith("get_project_directory", { projectId: "p1" });
    expect(useProjectStore.getState().directories["p1"]).toEqual(dir);
  });

  it("fetchDirectory stores null when unlinked", async () => {
    mockInvoke.mockResolvedValue(null);
    await useProjectStore.getState().fetchDirectory("p1");
    expect(useProjectStore.getState().directories["p1"]).toBeNull();
  });

  it("linkDirectory invokes command and refreshes directory", async () => {
    mockInvoke.mockResolvedValue(dir);
    await useProjectStore.getState().linkDirectory("p1", "/tmp/stage-a");
    expect(mockInvoke).toHaveBeenCalledWith("link_project_directory", {
      projectId: "p1",
      path: "/tmp/stage-a",
    });
    expect(useProjectStore.getState().directories["p1"]).toEqual(dir);
  });

  it("unlinkDirectory clears the entry", async () => {
    useProjectStore.setState({ directories: { p1: dir } });
    mockInvoke.mockResolvedValue(undefined);
    await useProjectStore.getState().unlinkDirectory("p1");
    expect(mockInvoke).toHaveBeenCalledWith("unlink_project_directory", { projectId: "p1" });
    expect(useProjectStore.getState().directories["p1"]).toBeNull();
  });

  it("rescanProject toggles scanning flag and refreshes state", async () => {
    let resolveRescan: (v: unknown) => void = () => {};
    mockInvoke.mockImplementation((cmd: unknown) => {
      if (cmd === "rescan_project_directory") {
        return new Promise((resolve) => { resolveRescan = resolve; });
      }
      return Promise.resolve(null);
    });

    const promise = useProjectStore.getState().rescanProject("p1");
    expect(useProjectStore.getState().scanningProjects["p1"]).toBe(true);

    resolveRescan({ status: "ok", regenerated: true, file_count: 3 });
    await promise;
    expect(useProjectStore.getState().scanningProjects["p1"]).toBeUndefined();
    expect(mockInvoke).toHaveBeenCalledWith("rescan_project_directory", { projectId: "p1" });
  });

  it("setAllowCloudContext invokes command and refreshes context", async () => {
    mockInvoke.mockResolvedValue(null);
    await useProjectStore.getState().setAllowCloudContext("p1", true);
    expect(mockInvoke).toHaveBeenCalledWith("set_allow_cloud_context", {
      projectId: "p1",
      allow: true,
    });
  });
});
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm test projectStore`
Expected: FAIL（新アクション未定義の型エラー）

- [ ] **Step 3: 実装**

`src/stores/projectStore.ts` を拡張:

インポートに追加:

```typescript
import type { ProjectContext, ProjectDirectory, RescanOutcome } from "../types/directory";
```

`ProjectState` interface に追加:

```typescript
  directories: Record<string, ProjectDirectory | null>;
  contexts: Record<string, ProjectContext | null>;
  scanningProjects: Record<string, boolean>;
  fetchDirectory: (projectId: string) => Promise<void>;
  linkDirectory: (projectId: string, path: string) => Promise<void>;
  unlinkDirectory: (projectId: string) => Promise<void>;
  rescanProject: (projectId: string) => Promise<void>;
  fetchProjectContext: (projectId: string) => Promise<void>;
  setAllowCloudContext: (projectId: string, allow: boolean) => Promise<void>;
```

ストア本体に初期値とアクションを追加:

```typescript
  directories: {},
  contexts: {},
  scanningProjects: {},

  fetchDirectory: async (projectId) => {
    try {
      const dir = await invoke<ProjectDirectory | null>("get_project_directory", {
        projectId,
      });
      set({ directories: { ...get().directories, [projectId]: dir } });
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  },

  linkDirectory: async (projectId, path) => {
    try {
      const dir = await invoke<ProjectDirectory>("link_project_directory", {
        projectId,
        path,
      });
      set({ directories: { ...get().directories, [projectId]: dir } });
    } catch (e) {
      useErrorStore.getState().addError(String(e));
      throw e;
    }
  },

  unlinkDirectory: async (projectId) => {
    try {
      await invoke("unlink_project_directory", { projectId });
      set({ directories: { ...get().directories, [projectId]: null } });
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  },

  rescanProject: async (projectId) => {
    set({ scanningProjects: { ...get().scanningProjects, [projectId]: true } });
    try {
      await invoke<RescanOutcome>("rescan_project_directory", { projectId });
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    } finally {
      const { [projectId]: _removed, ...rest } = get().scanningProjects;
      set({ scanningProjects: rest });
      void get().fetchDirectory(projectId);
      void get().fetchProjectContext(projectId);
    }
  },

  fetchProjectContext: async (projectId) => {
    try {
      const context = await invoke<ProjectContext | null>("get_project_context", {
        projectId,
      });
      set({ contexts: { ...get().contexts, [projectId]: context } });
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  },

  setAllowCloudContext: async (projectId, allow) => {
    try {
      await invoke("set_allow_cloud_context", { projectId, allow });
      await get().fetchProjectContext(projectId);
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  },
```

`fetchProjects` の成功パスに1行追加（`set({ projects, loading: false });` の直後）:

```typescript
      for (const p of projects) {
        void get().fetchDirectory(p.id);
      }
```

注意: これにより既存の `fetchProjects` テストでも `get_project_directory` の invoke が発生する。既存テストの `mockInvoke.mockResolvedValue(projects)` はそのままでもアサーション（`toHaveBeenCalledWith("get_projects", ...)`）は通るが、`directories` に不正な値が入らないよう、既存テストが壊れた場合は `mockInvoke.mockImplementation` でコマンド名により返し分ける形に更新してよい（アサーションの意図は変えない）。

- [ ] **Step 4: テストが通ることを確認**

Run: `pnpm test projectStore`
Expected: 既存 + 新規6件すべて PASS

- [ ] **Step 5: コミット**

```bash
git add src/stores/projectStore.ts src/__tests__/stores/projectStore.test.ts
git commit -m "feat(ui): projectStoreにディレクトリ連携の状態とアクションを追加"
```

---

### Task 4: サイドバー表示（📁/⚠アイコン・右クリックメニュー・スキャン表示）

**Files:**
- Modify: `src/components/sidebar/ProjectListItem.tsx`
- Modify: `src/components/sidebar/ProjectTree.tsx`
- Modify: `src/components/sidebar/Sidebar.tsx`（スキャン中インジケータ）
- Test: `src/__tests__/ProjectListItem.test.tsx`（新規）

**Interfaces:**
- Consumes: Task 3 の `directories` / `scanningProjects` / `rescanProject` / `unlinkDirectory` / `linkDirectory`、`open`（@tauri-apps/plugin-dialog）
- Produces: `ProjectListItem` の新 props `directory?: ProjectDirectory | null` / `scanning?: boolean`、ProjectTree の `onOpenCloudSettings(projectId)`（Task 6 のダイアログを開く。Task 4 時点では ProjectListInner 内の state `cloudSettingsProjectId` として持ち、ダイアログ本体は Task 6 で接続）

- [ ] **Step 1: 失敗するテストを書く**

`src/__tests__/ProjectListItem.test.tsx`:

```typescript
import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ProjectListItem } from "../components/sidebar/ProjectListItem";
import { ProjectRenameProvider } from "../components/sidebar/ProjectRenameContext";
import type { Project } from "../types/project";
import type { ProjectDirectory } from "../types/directory";

const project: Project = {
  id: "p1",
  account_id: "acc1",
  name: "春公演",
  description: null,
  color: "#3E617F",
  is_archived: false,
  created_at: "",
  updated_at: "",
};

function renderItem(directory?: ProjectDirectory | null, scanning?: boolean) {
  return render(
    <ProjectRenameProvider projects={[project]}>
      <ul>
        <ProjectListItem
          project={project}
          selected={false}
          onSelect={vi.fn()}
          onContextMenu={vi.fn()}
          onDrop={vi.fn()}
          directory={directory}
          scanning={scanning}
        />
      </ul>
    </ProjectRenameProvider>,
  );
}

describe("ProjectListItem directory indicators", () => {
  it("shows no folder icon when unlinked", () => {
    renderItem(null);
    expect(screen.queryByTitle(/\/tmp/)).not.toBeInTheDocument();
    expect(screen.queryByText("📁")).not.toBeInTheDocument();
  });

  it("shows 📁 when linked and status is ok", () => {
    renderItem({
      id: "d1", project_id: "p1", path: "/tmp/stage-a", is_primary: true,
      status: "ok", last_scanned_at: null, created_at: "",
    });
    expect(screen.getByTitle("/tmp/stage-a")).toHaveTextContent("📁");
  });

  it("shows warning icon when directory is missing", () => {
    renderItem({
      id: "d1", project_id: "p1", path: "/tmp/gone", is_primary: true,
      status: "missing", last_scanned_at: null, created_at: "",
    });
    const badge = screen.getByTitle(/フォルダにアクセスできません/);
    expect(badge).toHaveTextContent("⚠");
  });

  it("shows scanning indicator while rescanning", () => {
    renderItem(
      {
        id: "d1", project_id: "p1", path: "/tmp/stage-a", is_primary: true,
        status: "ok", last_scanned_at: null, created_at: "",
      },
      true,
    );
    expect(screen.getByTitle("スキャン中")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm test ProjectListItem`
Expected: FAIL（props 未定義）

- [ ] **Step 3: 実装**

`ProjectListItem.tsx`: props を拡張し、名前の右にバッジを描画。

```typescript
import type { ProjectDirectory } from "../../types/directory";

interface ProjectListItemProps {
  project: Project;
  selected: boolean;
  onSelect: () => void;
  onContextMenu: (e: React.MouseEvent) => void;
  onDrop: (projectId: string) => void;
  directory?: ProjectDirectory | null;
  scanning?: boolean;
}
```

`<span className="truncate">{project.name}</span>` の直後に追加:

```tsx
          {scanning ? (
            <span className="ml-auto flex-shrink-0 animate-pulse text-xs" title="スキャン中">
              ⏳
            </span>
          ) : directory ? (
            directory.status === "ok" ? (
              <span className="ml-auto flex-shrink-0 text-xs" title={directory.path}>
                📁
              </span>
            ) : (
              <span
                className="ml-auto flex-shrink-0 text-xs text-amber-500"
                title={`フォルダにアクセスできません（${directory.status}）: ${directory.path}`}
              >
                ⚠📁
              </span>
            )
          ) : null}
```

`ProjectTree.tsx` の `ProjectListInner`:

1. ストアから追加取得: `const { directories, scanningProjects, rescanProject, unlinkDirectory, linkDirectory } = useProjectStore();`（既存の分割代入に追加）
2. フォルダ選択ハンドラ:

```typescript
import { open } from "@tauri-apps/plugin-dialog";

  const handleLinkDirectory = async (projectId: string) => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "案件フォルダを選択",
    });
    if (typeof selected === "string") {
      await linkDirectory(projectId, selected);
      void rescanProject(projectId); // 紐付け直後に初回スキャン
    }
  };
```

3. `getProjectMenuItems` を差し替え:

```typescript
  const getProjectMenuItems = (projectId: string) => {
    const directory = directories[projectId] ?? null;
    return [
      {
        label: directory ? "フォルダを変更…" : "フォルダを紐付け…",
        onClick: () => void handleLinkDirectory(projectId),
      },
      ...(directory
        ? [
            { label: "再スキャン", onClick: () => void rescanProject(projectId) },
            {
              label: "クラウド送信設定…",
              onClick: () => setCloudSettingsProjectId(projectId),
            },
            { label: "紐付け解除", onClick: () => void unlinkDirectory(projectId) },
          ]
        : []),
      { label: "名前変更", onClick: () => startRename(projectId) },
      { label: "マージ", onClick: () => setMergeSourceId(projectId) },
      { label: "アーカイブ", onClick: async () => { await archiveProject(projectId); } },
      { label: "削除", danger: true, onClick: async () => { await deleteProject(projectId); } },
    ];
  };
```

4. state 追加（Task 6 でダイアログに接続。本タスクでは state のみ）:

```typescript
  const [cloudSettingsProjectId, setCloudSettingsProjectId] = useState<string | null>(null);
  void cloudSettingsProjectId; // Task 6 で CloudSettingsDialog に接続する（未使用警告の一時抑止）
```

5. `ProjectListItem` へ props を渡す:

```tsx
            directory={directories[project.id] ?? null}
            scanning={!!scanningProjects[project.id]}
```

`Sidebar.tsx`: `<aside>` の末尾（案件作成ボタンブロックの後）にスキャン中インジケータを追加:

```tsx
      <ScanIndicator />
```

同ファイル内ではなく `src/components/sidebar/ScanIndicator.tsx` を新規作成（1ファイル1コンポーネント規約）:

```tsx
import { useProjectStore } from "../../stores/projectStore";

export function ScanIndicator() {
  const scanningProjects = useProjectStore((s) => s.scanningProjects);
  const projects = useProjectStore((s) => s.projects);

  const scanningNames = projects
    .filter((p) => scanningProjects[p.id])
    .map((p) => p.name);

  if (scanningNames.length === 0) return null;

  return (
    <div className="border-t px-4 py-1.5 text-xs text-gray-500">
      スキャン中… {scanningNames.join(", ")}
    </div>
  );
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `pnpm test`
Expected: 新規4件を含め全 PASS（ProjectTree の既存テスト ContextMenu.test 等が壊れていないこと）

- [ ] **Step 5: コミット**

```bash
git add src/components/sidebar/ProjectListItem.tsx src/components/sidebar/ProjectTree.tsx \
        src/components/sidebar/Sidebar.tsx src/components/sidebar/ScanIndicator.tsx \
        src/__tests__/ProjectListItem.test.tsx
git commit -m "feat(ui): 案件ツリーにフォルダ紐付けメニューと状態表示を追加"
```

---

### Task 5: 案件作成フォームのフォルダ選択

**Files:**
- Modify: `src/components/sidebar/ProjectForm.tsx`
- Modify: `src/components/sidebar/Sidebar.tsx`（handleProjectSubmit の配線）
- Test: `src/__tests__/ProjectForm.test.tsx`（追記）

**Interfaces:**
- Consumes: `open`（@tauri-apps/plugin-dialog）、Task 3 の `linkDirectory` / `rescanProject`、`createProject`（既存、Project を返す）
- Produces: `ProjectFormProps.onSubmit` のシグネチャ変更: `(name: string, description?: string, color?: string, directoryPath?: string) => void`

- [ ] **Step 1: 失敗するテストを書く**

`src/__tests__/ProjectForm.test.tsx` に追記（既存の describe 内、既存テストの形式に合わせる）:

```typescript
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));
import { open } from "@tauri-apps/plugin-dialog";

  it("picks a folder and passes it to onSubmit", async () => {
    vi.mocked(open).mockResolvedValue("/tmp/stage-a");
    const onSubmit = vi.fn();
    render(<ProjectForm onSubmit={onSubmit} onCancel={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: /フォルダを選択/ }));
    await screen.findByText("/tmp/stage-a"); // 選択済みパスの表示を待つ

    fireEvent.change(screen.getByPlaceholderText("案件名を入力"), {
      target: { value: "春公演" },
    });
    fireEvent.submit(screen.getByRole("button", { name: "作成" }).closest("form")!);

    expect(onSubmit).toHaveBeenCalledWith("春公演", undefined, "#6b7280", "/tmp/stage-a");
  });

  it("submits without folder when none picked", () => {
    const onSubmit = vi.fn();
    render(<ProjectForm onSubmit={onSubmit} onCancel={vi.fn()} />);
    fireEvent.change(screen.getByPlaceholderText("案件名を入力"), {
      target: { value: "春公演" },
    });
    fireEvent.submit(screen.getByRole("button", { name: "作成" }).closest("form")!);
    expect(onSubmit).toHaveBeenCalledWith("春公演", undefined, "#6b7280", undefined);
  });
```

注意: 既存テストが `onSubmit` の引数を厳密に検証している場合（3引数）、第4引数 `undefined` を追加して更新する。

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm test ProjectForm`
Expected: FAIL

- [ ] **Step 3: 実装**

`ProjectForm.tsx`:

```typescript
import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

interface ProjectFormProps {
  onSubmit: (
    name: string,
    description?: string,
    color?: string,
    directoryPath?: string,
  ) => void;
  onCancel: () => void;
}
```

state に `const [directoryPath, setDirectoryPath] = useState<string | null>(null);` を追加。`handleSubmit` を:

```typescript
    onSubmit(name.trim(), description.trim() || undefined, color, directoryPath ?? undefined);
```

色の入力ブロックの後・ボタン行の前に追加:

```tsx
      <div className="mb-3">
        <label className="mb-1 block text-xs font-medium text-gray-600">
          案件フォルダ（任意）
        </label>
        <button
          type="button"
          onClick={async () => {
            const selected = await open({
              directory: true,
              multiple: false,
              title: "案件フォルダを選択",
            });
            if (typeof selected === "string") setDirectoryPath(selected);
          }}
          className="rounded border border-gray-300 px-2 py-1 text-sm text-gray-600 hover:bg-gray-100"
        >
          📁 フォルダを選択
        </button>
        {directoryPath && (
          <p className="mt-1 truncate text-xs text-gray-500" title={directoryPath}>
            {directoryPath}
          </p>
        )}
      </div>
```

`Sidebar.tsx` の `handleProjectSubmit`:

```typescript
  const { createProject, linkDirectory, rescanProject } = useProjectStore();

  const handleProjectSubmit = async (
    name: string,
    description?: string,
    color?: string,
    directoryPath?: string,
  ) => {
    if (!selectedAccountId) return;
    const project = await createProject(selectedAccountId, name, description, color);
    if (directoryPath) {
      await linkDirectory(project.id, directoryPath);
      void rescanProject(project.id);
    }
    setShowProjectForm(false);
  };
```

- [ ] **Step 4: テストが通ることを確認**

Run: `pnpm test`
Expected: 全 PASS

- [ ] **Step 5: コミット**

```bash
git add src/components/sidebar/ProjectForm.tsx src/components/sidebar/Sidebar.tsx \
        src/__tests__/ProjectForm.test.tsx
git commit -m "feat(ui): 案件作成フォームにフォルダ選択を追加"
```

---

### Task 6: クラウド送信設定ダイアログ + 最終確認

**Files:**
- Create: `src/components/sidebar/CloudSettingsDialog.tsx`
- Modify: `src/components/sidebar/ProjectTree.tsx`（ダイアログ接続）
- Test: `src/__tests__/CloudSettingsDialog.test.tsx`

**Interfaces:**
- Consumes: コマンド `list_project_files` / `get_cloud_rules` / `set_cloud_rule`（`allow: null` でルール削除）/ `get_project_context` / `set_allow_cloud_context`、Task 2 の `effectiveAllow` / `planToggle`
- Produces: `CloudSettingsDialog` props: `{ project: Project; directory: ProjectDirectory; onClose: () => void }`

- [ ] **Step 1: 失敗するテストを書く**

`src/__tests__/CloudSettingsDialog.test.tsx`:

```typescript
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { CloudSettingsDialog } from "../components/sidebar/CloudSettingsDialog";
import type { Project } from "../types/project";
import type { ProjectDirectory } from "../types/directory";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

const project: Project = {
  id: "p1", account_id: "acc1", name: "春公演", description: null,
  color: null, is_archived: false, created_at: "", updated_at: "",
};
const directory: ProjectDirectory = {
  id: "d1", project_id: "p1", path: "/tmp/stage-a", is_primary: true,
  status: "ok", last_scanned_at: null, created_at: "",
};

const files = [
  { id: "f1", directory_id: "d1", relative_path: "図面/平面図.pdf", size_bytes: 100, mtime: "", content_hash: null, content_kind: "pdf", extract_status: "unsupported", indexed_at: "" },
  { id: "f2", directory_id: "d1", relative_path: "香盤表.md", size_bytes: 50, mtime: "", content_hash: "h", content_kind: "text", extract_status: "ok", indexed_at: "" },
];

function setupInvoke(rules: unknown[] = [], context: unknown = null) {
  mockInvoke.mockImplementation((cmd: unknown) => {
    switch (cmd) {
      case "list_project_files": return Promise.resolve(files);
      case "get_cloud_rules": return Promise.resolve(rules);
      case "get_project_context": return Promise.resolve(context);
      default: return Promise.resolve(null);
    }
  });
}

describe("CloudSettingsDialog", () => {
  beforeEach(() => vi.clearAllMocks());

  it("renders file tree with all checkboxes off by default (deny by default)", async () => {
    setupInvoke();
    render(<CloudSettingsDialog project={project} directory={directory} onClose={vi.fn()} />);

    await screen.findByText(/香盤表\.md/); // ノードは「📄 香盤表.md」なので部分一致
    expect(screen.getByText(/平面図\.pdf/)).toBeInTheDocument();
    const checkboxes = screen.getAllByRole("checkbox");
    // 案件単位トグル + フォルダ「図面」 + ファイル2件
    for (const cb of checkboxes) {
      expect(cb).not.toBeChecked();
    }
  });

  it("checking a file sets an explicit allow rule", async () => {
    setupInvoke();
    render(<CloudSettingsDialog project={project} directory={directory} onClose={vi.fn()} />);
    const fileRow = await screen.findByText(/香盤表\.md/);
    const checkbox = fileRow.closest("li")!.querySelector("input[type=checkbox]")!;

    fireEvent.click(checkbox);

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("set_cloud_rule", {
        directoryId: "d1",
        scope: "file",
        relativePath: "香盤表.md",
        allow: true,
      });
    });
  });

  it("shows checked state derived from existing rules (directory rule cascades)", async () => {
    setupInvoke([
      { id: "r1", directory_id: "d1", scope: "directory", relative_path: "図面", allow: true },
    ]);
    render(<CloudSettingsDialog project={project} directory={directory} onClose={vi.fn()} />);
    const pdfRow = await screen.findByText(/平面図\.pdf/);
    const checkbox = pdfRow.closest("li")!.querySelector("input[type=checkbox]")!;
    expect(checkbox).toBeChecked();
  });

  it("toggles allow_cloud_context and shows context preview", async () => {
    setupInvoke([], {
      project_id: "p1", cached_context: "会場: 〇〇ホール", context_hash: null,
      inventory_hash: null, allow_cloud_context: false, generated_at: null,
    });
    render(<CloudSettingsDialog project={project} directory={directory} onClose={vi.fn()} />);

    expect(await screen.findByText(/会場: 〇〇ホール/)).toBeInTheDocument();
    const toggle = screen.getByLabelText(/コンテキストファイルをクラウドLLMへ送信/);
    fireEvent.click(toggle);
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("set_allow_cloud_context", {
        projectId: "p1",
        allow: true,
      });
    });
  });

  it("shows the local-LLM notice", async () => {
    setupInvoke();
    render(<CloudSettingsDialog project={project} directory={directory} onClose={vi.fn()} />);
    expect(await screen.findByText(/ローカルLLM/)).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm test CloudSettingsDialog`
Expected: FAIL（コンポーネント未定義）

- [ ] **Step 3: 実装**

`src/components/sidebar/CloudSettingsDialog.tsx`:

```tsx
import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Project } from "../../types/project";
import type {
  CloudRule,
  ProjectContext,
  ProjectDirectory,
  ProjectFile,
} from "../../types/directory";
import { effectiveAllow, planToggle } from "../../utils/cloudPolicy";
import { useErrorStore } from "../../stores/errorStore";

interface CloudSettingsDialogProps {
  project: Project;
  directory: ProjectDirectory;
  onClose: () => void;
}

interface TreeNode {
  name: string;
  path: string; // ディレクトリからの相対パス
  isDir: boolean;
  children: TreeNode[];
}

/** relative_path のリストからツリーを構築する（ディレクトリ優先・名前順） */
function buildTree(files: ProjectFile[]): TreeNode[] {
  const root: TreeNode = { name: "", path: "", isDir: true, children: [] };
  for (const file of files) {
    const segments = file.relative_path.split("/");
    let node = root;
    let pathSoFar = "";
    segments.forEach((segment, i) => {
      pathSoFar = pathSoFar ? `${pathSoFar}/${segment}` : segment;
      const isDir = i < segments.length - 1;
      let child = node.children.find((c) => c.name === segment && c.isDir === isDir);
      if (!child) {
        child = { name: segment, path: pathSoFar, isDir, children: [] };
        node.children.push(child);
      }
      node = child;
    });
  }
  const sortRec = (n: TreeNode) => {
    n.children.sort((a, b) =>
      a.isDir === b.isDir ? a.name.localeCompare(b.name, "ja") : a.isDir ? -1 : 1,
    );
    n.children.forEach(sortRec);
  };
  sortRec(root);
  return root.children;
}

export function CloudSettingsDialog({
  project,
  directory,
  onClose,
}: CloudSettingsDialogProps) {
  const [files, setFiles] = useState<ProjectFile[]>([]);
  const [rules, setRules] = useState<CloudRule[]>([]);
  const [context, setContext] = useState<ProjectContext | null>(null);
  const [loading, setLoading] = useState(true);

  const reload = useCallback(async () => {
    try {
      const [filesRes, rulesRes, contextRes] = await Promise.all([
        invoke<ProjectFile[]>("list_project_files", { directoryId: directory.id }),
        invoke<CloudRule[]>("get_cloud_rules", { directoryId: directory.id }),
        invoke<ProjectContext | null>("get_project_context", { projectId: project.id }),
      ]);
      setFiles(filesRes);
      setRules(rulesRes);
      setContext(contextRes);
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    } finally {
      setLoading(false);
    }
  }, [directory.id, project.id]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const tree = useMemo(() => buildTree(files), [files]);

  const handleToggleNode = async (node: TreeNode) => {
    const scope = node.isDir ? "directory" : "file";
    const action = planToggle(rules, scope, node.path);
    try {
      await invoke("set_cloud_rule", {
        directoryId: directory.id,
        scope,
        relativePath: node.path,
        allow: action.action === "set" ? action.allow : null,
      });
      const rulesRes = await invoke<CloudRule[]>("get_cloud_rules", {
        directoryId: directory.id,
      });
      setRules(rulesRes);
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  };

  const handleToggleContext = async () => {
    const allow = !(context?.allow_cloud_context ?? false);
    try {
      await invoke("set_allow_cloud_context", { projectId: project.id, allow });
      const contextRes = await invoke<ProjectContext | null>("get_project_context", {
        projectId: project.id,
      });
      setContext(contextRes);
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  };

  const renderNode = (node: TreeNode, depth: number): React.ReactNode => (
    <li key={`${node.isDir ? "d" : "f"}:${node.path}`}>
      <div
        className="flex items-center gap-2 py-1"
        style={{ paddingLeft: `${depth * 20}px` }}
      >
        <input
          type="checkbox"
          checked={effectiveAllow(rules, node.path)}
          onChange={() => void handleToggleNode(node)}
          className="h-4 w-4"
        />
        <span className="text-sm">
          {node.isDir ? "📂" : "📄"} {node.name}
          {node.isDir && "/"}
        </span>
      </div>
      {node.children.length > 0 && (
        <ul>{node.children.map((c) => renderNode(c, depth + 1))}</ul>
      )}
    </li>
  );

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="flex max-h-[80vh] w-[560px] flex-col rounded-lg bg-white shadow-xl">
        <div className="border-b px-5 py-3">
          <h2 className="text-sm font-bold">クラウド送信設定 — {project.name}</h2>
          <p className="mt-0.5 text-xs text-gray-500">
            チェックしたものだけがクラウドLLMへの入力に使われます（デフォルトはすべて送信オフ）。
          </p>
        </div>
        <div className="flex-1 overflow-y-auto px-5 py-3">
          <p className="mb-3 rounded bg-blue-50 px-3 py-2 text-xs text-blue-700">
            現在ローカルLLM（Ollama）使用中のため、データは外部に送信されません。
            この設定は保存され、クラウドLLM導入時に適用されます。
          </p>

          <label className="mb-1 flex items-start gap-2 rounded border border-gray-200 bg-gray-50 px-3 py-2">
            <input
              type="checkbox"
              checked={context?.allow_cloud_context ?? false}
              onChange={() => void handleToggleContext()}
              className="mt-0.5 h-4 w-4"
              aria-label="コンテキストファイルをクラウドLLMへ送信する"
            />
            <span className="text-sm">
              コンテキストファイル（PIGEON-CONTEXT.md）をクラウドLLMへ送信する
              <span className="block text-xs text-gray-500">
                分類のたびに以下の内容がプロンプトへ入ります。内容を確認してからONにしてください。
              </span>
            </span>
          </label>
          <pre className="mb-4 max-h-32 overflow-y-auto whitespace-pre-wrap rounded border border-gray-200 bg-gray-50 px-3 py-2 text-xs text-gray-600">
            {context?.cached_context ?? "（コンテキスト未生成。再スキャンで生成されます）"}
          </pre>

          <div className="mb-1 text-xs font-semibold uppercase tracking-wide text-gray-400">
            ファイルごとの送信許可
          </div>
          {loading ? (
            <p className="py-4 text-center text-sm text-gray-400">読み込み中…</p>
          ) : files.length === 0 ? (
            <p className="py-4 text-center text-sm text-gray-400">
              ファイルがありません。再スキャンしてください。
            </p>
          ) : (
            <ul>{tree.map((n) => renderNode(n, 0))}</ul>
          )}
        </div>
        <div className="flex justify-end border-t px-5 py-3">
          <button
            onClick={onClose}
            className="rounded bg-blue-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-blue-700"
          >
            閉じる
          </button>
        </div>
      </div>
    </div>
  );
}
```

`ProjectTree.tsx` の `ProjectListInner` 末尾（MergeProjectDialog の後）にダイアログを接続し、Task 4 で入れた一時的な `void cloudSettingsProjectId;` 行を削除:

```tsx
      {cloudSettingsProjectId && (() => {
        const targetProject = projects.find((p) => p.id === cloudSettingsProjectId);
        const targetDirectory = directories[cloudSettingsProjectId];
        if (!targetProject || !targetDirectory) return null;
        return (
          <CloudSettingsDialog
            project={targetProject}
            directory={targetDirectory}
            onClose={() => setCloudSettingsProjectId(null)}
          />
        );
      })()}
```

インポート: `import { CloudSettingsDialog } from "./CloudSettingsDialog";`

- [ ] **Step 4: テストが通ることを確認**

Run: `pnpm test`
Expected: 新規5件を含め全 PASS

- [ ] **Step 5: 最終確認とコミット**

Run: `pnpm test && pnpm build && cd src-tauri && cargo test 2>&1 | tail -3`
Expected: フロントテスト全パス / tsc+vite ビルド成功 / Rust 252 passed

```bash
git add src/components/sidebar/CloudSettingsDialog.tsx src/components/sidebar/ProjectTree.tsx \
        src/__tests__/CloudSettingsDialog.test.tsx
git commit -m "feat(ui): クラウド送信設定ダイアログを追加"
```
