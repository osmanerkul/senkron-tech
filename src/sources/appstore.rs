use crate::models::{compute_lang_flags, normalize_language, GameResult};
use reqwest::Client;
use serde_json::Value;
use std::collections::HashSet;

pub async fn search_appstore(client: &Client, query: &str, tags: &[String]) -> Result<Vec<GameResult>, String> {
    let term = if !query.trim().is_empty() {
        query.trim().to_string()
    } else if !tags.is_empty() {
        tags.join(" ")
    } else {
        "rpg game".to_string()
    };

    let url = format!(
        "https://itunes.apple.com/search?term={}&entity=software&genreId=6014&limit=15&country=US",
        urlencoding::encode(&term)
    );

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("App Store istek hatası: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("App Store HTTP {}", resp.status()));
    }

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("App Store JSON hatası: {}", e))?;

    let items = json
        .get("results")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut results = Vec::new();
    for item in items {
        let track_id = item.get("trackId").and_then(|v| v.as_u64()).unwrap_or(0);
        let title = item
            .get("trackName")
            .and_then(|v| v.as_str())
            .unwrap_or("Bilinmeyen iOS Oyunu")
            .to_string();
        let desc = item
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .chars()
            .take(260)
            .collect::<String>();
        let url = item
            .get("trackViewUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("https://apps.apple.com/")
            .to_string();
        let image_url = item
            .get("artworkUrl512")
            .or_else(|| item.get("artworkUrl100"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let price = item.get("price").and_then(|v| v.as_f64());
        let formatted_price = item
            .get("formattedPrice")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let is_free = price == Some(0.0) || formatted_price.eq_ignore_ascii_case("free");
        let price_display = if is_free {
            "Ücretsiz".to_string()
        } else if !formatted_price.is_empty() {
            formatted_price.to_string()
        } else if let Some(p) = price {
            format!("${:.2}", p)
        } else {
            "App Store".to_string()
        };

        // Exact ISO language codes from iTunes API!
        let mut lang_set = HashSet::new();
        if let Some(langs) = item.get("languageCodesISO2A").and_then(|v| v.as_array()) {
            for l in langs {
                if let Some(code) = l.as_str().and_then(normalize_language) {
                    lang_set.insert(code);
                }
            }
        }
        let mut languages: Vec<String> = lang_set.into_iter().collect();
        languages.sort();
        let (has_english, has_turkish) = compute_lang_flags(&languages);

        let mut tags_vec = Vec::new();
        if let Some(genres) = item.get("genres").and_then(|v| v.as_array()) {
            for g in genres {
                if let Some(s) = g.as_str() {
                    tags_vec.push(s.to_string());
                }
            }
        }

        let rating = item
            .get("averageUserRating")
            .and_then(|v| v.as_f64())
            .map(|r| (r / 5.0) * 100.0);

        let release_date = item
            .get("releaseDate")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(10).collect());

        results.push(GameResult {
            id: format!("appstore-{}", track_id),
            title,
            description: desc,
            url,
            image_url,
            store: "App Store".to_string(),
            store_category: "appstore".to_string(),
            platforms: vec!["Mobile".to_string(), "iOS".to_string()],
            tags: tags_vec,
            languages,
            has_english,
            has_turkish,
            is_free,
            price_usd: price,
            original_price_usd: None,
            price_display,
            discount_percent: None,
            rating,
            release_date,
            source_name: "Apple App Store API".to_string(),
            is_niche_or_news: false,
            relevance_score: 0.0,
        });
    }

    Ok(results)
}
