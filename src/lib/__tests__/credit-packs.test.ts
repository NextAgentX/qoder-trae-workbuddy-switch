import { describe, expect, it } from "vitest";

import {
  normalizePacks,
  packIsExpired,
  packRatio,
  secondsToMs,
  toDetailPacks,
  type CreditPackInput,
  type CreditPackTexts,
} from "@/lib/credit-packs";

const NOW = 1_700_000_000_000; // epoch 毫秒

function makePack(overrides: Partial<CreditPackInput> = {}): CreditPackInput {
  return {
    key: "pack",
    name: "资源包",
    total: 100,
    remaining: 50,
    used: 50,
    expireAtMs: NOW + 86_400_000,
    ...overrides,
  };
}

function makeTexts(): CreditPackTexts {
  return {
    remainingText: (value) => `remaining:${value}`,
    totalText: (value) => `total:${value}`,
    usedText: (value) => `used:${value}`,
    expiryText: (expireAtMs, expired) => `expiry:${expireAtMs}:${expired}`,
  };
}

describe("packIsExpired —— 后端标记 ∪ 到期时刻（边界 ±1ms）", () => {
  it("后端 expired 标记优先：即使没有到期时刻也算过期", () => {
    expect(packIsExpired(makePack({ expired: true, expireAtMs: null }), NOW)).toBe(true);
    expect(packIsExpired(makePack({ expired: true, expireAtMs: NOW + 1 }), NOW)).toBe(true);
  });

  it("到期时刻 == now 算过期（<= 不是 <）", () => {
    expect(packIsExpired(makePack({ expireAtMs: NOW }), NOW)).toBe(true);
    expect(packIsExpired(makePack({ expireAtMs: NOW - 1 }), NOW)).toBe(true);
    expect(packIsExpired(makePack({ expireAtMs: NOW + 1 }), NOW)).toBe(false);
  });

  it("expireAtMs 为 null 表示永久有效，不是「已过期」", () => {
    expect(packIsExpired(makePack({ expireAtMs: null }), NOW)).toBe(false);
  });

  it("非有限数不做比较（避免 NaN 把包误标成过期）", () => {
    expect(packIsExpired(makePack({ expireAtMs: Number.NaN }), NOW)).toBe(false);
    expect(packIsExpired(makePack({ expireAtMs: Number.POSITIVE_INFINITY }), NOW)).toBe(false);
  });
});

describe("packRatio —— 漏乘 100 的回归护栏", () => {
  it("50/100 必须是 50（不是 0.5）", () => {
    expect(packRatio(50, 100)).toBe(50);
  });

  it("夹紧到 0..100：超额、负数、零总量", () => {
    expect(packRatio(150, 100)).toBe(100);
    expect(packRatio(-10, 100)).toBe(0);
    expect(packRatio(0, 100)).toBe(0);
    expect(packRatio(50, 0)).toBe(0);
    expect(packRatio(50, -100)).toBe(0);
  });

  it("非有限输入返回 0，不产出 NaN 宽度", () => {
    expect(packRatio(Number.NaN, 100)).toBe(0);
    expect(packRatio(Number.POSITIVE_INFINITY, 100)).toBe(0);
    expect(packRatio(50, Number.NaN)).toBe(0);
  });
});

describe("secondsToMs —— #144 Unix 秒当毫秒的回归护栏", () => {
  it("秒 → 毫秒恰好乘 1000（不是 1970 年）", () => {
    expect(secondsToMs(1_700_000_000)).toBe(1_700_000_000_000);
    expect(new Date(secondsToMs(1_700_000_000) as number).getUTCFullYear()).toBe(2023);
  });

  it("null 保持 null（永久有效），0 保持 0", () => {
    expect(secondsToMs(null)).toBeNull();
    expect(secondsToMs(0)).toBe(0);
  });
});

