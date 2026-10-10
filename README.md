# Win 系统监控（Tauri）

Windows 桌面系统监控小工具，实时监控 **CPU / 内存 / 硬盘 / 电池 / 电源功率 / GPU / 网络 / 温度**。
后端 Rust + Tauri 2，前端 Vite + 原生 JS（图表全部 canvas 手绘，无第三方图表库）。
托盘常驻 + 开机自启，关闭窗口不退出。

## 界面

| 卡片 | 内容 |
| --- | --- |
| 处理器 | 总占用环形图、型号、物理核/线程数、当前主频、**温度**、**风扇转速**、每核负载网格 |
| 内存 | 已用/总量、可用、已提交内存（Commit Charge）、页面文件 |
| GPU | 占用环形图、型号、显存占用、温度、功耗、**风扇转速**；多卡时列出全部并取主卡汇总 |
| 电池 | 剩余电量、剩余/满充容量（Wh）、剩余续航、充满所需时间、充放电状态 |
| 电源功率 | 电池实时功率（W，充入 / 输出 / 已接入未充电时给 0）、**CPU 封装功耗（W）**、电池状态、是否接入适配器 |
| 网络 | 总下行/上行速率、累计流量、速率趋势折线（自适应量程）、每张网卡明细 |
| 硬盘 | 每个卷的容量占比、读/写速率、开机以来累计读写量、文件系统与介质类型 |
| Top 进程 | CPU 占用 Top 8 / 内存占用 Top 8 |
| 偏好设置 | 开机自启、关闭最小化到托盘、是否列出无流量网卡 |

> 窗口默认 **1240×820**、可自由缩放。内容量会随硬件规模（核心数 / 卷数 / 网卡数）变化，
> 当内容高于窗口时，**卡片区（`.grid`）内部会出现纵向滚动条**——向下滚动即可看到下方全部内容，
> 不再把网格行压扁，也不会让卡片内容互相叠压。
>
> 布局约束（改动前务必理解）：`#app` 固定为 `height:100vh; overflow:hidden`，
> `.grid` 是唯一的滚动容器（`flex:1 1 auto; min-height:0; overflow-y:auto`）。
> 关键是 `.grid` 必须声明 **`grid-auto-rows: max-content`**——否则 `.card{min-height:0}`
> 会让隐式行的 min-content 贡献为 0，配合默认 `align-content:stretch` 把行均摊拉伸到容器高度、
> 压扁卡片。删掉这一行滚动就会失效。

## 托盘与自启

- 关闭主窗口默认**隐藏到托盘**，后台采样线程继续运行；托盘菜单选「退出」才真正结束
- 左键单击托盘图标呼出面板，右键展开菜单
- 托盘菜单提供：显示面板、暂停刷新、开机自启（勾选即写入注册表 Run 项）、退出
- 面板底部的「偏好设置」可以关闭「关闭时最小化到托盘」，让关闭按钮直接退出

## 数据来源

统一由 Rust 侧在**后台采样线程**每 N 毫秒刷新一次，前端只拉取最后一次快照，
避免多窗口/多次刷新导致重复采样。速率类指标一律用**实测采样间隔**（而非配置值）换算。

