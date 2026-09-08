# dumpsys battery 功能手册

> 基于真机实测（OPLUS 设备，Android 16 / BP2A.250605.015，KernelSU root）。
> 所有行为均为 `su -c dumpsys battery ...` 实测结果，非纯源码推断。

Rust 库绑定电池服务时的 `UnexpectedNull` 问题，参见 [故障与修复记录](RSBINDER_UNEXPECTED_NULL.md)。

## 核心结论

`dumpsys battery` 是 Android `BatteryService` 的调试接口，核心机制只有两条：

1. **`set` / `unplug` = 冻结**：强制 Framework 使用伪造的电池状态，
   同时让 `BatteryService` 停止处理内核上报的真实数据
   （dump 输出出现 `(UPDATES STOPPED -- use 'reset' to restart)` 标记）。
2. **`reset` = 解冻**：恢复跟随内核真实值，冻结期间积累的内核事件被丢弃后重新同步。

冻结期间即使 `get -f`（强制读最新硬件值）也返回伪造值——
内核数据源被彻底短路。

## 命令总览

| 命令 | 形式 | 作用 | 是否改变系统状态 |
| --- | --- | --- | --- |
| `help` | `dumpsys battery help` | 打印帮助 | 否 |
| `get` | `get [-f] <prop>` | 读一个电池属性（单值输出） | 否 |
| `set` | `set [-f] <prop> <value>` | 伪造属性值并冻结状态 | **是**（可 reset 恢复） |
| `unplug` | `unplug [-f]` | 强制模拟"拔掉电源"并冻结 | **是**（可 reset 恢复） |
| `reset` | `reset [-f]` | 解冻，恢复真实硬件值 | **是**（恢复性） |
| （无参数） | `dumpsys battery` | 输出完整状态 dump | 否 |

`-f` 的含义见「`-f` 参数与广播 sequence」一节。

## help 原文

```text
Battery service (battery) commands:
  help
    Print this help text.
  get [-f] [ac|usb|wireless|dock|status|level|temp|present|counter|invalid|current_now|current_average]
    Gets the value of a battery state.
    -f: force to get the latest property value.
  set [-f] [ac|usb|wireless|dock|status|level|temp|present|counter|invalid|current_now|current_average] <value>
    Force a battery property value, freezing battery state.
    -f: force a battery change broadcast be sent, prints new sequence.
  unplug [-f]
    Force battery unplugged, freezing battery state.
    -f: force a battery change broadcast be sent, prints new sequence.
  reset [-f]
    Unfreeze battery state, returning to current hardware values.
    -f: force a battery change broadcast be sent, prints new sequence.
```

## 无参数 dump 输出解读

`dumpsys battery`（不带子命令）输出两段状态：

### OPLUS 扩展段（`Current OPLUS Battery Service state:`）

| 字段 | 实测样例 | 说明 |
| --- | --- | --- |
| `Charger voltage` | `0` | 充电器电压（mV，未插电为 0） |
| `Battery current` | `398` | 电池电流读数 |
| `ChargeFastCharger` | `false` | 是否快充充电器 |
| `PlugType` | `0` / `1` | 插电类型（`set ac 1` 后实测变为 `1`） |
| `UpdatesStopped` | `false` | OPLUS 侧的更新停止标志 |
| `PhoneTemp` | `-10` | 手机温度（未知单位，常态为负值） |
| `ThermalFeatureOn` | `true` | 温控特性开关 |

### AOSP 标准段（`Current Battery Service state:`）

| 字段 | 实测样例 | 说明 |
| --- | --- | --- |
| `(UPDATES STOPPED ...)` | 无 / 有 | **冻结标记**，仅 `set`/`unplug` 后出现，`reset` 消失 |
| `AC powered` 等 4 行 | `false` | ac / usb / wireless / dock 四路供电 |
| `Max charging current` | `0` | 最大充电电流 |
| `Charge counter` | `3688000` | 电量计数（µAh） |
| `status` | `3` | 电池状态码，见下表 |
| `health` | `2` | 健康度（2 = good） |
| `level` | `85` | 电量百分比 |
| `voltage` | `4209` | 电压（mV） |
| `temperature` | `402` | 温度（0.1 °C，即 40.2 °C） |
| `technology` | `Li-ion` | 电池技术 |
| `Charging state` / `Charging policy` | `0` | OPLUS 充电状态/策略 |
| `Capacity level` | `3` | 电量档位 |

### status 状态码

