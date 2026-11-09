import { describe, expect, it } from "vitest";

import {
  cooldownAgeKey,
  cooldownNeedsAttention,
  describeTraeCooldown,
} from "@/lib/trae-cooldown";

describe("describeTraeCooldown —— 冷却态三态映射（#108）", () => {
  it("没有冷却（null / undefined / 空串）不渲染徽标", () => {
    expect(describeTraeCooldown(null)).toBeNull();
    expect(describeTraeCooldown(undefined)).toBeNull();
    expect(describeTraeCooldown("")).toBeNull();
  });

  it("SessionDead 归为 destructive（需要人工重新登录，不是「等它好」）", () => {
    const display = describeTraeCooldown("SessionDead");
    expect(display).not.toBeNull();
    expect(display?.tone).toBe("destructive");
    expect(display?.labelKey).toBe("trae.comp.card.chip.needRelogin");
    expect(display?.hintKey).toBe("trae.comp.card.cooldown.hint.sessionDead");
  });

  it("AuthRejected 归为 warning（token 可自愈，60 秒后重试）", () => {
    const display = describeTraeCooldown("AuthRejected");
    expect(display?.tone).toBe("warning");
    expect(display?.labelKey).toBe("trae.comp.card.chip.authRejected");
    expect(display?.hintKey).toBe("trae.comp.card.cooldown.hint.authRejected");
  });

  it("PlanLimit 与瞬时限流等未知档位一律为 warning + transient 提示", () => {
    const planLimit = describeTraeCooldown("PlanLimit");
    expect(planLimit?.tone).toBe("warning");
    expect(planLimit?.hintKey).toBe("trae.comp.card.cooldown.hint.planLimit");

    for (const type of ["SoftRate", "Server", "Client", "BusinessError", "NotFound"]) {
      const display = describeTraeCooldown(type);
      expect(display?.tone).toBe("warning");
      expect(display?.labelKey).toBe("trae.comp.card.chip.cooling");
      expect(display?.hintKey).toBe("trae.comp.card.cooldown.hint.transient");
    }
  });

  it("未知字符串不抛错，按瞬时冷却处理（前后端枚举漂移时不白屏）", () => {
    expect(() => describeTraeCooldown("SomeFutureType")).not.toThrow();
    expect(describeTraeCooldown("SomeFutureType")?.hintKey).toBe(
      "trae.comp.card.cooldown.hint.transient",
    );
  });
});

describe("cooldownAgeKey —— 判定新旧（#108 反复重登）", () => {
  const now = 1_700_000_000_000; // epoch 毫秒
  const nowSeconds = now / 1000;

  it("拿不到时刻（null / undefined / 0 / 负数）返回 null，不补假值", () => {
    expect(cooldownAgeKey(null, now)).toBeNull();
    expect(cooldownAgeKey(undefined, now)).toBeNull();
    expect(cooldownAgeKey(0, now)).toBeNull();
    expect(cooldownAgeKey(-1, now)).toBeNull();
  });

  it("不足 1 分钟显示 justNow（包含 since 在未来时的钳制）", () => {
    expect(cooldownAgeKey(nowSeconds - 30, now)).toEqual({
      key: "trae.comp.card.cooldown.age.justNow",
      minutes: 0,
    });
    // since 比 now 还晚：不得出现负分钟
    expect(cooldownAgeKey(nowSeconds + 3600, now)).toEqual({
      key: "trae.comp.card.cooldown.age.justNow",
      minutes: 0,
    });
  });

  it("分钟档的边界：正好 1 分钟进 minutes，59.999 分钟仍为 59", () => {
    expect(cooldownAgeKey(nowSeconds - 60, now)).toEqual({
      key: "trae.comp.card.cooldown.age.minutes",
      minutes: 1,
    });
    const justUnderAnHour = cooldownAgeKey(nowSeconds - (59 * 60 + 59.999), now);
    expect(justUnderAnHour?.key).toBe("trae.comp.card.cooldown.age.minutes");
    expect(justUnderAnHour?.minutes).toBe(59);
  });

  it("小时档的边界：正好 60 分钟进 hours，23 小时 59 分仍为 23", () => {
    expect(cooldownAgeKey(nowSeconds - 60 * 60, now)).toEqual({
      key: "trae.comp.card.cooldown.age.hours",
      minutes: 1,
    });
    const justUnderADay = cooldownAgeKey(nowSeconds - (24 * 60 - 1) * 60, now);
    expect(justUnderADay?.key).toBe("trae.comp.card.cooldown.age.hours");
    expect(justUnderADay?.minutes).toBe(23);
  });

  it("天档：正好 24 小时为 1 天，且 minutes 字段改天数口径", () => {
    expect(cooldownAgeKey(nowSeconds - 24 * 60 * 60, now)).toEqual({
      key: "trae.comp.card.cooldown.age.days",
      minutes: 1,
    });
    expect(cooldownAgeKey(nowSeconds - 11 * 60 * 60, now)?.key).toBe(
      "trae.comp.card.cooldown.age.hours",
    );
  });
});

describe("cooldownNeedsAttention —— 「待处理」只认 SessionDead", () => {
  it("只有 SessionDead 需要人动手", () => {
    expect(cooldownNeedsAttention("SessionDead")).toBe(true);
  });

  it("会自己到期的档位不算待处理，缺省值也不算", () => {
    expect(cooldownNeedsAttention("AuthRejected")).toBe(false);
    expect(cooldownNeedsAttention("PlanLimit")).toBe(false);
    expect(cooldownNeedsAttention("SoftRate")).toBe(false);
    expect(cooldownNeedsAttention(null)).toBe(false);
    expect(cooldownNeedsAttention(undefined)).toBe(false);
    expect(cooldownNeedsAttention("")).toBe(false);
  });

  it("大小写/近似值不误判（精确匹配后端枚举）", () => {
    expect(cooldownNeedsAttention("sessiondead")).toBe(false);
    expect(cooldownNeedsAttention("SessionDead ")).toBe(false);
  });
});