| 指标 | 接口 |
| --- | --- |
| CPU 占用 / 每核 / 主频 | `sysinfo` (`System::refresh_cpu_all`) |
| 内存、页面文件 | `sysinfo` (`System::refresh_memory`) |
| 已提交内存 | Win32 `GlobalMemoryStatusEx` |
| 磁盘容量 | `sysinfo::Disks` |
| 磁盘读/写速率 | `Disk::usage()` 的 `read_bytes` / `written_bytes` 增量 ÷ 实测采样间隔 |
| 网络收/发速率 | `sysinfo::Networks` 的 `received()` / `transmitted()` 增量 ÷ 实测采样间隔 |
| 进程榜 | `sysinfo` (`System::refresh_processes`)，进程 CPU% 已 ÷逻辑核心数换算成「占总 CPU 算力」口径，与全局 CPU 一致 |
| GPU 占用 / 显存 / **温度** / 功耗 | NVIDIA：NVML（`nvml-wrapper`，动态加载 nvml.dll） |
| GPU 占用 / 显存（非 N 卡） | PDH `GPU Engine(*)\Utilization Percentage`、`GPU Adapter Memory(*)\Dedicated Usage` |
| **GPU 风扇** | ① NVML `nvmlDeviceGetFanSpeed`（占空比 %，桌面 N 卡常有值）／`nvmlDeviceGetFanSpeedRPM`（真转速，视型号）；② 硬件监控软件的 WMI `Sensor(SensorType='Fan')`（RPM） |
| **CPU 风扇** | ① **厂商私有通道（联想机型）**：WMI `root\WMI:LENOVO_FAN_METHOD.Fan_GetCurrentFanSpeed` → **RPM**（ACPI-WMI 映射，**需管理员**，由提权桥接子进程供数）；② 硬件监控软件（OpenHardwareMonitor / 旧版 LHM ≤ 0.9.4）的 WMI `Sensor(SensorType='Fan')` → **RPM**；③ WMI `Win32_Fan.DesiredSpeed`（ACPI，**绝大多数机器该字段为空**，有值也不保证是 RPM，仅当占空比展示） |
| CPU 温度 | ① **PawnIO + Intel MSR**：直读片内 DTS 寄存器（`0x1A2` 取 TjMax，`0x1B1` 封装、`0x19C` 逐核心），最准；需管理员，由提权桥接子进程供数（见下方「CPU 温度」）；② 硬件监控软件（OpenHardwareMonitor / 旧版 LHM ≤ 0.9.4）的 WMI `Sensor` 表；③ WMI `root\wmi:MSAcpi_ThermalZoneTemperature`（ACPI 热区，联想 LegionZone 等厂商工具的通用兜底，**联想机型免装软件即可读到 CPU 热区**）；④ PDH `Thermal Zone Information(*)\Temperature`（含有效性校验） |
| **CPU 封装功耗（W）** | PDH `\Energy Meter(*)\Power`（Intel / AMD RAPL，单位为 mW，取 `*_pkg` 实例） |
| 电池百分比、充放电方向、充放电功率、剩余/满充容量、预估时间 | Win32 `CallNtPowerInformation(SystemBatteryState)` |
| 电池降级读数 | Win32 `GetSystemPowerStatus` |
| 显卡名称（PDH 路径） | WMI `Win32_VideoController` |

### 关于几个 Windows 上的「坑」

**电源功率 / 适配充电功率**：Windows 只公开「电池的充放电速率」（`SYSTEM_BATTERY_STATE::Rate`，单位 mW），
**并不公开适配器的额定功率或输出功率**——没有任何公开接口能给出「充电器正在输出多少 W」。

`Rate` 需按**有符号**解读（正=充电，负=放电），且 `0xFFFFFFFF` 是「固件未提供」的哨兵值
——按无符号解释会得到四百多万瓦的荒谬读数。

另外这类机器很常见的一种真实状态是「**适配器已接入，但电池不充不放**」（满电后续充 / 电池养护模式），
此时 `Rate` 为 0 或极小值（本机实测长期为 `-90 mW` 的微放电）。早期版本只在「确实在充电」时赋值
充电功率，导致卡片一直显示 `--`。现在的处理：

- 主数值取 **电池实时功率**：充电时用充入功率，未接入时用放电功率，**已接入但未充电时明确给 `0.0 W`**，不再留空
- 副行补充 **CPU 封装功耗**（RAPL，来自 PDH `\Energy Meter(*)\Power`），这是一直存在的实时功耗读数
- 说明文案按「充电中 / 已接入未充电 / 电池供电」分状态表述，并在括号里带上电池功率与 CPU 功耗

**CPU 封装功耗**：`\Energy Meter(*)\Power` 的实例名形如 `rapl_package0_pkg`（整包）、
`rapl_package0_pp0`（核心）、`rapl_package0_dram`，单位是 **mW**；`_total` 实例恒为 0，不能直接取。
部分老平台 / 虚拟机没有这个计数器，此时该读数显示 `--`，不影响其它指标。

**CPU 温度**：Windows 没有统一的 hwmon 接口，温度只存在于 CPU 片内的 DTS 寄存器里，
**必须经内核驱动读 MSR**。本工具按优先级多源尝试：

