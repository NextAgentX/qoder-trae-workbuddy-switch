import { useCallback, useState } from "react";

/**
 * 账号列表的「紧凑模式」偏好。
 *
 * WorkBuddy 与 Trae 两个产品分区共用同一个键：它是**用户看表的习惯**，不是产品属性，
 * 在两个 Tab 之间来回切换时不应该来回跳。读取失败（隐私模式等）时退回默认值。
 *
 * ⚠️ **默认值改为「关闭」（宽松形态）**，与设计原型对齐：原型的账号卡只有一种形态
 * （`buddy-switch-prototype.html` 的 `.acct`：头像+名称+标签 → 指标行 → 程序位+更多），
 * 原型里**没有**紧凑形态。旧默认值是开启 ⇒ 首次打开看到的是「程序位挤在头部、卡片
 * 只有一行摘要」的紧凑卡，与原型观感明显不同（同一份 demo 数据下最直观的一处差异）。
 * 紧凑模式本身**保留**：这是给「账号多、想一屏看更多」的用户的可选偏好，删掉是砍功能。
 */
const STORAGE_KEY = "buddy-switch.compact";

export function useCompactMode(): [boolean, () => void] {
  const [compact, setCompact] = useState<boolean>(() => {
    try {
      return localStorage.getItem(STORAGE_KEY) === "1";
    } catch {
      return false;
    }
  });

  const toggle = useCallback(() => {
    setCompact((value) => {
      const next = !value;
      try {
        localStorage.setItem(STORAGE_KEY, next ? "1" : "0");
      } catch {
        /* 存储不可用时静默 */
      }
      return next;
    });
  }, []);

  return [compact, toggle];
}
