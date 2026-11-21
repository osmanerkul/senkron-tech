use crate::models::{compute_lang_flags, normalize_language, GameResult};
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::{json, Value};
use std::collections::HashSet;

/// 1. VNDB (Visual Novel Database) — Official Kana API v2 (No API key required!)
/// Incredible for checking exact English / Turkish / Japanese availability on niche & indie titles.
pub async fn search_vndb(client: &Client, query: &str, tags: &[String]) -> Result<Vec<GameResult>, String> {
    let term = if !query.trim().is_empty() {
        query.trim().to_string()
    } else if !tags.is_empty() {
        tags.join(" ")
    } else {
        "mystery".to_string()
    };

    let body = json!({
        "filters": ["search", "=", term],
        "fields": "id, title, alttitle, description, image.url, languages, platforms, released, rating, tags.name",
        "results": 12
    });

    let resp = client
        .post("https://api.vndb.org/kana/vn")
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(7))
        .send()
        .await
        .map_err(|e| format!("VNDB istek hatası: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("VNDB HTTP {}", resp.status()));
    }

    let json: Value = resp.json().await.map_err(|e| format!("VNDB JSON hatası: {}", e))?;
    let items = json
        .get("results")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut results = Vec::new();
    for item in items {
        let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("v0");
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Bilinmeyen VN")
            .to_string();
        let raw_desc = item
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("VNDB Visual Novel / Hikaye Odaklı Oyun");
        let description = clean_bbcode(raw_desc).chars().take(240).collect();

        let image_url = item
            .pointer("/image/url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut lang_set = HashSet::new();
        if let Some(langs) = item.get("languages").and_then(|v| v.as_array()) {
            for l in langs {
                if let Some(code) = l.as_str().and_then(normalize_language) {
                    lang_set.insert(code);
                }
            }
        }
        let mut languages: Vec<String> = lang_set.into_iter().collect();
        languages.sort();
        let (has_english, has_turkish) = compute_lang_flags(&languages);

        let mut platforms = Vec::new();
        if let Some(plats) = item.get("platforms").and_then(|v| v.as_array()) {
            for p in plats {
                match p.as_str().unwrap_or("") {
                    "win" => {
                        push_unique(&mut platforms, "PC");
                        push_unique(&mut platforms, "Windows");
                    }
                    "mac" => {
                        push_unique(&mut platforms, "PC");
                        push_unique(&mut platforms, "macOS");
                    }
                    "lin" => {
                        push_unique(&mut platforms, "PC");
                        push_unique(&mut platforms, "Linux");
                    }
                    "and" => {
                        push_unique(&mut platforms, "Mobile");
                        push_unique(&mut platforms, "Android");
                    }
                    "ios" => {
                        push_unique(&mut platforms, "Mobile");
                        push_unique(&mut platforms, "iOS");
                    }
                    "web" => push_unique(&mut platforms, "Web"),
                    "swi" => {
                        push_unique(&mut platforms, "Konsol");
                        push_unique(&mut platforms, "Switch");
                    }
                    "ps4" | "ps5" | "ps3" | "psp" | "psv" => {
                        push_unique(&mut platforms, "Konsol");
                        push_unique(&mut platforms, "PlayStation");
                    }
                    "xbo" | "xbs" => {
                        push_unique(&mut platforms, "Konsol");
                        push_unique(&mut platforms, "Xbox");
                    }
                    _ => {}
                }
            }
        }
        if platforms.is_empty() {
            platforms.push("PC".to_string());
        }

        let mut tags_vec = vec!["Visual Novel".to_string(), "VNDB".to_string()];
        if let Some(t_arr) = item.get("tags").and_then(|v| v.as_array()) {
            for t in t_arr.iter().take(5) {
                if let Some(name) = t.get("name").and_then(|v| v.as_str()) {
                    tags_vec.push(name.to_string());
                }
            }
        }

        let rating = item.get("rating").and_then(|v| v.as_f64());
        let release_date = item
            .get("released")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        results.push(GameResult {
            id: format!("vndb-{}", id),
            title,
            description,
            url: format!("https://vndb.org/{}", id),
            image_url,
            store: "VNDB".to_string(),
            store_category: "vndb".to_string(),
            platforms,
            tags: tags_vec,
            languages,
            has_english,
            has_turkish,
            is_free: false,
            price_usd: None,
            original_price_usd: None,
            price_display: "VNDB'de Gör".to_string(),
            discount_percent: None,
            rating,
            release_date,
            source_name: "VNDB API".to_string(),
            is_niche_or_news: true,
            relevance_score: 0.0,
        });
    }

    Ok(results)
}

/// 2. GameJolt — Indie / Experimental / Freeware platform JSON API
pub async fn search_gamejolt(client: &Client, query: &str, tags: &[String]) -> Result<Vec<GameResult>, String> {
    let term = if !query.trim().is_empty() {
        query.trim().to_string()
    } else if !tags.is_empty() {
        tags.join(" ")
    } else {
        "indie".to_string()
    };

    let url = format!(
        "https://gamejolt.com/site-api/web/search/games?q={}&page=1",
        urlencoding::encode(&term)
    );

    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(7))
        .send()
        .await
        .map_err(|e| format!("GameJolt istek hatası: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("GameJolt HTTP {}", resp.status()));
    }

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("GameJolt JSON hatası: {}", e))?;

    let games = json
        .pointer("/payload/games")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut results = Vec::new();
    for g in games.into_iter().take(12) {
        let id = g.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let title = g
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("GameJolt Oyunu")
            .to_string();
        let slug = g.get("slug").and_then(|v| v.as_str()).unwrap_or("game");
        let url = format!("https://gamejolt.com/games/{}/{}", slug, id);

        let image_url = g
            .pointer("/thumbnail_media_item/mediaserver_url")
            .or_else(|| g.pointer("/header_media_item/mediaserver_url"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let category = g
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("Indie")
            .to_string();

        let dev_name = g
            .pointer("/developer/display_name")
            .and_then(|v| v.as_str())
            .unwrap_or("Bağımsız Geliştirici");

        let mut platforms = Vec::new();
        if let Some(compat) = g.get("compatibility") {
            if compat.get("os_windows").and_then(|v| v.as_bool()).unwrap_or(false) {
                push_unique(&mut platforms, "PC");
                push_unique(&mut platforms, "Windows");
            }
            if compat.get("os_mac").and_then(|v| v.as_bool()).unwrap_or(false) {
                push_unique(&mut platforms, "PC");
                push_unique(&mut platforms, "macOS");
            }
            if compat.get("os_linux").and_then(|v| v.as_bool()).unwrap_or(false) {
                push_unique(&mut platforms, "PC");
                push_unique(&mut platforms, "Linux");
            }
            if compat.get("type_html").and_then(|v| v.as_bool()).unwrap_or(false) {
                push_unique(&mut platforms, "Web");
            }
            if compat.get("os_other").and_then(|v| v.as_bool()).unwrap_or(false) && platforms.is_empty() {
                push_unique(&mut platforms, "PC");
            }
        }
        if platforms.is_empty() {
            platforms.push("PC".to_string());
        }

        let sellable = g.get("sellable");
        let is_free = sellable
            .and_then(|s| s.get("type"))
            .and_then(|v| v.as_str())
            .map(|t| t == "free" || t == "pwyw")
            .unwrap_or(true);
        let price_cents = sellable
            .and_then(|s| s.pointer("/pricings/0/amount"))
            .and_then(|v| v.as_f64());
        let price_usd = if is_free {
            Some(0.0)
        } else {
            price_cents.map(|c| c / 100.0)
        };
        let price_display = if is_free {
            "Ücretsiz".to_string()
        } else if let Some(p) = price_usd {
            format!("${:.2}", p)
        } else {
            "GameJolt".to_string()
        };

        let mut tags_vec = vec!["GameJolt".to_string(), "Indie".to_string()];
        if !category.is_empty() {
            tags_vec.push(category);
        }

        results.push(GameResult {
            id: format!("gj-{}", id),
            title,
            description: format!("Geliştirici: {} • GameJolt bağımsız oyunu", dev_name),
            url,
            image_url,
            store: "GameJolt".to_string(),
            store_category: "gamejolt".to_string(),
            platforms,
            tags: tags_vec,
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
            source_name: "GameJolt".to_string(),
            is_niche_or_news: true,
            relevance_score: 0.0,
        });
    }

    Ok(results)
}

/// 3. Internet Archive (Archive.org) — Obscure, Retro, Freeware & Browser-playable Games API
pub async fn search_archive_org(client: &Client, query: &str, tags: &[String]) -> Result<Vec<GameResult>, String> {
    let term = if !query.trim().is_empty() {
        query.trim().to_string()
    } else if !tags.is_empty() {
        tags.join(" ")
    } else {
        "indie game".to_string()
    };

    let q_param = format!(
        "mediatype:software AND (collection:softwarelibrary OR collection:classicpcgames OR collection:internetarcade OR subject:game) AND ({})",
        term
    );
    let url = format!(
        "https://archive.org/advancedsearch.php?q={}&fl[]=identifier,title,description,date,subject&rows=12&output=json",
        urlencoding::encode(&q_param)
    );

    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(7))
        .send()
        .await
        .map_err(|e| format!("Archive.org istek hatası: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Archive.org HTTP {}", resp.status()));
    }

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("Archive.org JSON hatası: {}", e))?;

    let docs = json
        .pointer("/response/docs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut results = Vec::new();
    for doc in docs {
        let ident = doc.get("identifier").and_then(|v| v.as_str()).unwrap_or("");
        let title = doc
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or(ident)
            .to_string();
        if ident.is_empty() || title.is_empty() {
            continue;
        }
        let desc = match doc.get("description") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(arr)) => arr
                .first()
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            _ => "Archive.org ücretsiz / tarayıcıda oynanabilir retro & kıyı kenar oyun".to_string(),
        };
        let release_date = doc
            .get("date")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(10).collect());

        let mut tags_vec = vec!["Archive.org".to_string(), "Kıyı Kenar".to_string(), "Ücretsiz".to_string()];
        match doc.get("subject") {
            Some(Value::String(s)) => tags_vec.push(s.clone()),
            Some(Value::Array(arr)) => {
                for s in arr.iter().take(3) {
                    if let Some(st) = s.as_str() {
                        tags_vec.push(st.to_string());
                    }
                }
            }
            _ => {}
        }

        results.push(GameResult {
            id: format!("archive-{}", ident),
            title,
            description: desc.chars().take(240).collect(),
            url: format!("https://archive.org/details/{}", ident),
            image_url: Some(format!("https://archive.org/services/img/{}", ident)),
            store: "Archive.org".to_string(),
            store_category: "archive".to_string(),
            platforms: vec!["PC".to_string(), "Web".to_string()],
            tags: tags_vec,
            languages: vec!["EN".to_string()],
            has_english: Some(true),
            has_turkish: None,
            is_free: true,
            price_usd: Some(0.0),
            original_price_usd: None,
            price_display: "Ücretsiz (Tarayıcı / İndir)".to_string(),
            discount_percent: None,
            rating: None,
            release_date,
            source_name: "Archive.org (Retro & Kıyı Kenar)".to_string(),
            is_niche_or_news: true,
            relevance_score: 0.0,
        });
    }

    Ok(results)
}

