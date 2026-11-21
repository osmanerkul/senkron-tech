use crate::models::{compute_lang_flags, normalize_language, GameResult};
use futures::future::join_all;
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Map common user tags / keywords to official Steam Tag IDs
fn resolve_steam_tag_ids(terms: &[String]) -> Vec<u32> {
    let lowers: Vec<String> = terms.iter().map(|t| t.trim().to_lowercase()).collect();
    let mut ids = Vec::new();

    // Special combo: survival + craft -> Open World Survival Craft (1100689) gives the best real results (Once Human, Rust, Palworld, Valheim, Subnautica...)
    let has_survival = lowers.iter().any(|s| s == "survival" || s == "hayatta kalma");
    let has_craft = lowers.iter().any(|s| s == "craft" || s == "crafting" || s == "üretim");
    if has_survival && has_craft {
        ids.push(1100689); // Open World Survival Craft
    }

    for s in &lowers {
        let mapped: Option<u32> = match s.as_str() {
            "open world survival craft" => Some(1100689),
            "survival" | "hayatta kalma" => if has_craft { None } else { Some(1662) },
            "craft" | "crafting" | "üretim" => if has_survival { None } else { Some(1702) },
            "open world" | "open-world" | "açık dünya" => Some(1695),
            "roguelike" | "rogue-like" => Some(1716),
            "roguelite" | "rogue-lite" => Some(3959),
            "rpg" => Some(122),
            "jrpg" => Some(4434),
            "action" | "aksiyon" => Some(19),
            "adventure" | "macera" => Some(21),
            "indie" | "bağımsız" => Some(492),
            "co-op" | "coop" => Some(1685),
            "multiplayer" | "çok oyunculu" => Some(3859),
            "singleplayer" | "single-player" | "tek oyunculu" => Some(4182),
            "horror" | "korku" => Some(1667),
            "strategy" | "strateji" => Some(9),
            "puzzle" | "bulmaca" => Some(1664),
            "sandbox" => Some(3810),
            "building" | "base building" => Some(7332),
            "pixel" | "pixel-art" | "pixel art" | "pixel graphics" => Some(3964),
            "anime" => Some(4085),
            "metroidvania" => Some(1628),
            "zombie" | "zombies" => Some(1659),
            "shooter" => Some(1774),
            "fps" => Some(1663),
            "mmo" => Some(128),
            "mmorpg" => Some(1754),
            "simulation" | "simülasyon" => Some(599),
            "story rich" | "story-rich" | "hikaye" => Some(1742),
            "soulslike" | "souls-like" => Some(29482),
            "visual novel" => Some(3799),
            "deckbuilder" | "card" => Some(1091588),
            "sci-fi" | "scifi" | "bilim kurgu" => Some(3942),
            "fantasy" | "fantastik" => Some(1684),
            "post-apocalyptic" => Some(3835),
            "cyberpunk" => Some(4115),
            "turn-based" => Some(1677),
            "tower defense" => Some(1645),
            "city builder" => Some(4328),
            "battle royale" => Some(176981),
            "racing" | "yarış" => Some(699),
            "fighting" | "dövüş" => Some(1743),
            "stealth" | "gizlilik" => Some(1687),
            "vr" => Some(21978),
            "platformer" => Some(1625),
            "free" | "free to play" | "ücretsiz" => Some(113),
            _ => None,
        };
        if let Some(id) = mapped {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

pub async fn search_steam(client: &Client, query: &str, tags: &[String]) -> Result<Vec<GameResult>, String> {
    // Collect tag candidates from both `tags` and comma/space-split `query`
    let mut candidate_terms: Vec<String> = tags.to_vec();
    for part in query.split(',') {
        let trimmed = part.trim();
        if !trimmed.is_empty() && !candidate_terms.iter().any(|t| t.eq_ignore_ascii_case(trimmed)) {
            candidate_terms.push(trimmed.to_string());
        }
    }

    let tag_ids = resolve_steam_tag_ids(&candidate_terms);
    let mut combined: Vec<GameResult> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();

    // 1. If we resolved real Steam Tag IDs, query Steam's real Tag Search endpoint!
    // This finds games like "Once Human", "Valheim", "Rust", "Palworld" even when the tag words aren't in the title.
    if !tag_ids.is_empty() {
        if let Ok(tag_results) = search_steam_by_tags(client, &tag_ids, &candidate_terms).await {
            for g in tag_results {
                if seen_ids.insert(g.id.clone()) {
                    combined.push(g);
                }
            }
        }
    }

    // 2. Also run title search when the user typed a game title (not purely tags) or when tag search returned nothing
    let q_clean = query.replace(',', " ").trim().to_string();
    let is_pure_tag_query = !tag_ids.is_empty()
        && candidate_terms.iter().all(|t| !resolve_steam_tag_ids(&[t.clone()]).is_empty());

    if (!q_clean.is_empty() && !is_pure_tag_query) || combined.is_empty() {
        let term = if !q_clean.is_empty() {
            q_clean
        } else if !tags.is_empty() {
            tags.join(" ")
        } else {
            "indie".to_string()
        };
        if let Ok(title_results) = search_steam_by_title(client, &term).await {
            for g in title_results {
                if seen_ids.insert(g.id.clone()) {
                    combined.push(g);
                }
            }
        }
    }

    Ok(combined)
}

async fn search_steam_by_tags(
    client: &Client,
    tag_ids: &[u32],
    tag_labels: &[String],
) -> Result<Vec<GameResult>, String> {
    let tags_param = tag_ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let url = format!(
        "https://store.steampowered.com/search/results/?tags={}&count=25&l=english&cc=US&infinite=1",
        tags_param
    );

    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(7))
        .send()
        .await
        .map_err(|e| format!("Steam Tag Search hatası: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Steam Tag Search HTTP {}", resp.status()));
    }

    let json: Value = resp.json().await.map_err(|e| e.to_string())?;
    let html_str = json
        .get("results_html")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    struct RawRow {
        app_id: u64,
        title: String,
        url: String,
        image_url: Option<String>,
        platforms: Vec<String>,
        price_final_str: String,
        price_orig_str: String,
        discount_pct: Option<u32>,
        release_date: Option<String>,
    }

    let rows = {
        let doc = Html::parse_document(html_str);
        let row_sel = Selector::parse("a.search_result_row").unwrap();
        let title_sel = Selector::parse("span.title").unwrap();
        let img_sel = Selector::parse("div.search_capsule img").unwrap();
        let price_final_sel = Selector::parse("div.discount_final_price").unwrap();
        let price_orig_sel = Selector::parse("div.discount_original_price").unwrap();
        let discount_sel = Selector::parse("div.discount_pct").unwrap();
        let rel_sel = Selector::parse("div.search_released").unwrap();
        let win_sel = Selector::parse("span.win").unwrap();
        let mac_sel = Selector::parse("span.mac").unwrap();
        let lin_sel = Selector::parse("span.linux").unwrap();

        let mut rows = Vec::new();
        for el in doc.select(&row_sel) {
            if rows.len() >= 18 {
                break;
            }
            let app_id = el
                .value()
                .attr("data-ds-appid")
                .and_then(|s| s.split(',').next())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            if app_id == 0 {
                continue;
            }
            let title = el
                .select(&title_sel)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }

            let mut platforms = vec!["PC".to_string()];
            if el.select(&win_sel).next().is_some() {
                platforms.push("Windows".to_string());
            }
            if el.select(&mac_sel).next().is_some() {
                platforms.push("macOS".to_string());
            }
            if el.select(&lin_sel).next().is_some() {
                platforms.push("Linux".to_string());
            }

            let image_url = Some(format!(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{}/header.jpg",
                app_id
            ))
            .or_else(|| {
                el.select(&img_sel)
                    .next()
                    .and_then(|i| i.value().attr("src"))
                    .map(|s| s.to_string())
            });

            let price_final_str = el
                .select(&price_final_sel)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();
            let price_orig_str = el
                .select(&price_orig_sel)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .unwrap_or_default();
            let discount_pct = el
                .select(&discount_sel)
                .next()
                .and_then(|e| {
                    let d: String = e.text().collect::<String>().chars().filter(|c| c.is_ascii_digit()).collect();
                    d.parse::<u32>().ok().filter(|&v| v > 0)
                });
            let release_date = el
                .select(&rel_sel)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .filter(|s| !s.is_empty());

            rows.push(RawRow {
                app_id,
                title,
                url: format!("https://store.steampowered.com/app/{}/", app_id),
                image_url,
                platforms,
                price_final_str,
                price_orig_str,
                discount_pct,
                release_date,
            });
        }
        rows
    };

    // Enrich top 8 with appdetails concurrently for exact languages & descriptions
    let futures = rows.into_iter().enumerate().map(|(idx, row)| {
        let client = client.clone();
        let extra_tags = tag_labels.to_vec();
        async move {
            let parse_usd = |s: &str| -> Option<f64> {
                let cleaned: String = s.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
                cleaned.parse::<f64>().ok()
            };

            let mut is_free = row.price_final_str.to_lowercase().contains("free")
                || row.price_final_str == "$0.00";
            let mut price_usd = if is_free {
                Some(0.0)
            } else {
                parse_usd(&row.price_final_str)
            };
            let orig_price_usd = parse_usd(&row.price_orig_str);
            let mut description = format!("Steam Etiket Eşleşmesi: {}", extra_tags.join(", "));
            let mut tags_vec = extra_tags;
            let mut languages = Vec::new();
            let mut rating = None;

            if idx < 8 {
                if let Ok(detail) = fetch_steam_appdetails(&client, row.app_id).await {
                    if let Some(d) = detail.get("short_description").and_then(|v| v.as_str()) {
                        description = strip_html_tags(d);
                    }
                    if let Some(free) = detail.get("is_free").and_then(|v| v.as_bool()) {
                        is_free = free;
                        if free {
                            price_usd = Some(0.0);
                        }
                    }
                    if let Some(genres) = detail.get("genres").and_then(|v| v.as_array()) {
                        for g in genres {
                            if let Some(desc) = g.get("description").and_then(|v| v.as_str()) {
                                if !tags_vec.iter().any(|t| t.eq_ignore_ascii_case(desc)) {
                                    tags_vec.push(desc.to_string());
                                }
                            }
                        }
                    }
                    if let Some(lang_str) = detail.get("supported_languages").and_then(|v| v.as_str()) {
                        languages = parse_steam_languages(lang_str);
                    }
                    rating = detail
                        .get("metacritic")
                        .and_then(|m| m.get("score"))
                        .and_then(|v| v.as_f64());
                }
            }

            let price_display = if is_free {
                "Ücretsiz".to_string()
            } else if !row.price_final_str.is_empty() {
                row.price_final_str
            } else if let Some(p) = price_usd {
                format!("${:.2}", p)
            } else {
                "Fiyat Belirtilmemiş".to_string()
            };

            let (has_english, has_turkish) = compute_lang_flags(&languages);

            GameResult {
                id: format!("steam-{}", row.app_id),
                title: row.title,
                description,
                url: row.url,
                image_url: row.image_url,
                store: "Steam".to_string(),
                store_category: "steam".to_string(),
                platforms: row.platforms,
                tags: tags_vec,
                languages,
                has_english,
                has_turkish,
                is_free,
                price_usd,
                original_price_usd: orig_price_usd,
                price_display,
                discount_percent: row.discount_pct,
                rating,
                release_date: row.release_date,
                source_name: "Steam Tag Search".to_string(),
                is_niche_or_news: false,
                relevance_score: 0.0,
            }
        }
    });

    Ok(join_all(futures).await)
}

