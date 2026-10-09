# 项目长期记忆：Win 系统监控

## 项目定位

Windows 桌面系统监控工具（Tauri 2 + Rust + Vite），监控 CPU / 内存 / 硬盘 / 电池 / 适配充电功率。

## 约定

- 依赖版本锁定：sysinfo 0.39.6、windows-sys 0.59、Tauri 2.12.x、Vite 7
- 前端数据与 Rust 之间统一 camelCase（model.rs 用 `#[serde(rename_all = "camelCase")]`）
- 所有平台原生读数集中在 `src-tauri/src/win32.rs`，不要在业务层散写 unsafe
- 新增指标时优先查 sysinfo，sysinfo 没有的才加 Win32，并在 README 的「数据来源」表登记
- 图标用 `tools/make_icons.py` 重新生成（依赖系统 Python 3.11 的 Pillow）

## 温度数据源（2026-10-09 实测结论，勿再走弯路）

- **LibreHardwareMonitor 的 WMI 提供程序在 v0.9.5 已被上游删除**：`v0.9.0/0.9.2/0.9.4` 有
  `LibreHardwareMonitor/WMI/{WmiProvider,Sensor,Hardware,IWmiObject}.cs`，`v0.9.5` 起整个目录消失。
  所以 `root\LibreHardwareMonitor` 命名空间在 0.9.6 上**永远不会存在**，`wmi.rs` 里的
  `THERMAL_NAMESPACES[0]` 与 `lhm.rs` 注入的 `UseWMI=true`（那是内存 SMBus 的键，不是 WMI 提供程序开关）
  都是无效配置。**不要再尝试用 WMI 从 LHM 取温度。**
- 0.9.6 对外只有 **`HttpListener`**（`runWebServer`/`listenerIp`/`listenerPort` 默认 8085/`authenticationEnabled`），
  和内置 **CSV 传感器日志**（`Logger.cs`：`logSensorsMenuItem=true` 开启，文件写到 LHM.exe 同目录
  `LibreHardwareMonitorLog-YYYY-MM-DD-N.csv`，`FileMode.Append + FileShare.ReadWrite` 可边写边读）。
