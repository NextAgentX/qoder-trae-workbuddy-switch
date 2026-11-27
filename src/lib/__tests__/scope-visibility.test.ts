/**
 * `src/lib/scope-visibility.ts` 只导出一个 zustand store，没有导出的纯函数。
 * zustand store 可以脱离 React 直接读写（`getState` / `setState`），
 * 因此这里测的是 store 的状态机语义：收敛、顺序、以及「至少保留 1 个」的不变量。
 *
 * ⚠️ 开关的 `disabled` 方向（#85）写在 `components/app-settings.tsx` 的渲染逻辑里，
 * 不在本模块的导出面上 —— 本文件只锁住它依赖的「可见集合不会被清空」这一前提。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SCOPES, type Scope } from "@/lib/scope";
import { useVisibleScopes } from "@/lib/scope-visibility";

function resetTo(scopes: readonly Scope[]) {
  useVisibleScopes.setState({ visibleScopes: scopes });
}

/** 模拟 localStorage 里被旧版本/手改污染的值（运行期类型不受编译期约束）。 */
function dirty(values: readonly unknown[]): readonly Scope[] {
  return values as readonly Scope[];
}

describe("useVisibleScopes —— 白名单归一与顺序", () => {
  beforeEach(() => {
    resetTo(SCOPES);
  });

  it("冷启动默认全量可见，且顺序等于注册表顺序", () => {
    expect(useVisibleScopes.getState().visibleScopes).toEqual([...SCOPES]);
    expect(useVisibleScopes.getState().visibleScopes).toHaveLength(SCOPES.length);
  });

  it("乱序输入按 SCOPES 注册表排序（序号 = 注册表序号，不漂移）", () => {
    useVisibleScopes.getState().setVisibleScopes(["qoder-global", "wb-cn", "trae-global"]);
    expect(useVisibleScopes.getState().visibleScopes).toEqual([
      "wb-cn",
      "trae-global",
      "qoder-global",
    ]);
  });

  it("重复项去重，非法项丢弃", () => {
    useVisibleScopes
      .getState()
      .setVisibleScopes(dirty(["wb-cn", "wb-cn", "wb-jp", "", "trae-cn"]));
    expect(useVisibleScopes.getState().visibleScopes).toEqual(["wb-cn", "trae-cn"]);
  });

  it("关掉一个作用域只影响该作用域（方向敏感，不波及邻居）", () => {
    const before = useVisibleScopes.getState().visibleScopes;
    useVisibleScopes
      .getState()
      .setVisibleScopes(before.filter((scope) => scope !== "trae-cn"));
    const after = useVisibleScopes.getState().visibleScopes;
    expect(after).toEqual(SCOPES.filter((scope) => scope !== "trae-cn"));
    expect(after).not.toContain("trae-cn");
    expect(after).toContain("wb-cn");
  });

  it("重新打开后回到注册表位置，而不是追加到末尾", () => {
    resetTo(["wb-cn", "qoder-global"]);
    // 重新勾上 trae-cn：它应落在 wb-cn 之后、qoder-global 之前
    useVisibleScopes.getState().setVisibleScopes(["wb-cn", "qoder-global", "trae-cn"]);
    expect(useVisibleScopes.getState().visibleScopes).toEqual([
      "wb-cn",
      "trae-cn",
      "qoder-global",
    ]);
  });
});

