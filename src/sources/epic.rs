use crate::models::GameResult;
use reqwest::Client;
use serde_json::Value;

pub async fn search_epic(client: &Client, query: &str, tags: &[String]) -> Result<Vec<GameResult>, String> {
    let mut results = Vec::new();
    let q_lower = query.trim().to_lowercase();
    let tag_lowers: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();

    // 1. Epic Games Free Promotions API (always live, no key)
    let promo_url = "https://store-site-backend-static.ak.epicgames.com/freeGamesPromotions?locale=en-US&country=US&allowCountries=US";
    if let Ok(resp) = client.get(promo_url).send().await {
        if resp.status().is_success() {
            if let Ok(json) = resp.json::<Value>().await {
                if let Some(elements) = json
                    .pointer("/data/Catalog/searchStore/elements")
                    .and_then(|v| v.as_array())
                {
                    for el in elements {
                        let title = el.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        if title.is_empty() || title.to_lowercase().contains("mystery game") {
                            continue;
                        }
                        let desc = el.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let hay = format!("{} {}", title.to_lowercase(), desc.to_lowercase());

                        // Filter if user typed a query
                        if !q_lower.is_empty() && !hay.contains(&q_lower) {
                            continue;
                        }
                        if !tag_lowers.is_empty() && !tag_lowers.iter().any(|t| hay.contains(t)) && !q_lower.is_empty() {
                            continue;
                        }

                        let slug = el
                            .get("productSlug")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                            .or_else(|| {
                                el.pointer("/catalogNs/mappings/0/pageSlug")
                                    .and_then(|v| v.as_str())
                            })
                            .unwrap_or("");

                        let url = if !slug.is_empty() {
                            format!("https://store.epicgames.com/en-US/p/{}", slug)
                        } else {
                            format!(
                                "https://store.epicgames.com/en-US/browse?q={}",
                                urlencoding::encode(&title)
                            )
                        };

                        let image_url = el
                            .get("keyImages")
                            .and_then(|v| v.as_array())
                            .and_then(|arr| {
                                arr.iter()
                                    .find(|img| {
                                        matches!(
                                            img.get("type").and_then(|t| t.as_str()),
                                            Some("OfferImageWide") | Some("DieselStoreFrontWide") | Some("Thumbnail")
                                        )
                                    })
                                    .or_else(|| arr.first())
                            })
                            .and_then(|img| img.get("url"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());

                        let discount_price = el
                            .pointer("/price/totalPrice/discountPrice")
                            .and_then(|v| v.as_f64())
                            .map(|c| c / 100.0);
                        let orig_price = el
                            .pointer("/price/totalPrice/originalPrice")
                            .and_then(|v| v.as_f64())
                            .map(|c| c / 100.0);

                        let is_free = discount_price == Some(0.0);
                        let discount = match (orig_price, discount_price) {
                            (Some(o), Some(d)) if o > d && o > 0.0 => {
                                Some((((o - d) / o) * 100.0).round() as u32)
                            }
                            _ => None,
                        };

                        let price_display = if is_free {
                            "Ücretsiz (Epic Promosyon)".to_string()
                        } else if let Some(p) = discount_price {
                            format!("${:.2}", p)
                        } else {
                            "Epic Games Store".to_string()
                        };

                        results.push(GameResult {
                            id: format!("epic-promo-{}", slug),
                            title,
                            description: desc,
                            url,
                            image_url,
                            store: "Epic Games".to_string(),
                            store_category: "epic".to_string(),
                            platforms: vec!["PC".to_string(), "Windows".to_string()],
                            tags: vec!["Epic Games".to_string(), if is_free { "Ücretsiz".to_string() } else { "Promosyon".to_string() }],
                            languages: vec!["EN".to_string()],
                            has_english: Some(true),
                            has_turkish: None,
                            is_free,
                            price_usd: discount_price,
                            original_price_usd: orig_price,
                            price_display,
                            discount_percent: discount,
                            rating: None,
                            release_date: el.get("effectiveDate").and_then(|v| v.as_str()).map(|s| s.chars().take(10).collect()),
                            source_name: "Epic Games Store".to_string(),
                            is_niche_or_news: false,
                            relevance_score: 0.0,
                        });
                    }
                }
            }
        }
    }

    // 2. Also query Epic Games Store specifically via CheapShark storeID=25 when user typed a query
    if !q_lower.is_empty() {
        let cs_epic = format!(
            "https://www.cheapshark.com/api/1.0/deals?storeID=25&title={}&pageSize=10",
            urlencoding::encode(query.trim())
        );
        if let Ok(resp) = client.get(&cs_epic).send().await {
            if let Ok(items) = resp.json::<Vec<Value>>().await {
                for item in items {
                    let deal_id = item.get("dealID").and_then(|v| v.as_str()).unwrap_or("");
                    let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if title.is_empty() {
                        continue;
                    }
                    let sale_price = item.get("salePrice").and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok());
                    let normal_price = item.get("normalPrice").and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok());
                    let savings = item
                        .get("savings")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse::<f64>().ok())
                        .map(|f| f.round() as u32)
                        .filter(|&d| d > 0);
                    let is_free = sale_price == Some(0.0);

                    results.push(GameResult {
                        id: format!("epic-cs-{}", deal_id),
                        title,
                        description: "Epic Games Store PC oyunu".to_string(),
                        url: format!("https://www.cheapshark.com/redirect?dealID={}", deal_id),
                        image_url: item.get("thumb").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        store: "Epic Games".to_string(),
                        store_category: "epic".to_string(),
                        platforms: vec!["PC".to_string(), "Windows".to_string()],
                        tags: vec!["Epic Games".to_string(), "PC".to_string()],
                        languages: vec![],
                        has_english: None,
                        has_turkish: None,
                        is_free,
                        price_usd: sale_price,
                        original_price_usd: normal_price,
                        price_display: if is_free {
                            "Ücretsiz".to_string()
                        } else if let Some(p) = sale_price {
                            format!("${:.2}", p)
                        } else {
                            "Epic Games".to_string()
                        },
                        discount_percent: savings,
                        rating: item
                            .get("metacriticScore")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                            .filter(|&r| r > 0.0),
                        release_date: None,
                        source_name: "Epic Games Store".to_string(),
                        is_niche_or_news: false,
                        relevance_score: 0.0,
                    });
                }
            }
        }
    }

    Ok(results)
}
