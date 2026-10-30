//! 跨平台抽象层：Trae 客户端探测、进程控制、以及平台能力声明。
//!
//! ## 设计原则
//!
//! 参考实现是纯 Windows：注册表查安装路径、`tasklist`/`taskkill` 管进程、
//! `certutil` 装证书、`schtasks` 注册计划任务、注册表改系统代理与 `MachineGuid`。
//! 本模块把这些能力收敛成**一组带明确成败语义的函数**，并让每个平台给出自己的实现：
//!
//! - 三平台都能做的（安装探测、进程检测/终止、客户端启动、userData 目录定位）
//!   → 各写一份等价实现；
//! - 只有特定平台能做的（注册表 `MachineGuid`、`certutil`）→ 返回
//!   [`Unsupported`]，**带上「在哪支持、为什么这里不行」的说明**。
//!
//! 绝不用一个跨平台的「假实现」来冒充成功：例如把「重置 MachineGuid」在 macOS 上
//! 实现成空操作，会让用户以为设备已隔离，而实际上游仍能关联到原设备。
//!
//! ## 为什么 Trae 客户端目录名要枚举多个候选
//!
//! Trae 有**多条可以同机并存的产品线**：`TRAE SOLO CN`、`TRAE SOLO`、`Trae CN`、
//! `Trae`。它们各有独立的安装目录、userData 目录与进程名 —— 实测同一台机器上
//! `D:\Programs\TRAE SOLO CN\` 与 `D:\Programs\Trae CN\` 就各装了一份。
//! 写死一个名字会导致两类错误：
//!
//! - 「明明装了却提示未安装」（候选表没覆盖到实际的名字或安装位置）；
//! - 「在跑却判定为未运行」（进程名只认一个），进而让切换流程在客户端运行时
//!   照常去改登录态，而客户端退出时又会把改动写回覆盖掉。
//!
//! 因此统一走候选列表 + 存在性判定；多个候选同时存在时按**最近活跃**选
//! （见 [`data_dir_names_by_activity`]），并在返回的结构里带上实际命中的路径，
//! 便于用户核对。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{json, Value};

use crate::modules::trae::paths;
use crate::modules::trae::store;

// 平台受限能力的说明形状**已上提到** [`crate::modules::capability`]：WorkBuddy 侧的
// 「打开数据目录」要用同一个形状（前端据此用同一段分支渲染），两处各拼一遍 JSON
// 迟早让字段名漂移（`supportedOn` vs `supported_on`）。这里只做转发，
// **本模块及 Trae 侧所有调用点保持原路径不变**。
pub use crate::modules::capability::Unsupported;

/// 当前平台标识。
pub fn platform_tag() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

/// 带 `CREATE_NO_WINDOW` 的 `Command`（非 Windows 上原样返回）。
///
/// Windows 上不设该标志会让每次子进程调用都闪一个控制台窗口，
/// 对一个常驻托盘的应用而言是不可接受的干扰。
pub(crate) fn hidden_command(program: &str) -> std::process::Command {
    // `mut` 只有 Windows 用得上（唯一的变异在下面的 `#[cfg(windows)]` 块里）。
    //
    // ⚠️ 不要删掉这个 `mut`、也不要给整个函数加 `allow(unused_mut)`：
    // 同一份源码要在三平台编过，而 `#[cfg_attr(not(windows), …)]` 是唯一能把豁免
    // **精确限定在非 Windows** 的写法。此前缺了它，CI 三个非 Windows job 全部
    // 因 `unused_mut` 报 `error: variable does not need to be mutable`
    // 而退出码 101（CI 的 rust 工具链 action 默认 `build-warnings: deny`，
    // 警告即硬错误 —— 本机 Windows 构建永远看不到这条）。
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut command = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

/// Trae 客户端可执行文件名候选（按优先级，**跨全部变体**）。
///
/// `Trae CN` 是与 `TRAE SOLO CN` **并列的另一条产品线**，不是它的旧名 ——
/// 两者可以同机并存（实测：`D:\Programs\TRAE SOLO CN\` 与 `D:\Programs\Trae CN\`
/// 各一份，`%APPDATA%` 下也各有一份 userData）。漏掉它就会出现
/// 「明明装了却提示未安装」，以及「装的是 Trae CN、切换的却是 SOLO CN 的登录态」。
///
/// ## 值来自**程序位表**（`region::all_program_specs`），顺序按「区域 → 程序」
///
/// 摊平口径在 2026-09-28（issue #3）从 [`TraeVariant::all`]（**只含国内两条**）
/// 换成 [`super::region::all_program_specs`]（**全部程序位**，issue #20 后为
/// 3 程序 × 2 区域 = 6 条）。
/// 换口径的直接后果：国际版 TraeCode 的 `Trae.exe` / `Trae` 仍留在候选里，
/// 却**不再**冒充国内 TraeCode 的次候选（那条静默跨区误判已修，见
/// [`super::variant::GLOBAL_TRAE_CODE_SPEC`] 与 `TRAE_CN_SPEC` 的说明）。
///
/// 顺序与改造前不再逐字相同（改造前是 `TRAE SOLO CN` → `TRAE SOLO` →
/// `Trae CN` → `Trae`，现在按区域分组）。**首项仍是 `TRAE SOLO CN`**，
/// 而它是唯一会被当作「一个候选都不存在」兜底值的元素 ⇒ 兜底行为不变；
/// 其余顺序只在「多个候选同时存在」时参与活跃度排序，而那是按 mtime 排的，
/// 与候选表顺序无关（`data_dir_names_by_activity`）。
///
/// [`TraeVariant::all`]: super::variant::TraeVariant::all
/// [`VariantSpec::exe_names`]: super::variant::VariantSpec::exe_names
#[cfg(windows)]
const EXE_NAMES: &[&str] = &[
    // ★ issue #28：跨变体 exe 候选同样收敛为**两个合并客户端**
    //   （`TRAE SOLO*` 那两条客户端已被官方合并卸载）。
    "Trae CN.exe",
    "Trae.exe",
];

/// 非 Windows：可执行文件名不带 `.exe`（macOS 是 bundle 内的裸二进制名）。
#[cfg(not(windows))]
const EXE_NAMES: &[&str] = &["Trae CN", "Trae"];

fn exe_names() -> &'static [&'static str] {
    EXE_NAMES
}

/// userData 目录名候选（与 [`exe_names`] 一一对应，首项为兜底主候选）。
///
/// 同 [`exe_names`]：值来自程序位表，这里只做摊平。
///
/// ★ `pub(crate)` 而不是私有：测试夹具要按**同一张表**铺目录，否则夹具里写死的
/// 目录名一漂，用例就会在「目录不存在」上假红（此前就栽过：`TRAE SOLO CN` 硬编码）。
pub(crate) fn data_dir_names() -> &'static [&'static str] {
    // ★ issue #28：跨变体摊平表同样收敛为**两个合并客户端**。
    //   收敛前这里还有 `TRAE SOLO CN` / `TRAE SOLO`（两条已退役的客户端），
    //   它们留在跨变体探测里会让「本机装的是哪个 Trae」答出一条**不存在的**产品线。
    &["Trae CN", "Trae"]
}

/// 取某个变体的可执行文件名候选（Windows 带 `.exe`）。
///
/// 与 [`exe_names`] 的区别：这是**单变体**视角，用于「已知目标产品线」的场景
/// （例如用户显式指定了要走 Trae Work）。[`exe_names`] 是跨变体视角，
/// 用于自动探测（不知道目标是谁，只能都试）。
pub fn exe_names_for(variant: super::variant::TraeVariant) -> &'static [&'static str] {
    let spec = super::variant::variant_spec(variant);
    if cfg!(target_os = "windows") {
        spec.exe_names
    } else {
        // 非 Windows 去掉 `.exe` 后缀：`&'static str` 无法在运行期裁掉后缀，
        // 因此这里用一个与表一一对应的静态切片。新增变体时要同步。
        match variant {
            super::variant::TraeVariant::TraeWork => &["TRAE SOLO CN", "TRAE SOLO"],
            super::variant::TraeVariant::Trae => &["Trae CN"],
            // 国际版区域：exe 名是国际版客户端自己的（`TRAE SOLO.exe`）。
            super::variant::TraeVariant::Global => &["TRAE SOLO"],
            // 国际版 TraeCode：裸名（与 `TRAE SOLO` 同款「国内带 CN 后缀」规则）。
            super::variant::TraeVariant::GlobalTraeCode => &["Trae"],
            // 合并后的客户端（issue #20）：与各自遗留线**同名**（客户端复用了目录名），
            // 但标识与快照目录各自独立。
            super::variant::TraeVariant::TraeMerged => &["Trae CN"],
            super::variant::TraeVariant::GlobalTraeMerged => &["Trae"],
        }
    }
}

/// 求**[单项变体]**的 userData 目录名候选。
pub fn data_dir_names_for(variant: super::variant::TraeVariant) -> &'static [&'static str] {
    super::variant::variant_spec(variant).data_dir_names
}

/// 在**单个变体**的候选里探测该产品线的安装（不跨变体）。
///
/// 与 [`detect_install`] 的区别：后者接受一个自定义路径并横跨全部变体按最近活跃挑，
/// 只适合回答"本机装的是哪个 Trae"。本函数回答的是"**这一条**产品线装了吗"，
/// 因为界面上要**并排**显示各程序位的独立状态。
///
/// ## ★ 自定义路径按**区域**归属判，不按变体精确相等（issue #28）
///
/// `custom` 取**该变体所属区域**的那一条（设置页按区域渲染，见 [`TraeSettings::path_for`]），
/// 归属判据因此也只能是**区域**：合并版 `Trae CN` 与遗留 TraeCode 的候选名**逐字相同**
/// （同一个客户端），原先的「文件名反查变体必须精确等于本变体」会把同一区域内的
/// 另一条程序位判成「未检测到」—— 而那条按钮已经退役、用户根本点不到，
/// 症状却是「点 Trae 提示未检测到 TraeCode CN」（issue #28 报障原文）。
///
/// ⚠️ 但**跨区域**仍然必须拦住：国内路径填进国际区域会让国际页显示成已安装
/// （实测过：国内版页签显示国际版客户端 `v1.107.1` + `…\TRAE SOLO`）。
pub fn detect_install_for(variant: super::variant::TraeVariant) -> InstallProbe {
    let settings = crate::modules::trae::settings::load();
    if let Some(custom) = settings.path_for(variant.region()) {
        let path = PathBuf::from(custom);
        // 只接受**本区域**的 Trae 客户端（含已退役的老客户端名）——
        // 「指向另一个区域」这条必须拦住，「指向本区域的另一代」则放行。
        let belongs_to_region = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(super::variant::variant_of_name)
            .map(|found| found.region() == variant.region())
            .unwrap_or(false);
        if belongs_to_region && path.is_file() {
            return InstallProbe {
                installed: true,
                version: version_from_exe(&path),
                exe: Some(path),
            };
        }
    }

    // 装配候选路径必须走平台共用 helper（[`candidate_paths_for`]）。此前这里
    // 直接调 `windows_install_roots()`，而那个函数只在 `#[cfg(windows)]` 下存在
    // —— Windows 编得过、macOS / Linux 直接
    // `E0425 cannot find function windows_install_roots in this scope`
    // （CI 的 mac-arm64 / mac-x64 / linux-x64 三个 job 就是这样红的）。
    // 三平台共用一份装配逻辑后，这类「只在 Windows 定义」的缺口不可能再出现在此处。
    // ★ macOS：候选表只覆盖「包名 == 可执行体名」的已知名字，再补一层 bundle 扫描
    //   （issue #82：macOS 上国内版 bundle 实测叫 `Trae Code.app`，候选表里没有它）。
    //   `unused_mut` 只在非 macOS 出现，CI 的 `-D warnings` 会因此变红，故显式放行。
    #[allow(unused_mut)]
    let mut candidates = candidate_paths_for(data_dir_names_for(variant), exe_names_for(variant));
    #[cfg(target_os = "macos")]
    candidates.extend(macos_bundle_scan(Some(variant.region())));
    for candidate in candidates {
        if candidate.is_file() {
            return InstallProbe {
                installed: true,
                version: version_from_exe(&candidate),
                exe: Some(candidate),
            };
        }
    }

    InstallProbe {
        installed: false,
        exe: None,
        version: None,
    }
}

/// 「该变体没有数据目录」时的**用户可读原因**。
///
/// ## 为什么必须有它（2026-09-24 用户报障原话）
///
/// 「**已经安装了，为什么检查不到安装的客户端**」—— 用户把「没有数据目录」
/// 读成了「没装客户端」。两件事的下一步**完全不同**，所以文案必须分开说：
///
/// | 实际情况 | 文案要说清 |
/// |:--|:--|
/// | 装了、但**从没启动过** | 客户端在哪（给出路径）、**为什么读不到凭证**（首次启动才写）、下一步做什么 |
/// | 真的没装 | 去装，或在「设置」里指定可执行文件路径 |
///
/// ## 判据与「启动客户端」按钮**同源**
///
/// 用的是 [`detect_install_for`] —— 与 `handlers::launch_client_for`、与界面那个按钮
/// 完全同一个函数 ⇒ 不会出现「文案说已装、按钮却报找不到可执行文件」这种自相矛盾。
///
/// ## 为什么允许在这里做磁盘探测
///
/// 它**只在错误路径上**被调用（正常登录 / 导入不会走到），换来的是用户第一次就看得懂。
/// 换成「预先算好存起来」反而会让这个值在客户端装/卸后变陈旧。
pub fn data_dir_missing_reason(variant: super::variant::TraeVariant) -> String {
    let line = variant.display_name();
    match detect_install_for(variant).exe {
        // ⚠️ 本函数产出的是**用户直接看见的文案**（toast 描述 / 错误行），不是 Markdown：
        // 任何 `**粗体**` 都会**逐字**显示成星号（2026-09-29 在 Trae 切换失败的 toast 上
        // 实测到「但它**从未启动过**」）。要强调就换用「」这类真正会渲染的标点。
        Some(exe) => format!(
            "已检测到【{line}】客户端（{}），但它从未启动过 —— 设备凭证是客户端\
             首次启动时才写入的，所以现在还读不到。请先启动一次该客户端，\
             等它起来后再重试。",
            exe.display()
        ),
        None => format!(
            "未检测到【{line}】客户端：请先安装该客户端，\
             或在「设置」里指定它的可执行文件路径，再重试。"
        ),
    }
}

/// 某一个变体的客户端是否正在运行（只读探测）。
///
/// 与 [`is_running`] 的区别：后者只回答"有没有 Trae 在跑"（任一产品线），
/// 适合"切换前必须关掉客户端"这类**阻断性**判断；本函数回答的是
/// "**这一条**产品线在跑吗"，因为界面上要给两条产品线各自的运行指示灯。
///
/// 判定只比对**该变体**的进程名，因此两条产品线可以各自显示运行状态。
pub fn is_running_for(variant: super::variant::TraeVariant) -> bool {
    #[cfg(windows)]
    {
        match hidden_command("tasklist").args(["/NH"]).output() {
            Ok(output) => {
                let listing = String::from_utf8_lossy(&output.stdout).to_lowercase();
                exe_names_for(variant)
                    .iter()
                    .any(|exe| listing.contains(&exe.to_lowercase()))
            }
            Err(_) => false,
        }
    }
    #[cfg(not(windows))]
    {
        for name in process_names_for(variant) {
            let ok = hidden_command("pgrep")
                .args(["-x", name.as_str()])
                .output()
                .map(|output| output.status.success() && !output.stdout.is_empty())
                .unwrap_or(false);
            if ok {
                return true;
            }
        }
        false
    }
}

/// 该变体的**进程名候选**：写死的候选表 + **实际探测到的可执行体名**。
///
/// ## ★ 为什么必须补上后者（issue #82）
///
/// [`exe_names_for`] 是写死的候选表（国内 = `Trae CN`）。而 macOS 上国内版的可执行体
/// 实测叫 `Trae Code`（bundle 名与可执行体名都与 Windows 不同）。只认 `Trae CN` 会让
/// [`is_running_for`] **恒为 false**，于是切换流程跳过「先关闭客户端」这一步 ——
/// 客户端退出时用内存态回写，把刚恢复的登录态覆盖掉，症状是「提示切换成功但账号没变」。
///
/// 探测到的名字排在**最前**（它是真机上真的存在的那个），候选表兜底。
#[cfg(not(windows))]
fn process_names_for(variant: super::variant::TraeVariant) -> Vec<String> {
    let mut names: Vec<String> = exe_names_for(variant)
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    let detected = detect_install_for(variant).exe.and_then(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
    });
    if let Some(detected) = detected {
        if !names.iter().any(|existing| existing == &detected) {
            names.insert(0, detected);
        }
    }
    names
}

/// **全部变体**的环境状态（线上形态，数组）。
///
/// ## 为什么需要它（与 [`env_status`] 的分工）
///
/// [`env_status`] 是**单一视角**：它返回「自动探测挑中的那一条产品线」，
/// 适合"当前在操作哪条线"的页面。但界面上要**并排**显示两条产品线的独立状态
/// （对齐 WorkBuddy 右上角的三个图标 —— 每个图标是一个独立实体、各自有状态），
/// 这时单一视角就不够了：它只会告诉你挑中的那一条，另一条压根不出现。
///
/// 本函数一次返回**全部变体**，每条都带自己的安装/运行/数据目录/版本，
/// 前端直接遍历渲染即可。**不做「最近活跃」筛选** —— 那正是要避免的语义。
///
/// `installed` 与 `dataDirExists` 分开返回：装了但从未登录过的客户端
/// 有其安装目录、却没有 userData 目录，这两种状态在界面上要区别对待。
pub fn variants_status() -> Value {
    let items: Vec<Value> = super::region::TraeRegion::all()
        .iter()
        .map(|region| region_status(*region))
        .collect();
    json!({
        "platform": platform_tag(),
        "variants": items,
    })
}

