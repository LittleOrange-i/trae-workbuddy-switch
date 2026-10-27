//! Qoder **网页端**额度明细（issue #111）——逐资源包（积分包）的唯一数据源。
//!
//! ## 为什么单独一个模块
//!
//! 本仓既有的额度通道是 openapi（`Authorization: Bearer dt-…`），它只给三个聚合标量。
//! 逐包数据在**网页端私有接口** `/api/v2/me/usages/big_model_credits`，且该接口
//! **只认浏览器登录后的会话 Cookie**——openapi token 一律被拒（实测 401）。
//!
//! 2026-10-27 真机探针（本机 CN 账号，`GET {openapi_base}/api/v2/quota/usage`）确认：
//! 响应只有 `userQuota` / `addOnQuota` 两个标量块 + `outerProviders: []`（空数组），
//! **`raw` 里没有任何逐包数组**。所以「前端解析 `raw`」这条捷径不成立，必须另建会话链路。
//!
//! ## 本模块的边界
//!
//! - **只做协议**：拼 URL、发请求、校验归属、给出响应形状。**不碰 Cookie 的获取与落盘**
//!   ——那是宿主（`src-tauri/src/qoder_web.rs`）的职责，因为只有 Tauri 的
//!   `Webview::cookies()` 能拿到 httpOnly Cookie，而 core 刻意不依赖 Tauri。
//! - **不返回 Cookie 明文**：调用方传进来的是 `Cookie:` 头字符串，本模块从不把它写进
//!   返回值、日志或错误消息（与「会话 Cookie 是最高等级凭据」的纪律一致）。
//! - **失败即 `Err`，不写 0**：沿用 [`super::api::parse_quota`] 的同款纪律 ——
//!   「不知道」不能显示成「没有」。

use serde_json::Value;

use super::api;
use super::region::{self, QoderRegion};

/// 网页端逐资源包明细（`/api/v2/me/usages/big_model_credits`）。
///
/// ⚠️ 与 [`super::PATH_QUOTA_USAGE`]（openapi，`/api/v2/quota/usage`）是**两条独立通道**：
/// 前者认会话 Cookie，后者认 Bearer token。别把路径拼到对方的基址上。
pub const PATH_WEB_USAGE_CREDITS: &str = "/api/v2/me/usages/big_model_credits";

/// 网页端「用量明细」页——登录窗载它，同时也是「形态不支持」时的兜底外链。
///
/// 该 URL 不是猜的：`/api/v2/quota/usage` 的 `addOnQuota.detailUrl` 字段就指向它
/// （2026-10-27 实测 `https://qoder.com.cn/account/usage`）。
pub fn usage_page_url(region: QoderRegion) -> String {
    format!("{}/account/usage", region::realm_spec(region).website)
}

/// 逐包接口的完整 URL（基址是 **website**，不是 openapi_base —— 别搞混）。
pub fn credits_url(region: QoderRegion) -> String {
    format!(
        "{}{}",
        region::realm_spec(region).website,
        PATH_WEB_USAGE_CREDITS
    )
}

/// 归属校验用的字段名。
///
/// 上游把会话归属者写在 `user_id`（下划线，不是 `userId`）。两种都认，是因为
/// 逐包接口的字段风格与 openapi 的 `userId` 不同 —— 这一点来自参考实现的实测样本，
/// 本仓尚未拿到自己的样本（阶段零-2 待补）。**认两种只是容错，不是推测正确性**：
/// 两个字段都取不到时 [`owner_matches`] 返回 `false`（fail-closed）。
const OWNER_KEYS: [&str; 2] = ["user_id", "userId"];

/// 响应形状摘要（**只含键名与计数，不含额度数值与任何凭据**）。
///
/// 用途：阶段零-2 取证。把真实响应打回来时，人要看的是「有几个分组、每组几行、
/// 每行有哪些键、`expires_at` 是毫秒还是秒」，而不是一串数字。
pub fn shape_of(raw: &Value) -> Value {
    let mut groups = Vec::new();
    if let Some(map) = raw.as_object() {
        for (key, value) in map {
            let detail = value
                .get("quota_detail")
                .or_else(|| value.get("quotaDetail"));
            match detail {
                Some(Value::Array(rows)) => {
                    let keys: Vec<String> = rows
                        .first()
                        .and_then(|row| row.as_object())
                        .map(|obj| obj.keys().cloned().collect())
                        .unwrap_or_default();
                    groups.push(serde_json::json!({
                        "group": key,
                        "rows": rows.len(),
                        "rowKeys": keys,
                    }));
                }
                Some(Value::Null) => {
                    groups.push(serde_json::json!({
                        "group": key,
                        "rows": 0,
                        "rowKeys": Vec::<String>::new(),
                        "null": true,
                    }));
                }
                _ => {}
            }
        }
    }
    // 顶层键名也一并给出：探针要确认「有没有我们没预料到的字段」。
    let top_level: Vec<&str> = raw
        .as_object()
        .map(|obj| obj.keys().map(String::as_str).collect())
        .unwrap_or_default();
    serde_json::json!({ "topLevelKeys": top_level, "groups": groups })
}

