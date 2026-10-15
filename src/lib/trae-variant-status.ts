import * as api from "@/lib/api";
import { t } from "@/lib/i18n";
import type { TranslationKey } from "@/locales/zh";
import type { TraeProgramStatus, TraeRegionId, TraeVariantId, TraeVariantStatus } from "@/lib/trae-types";

/**
 * 造一个「状态未知」的程序位占位。
 *
 * `label` / `variant` 是**系统性标识**（前端比对与回传后端都用它，例如
 * `program === "trae_code"`），因此始终保持英文；只有 `nameAlias` 是**面向用户
 * 的展示名**，随语言切换。表在模块加载时定型 ⇒ 展示名由 `nameAliasKey` 延迟到
 * 读取时现取（`programNameAlias`），否则切语言后整张表不会更新。
 */
function programStub(
  program: TraeProgramStatus["program"],
  label: string,
  nameAliasKey: TranslationKey,
  variant: TraeVariantId,
): TraeProgramStatus {
  return {
    program,
    label,
    get nameAlias() {
      return t(nameAliasKey);
    },
    variant,
    installed: false,
    running: false,
    version: null,
    path: null,
    dataDir: null,
    dataDirExists: false,
    // 写侧目录（见 `TraeProgramStatus.writeDataDir`）：后端不可用时与 `dataDir` 同样置空。
    writeDataDir: null,
    writeDataDirExists: false,
  };
}

/**
 * 探测不到任何区域时仍列出的**两个区域**（状态未知的占位）。
 *
 * ⚠️ **只此一份**：账号页的区域切换条与账号卡片都要遍历「有哪些区域/程序位」，
 * 各自维护一份占位表迟早漂移（曾经由 `TraeVariantBar` 私有持有）。
 *
 * 程序位的 `variant` 是**切换时要回传给后端的标识**：
 * 2026-10-15 程序位收敛后，每个区域在役的只有**合并版客户端**一个程序位 ——
 * 国内是 `trae_merged`（`Trae CN`），国际是 `global_trae_merged`（`Trae`）。
 *
 * ⚠️ 遗留程序位（`trae_work` / `trae_cn` / `global` / `global_trae_code`）已从
 * **界面程序位表**退役（用户 2026-10-15 决策：只保留 Trae，TraeWork 不要了），
 * 但它们的**变体**在后端仍全部建模 —— 旧数据（快照目录 / Key 归属 / 日志分档）
 * 原样保留、不迁移不删除，旧版本的本项目也仍能读取。
 *
 * ⚠️ 2026-09-28（issue #3）之前，国际版 TraeCode 未建模 ⇒ 这里填 `null`、
 * 按钮禁用。现在每个程序位都已建模，**每个占位都必须带标识** ——
 * 留 `null` 会让「后端不可用」时的占位形态与正常形态不一致，
 * 而 `variant` 的类型已收成非空（`TraeProgramStatus.variant`）。
 *
 * `consoleBase` 是本表里**唯一**「只在后端不可用时才用到」的域值 —— 正常路径一律由后端
 * `region_endpoints(region).console_base` 提供（`platform.rs` 有单测钉住按区域分家）。
 * 它出现在这里是因为本表本就是「后端不可用时的展示快照」，与 `variantLabel` 同类；
 * ⚠️ **后端改域时必须回来同步这一处**，否则演示模式/后端挂掉时「关于」外链会指向旧站。
 */
export const TRAE_VARIANT_FALLBACK: TraeVariantStatus[] = [
  {
    variant: "cn",
    // 表在模块加载时就定型 ⇒ 只存文案键，展示名在**读取时**现取，语言切换才不会失效。
    // ⚠️ 区域名走 `trae.region.*`（客户端产品名），**不是** `shared.region.version.*`
    // —— 后端 `TraeRegion::display_name` 给的就是前者，用后者会让演示站与真机文案不同。
    get variantLabel() {
      return t("trae.region.cn");
    },
    consoleBase: "https://www.trae.cn",
    installed: false,
    running: false,
    version: null,
    path: null,
    dataDir: null,
    dataDirExists: false,
    // 写侧目录（见 `TraeProgramStatus.writeDataDir`）：后端不可用时与 `dataDir` 同样置空。
    writeDataDir: null,
    writeDataDirExists: false,
    programs: [
      // 2026-10-15 程序位收敛后，国内在役的只有合并版客户端（issue #20 合并产物），
      // 客户端现在就叫 `Trae CN`。`variant` 是它**自己的**标识 `trae_merged` ——
      // 变体决定快照目录 / Key 归属 / 日志分档。遗留程序位
      // （`trae_work` / `trae_cn`）已从界面退役，但后端变体全部保留（旧数据不动）。
      programStub("trae", "Trae CN", "trae.program.traeCn", "trae_merged"),
    ],
  },
  {
    variant: "global",
    get variantLabel() {
      return t("trae.region.global");
    },
    consoleBase: "https://www.trae.ai",
    installed: false,
    running: false,
    version: null,
    path: null,
    dataDir: null,
    dataDirExists: false,
    // 写侧目录（见 `TraeProgramStatus.writeDataDir`）：后端不可用时与 `dataDir` 同样置空。
    writeDataDir: null,
    writeDataDirExists: false,
    programs: [
      // 2026-10-15 程序位收敛后，国际在役的只有合并版客户端；国际版不带后缀 ⇒ `Trae`；
      // 变体是它**自己的** `global_trae_merged`。遗留程序位（`global` / `global_trae_code`）
      // 已从界面退役，但后端变体全部保留（旧数据不动）。
      programStub("trae", "Trae", "trae.program.traeGlobal", "global_trae_merged"),
    ],
  },
];

