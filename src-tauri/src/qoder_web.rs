//! Qoder **网页端登录窗** + 会话 Cookie 收割（issue #111 阶段一）。
//!
//! ## 为什么需要这个窗口
//!
//! 逐资源包明细只存在于网页端私有接口，且**只认浏览器登录后的会话 Cookie**。
//! 本仓 Qoder 的登录链路是「设备码 + 系统浏览器」，全程不经内嵌 webview，
//! 因此进程从不接触 cookie store —— 这是「拿不到逐包」的真正原因（不是 httpOnly）。
//!
//! 这里补的就是那条缺失的链路：一个独立的 webview 窗口载
//! `{website}/account/usage`，用户登录一次后，用 `Webview::cookies_for_url()` 收割。
//!
//! ## 三条硬约束（都踩过坑，别省）
//!
//! 1. ★★ **收割必须是 async command + 独立线程**。`Webview::cookies()` 在 Windows 上
//!    于**同步 command 或事件处理器**里会死锁（Tauri 文档 Known issues / wry#583）。
//!    本模块把 `cookies_for_url()` 放进 `spawn_blocking`，不要「顺手」改成同步调用。
//! 2. ★★ **Cookie 明文不出这个模块**。它等价于完整身份凭据：只拼成 `Cookie:` 头交给
//!    core 的请求函数，返回值里只有**名称与条数**，不落日志、不进错误消息。
//! 3. ★ **只对桌面宿主成立**。Docker / npm CLI / 裸二进制没有 Tauri、没有 `Webview`
//!    ⇒ 对应命令在 `buddy-switch-server` 侧返回结构化「仅桌面端可用」，
//!    **不得**返回空包列表冒充「该账号无包」。
//!
//! ## 未完成的部分（刻意留给后续阶段）
//!
//! 本阶段只做「登录 → 收割 → 探针」这条最小闭环，用于**取证**真实响应形状
//! （阶段零-2）。Cookie 的加密落盘、保活真探、401 回落、前端指标行与逐包浮层
//! 属后续阶段，见 `.trellis/tasks/10-27-qoder-credit-packs/prd.md`。

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use buddy_switch_core::modules::product::{require_qoder_region, resolve_scope_param};
use buddy_switch_core::modules::qoder::region::{self, QoderRegion};
use buddy_switch_core::modules::qoder::web_usage;

/// 登录窗的 label。
///
/// 一个窗口足够：两区域名不同，cookie store 按域隔离，不会互相污染。
/// 换区域时**导航**而不是新开窗口 —— 用户不该看到两个 Qoder 登录窗。
pub const LOGIN_LABEL: &str = "qoder-web-login";

/// 登录窗尺寸（网页端用量页是桌面版布局，窄了会挤成两行）。
const WINDOW_WIDTH: f64 = 1024.0;
const WINDOW_HEIGHT: f64 = 780.0;

