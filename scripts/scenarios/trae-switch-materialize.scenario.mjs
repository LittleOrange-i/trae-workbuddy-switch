// 「没有快照也能切换」：账号库 → 客户端（2026-09-29）。
//
// ## 这条契约是什么
//
// Trae 的切换原先只有「快照 → 客户端」一条路，于是必须先「在客户端里登录一次、
// 再保存登录态」——对**从未启动过的客户端**（本机 CN TraeCode 就是）根本做不到。
// 现在补上「账号库 → 客户端」：目标账号没有快照时，直接用账号库里的 JWT /
// refresh token 合成登录态写进客户端，交互形状与 WorkBuddy 的「切换＝写认证文件」一致。
//
// ## 证伪方式（本场景必须能红）
//
// 把 `switch_account` 的预检查改回「快照不存在即 fatal」⇒ 点下去得到的是
// 失败提示（`data-type="error"`），第 3 条断言变红。
//
// ## 前置（**必须沙箱化**，否则会写进真机客户端）
//
// 后端进程要同时重定向两个变量：
//
// ```bash
// APPDATA=D:\_bsbuild\e2e-appdata            # 客户端 userData 的父目录（沙箱）
// BUDDY_SWITCH_HOME=D:\_bsbuild\e2e-home     # store 的父目录（沙箱）
// ./target/debug/buddy-switch.exe serve --port 57890 --no-open
// ```
//
// 沙箱 `APPDATA\TRAE SOLO CN\User\globalStorage\storage.json` 要先铺一份
// 「客户端已启动过」的形态；目标账号在 store 里**不得**有快照。
// 验证结果看后端自己的读侧：`GET /api/trae/profiles?variant=trae_work` 的
// `currentAccount` 必须变成目标账号 —— 那是从客户端 `storage.json` 读出来的。

const SWITCH_BUTTON = `(() => {
  const card = [...document.querySelectorAll("article")].find((el) => el.textContent.includes("Jackey"));
  return card ? card.querySelector('button[aria-label="切换到 TraeWork"]') : null;
})()`;

export default async function (ctx) {
  await ctx.waitFor("账号卡片渲染完成", `document.querySelectorAll("article").length >= 2`, 30000);

  const clickable = await ctx.evaluate(`(() => { const b = ${SWITCH_BUTTON}; return !!b && !b.disabled; })()`);
  ctx.check("Jackey 在 TraeWork 上的切换按钮可点击", clickable === true);

  await ctx.press(SWITCH_BUTTON);
  await ctx.waitFor("出现切换结果提示", `document.querySelectorAll("[data-sonner-toast]").length > 0`, 60000);

  const toasts = JSON.parse(
    await ctx.evaluate(
      `JSON.stringify([...document.querySelectorAll("[data-sonner-toast]")].map((el) => ({ type: el.getAttribute("data-type"), text: el.textContent })))`,
    ),
  );
  const joined = toasts.map((item) => `${item.type}: ${item.text}`).join(" ｜ ");

  // ★ 核心：**没有快照也必须切成功**（走账号库 → 客户端），而不是报「请先保存登录态」。
  ctx.check("切换成功（success 提示）", toasts.some((item) => item.type === "success"), joined);
  ctx.check("没有失败提示", !toasts.some((item) => item.type === "error"), joined);

  await ctx.screenshot("trae-switch-materialize.png");
}