/// 4. TapTap — Global & Asian niche mobile (Android/iOS) web search scraper
pub async fn search_taptap(client: &Client, query: &str, tags: &[String]) -> Result<Vec<GameResult>, String> {
    let term = if !query.trim().is_empty() {
        query.trim().to_string()
    } else if !tags.is_empty() {
        tags.join(" ")
    } else {
        "rpg".to_string()
    };

    let url = format!(
        "https://www.taptap.io/search/{}",
        urlencoding::encode(&term)
    );

    let resp = client
        .get(&url)
        .header("Accept", "text/html,application/xhtml+xml")
        .header("Accept-Language", "en-US,en;q=0.9")
        .timeout(std::time::Duration::from_secs(7))
        .send()
        .await
        .map_err(|e| format!("TapTap istek hatası: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("TapTap HTTP {}", resp.status()));
    }

    let html_text = resp.text().await.map_err(|e| e.to_string())?;
    let doc = Html::parse_document(&html_text);
    let link_sel = Selector::parse("a[href*='/app/']").unwrap();
    let img_sel = Selector::parse("img").unwrap();

    let mut seen = HashSet::new();
    let mut results = Vec::new();
    for el in doc.select(&link_sel) {
        if results.len() >= 10 {
            break;
        }
        let href = el.value().attr("href").unwrap_or("");
        let app_id = href
            .split("/app/")
            .nth(1)
            .and_then(|s| s.split(&['/', '?', '#'][..]).next())
            .unwrap_or("");
        if app_id.is_empty() || !app_id.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if !seen.insert(app_id.to_string()) {
            continue;
        }
        let title = el
            .value()
            .attr("title")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| el.text().collect::<String>().trim().to_string());
        if title.len() < 2 {
            continue;
        }
        let image_url = el
            .select(&img_sel)
            .next()
            .and_then(|img| img.value().attr("src").or_else(|| img.value().attr("data-src")))
            .filter(|s| s.starts_with("http"))
            .map(|s| s.to_string());

        results.push(GameResult {
            id: format!("taptap-{}", app_id),
            title,
            description: "TapTap Global & Asya Mobil Oyun Keşfi".to_string(),
            url: format!("https://www.taptap.io/app/{}", app_id),
            image_url,
            store: "TapTap".to_string(),
            store_category: "taptap".to_string(),
            platforms: vec!["Mobile".to_string(), "Android".to_string(), "iOS".to_string()],
            tags: vec!["TapTap".to_string(), "Mobil".to_string(), "Kıyı Kenar".to_string()],
            languages: vec!["EN".to_string()],
            has_english: Some(true),
            has_turkish: None,
            is_free: true,
            price_usd: Some(0.0),
            original_price_usd: None,
            price_display: "Ücretsiz / TapTap".to_string(),
            discount_percent: None,
            rating: None,
            release_date: None,
            source_name: "TapTap (Mobil)".to_string(),
            is_niche_or_news: true,
            relevance_score: 0.0,
        });
    }

    Ok(results)
}

