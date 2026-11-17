# 客户端进程判活：0 线程内核残留不得当作「在运行」

> **状态**：现行 · 2026-11-17 落地
> **适用范围**：Windows 平台的写前进程守卫与「关闭并继续」路径。
> **代码依据**：`crates/buddy-switch-core/src/modules/process.rs`（共享判据）、
> `modules/trae/platform.rs`、`modules/vscode_ext.rs`、`modules/jetbrains.rs`、
> `modules/codebuddy_cn_ide.rs`（各产品入口）。

## 意图

切换账号前要确认客户端**不在运行**，否则客户端退出时会把内存态写回磁盘，
覆盖我们刚写入的凭据，表现为「切换成功但客户端仍是旧账号」。

这条守卫历史上只看**映像名**是否出现在进程列表里。Windows 上有一类
**0 线程内核残留**（EPROCESS 仍被驱动扣住）：`tasklist` 仍会列出它，
但它**没有任何可执行线程**，既不能写库，也**无法被 `taskkill` 清除**。

后果是用户被卡在「关闭并继续」的死循环里：每次都提示进程关不掉，
而实际上客户端窗口早已关闭。

## 判据（唯一事实源）

以 **ToolHelp 进程快照**为准（`CreateToolhelp32Snapshot` + `PROCESSENTRY32W`）：
快照同时给出**映像名**（`szExeFile`）与**线程数**（`cntThreads`），一次快照即可
覆盖全系统，不需要为每个候选名再起 `tasklist` 子进程：

| 观测结果 | 判定 | 理由 |
|:--|:--|:--|
| 线程数 **> 0** | 在运行 | 有活动线程，可能仍在写盘 |
| 线程数 **明确为 0** | **已退出** | 无可执行线程，也不会再写盘 |
| 快照失败 / PID 不在快照里 | **在运行**（fail-closed） | 宁可多等一次，也不能误判退出后去动它的数据 |

最后一行是硬约束：把「查不到」当成「已退出」会让一次枚举抖动直接变成
「客户端已退出」的误判，随后就去改它正在写的数据库 / 认证文件。

## 落地范围

- `process::is_image_running` / `is_workbuddy_running_for`：按线程数过滤候选。
- `process::close_image` / `close_workbuddy_for`：先剔除 0 线程残留再 `taskkill`；
  若全部是残留，直接视为已关闭并成功返回。
- `process::wait_windows_pids_gone`：每轮轮询都用同一判据复判，
  因此残留不会让关闭流程空等一个超时。
- CIM / PowerShell 来源（VS Code、JetBrains、CodeBuddy CN）：额外过一遍
  同一判据 —— **来源不同不改变判据**。
- Trae（`taskkill /IM <exe>`，按映像名匹配）：判活与候选收集都要求
  至少存在一个**有活动线程**的匹配进程；判据直接取自 ToolHelp 快照的映像名，
  不额外起 `tasklist` 子进程。

## 错误文案

关闭超时**不得**再提示「请手动结束进程」：残留进程往往手动也结束不掉。
现行文案改为说明「可能是系统驱动暂未回收的 0 线程内核残留，请等待片刻后重试」。

## 护栏

- 判据集中在 `process.rs` 的纯函数里（`row_has_live_threads` /
  `alive_pids_from_thread_counts`），单测钉住两条语义：
  0 线程判死、未知计数 fail-closed。
- 可证伪：把 `threads > 0` 改成 `threads >= 0`，单测
  `zero_thread_rows_are_not_live` 立刻变红。
