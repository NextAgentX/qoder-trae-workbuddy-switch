/**
 * 账号授权页的统一开关（公共设置 `auth_in_app`）。
 *
 * ## 为什么需要一个共用入口
 *
 * 三个登录对话框（WorkBuddy / Trae / Qoder）原来各自实现了一遍 `openInBrowser`，
 * 逻辑逐字相同。现在多了「默认在应用内打开」这条分支，如果也各写一遍，
 * 就会出现「某个平台忘了读设置」——而那种漂移在界面上完全看不出来
 * （用户只会觉得「这个平台怎么老跳浏览器」）。
 *
 * ## 三条约定
 *
 * ① **内嵌只是默认，不是唯一**：开窗失败（宿主不支持 / URL 被拒 / 建窗异常）一律退系统浏览器。
 *    否则一次异常就等于「登不了账号」，而这条路径没有任何备用入口。
 * ② **webui / 演示模式恒走浏览器**：那里没有 Tauri 窗口可开。
 * ③ **关窗失败不报错**：令牌由后端轮询取回，窗口关不掉只是留个窗，不影响登录结果。
 */

import * as api from "@/lib/api";

/** 用系统浏览器打开（webui 走新标签页；桌面走 opener 插件）。 */
export async function openInSystemBrowser(url: string): Promise<void> {
  if (!api.isDesktop()) {
    // 浏览器环境没有 Tauri 注入的 invoke；window.open 被拦截时，登录弹窗里的
    // 原生链接 / 二维码仍是完整路径，因此这里不把拦截视为授权失败。
    try {
      window.open(url, "_blank", "noopener,noreferrer");
    } catch {
      /* 忽略自动弹窗失败 */
    }
    return;
  }
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  return openUrl(url);
}

/**
 * 打开账号授权页：按设置决定「应用内窗口」还是「系统浏览器」。
 *
 * @param key 只用于窗口 label 命名与诊断（`wb-cn` / `trae-<variant>` / `qoder-<scope>`）。
 *            ★ **不是**「窗口身份」—— 每次调用都会开一个**新**窗口（多账号并行登录、
 *            互不覆盖；旧实现按 key 复用窗口，导致第二个账号的登录页顶掉第一个）。
 * @param url 后端给的 `verificationUri`。
 * @returns 应用内窗口的 `label`（关窗凭据）；走了系统浏览器则 `null`。
 */
export async function openAuthPage(key: string, url: string): Promise<string | null> {
  if (api.isDesktop() && !api.isDemoMode()) {
    let inApp = true;
    try {
      inApp = (await api.getAppPrefs()).auth_in_app;
    } catch {
      // 读不到偏好就按默认（开）走：这是**体验偏好**，不是安全开关，
      // 读失败不该把用户挡在「只能跳浏览器」那一侧。
      inApp = true;
    }
    if (inApp) {
      try {
        const opened = await api.authWebviewOpen(key, url);
        return opened.label || null;
      } catch (e) {
        // 宿主没建成窗（或 URL 被形状校验拒绝）⇒ 退回浏览器，登录流程不受影响。
        console.warn("[auth] 应用内授权窗打开失败，改用系统浏览器:", api.asError(e));
      }
    }
  }
  await openInSystemBrowser(url);
  return null;
}

/** 关闭授权窗（幂等；`label` 为空 / 非桌面端 / 演示模式都是空操作）。 */
export async function closeAuthPage(label: string | null | undefined): Promise<void> {
  if (!label || !api.isDesktop() || api.isDemoMode()) return;
  try {
    await api.authWebviewClose(label);
  } catch {
    /* 关不掉只是留个窗，令牌已经由后端轮询取回 —— 不打扰用户。 */
  }
}