describe("useVisibleScopes —— 至少保留 1 个（#85 的 store 侧前提）", () => {
  beforeEach(() => {
    resetTo(SCOPES);
  });

  it("只剩 1 个时提交空集合被忽略，不会变成 0 个", () => {
    resetTo(["wb-cn"]);
    useVisibleScopes.getState().setVisibleScopes([]);
    expect(useVisibleScopes.getState().visibleScopes).toEqual(["wb-cn"]);
    expect(useVisibleScopes.getState().visibleScopes).toHaveLength(1);
  });

  it("全是非法值时同样保留当前值（坏数据不清空可见集合）", () => {
    resetTo(["trae-cn"]);
    useVisibleScopes.getState().setVisibleScopes(dirty(["nope", 42, null]));
    expect(useVisibleScopes.getState().visibleScopes).toEqual(["trae-cn"]);
  });

  it("可见集合降到 1 时，其他作用域仍可重新开启", () => {
    const only = "wb-cn" as const;
    useVisibleScopes.getState().setVisibleScopes([only]);
    const visible = useVisibleScopes.getState().visibleScopes;
    expect(visible).toEqual([only]);
    // UI 只应禁用「唯一且可见」的那一个开关；注册表里的其他项仍可重新开启
    const remaining = SCOPES.filter((scope) => !visible.includes(scope));
    expect(remaining).toHaveLength(SCOPES.length - 1);
    expect(remaining).not.toContain(only);
  });

  it("逐个关闭到 1 个时，最后一个提交被按预期保留", () => {
    for (const scope of SCOPES.slice(0, -1)) {
      useVisibleScopes
        .getState()
        .setVisibleScopes(useVisibleScopes.getState().visibleScopes.filter((s) => s !== scope));
    }
    expect(useVisibleScopes.getState().visibleScopes).toEqual([SCOPES[SCOPES.length - 1]]);
    useVisibleScopes.getState().setVisibleScopes([]);
    expect(useVisibleScopes.getState().visibleScopes).toEqual([SCOPES[SCOPES.length - 1]]);
  });
});


/**
 * v1 → v2 迁移（2026-11-25 外部平台接入）。旧键只有 core 六个作用域，
 * 迁移必须补上三家外部平台；**坏数据**必须回落全量，而不是只剩外部平台。
 *
 * `readVisibleScopes` 在模块加载时执行，所以每个用例都要先装好 storage、
 * `vi.resetModules()` 后再 import store。测试环境是 node，没有 `window` ——
 * 这里给 `globalThis.localStorage` 一个内存实现。
 */
describe("useVisibleScopes —— v1 → v2 迁移", () => {
  const V2 = "buddy-switch:v2:visible-scopes";
  const V1 = "buddy-switch:v1:visible-scopes";
  const memory = new Map<string, string>();

  beforeEach(() => {
    memory.clear();
    (globalThis as { localStorage?: Storage }).localStorage = {
      getItem: (key: string) => memory.get(key) ?? null,
      setItem: (key: string, value: string) => void memory.set(key, value),
      removeItem: (key: string) => void memory.delete(key),
      clear: () => memory.clear(),
      key: () => null,
      length: 0,
    } as Storage;
  });

  afterEach(() => {
    delete (globalThis as { localStorage?: Storage }).localStorage;
    vi.resetModules();
  });

  async function loadWith(v1: string | null) {
    memory.clear();
    if (v1 !== null) memory.set(V1, v1);
    vi.resetModules();
    const mod = await import("@/lib/scope-visibility");
    return mod.useVisibleScopes.getState().visibleScopes;
  }

  it("旧值保留原有可见集合，并补上三家外部平台", async () => {
    expect(await loadWith(JSON.stringify(["wb-cn", "trae-global"]))).toEqual([
      "wb-cn", "trae-global", "doubao-cn", "loomy-cn", "raccoon-cn",
    ]);
  });

  it("旧值是空数组 ⇒ 回落全量（不能只剩三家外部平台）", async () => {
    const visible = await loadWith("[]");
    expect(visible).toEqual([...SCOPES]);
    expect(visible).toContain("wb-cn");
  });

  it("旧值全是非法项 ⇒ 同样回落全量", async () => {
    expect(await loadWith(JSON.stringify(["wb-jp", "", 42]))).toEqual([...SCOPES]);
  });

  it("迁移结果回写 v2，之后改旧键不再影响", async () => {
    await loadWith(JSON.stringify(["wb-cn"]));
    expect(memory.get(V2)).toBeDefined();
    memory.set(V1, JSON.stringify(["qoder-cn"]));
    vi.resetModules();
    const mod = await import("@/lib/scope-visibility");
    expect(mod.useVisibleScopes.getState().visibleScopes).toEqual([
      "wb-cn", "doubao-cn", "loomy-cn", "raccoon-cn",
    ]);
  });

  it("坏 JSON 不抛错，回落全量", async () => {
    expect(await loadWith("{not json")).toEqual([...SCOPES]);
  });
});
