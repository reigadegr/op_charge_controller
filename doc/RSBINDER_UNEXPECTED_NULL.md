# rsbinder UnexpectedNull 故障与修复记录

记录日期：2026-09-08。日志时间为设备本地时间。

## 结论

本机 `battery` 服务的 Binder 接口描述符可以为 `null`，但服务仍然支持
`dump`。`rsbinder` 在查询描述符时将返回值按非空 `String` 解析，错误地将
这个合法空值转换为 `StatusCode::UnexpectedNull`。

修复是在 `INTERFACE_TRANSACTION` 的回复解析处读取 `Option<String>`，
并将 `None` 转换为空字符串。修复后，电池服务绑定、只读 dump 和程序启动均已通过真机验证。

## 环境与现象

- 设备：OPLUS，Android 16，SDK 36。
- 运行环境：Termux；真机 dump 测试通过 KernelSU 的 `su` 获取 root 权限。
- 程序路径：`/data/data/com.termux/files/home/op_charge_controller/target/release/op_charge_controller`。
- 最初的依赖：远程 `dumpsys-rs` 0.2.0，提交 `a239b8efcba5bcb583dcde62922c6bd3c6c5aa66`，依赖 `rsbinder` 0.5.3。
- 切换到本地 `dumpsys-rs` 后：依赖 `rsbinder` 0.10.0，仍可复现问题。

最初运行 release 程序时出现：

```text
2026-09-08 23:00:39 ERROR rsbinder::parcelable: Deserialize for String: UnexpectedNull
```

两个版本的影响不同：

| 版本 | 实测表现 | 原因 |
| --- | --- | --- |
| `rsbinder` 0.5.3 | 启动时报错，随后仍能进入“未充电”主循环 | `strong_proxy_for_handle_stability` 对 `query_interface` 的错误使用 `unwrap_or_default()`，继续创建代理 |
| `rsbinder` 0.10.0，未修复 | 反复报错，无法完成电池服务绑定 | `slow_path_p2` 将描述符查询错误向上传递，服务查询返回失败，应用每秒重试 |
| 本地修复的 `rsbinder` 0.10.0 | 成功绑定并读取电池服务，启动时不再报此错误 | 将空描述符作为有效查询结果处理 |

未修复的 0.10.0 还会输出：

```text
ERROR rsbinder::parcelable: Deserialize for String: UnexpectedNull
ERROR rsbinder::hub::servicemanager_16: Failed to check service battery: NullPointer / UnexpectedNull:
```

## 定位依据

先通过原生命令确认服务本身正常：

```sh
su -c '/system/bin/service check battery'
su -c '/system/bin/dumpsys battery'
```

第一条返回 `Service battery: found`，第二条正常输出电池状态，包含
`Current Battery Service state:` 和 `level:`。相同设备上的 root 回归测试，
却在 `BoundDumpsys::new("battery")` 处返回 `service not exist`。
因此，该错误不能直接解释为系统没有注册电池服务，也不是电量字段的解析错误。

本地 0.10.0 的相关调用路径为：

1. `Looper::enter_loop` 调用 `BoundDumpsys::new("battery")`。
2. `BoundDumpsys::new` 使用 `hub::check_service` 查找服务。
3. `rsbinder` 在创建服务代理时，通过 `query_interface` 发送 `INTERFACE_TRANSACTION`。
4. `query_interface` 按 `String` 读取接口描述符。
5. 字符串反序列化遇到表示空值的长度 `-1`，记录错误并返回 `UnexpectedNull`。
6. 错误使服务查询失败，`dumpsys-rs` 最终将其表现为 `ServiceNotExist`。

空接口描述符与空 Binder 对象是不同情况。仅用于 dump 的 Binder 服务可以没有
附加的接口描述符；这不妨碍对有效 Binder 对象执行 `DUMP_TRANSACTION`。

## 修复内容

### 1. 切换本地依赖

工作区 [Cargo.toml](../Cargo.toml) 使用本地 `dumpsys-rs`：

```toml
dumpsys-rs = { path = "/data/data/com.termux/files/home/dumpsys-rs" }
```

本地 `dumpsys-rs/Cargo.toml` 使用带补丁的 `rsbinder`：

```toml
rsbinder = { version = "^0.10", path = "vendor/rsbinder", features = ["android_11_plus"] }
```

`dumpsys-rs/vendor/rsbinder/` 基于 crates.io 的 `rsbinder` 0.10.0 源码，
包含 `LICENSE` 和 `PATCHES.md`。补丁保存在本地依赖目录中，未修改 Cargo 缓存源码。
工作区 `Cargo.lock` 已随依赖切换更新。

