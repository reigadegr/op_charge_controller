# 电池与充电数据读取逻辑

## 结论

页面显示的电流和两路电芯电压都来自同一个 sysfs 节点：

```text
/sys/class/oplus_chg/battery/bcc_parms
```

程序读取该节点后，以逗号分隔内容，并从结果中取以下字段：

| 页面数据 | JavaScript 数组下标（从 0 开始） | 字段序号（从 1 开始） | 页面单位 |
| --- | ---: | ---: | --- |
| 第一电芯电压 | `6` | 第 7 项 | `mV` |
| 电流 | `8` | 第 9 项 | `mA` |
| 第二电芯电压 | `11` | 第 12 项 | `mV` |

因此，三项数据的对应关系是：

```text
bcc_parms 第 7 项  -> 第一电芯电压
bcc_parms 第 9 项  -> 电流
bcc_parms 第 12 项 -> 第二电芯电压
```

## 获取方式

编译后的代码通过 KernelSU WebUI 的命令执行接口运行：

```sh
cat /sys/class/oplus_chg/battery/bcc_parms
```

读取函数取命令的标准输出并调用 `trim()`；数据读取函数再调用 `split(",")`，最后用 `parseFloat()` 解析目标字段。还原后的关键逻辑如下：

```js
const fields = (
  await readFile("/sys/class/oplus_chg/battery/bcc_parms")
).split(",");

const cellVoltage1 = parseFloat(fields[6]);
const current = parseFloat(fields[8]);
const cellVoltage2 = parseFloat(fields[11]);
```

其中 `readFile()` 的实际行为等价于：

```js
const result = await exec(`cat ${path}`);
return result.stdout.trim();
```

若命令返回码不为 `0`，读取函数会抛出异常，本轮页面更新失败。

## 页面如何使用这些值

- 电芯电压直接显示为 `第一电芯电压 mv / 第二电芯电压 mv`，没有额外缩放。
- 电压差使用 `Math.abs(第一电芯电压 - 第二电芯电压).toFixed(0)`，即取绝对差并显示为整数 `mV`。
- 电流直接显示为 `电流 ma`，没有额外缩放，也没有在显示前取绝对值，所以节点字段的正负号会被保留。
- 页面加载时会立即读取一次，之后每 `2000 ms` 更新一次这组数据。

## 电池功率计算

页面上的“电池功率”不是从独立的功率节点直接读取，而是使用 `bcc_parms` 中的两路电芯电压和电池电流在前端计算：

```text
电池功率(W) = (第一电芯电压(mV) + 第二电芯电压(mV))
              * |电池电流(mA)|
              / 1000000
```

还原后的 JavaScript 逻辑为：

```js
const current = parseFloat(fields[8]);
const cellVoltage1 = parseFloat(fields[6]);
const cellVoltage2 = parseFloat(fields[11]);

const batteryPower = (
  ((cellVoltage1 + cellVoltage2) * Math.abs(current)) /
  1000000
).toFixed(2);
```

这里将两路电芯电压相加作为电池包总电压。由于 `mV * mA` 的结果是微瓦，除以 `1000000` 后得到瓦。`Math.abs(current)` 会去掉电流方向，因此无论节点使用正值还是负值表示充电，页面上的功率始终显示为正数。`toFixed(2)` 使结果保留两位小数。

例如两路电芯电压分别为 `4400 mV` 和 `4390 mV`，电流为 `-5000 mA`：

```text
电池功率 = (4400 + 4390) * |-5000| / 1000000
         = 43.95 W
```

该结果会写入页面的 `batteryPower` 元素，并加入功率曲线；页面加载时立即计算一次，之后每 `2000 ms` 重新读取并计算。

“电池功率”与充电信息区域中的“最大充电功率”不是同一个数据。后者来自 `/sys/class/oplus_chg/common/cpa_power`，按节点值除以 `1000` 后显示，不参与上述实时电池功率计算。

## 产物证据

以上逻辑来自编译产物：

```text
target/webroot/Oplus_battmon_webroot.857338fb.js
```

该文件中的压缩代码包含以下关键片段：

```js
let s=(await X("/sys/class/oplus_chg/battery/bcc_parms")).split(","),
    a=parseFloat(s[8]),
    n=parseFloat(s[6]),
    l=parseFloat(s[11]);
```

随后返回：

```js
{ battcur: a, battvbat1: n, battvbat2: l }
```

页面更新代码分别将 `battvbat1`、`battvbat2` 写入 `cellVoltage`，将 `battcur` 写入 `current`。

## 容易混淆的另一个电压节点

产物还会读取：

```text
/sys/class/oplus_chg/battery/vbat_uv
```

但它被用于页面上的“关机电压”信息，不是“电芯电压”栏显示的两路电压来源。

## 总线电压、充电器类型和充电功率

### 数据来源汇总