/// 确保登录窗存在并导航到该区域的用量页。
fn ensure_login_window<R: Runtime>(
    app: &AppHandle<R>,
    region: QoderRegion,
) -> Result<WebviewWindow<R>, String> {
    let url = web_usage::usage_page_url(region);
    let parsed: Url = url
        .parse()
        .map_err(|error| format!("登录页地址非法: {error}"))?;

    if let Some(window) = app.get_webview_window(LOGIN_LABEL) {
        // 已存在：**只在区域变了时才导航**。
        //
        // ⚠️ 无条件 `navigate` 会把用户正在填的登录表单刷掉 —— 前端每次点击都会走这里，
        //    「点两下按钮就把输了一半的账号密码清空」是必然被报的 bug。用 host 判定区域
        //    是否真的换了（两区域的 website 域名不同：qoder.com.cn / qoder.com）。
        let same_region = window
            .url()
            .map(|current| current.host_str() == parsed.host_str())
            .unwrap_or(false);
        if !same_region {
            let _ = window.navigate(parsed);
        }
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(app, LOGIN_LABEL, WebviewUrl::External(parsed))
        .title("登录 Qoder 网页端（用于读取逐包明细）")
        .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .min_inner_size(720.0, 560.0)
        .center()
        .build()
        .map_err(|error| format!("打开 Qoder 登录窗失败: {error}"))?;
    Ok(window)
}

/// 把该区域的会话 Cookie 拼成 `Cookie:` 头。
///
/// 只保留域名属于该区域的条目：cookie store 是**进程级共享**的，若不筛，
/// 会把别的站点的 cookie 一起发给 Qoder（既无意义，也是不必要的凭据外泄面）。
fn cookie_header_for<R: Runtime>(
    window: &WebviewWindow<R>,
    region: QoderRegion,
) -> Result<(String, Vec<String>), String> {
    let spec = region::realm_spec(region);
    let target: Url = web_usage::credits_url(region)
        .parse()
        .map_err(|error| format!("逐包接口地址非法: {error}"))?;
    let cookies = window
        .cookies_for_url(target)
        .map_err(|error| format!("读取网页会话 Cookie 失败: {error}"))?;

    let mut pairs: Vec<String> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for cookie in cookies {
        let domain = cookie.domain().unwrap_or_default();
        // `domain` 可能带前导点（`.qoder.com.cn`）；用后缀匹配覆盖两种形态。
        if !domain.is_empty() && !domain.trim_start_matches('.').ends_with(spec.domain) {
            continue;
        }
        let name = cookie.name().to_string();
        if names.contains(&name) {
            continue; // 同名多 path 只取第一条，避免头部重复
        }
        names.push(name.clone());
        pairs.push(format!("{name}={}", cookie.value()));
    }
    Ok((pairs.join("; "), names))
}

/// 打开（或前置）Qoder 网页端登录窗。
///
/// 前端入口：账号卡 / 抽屉额度区的「登录 Qoder 网页端以获取逐包明细」按钮。
///
/// ⚠️ 本模块只提供实现；`#[tauri::command]` 的薄包装在 `commands.rs`
/// （`scripts/check-api-contract.cjs` 只认 `commands::<name>` 形态的登记，写在别处会被判「未登记」）。
pub async fn login_open<R: Runtime>(app: &AppHandle<R>, scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    let window = ensure_login_window(app, region)?;
    let _ = window.show();
    let _ = window.set_focus();
    Ok(json!({
        "opened": true,
        "url": web_usage::usage_page_url(region),
    }))
}

/// 关闭登录窗（已关闭 ⇒ 幂等成功）。
pub async fn login_close<R: Runtime>(app: &AppHandle<R>) -> Result<Value, String> {
    let closed = match app.get_webview_window(LOGIN_LABEL) {
        Some(window) => {
            window
                .close()
                .map_err(|error| format!("关闭 Qoder 登录窗失败: {error}"))?;
            true
        }
        None => false,
    };
    Ok(json!({ "closed": closed }))
}

/// 收割会话 Cookie 并探一次逐包接口（**阶段零-2 取证**）。
///
/// 返回体刻意包含 `raw`：本阶段的唯一目的是拿到真实响应形状来设计解析器
/// （`shape` 只给键名与计数，`raw` 给完整样本）。⚠️ 两者都**不含 Cookie**。
pub async fn credit_packs_probe<R: Runtime>(
    app: &AppHandle<R>,
    scope: String,
    uid: String,
) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    if uid.trim().is_empty() {
        return Err("缺少 uid".to_string());
    }

    let window = app
        .get_webview_window(LOGIN_LABEL)
        .ok_or_else(|| "登录窗尚未打开：请先点击「登录 Qoder 网页端」".to_string())?;

    // ★★ 必须离开当前线程读 cookie：Windows 上 `cookies()` 在同步上下文会死锁
    //    （Tauri Known issues / wry#583）。`spawn_blocking` 同时满足「独立线程」。
    let (header, names) =
        tauri::async_runtime::spawn_blocking(move || cookie_header_for(&window, region))
            .await
            .map_err(|error| format!("读取网页会话失败: {error}"))??;

    if header.is_empty() {
        return Ok(json!({
            "hasSession": false,
            "cookieCount": 0,
            "cookieNames": Vec::<String>::new(),
            "message": "登录窗里还没有该区域的会话 Cookie：请先在窗口中完成登录。",
        }));
    }

    let raw = web_usage::fetch_credit_packs_raw(region, &header).await?;
    let matches = web_usage::owner_matches(&raw, &uid);
    Ok(json!({
        "hasSession": true,
        // ⚠️ 只给名称与条数，**不给值**。
        "cookieCount": names.len(),
        "cookieNames": names,
        "uidMatches": matches,
        "shape": web_usage::shape_of(&raw),
        "raw": raw,
    }))
}