/// 会话归属者是否与本仓账号一致（**fail-closed**：取不到就判否）。
///
/// 为什么必须有这道防线：会话 Cookie 是浏览器级的，用户可能在登录窗里登了另一个
/// 账号。若不做比对，就会把**别人的**资源包显示到当前账号上 —— 这比「没有明细」坏得多。
pub fn owner_matches(raw: &Value, expected_uid: &str) -> bool {
    if expected_uid.trim().is_empty() {
        return false;
    }
    let actual = OWNER_KEYS
        .iter()
        .find_map(|key| raw.get(*key).and_then(Value::as_str))
        .unwrap_or_default();
    // 大小写不敏感：uid 是 uuid，上游大小写形态未见统一。
    !actual.is_empty() && actual.eq_ignore_ascii_case(expected_uid.trim())
}

/// 用会话 Cookie 取逐包明细的**原始响应**。
///
/// `cookie_header` 是 `k=v; k2=v2` 形状（由宿主从 `Webview::cookies()` 拼出）。
/// 返回 `Ok(raw)` 只代表「HTTP 200 且是 JSON」；归属校验由调用方用
/// [`owner_matches`] 单独做——因为「能读到」与「读的是不是这个账号」是两件事，
/// 混在一起会让「归属不符」与「请求失败」无法区分。
pub async fn fetch_credit_packs_raw(
    region: QoderRegion,
    cookie_header: &str,
) -> Result<Value, String> {
    if cookie_header.trim().is_empty() {
        return Err("没有可用的网页会话 Cookie（请先登录 Qoder 网页端）".to_string());
    }
    let spec = region::realm_spec(region);
    let client = api::shared_client()?;
    let response = client
        .get(credits_url(region))
        .timeout(std::time::Duration::from_secs(20))
        .header("Accept", "application/json, text/plain, */*")
        .header("User-Agent", spec.user_agent)
        .header("Origin", spec.website)
        .header("Referer", format!("{}/account/usage", spec.website))
        .header("Cookie", cookie_header)
        .send()
        .await
        .map_err(|error| format!("网页端额度明细请求失败: {error}"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        // ⚠️ 只回状态码与截断后的响应体，**不回显 Cookie**。
        return Err(format!(
            "网页端额度明细请求失败: HTTP {} {}",
            status.as_u16(),
            body.chars().take(160).collect::<String>()
        ));
    }
    serde_json::from_str(&body).map_err(|error| format!("网页端额度明细响应不是 JSON: {error}"))
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn urls_use_website_base_not_openapi() {
        // 证伪点：把 `credits_url` 的基址写成 `openapi_base`，本用例立刻变红
        // ——那会让 Bearer 通道的基址与 Cookie 通道的基址混为一谈。
        assert_eq!(
            credits_url(QoderRegion::Cn),
            "https://qoder.com.cn/api/v2/me/usages/big_model_credits"
        );
        assert_eq!(
            credits_url(QoderRegion::Global),
            "https://qoder.com/api/v2/me/usages/big_model_credits"
        );
        assert!(usage_page_url(QoderRegion::Cn).ends_with("/account/usage"));
    }

    #[test]
    fn owner_matches_is_fail_closed() {
        let raw = json!({ "user_id": "ABC-123" });
        assert!(owner_matches(&raw, "abc-123"), "大小写不敏感");
        assert!(!owner_matches(&raw, "other"), "不一致必须判否");
        assert!(!owner_matches(&raw, ""), "空 uid 必须判否");
        // 字段缺失 ⇒ 判否（不得因为「取不到」就默认通过）
        assert!(!owner_matches(&json!({}), "abc-123"));
        // openapi 风格的 camelCase 也认
        assert!(owner_matches(&json!({ "userId": "abc-123" }), "abc-123"));
    }

    #[test]
    fn shape_reports_groups_and_row_keys() {
        let raw = json!({
            "user_id": "u1",
            "plan_quota": { "quota_detail": [ { "limit_value": 0, "used_value": 0 } ] },
            "resource_package_quota": { "quota_detail": [
                { "limit_value": 100, "used_value": 76, "remaining_value": 24, "source": "X", "expires_at": 1792635167844i64 }
            ] },
            "dedicated_resource_package_quota": { "quota_detail": null }
        });
        let shape = shape_of(&raw);
        let groups = shape["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 3, "三个分组都要出现在形状摘要里");
        let bonus = groups
            .iter()
            .find(|group| group["group"] == "resource_package_quota")
            .expect("资源包分组");
        assert_eq!(bonus["rows"], 1);
        let keys = bonus["rowKeys"].as_array().unwrap();
        assert!(keys.iter().any(|key| key == "expires_at"));
        // ⚠️ 摘要**不得**泄露额度数值
        assert!(!shape.to_string().contains("1792635167844"));
        assert!(!shape.to_string().contains("76"));
    }
}