### 2. 修正描述符的空值处理

修复文件：`/data/data/com.termux/files/home/dumpsys-rs/vendor/rsbinder/src/thread_state.rs`。

函数：`query_interface`。

```diff
 let reply = transact(handle, INTERFACE_TRANSACTION, &data, 0)?;
-let interface: String = reply
+// Dump-only Binder services can legitimately have no attached interface.
+let interface: Option<String> = reply
     .expect("INTERFACE_TRANSACTION should have reply parcel")
     .read()?;

-Ok(interface)
+Ok(interface.unwrap_or_default())
```

这个改动只接受接口描述符的合法空值。事务失败、畸形 Parcel 等错误仍通过 `?`
向上传递，其他非空字符串字段的反序列化规则保持不变。

### 3. 应用适配与检查入口

此前的接口适配已将 [looper.rs](../crates/scheduler/src/looper.rs) 和
[battery_display.rs](../crates/scheduler/src/battery_display.rs) 改为使用
`BoundDumpsys`，并适配构造函数的 `Result` 返回值。该适配解决了编译错误，
本次描述符补丁进一步解决了运行时兼容问题。

[debug.sh](../debug.sh) 为 Clippy 和测试命令增加了 `"$@"` 参数透传，
可以通过 `sh debug.sh --release` 检查并生成 release 产物。
单独执行 `sh debug.sh` 只更新 debug 产物，不会更新用户原先运行的 release 文件。

## 回归验证

### 常规检查

在 `op_charge_controller` 仓库根目录执行：

```sh
sh debug.sh
sh debug.sh --release
```

两种配置的 Clippy 和 35 项常规测试均通过。新增的真机测试在默认检查中被忽略，
需要另行运行。`.rustfmt.toml` 保持原样，其中两个 nightly 专用选项在稳定版
rustfmt 下产生的警告按约定忽略。整个修复过程未执行任何 `cargo build*` 命令。

### 真机只读测试

新增 [battery_service.rs](../crates/scheduler/tests/battery_service.rs)，测试名为
`battery_service_can_be_bound_and_dumped`。测试绑定 `battery`，调用 `dump(&[])`，
并断言输出包含 `Battery Service state:` 和 `level:`。
测试不执行 `set`、`unplug` 或 `reset`，不会主动修改电池状态。

测试只在 Android 上编译，默认标记 `ignore`，因为它依赖真实系统服务及 dump 权限。
先运行上述检查脚本，再使用脚本输出中 `Running tests/battery_service.rs (...)`
对应的测试可执行文件，以 root 运行。以下为本次验证的 release 产物路径；
后续文件名中的哈希可能变化，应以当次脚本输出为准：

```sh
su -c '/data/data/com.termux/files/usr/bin/timeout --kill-after=2s 10s \
  /data/data/com.termux/files/home/op_charge_controller/target/release/deps/battery_service-c81d18cd96e21b91 \
  --exact battery_service_can_be_bound_and_dumped --ignored --nocapture'
```

| 验证阶段 | 结果 |
| --- | --- |
| 未修复的本地 `rsbinder` 0.10.0 | `Error: service not exist`，测试失败，退出码 101 |
| 修复后的 debug 测试产物 | 测试通过，退出码 0 |
| 修复后的 release 测试产物 | 测试通过，退出码 0 |

### release 程序启动验证

更新 release 产物后，按以下方式进行了 12 秒的 root 运行验证：

```sh
su -c '/data/data/com.termux/files/usr/bin/timeout --kill-after=2s 12s \
  /system/bin/env RUST_LOG=info \
  /data/data/com.termux/files/home/op_charge_controller/target/release/op_charge_controller \
  /data/data/com.termux/files/home/op_charge_controller/op_charge.toml'
```

`timeout` 放在 `su -c` 内部，使计时器具备终止 root 子进程的权限。
本次正常进入“未充电”主循环，没有再出现 `UnexpectedNull` 或服务绑定失败。
退出码 124 来自预定超时结束，不是程序崩溃。

## 验证范围与后续维护

本次验证覆盖当前 Android 16 设备上的服务绑定、只读电池 dump、debug/release
检查，以及未充电状态下的短时启动。未通过真机插拔充电器或修改电池状态来验证完整充电周期。

当前配置依赖上述本地绝对路径，以及 `dumpsys-rs/vendor/rsbinder/` 中的补丁。
这些修改尚未发布到远程依赖。以后切回 Git 或 crates.io 依赖时，应确认所选版本
包含等效的描述符空值修复，同步更新锁文件，并重新运行真机回归测试。

电池 dump 命令的其他行为见 [dumpsys battery 功能手册](DUMPSYS_BATTERY.md)。
