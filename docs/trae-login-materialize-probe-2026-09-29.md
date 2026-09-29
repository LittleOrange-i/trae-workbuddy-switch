# Trae「账号库 → 客户端」可行性实测（2026-09-29）

> 结论先行：**用我们手上的数据无法「忠实」地把账号库里的凭据写进 Trae 客户端**。
> 下面全部是实测数据，不是推断。判定「半注入到底行不行」的实验在本会话被环境挡住
> （见 §4），配方留在本文，谁都能在 30 秒内跑完。

## 1. 起因

用户报障「TraeCode 那条线根本用不了」，逐层查下去是：

1. 客户端没启动过 ⇒ 没有 userData ⇒ **存不了快照** ⇒ 切不过去；
2. 界面还谎报成功（已修，见 `docs/` 同日的其它记录与 `scripts/scenarios/`）。

第 1 条的根治办法看起来是「别依赖快照，直接把账号库里的凭据写进客户端」。
本文回答的就是**这件事到底能不能做**。

## 2. 实测：客户端的登录态长什么样

方法：临时探针（`icube::tests`，跑完即删）读真机 `%APPDATA%\TRAE SOLO CN` 的
`storage.json`，用本仓既有的 `tc_decrypt` 解开相关键，只打印**键名与取值形状**。

### `iCubeAuthInfo://icube.cloudide`（登录态本体）

| 键 | 真机形状 | 我们能否复现 |
|:---|:---|:---|
| `token` | string(1004)，裸 JWT（无 `Cloud-IDE-JWT ` 前缀） | ✅ 账号库 `jwt` 去掉前缀 |
| `refreshToken` | string(61) | ✅ 账号库 `refresh_token` |
| `host` | `https://api.trae.cn` | ✅ 端点表 |
| `userId` | string(16) | ✅ |
| `expiredAt` / `refreshExpiredAt` / `tokenReleaseAt` | **string(24)**，ISO-8601（`2026-10-04T…Z`），**不是** epoch 数字 | 部分（JWT 只有 `exp`） |
| `userRegion` | **对象** `{"_aiRegion":"CN","region":"CN"}` | 国内版可推，国际版未知 |
| `account` | **对象**：`username` / `email` / `avatar_url` / `nonPlainTextMobile` / `storeRegion` / `userTag` / `migrateToSG` / `loginScope` … | ❌ **服务端下发**，账号库没有 |

⚠️ 顺带发现：`cloudide_from_plain` 用 `as_i64()` 读 `expiredAt`，而真机是字符串 ⇒
该字段**永远读不到值**。不是缺陷（它只用于诊断），但说明「读侧容忍」与
「写侧必须忠实」是两件事。

### 与登录态相关的**其它键**（此前未被注意到）

| 键 | 形态 | 能否复现 |
|:---|:---|:---|
| `iCubeServerData://icube.cloudide` | **明文 JSON（非 tc 信封），4415 字符**，客户端从服务端拉的账号数据缓存 | ❌ 账号库里没有 |
| `iCubeAuthInfo://usertag` | tc 信封，内容 `{"<uid>":"cn"}` | ✅ 能写 |
| `iCubeAuthInfo://icube-dc:<deviceId>` | tc 信封，`{"privateKeyPEM":…,"publicKeyPEM":…}` | ❌ **按设计由客户端写** |
| `iCubeInstallAction` / `iCubeLastVersion` / `iCubeNativeAppFirstLaunch` | 普通字符串 / 布尔 | 与登录无关 |

## 3. 结论：为什么「注入 token」是不忠实的

客户端要的登录态**不是「一个 token」，而是一组互相引用的键**。三个缺口：

1. **`account` 对象拿不到** —— 它是服务端下发的账号档案，账号库只存 `name`；
2. **`iCubeServerData` 拿不到** —— 4415 字符的服务端数据缓存，账号库里没有，
   我们也不知道客户端从哪个接口拉的；
3. **设备凭证不能自造** —— `device.rs` 里有明确红线：「自造 `device_id` 这个能力
   在**类型层面**就不存在」（自造值会被上游按 20403/20405 拒）。
   账号库里的 `device_bindings` 只是 `<deviceId>` 的**字符串**，私钥在客户端的
   `storage.json` 里。

因此最多只能写出一份**部分**信封（token / refreshToken / host / userId / expiredAt），
它会不会被客户端接受、会不会留下半登录态，**没有实测**。

## 4. 判定实验（本会话被环境挡住，配方在此）

### 为什么本会话跑不了

- 传任何 CLI 参数都被环境拦下：`D:\Programs\TRAE SOLO CN\TRAE SOLO CN.exe: bad option: --no-sandbox`
  （这是**外层启动器**的报错，不是 Electron 的 —— Electron 会忽略未知参数）；
- 不带参数启动则**立刻以 0 退出**，沙箱目录里一个新文件都没写；
- 对照：`notepad.exe` 能正常启动并常驻 ⇒ 不是「GUI 一律被禁」，是这个客户端起不来。

⚠️ 已验证：真实 `%APPDATA%\TRAE SOLO CN` **未被改动**（近 5 分钟零写入）。

### 配方（在你自己桌面的终端里跑）

沙箱原理：把客户端的 `APPDATA` 重定向到临时目录，**不碰真机 profile**。

