<!-- TRELLIS:START -->
# Trellis Instructions

These instructions are for AI assistants working in this project.

This project is managed by Trellis. The working knowledge you need lives under `.trellis/`:

- `.trellis/workflow.md` — development phases, when to create tasks, skill routing
- `.trellis/spec/` — package- and layer-scoped coding guidelines (read before writing code in a given layer)
- `.trellis/workspace/` — per-developer journals and session traces
- `.trellis/tasks/` — active and archived tasks (PRDs, research, jsonl context)

If a Trellis command is available on your platform (e.g. `/trellis:finish-work`, `/trellis:continue`), prefer it over manual steps. Not every platform exposes every command.

If you're using Codex or another agent-capable tool, additional project-scoped helpers may live in:
- `.agents/skills/` — reusable Trellis skills
- `.codex/agents/` — optional custom subagents

Managed by Trellis. Edits outside this block are preserved; edits inside may be overwritten by a future `trellis update`.

<!-- TRELLIS:END -->

## UI Component Policy

- For frontend UI, prefer the project's existing shadcn components and compose them before writing custom interactive primitives.
- If a required component is missing, add the matching shadcn/Radix component and wrap it under `src/components/ui/` so styling, accessibility, focus management, and behavior stay consistent.
- Write a custom component only when shadcn components and their composition APIs cannot satisfy the requirement. Record the reason before doing so.
- Custom UI must still reuse the project's Rhea theme tokens, spacing, radii, states, and accessibility conventions. Do not substitute native interactive shortcuts such as `details/summary` when an appropriate shadcn component exists.

## Git Commit Language

- Use Conventional Commit type prefixes such as `feat:`, `fix:`, and `docs:`.
- Write the commit subject and body in Chinese by default. Use English only when the user explicitly requests it.

## 并行提交（同一工作树里还有别人的改动时）

这个仓库**经常有两个及以上会话同时改同一个 checkout**。`git status` 里出现你不认识的文件时，
先判断归属，再动手 —— 别 `git add -A`，也别 `git checkout -- <file>`（会直接抹掉对方未提交的工作）。

三种情形，三种做法：

| 情形 | 做法 |
|:--|:--|
| 你改的文件**没有**别人的 hunk | 正常 `git add <你的文件>` 后提交；**共享 index 里若有别人已暂存的条目**，见下面第 3 行 |
| 同一文件里既有你的 hunk 又有别人的 | `git diff -- <file>` 切出 patch，按 `@@` 序号只取自己的（`awk '/^@@/{n++} n<=K'`），再 `git apply --cached`（只进暂存区、不动工作树） |
| 想提交时**共享 index 混着别人的暂存**，或要改写已有提交 | 用**独立 index + plumbing**，全程不碰 `.git/index` 与工作树 |

独立 index 的完整姿势（**不要**用 `git commit`，它会连带别人的暂存一起提交）：

```bash
export GIT_INDEX_FILE="$PWD/.git/index.tmp"   # 独立 index
git read-tree HEAD                            # 以当前提交为底
BLOB=$(git hash-object -w <你的文件>)          # 或 update-index --cacheinfo 取历史版本
git update-index --cacheinfo 100644,$BLOB,<你的文件>
TREE=$(git write-tree)
git diff-tree -r --name-status HEAD^{tree} "$TREE"   # ★ 先自证「差异只有我要改的」
git commit-tree "$TREE" -p HEAD -F msg.txt
git update-ref refs/heads/<branch> <new-commit>
rm -f .git/index.tmp                          # ⚠️ 必须用相对路径
```

三个坑：

- **`rm` 用绝对 Windows 路径会被安全护栏拦下**（`[SAFE_DELETE_INVALID_PATH] embedded drive prefix`），
  而且会让整条 `&&` 链**中断**（后面的 `git log` 之类全不执行，看起来像提交失败）。
- **rename 型冲突不要靠改文件内容解决**：如果对方正在把 `A` 改名/删除成 `B`，正确做法是让你的提交
  **完全不碰 `A`**；否则无论谁先合都会撞。
- 改写已推送的 PR 分支用 `git push --force-with-lease`，**不要** `--force`。

提交后三个自查：① `git diff-tree` 差异清单 = 你打算改的文件；② `git status` 里别人的条目**仍然在**；
③ `git show --stat HEAD` 不包含别人的文件。

## 文档规范（Documentation Layout）

`docs/` 按**生命周期**分区，入口是 [`docs/README.md`](docs/README.md)（文档地图）。**新增或移动文档前先读它。**

| 目录 | 放什么 | 更新策略 |
|:--|:--|:--|
| `docs/guides/` | 安装、开发、客户端接入步骤 | 跟版本更新 |
| `docs/policies/` | 许可、免责声明（README 引用） | 极少 |
| `docs/specs/` | 现行架构与决策（`PLAN.md` / `P*-ARCH.md` / `DECISIONS.md` / `VOCABULARY.md`） | **改行为就同步改** |
| `docs/history/` | 已完结的一次性报告（审计、探针、调研、旧方案） | **只读，不再改** |
| `docs/prototype/` | 原型 + JSDOM 校验套件 | 新交互先进这里 |
| `docs/releases/` | 每版发布说明（`gen-release-notes.mjs` 约定路径） | 每版一篇 |

硬性约定：

- **`docs/prototype/` 不是普通文档**：`scripts/check-scope-registry.cjs` 会直接读取其中的 `demo-app.js` 与 `buddy-switch-prototype.html`。改名或移动必须同步改脚本，否则 `npm run build` 会红。
- **不要新建同义文档**：`docs/specs/` 里已有主题就直接改那一篇；确认过时才新开，并在首段写状态行（`> 状态：现行 / 待拍板 / 实施中`）。
- **一次性文档带日期**（`xxx-2026-10-17.md`），完结后移入 `docs/history/` 并加 `> 状态：只读历史` 头。
- **移动/新增文档后必须更新 `docs/README.md` 的表格**——索引与文件不同步比没有索引更坏。
- 代码注释引用设计依据时写**仓库相对路径**（`docs/specs/P5-ARCH.md`），不要写裸文件名。
