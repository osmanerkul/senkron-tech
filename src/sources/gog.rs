use crate::models::{compute_lang_flags, normalize_language, GameResult};
use reqwest::Client;
use serde_json::Value;
use std::collections::HashSet;

pub async fn search_gog(client: &Client, query: &str, tags: &[String]) -> Result<Vec<GameResult>, String> {
    let term = if !query.trim().is_empty() {
        query.trim().to_string()
    } else if !tags.is_empty() {
        tags.join(" ")
    } else {
        "rpg".to_string()
    };

    let url = format!(
        "https://catalog.gog.com/v1/catalog?limit=15&query=like:{}&productType=in:game,pack&locale=en-US&countryCode=US&currencyCode=USD",
        urlencoding::encode(&term)
    );

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("GOG istek hatası: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("GOG HTTP {}", resp.status()));
    }

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("GOG JSON hatası: {}", e))?;

    let products = json
        .get("products")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut results = Vec::new();
    for prod in products {
        let id = prod
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("0")
            .to_string();
        let title = prod
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Bilinmeyen GOG Oyunu")
            .to_string();
        let slug = prod
            .get("slug")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let store_url = prod
            .get("storeLink")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("https://www.gog.com/en/game/{}", slug));

        let image_url = prod
            .get("coverHorizontal")
            .or_else(|| prod.get("coverVertical"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut platforms = vec!["PC".to_string()];
        if let Some(oses) = prod.get("operatingSystems").and_then(|v| v.as_array()) {
            for os in oses {
                if let Some(s) = os.as_str() {
                    match s.to_lowercase().as_str() {
                        "windows" => platforms.push("Windows".to_string()),
                        "osx" | "macos" => platforms.push("macOS".to_string()),
                        "linux" => platforms.push("Linux".to_string()),
                        _ => {}
                    }
                }
            }
        }

        let mut tags_vec = Vec::new();
        if let Some(genres) = prod.get("genres").and_then(|v| v.as_array()) {
            for g in genres {
                if let Some(name) = g.get("name").and_then(|v| v.as_str()) {
                    tags_vec.push(name.to_string());
                }
            }
        }
        if let Some(t_arr) = prod.get("tags").and_then(|v| v.as_array()) {
            for t in t_arr.iter().take(6) {
                if let Some(name) = t.get("name").and_then(|v| v.as_str()) {
                    tags_vec.push(name.to_string());
                }
            }
        }
        if let Some(features) = prod.get("features").and_then(|v| v.as_array()) {
            for f in features.iter().take(4) {
                if let Some(name) = f.get("name").and_then(|v| v.as_str()) {
                    tags_vec.push(name.to_string());
                }
            }
        }

        // Price
        let price_obj = prod.get("price");
        let final_str = price_obj
            .and_then(|p| p.get("final"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let base_str = price_obj
            .and_then(|p| p.get("base"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let discount_str = price_obj
            .and_then(|p| p.get("discount"))
            .and_then(|v| v.as_str());

        let parse_money = |s: &str| -> Option<f64> {
            let cleaned: String = s.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
            cleaned.parse::<f64>().ok()
        };

        let price_usd = parse_money(final_str);
        let orig_price_usd = parse_money(base_str);
        let is_free = final_str.to_lowercase().contains("free")
            || price_usd == Some(0.0)
            || prod.get("productState").and_then(|v| v.as_str()) == Some("free");

        let discount_percent = discount_str.and_then(|ds| {
            let digits: String = ds.chars().filter(|c| c.is_ascii_digit()).collect();
            digits.parse::<u32>().ok().filter(|&n| n > 0)
        });

        let price_display = if is_free {
            "Ücretsiz".to_string()
        } else if !final_str.is_empty() {
            final_str.to_string()
        } else {
            "Fiyat Belirtilmemiş".to_string()
        };

        // Languages if present in catalog payload
        let mut lang_set = HashSet::new();
        if let Some(langs) = prod.get("languages").and_then(|v| v.as_array()) {
            for l in langs {
                if let Some(s) = l.as_str() {
                    if let Some(code) = normalize_language(s) {
                        lang_set.insert(code);
                    }
                }
            }
        }
        // GOG catalog localized in en-US almost always has EN if not listed, leave accurate if empty
        let mut languages: Vec<String> = lang_set.into_iter().collect();
        languages.sort();
        let (has_english, has_turkish) = compute_lang_flags(&languages);

        let rating = prod
            .get("reviewsRating")
            .and_then(|v| v.as_f64())
            .map(|r| (r / 50.0) * 100.0); // GOG rating is out of 50

        let release_date = prod
            .get("releaseDate")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let description = if tags_vec.is_empty() {
            "DRM-Free PC oyunu (GOG)".to_string()
        } else {
            format!("DRM-Free • {}", tags_vec.iter().take(5).cloned().collect::<Vec<_>>().join(", "))
        };

        results.push(GameResult {
            id: format!("gog-{}", id),
            title,
            description,
            url: store_url,
            image_url,
            store: "GOG".to_string(),
            store_category: "gog".to_string(),
            platforms,
            tags: tags_vec,
            languages,
            has_english,
            has_turkish,
            is_free,
            price_usd,
            original_price_usd: orig_price_usd,
            price_display,
            discount_percent,
            rating,
            release_date,
            source_name: "GOG Catalog API".to_string(),
            is_niche_or_news: false,
            relevance_score: 0.0,
        });
    }

    Ok(results)
}
