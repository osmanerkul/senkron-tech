use crate::models::GameResult;
use regex::Regex;
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;
use std::collections::HashSet;

pub async fn search_google_play(client: &Client, query: &str, tags: &[String]) -> Result<Vec<GameResult>, String> {
    let term = if !query.trim().is_empty() {
        format!("{} game", query.trim())
    } else if !tags.is_empty() {
        format!("{} game", tags.join(" "))
    } else {
        "indie game".to_string()
    };

    let url = format!(
        "https://play.google.com/store/search?q={}&c=apps&hl=en&gl=US",
        urlencoding::encode(&term)
    );

    let resp = client
        .get(&url)
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .await
        .map_err(|e| format!("Google Play istek hatası: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Google Play HTTP {}", resp.status()));
    }

    let html_text = resp
        .text()
        .await
        .map_err(|e| format!("Google Play okuma hatası: {}", e))?;

    let doc = Html::parse_document(&html_text);
    let link_sel = Selector::parse("a[href*='/store/apps/details?id=']").unwrap();
    let img_sel = Selector::parse("img").unwrap();
    let id_re = Regex::new(r"id=([A-Za-z0-9._]+)").unwrap();

    let mut seen = HashSet::new();
    let mut results = Vec::new();

    for el in doc.select(&link_sel) {
        if results.len() >= 12 {
            break;
        }
        let href = match el.value().attr("href") {
            Some(h) => h,
            None => continue,
        };
        let pkg_id = match id_re.captures(href).and_then(|c| c.get(1)) {
            Some(m) => m.as_str().to_string(),
            None => continue,
        };
        if !seen.insert(pkg_id.clone()) {
            continue;
        }

        // Extract text lines inside the card
        let text_parts: Vec<String> = el
            .text()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty() && t != "star" && t != "INSTALLED")
            .collect();

        let title = text_parts
            .first()
            .cloned()
            .unwrap_or_else(|| pkg_id.clone());

        if title.len() < 2 {
            continue;
        }

        let dev_or_sub = text_parts.get(1).cloned().unwrap_or_default();

        // Check for price in card text
        let price_str = text_parts
            .iter()
            .find(|t| t.starts_with('$') || t.starts_with('₺') || t.eq_ignore_ascii_case("Free"))
            .cloned();

        let (is_free, price_usd, price_display) = match price_str {
            Some(p) if p.starts_with('$') => {
                let val = p.trim_start_matches('$').parse::<f64>().ok();
                (false, val, p)
            }
            _ => (true, Some(0.0), "Ücretsiz / Uygulama İçi".to_string()),
        };

        let image_url = el
            .select(&img_sel)
            .filter_map(|img| img.value().attr("src").or_else(|| img.value().attr("data-src")))
            .find(|src| src.starts_with("http"))
            .map(|s| s.to_string());

        let description = if !dev_or_sub.is_empty() {
            format!("Android Oyunu • {}", dev_or_sub)
        } else {
            format!("Google Play Android Oyunu ({})", pkg_id)
        };

        results.push(GameResult {
            id: format!("gplay-{}", pkg_id),
            title,
            description,
            url: format!("https://play.google.com/store/apps/details?id={}&hl=en", pkg_id),
            image_url,
            store: "Google Play".to_string(),
            store_category: "googleplay".to_string(),
            platforms: vec!["Mobile".to_string(), "Android".to_string()],
            tags: vec!["Android".to_string(), "Mobil".to_string()],
            languages: vec!["EN".to_string()],
            has_english: Some(true),
            has_turkish: None,
            is_free,
            price_usd,
            original_price_usd: None,
            price_display,
            discount_percent: None,
            rating: None,
            release_date: None,
            source_name: "Google Play Tarayıcı".to_string(),
            is_niche_or_news: false,
            relevance_score: 0.0,
        });
    }

    Ok(results)
}

pub async fn search_fdroid(client: &Client, query: &str, tags: &[String]) -> Result<Vec<GameResult>, String> {
    let term = if !query.trim().is_empty() {
        query.trim().to_string()
    } else if !tags.is_empty() {
        tags.join(" ")
    } else {
        "game".to_string()
    };

    let url = format!(
        "https://search.f-droid.org/api/search_apps?q={}",
        urlencoding::encode(&term)
    );

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("F-Droid istek hatası: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("F-Droid HTTP {}", resp.status()));
    }

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("F-Droid JSON hatası: {}", e))?;

    let apps = json
        .get("apps")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut results = Vec::new();
    for app in apps.into_iter().take(8) {
        let name = app.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let summary = app.get("summary").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let url = app.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let icon = app.get("icon").and_then(|v| v.as_str()).map(|s| s.to_string());
        if name.is_empty() || url.is_empty() {
            continue;
        }
        results.push(GameResult {
            id: format!("fdroid-{}", name.to_lowercase().replace(' ', "-")),
            title: name,
            description: format!("Açık Kaynak Android (F-Droid) • {}", summary),
            url,
            image_url: icon,
            store: "F-Droid".to_string(),
            store_category: "fdroid".to_string(),
            platforms: vec!["Mobile".to_string(), "Android".to_string()],
            tags: vec!["Açık Kaynak".to_string(), "Android".to_string(), "Kıyı Kenar".to_string()],
            languages: vec!["EN".to_string()],
            has_english: Some(true),
            has_turkish: None,
            is_free: true,
            price_usd: Some(0.0),
            original_price_usd: None,
            price_display: "Ücretsiz (FOSS)".to_string(),
            discount_percent: None,
            rating: None,
            release_date: None,
            source_name: "F-Droid (Açık Kaynak)".to_string(),
            is_niche_or_news: true,
            relevance_score: 0.0,
        });
    }

    Ok(results)
}
