//! Kitsu 开放 API 封面搜索兜底：bgm.tv / AniList 都被墙或要代理时，
//! 用 Kitsu（国内直连可达）按标题找海报。

use anyhow::Context;
use serde_json::Value;

const ENDPOINT: &str = "https://kitsu.io/api/edge/anime?page[limit]=5&filter[text]=";

fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| !matches!(
            c,
            ' ' | ':' | '：' | '·' | '-' | '—' | '_' | '（' | '）' | '(' | ')'
            | '"' | '\'' | '「' | '」' | '『' | '』' | ',' | '，' | '.' | '。'
            | '!' | '！' | '?' | '？' | '|' | '\u{3000}'
        ))
        .collect()
}

/// 去掉季数/篇章标记：「灵笼 上半季」「魔道祖师 第2季」「魔道祖师 2」
/// 都能归一成「灵笼」「魔道祖师」
fn strip_season(s: &str) -> String {
    let mut out = norm(s);
    for _ in 0..3 {
        let trimmed = strip_one_suffix(&out);
        if trimmed == out {
            break;
        }
        out = trimmed;
    }
    out.trim().to_string()
}

fn strip_one_suffix(s: &str) -> String {
    let t = s.trim();
    const WORDS: [&str; 8] = [
        "上半季", "下半季", "中章", "终章", "完结篇", "特别篇", "精编版", "OAD",
    ];
    for w in WORDS {
        if let Some(stripped) = t.strip_suffix(w) {
            return stripped.trim().to_string();
        }
    }
    // 「第X季」「第X部」「第X篇章」
    if t.ends_with("篇章") || t.ends_with('季') || t.ends_with('部') {
        if let Some(pos) = t.rfind('第') {
            let inner = &t[pos + 1..];
            let body = inner
                .trim_end_matches("季")
                .trim_end_matches("部")
                .trim_end_matches("篇章");
            if !body.is_empty()
                && body.chars().all(|c| {
                    c.is_ascii_digit()
                        || matches!(c, '一' | '二' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十')
                })
            {
                return t[..pos].trim().to_string();
            }
        }
    }
    // 结尾数字 / 罗马数字（I II III 1 2 3）
    let stripped = t.trim_end_matches(|c: char| {
        c.is_ascii_digit() || matches!(c.to_ascii_lowercase(), 'i' | 'v' | 'x')
    });
    if stripped.len() < t.len() && !stripped.is_empty() {
        return stripped.trim().to_string();
    }
    t.to_string()
}

fn title_candidates(attrs: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(t) = attrs.get("titles").and_then(Value::as_object) {
        for key in ["zh_cn", "en_cn", "ja_jp", "en_jp", "en"] {
            if let Some(v) = t.get(key).and_then(Value::as_str) {
                if !v.trim().is_empty() {
                    out.push(v.trim().to_string());
                }
            }
        }
    }
    if let Some(v) = attrs.get("canonicalTitle").and_then(Value::as_str) {
        for part in v.split('|') {
            let p = part.trim();
            if !p.is_empty() {
                out.push(p.to_string());
            }
        }
    }
    out
}

/// 标题匹配分：包含/相等 3；去季数后相等或包含 2.5；前缀 ≥3 字 1.2
fn title_score(query: &str, attrs: &Value) -> f64 {
    let q = norm(query);
    let q2 = strip_season(query);
    if q.is_empty() {
        return 0.0;
    }
    let mut best = 0.0f64;
    for cand in title_candidates(attrs) {
        let c = norm(&cand);
        let c2 = strip_season(&cand);
        if c.is_empty() {
            continue;
        }
        if c == q || c.contains(&q) || q.contains(&c) {
            best = best.max(3.0);
        } else if !q2.is_empty() && !c2.is_empty()
            && (c2 == q2 || c2.contains(&q2) || q2.contains(&c2))
        {
            best = best.max(2.5);
        } else if !q2.is_empty() && !c2.is_empty()
            && (q2.starts_with(&c2) || c2.starts_with(&q2))
            && q2.len().min(c2.len()) >= 3
        {
            best = best.max(1.2);
        }
    }
    best
}

async fn search(client: &reqwest::Client, query: &str) -> anyhow::Result<Vec<Value>> {
    let url = format!(
        "{ENDPOINT}{}",
        urlencoding::encode(query)
    );
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.api+json")
        .header("User-Agent", "CartoonNotebook/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .context("请求 Kitsu 失败")?;
    if !resp.status().is_success() {
        anyhow::bail!("Kitsu 返回 {}", resp.status());
    }
    let v: Value = resp.json().await.context("解析 Kitsu 响应失败")?;
    Ok(v.get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

/// 按标题搜海报地址；标题校验不通过返回 None（宁缺毋滥）
pub async fn find_cover(
    client: &reqwest::Client,
    query: &str,
) -> anyhow::Result<Option<String>> {
    let results = search(client, query).await?;
    let mut best_score = 0.0f64;
    let mut best_poster = String::new();
    for item in &results {
        let Some(attrs) = item.get("attributes") else {
            continue;
        };
        let poster = attrs
            .get("posterImage")
            .and_then(|p| p.get("large").and_then(Value::as_str).or_else(|| p.get("medium").and_then(Value::as_str)))
            .unwrap_or("");
        if poster.is_empty() {
            continue;
        }
        let score = title_score(query, attrs);
        if score > best_score {
            best_score = score;
            best_poster = poster.to_string();
        }
    }
    Ok((best_score >= 2.5).then_some(best_poster).filter(|s| !s.is_empty()))
}