/**
 * 某个程序位当前登录的账号：**身份与展示名成对出现**。
 *
 * 两者同源于**同一次** `get_trae_profiles` 调用，因此结构上不可能出现
 * 「名字已更新、身份还是旧的」这种半更新状态 —— 这正是把它们放进一个对象、
 * 而不是两个平行的 Map 的理由（两个 Map 迟早漂移成两把不同的键）。
 */
export interface TraeVariantLogin {
  /** 身份（uid）。卡片上「是不是当前账号」的相等比较用它；**不要**拿它渲染文本。 */
  userId: string;
  /**
   * 给人看的名字：优先账号库里的 `name`，**查不到时回落 uid**。
   *
   * 回落只在这里写一次：状态条与设置页显示的是同一句话，两处各写一遍回落链
   * 迟早出现「一处显示名字、一处显示数字」。回落而不是显示「未知账号」的理由见
   * Rust 侧 `account::display_name_for` —— 那串数字仍能让用户去客户端里核对。
   */
  name: string;
}

/** 程序位标识 → 该程序当前登录账号（无 / 读不到时为 `null`）。 */
export type TraeVariantLogins = Partial<Record<TraeVariantId, TraeVariantLogin | null>>;

/**
 * 「全部区域的环境状态」在快照缓存里的键。
 *
 * **只此一份**：侧栏那颗运行状态圆点与账号页都要用这份探测结果，
 * 各自写一遍字符串字面量迟早漂移成两把不同的键（症状是「缓存命中不了、
 * 每次切换都重新探测」，而且不会有任何报错）。
 */
export const TRAE_VARIANTS_KEY = "trae:variants";

/**
 * 读取**全部区域**的环境状态（并排视角）。
 *
 * 返回**永不为空**：探测命令不可用（演示模式、后端未起来）或返回空列表时，
 * 回落到 {@link TRAE_VARIANT_FALLBACK} 的两个区域占位 —— 调用方据此渲染
 * 「有哪些区域」的控件，选项必须在后端不可用时依然存在，
 * 否则用户连切回另一条线的入口都没有。
 */
export async function loadTraeVariantStatuses(): Promise<TraeVariantStatus[]> {
  try {
    const result = await api.getTraeVariants();
    return result.variants?.length ? result.variants : TRAE_VARIANT_FALLBACK;
  } catch {
    return TRAE_VARIANT_FALLBACK;
  }
}

/**
 * 读取**每个程序位各自的**当前登录账号（`profiles.currentAccount` + 展示名）。
 *
 * ## 为什么遍历的是**程序位**而不是区域
 *
 * 登录态快照是**客户端级**的（它只能恢复到采集它的那个客户端里去），因此
 * 「当前账号」天然是**每个程序位一条**。区域级那个值只是界面汇总用的展示，
 * 不能拿来判断某个账号是否"当前账号"。
 *
 * `statuses` 由调用方传入而不是本函数自己探测：调用方（账号页）本来就要用
 * 同一份 `statuses` 去渲染控件，再探一次会得到第二个可能不一致的快照。
 *
 * ## 为什么同时返回身份与展示名
 *
 * 界面需要两个不同的事实：身份用于**相等比较**（卡片上谁是当前账号），
 * 展示名用于**给人看**（「已登录: `<名字>`」）。改造前只返回 uid，
 * 于是展示端只能把 uid 摆上去 —— 用户看到一串 16 位数字，认不出是谁。
 * 两个都由后端同一次响应给出，前端只负责回落（见 {@link TraeVariantLogin.name}）。
 *
 * **单个程序位失败不影响其余**：读不到只记 `null`（视为「没有当前账号」），
 * 不抛错 —— 卡片上少一枚「当前账号」角标，远好过整页报错。
 */