/// 单个**区域**的状态，含它每个**程序位**的探测结果。
///
/// ## 为什么形状从「按产品线列条目」改成「按区域列条目 + 条目内列程序」
///
/// 区域才是**账号体系**的分界（国内 / 国际两套互不相通的账号），而程序只是
/// 「登录态写进哪个客户端、启动谁」（见 [`super::region`] 模块头）。
/// 界面因此是「顶部选区域 → 卡片按该区域列出程序」，数据形状必须同构，
/// 否则前端就得自己把产品线轴在本地折算成区域，很容易与后端口径分叉。
fn region_status(region: super::region::TraeRegion) -> Value {
    let programs: Vec<Value> = super::region::programs_of(region)
        .iter()
        .map(|spec| {
            // ★ 全部程序位**都已建模**（2026-09-28，issue #3；2026-10-13 issue #20
            //   增合并版 Trae）：本函数不再有
            //   「未建模 ⇒ 报未安装」的分支。所以探测结果就是事实，
            //   不存在「程序位存在但拿不到标识」这种中间态。
            let target = variant_for_program(spec.region, spec.program);
            let probe = detect_install_for(target);
            let dir = select_data_dir_for(target);
            // 写侧来源（`detect_data_dir_for`：候选表里**首个存在**的目录）。
            //
            // ★ 与 `dir`（读/展示侧：**最近活跃**）**可能不是同一个目录** ——
            // 本机就是：`TRAE SOLO CN` 有登录态却更旧、更活跃的是 `TRAE SOLO`。
            // 两者语义不同（见两个函数的文档），调用方必须自己选对：
            // 「客户端最近在用哪个」用 `dataDir`；「切换器正在操作哪个 / 登录态在哪个」
            // 用 `writeDataDir`（与 `profile::overview_for` 的 `dataDir` 同源）。
            let write_dir = detect_data_dir_for(target);
            let (installed, running, version, path, data_dir, write_data_dir) = (
                probe.installed,
                is_running_for(target),
                probe.version,
                probe.exe.as_ref().map(|p| p.to_string_lossy().to_string()),
                dir.as_ref().map(|p| p.to_string_lossy().to_string()),
                write_dir
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string()),
            );
            json!({
                "program": spec.program.as_str(),
                "label": spec.display_name,
                "nameAlias": spec.name_alias,
                // 前端「切换到此程序」时回传的标识。**每个程序位都有值**
                // （`null` 只在「未建模」时代出现过，见上方说明）。
                "variant": target.as_str(),
                "installed": installed,
                "running": running,
                "version": version,
                "path": path,
                "dataDir": data_dir,
                "dataDirExists": data_dir.as_deref().map(|p| std::path::Path::new(p).is_dir()).unwrap_or(false),
                // ★ 登录态**真的在不在**（issue #136）：`dataDirExists` 只回答「目录在不在」。
                //   两者必须分开下发 —— 前端拿 `dataDirExists` 去写「已探测的登录态目录」，
                //   在「目录是空壳」的机器上会把用户引向「文件在、只是读不出」的错方向。
                "storageJsonExists": data_dir
                    .as_deref()
                    .map(|p| storage_json_present(std::path::Path::new(p)))
                    .unwrap_or(false),
                // 写侧目录（见上面 `write_data_dir` 的注释）。前端「客户端环境」行必须用它：
                // 那一行的用途是让用户核对「切换器正在操作哪个目录、登录态在哪」。
                "writeDataDir": write_data_dir,
                "writeDataDirExists": write_data_dir
                    .as_deref()
                    .map(|p| std::path::Path::new(p).is_dir())
                    .unwrap_or(false),
            })
        })
        .collect();

    // 区域级汇总：**装了任一程序**即视为该区域可用（国际版目前只装了 TraeWork）。
    let any = |key: &str| {
        programs
            .iter()
            .any(|item| item.get(key).and_then(Value::as_bool).unwrap_or(false))
    };
    let primary = programs.iter().find(|item| {
        item.get("installed")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    });
    let field = |key: &str| {
        primary
            .and_then(|item| item.get(key).cloned())
            .unwrap_or(Value::Null)
    };

    json!({
        // 区域标识（`cn` / `global`）—— 前端选区域、以及账号类接口的入参。
        "variant": region.as_str(),
        "variantLabel": region.display_name(),
        // 该区域**用户看得见的那一页**所在的域（客户端 `bootConfig.consoleHost`）。
        // 与 `variantLabel` 不同：它**不是展示名**，而是可直接拼 URL 的基址 ——
        // 前端「关于」外链、以及任何「在浏览器里打开官方站点」的动作都取它，
        // 免得前端另立第二份域常量（漏改会把国际版用户导到国内站）。
        // 单一来源：与授权页域同取自 [`super::region::region_endpoints`]。
        "consoleBase": super::region::region_endpoints(region).console_base,
        "installed": any("installed"),
        "running": any("running"),
        "version": field("version"),
        "path": field("path"),
        "dataDir": field("dataDir"),
        "dataDirExists": field("dataDirExists"),
        // 登录态文件是否真的存在（与主程序的 `storageJsonExists` 同源，见程序位里那条注释）。
        "storageJsonExists": field("storageJsonExists"),
        // 写侧目录（与主程序的 `writeDataDir` 同源）。
        // ⚠️ 与 `dataDir` **不是同一个问题**：前者是「切换器在操作哪个」，
        // 后者是「客户端最近在用哪个」。两者在本机不同值，别互相顶替。
        "writeDataDir": field("writeDataDir"),
        "writeDataDirExists": field("writeDataDirExists"),
        // 该区域下的程序位（卡片上每个账号要渲染的切换按钮）。
        "programs": programs,
    })
}

/// 程序位 → 变体（**持久化身份**）。
///
/// ★ issue #28 收敛后**每个区域只有一个程序位** ⇒ 本函数返回的就是该区域的
/// **合并版变体**（`trae_merged` / `global_trae_merged`）。
///
/// 遗留程序位（`TraeWork` / `TraeCode`）**解析到同一区域的合并版**（而不是 `None`）：
/// 它们已从「可选程序位」表里移除，但老参数、老记录仍会带着它们进来，
/// 返回 `None` 会把调用点逼出一堆兜底分支。区域决定一切、程序轴只剩一个取值 —— 这条
/// 事实在这里落一次即可。恒等映射是它的不变量，有单测钉住。
fn variant_for_program(
    region: super::region::TraeRegion,
    _program: super::region::TraeProgram,
) -> super::variant::TraeVariant {
    use super::variant::TraeVariant;
    match region {
        super::region::TraeRegion::Cn => TraeVariant::TraeMerged,
        super::region::TraeRegion::Global => TraeVariant::GlobalTraeMerged,
    }
}

/// 程序位表与 [`exe_names`] / [`data_dir_names`] 的摊平结果是否一致。
///
/// 这是防「表改了但摊平常量忘了同步」的护栏：两处写着同一份名字，
/// 一旦漂移就会出现「某个程序位的 exe 永远探测不到」这类静默故障。
/// 编译期无法校验（`cfg` 分支 + 静态切片），故放一条测试。
///
/// ## 摊平基准是**程序位表**（6 条 = 3 程序 × 2 区域），不是 [`TraeVariant::all`]（2 个）
///
/// 2026-09-28（issue #3）换的基准：`all()` 只含国内两条程序位，
/// 用它当基准会让两个国际程序位的名字（`TRAE SOLO.exe` / `Trae.exe`）
/// **在跨变体探测里凭空消失** —— 症状是「国际版客户端在跑，'关闭全部 Trae'
/// 却说没有」。程序位表才是完整口径。
///
/// ## ⚠️ 必须**按序去重**再比对（issue #20）
///
/// 表里合并版 `Trae` 与遗留 `TraeCode` 的候选名**逐字相同**（同一个客户端，
/// 见 `region` 模块头）⇒ 直接摊平会得到 6 个名字、而 `exe_names()` 只有 4 个。
/// 去重是**语义正确**的：跨变体探测要的是「本机可能叫哪些名字」的集合，
/// 重复项没有任何意义。
///
/// 但去重会让这条护栏**变松**（表里少写一条、多写一条重复项都看不出来），
/// 因此另有一条用例专门钉「合并版与遗留 TraeCode 指向同一客户端」
/// （`region::tests::合并版与遗留traecode指向同一客户端`）。
///
/// [`TraeVariant::all`]: super::variant::TraeVariant::all
#[cfg(test)]
#[test]
fn flattened_candidates_match_variant_table() {
    let mut from_table_exe: Vec<&str> = Vec::new();
    let mut from_table_dir: Vec<&str> = Vec::new();
    for spec in super::region::all_program_specs() {
        for name in spec.data_dir_names {
            if !from_table_dir.contains(name) {
                from_table_dir.push(name);
            }
        }
        for name in spec.exe_names {
            if !from_table_exe.contains(name) {
                from_table_exe.push(name);
            }
        }
    }
    // ★ issue #28 收敛后**没有任何重复候选名**（每区域一个程序位，`Trae CN` 与 `Trae`
    //   互不为子串）。断言「零重复」比断言「恰好 N 条重复」更强：
    //   有人把退役程序位塞回表里 ⇒ 立刻多出重复项 ⇒ 变红。
    assert_eq!(
        super::region::all_program_specs()
            .iter()
            .map(|s| s.exe_names.len())
            .sum::<usize>(),
        from_table_exe.len(),
        "收敛后可选程序位之间不应有重复的 exe 候选"
    );
    assert_eq!(
        data_dir_names(),
        from_table_dir.as_slice(),
        "userData 目录名候选与程序位表漂移"
    );
    // exe 名只在 Windows 上与表逐字相同（非 Windows 去过 `.exe`）。
    if cfg!(target_os = "windows") {
        assert_eq!(
            exe_names(),
            from_table_exe.as_slice(),
            "exe 名候选与程序位表漂移"
        );
    }
}

/// Windows 安装目录候选。
#[cfg(windows)]
fn windows_install_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        roots.push(PathBuf::from(&local).join("Programs"));
    }
    for key in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
        if let Ok(dir) = std::env::var(key) {
            roots.push(PathBuf::from(dir));
        }
    }
    roots.extend(secondary_drive_roots());
    roots
}

/// 非系统盘上的 `<盘符>:\Programs`。
///
/// 上面那几个环境变量**只覆盖系统盘**，而 Trae 安装器允许自选目录，装到
/// `D:\Programs\...` 是很常见的选择（实测本机就是 `D:\Programs\TRAE SOLO CN\`）。
/// 此时自动探测会一无所获、界面显示「未安装」，而用户几乎不可能想到
/// 「原因是它装在 D 盘」。
///
/// 先对 `<盘符>:\Programs` 做一次 `is_dir()` 再展开：不存在的路径判定很快，
/// 而展开后每个候选都要 `is_file()`，先过滤能把探测次数从
/// 「盘符数 × 名字数 × exe 数」压到只剩真实存在的那些。`C:` 不重复探测
/// （`%LOCALAPPDATA%\Programs` 与 `ProgramFiles` 已覆盖）。
///
/// 已知代价：盘符里若挂着**已断开的网络驱动器**，`is_dir()` 可能要等超时。
/// 这里接受该代价 —— 探测只发生在「环境状态」查询上，不是热路径。
#[cfg(windows)]
fn secondary_drive_roots() -> Vec<PathBuf> {
    (b'D'..=b'Z')
        .map(|letter| PathBuf::from(format!("{}:\\Programs", letter as char)))
        .filter(|dir| dir.is_dir())
        .collect()
}

/// 从「可执行文件所在目录」推导版本号。
///
/// Trae 是 VS Code 系 Electron 应用，`resources/app/package.json` 里带版本号。
/// 读文件比 spawn 一个 `powershell` 取 `VersionInfo` 更快、无窗口闪烁、且三平台通用。
fn version_from_install_dir(dir: &Path) -> Option<String> {
    // 候选清单按「元素个数」成对给出（Windows 布局 3 段，macOS bundle 4 段），
    // 统一转成 `&[&str]` 切片遍历，避免数组字面量长度必须一致的约束。
    let relative_candidates: &[&[&str]] = &[
        &["resources", "app", "package.json"],
        &["Contents", "Resources", "app", "package.json"],
        &["resources", "app", "product.json"],
    ];
    for relative in relative_candidates {
        let path = relative
            .iter()
            .fold(dir.to_path_buf(), |acc, part| acc.join(part));
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                if let Some(version) = value.get("version").and_then(|v| v.as_str()) {
                    let version = version.trim();
                    if !version.is_empty() && version != "0.0.0" {
                        return Some(version.to_string());
                    }
                }
            }
        }
    }
    // macOS bundle：回落到 Info.plist
    let plist = dir.join("Contents").join("Info.plist");
    if plist.is_file() {
        return crate::modules::identity::read_bundle_version(&plist);
    }
    None
}

/// 从**可执行文件路径**推导版本号。
///
/// 单独包一层，是因为 [`version_from_install_dir`] 收的是**目录**，而
/// [`detect_install`] 手上只有 exe 的完整路径。曾经直接把 exe 路径当目录传进去，
/// 于是拼出 `...\TRAE SOLO CN.exe\resources\app\package.json` 这种不可能存在的路径
/// （Windows 报 `Not a directory`），**版本号因此永远是 `None`** ——
/// 而 `None` 在界面上只表现为「不显示版本」，几乎不会有人当成 bug 报上来。
fn version_from_exe(exe: &Path) -> Option<String> {
    exe.parent().and_then(version_from_install_dir)
}

// ---------------------------------------------------------------------------
// 客户端「自述事实」：安装元数据 + 系统信息
// ---------------------------------------------------------------------------
//
// ★ 为什么单独有这一节（2026-09-24 真机缺陷的修复）
//
// 授权 URL 与 `ExchangeToken` 请求体里**同一件事实只能有一个来源**。真机抓到的
// 客户端原文（`%APPDATA%\TRAE SOLO CN\logs\<ts>\main.log` 的
// `OAuthenticator# openLogin getLoginUrl` 与 `[exchangeTokenByAuthCode] request`
// 两行）显示上游会比对下面这几对字段，而本实现此前**每一对都自相矛盾**：
//
// | 同源对 | 授权 URL | 兑换请求体 | 修复前 |
// |:--|:--|:--|:--|
// | 机器标识 | `machine_id` / `x_machine_id` | `DeviceInfo.MachineID` | URL 用**自造**值、请求体用 `telemetry.machineId` |
// | 应用版本 | `x_app_version` | `ClientVersion` / `IDEVersion` | URL 用 IDE 线常量、请求体用安装目录版本 |
// | 设备型号 | `x_device_brand` | `DeviceModel` | URL 用主机名、请求体空串 |
// | 系统版本 | `x_os_version` | `OSVersion` | URL 写死 `Windows`、请求体空串 |
//
// 上游对不一致的回应是 `20403/040036: Token device not match`。

/// 客户端安装根目录 `manifest.json` 里、**上游请求要用到**的那几个取值。
///
/// ## 为什么不能复用 [`InstallProbe::version`]
///
/// `InstallProbe::version` 读的是 `resources/app/package.json` 的 `version`
/// （Electron/VS Code 内核版本，本机 `1.107.1`）—— 界面展示用它是对的。
/// 但客户端在 `x_app_version` / `DeviceInfo.ClientVersion` / `IDEVersion`
/// 三处报的是**安装包版本**（`manifest.json` → `appVersion`，本机 `0.1.69`）。
/// 两者不是一回事，混用会让「URL 与请求体」这一对同源字段不一致。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientInstallMeta {
    /// `manifest.json` → `appVersion`（本机 `0.1.69`）。
    pub app_version: String,
    /// `manifest.json` → `buildVersion`（本机 `2.3.87413`，即授权 URL 的 `plugin_version`）。
    pub build_version: String,
    /// `manifest.json` → `channel`（本机 `stable`，即授权 URL 的 `x_app_type`）。
    pub channel: String,
}

/// 读该变体客户端的安装元数据。
///
/// 安装目录由 [`detect_install_for`] 定位 —— 与「启动客户端」按钮、界面上的
/// 「已检测到 vX」**同源**，不另立第二份探测逻辑。
/// 文件缺失或字段为空 ⇒ `None`，由调用方决定回落值（不在这里编默认值：
/// 编出来的值会静默进入授权 URL，而登录失败时没人看得出它来自哪里）。
pub fn client_install_meta_for(variant: super::variant::TraeVariant) -> Option<ClientInstallMeta> {
    let exe = detect_install_for(variant).exe?;
    let manifest = exe.parent()?.join("manifest.json");
    let text = std::fs::read_to_string(&manifest).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let field = |key: &str| {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    Some(ClientInstallMeta {
        app_version: field("appVersion").unwrap_or_default(),
        build_version: field("buildVersion").unwrap_or_default(),
        channel: field("channel").unwrap_or_default(),
    })
}

/// 客户端在上游眼里的「系统信息」。
///
/// 真机取值来自客户端的 `iCubeSystemInformationService`（`deviceModel` /
/// `deviceManufacturer` 取自 aha 原生模块，`osName` / `osVersion` 取自 Node 的
/// `os.platform()` / `os.version()`）。本实现复刻同一批值，见 [`system_profile`]。
///
/// ★ 这里每个字段都**同时**喂给授权 URL 与 `DeviceInfo` —— 两处若各取一份来源，
/// 就是 2026-09-24 那个 `20403` 的形态。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemProfile {
    /// `DeviceInfo.DeviceName`。真值是「Windows 账户全名 + 本地化后缀」（本机
    /// `Jackey的电脑`）——**本实现留空**，理由见 [`system_profile`]。
    pub device_name: String,
    /// `x_device_brand` 与 `DeviceInfo.DeviceModel`（本机 `System Product Name`）。
    pub device_model: String,
    /// `DeviceInfo.DeviceBrand`（本机 `ASUS`）。
    pub device_manufacturer: String,
    /// `DeviceInfo.DeviceCPU`（本机 `Intel(R) Core(TM) i9-14900K`）。
    pub cpu_brand: String,
    /// `x_device_type` 与 `DeviceInfo.OSInfo`（`windows`）。
    pub os_name: String,
    /// `x_os_version` 与 `DeviceInfo.OSVersion`（本机 `Windows 11 Pro`）。
    pub os_version: String,
}

