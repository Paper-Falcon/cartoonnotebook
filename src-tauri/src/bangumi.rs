use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

const API: &str = "https://api.bgm.tv";
// bgm.tv 的 API 礼仪：必须带能联系到项目的信息
const UA: &str = "Paper-Falcon/cartoonnotebook (https://github.com/Paper-Falcon/cartoonnotebook)";

/// 补全用的元数据，字段对齐 anilist::AnimeMeta
#[derive(Debug, Default, Clone)]
pub struct SubjectMeta {
    pub name_cn: Option<String>,
    pub name: Option<String>,
    pub year: Option<i64>,
    pub episodes: Option<i64>,
    pub summary: Option<String>,
    pub cover_url: Option<String>,
    pub genres: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectHit {
    pub id: i64,
    pub name: String,
    pub name_cn: String,
    pub year: Option<i64>,
    pub eps: Option<i64>,
    pub cover: Option<String>,
}

#[derive(Deserialize, Default)]
struct Images {
    #[serde(default)]
    common: Option<String>,
    #[serde(default)]
    large: Option<String>,
    #[serde(default)]
    medium: Option<String>,
}

#[derive(Deserialize)]
struct Subject {
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(rename = "name_cn", default)]
    name_cn: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    eps: Option<i64>,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    images: Images,
}

#[derive(Deserialize)]
struct SearchResp {
    #[serde(default)]
    data: Vec<Subject>,
}

fn nonempty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn to_meta(s: &Subject) -> SubjectMeta {
    let year = s
        .date
        .as_deref()
        .and_then(|d| d.get(0..4))
        .and_then(|y| y.parse::<i64>().ok());
    SubjectMeta {
        name_cn: nonempty(&s.name_cn),
        name: nonempty(&s.name),
        year,
        episodes: s.eps,
        summary: nonempty(&s.summary).map(|t| t.replace('\r', "")),
        cover_url: s
            .images
            .large
            .clone()
            .or(s.images.common.clone())
            .or(s.images.medium.clone()),
            genres: vec![],
    }
}

fn hit_from(s: &Subject) -> SubjectHit {
    let m = to_meta(s);
    SubjectHit {
        id: s.id,
        name: m.name.unwrap_or_default(),
        name_cn: m.name_cn.unwrap_or_default(),
        year: m.year,
        eps: m.episodes,
        cover: m.cover_url,
    }
}

/// 在线搜索动画条目（type=2 是动画；国漫、日漫都在里面）
pub async fn search(client: &reqwest::Client, q: &str) -> Result<Vec<SubjectHit>> {
    let body = serde_json::json!({ "keyword": q, "filter": { "type": [2] } });
    let resp = client
        .post(format!("{API}/v0/search/subjects"))
        .header("User-Agent", UA)
        .json(&body)
        .send()
        .await
        .context("请求 Bangumi 失败")?;
    if !resp.status().is_success() {
        anyhow::bail!("Bangumi 返回 {}", resp.status());
    }
    let parsed: SearchResp = resp.json().await.context("解析 Bangumi 响应失败")?;
    Ok(parsed.data.iter().take(8).map(hit_from).collect())
}

/// 拉单个条目的完整信息（添加入库时补全用）
pub async fn subject_meta(client: &reqwest::Client, id: i64) -> Result<SubjectMeta> {
    let resp = client
        .get(format!("{API}/v0/subjects/{id}"))
        .header("User-Agent", UA)
        .send()
        .await
        .context("请求 Bangumi 失败")?;
    if !resp.status().is_success() {
        anyhow::bail!("Bangumi 返回 {}", resp.status());
    }
    let subject: Subject = resp.json().await.context("解析 Bangumi 响应失败")?;
    Ok(to_meta(&subject))
}

/// 封面下载直接复用 anilist 的实现（同一个 reqwest 客户端）
pub async fn download_cover(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
) -> Result<()> {
    crate::anilist::download_cover(client, url, dest).await
}

impl From<SubjectMeta> for crate::anilist::AnimeMeta {
    fn from(m: SubjectMeta) -> Self {
        crate::anilist::AnimeMeta {
            title_cn: m.name_cn,
            title_romaji: None,
            title_native: m.name,
            title_english: None,
            year: m.year,
            episodes: m.episodes,
            format: None,
            description: m.summary,
            genres: m.genres,
            cover_url: m.cover_url,
        }
    }
}