| 值 | 常量 | 含义 |
| ---: | --- | --- |
| 1 | `BATTERY_STATUS_UNKNOWN` | 未知 |
| 2 | `BATTERY_STATUS_CHARGING` | 充电中 |
| 3 | `BATTERY_STATUS_DISCHARGING` | 放电中 |
| 4 | `BATTERY_STATUS_NOT_CHARGING` | 已插电但未充电 |
| 5 | `BATTERY_STATUS_FULL` | 已充满 |

## get：读单个属性

形式：`dumpsys battery get [-f] <prop>`，输出只有一行值，适合脚本解析。

12 个属性及实测样例（未插电、电量 85%）：

| 属性 | 实测值 | 类型 / 单位 | 说明 |
| --- | --- | --- | --- |
| `ac` | `false` | 布尔 | AC（大功率电源）供电 |
| `usb` | `false` | 布尔 | USB 供电 |
| `wireless` | `false` | 布尔 | 无线供电 |
| `dock` | `false` | 布尔 | 底座供电 |
| `status` | `3` | 整数 | 电池状态码（见上表） |
| `level` | `85` | 0–100 | 电量百分比 |
| `temp` | `400` | 0.1 °C | 电池温度（400 = 40.0 °C） |
| `present` | `true` | 布尔 | 电池是否在位 |
| `counter` | `3672000` | µAh | 电量计数 |
| `invalid` | `0` | 0/1 | 充电器有效性标志（**有坑，见注意事项**） |
| `current_now` | `305` | mA | 瞬时电流读数 |
| `current_average` | `0` | mA | 平均电流读数 |

`get` 不带 `-f` 时返回的是 BatteryService 缓存的值；
带 `-f`（`force to get the latest property value`）会先向底层强制拉取一次最新值。
但注意：**处于冻结状态时，`-f` 也返回伪造值**（实测 `set status 2` 后
`get -f status` 仍返回 2），冻结优先级高于强制刷新。

## set：伪造属性并冻结

形式：`dumpsys battery set [-f] <prop> <value>`。

行为（实测）：

1. 写入伪造值，立即生效（`get` 立刻返回新值）；
2. 整个 BatteryService 进入冻结态：dump 出现
   `(UPDATES STOPPED -- use 'reset' to restart)`；
3. 冻结后内核上报的任何真实变化都不再影响 Framework；
4. 各属性可以叠加伪造（实测依次 `set status 2`、`set ac 1`、
   `set level 50`、`set temp 350`、`set invalid 1`、`set current_now -500`
   全部生效并存）。

实测可 set 的属性与 get 相同（12 个）。常用组合示例：

```sh
# 伪装成"插着 AC 电源、充电中"（充电控制程序初始化序列）
su -c "dumpsys battery set ac 1"
su -c "dumpsys battery set status 2"

# 恢复真实状态
su -c "dumpsys battery reset"
```

副作用提示：`set level 50` 会让系统 UI 电量立即显示 50%，
省电策略、低电量警告等都会按伪造值运作，直到 `reset`。

## unplug：模拟拔掉电源

形式：`dumpsys battery unplug [-f]`。

等效于把"是否插电"伪造为否，同样进入冻结态。适用场景：
测试应用在"拔掉充电器"瞬间的行为（如停止高耗电任务、弹出省电提示），
而无需物理拔线。

注意它只是 Framework 层的伪装，**不会真正切断充电电流**——
内核充电 IC 仍在充电（除非另有充电禁用节点）。

## reset：解冻并恢复真实值

形式：`dumpsys battery reset [-f]`。

行为（实测）：

1. 清除冻结态（`UPDATES STOPPED` 标记消失）；
2. `level`、`temp`、`status`、`ac`、`current_now` 等属性立即回到
   内核真实值（实测 level 85 / temp 400 / status 3 全部自动恢复）；
3. **例外：`invalid` 标志不随 reset 恢复**（实测 reset 后 `get invalid`
   仍为 1，需手动 `set invalid 0`，见注意事项）。

典型用途：

- 充电控制程序周期结束时"放手"，让系统重新同步真实状态；
- 任何 set/unplug 实验后的收尾清理。

## `-f` 参数与广播 sequence

`-f` = force（强制）。三个命令上的含义不同：

| 命令 | `-f` 的作用 | 实测输出 |
| --- | --- | --- |
| `get` | 强制向底层拉取最新属性值 | 属性值一行 |
| `set` | 强制发送一次电池状态变化广播 | **本机实测无输出**（见坑 #2） |
| `unplug` / `reset` | 强制发送一次电池状态变化广播 | 打印新广播的 sequence 整数 |

