# 第三方组件声明：PawnIO

本目录随安装包分发 **PawnIO** 的两部分内容：**驱动安装器**与两个**官方签名的模块**。
模块用于经内核驱动直读 CPU 温度——Windows 没有公开的用户态 CPU 温度接口，Intel 的温度
只存在于片内 DTS 寄存器（MSR），AMD 的只在 SMN 空间里，都必须经内核驱动读。

| 模块 | 适用平台 | 读取的寄存器 | 用到的函数 |
| --- | --- | --- | --- |
| `IntelMSR.bin` | Intel | `IA32_THERM_STATUS`(0x19C)、`IA32_PACKAGE_THERM_STATUS`(0x1B1)、`IA32_TEMPERATURE_TARGET`(0x1A2) | `ioctl_read_msr` |
| `AMDFamily17.bin` | AMD **Zen 架构（family 17h~1Ah）** | SMN `THM_TCON_CUR_TMP`(0x00059800)；封装功耗另读 `MSR_PWR_UNIT`(0xC0010299)、`MSR_PKG_ENERGY_STAT`(0xC001029B) | `ioctl_read_smn`、`ioctl_read_msr` |

## 1. 模块：`IntelMSR.bin` / `AMDFamily17.bin`

| 项 | 值 |
| --- | --- |
| 来源 | <https://github.com/namazso/PawnIO.Modules> 发布版 **v0.2.3**（这两个文件在 v0.2.2 与 v0.2.3 中字节完全相同） |
| 许可 | GNU Lesser General Public License v2.1（全文见同目录 `COPYING`；正文与上游发布包逐字相同，仅换行符按本仓库 `.gitattributes` 统一为 LF） |
| 签名 | 官方发布渠道的**已签名**模块，PawnIO 驱动会校验签名；**请勿修改文件内容**，改一个字节即破坏签名、驱动将拒绝载入 |

随包的两个文件均取自上述发布包、未做任何修改（`*.bin` 在本仓库 `.gitattributes` 中
声明为 `binary`，不会被换行符规范化触碰）：

| 文件 | 大小 | SHA-256 |
| --- | --- | --- |
| `IntelMSR.bin` | 4068 B | `ed41a0d0de082f4668a9caccb9681ebbad97a8ed3452c41918dda1eb454b77a9` |
| `AMDFamily17.bin` | 9300 B | `099dc01d6db97ea997fec4a461e191cc64b9d7ce47c9d2153c451c56c2adcf50` |

PawnIO 驱动本身采用 GPL-2.0，并附有一条特殊例外：**仅通过 DeviceIoControl 接口与驱动通信**
的独立模块不受 GPL 传染。上面两个模块正是这类模块，故可随本工具分发（全文见
`COPYING-GPL-2.0.txt`）。

> AMD 模块自己会在载入时按 CPUID 校验：非 AMD、或家族不在 17h~1Ah（Zen 之前的老平台）
> 会以 `STATUS_NOT_SUPPORTED` 拒绝载入。本工具据此显示明确原因，不会静默失败。

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

- 安装需要**管理员权限**（会弹 UAC）；驱动仅对管理员开放设备，因此读 MSR / SMN 也需提权。
- 仅支持 **Windows 10 x64 1809（build 17763）及以上**。
- 若系统已安装**更旧**的 PawnIO，安装器会拒绝安装并要求先卸载——本工具据此不会自动覆盖，
  只把安装器的原始退出码与提示回传给界面。
- 用户不安装也可正常使用本工具，只是 CPU 温度会回退到 ACPI 热区 / PDH，精度明显较差。

## 许可文件清单

| 文件 | 内容 |
| --- | --- |
| `COPYING` | LGPL-2.1 全文（`IntelMSR.bin` / `AMDFamily17.bin`，来自 PawnIO.Modules） |
| `COPYING-GPL-2.0.txt` | GPL-2.0 全文（PawnIO 驱动本体，来自 namazso/PawnIO） |
| `README.txt` | PawnIO.Modules 官方 README |
