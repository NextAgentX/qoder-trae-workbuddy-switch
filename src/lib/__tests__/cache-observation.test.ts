import { describe, expect, it } from "vitest";

import { gatewayCacheHit } from "@/lib/usage-ledger";

const window = {
  kind: "custom" as const,
  from: "2026-11-17",
  to: "2026-11-17",
};

const model = (overrides: Record<string, unknown>) => ({
  model: "m",
  input: 100,
  output: 10,
  total: 110,
  requests: 1,
  failed: 0,
  credit: 0,
  ...overrides,
});

describe("gatewayCacheHit cache observation", () => {
  it("keeps an explicitly measured miss at zero", () => {
    const hit = gatewayCacheHit(
      { days: [{ key: "2026-11-17", models: [model({ cached: 0, cacheObserved: true })] }] } as never,
      window,
    );
    expect(hit).toMatchObject({ observed: true, cached: 0, hitRate: 0 });
  });

  it("excludes an unobserved request from the hit-rate denominator", () => {
    const hit = gatewayCacheHit(
      {
        days: [
          {
            key: "2026-11-17",
            models: [
              model({ input: 100, cached: null }),
              model({ input: 900, cached: 450, cacheObserved: true }),
            ],
          },
        ],
      } as never,
      window,
    );
    expect(hit).toMatchObject({ observed: true, input: 900, cached: 450, hitRate: 0.5 });
  });

  it("reports null when every request in the window is unobserved", () => {
    const hit = gatewayCacheHit(
      { days: [{ key: "2026-11-17", models: [model({ cached: null })] }] } as never,
      window,
    );
    expect(hit).toMatchObject({ observed: false, input: 0, cached: 0, hitRate: null });
  });
});