/// 读本机系统信息。**进程内只读一次**：它不随变体变化。
///
/// ## ⚠️ 已知缺口：四个「描述性」字段留空（有意，不是漏改）
///
/// 真机客户端（`TRAE SOLO CN`，2026-09-24 实测）的取值来自 aha 原生模块与 Node：
///
/// | 字段 | 真机值 | 本实现 |
/// |:--|:--|:--|
/// | `device_model` | `System Product Name` | 空 |
/// | `device_manufacturer` | `ASUS` | 空 |
/// | `cpu_brand` | `Intel(R) Core(TM) i9-14900K` | 空 |
/// | `os_version` | `Windows 11 Pro` | 空 |
///
/// 它们在 Windows 上分别来自注册表 `HKLM\HARDWARE\DESCRIPTION\System\BIOS`、
/// `…\CentralProcessor\0` 与 `…\SOFTWARE\Microsoft\Windows NT\CurrentVersion`
/// （最后一项还要复刻 Node `os.version()` 的「按内部版本号纠正主版本」行为）。
///
/// **本轮刻意不读注册表**：读它要么起 `reg.exe` 子进程（开发沙箱把 `reg.exe`
/// 列进了程序黑名单，直接被拒），要么给 `windows` crate 加
/// `Win32_System_Registry` 特性 —— 两条路都无法在本次会话内实测，而把
/// 「无法实测的 IO」放进登录路径正是本轮要修的毛病。
///
/// **留空是安全的**：这四个字段在授权 URL 与兑换请求体里**取同一个值**
/// （URL 的 `x_device_brand` / `x_os_version` 与请求体的 `DeviceModel` /
/// `OSVersion` 都从这里取），所以「两处不一致」这个已证实的缺陷不会因留空而复发。
/// 若后续实测发现上游会拿它们与设备注册记录比对，再按上表的注册表路径补齐
/// —— 优先用 `windows` crate 的 `RegGetValueW` 进程内读取，不起子进程。
pub fn system_profile() -> SystemProfile {
    static CACHE: std::sync::OnceLock<SystemProfile> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| SystemProfile {
            // 真值是「Windows 账户全名 + 本地化后缀」：客户端跑 `net user <user>` 取
            // `Full Name`，再拼它自己的本地化文案（本机 = `Jackey` + `的电脑`）。
            // 后缀随客户端语言变化、没有稳定来源 ⇒ 留空（填错比留空更坏）。
            // 该字段**不在授权 URL 里**，不参与「URL ↔ 请求体」同源比对。
            device_name: String::new(),
            device_model: String::new(),
            device_manufacturer: String::new(),
            cpu_brand: String::new(),
            // 客户端在 macOS 上报的是 `mac`（`os.platform()==="darwin" ? "mac" : …`）。
            os_name: match std::env::consts::OS {
                "macos" => "mac".to_string(),
                other => other.to_string(),
            },
            os_version: String::new(),
        })
        .clone()
}

/// 探测结果。
#[derive(Debug, Clone)]
pub struct InstallProbe {
    /// 是否已安装。
    pub installed: bool,
    /// 可执行文件路径。
    pub exe: Option<PathBuf>,
    /// 版本号。
    pub version: Option<String>,
}

/// 在给定候选路径中探测 Trae 客户端。
///
/// `custom` 为用户在设置里显式指定的路径列表（**按区域各一条**，见
/// `TraeSettings::all_paths`），**优先级最高**、按传入顺序取第一个真实存在的文件。
///
/// ## 为什么是**列表**而不是单个值（issue #20）
///
/// 设置里的客户端路径已按区域分家。本函数回答的是「**本机装的是哪一条** Trae」，
/// 横跨全部区域 —— 因此必须把每个区域配置的路径都试一遍，只看一条会漏掉另一区域。
///
/// 自动探测（[`candidate_exe_paths`]）覆盖 `%LOCALAPPDATA%\Programs`、三个
/// `ProgramFiles` 变体，以及非系统盘上的 `<盘符>:\Programs` —— 最后一项是必需的：
/// Trae 安装器允许自选目录，装到 `D:\Programs\...` 很常见，而那几个环境变量
/// 只覆盖系统盘，漏掉就会表现为「明明装了却提示未安装」。
///
/// 仍然覆盖不到的只剩「装在任意自定义目录」（例如 `D:\Apps\Trae\`），
/// 该场景由 `custom` 兜底，且报错文案会主动提示这一点。
pub fn detect_install(custom: &[String]) -> InstallProbe {
    for raw in custom {
        let path = raw.trim();
        if path.is_empty() {
            continue;
        }
        let path = PathBuf::from(path);
        if path.is_file() {
            return InstallProbe {
                installed: true,
                version: version_from_exe(&path),
                exe: Some(path),
            };
        }
    }

    // ★ macOS bundle 扫描（issue #82）：跨变体场景不过滤区域，名字含 `trae` 即收。
    #[allow(unused_mut)]
    let mut candidates = candidate_exe_paths();
    #[cfg(target_os = "macos")]
    candidates.extend(macos_bundle_scan(None));
    for candidate in candidates {
        if candidate.is_file() {
            return InstallProbe {
                installed: true,
                version: version_from_exe(&candidate),
                exe: Some(candidate),
            };
        }
    }

    // 刻意**不查询注册表卸载项**：
    //
    // 1. 参考实现用 `reg query /s /f TRAE` 兜底，代价是每次探测都起一个子进程
    //    （Windows 上还会有控制台窗口闪烁），且在受限环境（沙箱、企业策略）里会被拦截；
    // 2. [`candidate_exe_paths`] 已覆盖全部**常见**安装位置（`%LOCALAPPDATA%\Programs`、
    //    `ProgramFiles` / `ProgramFiles(x86)` / `ProgramW6432`，以及非系统盘的
    //    `<盘符>:\Programs`）；
    // 3. 真正需要注册表兜底的场景只剩「装在任意自定义目录」（例如 `D:\Apps\Trae\`），
    //    而该场景已有**更好的**手段：用户在设置里显式指定 `trae_path`
    //    （见本函数开头的 `custom` 分支），且报错文案会主动提示这一点。
    //
    // 换句话说，注册表兜底换来的是「极少数用户少点一次设置」，代价是
    // 「所有用户每次探测都付一次子进程」。这里选择不付。
    InstallProbe {
        installed: false,
        exe: None,
        version: None,
    }
}