describe("normalizePacks —— 过滤 / 排序 / 临期标记", () => {
  it("null / undefined / 空数组返回空列表", () => {
    expect(normalizePacks(null, NOW)).toEqual([]);
    expect(normalizePacks(undefined, NOW)).toEqual([]);
    expect(normalizePacks([], NOW)).toEqual([]);
  });

  it("remaining <= 0 与 total <= 0 都不列（#143/#144 判据）", () => {
    const rows = normalizePacks(
      [
        makePack({ key: "ok" }),
        makePack({ key: "zero-remaining", remaining: 0 }),
        makePack({ key: "negative-remaining", remaining: -1 }),
        makePack({ key: "zero-total", total: 0, remaining: 0 }),
        makePack({ key: "nan-remaining", remaining: Number.NaN }),
      ],
      NOW,
    );
    expect(rows.map((row) => row.key)).toEqual(["ok"]);
  });

  it("排序：未过期按到期升序 → null（永久）最后 → 已过期整体垫底", () => {
    const rows = normalizePacks(
      [
        makePack({ key: "expired-late", expireAtMs: NOW - 1 }),
        makePack({ key: "permanent", expireAtMs: null }),
        makePack({ key: "later", expireAtMs: NOW + 5_000 }),
        makePack({ key: "expired-flag", expired: true, expireAtMs: NOW + 9_999 }),
        makePack({ key: "sooner", expireAtMs: NOW + 1_000 }),
        makePack({ key: "expires-now", expireAtMs: NOW }),
      ],
      NOW,
    );
    expect(rows.map((row) => row.key)).toEqual([
      "sooner",
      "later",
      "permanent",
      "expired-late",
      "expires-now",
      "expired-flag",
    ]);
  });

  it("不改入参（调用方常直接传 store 数组）", () => {
    const input = [
      makePack({ key: "b", expireAtMs: NOW + 5_000 }),
      makePack({ key: "a", expireAtMs: NOW + 1_000 }),
    ];
    const snapshot = input.map((pack) => pack.key);
    normalizePacks(input, NOW);
    expect(input.map((pack) => pack.key)).toEqual(snapshot);
  });

  it("expiringSoon 只认后端 === true，且与 expired 互斥（不推测窗口）", () => {
    const rows = normalizePacks(
      [
        // 距到期仅 1ms 但没有标记 ⇒ 不是临期（窗口不在这里推）
        makePack({ key: "near-without-flag", expireAtMs: NOW + 1 }),
        makePack({ key: "flag-true", expiringSoon: true }),
        makePack({ key: "expired-with-flag", expiringSoon: true, expireAtMs: NOW - 1 }),
      ],
      NOW,
    );
    const byKey = new Map(rows.map((row) => [row.key, row]));
    expect(byKey.get("near-without-flag")?.expiringSoon).toBe(false);
    expect(byKey.get("flag-true")?.expiringSoon).toBe(true);
    expect(byKey.get("expired-with-flag")?.expiringSoon).toBe(false);
    expect(byKey.get("expired-with-flag")?.expired).toBe(true);
  });

  it("ratio 已按统一算式算好（含夹紧），不再留给调用方", () => {
    const rows = normalizePacks(
      [makePack({ remaining: 30, total: 100 }), makePack({ remaining: 200, total: 100 })],
      NOW,
    );
    expect(rows.map((row) => row.ratio)).toEqual([30, 100]);
  });
});

describe("toDetailPacks —— 文案只换措辞，不改逻辑", () => {
  it("按归一后的顺序映射，且 expiryText 收到 (epoch 毫秒, expired)", () => {
    const texts = makeTexts();
    const packs = toDetailPacks(
      [makePack({ key: "permanent", expireAtMs: null, remaining: 30, total: 120, used: 90 })],
      texts,
      NOW,
    );
    expect(packs).toHaveLength(1);
    expect(packs[0]).toEqual({
      key: "permanent",
      name: "资源包",
      remainingText: "remaining:30",
      totalText: "total:120",
      usedText: "used:90",
      expiryText: "expiry:null:false",
      expired: false,
      expiringSoon: false,
      ratio: 25,
    });
  });

  it("过期包透传 expired=true 给 expiryText，过滤器仍然生效", () => {
    const packs = toDetailPacks(
      [
        makePack({ key: "gone", expireAtMs: NOW - 1 }),
        makePack({ key: "empty", remaining: 0 }),
      ],
      makeTexts(),
      NOW,
    );
    expect(packs.map((pack) => pack.key)).toEqual(["gone"]);
    expect(packs[0].expiryText).toBe(`expiry:${NOW - 1}:true`);
  });
});
