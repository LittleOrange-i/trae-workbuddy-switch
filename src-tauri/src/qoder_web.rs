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
use buddy_switch_core::modules::qoder::web_session;
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
///
/// ## 会话来源（`source` 字段）
///
/// 1. `window` —— 登录窗里现读的 Cookie（用户刚登录）；
/// 2. `stored` —— 窗口里没有、但本地缓存里有一份**新鲜的、uid 匹配**的会话
///    （用户之前登录过，这次只是没开窗）。少这一步的话，每次取数都要重新登录一次。
///
/// 上游 401/403 ⇒ 本地缓存立刻作废（那份 Cookie 已经死了，留着只会反复失败）。
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

    // ① 登录窗（可能没开 —— 没开不等于失败，下面还有本地缓存）。
    let window = app.get_webview_window(LOGIN_LABEL);
    let mut names: Vec<String> = Vec::new();
    let mut header = String::new();
    let mut source = "none";
    if let Some(window) = window {
        // ★★ 必须离开当前线程读 cookie：Windows 上 `cookies()` 在同步上下文会死锁
        //    （Tauri Known issues / wry#583）。`spawn_blocking` 同时满足「独立线程」。
        let window = window.clone();
        if let Ok(pair) =
            tauri::async_runtime::spawn_blocking(move || cookie_header_for(&window, region)).await
        {
            if let Ok((header_from_window, window_names)) = pair {
                if !header_from_window.is_empty() {
                    header = header_from_window;
                    names = window_names;
                    source = "window";
                }
            }
        }
    }

    // ② 本地缓存（保鲜 + uid 归属双重校验）。
    if header.is_empty() {
        let now = web_usage_now_ms();
        if let Some(session) = web_session::load(region) {
            if session.is_fresh(now) && session.matches_uid(&uid) {
                header = session.cookie;
                names = session.cookie_names;
                source = "stored";
            }
        }
    }

    if header.is_empty() {
        return Ok(json!({
            "hasSession": false,
            "source": source,
            "cookieCount": 0,
            "cookieNames": Vec::<String>::new(),
            "message": "还没有可用的网页会话：请先点击「登录 Qoder 网页端」并在窗口中完成登录。",
        }));
    }

    let raw = match web_usage::fetch_credit_packs_raw(region, &header).await {
        Ok(raw) => raw,
        Err(error) => {
            // ★ 只有「上游明确拒绝」才清缓存；网络抖动不清（否则一次抖动就掉登录态）。
            if error.unauthorized {
                let _ = web_session::clear(region);
            }
            return Err(error.message);
        }
    };
    let matches = web_usage::owner_matches(&raw, &uid);
    if matches {
        // 归属对得上 ⇒ 这份会话值得留下来，下次不必重新登录。
        let now = web_usage_now_ms();
        let _ = web_session::save(
            region,
            &web_session::WebSession {
                uid: uid.trim().to_string(),
                cookie: header,
                cookie_names: names.clone(),
                saved_at_ms: now,
                verified_at_ms: now,
            },
        );
    } else {
        // ★ 归属不符 ⇒ 这是**别人的**会话，绝不能留在本机、更不能显示给当前账号。
        let _ = web_session::clear(region);
    }
    Ok(json!({
        "hasSession": true,
        "source": source,
        // ⚠️ 只给名称与条数，**不给值**。
        "cookieCount": names.len(),
        "cookieNames": names,
        "uidMatches": matches,
        "shape": web_usage::shape_of(&raw),
        "raw": raw,
    }))
}

/// 当前区域有没有可用的网页会话（前端据此决定显示「登录」按钮还是「已登录」）。
///
/// 返回 `hasSession`（新鲜且 uid 匹配）、`expired`（有缓存但过期/换过账号）。
/// 两者**不是**一件事：过期要引导重新登录，匹配要引导清除。
pub async fn session_status(scope: String, uid: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    let now = web_usage_now_ms();
    let mut source = "none";
    let stored = web_session::load(region);
    let has_session = stored
        .as_ref()
        .map(|session| session.is_fresh(now) && session.matches_uid(&uid))
        .unwrap_or(false);
    if has_session {
        source = "stored";
    }
    let payload = serde_json::json!({
        "hasSession": has_session,
        "expired": !has_session && stored.is_some(),
        "source": source,
        "cookieNames": stored.as_ref().map(|s| s.cookie_names.clone()).unwrap_or_default(),
        "savedAtMs": stored.as_ref().map(|s| s.saved_at_ms).unwrap_or(0),
        "verifiedAtMs": stored.as_ref().map(|s| s.verified_at_ms).unwrap_or(0),
    });
    Ok(payload)
}

/// 清除本地会话缓存（「退出网页登录」；幂等）。
pub async fn session_logout(scope: String) -> Result<Value, String> {
    let scope = resolve_scope_param(Some(&scope))?;
    let region = require_qoder_region(scope)?;
    web_session::clear(region)?;
    Ok(json!({ "cleared": true }))
}

/// 当前毫秒。包一层是为了测试里能替换（当前没有单元测试直接依赖它，
/// 但把时间源收在一处，别让 `Instant::now()` 散落）。
fn web_usage_now_ms() -> i64 {
    buddy_switch_core::modules::config::now_ms()
}