/// 5. SteamSpy + SteamDB — Tag-based Steam discovery with SteamDB links!
pub async fn search_steamspy_steamdb(client: &Client, query: &str, tags: &[String]) -> Result<Vec<GameResult>, String> {
    // SteamSpy shines when searching by tag (e.g. "Roguelike", "Indie", "Horror", "Metroidvania")
    let raw_tag = tags
        .first()
        .map(|s| s.as_str())
        .or_else(|| {
            let q = query.trim();
            if !q.is_empty() && !q.contains(' ') {
                Some(q)
            } else {
                None
            }
        })
        .unwrap_or("Indie");

    // Title-case the tag for SteamSpy (e.g. "roguelike" -> "Roguelike")
    let formatted_tag = title_case_tag(raw_tag);

    let url = format!(
        "https://steamspy.com/api.php?request=tag&tag={}",
        urlencoding::encode(&formatted_tag)
    );

    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(7))
        .send()
        .await
        .map_err(|e| format!("SteamSpy/SteamDB istek hatası: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("SteamSpy HTTP {}", resp.status()));
    }

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("SteamSpy JSON hatası: {}", e))?;

    let map = match json.as_object() {
        Some(m) => m,
        None => return Ok(vec![]),
    };

    let q_lower = query.trim().to_lowercase();
    let mut results = Vec::new();

    for (appid_str, entry) in map.iter() {
        let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        if !q_lower.is_empty()
            && !name.to_lowercase().contains(&q_lower)
            && !formatted_tag.to_lowercase().contains(&q_lower)
        {
            continue;
        }

        let pos = entry.get("positive").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let neg = entry.get("negative").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let total_rev = pos + neg;
        let rating = if total_rev > 0.0 {
            Some((pos / total_rev) * 100.0)
        } else {
            None
        };

        let price_cents = entry
            .get("price")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok());
        let orig_cents = entry
            .get("initialprice")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok());
        let discount = entry
            .get("discount")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|&d| d > 0);

        let price_usd = price_cents.map(|c| c / 100.0);
        let orig_price_usd = orig_cents.map(|c| c / 100.0);
        let is_free = price_usd == Some(0.0);

        let owners = entry.get("owners").and_then(|v| v.as_str()).unwrap_or("");
        let dev = entry.get("developer").and_then(|v| v.as_str()).unwrap_or("");

        results.push(GameResult {
            id: format!("steamdb-{}", appid_str),
            title: name,
            description: format!(
                "SteamDB & SteamSpy Analizi • Geliştirici: {} • Tahmini Sahip: {}",
                dev, owners
            ),
            url: format!("https://steamdb.info/app/{}/", appid_str),
            image_url: Some(format!(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{}/header.jpg",
                appid_str
            )),
            store: "SteamDB".to_string(),
            store_category: "steamdb".to_string(),
            platforms: vec!["PC".to_string(), "Windows".to_string()],
            tags: vec!["SteamDB".to_string(), formatted_tag.clone()],
            languages: vec!["EN".to_string()],
            has_english: Some(true),
            has_turkish: None,
            is_free,
            price_usd,
            original_price_usd: orig_price_usd,
            price_display: if is_free {
                "Ücretsiz".to_string()
            } else if let Some(p) = price_usd {
                format!("${:.2}", p)
            } else {
                "SteamDB'de Gör".to_string()
            },
            discount_percent: discount,
            rating,
            release_date: None,
            source_name: "SteamDB / SteamSpy".to_string(),
            is_niche_or_news: true,
            relevance_score: 0.0,
        });

        if results.len() >= 12 {
            break;
        }
    }

    Ok(results)
}