- **CSV 方案不可取**：本机实测 LHM 枚举 **545 个传感器（90 个温度）**，1Hz 全量写盘约 3 KB/行 ≈ 200+ MB/天。
- **0.9.6 的驱动是外部的 PawnIO**（不自带）：`MainForm` 里检测 `PawnIo.PawnIo.IsInstalled`，
  缺失就弹框并释放内嵌的 `PawnIO_setup.exe -install`。本机已装 `C:\Program Files\PawnIO\`（2026-10-09 20:18）。
- `hw` crate（OHM 外壳）、`systemstat` crate（Windows `cpu_temp()` 直接返回 `Not supported`，只支持 Linux/BSD/macOS）
  **都拿不到 Windows CPU 温度**，别再加这两个依赖。
- **直连 PawnIO 的协议（LHM `PawnIo/PawnIo.cs` 已验证可用）**，是绕开 WMI/HTTP 的可行路径：
  - 设备：`\\?\GLOBALROOT\Device\PawnIO`，`CreateFileW(RW, FILE_SHARE_READ|WRITE, OPEN_EXISTING)`
  - `DEVICE_TYPE = 41394 << 16`；`IOCTL_PIO_LOAD_BINARY = 0x821 << 2`；`IOCTL_PIO_EXECUTE_FN = 0x841 << 2`
  - 载入模块：DeviceIoControl(LoadBinary, in = `.bin` 模块字节)
  - 执行函数：in = `[函数名 ASCII 32 字节补零][i64 参数...]`，out = `i64` 数组
  - `ioctl_read_msr`（参数 1 个：MSR 索引）→ 返回 `eax | (edx << 32)`
  - 模块 `.bin` 在 LHM 仓库 `LibreHardwareMonitorLib/Resources/PawnIo/*.bin`（MPL-2.0），含 `IntelMSR.bin`
  - Intel 温度：`0x1A2`(IA32_TEMPERATURE_TARGET) 取 TjMax，`0x1B1`(IA32_PACKAGE_THERM_STATUS) bits16-22 = DTS，
    `temp = TjMax - DTS`；`0x1B1` 不支持时逐核读 `0x19C`（**必须先 `SetThreadAffinityMask` 绑核**，MSR 是每核心的）。
  - 需要管理员（驱动只对管理员开放），**因此已实现为「提权子进程」架构**（勿改成在主进程直读）：
    `msr_bridge.rs` 用 `ShellExecuteExW`+`runas`+隐藏窗口拉起**本 exe 自己**
    （`--pawnio-bridge --module … --out … --parent-pid N`，`main.rs` 里拦截该参数、不建窗口），
    子进程每秒写 `%LOCALAPPDATA%\WinMonitor\pawnio.json`（写临时文件 + rename 原子覆盖），
    并用 `OpenProcess` 检测父进程存活后自退 → 不需要跨权限 `TerminateProcess`。
    一个会话只尝试提权一次（避免 UAC 轰炸）；状态里带 `parentPid` 以识别上一轮残留。
  - 温度源优先级（`thermal.rs`）：**Intel MSR → OHM/旧版 LHM 的 WMI → ACPI 热区(WMI) → PDH**。
  - 只内置了 Intel 模块，**AMD 平台不支持 MSR 直读**，会给出明确提示并回退 ACPI/PDH。
  - 模块资源：`src-tauri/resources/pawnio/IntelMSR.bin`（来自 namazso/PawnIO.Modules v0.1.6，LGPL-2.1，
    已签名——签名是 PawnIO 接受模块的前提），由 `tauri.conf.json` 的
    `bundle.resources: {"resources/pawnio/": "pawnio/"}` 打进安装包。

### PawnIO 的探测与「一键安装引导」（2026-10-09 新增，勿改成静默安装）

- **探测（`pawnio_setup.rs::probe()`）不需要读注册表**：直接 `CreateFileW` 那个设备路径即可，
  失败码有区分度 —— `ERROR_ACCESS_DENIED`(5) = 已安装但没权限；`ERROR_FILE_NOT_FOUND`(2) / `PATH_NOT_FOUND`(3)
  = **未安装**。本机实测确认。
- **安装器随包原样分发**：`resources/pawnio/PawnIO_setup.exe`（官方 2.2.0，3,410,960 B，
  sha256 `1f519a22e47187f70a1379a48ca604981c4fcf694f4e65b734aaa74a9fba3032`）。
  其许可是专有的「All rights reserved. **This installer can be redistributed unmodified.**」
  → **必须原样分发**，不要改字节/换图标/重签名（破坏 Authenticode 签名）。先例：LHM 内嵌它、
  Chocolatey `pawnio` 包、NWinfo 都分发它。
- **安装器 CLI（从二进制 UTF-16 串里挖出）**：
  `Usage: PawnIOSetup.exe [-install] [-uninstall] [-unrestricted] [-debuginfo] [-silent]`。
  用 `-install -silent`；退出码 `0` 成功、`3010` 成功但需重启、其他为 DOS 错误码
  （已装旧版会拒绝并提示先卸载，所以**装之前必须先 probe**）。仅支持 Win10 x64 1809+ (build 17763)。
- **交互设计（重要）**：启动时若 probe 为 `Missing` → **不拉起**提权子进程（避免白弹 UAC），
  改由界面在 `#thermal-hint` 显示「安装 PawnIO」按钮；用户点击后带 `--install-if-missing` 拉起桥接，
  **此刻子进程已是管理员，直接跑安装器（`elevate=false`）** → 装+读**只弹一次 UAC**。
  `--install-if-missing` 就是「用户已授权」的凭据，**不要在启动路径上带它**（否则等于静默装内核驱动）。
- 相关 Tauri 命令：`pawnio_status`（前端每 10 拍轮询一次）、`install_pawnio`（立即返回，前端自行轮询状态）。
- 无头预览：`?mock=1&pawnio=missing` 可模拟「未装 PawnIO」的引导态。

## Windows 电源读数注意事项

- `SYSTEM_BATTERY_STATE::Rate` 按有符号解读：正=充电 mW，负=放电 mW，0/±1=固件未提供
- `0xFFFFFFFF` 是「字段无效」哨兵值，需要对 MaxCapacity / RemainingCapacity / EstimatedTime 过滤
- Windows 不公开充电器额定功率，只公开「充入电池的功率」，UI 文案已按此表述
- **电源卡片口径（第六阶段定稿，勿随手改）**：
  - 主数值 = `chargeRateW ?? dischargeRateW ?? (acOnline ? 0 : null)`。
    已接入但电池不充不放（满电常态）明确给 `0.0`，**不能留 `--`**——这正是「不论是否插适配器都不显示」的修复点。
  - 副行三列：CPU 功耗 / 电池状态（充电中·微放电·放电中·未充电·待机）/ 电源（已接入适配器·电池供电）。
  - 「CPU 功耗」= PDH `\Energy Meter(*)\Power`（RAPL 封装功耗，单位 mW），`_total` 恒 0 已避开，
    优先 `*_pkg` 实例；属「系统功耗参考」，非适配器输出功率。

## UI 验证方法（真机锁屏时截不到窗口）

用本地静态服务 + Chrome 无头渲染前端，`?mock=1` 会加载 `public/mock-preview.js` 注入假数据：

```bash
python -m http.server 8099 --directory dist
chrome --headless=new --window-size=1240,820 --screenshot=out.png "http://localhost:8099/?mock=1"
```

定位布局问题时，把探针数据（`clientHeight`/`scrollHeight`）渲染进页面再截图，比猜 CSS 可靠。

## 布局 / 滚动（重要，改动前必读）

窗口是固定外壳，**唯一的滚动发生在卡片区 `.grid` 内部**：

- `#app`：`height:100vh; overflow:hidden`（固定外壳，自身不滚动，也不裁内容）
- `.grid`：`flex:1 1 auto; min-height:0; overflow-y:auto; grid-auto-rows:max-content; align-content:start`
- **`grid-auto-rows:max-content` 不可删**：否则 `.card{min-height:0}` 让隐式行的 min-content 贡献变成 0，
  叠加默认 `align-content:stretch`，隐式行会被均摊拉伸到刚好填满容器高度 → 卡片被压扁、
  内容溢出叠层、滚动条消失（这正是「窗口缺纵向滚动条」的根因）。
- 通用坑：flex/grid 滚动容器里，① 子项被默认 `stretch` 均摊；② `overflow` 元素不贡献内在高度。

## 构建模式 dev vs prod（踩过坑，务必遵守）

- **出包只能走 npm 脚本**：`npm run build`（前端+exe+NSIS 安装包）或 `npm run build:exe`（`tauri build --no-bundle`，只出 exe）。
- **禁止用 `cargo build --release` 直接产包**：`src-tauri/Cargo.toml` 的 `custom-protocol` 是非默认 feature，
  只有 `tauri build` 会自动加。缺它 → `tauri/build.rs` 里 `let dev = !custom_protocol` 判为 dev →
  窗口去请求 `devUrl`（`http://localhost:1420`）→ 双击 exe 显示「localhost 拒绝连接 / ERR_CONNECTION_REFUSED」。
  **这不是前端坏了，是构建姿势错了**（2026-10-09 实际踩到）。
- 判定依据（权威、无侵入，不用真机点开）：看 `src-tauri/target/release/build/win-monitor-*/output`，
  有 `cargo:rustc-cfg=dev` 即 dev 版；对照 `tauri-*/output` 里的 `cargo:dev=true|false`。
- `tauri build` 会自动下载 NSIS 到 `%LOCALAPPDATA%\tauri`，**无需手动装 makensis**。
- `npm run dev` 期间必须保持 Vite 进程存活，否则窗口刷新同样报连接被拒绝。

## 操作约定

- **同一文件的多个 Edit 必须串行下发**：并行下发会互相覆盖丢失，曾导致 `#app` 的改动静默失效、构建产物与预期不符。同文件改造优先用一次脚本原子替换。
- 验证 GUI 改动优先用「构建产物 cfg 检查 + 无头 Chrome 渲染」这类无侵入手段，不去点开真机窗口（见下文 UI 验证方法）。

## LibreHardwareMonitor：已从项目中**彻底移除**（2026-10-09）

- 原因：0.9.5 起上游删除 WMI 提供程序 → 在 0.9.6 上没有任何「无端口」通道可用；
  其余通道（回环 HTTP 8085 / 每秒写 545 列 CSV ≈ 200+MB/天）都被否决或不可取。
- 现状：LHM 的 `resources/lhm`（11MB）、`lhm.rs`、启动/清理钩子、`tauri.conf.json` 资源项全部删除，
  安装包由 6.41 MiB 降到 **1.21 MiB**。历史实现细节见 `2026-10-09.md`，**不要恢复**。
- `wmi.rs` 里的 `hardware_temperatures()` 保留，但只对 **OpenHardwareMonitor / LHM ≤ 0.9.4** 有效。

## 出包时的常见阻塞

- **`target/release/win-monitor.exe` 被占用 → 构建报 `failed to remove file … 拒绝访问 (os error 5)`**：
  说明程序正在运行（或提权桥接子进程还活着）。先 `Stop-Process` 掉 `win-monitor` 主进程
  （桥接子进程会在数秒内自行退出），再重新构建。