| 页面数据 | 节点 | 取值方式 | 页面显示 |
| --- | --- | --- | --- |
| 总线电压 | `/sys/class/oplus_chg/battery/battery_log_content` | 逗号分隔后的 `fields[12]`，即第 13 项 | 原值后加 `mv` |
| 充电器类型 | `/sys/class/oplus_chg/battery/battery_log_content` | 逗号分隔后的 `fields[9]`，即第 10 项，再查类型表 | 类型名称 |
| 最大充电功率 | `/sys/class/oplus_chg/common/cpa_power` | 节点值除以 `1000`，再取整 | 结果后加 `W` |
| 功率范围前值 | `/sys/class/oplus_chg/battery/bcc_parms` | 逗号分隔后的 `fields[9]`，即第 10 项 | 原值后加 `W` |
| 功率范围后值 | `/sys/class/oplus_chg/battery/bcc_parms` | 逗号分隔后的 `fields[10]`，即第 11 项 | 原值后加 `W` |

### USB 在线状态

代码先读取：

```text
/sys/class/power_supply/usb/online
```

只有该节点去除首尾空白后的值严格等于 `1` 时，才会：

- 读取 `battery_log_content` 中的总线电压和充电器类型；
- 展开页面上的充电信息区域；
- 显示最大充电功率等充电数据。

USB 不在线时，不读取 `battery_log_content`，总线电压和充电器类型在本轮数据中保持为“等待数据刷新”，同时页面会收起整个充电信息区域。

### 总线电压

USB 在线时，程序执行：

```sh
cat /sys/class/oplus_chg/battery/battery_log_content
```

然后以逗号分隔，使用 `parseFloat(fields[12])` 取得第 13 项：

```js
const logFields = (
  await readFile("/sys/class/oplus_chg/battery/battery_log_content")
).split(",");

const wiredVbusMv = parseFloat(logFields[12]);
```

页面直接显示为 `${wiredVbusMv}mv`，没有额外缩放。产物将该字段命名为 `wired_vbus_mv`，因此可确认其预期单位为毫伏。

### 充电器类型

充电器类型和总线电压来自同一次 `battery_log_content` 读取。代码使用 `parseFloat(fields[9])` 取得第 10 项类型编号，再按以下表格转换：

| 编号 | 显示名称 | 编号 | 显示名称 |
| ---: | --- | ---: | --- |
| `1` | `SDP` | `9` | `PD_SDP` |
| `2` | `DCP` | `10` | `APPLE_BKID` |
| `3` | `CDP` | `11` | `QC2` |
| `4` | `ACA` | `12` | `QC3` |
| `5` | `C` | `13` | `VOOC` |
| `6` | `PD` | `14` | `SVOOC` |
| `7` | `PD_DRP` | `15` | `UFCS` |
| `8` | `PD_PPS` | 其他值 | `未知类型` |

还原后的逻辑为：

```js
const chargeType = parseFloat(logFields[9]);
const chargeTypeName = chargeTypeNames[chargeType] || "未知类型";
```

### 最大充电功率

最大充电功率直接读取独立节点：

```sh
cat /sys/class/oplus_chg/common/cpa_power
```

读取结果最初以字符串保存在 `cpaPw` 中。更新页面时，JavaScript 利用数值运算进行隐式转换，除以 `1000` 后调用 `toFixed(0)`：

```js
const maxChargePower = (cpaPw / 1000).toFixed(0);
```

最终显示为 `${maxChargePower}W`。这意味着页面会把节点值按 `1000` 缩放，并四舍五入为整数瓦。

该节点在每轮数据采集时都会读取，但“最大充电功率”所在区域只有 USB 在线时才展开显示。

### 功率范围

功率范围与电流、电芯电压一样，来自：

```sh
cat /sys/class/oplus_chg/battery/bcc_parms
```

代码从逗号分隔结果中读取：

```js
const chargeHigh = parseFloat(fields[9]);
const chargeLow = parseFloat(fields[10]);
```

产物内部将两项分别命名为 `chghigh` 和 `chglow`，页面按以下顺序原样显示：

```js
`${chargeHigh}W~${chargeLow}W`
```

这里没有除法、取整或大小排序。也就是说，显示顺序固定为 `bcc_parms` 第 10 项在前、第 11 项在后，不保证页面字符串一定按数值从小到大排列。

功率范围仅在充电器类型编号为以下任一值时显示：

- `14`：`SVOOC`
- `15`：`UFCS`

其他充电器类型即使 USB 在线，该行也会被隐藏。

### 刷新周期

上述数据与电流、电芯电压位于同一个更新函数中：页面加载时立即读取一次，此后每 `2000 ms` 重新读取和更新一次。

## 未充电时电芯压差的物理含义

### 先明确“未充电”不等于“静置”

