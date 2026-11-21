use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub stores: Vec<String>,
    /// "all", "free", "paid", "sale"
    #[serde(default = "default_price_mode")]
    pub price_mode: String,
    pub max_price_usd: Option<f64>,
    /// e.g. ["EN", "TR"]
    #[serde(default)]
    pub languages: Vec<String>,
    /// If true, include results whose language metadata is unknown when filtering by language
    #[serde(default = "default_true")]
    pub include_unknown_lang: bool,
    /// Enable obscure/indie/news/RSS deep scan
    #[serde(default = "default_true")]
    pub include_niche_news: bool,
    /// User-supplied custom RSS / news URLs to scan
    #[serde(default)]
    pub custom_feeds: Vec<String>,
    /// Sort: "relevance", "price_asc", "price_desc", "discount", "rating"
    #[serde(default = "default_sort")]
    pub sort_by: String,
}

fn default_price_mode() -> String {
    "all".to_string()
}

fn default_true() -> bool {
    true
}

fn default_sort() -> String {
    "relevance".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameResult {
    pub id: String,
    pub title: String,
    pub description: String,
    pub url: String,
    pub image_url: Option<String>,
    /// Download / discovery store: "Steam", "Epic Games", "App Store", "Google Play", "GOG", "itch.io", "Websitesi", "F-Droid", "Humble", "Haber / Kıyı Kenar", etc.
    pub store: String,
    pub store_category: String,
    /// e.g. ["PC", "Windows", "macOS", "Linux", "Android", "iOS", "Web", "PlayStation", "Xbox", "Switch"]
    pub platforms: Vec<String>,
    pub tags: Vec<String>,
    /// ISO-ish uppercase codes: "EN", "TR", "JA", "DE", "FR", "ES", "RU", "ZH", "KO"
    pub languages: Vec<String>,
    pub has_english: Option<bool>,
    pub has_turkish: Option<bool>,
    pub is_free: bool,
    pub price_usd: Option<f64>,
    pub original_price_usd: Option<f64>,
    pub price_display: String,
    pub discount_percent: Option<u32>,
    pub rating: Option<f64>,
    pub release_date: Option<String>,
    pub source_name: String,
    pub is_niche_or_news: bool,
    pub relevance_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceStatus {
    pub name: String,
    pub category: String,
    pub count: usize,
    pub elapsed_ms: u64,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub total_found: usize,
    pub filtered_count: usize,
    pub elapsed_ms: u64,
    pub sources: Vec<SourceStatus>,
    pub results: Vec<GameResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarRequest {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub store_category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotItem {
    pub thumb_url: String,
    pub full_url: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarResponse {
    pub base_title: String,
    pub used_tags: Vec<String>,
    pub elapsed_ms: u64,
    pub screenshots: Vec<ScreenshotItem>,
    pub riotpixels_url: String,
    pub results: Vec<GameResult>,
}

/// Normalize language names / codes into standard 2-letter uppercase codes
pub fn normalize_language(raw: &str) -> Option<String> {
    let s = raw.trim().to_lowercase();
    if s.is_empty() {
        return None;
    }
    let code = if s == "en" || s.starts_with("en-") || s.starts_with("en_") || s.contains("english") || s.contains("ingilizce") {
        "EN"
    } else if s == "tr" || s.starts_with("tr-") || s.contains("turkish") || s.contains("türkçe") || s.contains("turkce") {
        "TR"
    } else if s == "ja" || s == "jp" || s.contains("japanese") || s.contains("japonca") {
        "JA"
    } else if s == "de" || s.contains("german") || s.contains("almanca") {
        "DE"
    } else if s == "fr" || s.contains("french") || s.contains("fransızca") {
        "FR"
    } else if s == "es" || s.contains("spanish") || s.contains("ispanyolca") {
        "ES"
    } else if s == "ru" || s.contains("russian") || s.contains("rusça") {
        "RU"
    } else if s == "zh" || s.starts_with("zh-") || s.contains("chinese") || s.contains("çince") {
        "ZH"
    } else if s == "ko" || s.contains("korean") || s.contains("korece") {
        "KO"
    } else if s == "pt" || s.starts_with("pt-") || s.contains("portuguese") {
        "PT"
    } else if s == "it" || s.contains("italian") {
        "IT"
    } else if s == "pl" || s.contains("polish") {
        "PL"
    } else {
        return None;
    };
    Some(code.to_string())
}

pub fn compute_lang_flags(langs: &[String]) -> (Option<bool>, Option<bool>) {
    if langs.is_empty() {
        (None, None)
    } else {
        let has_en = langs.iter().any(|l| l == "EN");
        let has_tr = langs.iter().any(|l| l == "TR");
        (Some(has_en), Some(has_tr))
    }
}
