# 第三方组件声明：PawnIO

本目录随安装包分发 **PawnIO** 的两部分内容，用于通过内核驱动读 Intel CPU 的 MSR
（`IA32_THERM_STATUS` / `IA32_PACKAGE_THERM_STATUS` / `IA32_TEMPERATURE_TARGET`），
从而取得 CPU 核心 / 封装温度——这是 Windows 上唯一不依赖第三方常驻软件的官方级读数途径
（CPU 温度没有公开用户态接口，必须经由内核驱动读 MSR）。

## 1. 模块：`IntelMSR.bin`

| 项 | 值 |
| --- | --- |
| 文件 | `IntelMSR.bin`（4 KB） |
| 来源 | <https://github.com/namazso/PawnIO.Modules> 发布版 **v0.1.6** |
| 许可 | GNU Lesser General Public License v2.1（全文见同目录 `COPYING`） |
| 签名 | 官方发布渠道的**已签名**模块，PawnIO 驱动会校验签名；**请勿修改文件内容** |

PawnIO 驱动本身采用 GPL-2.0，并附有一条特殊例外：**仅通过 DeviceIoControl 接口与驱动通信**
的独立模块不受 GPL 传染。`IntelMSR.bin` 正是这类模块，故可随本工具分发（全文见
`COPYING-GPL-2.0.txt`）。

## 2. 安装器：`PawnIO_setup.exe`

| 项 | 值 |
| --- | --- |
| 文件 | `PawnIO_setup.exe`（3.25 MiB） |
| 来源 | <https://github.com/namazso/PawnIO.Setup> 发布版 **2.2.0** |
| SHA-256 | `1f519a22e47187f70a1379a48ca604981c4fcf694f4e65b734aaa74a9fba3032` |
| 许可 | 专有：「All rights reserved. This installer can be redistributed **unmodified**.」 |

**再分发条款（重要）**：该安装器**未做任何修改**地原样随包分发，这正是其许可所要求的唯一形式。
请**不要**重打包、改字节、替换图标或重新签名——它带有官方 Authenticode 签名，
篡改会破坏签名，PawnIO 驱动也会拒绝加载未签名模块（见 `PawnIO_setup.exe` 内嵌文本
`All rights reserved.` / `This installer can be redistributed unmodified.`）。

该安装器已存在的先例（均为原样分发）：LibreHardwareMonitor（内嵌为 embedded resource）、
Chocolatey 官方 `pawnio` 包、NWinfo。

### 命令行（从安装器二进制中确认）

```text
Usage: PawnIOSetup.exe [-install] [-uninstall] [-unrestricted] [-debuginfo] [-silent]
  -install       Install PawnIO
  -uninstall     Uninstall PawnIO
  -unrestricted  Install unrestricted edition
  -debuginfo     Install debug info
  -silent        Run in silent mode (no UI, even on error)
```

本工具使用 `-install -silent`，并按下述退出码判定结果：

| 退出码 | 含义 |
| --- | --- |
| `0` | 成功 |
| `3010` | 成功，但需要重启系统才完全生效 |
| 其他 | 失败（DOS 错误码；若已装旧版会提示先卸载） |

### 使用前提

- 安装需要**管理员权限**（会弹 UAC）；驱动仅对管理员开放设备，因此读 MSR 也需提权。
- 仅支持 **Windows 10 x64 1809（build 17763）及以上**。
- 若系统已安装**更旧**的 PawnIO，安装器会拒绝安装并要求先卸载——本工具据此不会自动覆盖，
  只把安装器的原始退出码与提示回传给界面。
- 用户不安装也可正常使用本工具，只是 CPU 温度会回退到 ACPI 热区 / PDH，精度明显较差。

## 许可文件清单

| 文件 | 内容 |
| --- | --- |
| `COPYING` | LGPL-2.1 全文（`IntelMSR.bin`，来自 PawnIO.Modules） |
| `COPYING-GPL-2.0.txt` | GPL-2.0 全文（PawnIO 驱动本体，来自 namazso/PawnIO） |
| `README.txt` | PawnIO.Modules 官方 README |
