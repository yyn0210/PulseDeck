<div align="center">

# 脉盘 PulseDeck

** 桌面系统监控小工具 + 会随 CPU 温度换装的桌面宠物 + 任务栏显示**

*A lightweight desktop system monitor with a temperature-reactive desktop pet.*

[![License](https://img.shields.io/badge/License-Apache--2.0-blue)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%2B%20x64%20%7C%20Linux%20x64-0078D6)](#下载与安装)
[![Tauri](https://img.shields.io/badge/Tauri-2.x-FFC131?logo=tauri&logoColor=black)](https://tauri.app)
[![Release](https://img.shields.io/github/v/release/yyn0210/PulseDeck?display_name=tag&include_prereleases)](https://github.com/yyn0210/PulseDeck/releases)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen)](#参与贡献)

如果它对你有用，欢迎点一个 ⭐ Star 支持一下。

</div>

---

实时监控 **CPU / 内存 / 硬盘 / 电池 / 电源功率 / GPU / 网络 / 温度 / 风扇转速**。
后端 Rust + Tauri 2，前端 Vite + 原生 JS（图表全部 canvas 手绘，**零第三方图表库 / 前端框架**），
托盘常驻 + 开机自启，关闭窗口不退出。数据源**诚实降级**：能读到就报，读不到显示 `--` 并给出具体原因，
绝不拿占空比冒充转速。

<p align="center">
  <img src="shots/ui_full.png" alt="PulseDeck 主界面" width="880">
</p>

## 目录

- [功能特性](#功能特性)
- [界面一览](#界面一览)
- [下载与安装](#下载与安装)
- [快速上手](#快速上手)
- [桌面宠物](#桌面宠物)
- [任务栏显示](#任务栏显示)
- [数据来源与降级策略](#数据来源与降级策略)
- [从源码构建](#从源码构建)
- [已知限制](#已知限制)
- [路线图](#路线图)
- [参与贡献](#参与贡献)
- [致谢](#致谢)
- [许可证](#许可证)

## 功能特性

- **一个面板看全整机**：CPU（占用 / 每核 / 主频 / 温度 / 风扇）、内存（含已提交内存、页面文件）、
  硬盘（容量 / 读写速率 / 累计量）、电池与电源功率（含 CPU 封装功耗）、GPU（占用 / 显存 / 温度 / 功耗 / 风扇）、
  网络（速率 / 累计流量 / 每网卡明细）、Top 进程榜（CPU / 内存各 8）
- **温度与风扇读数是真的**：CPU 温度走 PawnIO 直读片内传感器（ **Intel 读 MSR 的 DTS、AMD 读 SMN** ，
  最准路径），联想机型还能读厂商私有的真实风扇 RPM——绝不拿占空比冒充转速
- **桌面宠物**：置顶小狐狸在桌面上跑动，显示 CPU / 内存 / GPU 使用率，
  **形象随 CPU 温度 6 档切换**（38 蓝 → 94 红），过热档还会加速喘气
- **任务栏显示**：把 CPU / 内存 / GPU / 网络 / 磁盘 / 电池 / 温度 / 功率贴在 Windows 任务栏上的一条
  细挂件里（任务栏显示模式）：停在通知区域（托盘图标 / 时钟）左侧、不抢焦点、不进 Alt+Tab，
  全屏游戏 / 视频时自动隐藏（Linux 上为贴屏幕右下角的置顶边条，见支持矩阵）
- **托盘常驻**：关窗不退出，后台继续采样；支持开机自启、暂停刷新
- **轻量原生**：Tauri 2 打包，无 Electron、无第三方图表库，canvas 手绘所有图表
- **隐私友好**：全部采集本地完成，无任何上传 / 遥测
- **提权最小化**：主程序普通权限运行；读 MSR / 风扇由按需拉起的提权子进程完成，
  一次 UAC 搞定，绝不静默安装驱动

## 界面一览

| 卡片     | 内容                                                          |
| ------ | ----------------------------------------------------------- |
| 处理器    | 总占用环形图、型号、物理核/线程数、当前主频、**温度**、**风扇转速**、每核负载网格               |
| 内存     | 已用/总量、可用、已提交内存（Commit Charge）、页面文件                          |
| GPU    | 占用环形图、型号、显存占用、温度、功耗、**风扇转速**；多卡时列出全部并取主卡汇总                  |
| 电池     | 剩余电量、剩余/满充容量（Wh）、剩余续航、充满所需时间、充放电状态                          |
| 电源功率   | 电池实时功率（W，充入 / 输出 / 已接入未充电时给 0）、**CPU 封装功耗（W）**、电池状态、是否接入适配器 |
| 网络     | 总下行/上行速率、累计流量、速率趋势折线（自适应量程）、每张网卡明细                          |
| 硬盘     | 每个卷的容量占比、读/写速率、开机以来累计读写量、文件系统与介质类型                          |
| Top 进程 | CPU 占用 Top 8 / 内存占用 Top 8                                   |

顶部导航在 **监控** / **设置** 两个页面间切换（会记住上次停留的页面）：

| 设置页分组  | 内容                                              |
| ------ | ----------------------------------------------- |
| 常规     | 开机自启、关闭时最小化到托盘、是否列出无流量网卡                        |
| 采样     | 刷新间隔（500 ms / 1 s / 2 s / 5 s）                  |
| 桌面宠物   | 是否启用、显示内容（CPU / 内存 / GPU 使用率）、不透明度；形象按 CPU 温度分 6 档切换 |
| 任务栏显示  | 是否启用、显示内容（CPU / 内存 / GPU / 网络 / 磁盘 / 电池 / 温度 / 功率）、挂件状态行 |
| 高级采集   | 各采集通道状态（PawnIO 内核驱动、厂商风扇通道）+ 未就绪时的操作入口          |

> 窗口默认 **1240×820**、可自由缩放。内容量会随硬件规模（核心数 / 卷数 / 网卡数）变化，
> 当内容高于窗口时，**页面内部会出现纵向滚动条**——向下滚动即可看到下方全部内容。
>
> 窗口没有系统标题栏，顶上那条深色标题栏**是页面自绘的**（Windows 与 Linux 统一观感）：
> 左端是品牌标记，右端是最小化 / 最大化 / 关闭，整条可拖动、双击切最大化，
> 四边四角用隐形热区拖拽缩放。

## 下载与安装

### 系统要求

**Windows**

| 项 | 要求 |
| -- | -- |
| 操作系统 | **Windows 10 x64（build 17763 / 1809）及以上**（也是随包分发 PawnIO 驱动的前置条件） |
| 架构 | 仅 x86_64 |
| WebView2 | 需要运行时（Windows 11 一般已内置；安装包缺失时会引导安装） |
| 权限 | 普通用户权限即可运行；CPU 温度 / 风扇等高级采集需管理员（按需授权一次） |

**Linux**

| 项 | 要求 |
| -- | -- |
| 操作系统 | x86_64 桌面发行版（deb 系用 `.deb`；其他发行版用 `.AppImage`） |
| 架构 | 仅 x86_64 |
| WebView | webkit2gtk 4.1（`.deb` 已声明依赖，安装时自动带上；AppImage 自带） |
| 桌面环境 | X11 或 Wayland；托盘图标需要应用指示器支持（GNOME 需装 AppIndicator 扩展） |
| 权限 | 普通用户即可；CPU 温度 / 风扇走内核 hwmon，**无需任何提权** |

### 安装

**Windows**

1. 打开 [Releases](https://github.com/yyn0210/PulseDeck/releases) 页面；
2. 下载最新版 NSIS 安装包（`PulseDeck_x.y.z_x64-setup.exe`）并双击安装；
3. 启动后即可使用，**无需联网、无需管理员**。

**Linux**

1. 打开 [Releases](https://github.com/yyn0210/PulseDeck/releases) 页面；
2. deb 系发行版（Debian / Ubuntu / Deepin / UOS 等）下载 `PulseDeck_x.y.z_amd64.deb`，
   执行 `sudo apt install ./PulseDeck_x.y.z_amd64.deb`；
3. 其他发行版下载 `PulseDeck_x.y.z_amd64.AppImage`，`chmod +x` 后直接运行
   （部分发行版需要 `libfuse2`）。

Linux 上**不需要 PawnIO**：CPU 温度 / 风扇 / 电池 / 已提交内存分别走内核 hwmon、
`/sys/class/power_supply` 与 `/proc/meminfo`，普通权限直读，设置页也不会出现 Windows 的
「高级采集」区块。

**关于高级采集（可选，仅 Windows）**：CPU 温度最准的路径（Intel 读 MSR、AMD 读 SMN）与联想风扇
转速需要内核驱动 PawnIO。本工具**绝不静默安装**——未安装时只是顶部多一条可关闭的提醒，安装入口在
**设置 → 高级采集**，点按钮确认一次 UAC 即可（安装器随包分发，全程离线可用）。
不装也能正常使用，温度会回退到 ACPI / PDH 通道。

## 快速上手

- **托盘与自启**：
  - 关闭主窗口默认**隐藏到托盘**，后台采样线程继续运行；托盘菜单选「退出」才真正结束；
  - 左键单击托盘图标呼出面板，右键展开菜单；
  - 托盘菜单提供：显示面板、暂停刷新、开机自启（Windows 写注册表 Run 项，Linux 写 `~/.config/autostart`）、退出；
  - 设置页「常规」可以关闭「关闭时最小化到托盘」，让关闭按钮直接退出。
- **顶部提示条**：未安装 PawnIO 时，监控页顶部会出现一条可关闭的提醒（`#tipbar`），
  点「去设置」直接跳到设置页「高级采集」；点「×」后可选 **「永久关闭」**（跨启动生效）或
  **「仅本次」**（只挡本次运行），也可「取消」。永久关闭后，设置页「高级采集」底部会留一条
  「重新开启」链接，避免变成单向门。提示条只做提醒，**不在主页面弹 UAC**——安装入口始终在设置页。
- **可调项**：
  - 采样间隔：设置页「采样 → 刷新间隔」下拉框（500 ms / 1 s / 2 s / 5 s），同步调整后端采样线程间隔；
  - 暂停/继续：停止前后端轮询，保留最后一帧数据（右上角按钮，托盘菜单里也能切）；
  - 开机自启：设置页「常规」开关，或托盘菜单直接勾选；
  - 桌面宠物 / 任务栏显示：设置页对应卡片（开关 + 显示内容），改完立即生效。

## 桌面宠物

设置页「桌面宠物」里开启后，桌面上会出现一只**置顶、无边框、透明、不进任务栏**的小宠物：
显示勾选的指标（CPU / 内存 / GPU 使用率），**形象随 CPU 温度换档**。

<p align="center">
  <img src="shots/ui_pet_final_v3.png" alt="桌面宠物：随温度换档的小狐狸" width="360">
</p>

- 形象是 **6 档 × 每档 8 帧**的机器人狐狸贴图：
  `38 蓝 → 50 绿 → 63 黄绿 → 74 黄 → 84 橙 → 94 红`（与设置页色带同一组刻度）。
  档位内 8 帧循环播放（260ms/帧，约 3.8fps；整段播完约 2 秒）；**跨档位时整段换成新档位的 8 帧序列**（从第 0 帧起播）。
  温度 `≥84℃` 的两档算「过热」：帧速加快到 200ms/帧（约 5fps），脚下温度文字变橙。
- 档位按**就近**取刻度（如 68℃ → 63 档、88℃ → 84 档；正好居中时取较冷的一档）。
- **温度读不到时**（多数机器要先装 PawnIO 驱动才有 CPU 温度）自动**退回按 CPU 占用选档**，
  并在脚下标注「温度 -- · 按占用切换形象」、悬浮提示里说明原因——**不做无声降级**。
- 整只宠物可拖动，**双击呼出主面板**（主面板常驻托盘时很方便）。
- 宠物窗口与主窗口生命周期解耦：主面板「关闭时最小化到托盘」不影响宠物。

## 任务栏显示

设置页「任务栏显示」里开启后，任务栏上会多出一条**细挂件**：默认停在**通知区域（托盘图标 / 时钟）左侧**，
实时显示你勾选的指标。它相当于系统监控类工具里的「任务栏显示模式」，但**不修改系统任务栏**——
是一条贴在任务栏上的置顶无边框小窗。

- **可选内容**（设置页勾选，至少一项，按勾选顺序排列）：
  | 键 | 显示 | 缺数据时 |
  | -- | -- | -- |
  | `cpu` / `mem` / `gpu` | `CPU 34%`（占用 ≥60% 转黄、≥85% 转红） | GPU 读不到显示 `GPU --` |
  | `net` | `↓1.24MB/s ↑56KB/s`（下行绿 / 上行黄，全机合计） | — |
  | `disk` | `读 2.1MB/s 写 300KB/s`（所有卷合计） | — |
  | `bat` | `电池 78%`（≤30% 黄、≤15% 红） | 无电池显示 `电池 --` |
  | `temp` | `温度 62℃`（≥74℃ 黄、≥84℃ 红，与宠物分档同源） | 读不到显示 `温度 --` |
  | `power` | `功率 19.4W`：优先电池充/放电功率，没充没放时是 CPU 封装功耗 | 都没有显示 `功率 --` |
- **不抢焦点**：点击挂件不会把焦点从你正在用的应用上抢走；也**不进 Alt+Tab、不在任务栏里多出按钮**。
- **单击挂件呼出主面板**（和托盘图标一样方便）；主面板最小化到托盘时挂件照常贴在任务栏上。
- **全屏应用在前台时自动隐藏**（游戏 / 播放器 / 网页全屏视频），退出全屏就回来——不会压在你的视频画面上。
- **跟着任务栏走**：任务栏换成上下左右任一条边、改厚度（小图标 / 大图标）、改 DPI、切浅色 / 深色主题，
  挂件都会在一秒内跟过去，explorer 重启也能自愈。浅色任务栏用深色文字、深色任务栏用浅色文字。
- 开了**任务栏自动隐藏**时，挂件贴在屏幕底边——那一处正是任务栏弹出来的位置（任务栏滑走了，挂件仍然可见）。

## 数据来源与降级策略

统一由 Rust 侧在**后台采样线程**定时刷新，前端只拉取最后一次快照，避免多窗口重复采样；
速率类指标一律用**实测采样间隔**（而非配置值）换算。

### 平台支持矩阵

| 功能 | Windows | Linux | macOS |
| --- | --- | --- | --- |
| CPU / 内存 / 磁盘 / 网络 / 进程（sysinfo） | ✅ | ✅ | ✅ |
| GPU（占用 / 显存 / 温度 / 功耗 / 风扇） | NVML + PDH 兜底 | NVML + amdgpu sysfs 兜底 | — |
| CPU 温度 | PawnIO 直读 MSR（Intel）/ SMN（AMD）→ LHM → ACPI → PDH 四级 | hwmon（coretemp / k10temp / zenpower） | — |
| CPU 封装功耗 | PDH RAPL（Intel）/ MSR 能量计数器（AMD） | RAPL powercap（Intel） | — |
| 风扇转速 | 联想厂商通道 / LHM / ACPI | hwmon fan / pwm | — |
| 电池与充电功率 | CallNtPowerInformation | `/sys/class/power_supply` | — |
| 已提交内存 | GlobalMemoryStatusEx | `/proc/meminfo`（Committed_AS） | — |
| 桌面宠物 | ✅ | ✅ | ✅ |
| 任务栏挂件 | 贴任务栏条带（含全屏自动隐藏、主题跟随） | 贴屏幕右下角边条（无全屏隐藏、主题读 gsettings） | 同 Linux |
| 提权桥接（PawnIO） | ✅（设置页「高级采集」） | 不需要（hwmon 免提权） | — |

未适配平台（macOS 等）统一报「当前操作系统暂不支持该指标的采集」，绝不编造数值；
macOS 的温度 / 风扇 / 电池功率需要 Mac 才能开发与验证（IOKit / SMC），暂不实现。

### 关键通道

- **CPU 温度**：PawnIO 直读片内传感器（最准，需管理员，一次 UAC）——
  **Intel 读 MSR 的 DTS 寄存器（`0x1B1` / `0x19C`），AMD 读 SMN 的 `THM_TCON_CUR_TMP`**，
  两个模块都随包分发、官方签名，装完 PawnIO 驱动即可用 →
  硬件监控软件的 WMI `Sensor` 表（只对 OpenHardwareMonitor / 旧版 LHM ≤ 0.9.4 有效）→
  ACPI 热区 `MSAcpi_ThermalZoneTemperature`（联想等机型免装软件即可读到）→ PDH（含有效性校验）。
  四层都拿不到就显示 `--` 并给出原因。AMD 侧要求 **Zen 架构（family 17h~1Ah）**，
  更早的 AMD 平台仍只能靠 ACPI / PDH。
- **风扇转速**：Windows 没有任何公开的用户态接口。联想机型可走厂商私有 ACPI-WMI 通道读到
  **真实 RPM**（需管理员，与 CPU 温度共用同一个提权子进程，一次 UAC 同时搞定两者）；
  其余机器靠硬件监控软件的 WMI `Sensor` 或 ACPI `Win32_Fan`（绝大多数机器该字段为空）。
  **笔记本独显风扇普遍读不到**（由 EC 控制，NVML 报 `N/A`），桌面 N 卡常有值。
  展示口径：**优先真实 RPM，其次占空比 `%`，两者都没有才显示 `--`**。
- **GPU**：NVIDIA 走 NVML（占用 / 显存 / 温度 / 功耗 / 风扇）；非 N 卡走 PDH 计数器（占用 / 显存）。
- **电源功率**：Windows 只公开电池的充放电速率，**不公开适配器输出功率**；「电源功率」卡片同时给
  电池实时功率与 CPU 封装功耗（Intel 走 PDH RAPL，AMD 走 MSR 能量计数器），
  已接入但未充电时明确显示 `0.0 W`，不留空。
- **读不到时的求助入口**：设置页「高级采集」会写明每个通道的状态与具体原因；
  提权桥接的失败明细在 `%LOCALAPPDATA%\PulseDeck\pawnio.log`。

## 从源码构建

### 环境要求

公共：[Rust](https://rustup.rs)（stable）、[Node.js](https://nodejs.org) ≥ 20.19（Vite 7 的要求）与 npm；
Python 3.11 + Pillow / numpy / scipy 仅**改宠物贴图**时需要。

Windows：`x86_64-pc-windows-msvc` 目标 + VS2022 的 MSVC 工具链（cl.exe / link.exe）、WebView2 运行时。

Linux（deb 系示例）：

```bash
sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

### 构建命令

```bash
git clone https://github.com/yyn0210/PulseDeck.git
cd PulseDeck
npm ci             # 按 package-lock.json 精确安装（改过 package.json 依赖后改用 npm install 刷新锁文件）
npm run dev        # 开发模式（Vite dev server :1420 + Tauri 窗口；期间保持 Vite 存活）
npm run build      # 完整打包：前端 + 二进制 + 当前平台的安装包
npm run build:exe  # 只出二进制（跳过安装包）
```

产物位置：

| 平台 | 二进制 | 安装包 |
| -- | -- | -- |
| Windows | `src-tauri/target/release/pulse-deck.exe` | `src-tauri/target/release/bundle/nsis/*.exe` |
| Linux | `src-tauri/target/release/pulse-deck` | `src-tauri/target/release/bundle/deb/*.deb`、`bundle/appimage/*.AppImage` |

> ⚠️ **不要直接 `cargo build --release`**：`custom-protocol` feature 只有 tauri CLI 会追加，
> 手工 cargo 构建出的 exe 双击会去请求 `localhost:1420` 而白屏。一律走上面的 npm 脚本。

## 已知限制

- **支持 Windows x64 与 Linux x64**（安装包见[下载与安装](#下载与安装)）；macOS 暂无计划——
  温度 / 风扇 / 电池功率等高级指标需要 IOKit / SMC，得有 Mac 才能开发验证；
  两平台各自的采集通道差异见[平台支持矩阵](#平台支持矩阵)；
- **AMD 平台支持 Zen 架构（family 17h~1Ah）的 CPU 温度直读**：走 PawnIO + SMN，与 Intel 的
  MSR 路径同一个提权子进程；**更早的 AMD 平台（FX / Phenom 等）**仍只能靠 ACPI 热区 / PDH，
  能否读到取决于固件；GPU 侧的 AMD 读数（占用 / 显存）走 PDH 计数器，**A 卡温度暂不支持**；
- **非联想机型的 CPU 风扇读不到**：风扇挂在主板 Super I/O / EC 上，通用接口不存在
  （引入第三方监控服务 / 直读 Super I/O 的方案已评估并否决）；
- **笔记本 N 卡风扇普遍 `N/A`**（由 EC 控制，NVML 读不到），桌面 N 卡常有值；
- 部分老平台 / 虚拟机没有 RAPL 能量计数器，CPU 封装功耗显示 `--`；
- Windows 不公开适配器输出功率，「电源功率」只能给电池侧功率 + CPU 封装功耗；
- 桌面宠物目前是固定一套机器人狐狸素材，暂不支持自定义形象；
- **窗口用的是自绘标题栏**（Windows 与 Linux 一致），代价是 Windows 11 的**贴靠布局**
  （悬停最大化按钮弹出的那套分屏菜单）不再出现——`Win` + 方向键的分屏不受影响；
  最大化 / 最小化 / 关闭 / 边缘拖拽缩放都正常；
- **任务栏挂件只贴主显示器的任务栏**：开了「在所有显示器上显示任务栏」时，副屏那几条任务栏上不会出现挂件；
- 挂件的位置去 **通知区域左侧**，不占任务栏按钮区：勾了特别多指标（或屏幕很窄）时
  会压住左边的任务栏按钮，此时建议少勾几项；
- 任务栏开「自动隐藏」时挂件固定贴在屏幕底边（那里是任务栏弹出来的位置），
  关掉自动隐藏才会重新回到通知区域左侧。

## 路线图

- 采样数据落盘 CSV，用于长时间基线分析
- 进程级 GPU 归属（`GPU Engine` 计数器里本来就带 pid，可以做成 GPU 版进程榜）
- 告警阈值：CPU 温度 / GPU 温度超阈值时托盘气泡提醒

## 参与贡献

欢迎 Issue 与 PR！

1. Fork 本仓库并创建你的分支（如 `feat/xxx`）；
2. 按[从源码构建](#从源码构建)搭好环境，`npm run dev` 跑起来；
3. 提交前跑通测试：`cargo test -p pulse-deck`；动了宠物贴图还要过
   `python tools/check_pet_frames.py`；
4. commit message 用中文约定式前缀（`feat(fan):` / `fix(pet):` / `docs:` …），一个提交做一件事；
5. 发 PR 时简单说清改了什么、在什么机型上验证过（CPU / GPU / 系统版本）。

**改动采集类代码前请先读 `docs/` 下的机制分析笔记**（联想通道逆向、风扇转速方案评估）。
数据采集类 Issue 建议附上机型信息与 `%LOCALAPPDATA%\PulseDeck\pawnio.log`（如存在），方便定位通道问题。

## 致谢

- [Tauri](https://tauri.app) / [wry](https://github.com/tauri-apps/wry) —— 应用框架与 WebView 封装
- [sysinfo](https://github.com/GuillaumeGomez/sysinfo) —— CPU / 内存 / 磁盘 / 网络采样
- [nvml-wrapper](https://github.com/Cldfire/nvml-wrapper) —— NVIDIA NVML 绑定
- [PawnIO](https://pawnio.eu)（[namazso](https://github.com/namazso)）—— 通用内核 IO 驱动；
  IntelMSR / AMDFamily17 模块来自 [namazso/PawnIO.Modules](https://github.com/namazso/PawnIO.Modules)
- [OpenHardwareMonitor](https://openhardwaremonitor.org) / [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor)
  —— WMI `Sensor` 通道的兼容对象与 PawnIO 安装器分发先例
- 厂商风扇通道的适配参考了联想 LegionZone 的可观察行为；同一组标准 WMI 接口亦被 LenovoLegionToolkit 等开源社区工具广泛使用
- 桌面宠物的狐狸形象由豆包（字节跳动）AI 图像生成，素材声明见 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)

## 许可证

本项目自身代码以 **Apache-2.0** 发布，见 [LICENSE](LICENSE)。

随包分发的第三方组件有其独立条款，均记录在 `src-tauri/resources/pawnio/NOTICE.md`：

| 组件 | 许可 | 分发要求 |
| -- | -- | -- |
| `PawnIO_setup.exe`（官方安装器） | 专有（All rights reserved） | **仅允许原样（unmodified）再分发**，改一个字节即违约且破坏 Authenticode 签名 |
| `IntelMSR.bin` / `AMDFamily17.bin`（PawnIO 模块） | LGPL-2.1 | 来源 namazso/PawnIO.Modules v0.2.3，官方签名、原样分发（逐文件 SHA-256 见 `NOTICE.md`） |

随包分发的**全部第三方内容**——上表组件、宠物贴图的 AI 素材来源、全部 Rust/npm 依赖清单
及其许可证全文——汇总记录在 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)。