页面用 `/sys/class/power_supply/usb/online` 判断是否正在接入 USB。它为 `0` 时只能说明充电输入不在线，手机本身通常仍在耗电，因此电池仍有放电电流。

产物按下面的公式计算电池功率：

```text
P = (V1 + V2) * |I|
```

这说明程序把两路电芯电压相加作为电池包电压，即按两颗串联电芯处理。在串联结构中，流过两颗电芯的电流相同，但它们的端电压可以不同。

### 等效电路解释

对正在放电的单颗电芯，可以用简化模型表示其端电压：

```text
V端 = VOC(SOC, 温度, 历史状态) - I * R内阻 - V极化
```

其中：

- `VOC` 是该电芯在当前荷电状态（SOC）下的开路电压；
- `I * R内阻` 是放电电流造成的瞬时压降；
- `V极化` 是电化学反应和离子扩散造成的动态压降，负载消失后会逐渐恢复；
- 实际读数还包含采样误差、线路压降和温度影响。

因此页面显示的压差：

```text
ΔV = |V1 - V2|
```

在放电时并不是单一物理量，而是以下因素共同作用的结果：

```text
两颗电芯的开路电压差
+ 两颗电芯的内阻压降差
+ 两颗电芯的极化差
+ 温度、采样和连接误差
```

### 它通常反映什么

| 观察到的现象 | 更可能的物理原因 |
| --- | --- |
| 小电流并充分静置后仍有稳定压差 | 两颗电芯的 SOC 不一致，也可能包含电压采样偏差或自放电差异 |
| 负载一增大，压差立即明显增大；负载降低后很快回落 | 两颗电芯的内阻不同，内阻较大的电芯在相同电流下下陷更多 |
| 放电过程中压差逐渐扩大，尤其接近低电量时更明显 | 容量或 SOC 不匹配；容量较小的电芯 SOC 下降得更快，也可能是弱电芯接近放电曲线陡峭区 |
| 拔掉充电器后压差缓慢变化并逐渐收敛 | 两颗电芯的电化学极化在松弛，属于动态恢复过程 |
| 数值无规律跳变，且和电流、SOC、温度都没有关系 | 采样分辨率、驱动数据刷新、采样校准或连接问题更值得怀疑 |

### 为什么不能直接理解为“容量相差多少”

电压与剩余容量不是线性对应关系。同样的 `10 mV` 压差，在不同化学体系、温度和 SOC 区间代表的 SOC 差可能完全不同：

- 在放电曲线较平坦的中间区域，明显的 SOC 差也可能只形成很小的电压差；
- 在接近充满或接近放空的陡峭区域，很小的 SOC 差就可能形成较大的电压差；
- 负载下测到的压差还混入了 `I * R` 压降，不能只用 OCV-SOC 曲线反推容量。

容量较小的电芯在串联电流相同的情况下，经过相同的充放电电量后 SOC 变化更快，所以容量差最终可能表现为压差扩大。但压差只是容量不匹配的间接迹象，无法单独换算成容量差或健康度。

### 对串联电池包的实际影响

串联电池包的可用容量受较弱的那颗电芯限制：

- 放电时，电压较低的电芯可能先到欠压保护阈值，整包必须停止放电，即使另一颗电芯还有余量；
- 充电时，电压较高的电芯可能先到过压保护阈值，整包必须停止充电，即使另一颗尚未充满；
- 因此持续存在且会在充放电末端扩大的压差，会降低电池包实际可用能量，并增加保护提前触发的可能性。

BMS 的均衡功能通常用于减小 SOC 不一致，但是否均衡、何时启动以及允许多大压差由具体设备的电池管理策略决定，单凭这份 WebUI 产物无法确认。

### 更可靠的判断方法

判断压差来源时，应同时记录 `V1`、`V2`、电流、SOC 和温度，而不是只看页面给出的绝对压差：

1. 在相近 SOC 和温度下，拔掉充电器并尽量降低系统负载，让电池充分松弛后观察。此时电流越接近零，压差越接近两颗电芯的开路电压差。
2. 比较低负载与正常负载下的压差。若压差随电流近似同步增减，主要线索是内阻差；近似关系为 `ΔR ≈ 电压差变化量 / 电流变化量`。
3. 在高、中、低 SOC 分别观察趋势。只在低 SOC 急剧扩大，更像容量、内阻或弱电芯问题；全程保持近似固定偏差，也要考虑采样偏置。
4. 保留 `V1 - V2` 的正负方向。当前页面只显示 `Math.abs(V1 - V2)`，会丢失“始终是哪一颗更低”的重要诊断信息，但页面同时展示了两个原始电压值，可以人工比较。

不存在脱离电芯化学体系、SOC、温度、负载和厂商 BMS 规格的通用毫伏阈值。瞬时压差只能作为现象；在小电流、充分静置并控制温度后仍反复出现同方向压差，才更能支持电芯 SOC 不均衡或老化不一致的判断。
