/**
 * 图片输入能力的**三态**归一（issue #101）—— 三个平台模型页共用的唯一判据。
 *
 * ## 为什么要一个独立文件，而不是各页各写一行
 *
 * 「这个模型吃不吃图」有两个来源：上游目录声明（`supports_images` / `image_input_source`）
 * 与用户在本页的手动覆盖（`get_model_image_caps`）。三处页面各自写
 * `override ?? entry.supports_images ?? null` 的话，最迟一个月后就会有一页写成
 * `?? false` —— 那一页会把「未声明」显示成「不支持」，而这两件事对用户的含义完全相反。
 *
 * ## 三态（**不是布尔**）
 *
 * | `supports` | 含义 | 客户端行为 |
 * |:--|:--|:--|
 * | `true` | 支持图片输入 | 允许上传 / 发送 `image_url` parts |
 * | `false` | 明确不支持 | 隐藏上传入口 |
 * | `null` | **未声明** | 回落到客户端自己的模型配置（本仓不替它断言） |
 *
 * ⚠️ 把 `null` 压成 `false` 就是撒谎：Trae / Qoder 的上游清单**根本没有**多模态字段，
 * 压平后整张表会变成「这些模型都不支持图片」，而网关并没有这个信息。
 */

/**
 * 能力来源：上游目录声明 / 网关内置表 / 本页手动覆盖 / 都没说。
 *
 * `known` = 上游**没有**能力字段（Trae / Qoder 的常态），但该模型在网关的内置表里
 * 已确认能吃图（issue #142）。与 `upstream` 分开显示，是为了不把「我们猜的」说成
 * 「上游说的」——后者权威、前者可被用户覆盖。
 */
export type ImageCapSource = "upstream" | "known" | "override" | "undeclared";

/** 一条模型的图片能力视图。 */
export interface ImageCapView {
  /** `true` = 支持、`false` = 明确不支持、`null` = 未声明（**不要**压成 `false`）。 */
  supports: boolean | null;
  source: ImageCapSource;
}

/** 后端模型条目里与本判据相关的两个字段（WB 目录 / 管理面视图都带）。 */
export interface ImageCapEntry {
  supports_images?: boolean | null;
  image_input_source?: ImageCapSource | null;
}

/**
 * 合成三态视图。`override` 必须是**覆盖表里的原值**（`undefined` = 没有覆盖）。
 *
 * 优先级：用户覆盖 > 上游声明 > 未声明。与后端 `model_capability::resolve` **同序**
 * （后端负责出口 ①，这里负责界面，两边必须给同一个答案）。
 */
export function resolveImageCap(
  entry: ImageCapEntry | undefined,
  override: boolean | undefined,
): ImageCapView {
  if (override !== undefined) return { supports: override, source: "override" };
  const declared = entry?.supports_images;
  if (typeof declared === "boolean") {
    // 后端已把来源算好；老响应没有该字段时按「上游」处理（那时唯一可能的来源）。
    const source: ImageCapSource =
      entry?.image_input_source === "override"
        ? "override"
        : entry?.image_input_source === "known"
          ? "known"
          : "upstream";
    return { supports: declared, source };
  }
  return { supports: null, source: "undeclared" };
}
