/**
 * WorkBuddy 的**外部程序位状态**（CLI / IDE / VS Code / JetBrains）—— 聚合页用。
 *
 * ## 为什么单独一层
 *
 * 账号页（`AccountsPage`）自己拉这四条 `*_status` 命令并逐个包成 props 传给卡片。
 * 聚合页要渲染**同一批**程序位胶囊，若再抄一遍拉取与判定的代码，两处就会漂：
 * 最容易漂的是「可用」的判据 —— CLI 侧是
 * `configured && !migrationRequired && !syncPending`（三项与运算），
 * 少一项就会让一枚「配置了但待迁移」的按钮看起来可用，点了必然失败。
 *
 * ⇒ 这里抽成一份纯函数 + 一个 hook，两边共用。
 *
 * ## 为什么是**全局**而不是按作用域
 *
 * 这四条命令**没有 region 参数** —— 它们读的是本机的固定状态文件
 * （CLI 的配置、IDE 的安装目录、编辑器的 `state.vscdb`），与账号库分家。
 * 所以 6 个作用域共用同一份快照 ⇒ 聚合页只拉一次（4 条命令），不是 6×4 次。
 *
 * ⚠️ 但「当前登着哪个账号」是**按区域**的（账号 id 不同）⇒ 判定在页面侧做。
 */

import { useCallback, useEffect, useState } from "react";

import * as api from "@/lib/api";
import type {
  CodeBuddyCliStatus,
  CodeBuddyCnIdeStatus,
  JetbrainsCodebuddyPluginStatus,
  VscodeCodebuddyPluginStatus,
} from "@/lib/types";

/** 四个程序位的状态快照。任一项取不到 ⇒ `null`（**不是 false**）。 */
export interface WbProgramStatuses {
  cli: CodeBuddyCliStatus | null;
  cnIde: CodeBuddyCnIdeStatus | null;
  vscode: VscodeCodebuddyPluginStatus | null;
  jetbrains: JetbrainsCodebuddyPluginStatus | null;
}

const EMPTY: WbProgramStatuses = { cli: null, cnIde: null, vscode: null, jetbrains: null };

/**
 * 拉取四个程序位状态（**只拉一次**，全局共用）。
 *
 * ⚠️ 这四条命令**没有 region 参数**（读本机固定状态文件）⇒ 不按作用域重复拉。
 * ⚠️ `getVscodeCodebuddyPluginStatus` / `getJetbrainsCodebuddyPluginStatus` 只读状态、
 *     **不解密 secret**（解密会弹钥匙串授权，见 `AccountsPage` 的同款注释）。
 */
export function useWbProgramStatuses(): WbProgramStatuses {
  const [statuses, setStatuses] = useState<WbProgramStatuses>(EMPTY);

  const reload = useCallback(async () => {
    const [cli, cnIde, vscode, jetbrains] = await Promise.all([
      api.getCodebuddyCliStatus().catch(() => null),
      api.getCodebuddyCnIdeStatus().catch(() => null),
      api.getVscodeCodebuddyPluginStatus().catch(() => null),
      api.getJetbrainsCodebuddyPluginStatus().catch(() => null),
    ]);
    setStatuses({ cli, cnIde, vscode, jetbrains });
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  return statuses;
}

/**
 * 各程序位的「可用」判据（与账号页**逐字一致**）。
 *
 * ⚠️ 三种「不可用」的原因不同，UI 的措辞也不同：
 *   · CLI：`configured && !migrationRequired && !syncPending`（三项与运算）；
 *   · IDE：`installed`（装了客户端）；
 *   · 编辑器插件：`extensionInstalled` / `pluginInstalled`（**扩展装了**，不是「编辑器装了」）。
 */
export function wbProgramAvailability(statuses: WbProgramStatuses): {
  cliConfigured: boolean;
  /**
   * CLI 是否已被指向**本机网关**（issue #109）。
   *
   * `true` ⇒ 选号由网关按模型决定，「切到 CLI」这个动作**语义上已不存在**：
   * 后端会拒绝（`ensure_not_gateway_mode`），卡片应置灰并说明，而不是让人点了才看到报错。
   */
  cliGatewayMode: boolean;
  cnIdeAvailable: boolean;
  vscodeAvailable: boolean;
  jetbrainsAvailable: boolean;
} {
  return {
    cliConfigured: Boolean(
      statuses.cli?.configured && !statuses.cli.migrationRequired && !statuses.cli.syncPending,
    ),
    cliGatewayMode: Boolean(statuses.cli?.gatewayMode),
    cnIdeAvailable: Boolean(statuses.cnIde?.installed),
    vscodeAvailable: Boolean(statuses.vscode?.extensionInstalled),
    jetbrainsAvailable: Boolean(statuses.jetbrains?.pluginInstalled),
  };
}