① **PawnIO + Intel MSR**（最准，推荐）：借通用 IO 驱动 [PawnIO](https://pawnio.eu) 执行官方签名的
`IntelMSR` 模块，直接读 MSR —— `0x1A2` 取 TjMax、`0x1B1` 取封装 DTS、`0x19C` 逐核心取 DTS，
`温度 = TjMax − DTS`。读 MSR 需要管理员权限，故这一步放在**提权子进程**里做（详见下节）；
② **硬件监控软件的 WMI `Sensor` 表**：只对 **OpenHardwareMonitor** 或**旧版 LHM（≤ 0.9.4）**有效 ——
LibreHardwareMonitor 自 **0.9.5 起已删除自带的 WMI 提供程序**，`root\LibreHardwareMonitor` 在新版上不存在；
③ **WMI `root\wmi:MSAcpi_ThermalZoneTemperature`**（ACPI 热区，厂商工具如联想 LegionZone 也拿它当通用兜底 ——
联想等机型的 BIOS 会暴露 CPU 热区，免装任何软件即可读到）；
④ **PDH `Thermal Zone Information(*)\Temperature`**（含有效性校验）。

本机（台式 / 虚拟机）实测既无 ACPI 热区实例、`Thermal Zone Information` 又恒定返回
`273`（对应 0°C，是无实现的假读数），因此代码会先换算成摄氏度、再用合理区间过滤掉这类值。
若以上通道都拿不到（例如非 Intel 平台且固件不暴露热区），界面显示 `--` 并给出具体原因 ——
`%LOCALAPPDATA%\WinMonitor\pawnio.log` 里会有提权桥接进程留下的失败明细。

> 逆向笔记：联想 LegionZone 取 CPU 温度的主路径是私有 WMI 提供程序 `root\wmi:LZWmiX80`
>（方法 `Query_CAPABILITY_DATA_00/01` 返回 `LENOVO_CAPABILITY_DATA_xx` 二进制 blob，内嵌
> `CPU DTS Sensor Temperature` 片上数字温度传感器读数），其通用兜底才是上面的 `MSAcpi_ThermalZoneTemperature`。
> `LZWmiX80` 的 blob 格式未公开，无法盲接；本工具走的是更通用的 MSR 直读路径。

### PawnIO + Intel MSR（温度源 ①）

PawnIO 是一个**通用**内核驱动：它本身不含硬件逻辑，只负责执行**已签名的模块字节码**，
因此不需要用户安装任何硬件监控软件。本工具把官方发布的 `IntelMSR.bin`
（来源 [namazso/PawnIO.Modules](https://github.com/namazso/PawnIO.Modules) v0.1.6，LGPL-2.1，
见 `src-tauri/resources/pawnio/NOTICE.md`）随安装包分发。

**为什么要拆成两个进程**：读 MSR 需要管理员权限，而主程序是普通权限运行的。所以：

1. 主程序启动时先探测 PawnIO 设备（`CreateFileW` 的失败码即可区分「没装」/「装了但没权限」）；
   **未安装就直接不拉**，交给界面引导（见下节）；已安装则检查
   `%LOCALAPPDATA%\WinMonitor\pawnio.json`，**没有新鲜读数**才拉起桥接进程，
   且**一个会话只尝试一次**，不会反复弹 UAC；
2. 桥接进程是**本 exe 自己**（命令行带 `--pawnio-bridge --module <path> --out <path> --parent-pid <pid>`），
   用 `ShellExecuteExW` 的 `runas` 动词 + 隐藏窗口启动 —— 首次运行会弹**一次 UAC**；
3. 它打开 `\\?\GLOBALROOT\Device\PawnIO`、载入模块、每秒读一次温度，把结果**原子写入**（临时文件 + 改名）
   那个 JSON；主进程只是读文件，因此完全不需要跨权限 IPC；
4. 它**启动时打开一次父进程句柄并一直持有**，之后每秒用 `WaitForSingleObject` 等待它——
   父进程一退出（进程对象被置为有信号态）就自行收尾 —— 不需要（也不该）在这里做跨权限
   `TerminateProcess`，那在普通权限下大概率会失败。

> ⚠️ 判据**必须**是句柄等待，**不能**是「`OpenProcess` 打不开就算父进程退出」：Windows 上
> 进程退出后只要有别的句柄还引用它的进程对象（WebView2 之类很常见），`OpenProcess(父PID)`
> 会继续成功，子进程就会永远以为父进程活着、应用关了它还赖在后台（2026-10-10 修的
> 「残留进程」正是此因）。详见 `msr_bridge.rs` 的 `ParentWatch` 与回归测试
> `parent_watch_notices_exit_even_when_object_survives`。

> 目前只内置了 Intel 模块，**AMD 平台暂不支持 MSR 直读**（会给出明确提示并回退 ACPI/PDH）。

#### PawnIO 没装怎么办：随包分发 + 一键安装引导

装 PawnIO 需要管理员权限（它是内核驱动），所以本工具**绝不静默安装**，而是分两种情况：

| 情况 | 行为 |
| --- | --- |
| 启动时探测到**未安装** | **不**拉起提权子进程（拉起来也只能报错、还白弹一次 UAC），改在「偏好设置」卡片里显示一条提示 + **「安装 PawnIO」按钮** |
| 用户点了按钮 | 拉起提权子进程并带 `--install-if-missing`（= 已获授权）；子进程**已经是管理员**，于是直接跑随包安装器 `-install -silent`，装完接着读温度 —— **全程只弹一次 UAC** |
| 已安装 | 正常拉起子进程读 MSR |

安装器**随安装包原样分发**（源码里就是官方 2.2.0 的 `PawnIO_setup.exe`，未做任何修改），
所以目标机器不需要联网、也不需要用户自己去下载。命令行与退出码：

```text
Usage: PawnIOSetup.exe [-install] [-uninstall] [-unrestricted] [-debuginfo] [-silent]
```

`-install -silent` 静默安装；退出码 `0` = 成功，`3010` = 成功但需重启，其他为 DOS 错误码。
用户**不装也能正常使用**本工具，只是温度会回退到 ACPI / PDH。

> **再分发条款**：安装器二进制是专有的「All rights reserved. This installer can be
> redistributed **unmodified**.」——所以它必须**原样**分发，不要改字节 / 换图标 / 重新签名
> （它带官方 Authenticode 签名，篡改会破坏签名）。先例：LibreHardwareMonitor 内嵌它、
> Chocolatey 官方 `pawnio` 包分发它、NWinfo 也分发它。详见
> `src-tauri/resources/pawnio/NOTICE.md`。
>
> 前置条件：**Windows 10 x64 1809（build 17763）及以上**；若系统已装更旧的 PawnIO，
> 安装器会拒绝并要求先卸载——本工具不会强行覆盖，只把原始退出码回传给界面。

**风扇转速**：这是个比温度更封闭的指标，**Windows 没有任何公开的用户态接口**。
本工具的策略是「**能读到就报、读不到就明确说读不到**」，绝不拿占空比冒充转速：

| 通道 | 覆盖情况 | 单位 |
| --- | --- | --- |
| **厂商私有（联想）** `root\WMI:LENOVO_FAN_METHOD.Fan_GetCurrentFanSpeed` | **联想机型有效**（ACPI-WMI 映射，**需管理员**，走提权桥接） | **RPM（可信）** |
| NVML `nvmlDeviceGetFanSpeed` / `…FanSpeedRPM` | 桌面 N 卡常有值；**笔记本独显普遍报 `N/A`**（风扇由 EC 控制，驱动读不到） | %（占空比）/ RPM |
| LHM / OHM 的 WMI `Sensor(SensorType='Fan')` | 需装 OpenHardwareMonitor 或旧版 LHM（≤ 0.9.4） | **RPM（可信）** |
| ACPI `Win32_Fan.DesiredSpeed` | 类存在但**绝大多数机器该字段为空**；有值也不保证是 RPM | 相对速度 |

前端展示口径：**优先真实 RPM，其次占空比 `%`，两者都没有才显示 `--`**，
并在「偏好设置」卡片下方给出具体原因（CPU 与 GPU 的成因不同，分开说明）。

**联想机型**：CPU 风扇可通过厂商私有的 **ACPI-WMI 通道**读取（联想官方 LegionZone 用的就是它），
但实测**需要管理员权限**。本工具把它接进与 CPU 温度同一个**提权子进程**（一次 UAC 同时搞定两者），
所以联想机型上温度与风扇是「一荣俱荣」的。检测到联想设备且风扇不可用时，
界面会在提示区显示「**启用**」按钮，点击并确认一次 UAC 即可。

> **刷新节奏**：风扇读数与 CPU 温度**同一个来源、同一个节拍**——桥接子进程 **1Hz** 写出，
> 主进程**每拍直接读**（本地小文件，微秒级），界面跟着采样间隔走。
> 主进程侧**没有任何时间窗缓存**：桥接每秒给一次，界面就每秒更新一次。
> （采样间隔选 500 ms 时，风扇受桥接 1Hz 的产出速率限制，会呈现"两次一更新"，
> 这是数据源的真实节拍，不是丢帧。）
> 提权后本机实测 CPU 风扇 ≈ 2800 RPM、GPU 风扇 ≈ 2785 RPM（相邻两拍通常有 3~10 RPM 抖动）。

> 本机（RTX 3060 Laptop + 12th Gen Intel）实测：`nvml.fan_speed(0)` 返回 `Err(InvalidArg)`、
> `Win32_Fan.DesiredSpeed` 为空、未装 OHM/LHM → 通用通道全读不到；
> 但**联想通道提权后可读到 CPU 风扇 ≈ 3007 RPM、GPU 风扇 ≈ 2978 RPM**。
> 用 `cargo test -p win-monitor diag::probe_fan -- --nocapture` 可以复现通用通道的判断过程。
>
> CPU 侧还有个更深的现状：CPU 风扇在非联想机型上挂在主板 Super I/O / EC 上，即便是本工具已有的
> PawnIO 通道也帮不上忙 —— 实测 `IntelMSR.bin` 只导出 `ioctl_read_msr` / `cpuid` / `msr_read` /
> `get_arch`，**不含任何 EC 读取函数**。要拿真实 RPM 只能另配 Super I/O 模块（机型强相关），暂不引入。
> 详见 `docs/LegionZone采集机制分析.md`。

**网络重复计数**：Windows 上同一张网卡会同时出现真实接口和 NDIS 过滤驱动两个条目
（本机 WLAN 与 WLAN-Huorong NDIS Filter 的累计流量完全相同），不去重会让汇总速率翻倍。
采样时按 MAC / 累计流量相同识别并剔除重复项。

## 目录结构

```
├─ index.html                 前端入口
├─ vite.config.js
├─ package.json
├─ src/                       前端（Vite）
│  ├─ main.js                 轮询 + 渲染
│  ├─ style.css
│  └─ modules/{format.js,charts.js}
├─ src-tauri/
│  ├─ tauri.conf.json
│  ├─ capabilities/default.json
│  ├─ resources/pawnio/       IntelMSR.bin（温度模块）+ PawnIO_setup.exe（官方安装器，原样分发）
│  │                          来源 / 许可 / 再分发条款见同目录 NOTICE.md
│  └─ src/
│     ├─ main.rs              二进制入口
│     ├─ lib.rs               Tauri 装配 + 托盘 + 采样线程 + 命令
│     ├─ sampler.rs           采样编排（CPU/内存/磁盘/网络/进程/GPU/温度）
│     ├─ gpu.rs               GPU：NVML 优先，PDH 兜底
│     ├─ fan.rs               风扇转速：厂商私有(联想) → NVML → LHM/OHM WMI → ACPI，读不到就诚实降级
│     ├─ vendor.rs            厂商私有通道的调度与结果挑选（读取桥接产出的 JSON）
│     ├─ thermal.rs           CPU/GPU 温度 + CPU 封装功耗：多源探测 + 有效性过滤
│     ├─ pdh.rs               PDH 性能计数器封装
│     ├─ wmi.rs               WMI(COM) 查询封装（ACPI 热区、显卡名、OHM/旧版 LHM 传感器、联想 ACPI-WMI 方法调用）
│     ├─ pawnio.rs            PawnIO 驱动直连：载入模块、读 MSR、解 DTS 温度
│     ├─ pawnio_setup.rs      PawnIO 探测（设备能否打开）+ 静默安装随包安装器
│     ├─ msr_bridge.rs        提权子进程：读 MSR + 联想风扇，写 pawnio.json（主进程只读该文件）
│     ├─ battery.rs           电池读数整理
│     ├─ win32.rs             Win32 薄封装（电源、内存提交量）
│     └─ model.rs             与前端交换的数据模型
├─ tools/make_icons.py        图标生成脚本（Pillow）
└─ tools/verify_bridge_exit.py 验收脚本：桥接进程是否跟着父进程一起收尾（见「调试与自测」）
```

## 运行

```bash
npm install
npm run dev        # 开发模式（Vite + Tauri）
npm run build      # 产出安装包 / exe（src-tauri/target/release）
npm run build:exe  # 只产出 exe，跳过 NSIS 打包
```

Release 产物：`src-tauri/target/release/win-monitor.exe`，
NSIS 安装包：`src-tauri/target/release/bundle/nsis/*.exe`。

> 本项目在 `x86_64-pc-windows-msvc` 下编译验证通过，需要 VS2022 MSVC 工具链与 WebView2 运行时。

### ⚠️ 不要直接 `cargo build --release`

`src-tauri/Cargo.toml` 里 `custom-protocol = ["tauri/custom-protocol"]` 是一个**非默认** feature，
它决定 Tauri 加载「内置前端资源」还是「devUrl 上的 Vite 开发服务器」：

```rust
// tauri/build.rs
let dev = !custom_protocol;        // 没启用 custom-protocol → 判定为 dev
println!("cargo:dev={dev}");
```

`dev=true` 时窗口会去请求 `http://localhost:1420`，双击 exe 就会看到
`localhost 拒绝连接（ERR_CONNECTION_REFUSED）`——**不是页面坏了，是构建姿势不对**。
只有 `tauri build`（CLI）会自动追加该 feature，所以务必走 npm 脚本：

```bash
npm run build        # 完整打包：前端 + exe + NSIS 安装包
npm run build:exe    # 只出可运行的 exe，跳过安装包（无需 NSIS）
```

`npm run dev` 期间请保持 Vite 进程存活，否则窗口刷新同样会报连接被拒绝。

## 调试与自测

`lib.rs` 里留了一组分段诊断用例，加指标或换机器排查时很有用：

```bash
# 完整快照（会打出一份完整 JSON）
cargo test -p win-monitor smoke::collect_once -- --nocapture

# 逐层确认原生数据源
cargo test -p win-monitor diag::nvml          -- --nocapture   # NVIDIA 驱动读数
cargo test -p win-monitor diag::probe_gpu     -- --nocapture   # GPU 探测（NVML → PDH）
cargo test -p win-monitor diag::probe_thermal -- --nocapture   # 温度 + CPU 封装功耗
cargo test -p win-monitor diag::probe_fan     -- --nocapture   # 风扇转速（NVML / LHM/OHM / ACPI 逐层确认）
cargo test -p win-monitor diag::probe_battery -- --nocapture   # 电池原始字段（SYSTEM_BATTERY_STATE / WMI）
cargo test -p win-monitor diag::wmi           -- --nocapture   # WMI 连通性
cargo test -p win-monitor diag::pawnio_msr    -- --nocapture   # PawnIO 设备 / 模块 / MSR 温度（提权运行才有意义）
cargo test -p win-monitor diag::pawnio_setup  -- --nocapture   # PawnIO 是否已安装 + 随包安装器是否就位
```

WMI 逐步 HRESULT 日志：`WB_WMI_DIAG=1 cargo test -p win-monitor diag::wmi -- --nocapture`

### 桥接进程会不会留成孤儿

`msr_bridge.rs` 的存活判据有两条用例锁着（`cargo test -p win-monitor msr_bridge::tests`）。
其中 `parent_watch_notices_exit_even_when_object_survives` 复刻了「进程已退出但进程对象还没被
回收」的场景 —— 旧判据（`OpenProcess` 打不开才算退出）在这个场景下恒为「还活着」，
就是 2026-10-10 那个「应用关了还有残留进程」的根因，**别把它改回去**。

要看真实二进制（而不是测试里造的场面）的表现，跑验收脚本 —— 不弹 UAC、不开窗口：

```bash
npm run build             # 先出包，脚本读 src-tauri/target/release/win-monitor.exe
python tools/verify_bridge_exit.py
```

它会拉起一个自己控制的假父进程 + 一个 `--pawnio-bridge` 实例，确认「父进程活着→桥接保持
存活」「父进程结束→桥接 1 拍内自行退出」，并顺带打印旧判据在同一时刻的错误结论作对照。

## 可调项

- 采样间隔：界面右上角下拉框（500 ms / 1 s / 2 s / 5 s），同步调整后端采样线程间隔
- 暂停/继续：停止前后端轮询，保留最后一帧数据（托盘菜单里也能切）
- 开机自启：面板「偏好设置」开关，或托盘菜单直接勾选
- `src-tauri/src/sampler.rs` 中的 `TOP_N` 控制进程榜长度

## 可扩展方向

- 采样数据落盘 CSV，用于长时间基线分析
- 进程级 GPU 归属（`GPU Engine` 计数器里本来就带 pid，可以做成 GPU 版进程榜）
- 告警阈值：CPU 温度 / GPU 温度超阈值时托盘气泡提醒