fn push_unique(vec: &mut Vec<String>, val: &str) {
    if !vec.iter().any(|s| s == val) {
        vec.push(val.to_string());
    }
}

fn clean_bbcode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut inside = false;
    for ch in input.chars() {
        match ch {
            '[' => inside = true,
            ']' => inside = false,
            _ if !inside => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn title_case_tag(raw: &str) -> String {
    match raw.trim().to_lowercase().as_str() {
        "roguelike" | "rogue-like" => return "Rogue-like".to_string(),
        "roguelite" | "rogue-lite" => return "Rogue-lite".to_string(),
        "rpg" => return "RPG".to_string(),
        "jrpg" => return "JRPG".to_string(),
        "fps" => return "FPS".to_string(),
        "mmo" => return "MMO".to_string(),
        "mmorpg" => return "MMORPG".to_string(),
        "co-op" | "coop" => return "Co-op".to_string(),
        "pixel" | "pixel-art" | "pixel art" => return "Pixel Graphics".to_string(),
        "open-world" | "open world" => return "Open World".to_string(),
        "story-rich" | "story rich" => return "Story Rich".to_string(),
        "singleplayer" | "single-player" => return "Singleplayer".to_string(),
        "multiplayer" => return "Multiplayer".to_string(),
        _ => {}
    }
    raw.split(|c: char| c == ' ' || c == '-')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