广播 sequence 是 `ACTION_BATTERY_CHANGED` 粘性广播的序号，
每次强制广播自增（实测 `unplug -f` → `10010`，随后 `reset -f` → `10012`）。
拿到新 sequence 说明监听广播的组件（系统 UI、省电策略等）已被强制刷新。

## 实测踩到的坑

### 坑 1：`invalid` 不随 reset 恢复

实测序列：`set invalid 1` → `reset` → `get invalid` 仍返回 `1`。
它不像其他属性走"冻结-解冻"路径，而是一个独立标志，
`reset` 不清除。恢复方法：

```sh
su -c "dumpsys battery set invalid 0"
```

### 坑 2：清除 invalid 又会重新触发冻结

`set invalid 0` 本身也是一次 `set`，实测执行后 dump 又出现
`UPDATES STOPPED`。完整清理序列应为：

```sh
su -c "dumpsys battery set invalid 0"   # 清标志（会再次冻结）
su -c "dumpsys battery reset"            # 再解冻
```

验证清理干净的三项检查：

```sh
su -c "dumpsys battery get invalid"      # 期望 0
su -c "dumpsys battery get status"       # 期望回到真实值（本机未插电为 3）
su -c "dumpsys battery" | grep -c "UPDATES STOPPED"   # 期望 0
```

### 坑 3：`set ... -f` 在本机不打印 sequence

帮助文本称 `set -f` 会 "prints new sequence"，但本机实测
`set status 5 -f`、`set level 50 -f` 均无任何输出（OPLUS 修改所致？）。
若脚本需要拿到广播序号做同步，改用 `unplug -f` / `reset -f` 的输出。

### 坑 4：冻结期间系统行为按伪造值走

冻结不只是"显示"问题——省电模式触发、低电量告警、充电动画、
部分 app 的充电监听逻辑都吃 `ACTION_BATTERY_CHANGED` 的伪造值。
长时间冻结（如 `set level 50` 后忘记 `reset`）会造成系统误动作。
做实验时务必结尾 `reset` 并按坑 2 的三项检查收尾。

## 在充电控制程序中的用法（`~/misc/batt` 项目）

该项目复刻 OPPO 原版充电控制（strace 逆向），只用到了 3 条命令：

| 调用 | 位置 | 时机 |
| --- | --- | --- |
| `dumpsys battery set ac 1` | `batt-charging/src/loop_.rs`（`init_charging`） | 充电控制启动 |
| `dumpsys battery set status 2` | 同上 | 充电控制启动 |
| `dumpsys battery reset` | `batt-charging/src/phase/cycle.rs`（`handle_cycle_end`） | 充电周期结束（`thermal_hi ≤ 20`） |

设计意图：程序在 sysfs 层持续强改充电 IC 电流（`UFCS_CURR/PPS_CURR`
的 `force_val`/`force_active`），内核侧状态会剧烈波动。启动时先
`set ac 1` + `set status 2` 把 Framework **冻结在"AC 电源 + 充电中"**，
防止系统 UI / 策略随瞬时状态乱跳；周期结束时 `reset_votables()` +
`dumpsys battery reset` 放手解冻，让充电栈重新协商协议后从 500 mA
重新爬流（`RESTART_RISE` 阶段）。

一句话：**dumpsys battery 是"蒙住系统眼睛"的开关——控制期间冻结
Framework 的电池状态感知，结束后解冻还原。**

## 速查表

```sh
# 查看
su -c dumpsys battery                    # 完整状态（含冻结标记）
su -c "dumpsys battery get status"       # 单值读，脚本友好
su -c "dumpsys battery get -f status"    # 强制拉最新（冻结时仍返回伪造值）
su -c "dumpsys battery help"             # 帮助

# 伪造（都会冻结，记得收尾）
su -c "dumpsys battery set ac 1"         # 伪装 AC 供电
su -c "dumpsys battery set status 2"     # 伪装充电中
su -c "dumpsys battery set level 50"     # 伪装电量 50%
su -c "dumpsys battery set temp 350"     # 伪装温度 35.0°C
su -c "dumpsys battery unplug"           # 伪装拔线（不切断真实充电）

# 恢复
su -c "dumpsys battery reset"            # 解冻 + 恢复真实值（invalid 除外）
su -c "dumpsys battery set invalid 0"    # invalid 需手动清
su -c "dumpsys battery reset -f"         # 解冻 + 强制广播（打印 sequence）
```