/// 按平台装配「安装根目录 × 目录名 × exe 名」的候选路径（**三平台共用一份**）。
///
/// 调用方只负责给出**已按优先级/活跃度排序**的目录名与 exe 名候选，本函数负责各平台的
/// **路径形状**：
///
/// - Windows：`%LOCALAPPDATA%\Programs`、`ProgramFiles*` 与非系统盘 `<盘符>:\Programs`
///   下的 `<name>\<exe>`（roots 由 [`windows_install_roots`] 提供）；
/// - macOS：`/Applications`、`/System/Applications`、`~/Applications` 下的
///   `<name>.app/Contents/MacOS/<exe>`；
/// - Linux：`/opt`、`/usr/local`、`/usr/share`、`~/.local/share` 下的 `<name>/<exe>`。
///
/// ## 为什么必须共用（而不是各写一份）
///
/// 本函数的前身是 `candidate_exe_paths()`（只被跨变体探测使用），而**单项变体探测
/// [`detect_install_for`] 另写了一份、并且直接调用 `#[cfg(windows)]` 的
/// [`windows_install_roots`]**。于是留下「Windows 编得过、macOS/Linux 编不过」的缺口 ——
/// 本机全在 Windows 上开发，本地构建**永远发现不了**（CI 的 mac-arm64 / mac-x64 /
/// linux-x64 三个 job 因此在 `Build server binary` 报
/// `E0425 cannot find function windows_install_roots`，退出码 101）。
///
/// 现在平台分支集中在这一处，调用方不认识平台细节：
/// 新增「按变体探测安装」之类的需求时，直接复用本函数即可，不会再复制出一份
/// 只在 Windows 存在的实现。
///
/// ## 循环顺序（保持既有行为，勿随意调整）
///
/// Windows 上**目录名放外层**：名字已按「最近活跃」排序
/// （见 [`data_dir_names_by_activity`]），放外层才能让「活跃产品」的安装路径整体优先于
/// 「非活跃产品」的；若把安装根目录放外层，一个装在 C 盘的非活跃产品会盖过装在 D 盘的
/// 活跃产品，于是出现「切换器管的是我没在用的那个 Trae」。
/// macOS / Linux 沿用「系统目录在前、用户目录在后」的既有顺序。
fn candidate_paths_for(names: &[&'static str], exes: &[&'static str]) -> Vec<PathBuf> {
    let mut out = Vec::new();

    #[cfg(windows)]
    {
        // 提到循环外：`windows_install_roots` 会做一轮盘符探测，不必每个名字都重算。
        let roots = windows_install_roots();
        for &name in names {
            for root in &roots {
                for &exe in exes {
                    out.push(root.join(name).join(exe));
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        for root in ["/Applications", "/System/Applications"] {
            for &name in names {
                for &exe in exes {
                    out.push(macos_bundle_exe(Path::new(root), name, exe));
                }
            }
        }
        if let Some(home) = dirs::home_dir() {
            for &name in names {
                for &exe in exes {
                    out.push(macos_bundle_exe(&home.join("Applications"), name, exe));
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        for root in ["/opt", "/usr/local", "/usr/share"] {
            for &name in names {
                for &exe in exes {
                    out.push(PathBuf::from(root).join(name).join(exe));
                }
            }
        }
        if let Some(home) = dirs::home_dir() {
            for &name in names {
                for &exe in exes {
                    out.push(home.join(".local").join("share").join(name).join(exe));
                }
            }
        }
    }

    // 既非 Windows 也非 macOS/Linux：显式吞掉参数，免得在 `-D warnings` 的 CI 上
    // 因「未使用变量」把整个 job 打成红的（本仓 CI 的 rust 工具链 action 默认
    // `build-warnings: deny`）。返回空列表 = 探测不到，不伪造任何命中。
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = (names, exes);
    }

    out
}

/// macOS 的安装是 `.app` bundle，可执行文件在 `Contents/MacOS/` 下。
#[cfg(target_os = "macos")]
fn macos_bundle_exe(root: &Path, name: &str, exe: &str) -> PathBuf {
    root.join(format!("{name}.app"))
        .join("Contents")
        .join("MacOS")
        .join(exe)
}

/// macOS：**扫描 `.app` bundle 目录**，用 `Info.plist` 的 `CFBundleExecutable` 定位可执行体。
///
/// ## ★ 为什么必须有这一层（issue #82「trae 国内版切换账号未检测到」）
///
/// 上表的候选路径是 `<包名>.app/Contents/MacOS/<包名>` —— 它**假设包名 == 可执行体名**。
/// 本仓在别处已经吃过这个假设的亏（[`crate::modules::identity::read_bundle_executable`]
/// 的文档：两个 WorkBuddy bundle 的可执行体都是 `Electron`，按包名拼出来的路径不存在，
/// 且该错误表现为「客户端未安装」而不是「启动失败」）。
///
/// macOS 上国内版 Trae 的 bundle 实测叫 **`Trae Code.app`**，而候选表里只有
/// `Trae CN.app` / `Trae.app` ⇒ 探测恒为 `installed = false` ⇒ 账号卡切换胶囊、
/// 空态卡、设置页「安装」行全部显示「未检测到」。这条一直是错的，只是 macOS 用户少。
///
/// 这一层**不认识任何具体名字**：枚举 `/Applications`、`/System/Applications`、
/// `~/Applications` 下名字含 `trae` 的 `.app`，读 plist 拿真实可执行体名。
/// 客户端下次改名/换 bundle 也不需要再改代码。
///
/// `region` 为 `Some` 时**按区域过滤**（跨区域必须拦住：`/Applications/Trae.app`
/// 是国际版，不能因为「名字含 trae」就算国内版已安装 —— 同 [`detect_install_for`]
/// 自定义路径分支的纪律）。
#[cfg(target_os = "macos")]
fn macos_bundle_scan(region: Option<super::region::TraeRegion>) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }

    let mut out: Vec<PathBuf> = Vec::new();
    for root in roots {
        // 目录不存在 / 无权限读 ⇒ 跳过这一层，不当作错误（探测必须永不 panic）。
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let bundle = entry.path();
            if bundle.extension().and_then(|ext| ext.to_str()) != Some("app") {
                continue;
            }
            let Some(name) = bundle.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if !name.to_ascii_lowercase().contains("trae") {
                continue;
            }
            if let Some(wanted) = region {
                let belongs = super::variant::variant_of_name(name)
                    .map(|found| found.region() == wanted)
                    // 反查不到（全新名字）：**放行** —— 名字含 `trae` 已经是强信号，
                    // 拦掉会让「客户端改名」重新变回「未检测到」，正是本层要解决的问题。
                    .unwrap_or(true);
                if !belongs {
                    continue;
                }
            }
            let exe = crate::modules::identity::read_bundle_executable(
                &bundle.join("Contents").join("Info.plist"),
            )
            .unwrap_or_else(|| name.to_string());
            let candidate = bundle.join("Contents").join("MacOS").join(exe);
            if candidate.is_file() {
                out.push(candidate);
            }
        }
    }
    // 排序只为了让结果**确定**（多份并存时行为可复现，便于测试与排查）。
    out.sort();
    out
}

/// **全部变体**的可执行文件候选路径（跨平台）。
///
/// 平台差异全部收在 [`candidate_paths_for`]；本函数只负责把「按最近活跃排序的
/// 跨变体目录名」与 exe 名候选喂进去。
fn candidate_exe_paths() -> Vec<PathBuf> {
    candidate_paths_for(&data_dir_names_by_activity(), exe_names())
}

/// 定位 Trae 客户端的 userData 目录（**跨变体、最近活跃**；环境自检用）。
///
/// 多个渠道的 userData **可以同时存在**（实测同一台机器上 `TRAE SOLO CN` 与
/// `Trae CN` 各有一份）。此时按**最近活跃**选，而不是按候选表顺序取第一个 ——
/// 否则用户在用的是 Trae CN，切换器却去改 SOLO CN 的登录态，且界面上看不出任何异常。
///
/// 全部不存在时返回主候选（用于 UI 展示「预期位置」），而不是 `None`——
/// 否则用户看不到「应该把客户端数据放在哪」。
///
/// **注意**：本函数的候选列表横跨全部变体，只适合"环境自检"。
/// 要按产品线取目录必须用 [`detect_data_dir_for`]（**写侧来源**）
/// 或 [`select_data_dir_for`]（**读 / 展示侧**）—— 两者语义不同，见各自文档。
pub fn detect_data_dir() -> Option<PathBuf> {
    let base = data_dir_base()?;
    Some(base.join(data_dir_names_by_activity()[0]))
}

/// 定位**指定变体**的 userData 目录（**写侧来源**）。
///
/// ## 唯一语义（R4：两个选择器不得都自称「登录态所在处」）
///
/// 本函数是**写**侧的唯一来源：[`crate::modules::trae::profile::backup_to_slot_for`]
/// 从它复制、`restore_from_slot_in_dir` 的调用方向它写入、保存守卫从它取证。
/// 因此它必须**确定**（不依赖活跃度），否则同一台机器上「写入」与「校验」会各自漂移。
///
/// 取值规则：**候选表里第一个存在的目录**（`is_dir`）；一个候选都不存在时，
/// 才回落到主候选名 `names[0]`，**仅作「应该放在哪」的展示值**。
///
/// ## 【R3】为什么不能恒取 `names[0]`
///
/// 曾恒返回 `base.join(names[0])`、**完全不查存在性**。在只装了 `TRAE SOLO`
/// （没有 `TRAE SOLO CN`）的机器上，它会返回一个**不存在**的路径，
/// 备份 / 恢复 / 守卫全部落空 —— 症状是「Trae 明明在用，切换器却说找不到数据目录」。
///
/// ## 想要别的语义时，用别的函数（不要改这里）
///
/// - 「这条产品线**最近被用过**的是哪个目录」→ [`select_data_dir_for`]；
/// - 「本机**跨变体**最常用的那条产品线」→ [`detect_data_dir`]（环境自检）。
///
/// ⚠️ **纯展示不要用本函数**：它回答的是「写哪」，不是「用户最常用哪个」。
pub fn detect_data_dir_for(variant: super::variant::TraeVariant) -> Option<PathBuf> {
    let base = data_dir_base()?;
    let names = data_dir_names_for(variant);
    // 候选表顺序（不是活跃度顺序）：写侧必须确定，不能被「谁更活跃」左右。
    names
        .iter()
        .map(|name| base.join(name))
        .find(|dir| dir.is_dir())
        // 一个都不存在 ⇒ 回落主候选名，仅供 UI 展示「应该放在哪」。
        .or_else(|| Some(base.join(names[0])))
}

/// 在**单个变体**的候选目录里挑**最近活跃**的那个（**读 / 展示侧**）。
///
/// ## 唯一语义（R4：两个选择器不得都自称「登录态所在处」）
///
/// 本函数回答的是「这条产品线**最近被用过**的那个目录在哪」，用于**展示**与自动探测。
/// 它**不是**写侧来源 —— 写侧是 [`detect_data_dir_for`]。两者语义不同，
/// **同一台机器上可能给出不同目录**（实测 Trae Work：`TRAE SOLO CN` 有登录态却更旧、
/// `TRAE SOLO` 更活跃），且**不保证同值**。任何「校验的对象必须与操作的对象同源」
/// 的场合都必须走 [`detect_data_dir_for`]（或 profile 里的唯一取值点），不得用本函数。
///
/// ## 排序实现只有一处（消除漂移）
///
/// 排序由 [`data_dirs_by_activity_for`] 提供（活跃度降序、只含**存在**的候选），
/// 与全局视角 [`data_dir_names_by_activity`] 是**同一套规则**。
/// 本函数**不再自己写一遍排序** —— 两套实现会随维护漂移，正是本模块曾经的病根之一。
///
/// 该变体一个候选目录都不存在时返回 `None` —— 调用方据此产出「未找到【Trae Work】的
/// 数据目录，请先启动一次该客户端」这类**指向该变体**的错误，而不是含糊的「未检测到」。
///
/// 返回 `None` 不区分「base 取不到」与「候选都不存在」：两者对调用方的处置相同
/// （都提示该变体没有数据目录），且后者是唯一可操作的情形。
pub fn select_data_dir_for(variant: super::variant::TraeVariant) -> Option<PathBuf> {
    data_dirs_by_activity_for(variant).into_iter().next()
}

/// 该变体**存在**的候选目录，按「最近活跃」降序。
///
/// 与 [`data_dir_names_by_activity`] 共用**同一套排序规则**（活跃度降序、稳定排序保持
/// 候选表原顺序），区别只有两点：**按变体限定**、且**只保留存在的目录**。
/// [`select_data_dir_for`] 取它的首项，因此「按变体选活跃目录」与「跨变体选活跃目录」
/// 永远不会因为两套排序实现而漂移。
///
/// 排序规则：有活跃时间的在前、新的在前；取不到活跃时间的垫底
/// （`Option` 的 `Ord` 里 `None < Some(_)`，故反转比较）。
///
/// 供 [`select_data_dir_for`] 取首项；`icube` 侧**遍历全部候选**时也用它
/// （[`crate::modules::trae::icube::device_secret_by_device_id`] 按 `deviceId` 精确取、
/// [`crate::modules::trae::icube::login_state_dir_for`] 找「装着登录态的那个目录」）——
/// 这两个场景都需要「按变体限定的全部存在候选」，而不是单个目录。
pub(crate) fn data_dirs_by_activity_for(variant: super::variant::TraeVariant) -> Vec<PathBuf> {
    let Some(base) = data_dir_base() else {
        return Vec::new();
    };
    let mut scored: Vec<(PathBuf, Option<SystemTime>)> = data_dir_names_for(variant)
        .iter()
        .map(|name| base.join(name))
        .filter(|dir| dir.is_dir())
        .map(|dir| {
            let activity = data_dir_activity(&dir);
            (dir, activity)
        })
        .collect();

    scored.sort_by_key(|s| std::cmp::Reverse(s.1));
    scored.into_iter().map(|(dir, _)| dir).collect()
}

/// 按「最近活跃」排序的 userData 候选名。
///
/// 排序规则：**存在且活跃时间新**的排前面；目录不存在的排在后面，且彼此保持候选表
/// 原顺序（`sort_by` 是稳定排序）。因此当没有任何候选存在时，结果与
/// [`data_dir_names`] 完全一致 —— 兜底返回值仍然是主候选 `TRAE SOLO CN`。
fn data_dir_names_by_activity() -> Vec<&'static str> {
    let base = data_dir_base();
    let mut scored: Vec<(&'static str, Option<SystemTime>)> = data_dir_names()
        .iter()
        .map(|name| {
            let activity = base
                .as_ref()
                .map(|base| base.join(name))
                .filter(|dir| dir.is_dir())
                .and_then(|dir| data_dir_activity(&dir));
            (*name, activity)
        })
        .collect();

    // 降序：`Option` 的 `Ord` 里 `None < Some(_)`，所以反过来比就是
    // 「有活跃时间的在前、新的在前、没有的垫底」。
    scored.sort_by_key(|s| std::cmp::Reverse(s.1));
    scored.into_iter().map(|(name, _)| name).collect()
}

/// userData 目录里 `storage.json` 的**唯一**拼法（设备凭证 / 登录态都在这个文件里）。
///
/// 抽成函数是为了让「活跃度判据」「登录态存在性判据」「读取路径」共用同一份拼装 ——
/// 三处各写一遍，改一处就会让「目录算活跃」与「说它有登录态」指向不同的文件。
///
/// 逐段 `join` 而**不是**一次 `join("User/globalStorage/storage.json")`：后者在 Windows 上
/// 会拼出 `…\Trae CN/User/globalStorage/storage.json` 这种混合分隔符，虽然能打开，
/// 但错误文案要把它原样显示给用户，混合分隔符会让人怀疑「路径是不是拼错了」。
pub fn storage_json_path(dir: &Path) -> PathBuf {
    dir.join("User").join("globalStorage").join("storage.json")
}

/// 该 userData 目录里是否**真的存在** `storage.json`。
///
/// ## 为什么必须与 `is_dir()` 分开（issue #136）
///
/// 目录存在 ≠ 登录态存在。真机报障现场：`%APPDATA%\Trae CN` 目录存在（客户端启动过、
/// 写过 Cache / Local Storage），但 `User\globalStorage\storage.json` 从未生成 ——
/// 此时 `dataDirExists = true`，界面就把**一个没有凭据的目录**说成「已探测的登录态目录」，
/// 后端则在读文件那一步抛 `os error 2`，文案还劝用户「关闭客户端后重试」。
/// 用户照着做了十几轮，问题与他的动作完全无关。
///
/// 判据用 `is_file()` 而不是 `exists()`：同名目录也能让 `exists()` 为真，
/// 而那同样读不出内容。
pub fn storage_json_present(dir: &Path) -> bool {
    storage_json_path(dir).is_file()
}

/// 本机**其它** Trae userData 目录里是否有一份 `storage.json`（「你可能装的是另一条线」的线索）。
///
/// ## 为什么需要它（issue #136）
///
/// 用户报障「网页版登录、客户端登录都捕捉不到凭证」。真机上常见的两种成因**看起来
/// 一模一样**（本变体目录缺 `storage.json`）：
///
/// 1. 这条产品线的客户端确实没启动过；
/// 2. 用户装的是**另一条**产品线的客户端（国内 `Trae CN` / 国际 `Trae` 是两个独立客户端、
///    两套 userData），而界面停在了错的区域页上。
///
/// 第 2 种光靠「请启动一次客户端」永远好不了 —— 用户明明启动过、也登录过。
/// 本函数把「别处有凭据」这条事实取出来，让文案能直接指向真正的出路
/// （切区域 / 装那条线），而不是让用户反复重启。
///
/// ## ★ 必须用 `all_known_specs()`，不能用 [`data_dir_names`]
///
/// [`data_dir_names`] 是**收敛后**的摊平表（只剩 `Trae CN` / `Trae` 两个合并客户端），
/// 而用户机器上**已退役客户端的 userData 目录依然在磁盘上**（卸载客户端不删 userData）。
/// 只扫收敛表就会漏掉「凭据在 `TRAE SOLO CN` 里」这种最常见的情形 —— 那正好是
/// 「我明明登录过」的用户。`all_known_specs()` 含全部 6 个程序位的目录名，才是完整口径。
///
/// 返回**目录路径**（不含文件名），取规格表顺序里第一个命中的；都没有则 `None`。
/// 只读探测，不创建任何目录。
pub fn other_line_storage_hint(variant: super::variant::TraeVariant) -> Option<String> {
    let base = data_dir_base()?;
    let own = data_dir_names_for(variant);
    let mut seen: Vec<&str> = Vec::new();
    for spec in super::variant::all_known_specs() {
        for name in spec.data_dir_names {
            // 排除本变体自己的目录（那是「本该有却没有」的那个），并按名去重
            // （合并版与遗留 TraeCode 的候选名逐字相同，扫两遍没有信息增益）。
            if own.iter().any(|mine| mine == name) || seen.contains(name) {
                continue;
            }
            seen.push(name);
            let dir = base.join(name);
            if storage_json_present(&dir) {
                return Some(dir.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// userData 目录的「最近活跃时间」。
///
/// **刻意不直接用目录自身的 mtime**：Electron 的 userData 目录只在增删顶层条目时
/// 更新 mtime，而登录态文件是「内容一变就写」，后者才真正反映「这个客户端最近被用过」。
/// 取一组启动/登录必写文件里最新的那个 mtime；一个都没有时回落到目录 mtime；
/// 都拿不到则返回 `None`（该候选按「从未使用」处理）。
fn data_dir_activity(dir: &Path) -> Option<SystemTime> {
    // 每次启动或登录都会被写到的文件，跨渠道/版本都比较稳定。
    // `storage.json` 单独走 [`storage_json_path`]（唯一拼法），其余在这里。
    const MARKERS: &[&str] = &[
        "aha/TinyStorage",
        "Local Storage",
        "Network/Cookies",
        "machineid",
    ];

    MARKERS
        .iter()
        .map(|marker| dir.join(marker))
        .chain(std::iter::once(storage_json_path(dir)))
        .filter_map(|path| std::fs::metadata(path).ok())
        .filter_map(|meta| meta.modified().ok())
        .max()
        .or_else(|| {
            std::fs::metadata(dir)
                .ok()
                .and_then(|meta| meta.modified().ok())
        })
}

/// userData 的父目录（平台约定）。
///
/// ## 覆盖优先级：`APPDATA` → 平台默认
///
/// `APPDATA` 在 Windows 上本来就是平台约定（`...\AppData\Roaming`）；在 macOS / Linux 上
/// 它**不是**约定变量，但它是**唯一**能被测试注入「userData 父目录」的口子
/// （[`super::test_support::TempEnv`] 同时隔离 `BUDDY_SWITCH_HOME` 与 `APPDATA`，
/// fixture 就铺在 `APPDATA` 下）。
///
/// 不认它的时候，macOS / Linux 上的用例会去读**真机**客户端目录：
/// 真机恰好登着账号 ⇒ 断言红；失败输出里还会带上真机的 `userId` / `deviceId`。
/// 这就是 issue #37 的 5 条失败（`profile::overview_*` / `oauth::*`）。
/// 更危险的是写路径（`restore_from_slot_for` → `snapshot_data_dir_for`）同样经过本函数，
/// 用例一旦走到那里就会**真的写进真机目录** —— 所以这里按「根因」修，而不是逐个用例打补丁。
fn data_dir_base() -> Option<PathBuf> {
    // 覆盖优先：三平台同一条链。真机 macOS / Linux 上没有 `APPDATA`，这一支不会生效；
    // Windows 上这一支就是原来的行为（环境变量优先于 `dirs::config_dir()` 兜底）。
    let overridden = std::env::var("APPDATA")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from);
    if overridden.is_some() {
        return overridden;
    }

    #[cfg(windows)]
    {
        // 缺失时用 `dirs::config_dir()`（Windows 上就是 `%APPDATA%`，
        // 即 `C:\Users\<用户>\AppData\Roaming`）。
        //
        // 这层兜底不多余：`APPDATA` 属于「通常有、但不保证有」的变量，
        // 某些终端、CI、被裁剪过的启动环境里就是没有。缺了它 `detect_data_dir()`
        // 会直接返回 `None`，表现成「登录态快照与账号切换整体不可用」，
        // 而根因仅仅是取不到一个目录 —— 排查成本极高。
        dirs::config_dir()
    }
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|home| home.join("Library").join("Application Support"))
    }
    #[cfg(target_os = "linux")]
    {
        // XDG 规范优先，其次 ~/.config
        std::env::var("XDG_CONFIG_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join(".config")))
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// 某个**程序位**（区域 × 程序）的 userData 目录：`%APPDATA%\<data_dir_names[0]>`。
///
/// 与 [`detect_data_dir_for`] 的区别（**不可互换**）：本函数按
/// [`super::region::program_spec`] 精确取**该程序位自己声明的唯一目录名**，
/// **不做候选回落**；而 `detect_data_dir_for` 走的是**变体**候选表，
/// `TRAE_WORK` 那张表里混着 `TRAE SOLO`（**国际版客户端**），缺席时会静默落到它 ——
/// 取「这个程序位的 settings.json 在哪」必须用本函数。
///
/// 目录**不保证存在**；调用方据 `is_file()` / `is_dir()` 判「是否已安装」。
pub fn user_data_dir_for_program(
    region: super::region::TraeRegion,
    program: super::region::TraeProgram,
) -> Option<PathBuf> {
    let base = data_dir_base()?;
    let spec = super::region::program_spec(region, program);
    let name = *spec.data_dir_names.first()?;
    Some(base.join(name))
}

// 这里刻意**没有**「Trae 客户端进程名」这样的单值函数。
// `TRAE SOLO CN` 与 `Trae CN` 是可以并存的两条产品线，进程名、安装目录、
// userData 目录各有一套，因此一律以 [`exe_names`] 的候选列表为准。

/// 当前正在运行的 Trae 候选进程（Windows）。
///
/// 只调一次 `tasklist` 拿到全量列表再逐个名字比对，而不是「每个候选名各调一次
/// `tasklist /FI IMAGENAME eq ...`」—— 后者在装了多个渠道时要起 4 个子进程，
/// 这里只起 1 个。
#[cfg(windows)]
fn running_processes() -> Vec<&'static str> {
    match hidden_command("tasklist").args(["/NH"]).output() {
        Ok(output) => {
            let listing = String::from_utf8_lossy(&output.stdout).to_lowercase();
            exe_names()
                .iter()
                .copied()
                .filter(|exe| listing.contains(&exe.to_lowercase()))
                .collect()
        }
        Err(_) => Vec::new(),
    }
}

/// 当前正在运行的**指定变体**候选进程（Windows）。
///
/// 与 [`running_processes`] 的区别：只比对传入变体的进程名。用于按变体结束客户端
/// ——若用全局版本，`kill_client` 会把另一条产品线的客户端一起杀掉。
#[cfg(windows)]
fn running_processes_for(variant: super::variant::TraeVariant) -> Vec<&'static str> {
    match hidden_command("tasklist").args(["/NH"]).output() {
        Ok(output) => {
            let listing = String::from_utf8_lossy(&output.stdout).to_lowercase();
            exe_names_for(variant)
                .iter()
                .copied()
                .filter(|exe| listing.contains(&exe.to_lowercase()))
                .collect()
        }
        Err(_) => Vec::new(),
    }
}

/// 客户端是否正在运行。
///
/// Windows 走 `tasklist`，其余平台走 `pgrep -f`。两者都只做**只读**探测，
/// 失败一律按「未运行」处理——代价是可能让切换流程在客户端仍在运行时继续，
/// 而该情形已由切换前置检查兜底（见 [`crate::modules::trae::profile`]）。
///
/// **必须遍历全部候选名**：只认 `TRAE SOLO CN` 会把「Trae CN 正在跑」判成未运行，
/// 于是切换流程照常去改登录态，而客户端退出时又会把改动写回覆盖掉。
pub fn is_running() -> bool {
    #[cfg(windows)]
    {
        !running_processes().is_empty()
    }
    #[cfg(not(windows))]
    {
        // 用可执行文件基名匹配，避免 pid 被别的进程复用；`-x` 要求精确进程名。
        //
        // ★ 除写死的候选名，还要算上 **macOS bundle 扫描到的真实可执行体名**
        //   （issue #82）：国内版在 macOS 上是 `Trae Code`，只认 `Trae CN` 会把
        //   「客户端正在跑」判成未运行。
        #[allow(unused_mut)]
        let mut names: Vec<String> = exe_names().iter().map(|name| (*name).to_string()).collect();
        #[cfg(target_os = "macos")]
        names.extend(macos_bundle_scan(None).iter().filter_map(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        }));
        for name in names {
            let ok = hidden_command("pgrep")
                .args(["-x", name.as_str()])
                .output()
                .map(|output| output.status.success() && !output.stdout.is_empty())
                .unwrap_or(false);
            if ok {
                return true;
            }
        }
        false
    }
}

/// 结束 Trae 客户端进程。
///
/// 返回 `Ok(true)` 表示确实终止了进程，`Ok(false)` 表示本来就没运行。
///
/// 为什么必须「先杀再启」：Trae 是 Electron 单实例应用，已运行的实例会忽略新的
/// 启动参数（包括 `--proxy-server`）。不先结束旧进程，注入代理后用户看到的是
/// 「窗口被聚焦但完全不走代理」，而日志显示启动成功——极难排查。
pub fn kill_client() -> Result<bool, String> {
    kill_client_for(super::variant::TraeVariant::default())
}

/// 优雅关闭的等待上限（秒）：超时就进强杀。
///
/// ⚠️ **刻意不带 `#[cfg(windows)]`**：它们与 [`wait_until_gone`] 在
/// `kill_client_for` 的**两个**分支里都被用到。
///
/// 反面教材（2026-09-29 CI 实测）：最初只给 Windows 分支加了这个等待，常量在非 Windows
/// 构建里就成了「未使用的常量」⇒ linux/macOS 四个 job 全红、只有 win-x64 绿、**Release 发不出来**。
/// 让两个分支共用同一套助手，比靠 `#[cfg]` 去门更稳 —— 后者漏一处就整批构建失败。
///
/// 3 秒是实测的折中 —— Electron 客户端收到 `WM_CLOSE` 后要把 leveldb / SQLite 落盘，
/// 大一点的 userData 需要 1~2 秒；再长会让「切换」看起来卡住。
const GRACEFUL_CLOSE_WAIT_SECS: u64 = 3;

/// 强杀后等待进程**完全退出**的上限（秒）。
///
/// 不等的话紧接着的启动会被 Electron 的单实例锁拒掉（症状：切换成功但客户端没起来）。
const FORCE_EXIT_WAIT_SECS: u64 = 5;

/// 在 `secs` 秒内轮询等待该变体的客户端退出；返回是否已退出。
///
/// 轮询而不是单次 `sleep`：绝大多数情况进程几百毫秒就走完了，等满 3 秒纯属浪费。
fn wait_until_gone(variant: super::variant::TraeVariant, secs: u64) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        if !is_running_for(variant) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}

/// 结束**指定变体**的 Trae 客户端进程。
///
/// 为什么需要按变体：两条产品线可以同机并存，`taskkill /IM <exe>` 只能针对具体进程名。
/// 若用 [`kill_client`]（横跨全部候选名），切换 Trae Work 会把正在用的 Trae CN 一起杀掉。
///
/// 返回 `Ok(true)` 表示确实终止了进程，`Ok(false)` 表示本来就没运行。
pub fn kill_client_for(variant: super::variant::TraeVariant) -> Result<bool, String> {
    if !is_running_for(variant) {
        return Ok(false);
    }
    #[cfg(windows)]
    {
        // 只对「确实在跑」的候选名下手，不为每个候选名都白起一次 taskkill。
        let targets = running_processes_for(variant);
        if targets.is_empty() {
            return Ok(false);
        }

        // ★ 一级：**优雅关闭**。
        //
        // `taskkill` **不带** `/F` = 向目标进程的顶层窗口投递 `WM_CLOSE`（等价于用户点
        // 窗口右上角的 ×），Electron 客户端收到后才会**正常落盘**（leveldb、
        // `state.vscdb` 的 WAL、Cookie 库…）。
        //
        // 为什么不能一上来就 `/F`（2026-09-29 对照同类实现修正）：强杀会让这些写入
        // **停在 WAL / 内存里**，而我们紧接着就要**备份**这些文件 —— 快照于是缺最新登录
        // 数据，恢复后的症状正是「切换后客户端变成未登录」。同类实现的三级关闭策略
        // 注释同样写着「让 Electron 正常落盘，避免 leveldb/vscdb 文件锁」。
        for exe in &targets {
            let _ = hidden_command("taskkill").args(["/IM", exe]).output();
        }
        if wait_until_gone(variant, GRACEFUL_CLOSE_WAIT_SECS) {
            return Ok(true);
        }

        // 二级：优雅关闭超时（客户端有模态框挡住 WM_CLOSE 是常态）⇒ 强杀。
        let mut killed = false;
        let mut last_error = String::new();
        for exe in &targets {
            let output = hidden_command("taskkill")
                .args(["/F", "/IM", exe])
                .output()
                .map_err(|e| format!("结束客户端进程失败: {e}"))?;
            if output.status.success() {
                killed = true;
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if !stderr.is_empty() {
                    last_error = stderr;
                }
            }
        }
        // 三级：等进程**完全退出**（句柄释放）。不等的话紧接着的启动会被 Electron 的
        // 单实例锁拒掉，表现为「切换成功但客户端没起来」。
        let _ = wait_until_gone(variant, FORCE_EXIT_WAIT_SECS);

        // taskkill 在「进程刚退出」时会返回非 0，此时按成功处理更贴近事实。
        if killed || !is_running_for(variant) {
            return Ok(true);
        }
        Err(format!(
            "结束客户端进程失败: {}",
            if last_error.is_empty() {
                "需要手动关闭 Trae".to_string()
            } else {
                last_error
            }
        ))
    }
    #[cfg(not(windows))]
    {
        // 与 Windows 分支**逐级对称**（SIGTERM ≈ WM_CLOSE）—— 顺带让两个等待常量在
        // 三平台上都被用到，不必靠 `#[cfg]` 去门（漏门会让非 Windows 构建因
        // 「未使用的常量」直接失败，2026-09-29 CI 实测过）。
        // 一级：优雅关闭（`pkill` 不带信号 = SIGTERM）。
        // ★ 走 `process_names_for`（含**实际探测到的可执行体名**，issue #82）：
        //   只认写死的 `Trae CN` 会让 macOS 上的 `Trae Code` 永远杀不掉。
        for name in process_names_for(variant) {
            let _ = hidden_command("pkill").args(["-x", name.as_str()]).output();
        }
        if wait_until_gone(variant, GRACEFUL_CLOSE_WAIT_SECS) {
            return Ok(true);
        }

        // 二级：强杀。
        let mut killed = false;
        for name in process_names_for(variant) {
            let output = hidden_command("pkill")
                .args(["-KILL", "-x", name.as_str()])
                .output()
                .map_err(|e| format!("结束客户端进程失败: {e}"))?;
            if output.status.success() {
                killed = true;
            }
        }
        // 三级：等进程完全退出（避免紧接着的启动被单实例锁拒）。
        let _ = wait_until_gone(variant, FORCE_EXIT_WAIT_SECS);
        if killed || !is_running_for(variant) {
            Ok(true)
        } else {
            Err("结束客户端进程失败: 需要手动关闭 Trae".into())
        }
    }
}

/// 启动 Trae 客户端。
///
/// `proxy_port` 为 `Some` 时注入 `--proxy-server`，让客户端流量走本地代理
/// ——这是让用户「不必去 Trae 设置里手填代理」的关键，也是唯一可靠的方式：
/// Electron 不会读取系统代理来走 MITM 的自签 CA。
pub fn launch_client(exe: &Path, proxy_port: Option<u16>) -> Result<(), String> {
    launch_client_for(super::variant::TraeVariant::default(), exe, proxy_port)
}

/// 启动**指定变体**的 Trae 客户端。
///
/// `proxy_port` 为 `Some` 时注入 `--proxy-server`；注入前先按变体结束已运行实例
/// （Electron 单实例语义会让新参数被忽略，且用全局 kill 会误杀另一条产品线）。
pub fn launch_client_for(
    variant: super::variant::TraeVariant,
    exe: &Path,
    proxy_port: Option<u16>,
) -> Result<(), String> {
    if !exe.is_file() {
        return Err(format!("找不到可执行文件: {}", exe.display()));
    }
    // 注入代理前必须先结束已运行实例，否则参数不生效（单实例语义）。
    if proxy_port.is_some() {
        kill_client_for(variant)?;
    }
    let mut command = std::process::Command::new(exe);
    if let Some(port) = proxy_port {
        command.arg(format!("--proxy-server=http://127.0.0.1:{port}"));
    }
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("启动 Trae 失败: {e}"))?;
    Ok(())
}

/// 当前平台的能力清单（用于前端的「平台能力」说明面板）。
///
/// 除平台级受限项（`machine_guid_reset` / `system_ca_install`）外，还会列出**产品级**
/// 不支持项——即 WorkBuddy 有、而 Trae 产品本身不提供的能力（自动旅行、CodeBuddy
/// CLI/IDE、会话/记忆迁移…）。它们与操作系统无关，故**无条件**下发，`supported_on`
/// 标为「WorkBuddy」以说明「这条能力在哪有」，界面据此渲染为置灰说明而不造假控件。
pub fn capabilities() -> Value {
    // ---- 产品级：WorkBuddy 有、Trae 无（与平台无关，故无条件列出）----
    let mut unsupported: Vec<Value> = vec![Unsupported::new(
        "auto_travel",
        "自动旅行（派猫猫）",
        "WorkBuddy",
        "Trae 客户端没有该活动接口，本工具也无对应后端实现。",
    )
    .to_json()];
    unsupported.push(
        Unsupported::new(
            "codebuddy_cli",
            "CodeBuddy CLI / IDE 接入",
            "WorkBuddy",
            "CodeBuddy 属 WorkBuddy 生态，Trae 分区不提供该客户端的接入与切换。",
        )
        .to_json(),
    );
    unsupported.push(
        Unsupported::new(
            "account_data_migration",
            "会话 / 记忆 / 连接器迁移",
            "WorkBuddy",
            "Trae 登录态是一组 Cloud-IDE-JWT 文件，没有会话树 / 记忆 / 连接器对象可迁移。",
        )
        .to_json(),
    );
    unsupported.push(
        Unsupported::new(
            "session_tree",
            "会话列表 / 复制会话 / 切换进度流",
            "WorkBuddy",
            "Trae 的账号切换是文件级快照替换，不存在会话列表与切换进度事件流。",
        )
        .to_json(),
    );

    // ---- 平台级：仅在缺失该能力的平台上列出 ----
    if !cfg!(windows) {
        unsupported.push(
            Unsupported::new(
                "machine_guid_reset",
                "注册表 MachineGuid 重置",
                "Windows",
                "MachineGuid 是 Windows 专有的系统级设备标识，其他平台没有等价机制。",
            )
            .to_json(),
        );
        unsupported.push(
            Unsupported::new(
                "system_ca_install",
                "系统根证书安装",
                "Windows / macOS / Linux",
                // 这条在 mac/linux 上是「实现了但机制不同」，此处只在真正缺失时下发。
                "当前平台使用系统自带证书工具安装，若失败需要管理员权限。",
            )
            .to_json(),
        );
    }

    json!({
        "platform": platform_tag(),
        "processControl": true,
        "clientDetection": true,
        "userDataDir": detect_data_dir().map(|dir| dir.to_string_lossy().to_string()),
        "machineGuidReset": cfg!(windows),
        "scheduledTask": cfg!(any(windows, target_os = "macos", target_os = "linux")),
        "unsupported": unsupported,
    })
}

/// Trae 客户端环境状态（线上形态）。
///
/// ## `variant` / `variantLabel`：探测到的是哪条产品线
///
/// 自动探测会**横跨全部变体**挑最近活跃的那一个（见 [`data_dir_names_by_activity`]），
/// 所以界面必须能说出「挑中的是谁」。这两个字段就是那个答案：
///
/// - `variant`：稳定标识（`"trae_work"` / `"trae_cn"`），供程序判定；
/// - `variantLabel`：展示名（`"TraeWork CN"` / `"Trae CN"`），直接上界面。
///
/// 判定依据是 [`detect_data_dir`] 的目录名（唯一的权威来源 —— 它是 `%APPDATA%\<产品名>`
/// 逐字拼出来的）；目录不存在时退到安装路径的 exe 名。
/// **两者都推不出来时为 `null`**，调用方应省略该标签而不是显示一个猜测值。
pub fn env_status() -> Value {
    let settings = crate::modules::trae::settings::load();
    // 跨区域探测：每个区域配置的路径都要试（只看一条会漏掉另一区域，issue #20）。
    let probe = detect_install(&settings.all_paths());
    let data_dir = detect_data_dir();

    let variant = detected_variant();

    // 「设置里填的客户端路径现在还指向一个真实文件吗」（issue #20）。
    //
    // ## 为什么必须单独下发这个布尔值
    //
    // [`detect_install`] 在自定义路径失效时会**静默回落**到自动探测 —— 这是刻意的：
    // 客户端合并/升级会搬走甚至卸载旧 exe（本机 `D:\Programs\TRAE SOLO CN\` 现在就只剩
    // 一个 debug.log），不该因此卡住启动。但用户需要知道「我填的那个路径已经没用了」，
    // 否则设置页会一直显示一个不存在的路径、而界面同时说「已检测到」——
    // 两边对不上，正是 issue #20 报障原话「设置了启动 exe 已经无法找到」的形态。
    //
    // 判据用**存盘值原样**判存在性，与 `detect_install` 的采纳逻辑**分开**：
    // 后者还要求路径归属该变体，混在一起会把「路径存在但不属于这条产品线」
    // 也说成「失效」，那是另一回事（`detect_install_for` 的 `belongs` 分支负责）。
    //
    // ## ★ 为什么下发**两个映射**而不是单值（issue #20）
    //
    // 设置里的路径已按区域分家，而本命令是**全局**视角（「自动挑中的是哪一条」）。
    // 设置页按区域渲染，需要拿到**本区域**那一条 —— 用检测到的变体所属区域去取会张冠李戴
    // （用户在国际版页面上却看到国内版的路径）。因此把每个区域的值都下发，
    // 由前端按当前区域取。`configuredPath` / `configuredPathExists` 保留为
    // 「**检测到的那条线**所属区域」的值，供不关心区域的旧调用点继续读。
    let mut configured_paths = serde_json::Map::new();
    let mut configured_paths_exist = serde_json::Map::new();
    for region in super::region::TraeRegion::all() {
        let path = settings.path_for(region);
        configured_paths.insert(region.as_str().to_string(), json!(path));
        configured_paths_exist.insert(
            region.as_str().to_string(),
            json!(path
                .map(|path| std::path::Path::new(path).is_file())
                .unwrap_or(false)),
        );
    }

    let configured_path = variant.and_then(|variant| settings.path_for(variant.region()));
    let configured_path_exists = configured_path
        .map(|path| std::path::Path::new(path).is_file())
        .unwrap_or(false);

    json!({
        "installed": probe.installed,
        "running": is_running(),
        "version": probe.version,
        "path": probe.exe.as_ref().map(|p| p.to_string_lossy().to_string()),
        "dataDir": data_dir.as_ref().map(|p| p.to_string_lossy().to_string()),
        "dataDirExists": data_dir.map(|p| p.is_dir()).unwrap_or(false),
        "platform": platform_tag(),
        "configuredPath": configured_path,
        // `configuredPath` 为空时恒 `false` —— 前端只在「填了且失效」时才提示，
        // 见 `TraeSettingsPage` 的客户端路径一栏。
        "configuredPathExists": configured_path_exists,
        // ★ 按区域分家的完整视图（键 = `TraeRegion::as_str()`）。
        "configuredPaths": Value::Object(configured_paths),
        "configuredPathsExist": Value::Object(configured_paths_exist),
        "variant": variant.map(|v| v.as_str()),
        "variantLabel": variant.map(|v| v.display_name()),
    })
}

/// 探测「当前选定的是哪条 Trae 产品线」。
///
/// 判据顺序：**userData 目录名优先**（它由 Rust 按 `<产品名>` 逐字拼出，等于产品名），
/// 再退到 exe 文件名。两边都用 [`super::variant::variant_of_name`] 反查，
/// 因此新增产品线只需改变体表。
///
/// 与 [`env_status`] 共用同一份逻辑：`env_status` 把它透出给前端，
/// 凭据提取失败时的诊断信息也用它 —— **不要在两处各写一份**，否则会出现
/// 「界面显示 Trae CN，报错却说 Trae Work」这类错位。
pub fn detected_variant() -> Option<super::variant::TraeVariant> {
    let settings = crate::modules::trae::settings::load();
    detect_data_dir()
        .as_ref()
        .and_then(|dir| dir.file_name().and_then(|n| n.to_str()))
        .and_then(super::variant::variant_of_name)
        .or_else(|| {
            // 自定义路径**逐个区域**试（issue #20：只看单值会漏掉另一区域）。
            // `detect_install` 已按传入顺序取第一个存在的文件，故这里直接取它的 exe 名反查。
            detect_install(&settings.all_paths())
                .exe
                .as_ref()
                .and_then(|exe| exe.file_name().and_then(|n| n.to_str()))
                .and_then(super::variant::variant_of_name)
        })
}

/// Trae 模块数据目录（供 UI 展示与「打开目录」）。
pub fn trae_data_dir() -> String {
    paths::trae_dir().to_string_lossy().to_string()
}

// ---------------------------------------------------------------------------
// 6 层设备标识重置
// ---------------------------------------------------------------------------

/// 生成 32 位十六进制机器码（替代 PowerShell 的 `New-Guid` 去连字符形态）。
fn new_machine_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// 生成标准 GUID（带连字符），用于 `telemetry.sqmId` 与注册表 `MachineGuid`。
fn new_guid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 重置 Trae 客户端的设备标识（6 层）。
///
/// **调用前置条件**：当前登录态必须已备份。本操作会改写客户端数据文件，
/// 与 [`crate::modules::trae::profile::switch_account`] 的备份步骤配合使用；
/// 单独调用前请自行确认已保存快照。
///
/// 逐层行为：
///
/// | # | 位置 | 操作 | 平台 |
/// |:--|:---|:---|:---|
/// | 1 | `<userData>/machineid` | 覆写为新的 32 位 hex | 全平台 |
/// | 2 | `storage.json` 的 `telemetry.machineId` / `telemetry.sqmId` | 替换 | 全平台 |
/// | 3 | `storage.json` 的 `aha.device.device_id` | 替换 | 全平台 |
/// | 4 | `<userData>/aha/TinyStorage` 内含 `device_id` 的文件 | 删除 | 全平台 |
/// | 5 | 注册表 `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid` | 替换 | **仅 Windows** |
/// | 6 | `Partitions/trae-webview` 的 Network / Local Storage / Session Storage | 删除 | 全平台 |
///
/// 另外删除 `storage.json` 的 `has_device_id_updated_to_aha` 标记位——它会让客户端
/// 认为「设备 ID 已上报过」，从而跳过重新生成的流程，使前几步白做。
///
/// 返回线上形态的重置报告。**任一层失败都不返回 `Err`**，而是记入 `steps` 的
/// `skip` 状态：设备隔离是「尽力而为」的加固措施，个别文件被占用不应该让整次
/// 账号切换失败。
pub fn reset_device_identity() -> Result<Value, String> {
    reset_device_identity_for(super::variant::TraeVariant::default())
}

/// 重置**指定变体**的设备标识（默认变体见 [`reset_device_identity`]）。
///
/// 设备标识存在**该变体客户端的 userData** 里，故必须按变体重置 ——
/// 否则在 Trae CN 分区点"重置设备"，改的是 Trae Work 的目录（或反之），
/// 而界面会显示"成功"，实际什么都没生效。
pub fn reset_device_identity_for(variant: super::variant::TraeVariant) -> Result<Value, String> {
    let data_dir = detect_data_dir_for(variant)
        .filter(|dir| dir.is_dir())
        .ok_or("未找到 Trae 客户端数据目录，无法重置设备标识")?;

    let machine_id = new_machine_id();
    let guid = new_guid();
    let mut steps: Vec<Value> = Vec::new();
    let mut reset_count = 0u32;

    // 1. machineid 文件
    let machineid_file = data_dir.join("machineid");
    if machineid_file.is_file() {
        match std::fs::write(&machineid_file, &machine_id) {
            Ok(()) => {
                reset_count += 1;
                steps.push(json!({"layer": 1, "label": "machineid 文件", "status": "ok"}));
            }
            Err(e) => steps.push(
                json!({"layer": 1, "label": "machineid 文件", "status": "skip", "reason": e.to_string()}),
            ),
        }
    } else {
        steps.push(json!({"layer": 1, "label": "machineid 文件", "status": "skip", "reason": "文件不存在"}));
    }

    // 2 + 3. storage.json（点号扁平键，不是嵌套对象）
    let storage_file = data_dir
        .join("User")
        .join("globalStorage")
        .join("storage.json");
    if storage_file.is_file() {
        match rewrite_storage_json(&storage_file, &machine_id, &guid) {
            Ok(changed) => {
                if changed {
                    reset_count += 1;
                    steps.push(json!({"layer": 2, "label": "storage.json 遥测与设备标识", "status": "ok"}));
                } else {
                    steps.push(json!({
                        "layer": 2,
                        "label": "storage.json 遥测与设备标识",
                        "status": "skip",
                        "reason": "未找到需要替换的键"
                    }));
                }
            }
            Err(e) => steps.push(
                json!({"layer": 2, "label": "storage.json 遥测与设备标识", "status": "skip", "reason": e}),
            ),
        }
    } else {
        steps.push(json!({
            "layer": 2,
            "label": "storage.json 遥测与设备标识",
            "status": "skip",
            "reason": "storage.json 不存在"
        }));
    }

    // 4. aha/TinyStorage 内含 device_id 的文件
    let tiny_storage = data_dir.join("aha").join("TinyStorage");
    if tiny_storage.is_dir() {
        let removed = purge_files_containing(&tiny_storage, "device_id");
        if removed > 0 {
            reset_count += 1;
        }
        steps.push(json!({
            "layer": 4,
            "label": "aha/TinyStorage device_id",
            "status": if removed > 0 { "ok" } else { "skip" },
            "removed": removed,
        }));
    } else {
        steps.push(json!({
            "layer": 4,
            "label": "aha/TinyStorage device_id",
            "status": "skip",
            "reason": "目录不存在"
        }));
    }

    // 5. 注册表 MachineGuid —— Windows 专有
    #[cfg(windows)]
    {
        match reset_machine_guid(&guid) {
            Ok(()) => {
                reset_count += 1;
                steps.push(json!({"layer": 5, "label": "注册表 MachineGuid", "status": "ok"}));
            }
            Err(e) => steps.push(json!({
                "layer": 5,
                "label": "注册表 MachineGuid",
                "status": "skip",
                "reason": format!("{e}（重置注册表需要管理员权限，不影响账号切换）"),
            })),
        }
    }
    #[cfg(not(windows))]
    {
        // 不加 `supported_on` 之外的任何伪装：明确标注该层在非 Windows 上不存在。
        steps.push(json!({
            "layer": 5,
            "label": "注册表 MachineGuid",
            "status": "unsupported",
            "reason": "MachineGuid 是 Windows 专有的系统级设备标识，当前平台不存在该机制",
        }));
    }

    // 6. trae-webview 追踪数据
    let webview = data_dir.join("Partitions").join("trae-webview");
    if webview.is_dir() {
        let mut cleared = 0u32;
        for name in ["Network", "Local Storage", "Session Storage"] {
            let target = webview.join(name);
            if target.exists() && std::fs::remove_dir_all(&target).is_ok() {
                cleared += 1;
            }
        }
        if cleared > 0 {
            reset_count += 1;
        }
        steps.push(json!({
            "layer": 6,
            "label": "WebView 追踪数据",
            "status": if cleared > 0 { "ok" } else { "skip" },
            "cleared": cleared,
        }));
    } else {
        steps.push(json!({
            "layer": 6,
            "label": "WebView 追踪数据",
            "status": "skip",
            "reason": "目录不存在"
        }));
    }

    store::append_log(
        &paths::app_log_file(),
        &format!("设备标识重置完成: {reset_count} 项生效"),
    );

    Ok(json!({
        "resetCount": reset_count,
        "machineId": machine_id,
        "guid": guid,
        "totalLayers": 6,
        "steps": steps,
    }))
}

/// 改写 `storage.json` 中的遥测与设备标识键。
///
/// `storage.json` 的键是**扁平的点号字符串**（`"telemetry.machineId"`），
/// 而不是嵌套对象。这是 Trae/VS Code 系的存储约定，按嵌套结构去改会静默失败。
///
/// 返回是否真的发生了替换。
fn rewrite_storage_json(path: &Path, machine_id: &str, guid: &str) -> Result<bool, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("读取失败: {e}"))?;
    let Ok(mut value) = serde_json::from_str::<Value>(&text) else {
        return Err("不是合法的 JSON，已跳过".into());
    };
    let Some(map) = value.as_object_mut() else {
        return Err("顶层不是 JSON 对象，已跳过".into());
    };

    let mut changed = false;
    for (key, new_value) in [
        ("telemetry.machineId", machine_id),
        ("telemetry.sqmId", guid),
        ("aha.device.device_id", machine_id),
    ] {
        if map.contains_key(key) {
            map.insert(key.to_string(), Value::String(new_value.to_string()));
            changed = true;
        }
    }
    // 标记位：留着它客户端会认为设备 ID 已上报，跳过重新生成。
    if map.remove("has_device_id_updated_to_aha").is_some() {
        changed = true;
    }

    if changed {
        let pretty =
            serde_json::to_string_pretty(&value).map_err(|e| format!("序列化失败: {e}"))?;
        // 先写临时文件再替换，避免客户端同时读写导致 storage.json 半截损坏。
        // 该文件一旦损坏，客户端会丢失全部本地状态（比丢设备标识严重得多）。
        store::atomic_write_text(path, &pretty)?;
    }
    Ok(changed)
}