async fn search_steam_by_title(client: &Client, term: &str) -> Result<Vec<GameResult>, String> {
    let url = format!(
        "https://store.steampowered.com/api/storesearch/?term={}&l=english&cc=US",
        urlencoding::encode(term)
    );

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Steam istek hatası: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Steam HTTP {}", resp.status()));
    }

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("Steam JSON hatası: {}", e))?;

    let items = json
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let limited: Vec<Value> = items.into_iter().take(12).collect();

    let futures = limited.into_iter().enumerate().map(|(idx, item)| {
        let client = client.clone();
        async move {
            let app_id = item.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let name = item
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Bilinmeyen Oyun")
                .to_string();

            let tiny_image = item
                .get("tiny_image")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let mut platforms = vec!["PC".to_string()];
            if let Some(p) = item.get("platforms") {
                if p.get("windows").and_then(|v| v.as_bool()).unwrap_or(false) {
                    platforms.push("Windows".to_string());
                }
                if p.get("mac").and_then(|v| v.as_bool()).unwrap_or(false) {
                    platforms.push("macOS".to_string());
                }
                if p.get("linux").and_then(|v| v.as_bool()).unwrap_or(false) {
                    platforms.push("Linux".to_string());
                }
            }

            let price_obj = item.get("price");
            let mut price_usd = price_obj
                .and_then(|p| p.get("final"))
                .and_then(|v| v.as_f64())
                .map(|cents| cents / 100.0);
            let mut orig_price_usd = price_obj
                .and_then(|p| p.get("initial"))
                .and_then(|v| v.as_f64())
                .map(|cents| cents / 100.0);

            let mut is_free = price_obj.is_none() || price_usd == Some(0.0);
            let mut discount = None;
            if let (Some(orig), Some(fin)) = (orig_price_usd, price_usd) {
                if orig > fin && orig > 0.0 {
                    discount = Some((((orig - fin) / orig) * 100.0).round() as u32);
                }
            }

            let mut description = String::new();
            let mut tags_vec: Vec<String> = Vec::new();
            let mut languages: Vec<String> = Vec::new();
            let mut release_date: Option<String> = None;
            let mut header_image = tiny_image;
            let mut rating: Option<f64> = item
                .get("metascore")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok());

            if idx < 8 && app_id > 0 {
                if let Ok(detail) = fetch_steam_appdetails(&client, app_id).await {
                    if let Some(d) = detail.get("short_description").and_then(|v| v.as_str()) {
                        description = strip_html_tags(d);
                    }
                    if let Some(img) = detail.get("header_image").and_then(|v| v.as_str()) {
                        header_image = Some(img.to_string());
                    }
                    if let Some(free) = detail.get("is_free").and_then(|v| v.as_bool()) {
                        is_free = free;
                        if free {
                            price_usd = Some(0.0);
                        }
                    }
                    if let Some(po) = detail.get("price_overview") {
                        if let Some(fin) = po.get("final").and_then(|v| v.as_f64()) {
                            price_usd = Some(fin / 100.0);
                            is_free = fin == 0.0;
                        }
                        if let Some(init) = po.get("initial").and_then(|v| v.as_f64()) {
                            orig_price_usd = Some(init / 100.0);
                        }
                        if let Some(dp) = po.get("discount_percent").and_then(|v| v.as_u64()) {
                            if dp > 0 {
                                discount = Some(dp as u32);
                            }
                        }
                    }
                    if let Some(genres) = detail.get("genres").and_then(|v| v.as_array()) {
                        for g in genres {
                            if let Some(desc) = g.get("description").and_then(|v| v.as_str()) {
                                tags_vec.push(desc.to_string());
                            }
                        }
                    }
                    if let Some(cats) = detail.get("categories").and_then(|v| v.as_array()) {
                        for c in cats.iter().take(6) {
                            if let Some(desc) = c.get("description").and_then(|v| v.as_str()) {
                                tags_vec.push(desc.to_string());
                            }
                        }
                    }
                    if let Some(lang_str) = detail.get("supported_languages").and_then(|v| v.as_str()) {
                        languages = parse_steam_languages(lang_str);
                    }
                    if let Some(rd) = detail.get("release_date").and_then(|v| v.get("date")).and_then(|v| v.as_str()) {
                        if !rd.is_empty() {
                            release_date = Some(rd.to_string());
                        }
                    }
                    if rating.is_none() {
                        rating = detail
                            .get("metacritic")
                            .and_then(|m| m.get("score"))
                            .and_then(|v| v.as_f64());
                    }
                }
            }

            let price_display = if is_free {
                "Ücretsiz".to_string()
            } else if let Some(p) = price_usd {
                format!("${:.2}", p)
            } else {
                "Fiyat Belirtilmemiş".to_string()
            };

            let (has_english, has_turkish) = compute_lang_flags(&languages);

            GameResult {
                id: format!("steam-{}", app_id),
                title: name,
                description,
                url: format!("https://store.steampowered.com/app/{}/", app_id),
                image_url: header_image,
                store: "Steam".to_string(),
                store_category: "steam".to_string(),
                platforms,
                tags: tags_vec,
                languages,
                has_english,
                has_turkish,
                is_free,
                price_usd,
                original_price_usd: orig_price_usd,
                price_display,
                discount_percent: discount,
                rating,
                release_date,
                source_name: "Steam Store API".to_string(),
                is_niche_or_news: false,
                relevance_score: 0.0,
            }
        }
    });

    Ok(join_all(futures).await)
}