```bat
:: ① 造沙箱：只拷核心小文件（真机 userData 有几个 GB，整目录拷不动），
::    并把目标账号的合成信封写进 storage.json（用 icube::tc_encrypt）
::    —— 这一步的代码见 git 历史里 2026-09-29 的 probe_prepare_sandbox

:: ② 以沙箱 profile 启动客户端
set APPDATA=D:\_bsbuild\probe-appdata
start "" "D:\Programs\TRAE SOLO CN\TRAE SOLO CN.exe"

:: ③ 等它起来（看界面是否显示为 Jackey），然后关掉，再看沙箱里 storage.json 变成了什么
```

**判据**（三种结果，含义完全不同）：

| 沙箱 `cloudide` 解出来的 `userId` | 含义 |
|:---|:---|
| 变成**别的账号**（或键被删） | 客户端**拒绝**了我们的信封 ⇒ 注入路线不可行 |
| 仍是目标账号，且出现 `iCubeServerData` | 客户端**接受**并自己补齐了服务端数据 ⇒ 注入可行 |
| 仍是目标账号，但没有 `iCubeServerData` | 接受但降级 ⇒ 需要看界面是否真的可用 |

## 5. 决策记录

- 2026-09-29：用户要求实现「账号库 → 客户端」。按「先量化再投入」先做本文的实测，
  得出「不忠实」的结论后**暂停实现**，未提交任何功能代码（避免死代码与未验证行为）。
- 下一步待定：① 用户自己跑 §4 的实验；或 ② 改做「一键启动 + 引导登录 + 自动建快照」
  （编排现有能力，零风险，但仍需用户真的登录一次）；或 ③ 先攻 `iCubeServerData` 的来源。

---

## 6. 后续（2026-09-29 同日）：已按「与 WorkBuddy 交互一致」实现

用户确认目标交互要与 WorkBuddy 分区一致（点一下就把账号写进客户端，不要「先存快照」的仪式），
于是落地了本文 §3 判定为「不忠实但可用」的那条路，并**把风险显式标注**：

| 位置 | 改动 |
|:---|:---|
| `icube::tc_encrypt` | 新增生产侧信封**加密**（随机盐）；测试的 `seal_envelope` 改为委托它，两侧不再各写一份 |
| `icube::build_cloudide_envelope` | 由账号库凭据合成 `cloudide` 信封；**缺的字段一律不写**（宁缺勿造），`account` 只写 `username` |
| `icube::merge_usertag_plain` | `usertag` 是整机共享表 ⇒ 读旧的、只改自己那一条 |
| `profile::materialize_login_in_dir` | 写入 + **写完立刻读回复核**（与恢复路径同一套口径）；缺 `refresh_token` 时**响亮失败** |
| `profile::switch_account` | 预检查改为**选路**：有快照 ⇒ 恢复；没快照但客户端数据目录在、账号库里有凭据 ⇒ 直接写客户端 |

**验证**（沙箱化：后端进程同时重定向 `APPDATA` 与 `BUDDY_SWITCH_HOME`，真机客户端全程零改动）：

- 变异（把预检查改回「快照不存在即 fatal」）⇒ `trae-switch-materialize` 场景**红**，
  逐字复现旧文案「账号 … 的登录态快照不存在，请先保存该账号的登录态」；还原后全绿；
- 客户端读侧确认：切换后 `GET /api/trae/profiles?variant=trae_work` 的
  `currentAccount` = 目标账号（那是从客户端 `storage.json` 读出来的，不是我们自己的记录）；
- `cargo test -p buddy-switch-core --lib` = **844 passed / 0 failed**。

### ⚠️ 仍未验证：真实客户端是否接受这份「部分登录态」

见 §2 的两个缺口（`account` 富对象、`iCubeServerData` 缓存）。**这一条只能在真机客户端上验**，
本会话环境起不来客户端（§4）。判据：启动一次 TraeWork，看它是否仍显示为 JackDev、
以及 `%APPDATA%\TRAE SOLO CN\User\globalStorage\storage.json` 的 `cloudide` 是否被它刷新。
若不接受，`last` 槽位里的备份可回滚。

### ★★ 事故记录：一次**污染真机**的用例（必须记住）

写「切换会回退到 materialize」的单测时，我只隔离了 `BUDDY_SWITCH_HOME`，
**没有隔离 `APPDATA`** ⇒ `snapshot_data_dir_for` 指向**真机** `%APPDATA%\TRAE SOLO CN`，
用例把一份假账号的登录态写了进去，**覆盖掉真机 TraeWork 的 `cloudide` 条目**。

`test_support.rs` 的模块头第 3 条**早就写明**了这个陷阱：

> 3. **只隔离其中一个变量** —— 隔离了 home 却没隔离 `APPDATA`，
>    于是用例读到真机的 Trae 数据目录（真机上恰好有数据时「碰巧」通过）。

**处置**：用账号库里的真凭据（JWT + refresh token）重建了 `cloudide`，清掉了假 `usertag` 条目；
其余 18 个键（含 `iCubeServerData` 与 `icube-dc` 设备凭证）**未被触碰**；
应用读侧确认仍是 JackDev。**不可恢复的只有 `account` 富对象与两个时间戳字段**。
那个危险用例**已删除**，改由沙箱化的 CDP 场景覆盖（`scripts/scenarios/trae-switch-materialize.scenario.mjs`）。

**规则**：任何会走到 `switch_account` 的用例，**必须**用 `TempEnv`（它同时隔离两个变量），
或者干脆别在单测里走那条路径 —— 用沙箱化的 E2E 覆盖。