/// 递归删除内容中包含指定关键词的文本文件，返回删除数量。
fn purge_files_containing(dir: &Path, needle: &str) -> u32 {
    let mut removed = 0u32;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            removed += purge_files_containing(&path, needle);
            continue;
        }
        // 只处理小体积文本文件：把整个 TinyStorage 逐文件读进内存判断关键词，
        // 遇到大二进制文件既慢又无意义。
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.len() > 4 * 1024 * 1024 {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if content.contains(needle) && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// 重置注册表 `MachineGuid`（仅 Windows；需要管理员权限）。
#[cfg(windows)]
fn reset_machine_guid(guid: &str) -> Result<(), String> {
    let output = hidden_command("reg")
        .args([
            "add",
            "HKLM\\SOFTWARE\\Microsoft\\Cryptography",
            "/v",
            "MachineGuid",
            "/t",
            "REG_SZ",
            "/d",
            guid,
            "/f",
        ])
        .output()
        .map_err(|e| format!("执行 reg 失败: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★「没有数据目录」的解释必须**区分**「没装客户端」与「装了但从没启动过」。
    ///
    /// 现场（2026-09-24 用户报障原话）：「**已经安装了，为什么检查不到安装的客户端**」——
    /// 旧文案只说「未找到【Trae Work】的数据目录」，用户读成「你没装客户端」，
    /// 于是去反复重装/重试，而真正缺的是「首次启动才会写入的设备凭证」。
    ///
    /// ## 断言集随机器分支（但两支都可证伪）
    ///
    /// 本机装没装 Trae 决定走哪一支；**两支都能被旧文案弄红**：
    /// - 装了：旧文案不含「从未启动过」、也不含 exe 路径 ⇒ 红；
    /// - 没装：旧文案不含「未检测到」/「设置」 ⇒ 红。
    ///
    /// 持 `env_lock()`：本用例既读 `detect_install_for`（内部读 `settings` → `store_dir()`）
    /// 又自己再读一次，两次之间若被别的用例改走 `BUDDY_SWITCH_HOME`，
    /// 就会拿到「文案说装了、探测说没装」的**假失败**（见验证纪律「无参全局路径函数」一条）。
    #[test]
    fn data_dir_missing_reason_distinguishes_not_installed_from_never_launched() {
        let _lock = crate::modules::config::env_lock();
        let variant = super::super::variant::TraeVariant::TraeWork;

        let reason = data_dir_missing_reason(variant);
        // ① 必须指名道姓 —— 否则用户不知道该去启动/安装哪一个产品线。
        assert!(
            reason.contains(variant.display_name()),
            "必须点明是哪条产品线: {reason}"
        );

        // ② 与「启动客户端」按钮**同源**的探测结果决定说哪一套话。
        match detect_install_for(variant).exe {
            Some(exe) => {
                assert!(
                    reason.contains("从未启动过"),
                    "装了但没数据目录 ⇒ 必须说清「从未启动过」: {reason}"
                );
                assert!(
                    reason.contains(&exe.display().to_string()),
                    "必须给出**检测到的**可执行文件路径（这正是用户「为什么检查不到」的答案）: {reason}"
                );
            }
            None => {
                assert!(
                    reason.contains("未检测到"),
                    "真的没装 ⇒ 必须说「未检测到」: {reason}"
                );
                assert!(
                    reason.contains("设置"),
                    "真的没装 ⇒ 必须引导去「设置」里指定路径: {reason}"
                );
            }
        }
    }

    #[test]
    fn platform_tag_is_known_value() {
        assert!(matches!(
            platform_tag(),
            "windows" | "macos" | "linux" | "unknown"
        ));
    }

    #[test]
    fn unsupported_carries_actionable_context() {
        let unsupported = Unsupported::new(
            "machine_guid_reset",
            "注册表 MachineGuid 重置",
            "Windows",
            "仅 Windows 提供该机制",
        );
        let value = unsupported.to_json();
        assert_eq!(
            value.get("capability").unwrap().as_str(),
            Some("machine_guid_reset")
        );
        assert_eq!(value.get("supportedOn").unwrap().as_str(), Some("Windows"));
        assert!(value.get("reason").unwrap().as_str().unwrap().len() > 5);
        // 线上形态必须 camelCase
        assert!(value.get("supported_on").is_none());
    }

    #[test]
    fn exe_names_are_non_empty_and_distinct() {
        let names = exe_names();
        assert!(!names.is_empty());
        let mut sorted = names.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "候选名不能重复");
    }

    #[test]
    fn data_dir_names_include_primary_candidate() {
        // 主候选必须排第一，否则 detect_data_dir 的兜底返回值会变成次要名字。
        // ★ issue #28：主候选是 **CN 合并版** `Trae CN`（收敛前是 `TRAE SOLO CN`，
        //   那条客户端已被官方合并卸载，继续当主候选就是「指向不存在的客户端」）。
        assert_eq!(data_dir_names()[0], "Trae CN");
    }

    #[test]
    fn detect_install_prefers_custom_path_when_valid() {
        let temp = std::env::temp_dir().join(format!("fake-trae-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp);
        let exe = temp.join(exe_names()[0]);
        std::fs::write(&exe, b"stub").unwrap();

        let probe = detect_install(&[exe.to_string_lossy().to_string()]);
        assert!(probe.installed);
        assert_eq!(probe.exe.as_deref(), Some(exe.as_path()));

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn detect_install_ignores_invalid_custom_path() {
        // 自定义路径不存在时不能直接报「已安装」，应继续走自动探测。
        let probe = detect_install(&["/definitely/not/here/trae".to_string()]);
        assert!(!probe.installed || probe.exe.is_some());
        if !probe.installed {
            assert!(probe.exe.is_none());
        }
    }

    #[test]
    fn detect_install_blank_custom_path_does_not_error() {
        let probe = detect_install(&["   ".to_string()]);
        // 空/空白自定义路径应被忽略而不是当成路径 ""
        if let Some(exe) = probe.exe {
            assert!(!exe.to_string_lossy().trim().is_empty());
        }
    }

    #[test]
    fn version_read_from_electron_package_json() {
        let temp = std::env::temp_dir().join(format!("fake-trae-ver-{}", std::process::id()));
        let app = temp.join("resources").join("app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(
            app.join("package.json"),
            br#"{"name":"trae","version":"1.2.3"}"#,
        )
        .unwrap();
        assert_eq!(version_from_install_dir(&temp), Some("1.2.3".into()));
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn version_ignores_zero_placeholder() {
        // VS Code 系打包常把 version 写成 0.0.0 占位，必须跳过而不是展示 0.0.0。
        let temp = std::env::temp_dir().join(format!("fake-trae-zero-{}", std::process::id()));
        let app = temp.join("resources").join("app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("package.json"), br#"{"version":"0.0.0"}"#).unwrap();
        assert_eq!(version_from_install_dir(&temp), None);
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn version_tolerates_missing_and_broken_manifests() {
        let temp = std::env::temp_dir().join(format!("fake-trae-broken-{}", std::process::id()));
        let app = temp.join("resources").join("app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("package.json"), b"{ not json").unwrap();
        assert_eq!(version_from_install_dir(&temp), None);
        assert_eq!(version_from_install_dir(Path::new("/nonexistent")), None);
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn capabilities_reports_platform_and_process_support() {
        let value = capabilities();
        assert_eq!(
            value.get("platform").unwrap().as_str(),
            Some(platform_tag())
        );
        assert_eq!(value.get("processControl").unwrap().as_bool(), Some(true));
        // machineGuidReset 必须与编译目标一致，不能在非 Windows 上谎报支持。
        assert_eq!(
            value.get("machineGuidReset").unwrap().as_bool(),
            Some(cfg!(windows))
        );
        assert!(value.get("unsupported").unwrap().is_array());
    }

    #[test]
    fn env_status_has_camel_case_wire_fields() {
        let value = env_status();
        for key in [
            "installed",
            "running",
            "version",
            "path",
            "dataDir",
            "dataDirExists",
            "platform",
            "configuredPath",
            "configuredPathExists",
            "configuredPaths",
            "configuredPathsExist",
        ] {
            assert!(value.get(key).is_some(), "缺少线上字段 {key}");
        }
        // 不能泄漏 snake_case 形式
        assert!(value.get("data_dir").is_none());
        assert!(value.get("configured_path_exists").is_none());
        assert!(value.get("running").unwrap().is_boolean());
        assert!(value.get("installed").unwrap().is_boolean());
        assert!(value.get("configuredPathExists").unwrap().is_boolean());
    }

    /// ★ `configuredPathExists` 必须如实反映「设置里填的那个路径**还在不在**」（issue #20）。
    ///
    /// ## 为什么这条非钉不可
    ///
    /// issue #20 的报障原话是「设置了启动 exe 已经无法找到」：客户端合并/升级后
    /// 旧 exe 被搬走甚至整个目录被卸载（本机 `D:\Programs\TRAE SOLO CN\` 现在只剩 debug.log），
    /// 而设置里仍存着旧路径。`detect_install` 会**静默回落**（这是对的），
    /// 于是界面显示「已检测到」、设置页却摆着一个不存在的路径 —— 用户无从知道该改。
    ///
    /// 反例（改坏会红）：
    /// - 把它写成「探测到的路径存在」⇒ 几乎恒为 `true`，提示永远不出现；
    /// - 干脆不判存在性、恒 `true` ⇒ 同上。
    #[test]
    fn configured_path_existence_reflects_saved_value() {
        let env = crate::modules::trae::test_support::TempEnv::with_device_fixture();
        let dir = std::env::temp_dir().join(format!(
            "buddy-switch-configured-path-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("临时目录应能创建");

        // 1) 没填 → 两个区域都 false（前端据此**不**提示）。
        crate::modules::trae::settings::patch(serde_json::json!({ "traePaths": {} }))
            .expect("清空 traePaths 应成功");
        let value = env_status();
        for region in ["cn", "global"] {
            assert_eq!(
                value
                    .get("configuredPathsExist")
                    .and_then(|map| map.get(region))
                    .and_then(|v| v.as_bool()),
                Some(false),
                "未配置路径时 {region} 不得报 true"
            );
        }

        // 2) **只**给国内版填一个**存在**的文件 → cn=true、global 仍 false。
        //    ★ 这一对断言就是 issue #20 的回归护栏：旧实现是单个全局字段，
        //    给任一区域设置都会让两边同时变 true（「以最后的设置为准」）。
        let real = dir.join("Trae CN.exe");
        std::fs::write(&real, b"x").expect("应能写临时 exe");
        crate::modules::trae::settings::patch(serde_json::json!({
            "traePaths": { "cn": real.to_string_lossy() }
        }))
        .expect("设置 traePaths 应成功");
        let value = env_status();
        let exist = value
            .get("configuredPathsExist")
            .expect("应有 configuredPathsExist");
        assert_eq!(
            exist.get("cn").and_then(|v| v.as_bool()),
            Some(true),
            "国内版存在的路径必须报 true"
        );
        assert_eq!(
            exist.get("global").and_then(|v| v.as_bool()),
            Some(false),
            "★ 区域必须隔离：给国内版设路径不得让国际版也变成已配置"
        );
        assert_eq!(
            value
                .get("configuredPaths")
                .and_then(|map| map.get("cn"))
                .and_then(|v| v.as_str()),
            Some(real.to_string_lossy().as_ref()),
            "configuredPaths.cn 必须原样回显存盘值"
        );
        assert!(
            value
                .get("configuredPaths")
                .and_then(|map| map.get("global"))
                .map(|v| v.is_null())
                .unwrap_or(false),
            "未配置的区域必须为 null 而不是空串"
        );

        // 3) 填了一个**已失效**的路径（客户端合并/升级搬走了 exe）→ false，
        //    且探测必须仍然跑完（回落自动探测，不因失效路径报错/卡死）。
        let dead = dir.join("TRAE SOLO CN.exe");
        crate::modules::trae::settings::patch(serde_json::json!({
            "traePaths": { "cn": dead.to_string_lossy() }
        }))
        .expect("设置失效 traePaths 应成功");
        let value = env_status();
        assert_eq!(
            value
                .get("configuredPathsExist")
                .and_then(|map| map.get("cn"))
                .and_then(|v| v.as_bool()),
            Some(false),
            "失效路径必须如实报 false —— 设置页据此提示用户重新选择"
        );
        assert!(
            value.get("installed").unwrap().is_boolean(),
            "回落探测必须仍然给出 installed，不得因失效路径中断"
        );

        drop(env);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn data_dir_base_and_detect_never_panic() {
        // 探测必须对任意环境安全：只验证不 panic 且返回值形态正确。
        let dir = detect_data_dir();
        if let Some(dir) = dir {
            assert!(!dir.to_string_lossy().is_empty());
        }
    }

    /// ★ 逐变体探测必须**互不干扰**：装了 A 不能让 B 也显示"已安装"。
    ///
    /// 这是「两条产品线并排显示各自状态」的正确性基础。若 `detect_install_for`
    /// 退化成跨变体探测，两个图标会一起亮/一起灭，用户看到的信息量为零。
    #[test]
    fn 逐变体安装探测互不干扰() {
        let work = detect_install_for(super::super::variant::TraeVariant::TraeWork);
        let cn = detect_install_for(super::super::variant::TraeVariant::Trae);

        // 探到的 exe 必须落在该变体自己的候选目录名下（不能串到另一条产品线）。
        for (variant, probe) in [
            (super::super::variant::TraeVariant::TraeWork, &work),
            (super::super::variant::TraeVariant::Trae, &cn),
        ] {
            if let Some(exe) = probe.exe.as_ref() {
                let parent = exe
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                let belongs = data_dir_names_for(variant)
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(parent));
                assert!(
                    belongs,
                    "变体 {variant:?} 探到的 exe 不在自己的目录名下: {exe:?}（父目录 {parent}）"
                );
            }
        }
    }

    /// `variants_status()` 必须返回**全部**变体（不止挑中的那一条），
    /// 且字段与 `env_status` 同形（camelCase）供前端直接消费。
    #[test]
    fn variants_status_returns_every_variant() {
        let value = variants_status();
        let items = value
            .get("variants")
            .and_then(|v| v.as_array())
            .expect("variants 应为数组");

        // 数量必须等于**区域总数** —— 少一条就意味着界面上会少一个区域入口。
        // 形状自 2026-09-21 起是「按区域列条目、条目内含程序位」，不再是「按产品线列条目」。
        assert_eq!(
            items.len(),
            super::super::region::TraeRegion::all().len(),
            "variants_status 未返回全部区域"
        );

        // 每个条目都必须带 `programs`，且该区域的程序位数量与规格表一致 ——
        // 少一枚就意味着卡片上少一个切换按钮（用户看不到那个客户端）。
        for item in items {
            let region = item
                .get("variant")
                .and_then(|v| v.as_str())
                .expect("区域标识必须存在");
            let programs = item
                .get("programs")
                .and_then(|v| v.as_array())
                .unwrap_or_else(|| panic!("区域 {region} 缺少 programs 数组"));
            let parsed = super::super::region::TraeRegion::parse(region)
                .unwrap_or_else(|| panic!("区域标识无法回解: {region}"));
            assert_eq!(
                programs.len(),
                super::super::region::programs_of(parsed).len(),
                "区域 {region} 的程序位数与规格表不一致"
            );
        }

        let mut seen: Vec<&str> = Vec::new();
        for item in items {
            for key in [
                "variant",
                "variantLabel",
                "consoleBase",
                "installed",
                "running",
                "version",
                "path",
                "dataDir",
                "dataDirExists",
                "writeDataDir",
                "writeDataDirExists",
                "programs",
            ] {
                assert!(item.get(key).is_some(), "区域项缺少线上字段 {key}");
            }
            assert!(item.get("installed").unwrap().is_boolean());
            assert!(item.get("running").unwrap().is_boolean());
            // 不能泄漏 snake_case 形式。
            assert!(item.get("variant_label").is_none());
            assert!(item.get("write_data_dir").is_none());
            // 官方别名**下移到程序位**（区域不是客户端，没有 nameAlias）。
            for program in item.get("programs").and_then(|v| v.as_array()).unwrap() {
                for key in [
                    "program",
                    "label",
                    "nameAlias",
                    "variant",
                    "installed",
                    "running",
                    "dataDir",
                    "writeDataDir",
                    "writeDataDirExists",
                ] {
                    assert!(program.get(key).is_some(), "程序位缺少线上字段 {key}");
                }
            }
            seen.push(item.get("variant").and_then(|v| v.as_str()).unwrap());
        }

        // 两个区域的标识必须都在，且不重复。
        for expected in ["cn", "global"] {
            assert!(seen.contains(&expected), "缺少区域 {expected}");
        }
        assert_eq!(seen.len(), 2, "区域标识重复: {seen:?}");
    }

    /// ★★ 全部程序位**已建模**，且每个程序位回传的 `variant` 都能反解回
    /// 「同一个区域 + 同一个程序」（2026-09-28 issue #3；2026-10-13 issue #20 增合并版）。
    ///
    /// ## 这条是 issue #3 的直接验收
    ///
    /// 报障是「不支持 TraeCode」。当时的形状是：`(Global, TraeCode)` 返回 `None`
    /// ⇒ 国际版页签上的 TraeCode 程序位 `variant: null`、`installed: false`，
    /// 前端把它渲染成**禁用按钮**，用户看到的就是「不支持」。
    ///
    /// 现在全部组合都有值。断言三件事：
    /// 1. **没有一个程序位的 `variant` 是 `null`**（这正是当时那条分支的痕迹）；
    /// 2. 回传的 `variant` 反解后的**区域**等于它所在的区域条目；
    /// 3. 回传的 `variant` 反解后的**程序**等于该程序位的 `program`。
    ///
    /// 第 2、3 条合起来才排得掉「值有、但指向另一个客户端」这种更坏的形态
    /// （切换会把登录态写进错的 userData，而界面上一切正常）。
    ///
    /// ⚠️ 第 3 条在 issue #20 之后**不再是「同一区域内的唯一解」**：合并版 `Trae`
    /// 与遗留 `TraeCode` 共享同一个变体（`trae_cn` / `global_trae_code`），
    /// 两者都能通过 `variant_for_program` 的往返 —— 这是刻意的（同一个客户端）。
    #[test]
    fn 全部程序位已建模且自洽() {
        use super::super::region::{TraeProgram, TraeRegion};
        use super::super::variant::TraeVariant;

        let value = variants_status();
        let items = value.get("variants").and_then(|v| v.as_array()).unwrap();
        let mut checked = 0usize;

        for item in items {
            let region = TraeRegion::parse(item.get("variant").and_then(|v| v.as_str()).unwrap())
                .expect("区域标识必须能反解");
            for program in item.get("programs").and_then(|v| v.as_array()).unwrap() {
                checked += 1;
                let program_id = program.get("program").and_then(|v| v.as_str()).unwrap();
                let expected_program = TraeProgram::parse(program_id)
                    .unwrap_or_else(|| panic!("程序位标识无法反解: {program_id}"));
                let raw = program
                    .get("variant")
                    .unwrap_or_else(|| panic!("程序位 {region:?}/{program_id} 缺少 variant 键"));
                let raw = raw.as_str().unwrap_or_else(|| {
                    panic!(
                        "程序位 {region:?}/{program_id} 的 variant 为 null —— \
                         即 issue #3 的「未建模」形态，必须已修"
                    )
                });
                let target =
                    TraeVariant::parse(raw).unwrap_or_else(|| panic!("variant 值无法反解: {raw}"));
                assert_eq!(
                    target.region(),
                    region,
                    "程序位 {region:?}/{program_id} 的 variant={raw} 指向了别的区域"
                );
                assert_eq!(
                    variant_for_program(region, expected_program),
                    target,
                    "程序位 {region:?}/{program_id} 的 variant={raw} 与程序位表不一致"
                );
            }
        }
        // ★ issue #28：每区域**只有一个**程序位（合并版）⇒ 全表 2 条。
        //   前置：必须真的把可选程序位都过了一遍，否则本用例会**恒真**。
        assert_eq!(checked, 2, "应有 2 个可选程序位（2 区域 × 1）");
    }

    /// 区域状态带出的「站点域」（`consoleBase`）必须**按区域分家**，且与授权页域**同源**。
    ///
    /// 断言的是**字面值**，不是「等于某个函数」—— 后者会退化成同义反复：函数改错时
    /// 两边一起错、测试照样绿。字面值来自实测的客户端 `bootConfig.consoleHost`。
    #[test]
    fn 区域状态带出的站点域按区域分家() {
        let value = variants_status();
        let items = value.get("variants").and_then(|v| v.as_array()).unwrap();
        let domain_of = |region: &str| -> &str {
            items
                .iter()
                .find(|item| item.get("variant").and_then(|v| v.as_str()) == Some(region))
                .unwrap_or_else(|| panic!("缺少区域 {region}"))
                .get("consoleBase")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("区域 {region} 缺少 consoleBase"))
        };

        assert_eq!(domain_of("cn"), "https://www.trae.cn");
        assert_eq!(domain_of("global"), "https://www.trae.ai");
        assert_ne!(
            domain_of("cn"),
            domain_of("global"),
            "两个区域的站点域不得相同 —— 否则国际版用户会被导到国内站"
        );

        // **同源**：前端「关于」外链的域与授权页的域必须出自同一份端点表，
        // 否则会出现「外链指 trae.ai、授权页却开 trae.cn」这种自相矛盾。
        for (region, variant) in [
            ("cn", super::super::variant::TraeVariant::TraeWork),
            ("global", super::super::variant::TraeVariant::Global),
        ] {
            assert_eq!(
                domain_of(region),
                super::super::endpoints_for(variant).console_base,
                "区域 {region} 的站点域与授权页域不同源"
            );
        }
    }

    /// 逐变体运行探测：结果只取决于该变体的进程名。
    ///
    /// 不假设本机装了什么（CI 上两条都没装），只验证「不 panic 且互不耦合」，
    /// 以及「任一产品线在跑 ⇒ 全局 `is_running()` 为真」这条蕴含关系。
    #[test]
    fn 逐变体运行探测与全局一致() {
        let work = is_running_for(super::super::variant::TraeVariant::TraeWork);
        let cn = is_running_for(super::super::variant::TraeVariant::Trae);

        // 全局判定是逐变体判定的并集：任一为真则全局必须为真。
        if work || cn {
            assert!(
                is_running(),
                "逐变体探测说有客户端在跑，全局 is_running() 却说没有"
            );
        }
    }

    #[test]
    fn is_running_is_side_effect_free() {
        // 只读探测：连续调用结果一致，且不会误报「运行中」为错误。
        let first = is_running();
        let second = is_running();
        assert_eq!(first, second);
    }

    #[test]
    fn machine_id_and_guid_have_expected_shapes() {
        let machine_id = new_machine_id();
        assert_eq!(machine_id.len(), 32);
        assert!(machine_id.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!machine_id.contains('-'), "machineid 文件不带连字符");

        let guid = new_guid();
        assert_eq!(guid.len(), 36);
        assert!(uuid::Uuid::parse_str(&guid).is_ok());
        // 两次调用必须不同，否则「重置」实际没换标识
        assert_ne!(machine_id, new_machine_id());
        assert_ne!(guid, new_guid());
    }

    #[test]
    fn rewrite_storage_json_uses_flat_dot_keys_not_nested() {
        // 关键回归：Trae 的 storage.json 是**扁平点号键**。
        // 若按嵌套对象去改，"telemetry.machineId" 会被静默漏掉（改了个不存在的嵌套路径）。
        let base = std::env::temp_dir().join(format!("trae-storage-flat-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&base);
        let path = base.join("storage.json");
        std::fs::write(
            &path,
            r#"{"telemetry.machineId":"old-machine","telemetry.sqmId":"old-sqm","aha.device.device_id":"old-device","has_device_id_updated_to_aha":true,"other":"keep"}"#,
        )
        .unwrap();

        let changed = rewrite_storage_json(&path, "NEWMACHINE", "NEW-GUID").unwrap();
        assert!(changed);

        let after: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            after.get("telemetry.machineId").unwrap().as_str(),
            Some("NEWMACHINE")
        );
        assert_eq!(
            after.get("telemetry.sqmId").unwrap().as_str(),
            Some("NEW-GUID")
        );
        assert_eq!(
            after.get("aha.device.device_id").unwrap().as_str(),
            Some("NEWMACHINE")
        );
        // 标记位必须被删除，否则客户端会跳过设备 ID 重新上报
        assert!(after.get("has_device_id_updated_to_aha").is_none());
        // 无关键必须原样保留
        assert_eq!(after.get("other").unwrap().as_str(), Some("keep"));
        // 不能把点号键拆成嵌套对象
        assert!(after.get("telemetry").is_none());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn rewrite_storage_json_reports_no_change_when_keys_absent() {
        let base = std::env::temp_dir().join(format!("trae-storage-absent-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&base);
        let path = base.join("storage.json");
        std::fs::write(&path, r#"{"unrelated":1}"#).unwrap();
        assert!(!rewrite_storage_json(&path, "m", "g").unwrap());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn rewrite_storage_json_refuses_to_corrupt_broken_files() {
        // 文件不是 JSON 时必须报错且**保持原样**——重写成半截内容会让客户端丢全部本地状态。
        let base = std::env::temp_dir().join(format!("trae-storage-broken-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&base);
        let path = base.join("storage.json");
        let original = "{ not json at all";
        std::fs::write(&path, original).unwrap();

        assert!(rewrite_storage_json(&path, "m", "g").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

        // 顶层是数组而非对象，同样拒绝改写
        std::fs::write(&path, "[1,2,3]").unwrap();
        assert!(rewrite_storage_json(&path, "m", "g").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[1,2,3]");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn purge_files_containing_removes_only_matching_text_files() {
        let base = std::env::temp_dir().join(format!("trae-purge-{}", std::process::id()));
        let nested = base.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(base.join("has.txt"), "x device_id y").unwrap();
        std::fs::write(base.join("plain.txt"), "nothing here").unwrap();
        std::fs::write(nested.join("deep.txt"), "device_id").unwrap();
        // 二进制文件不能被当作文本读取而误删
        std::fs::write(base.join("blob.bin"), [0xff, 0xfe, 0x00, 0x01]).unwrap();

        let removed = purge_files_containing(&base, "device_id");
        assert_eq!(removed, 2);
        assert!(!base.join("has.txt").exists());
        assert!(base.join("plain.txt").exists());
        assert!(!nested.join("deep.txt").exists());
        assert!(base.join("blob.bin").exists());

        assert_eq!(purge_files_containing(&base.join("nope"), "x"), 0);
        let _ = std::fs::remove_dir_all(&base);
    }

    // ---------- 两个区域各一个客户端（CN / 国际）相关 ----------

    /// ★ 跨变体 exe 候选必须**两个区域的合并客户端都在**（issue #28 收敛后共 2 条）。
    ///
    /// 漏掉任一个 → 「明明装了却提示未安装」，以及在跑的那个被判成未运行。
    #[test]
    fn exe_names_cover_both_merged_clients() {
        let names = exe_names();
        let suffix = if cfg!(target_os = "windows") {
            ".exe"
        } else {
            ""
        };
        for expected in ["Trae CN", "Trae"] {
            let full = format!("{expected}{suffix}");
            assert!(
                names.contains(&full.as_str()),
                "候选名缺少 {full}：{names:?}"
            );
        }
        // ★ 退役客户端的名字**不得**再出现在跨变体候选里。
        for retired in ["TRAE SOLO CN", "TRAE SOLO"] {
            let full = format!("{retired}{suffix}");
            assert!(
                !names.contains(&full.as_str()),
                "退役客户端 {full} 仍在跨变体探测候选里"
            );
        }
    }

    /// ★ 「客户端环境」那一行必须能拿到**写侧**目录，而不是「最近活跃」的那个。
    ///
    /// ## 对应用户报障
    ///
    /// 国内版页签的「客户端数据目录」写着**国际版**客户端的目录
    /// （`%APPDATA%\TRAE SOLO`，其 `packageType = SOLO_I18N`），而登录态在
    /// `TRAE SOLO CN` 里。真因是那一行读的是 `get_trae_env`（**跨变体**全局探测），
    /// 而区域页必须用本区域的目录；且**本区域**里也还得选对来源 ——
    /// 「最近活跃」与「首个存在」在本机不是同一个目录。
    ///
    /// ## ★ issue #28 后这条用例拆成两半
    ///
    /// 「活跃 ≠ 首个存在」这个前提来自 `TraeWork` 的**两个候选目录**
    /// （`TRAE SOLO CN` / `TRAE SOLO` —— 那条跨区重叠缺陷）。收敛后每个区域只有一个
    /// 程序位、一个候选目录 ⇒ 在线状态上**构造不出**分叉。
    /// ⇒ ① 分叉判据改到函数层（`select_data_dir_for` vs `detect_data_dir_for`，
    ///    仍用退役的 `TraeWork`，那份重叠数据仍在 `all_known_specs` 的候选名里）；
    /// ② 「状态响应里的 `writeDataDir` 必须等于写侧取值」这条线上契约仍在。
    #[cfg(windows)]
    #[test]
    fn write_side_dir_is_the_first_existing_candidate_not_the_most_active() {
        let exp = chrono::Utc::now().timestamp() + 3600;
        let env = crate::modules::trae::test_support::TempEnv::empty();
        // 退役变体仍有 2 个候选目录 —— 这正是本用例要的「活跃 ≠ 首个存在」形态。
        let variant = crate::modules::trae::variant::TraeVariant::TraeWork;
        let names = data_dir_names_for(variant);
        assert!(
            names.len() >= 2,
            "本用例需要至少两个候选目录才能构造「活跃 ≠ 首个存在」"
        );

        // 全部候选都存在：**首位**装着登录态但不活跃，**末位**活跃但无登录态 —— 真机同形。
        let mut cells = vec![(true, false, false); names.len()];
        cells[0] = (true, true, false);
        cells[names.len() - 1] = (true, false, true);
        let grid = crate::modules::trae::icube::test_support::write_selection_grid(
            &env.appdata(),
            variant,
            &cells,
            exp,
        );
        assert!(
            grid.selectors_diverge,
            "前置：两个选择器必须分叉，否则本用例证明不了什么"
        );

        // ① 两个选择器在同一个变体上确实分叉（活跃度 ≠ 首个存在）。
        let active = select_data_dir_for(variant).expect("候选都在，应能选出活跃目录");
        let write_side = detect_data_dir_for(variant).expect("候选都在，应能选出写侧目录");
        assert_ne!(
            active, write_side,
            "前置：两个选择器必须分叉，否则本用例证明不了任何事"
        );

        // ② 线上契约：`variants_status` 的每个程序位都必须带**写侧**目录，
        //    且它等于写侧取值点 `detect_data_dir_for`（收敛后每个区域只有一个候选目录）。
        let merged = crate::modules::trae::variant::TraeVariant::TraeMerged;

        // 只比 basename：绝对路径前缀依赖临时目录，比较它只会让断言更脆。
        let basename = |item: &Value, key: &str| -> String {
            item[key]
                .as_str()
                .and_then(|path| std::path::Path::new(path).file_name())
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string()
        };
        // 合并版的 userData 目录也必须存在，否则 `writeDataDirExists` 恒为 false。
        let merged_dir = env.appdata().join("Trae CN");
        std::fs::create_dir_all(&merged_dir).expect("合并版 userData 应能创建");
        let value = variants_status();
        let cn = value["variants"]
            .as_array()
            .expect("variants 应是数组")
            .iter()
            .find(|item| item["variant"].as_str() == Some("cn"))
            .expect("应有 cn 区域");
        let program = &cn["programs"]
            .as_array()
            .expect("programs 应是数组")
            .iter()
            .find(|item| item["variant"].as_str() == Some(merged.as_str()))
            .expect("cn 区域应有合并版程序位")
            .clone();
        assert_eq!(
            basename(program, "writeDataDir"),
            "Trae CN",
            "writeDataDir 必须是写侧（首个存在的候选）—— 区域页核对「登录态在哪」用的是它"
        );
        assert_eq!(
            program["writeDataDirExists"],
            Value::Bool(true),
            "目录存在时 writeDataDirExists 必须为 true"
        );
    }

    #[test]
    fn data_dir_names_cover_the_trae_cn_product_line() {
        let names = data_dir_names();
        assert!(
            names.contains(&"Trae CN"),
            "userData 候选缺少 Trae CN：{names:?}"
        );
        // ★ issue #28：主候选改为 CN 合并版（它是「一个候选都不存在」时的兜底返回值）。
        assert_eq!(names[0], "Trae CN");
    }

    #[test]
    fn data_dir_activity_prefers_the_more_recently_written_marker() {
        let base = std::env::temp_dir().join(format!("trae-activity-{}", std::process::id()));
        let old = base.join("old");
        let fresh = base.join("fresh");
        for dir in [&old, &fresh] {
            let storage = dir.join("User").join("globalStorage");
            std::fs::create_dir_all(&storage).unwrap();
            std::fs::write(storage.join("storage.json"), b"{}").unwrap();
        }
        // 隔开一点再写 fresh，确保两者 mtime 明确不同（NTFS 精度远高于 20ms）
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(
            fresh
                .join("User")
                .join("globalStorage")
                .join("storage.json"),
            br#"{"touched":1}"#,
        )
        .unwrap();

        let old_activity = data_dir_activity(&old).expect("old 应有活跃时间");
        let fresh_activity = data_dir_activity(&fresh).expect("fresh 应有活跃时间");
        assert!(
            fresh_activity > old_activity,
            "刚写过的目录活跃时间应更晚：fresh={fresh_activity:?} old={old_activity:?}"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn data_dir_activity_falls_back_to_dir_mtime_then_none() {
        let base = std::env::temp_dir().join(format!("trae-activity-fb-{}", std::process::id()));
        let empty = base.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        // 一个 marker 都没有时回落到目录自身 mtime，而不是直接判成「从未使用」
        assert!(data_dir_activity(&empty).is_some());
        // 目录不存在才是 None
        assert!(data_dir_activity(&base.join("missing")).is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn data_dir_names_by_activity_is_a_permutation_of_candidates() {
        // 排序只能改变顺序，不能多出或漏掉候选名。
        let mut ordered = data_dir_names_by_activity();
        assert_eq!(ordered.len(), data_dir_names().len());
        ordered.sort_unstable();
        let mut expected = data_dir_names().to_vec();
        expected.sort_unstable();
        assert_eq!(ordered, expected);
    }

    /// `select_data_dir_for` 必须**只在传入变体的候选里**选，绝不跨变体。
    ///
    /// 这是 Bug 1 的底层护栏：即便另一条产品线的目录更活跃，
    /// TraeWork 的请求也只能命中 `TRAE SOLO CN` / `TRAE SOLO`。
    #[cfg(windows)]
    #[test]
    fn select_data_dir_for_stays_within_the_requested_variant() {
        let base = std::env::temp_dir().join(format!("trae-select-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        // 两条产品线的目录都存在，且把 Trae CN 造得更活跃。
        for name in ["TRAE SOLO CN", "Trae CN"] {
            let storage = base.join(name).join("User").join("globalStorage");
            std::fs::create_dir_all(&storage).unwrap();
            std::fs::write(storage.join("storage.json"), b"{}").unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(
            base.join("Trae CN")
                .join("User")
                .join("globalStorage")
                .join("storage.json"),
            br#"{"touched":1}"#,
        )
        .unwrap();

        let _lock = crate::modules::config::env_lock();
        let original = std::env::var("APPDATA").ok();
        std::env::set_var("APPDATA", &base);

        let work = select_data_dir_for(super::super::variant::TraeVariant::TraeWork);
        let cn = select_data_dir_for(super::super::variant::TraeVariant::Trae);

        match original {
            Some(value) => std::env::set_var("APPDATA", value),
            None => std::env::remove_var("APPDATA"),
        }
        let _ = std::fs::remove_dir_all(&base);

        let work_name = work
            .as_ref()
            .and_then(|dir| dir.file_name())
            .map(|name| name.to_string_lossy().to_string());
        assert_eq!(
            work_name.as_deref(),
            Some("TRAE SOLO CN"),
            "TraeWork 只能落在 Trae Work 的候选目录里"
        );
        let cn_name = cn
            .as_ref()
            .and_then(|dir| dir.file_name())
            .map(|name| name.to_string_lossy().to_string());
        assert_eq!(cn_name.as_deref(), Some("Trae CN"));
    }

    /// 变体没有任何存在的候选目录时，`select_data_dir_for` 必须返回 `None`
    /// （而不是含糊地退回一个不存在的路径，那会让调用方以为「目录存在」）。
    #[cfg(windows)]
    #[test]
    fn select_data_dir_for_returns_none_when_variant_has_no_dir() {
        let base = std::env::temp_dir().join(format!("trae-select-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let _lock = crate::modules::config::env_lock();
        let original = std::env::var("APPDATA").ok();
        std::env::set_var("APPDATA", &base);

        let work = select_data_dir_for(super::super::variant::TraeVariant::TraeWork);
        let cn = select_data_dir_for(super::super::variant::TraeVariant::Trae);

        match original {
            Some(value) => std::env::set_var("APPDATA", value),
            None => std::env::remove_var("APPDATA"),
        }
        let _ = std::fs::remove_dir_all(&base);

        assert!(work.is_none(), "两条候选目录都不存在时必须是 None");
        assert!(cn.is_none(), "两条候选目录都不存在时必须是 None");
    }

    // ---------- T13-1：两个选择器的语义分家（R3 / R4） ----------

    /// ★【R3】`detect_data_dir_for`（**写侧来源**）必须取**候选表里第一个存在**的目录。
    ///
    /// 真机形态：只装了 `TRAE SOLO`（没有 `TRAE SOLO CN`）。改前恒取 `names[0]`，
    /// 于是返回一个**不存在**的路径，备份 / 恢复 / 守卫全部落空 ——
    /// 症状是「Trae 明明在用，切换器却说找不到数据目录」。
    #[cfg(windows)]
    #[test]
    fn detect_data_dir_for_picks_the_first_existing_candidate() {
        let env = crate::modules::trae::test_support::TempEnv::empty();
        let variant = super::super::variant::TraeVariant::TraeWork;
        // 首位候选**不存在**、次位候选存在且有登录态。
        let grid = crate::modules::trae::icube::test_support::write_selection_grid(
            &env.appdata(),
            variant,
            &[(false, false, false), (true, true, true)],
            chrono::Utc::now().timestamp() + 3600,
        );

        let picked = detect_data_dir_for(variant).expect("临时 APPDATA 下应能定位目录");
        assert_eq!(
            picked.file_name().and_then(|n| n.to_str()),
            Some(grid.cells[1].name),
            "必须跳过不存在的首位候选，选中存在的次位候选"
        );
        assert!(
            picked.is_dir(),
            "返回的目录必须真实存在：{}",
            picked.display()
        );
    }

    /// ★【R3】候选**一个都不存在**时仍回落 `names[0]`（纯展示值），保持既有契约。
    ///
    /// 这条与上一条成对：不能为了「首个存在」把兜底展示值也一起去掉 ——
    /// UI 需要显示「应该把客户端数据放在哪」。
    #[cfg(windows)]
    #[test]
    fn detect_data_dir_for_falls_back_to_primary_name_when_none_exist() {
        let env = crate::modules::trae::test_support::TempEnv::empty();
        let variant = super::super::variant::TraeVariant::TraeWork;
        let grid = crate::modules::trae::icube::test_support::write_selection_grid(
            &env.appdata(),
            variant,
            &[(false, false, false), (false, false, false)],
            chrono::Utc::now().timestamp() + 3600,
        );

        let picked = detect_data_dir_for(variant).expect("临时 APPDATA 下应能定位目录");
        assert_eq!(
            picked.file_name().and_then(|n| n.to_str()),
            Some(grid.cells[0].name),
            "全不存在时应回落到主候选名（展示值）"
        );
        assert!(
            !picked.is_dir(),
            "兜底展示值本就不存在 —— 调用方必须自己判存在性：{}",
            picked.display()
        );
    }

    /// ★【R4 + 排序唯一】`select_data_dir_for`（**读 / 展示侧**）取**最近活跃**的
    /// **存在**候选，与 `detect_data_dir_for` 在同一 fixture 上**分叉**。
    ///
    /// fixture 用四格显式构造：首位存在但**不活跃**、次位存在且**活跃**。
    /// 于是 `detect` → 首位（写侧确定）、`select` → 次位（最活跃）。
    /// 两者分叉这件事由 [`SelectionGrid::selectors_diverge`] 在构造时算好，
    /// 避免用例各写一遍比较（比较写错或 fixture 碰巧不分叉时会**假绿**）。
    #[cfg(windows)]
    #[test]
    fn select_data_dir_for_picks_the_most_active_existing_candidate() {
        let env = crate::modules::trae::test_support::TempEnv::empty();
        let variant = super::super::variant::TraeVariant::TraeWork;
        let grid = crate::modules::trae::icube::test_support::write_selection_grid(
            &env.appdata(),
            variant,
            &[(true, true, false), (true, true, true)],
            chrono::Utc::now().timestamp() + 3600,
        );
        assert!(
            grid.selectors_diverge,
            "前置：本 fixture 必须让两个选择器分叉（否则本用例证明不了任何事）"
        );
        assert!(
            !grid.cells[0].active && grid.cells[1].active,
            "前置：四格必须把「首位不活跃、次位活跃」显式钉死"
        );

        let picked = select_data_dir_for(variant).expect("应选中一个存在的候选");
        assert_eq!(
            picked.file_name().and_then(|n| n.to_str()),
            Some(grid.cells[1].name),
            "select 必须取**最近活跃**的那个（次位），而不是候选表首位"
        );
        assert_eq!(
            detect_data_dir_for(variant)
                .and_then(|d| d.file_name().map(|n| n.to_string_lossy().to_string())),
            Some(grid.cells[0].name.to_string()),
            "同一 fixture 下 detect 必须取候选表首个存在（首位）"
        );
    }

    /// ★【排序唯一】`select_data_dir_for` **只**在该变体的候选里选，且**只**返回存在的目录。
    ///
    /// 这一条同时钉住「按变体限定」（不跨变体）与「不存在的不返回」两件事。
    #[cfg(windows)]
    #[test]
    fn select_data_dir_for_only_returns_existing_dirs_of_that_variant() {
        let env = crate::modules::trae::test_support::TempEnv::empty();
        let variant = super::super::variant::TraeVariant::TraeWork;
        let grid = crate::modules::trae::icube::test_support::write_selection_grid(
            &env.appdata(),
            variant,
            &[(true, false, false), (false, false, false)],
            chrono::Utc::now().timestamp() + 3600,
        );

        let picked = select_data_dir_for(variant).expect("首位存在，应能选中");
        assert_eq!(
            picked.file_name().and_then(|n| n.to_str()),
            Some(grid.cells[0].name)
        );
        assert!(picked.is_dir());
    }

    #[cfg(windows)]
    #[test]
    fn secondary_drive_roots_only_return_existing_directories() {
        for root in secondary_drive_roots() {
            assert!(root.is_dir(), "返回了不存在的根目录：{}", root.display());
            let text = root.to_string_lossy().to_uppercase();
            // C 盘交给 %LOCALAPPDATA%\Programs 与 ProgramFiles，不要重复探测
            assert!(
                !text.starts_with("C:"),
                "C 盘不应出现在非系统盘根目录里：{text}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn running_processes_lists_only_registered_names() {
        // 只读探测，且不得返回候选表以外的名字（否则 taskkill 会打错目标）。
        let running = running_processes();
        eprintln!("[trae] running_processes -> {running:?}");
        for name in &running {
            assert!(exe_names().contains(name), "返回了未登记的进程名：{name}");
        }
    }

    #[test]
    fn detect_install_returns_a_registered_exe_name_when_installed() {
        // 命中安装时，返回的必须是候选表里的文件名（防止拼出错误路径）。
        // 同时把结果打出来，便于用 `--nocapture` 核对本机真实探测结果。
        let probe = detect_install(&[]);
        eprintln!(
            "[trae] detect_install -> installed={} exe={:?} version={:?}",
            probe.installed,
            probe.exe.as_ref().map(|p| p.display().to_string()),
            probe.version
        );
        eprintln!(
            "[trae] detect_data_dir -> {:?}",
            detect_data_dir().map(|p| p.display().to_string())
        );
        if let Some(exe) = probe.exe {
            assert!(exe.is_file(), "返回了不存在的可执行文件：{}", exe.display());
            let file = exe.file_name().unwrap().to_string_lossy().to_string();
            assert!(
                exe_names().contains(&file.as_str()),
                "探测到的可执行文件名不在候选表内：{file}"
            );
        }
    }

    #[test]
    fn detect_install_reads_version_from_the_exe_directory() {
        // 回归：detect_install 曾把**可执行文件路径**当目录传给
        // version_from_install_dir，拼出
        // `...\TRAE SOLO CN.exe\resources\app\package.json` 这种不可能存在的路径，
        // 于是版本号永远是 None —— 界面上只表现为「不显示版本」，很难被发现。
        let temp = std::env::temp_dir().join(format!("fake-trae-ver-exe-{}", std::process::id()));
        let app = temp.join("resources").join("app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("package.json"), br#"{"version":"9.9.9"}"#).unwrap();
        let exe = temp.join(exe_names()[0]);
        std::fs::write(&exe, b"stub").unwrap();

        let probe = detect_install(&[exe.to_string_lossy().to_string()]);
        assert!(probe.installed);
        assert_eq!(probe.version.as_deref(), Some("9.9.9"));
        assert_eq!(probe.exe.as_deref(), Some(exe.as_path()));

        let _ = std::fs::remove_dir_all(&temp);
    }
}
