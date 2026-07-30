# 文件树原位重命名实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将右侧文件树的重命名从原生弹窗替换为支持 Enter 提交和 Escape 取消的原位编辑输入框。

**Architecture:** 在 `FileTree` 维护当前编辑节点路径与输入值，并将其作为受控属性传入递归 `FileTreeNode`。节点在路径匹配时渲染具有侧栏主题样式的输入框；键盘事件交给父组件的提交与取消回调。提取名称归一化逻辑为纯函数，使空名、未变更名称和有效名称的规则可由 Vitest 直接验证。

**Tech Stack:** React 18、TypeScript、Tailwind CSS、Vitest、Tauri window API。

---

## 文件结构

- Create: `src/utils/inlineRename.ts` — 归一化编辑值，区分提交与取消情况。
- Create: `src/__tests__/inlineRename.test.ts` — 覆盖归一化与提交判定规则。
- Modify: `src/components/layout/FileTree.tsx:28,160-172,458-472,858-866,882-959` — 管理编辑状态、提交重命名，并将输入框接入树节点。

### Task 1: 编写原位重命名的纯逻辑测试

**Files:**
- Create: `src/__tests__/inlineRename.test.ts`
- Create: `src/utils/inlineRename.ts`

- [ ] **Step 1: 写入失败测试，定义提交值判定规则**

```ts
import { describe, expect, it } from "vitest";
import { resolveInlineRename } from "../utils/inlineRename";

describe("resolveInlineRename", () => {
  it("trims and returns a changed non-empty name", () => {
    expect(resolveInlineRename("  renamed.ts  ", "old.ts")).toEqual({
      shouldRename: true,
      name: "renamed.ts",
    });
  });

  it.each(["", "   ", " old.ts "])("does not submit %j", (value) => {
    expect(resolveInlineRename(value, "old.ts")).toEqual({ shouldRename: false });
  });
});
```

- [ ] **Step 2: 运行测试，确认其因模块尚不存在而失败**

Run: `node scripts/run-package-bin.mjs vitest run src/__tests__/inlineRename.test.ts`

Expected: FAIL，提示无法解析 `../utils/inlineRename`。

- [ ] **Step 3: 实现最小的名称归一化函数**

```ts
export type InlineRenameResolution =
  | { shouldRename: false }
  | { shouldRename: true; name: string };

export function resolveInlineRename(value: string, originalName: string): InlineRenameResolution {
  const name = value.trim();
  if (!name || name === originalName) {
    return { shouldRename: false };
  }
  return { shouldRename: true, name };
}
```

- [ ] **Step 4: 运行单测，确认归一化规则通过**

Run: `node scripts/run-package-bin.mjs vitest run src/__tests__/inlineRename.test.ts`

Expected: PASS，3 个断言场景全部通过。

- [ ] **Step 5: 提交纯逻辑与单测**

```powershell
git add src/utils/inlineRename.ts src/__tests__/inlineRename.test.ts
git commit -m "test: cover inline rename submission rules"
```

### Task 2: 将文件树重命名改为原位编辑

**Files:**
- Modify: `src/components/layout/FileTree.tsx:28,160-172,458-472,858-866,882-959`
- Modify: `src/utils/inlineRename.ts`
- Test: `src/__tests__/inlineRename.test.ts`

- [ ] **Step 1: 在 `FileTree` 导入名称归一化函数并声明编辑状态**

在导入区添加：

```ts
import { resolveInlineRename } from "../../utils/inlineRename";
```

在其他 `useState` 声明旁添加：

```ts
const [renaming, setRenaming] = useState<{ path: string; value: string } | null>(null);
```

- [ ] **Step 2: 用启动、取消和提交回调替换 `window.prompt` 重命名回调**

```ts
const handleStartRename = useCallback((node: TreeNode) => {
  setRenaming({ path: node.path, value: node.name });
}, []);

const handleCancelRename = useCallback(() => {
  setRenaming(null);
}, []);

const handleSubmitRename = useCallback(async (node: TreeNode, value: string) => {
  const resolution = resolveInlineRename(value, node.name);
  setRenaming(null);
  if (!resolution.shouldRename) return;

  setActionError(null);
  try {
    await renamePathEntry(node.path, joinPath(getParentPath(node.path), resolution.name));
    await refreshTree();
  } catch (err) {
    setActionError(String(err));
  }
}, [refreshTree]);
```

将上下文菜单 `rename` 项的点击处理替换为：

```ts
onClick: () => handleStartRename(contextMenuNode),
```

- [ ] **Step 3: 传递编辑状态和键盘回调给每个递归节点**

在根节点与子节点的 `<FileTreeNode>` 调用中添加：

```tsx
renaming={renaming}
onRenameValueChange={(value) =>
  setRenaming((current) => current ? { ...current, value } : null)
}
onSubmitRename={handleSubmitRename}
onCancelRename={handleCancelRename}
```

扩展 `FileTreeNodeProps` 以包含：

```ts
renaming: { path: string; value: string } | null;
onRenameValueChange: (value: string) => void;
onSubmitRename: (node: TreeNode, value: string) => void;
onCancelRename: () => void;
```

- [ ] **Step 4: 在匹配节点渲染自动聚焦、全选的输入框**

在 `FileTreeNode` 中派生 `const isRenaming = renaming?.path === node.path;`，并替换名称 `<span>`：

```tsx
{isRenaming ? (
  <input
    autoFocus
    aria-label={`Rename ${node.name}`}
    className="min-w-0 flex-1 rounded border border-[var(--accent)] bg-[var(--surface-elevated)] px-1 py-0 text-[12px] leading-4 text-[var(--text-base)] outline-none"
    value={renaming.value}
    onChange={(event) => onRenameValueChange(event.target.value)}
    onFocus={(event) => event.currentTarget.select()}
    onClick={(event) => event.stopPropagation()}
    onKeyDown={(event) => {
      event.stopPropagation();
      if (event.key === "Enter") {
        event.preventDefault();
        onSubmitRename(node, renaming.value);
      } else if (event.key === "Escape") {
        event.preventDefault();
        onCancelRename();
      }
    }}
  />
) : (
  <span className="min-w-0 truncate text-[var(--text-base)]">{node.name}</span>
)}
```

保留容器的点击行为；输入框通过 `stopPropagation` 防止 Enter 编辑时意外展开目录或打开文件。

- [ ] **Step 5: 运行相关测试和 TypeScript 构建**

Run: `node scripts/run-package-bin.mjs vitest run src/__tests__/inlineRename.test.ts src/__tests__/fileTreeSelection.test.ts; pnpm build`

Expected: 两个测试文件通过；`tsc --noEmit` 和 Vite 生产构建成功。

- [ ] **Step 6: 审查改动并提交组件实现**

Run: `git diff --check; git diff -- src/components/layout/FileTree.tsx src/utils/inlineRename.ts src/__tests__/inlineRename.test.ts`

Expected: 无空白错误；无 `window.prompt` 重命名调用；输入框仅在当前路径处显示。

```powershell
git add src/components/layout/FileTree.tsx src/utils/inlineRename.ts src/__tests__/inlineRename.test.ts
git commit -m "feat: rename file tree entries inline"
```

### Task 3: 完整回归验证

**Files:**
- Modify: 无

- [ ] **Step 1: 运行完整前端测试套件**

Run: `pnpm test`

Expected: PASS，既有单元测试与新增原位重命名测试均通过。

- [ ] **Step 2: 运行前端生产构建与 Git 健康检查**

Run: `pnpm build; git diff --check; git status --short`

Expected: 构建成功；无 diff 格式错误；工作区无未提交改动。