async fn fetch_steam_appdetails(client: &Client, app_id: u64) -> Result<Value, String> {
    let url = format!(
        "https://store.steampowered.com/api/appdetails?appids={}&l=english&cc=US",
        app_id
    );
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let json: Value = resp.json().await.map_err(|e| e.to_string())?;
    let entry = json
        .get(app_id.to_string())
        .ok_or_else(|| "missing appid".to_string())?;
    if entry.get("success").and_then(|v| v.as_bool()) != Some(true) {
        return Err("success=false".to_string());
    }
    entry
        .get("data")
        .cloned()
        .ok_or_else(|| "missing data".to_string())
}

fn parse_steam_languages(raw: &str) -> Vec<String> {
    let cleaned = strip_html_tags(raw);
    let mut set = HashSet::new();
    for part in cleaned.split(',') {
        let piece = part.replace('*', "").trim().to_string();
        if let Some(code) = normalize_language(&piece) {
            set.insert(code);
        }
    }
    let mut list: Vec<String> = set.into_iter().collect();
    list.sort();
    list
}

fn strip_html_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut inside = false;
    for ch in input.chars() {
        match ch {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(ch),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .trim()
        .to_string()
}

#[allow(dead_code)]
fn _unused_map_guard(_: HashMap<String, String>) {}
