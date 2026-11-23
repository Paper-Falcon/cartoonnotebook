use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

const ENDPOINT: &str = "https://graphql.anilist.co";

const QUERY: &str = r#"
query ($id: Int) {
  Media(id: $id, type: ANIME) {
    id
    title { romaji native english }
    startDate { year }
    episodes
    format
    description(asHtml: false)
    genres
    coverImage { extraLarge large }
  }
}
"#;

#[derive(Debug, Default, Clone)]
pub struct AnimeMeta {
    pub title_romaji: Option<String>,
    pub title_native: Option<String>,
    pub title_english: Option<String>,
    pub year: Option<i64>,
    pub episodes: Option<i64>,
    pub format: Option<String>,
    pub description: Option<String>,
    pub genres: Vec<String>,
    pub cover_url: Option<String>,
}

#[derive(Deserialize)]
struct Resp {
    data: Option<Data>,
    #[serde(default)]
    errors: Vec<ApiErr>,
}

#[derive(Deserialize)]
struct ApiErr {
    message: String,
}

#[derive(Deserialize)]
struct Data {
    #[serde(rename = "Media")]
    media: Option<Media>,
}

#[derive(Deserialize)]
struct Media {
    title: Option<Titles>,
    #[serde(rename = "startDate")]
    start_date: Option<DateY>,
    episodes: Option<i64>,
    format: Option<String>,
    description: Option<String>,
    genres: Option<Vec<String>>,
    #[serde(rename = "coverImage")]
    cover_image: Option<Cover>,
}

#[derive(Deserialize)]
struct Titles {
    romaji: Option<String>,
    native: Option<String>,
    english: Option<String>,
}

#[derive(Deserialize)]
struct DateY {
    year: Option<i64>,
}

#[derive(Deserialize)]
struct Cover {
    #[serde(rename = "extraLarge")]
    extra_large: Option<String>,
    large: Option<String>,
}

pub async fn fetch(client: &reqwest::Client, id: i64) -> Result<AnimeMeta> {
    let body = serde_json::json!({ "query": QUERY, "variables": { "id": id } });
    let resp = client
        .post(ENDPOINT)
        .header("User-Agent", "CartoonNotebook/0.1")
        .json(&body)
        .send()
        .await
        .context("请求 AniList 失败")?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("AniList 返回 {status}: {}", truncate(&text, 200));
    }

    let parsed: Resp = serde_json::from_str(&text).context("解析 AniList 响应失败")?;
    if let Some(e) = parsed.errors.first() {
        anyhow::bail!("AniList 报错: {}", e.message);
    }
    let media = parsed
        .data
        .and_then(|d| d.media)
        .context("AniList 没有返回 Media")?;

    let t = media.title.unwrap_or(Titles {
        romaji: None,
        native: None,
        english: None,
    });

    Ok(AnimeMeta {
        title_romaji: t.romaji,
        title_native: t.native,
        title_english: t.english,
        year: media.start_date.and_then(|d| d.year),
        episodes: media.episodes,
        format: media.format,
        description: media.description.map(|d| strip_html(&d)),
        genres: media.genres.unwrap_or_default(),
        cover_url: media
            .cover_image
            .and_then(|c| c.extra_large.or(c.large)),
    })
}

pub async fn download_cover(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
) -> Result<()> {
    let resp = client
        .get(url)
        .header("User-Agent", "CartoonNotebook/0.1")
        .send()
        .await
        .context("下载封面失败")?;
    if !resp.status().is_success() {
        anyhow::bail!("封面下载返回 {}", resp.status());
    }
    let bytes = resp.bytes().await.context("读取封面数据失败")?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(dest, &bytes)?;
    Ok(())
}

fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&amp;", "&")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&nbsp;", " ")
        .trim()
        .to_string()
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