export async function loadTraeVariantLogins(
  statuses: TraeVariantStatus[],
): Promise<TraeVariantLogins> {
  // 每个程序位都带标识（2026-09-28 起 `TraeProgramStatus.variant` 非空），
  // 因此这里不再有「跳过未建模程序位」的过滤 —— 四个程序位各取一次登录态。
  const targets: TraeVariantId[] = statuses.flatMap((item) =>
    item.programs.map((program) => program.variant),
  );
  const pairs = await Promise.all(
    targets.map(async (variant): Promise<[TraeVariantId, TraeVariantLogin | null]> => {
      try {
        const profiles = await api.getTraeProfiles(variant);
        const userId = profiles.currentAccount ?? null;
        // 没有当前账号 ⇒ `null`（**不是** `{ userId: "" , name: "" }`）：
        // 调用方靠它区分「未登录」与「已登录但名字读不到」。
        if (userId === null) return [variant, null];
        return [variant, { userId, name: profiles.currentAccountName || userId }];
      } catch {
        return [variant, null];
      }
    }),
  );
  return Object.fromEntries(pairs) as TraeVariantLogins;
}

/** 取某个区域的条目（找不到时返回 `undefined`，调用方自行回落占位）。 */
export function findRegionStatus(
  statuses: TraeVariantStatus[],
  region: TraeRegionId,
): TraeVariantStatus | undefined {
  return statuses.find((item) => item.variant === region);
}

/**
 * 本区域下的**程序位**（2026-10-15 收敛后每区域 1 个：合并版），供「按程序位取数据」的卡片使用。
 *
 * ## 为什么取的是 {@link TRAE_VARIANT_FALLBACK} 而不是再探测一次
 *
 * 本函数要回答的是「这个区域**有哪些**程序位」，而这是一个**结构性事实**
 * （收敛后每区域只剩合并版一个程序位，见 `TraeProgramId` 的说明），与
 * 「装了没 / 在不在跑」无关。
 * `TRAE_VARIANT_FALLBACK` 正是这份结构表（区域切换器在后端不可用时也用它），
 * 复用它就**不会**多出第二份会漂移的程序位清单。
 *
 * ## ⚠️ 别拿它的 `installed` 当判断依据
 *
 * 兜底表里每个程序位都是 `installed: false`（它是「状态未知」的占位）。
 * 调用方需要真实安装状态时应改传探测结果（`get_trae_variants`），
 * **不要**用本函数的结果去禁用控件 —— 那会把所有程序位都禁掉。
 * 本函数只提供 `variant`（回传标识）与 `label`（展示名）。
 *
 * 清单读不到（客户端没启动过 / 没登录）时不需要预先禁用：后端会回
 * `source = "missing"` 与可读原因，界面按空态呈现即可。
 */
export function regionPrograms(region: TraeRegionId): { variant: TraeVariantId; label: string }[] {
  const entry = findRegionStatus(TRAE_VARIANT_FALLBACK, region);
  return (entry?.programs ?? []).map((program) => ({
    variant: program.variant,
    label: program.label,
  }));
}

/**
 * **全部**程序位（区域 × 程序），顺序与 {@link TRAE_VARIANT_FALLBACK} 一致，
 * **按 `variant` 去重**。
 *
 * 与 {@link regionPrograms} 的分工：后者只回答「这个区域有哪几条」，
 * 本函数回答「一共有哪几个**变体**」—— 用于**不按区域分区**、而按程序位归属的控件
 * （目前是 API Key 的「归属程序位」下拉：一把 Key 绑定一个 `variant`，由它决定
 * `/v1/models` 列哪份清单、请求体带哪个 `function`）。
 *
 * ## ⚠️ 去重现在是**安全网**（2026-10-15 收敛后每区域只剩 1 个程序位）
 *
 * 历史上这里出现过「合并版 `trae` 与遗留 `trae_code` 共用变体」的缺陷，不去重会
 * 出现两个 value 完全相同的选项（`Select` 选中态指向第一个、点第二个像"没反应"）。
 * 收敛后在役程序位只有 2 个、变体互不相同 ⇒ 去重不再触发。**保留它**是因为
 * 「同一个变体被两个程序位引用」这件事一旦再次发生，症状（下拉里两个一样的选项）
 * 极难定位，而这段守卫的成本只是几行循环。
 *
 * ⚠️ 与 {@link regionPrograms} 同样：`installed` 在兜底表里恒为 `false`，
 * 不能拿它禁用控件；本函数只提供 `variant`（回传标识）与 `label`（展示名）。
 */
export function allPrograms(): { variant: TraeVariantId; label: string }[] {
  const seen = new Set<TraeVariantId>();
  const out: { variant: TraeVariantId; label: string }[] = [];
  for (const region of TRAE_VARIANT_FALLBACK) {
    for (const program of region.programs) {
      if (seen.has(program.variant)) continue;
      seen.add(program.variant);
      out.push({ variant: program.variant, label: program.label });
    }
  }
  return out;
}
