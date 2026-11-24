use chrono::Utc;
use futures::future::join_all;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

const JUSTWATCH_GRAPHQL_URL: &str = "https://apis.justwatch.com/graphql";
const CINEMETA_BASE: &str = "https://v3-cinemeta.strem.io";
const SEARCH_CACHE_TTL_SECS: u64 = 300;
const DETAIL_CACHE_TTL_SECS: u64 = 900;
const MAX_SEARCH_CACHE_ENTRIES: usize = 128;
const MAX_DETAIL_CACHE_ENTRIES: usize = 256;

const JW_SEARCH_QUERY: &str = r#"
query SearchMovies($country: Country!, $language: Language!, $first: Int!, $filter: TitleFilter, $sortBy: PopularTitlesSorting!) {
  popularTitles(country: $country, first: $first, filter: $filter, sortBy: $sortBy) {
    edges {
      node {
        id
        objectId
        content(country: $country, language: $language) {
          title
          originalTitle
          originalReleaseYear
          shortDescription
          runtime
          posterUrl
          backdrops { backdropUrl }
          genres { shortName translation(language: $language) }
          productionCountries
          scoring {
            imdbScore
            imdbVotes
            tmdbScore
            tmdbPopularity
            tomatoMeter
            certifiedFresh
            jwRating
          }
          externalIds { imdbId tmdbId }
          clips { externalId provider name }
          credits { name role characterName }
          fullPath
        }
        offers(country: $country, platform: WEB) {
          monetizationType
          retailPrice(language: $language)
          standardWebURL
          package { clearName shortName icon }
        }
      }
    }
  }
}
"#;

const JW_DETAIL_QUERY: &str = r#"
query MovieDetail($id: ID!, $country: Country!, $language: Language!) {
  node(id: $id) {
    ... on Movie {
      id
      objectId
      content(country: $country, language: $language) {
        title
        originalTitle
        originalReleaseYear
        shortDescription
        runtime
        posterUrl
        backdrops { backdropUrl }
        genres { shortName translation(language: $language) }
        productionCountries
        scoring {
          imdbScore
          imdbVotes
          tmdbScore
          tmdbPopularity
          tomatoMeter
          certifiedFresh
          jwRating
        }
        externalIds { imdbId tmdbId }
        clips { externalId provider name }
        credits { name role characterName }
        fullPath
      }
      offers(country: $country, platform: WEB) {
        monetizationType
        retailPrice(language: $language)
        standardWebURL
        package { clearName shortName icon }
      }
      similarTitles(country: $country, limit: 10) {
        id
        objectId
        content(country: $country, language: $language) {
          title
          originalTitle
          originalReleaseYear
          shortDescription
          runtime
          posterUrl
          backdrops { backdropUrl }
          genres { shortName translation(language: $language) }
          productionCountries
          scoring {
            imdbScore
            imdbVotes
            tmdbScore
            tmdbPopularity
            tomatoMeter
            certifiedFresh
            jwRating
          }
          externalIds { imdbId tmdbId }
          clips { externalId provider name }
          credits { name role characterName }
          fullPath
        }
        offers(country: $country, platform: WEB) {
          monetizationType
          retailPrice(language: $language)
          standardWebURL
          package { clearName shortName icon }
        }
      }
    }
  }
}
"#;

fn default_true() -> bool {
    true
}

fn default_all() -> String {
    "all".to_string()
}

fn default_region() -> String {
    "US".to_string()
}

fn default_lang() -> String {
    "en".to_string()
}

fn default_rating_system() -> String {
    "imdb".to_string()
}

fn default_sort_by() -> String {
    "popular".to_string()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MovieSearchRequest {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub actor: String,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub mood: String,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default = "default_all")]
    pub monetization: String,
    #[serde(default)]
    pub origin_countries: Vec<String>,
    #[serde(default = "default_region")]
    pub region: String,
    #[serde(default = "default_lang")]
    pub lang: String,
    #[serde(default = "default_rating_system")]
    pub rating_system: String,
    #[serde(default)]
    pub min_rating: Option<f64>,
    #[serde(default)]
    pub min_votes: Option<u64>,
    #[serde(default = "default_true")]
    pub must_have_rating: bool,
    #[serde(default)]
    pub year_from: Option<i32>,
    #[serde(default)]
    pub year_to: Option<i32>,
    #[serde(default = "default_sort_by")]
    pub sort_by: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MovieDetailQuery {
    pub jw_id: Option<String>,
    pub imdb_id: Option<String>,
    pub title: Option<String>,
    pub year: Option<i32>,
    pub country: Option<String>,
    pub lang: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GenreTag {
    pub code: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CastMember {
    pub name: String,
    pub character: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WatchOffer {
    pub provider_name: String,
    pub provider_short: String,
    pub provider_icon: Option<String>,
    pub monetization_type: String,
    pub price: Option<String>,
    pub url: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TrailerClip {
    pub title: String,
    pub youtube_id: String,
    pub youtube_url: String,
    pub embed_url: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceLink {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceStatus {
    pub name: String,
    pub ok: bool,
    pub count: usize,
    pub elapsed_ms: u128,
}

#[derive(Clone, Debug, Serialize)]
pub struct MovieItem {
    pub id: String,
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<String>,
    pub title: String,
    pub original_title: Option<String>,
    pub year: Option<i32>,
    pub runtime_min: Option<i32>,
    pub synopsis: String,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub screenshots: Vec<String>,
    pub genres: Vec<GenreTag>,
    pub moods: Vec<String>,
    pub origin_countries: Vec<String>,
    pub imdb_score: Option<f64>,
    pub imdb_votes: Option<u64>,
    pub tmdb_score: Option<f64>,
    pub tomato_meter: Option<u32>,
    pub certified_fresh: bool,
    pub netflix_match_pct: Option<u32>,
    pub netflix_stars: Option<f64>,
    pub directors: Vec<String>,
    pub cast: Vec<CastMember>,
    pub offers: Vec<WatchOffer>,
    pub trailers: Vec<TrailerClip>,
    pub sources: Vec<SourceLink>,
    #[serde(skip_serializing)]
    all_people_norm: Vec<String>,
    #[serde(skip_serializing)]
    tmdb_popularity: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct MovieSearchResponse {
    pub results: Vec<MovieItem>,
    pub total_fetched: usize,
    pub filtered_count: usize,
    pub region: String,
    pub lang: String,
    pub elapsed_ms: u128,
    pub sources_status: Vec<SourceStatus>,
    pub cached: bool,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct MovieDetailResponse {
    pub movie: MovieItem,
    pub awards: Option<String>,
    pub writers: Vec<String>,
    pub wiki_extract: Option<String>,
    pub wiki_url: Option<String>,
    pub trakt_popularity: Option<f64>,
    pub similar_movies: Vec<MovieItem>,
    pub cached: bool,
}

struct CachedSearch {
    data: MovieSearchResponse,
    fetched_at: Instant,
}

struct CachedDetail {
    data: MovieDetailResponse,
    fetched_at: Instant,
}

#[derive(Clone)]
pub struct MovieFinderCache {
    search_cache: Arc<RwLock<HashMap<String, CachedSearch>>>,
    detail_cache: Arc<RwLock<HashMap<String, CachedDetail>>>,
}

impl MovieFinderCache {
    pub fn new() -> Self {
        Self {
            search_cache: Arc::new(RwLock::new(HashMap::new())),
            detail_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn search(
        &self,
        client: &Client,
        req: &MovieSearchRequest,
    ) -> MovieSearchResponse {
        let key = build_search_cache_key(req);
        let skip_cache = req.sort_by.eq_ignore_ascii_case("random");

        if !skip_cache {
            let guard = self.search_cache.read().await;
            if let Some(entry) = guard.get(&key) {
                if entry.fetched_at.elapsed() < Duration::from_secs(SEARCH_CACHE_TTL_SECS) {
                    let mut cached = entry.data.clone();
                    cached.cached = true;
                    return cached;
                }
            }
        }

        let resp = execute_search(client, req).await;

        if !skip_cache && !resp.results.is_empty() {
            let mut guard = self.search_cache.write().await;
            if guard.len() >= MAX_SEARCH_CACHE_ENTRIES {
                guard.retain(|_, v| v.fetched_at.elapsed() < Duration::from_secs(SEARCH_CACHE_TTL_SECS));
                if guard.len() >= MAX_SEARCH_CACHE_ENTRIES {
                    guard.clear();
                }
            }
            guard.insert(
                key,
                CachedSearch {
                    data: resp.clone(),
                    fetched_at: Instant::now(),
                },
            );
        }

        resp
    }

    pub async fn detail(
        &self,
        client: &Client,
        query: &MovieDetailQuery,
    ) -> Result<MovieDetailResponse, String> {
        let region = normalize_region(query.country.as_deref().unwrap_or("US"));
        let lang = normalize_lang(query.lang.as_deref().unwrap_or("en"));
        let key = format!(
            "{}|{}|{}|{}|{}",
            query.jw_id.as_deref().unwrap_or(""),
            query.imdb_id.as_deref().unwrap_or(""),
            query.title.as_deref().unwrap_or(""),
            region,
            lang
        );

        {
            let guard = self.detail_cache.read().await;
            if let Some(entry) = guard.get(&key) {
                if entry.fetched_at.elapsed() < Duration::from_secs(DETAIL_CACHE_TTL_SECS) {
                    let mut cached = entry.data.clone();
                    cached.cached = true;
                    return Ok(cached);
                }
            }
        }

        let resp = execute_detail(client, query, region, lang).await?;

        {
            let mut guard = self.detail_cache.write().await;
            if guard.len() >= MAX_DETAIL_CACHE_ENTRIES {
                guard.retain(|_, v| v.fetched_at.elapsed() < Duration::from_secs(DETAIL_CACHE_TTL_SECS));
                if guard.len() >= MAX_DETAIL_CACHE_ENTRIES {
                    guard.clear();
                }
            }
            guard.insert(
                key,
                CachedDetail {
                    data: resp.clone(),
                    fetched_at: Instant::now(),
                },
            );
        }

        Ok(resp)
    }
}

/* ---------------- JustWatch Raw Deserialization ---------------- */

#[derive(Debug, Deserialize)]
struct JwGraphqlResponse {
    data: Option<JwData>,
}

#[derive(Debug, Deserialize)]
struct JwData {
    #[serde(rename = "popularTitles")]
    popular_titles: Option<JwConnection>,
    node: Option<JwNode>,
}

#[derive(Debug, Deserialize)]
struct JwConnection {
    #[serde(default)]
    edges: Vec<JwEdge>,
}

#[derive(Debug, Deserialize)]
struct JwEdge {
    node: Option<JwNode>,
}

#[derive(Debug, Deserialize)]
struct JwNode {
    id: Option<String>,
    content: Option<JwContent>,
    #[serde(default)]
    offers: Vec<JwOffer>,
    #[serde(default, rename = "similarTitles")]
    similar_titles: Vec<JwNode>,
}

#[derive(Debug, Deserialize)]
struct JwContent {
    title: Option<String>,
    #[serde(rename = "originalTitle")]
    original_title: Option<String>,
    #[serde(rename = "originalReleaseYear")]
    original_release_year: Option<i32>,
    #[serde(rename = "shortDescription")]
    short_description: Option<String>,
    runtime: Option<i32>,
    #[serde(rename = "posterUrl")]
    poster_url: Option<String>,
    #[serde(default)]
    backdrops: Vec<JwBackdrop>,
    #[serde(default)]
    genres: Vec<JwGenre>,
    #[serde(default, rename = "productionCountries")]
    production_countries: Vec<String>,
    scoring: Option<JwScoring>,
    #[serde(rename = "externalIds")]
    external_ids: Option<JwExternalIds>,
    #[serde(default)]
    clips: Vec<JwClip>,
    #[serde(default)]
    credits: Vec<JwCredit>,
    #[serde(rename = "fullPath")]
    full_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JwBackdrop {
    #[serde(rename = "backdropUrl")]
    backdrop_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JwGenre {
    #[serde(rename = "shortName")]
    short_name: Option<String>,
    translation: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JwScoring {
    #[serde(rename = "imdbScore")]
    imdb_score: Option<f64>,
    #[serde(rename = "imdbVotes")]
    imdb_votes: Option<f64>,
    #[serde(rename = "tmdbScore")]
    tmdb_score: Option<f64>,
    #[serde(rename = "tmdbPopularity")]
    tmdb_popularity: Option<f64>,
    #[serde(rename = "tomatoMeter")]
    tomato_meter: Option<f64>,
    #[serde(rename = "certifiedFresh")]
    certified_fresh: Option<bool>,
    #[serde(rename = "jwRating")]
    jw_rating: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct JwExternalIds {
    #[serde(rename = "imdbId")]
    imdb_id: Option<String>,
    #[serde(rename = "tmdbId")]
    tmdb_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JwClip {
    #[serde(rename = "externalId")]
    external_id: Option<String>,
    provider: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JwCredit {
    name: Option<String>,
    role: Option<String>,
    #[serde(rename = "characterName")]
    character_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JwOffer {
    #[serde(rename = "monetizationType")]
    monetization_type: Option<String>,
    #[serde(rename = "retailPrice")]
    retail_price: Option<String>,
    #[serde(rename = "standardWebURL")]
    standard_web_url: Option<String>,
    package: Option<JwPackage>,
}

#[derive(Debug, Deserialize)]
struct JwPackage {
    #[serde(rename = "clearName")]
    clear_name: Option<String>,
    #[serde(rename = "shortName")]
    short_name: Option<String>,
    icon: Option<String>,
}

/* ---------------- Cinemeta & Wikipedia Deserialization ---------------- */

#[derive(Debug, Deserialize)]
struct CinemetaCatalogResp {
    #[serde(default)]
    metas: Vec<CinemetaMeta>,
}

#[derive(Debug, Deserialize)]
struct CinemetaMetaResp {
    meta: Option<CinemetaMeta>,
}

#[derive(Debug, Deserialize)]
struct CinemetaMeta {
    id: Option<String>,
    imdb_id: Option<String>,
    moviedb_id: Option<Value>,
    name: Option<String>,
    #[serde(rename = "releaseInfo")]
    release_info: Option<String>,
    year: Option<Value>,
    description: Option<String>,
    runtime: Option<String>,
    poster: Option<String>,
    background: Option<String>,
    #[serde(default)]
    genres: Vec<String>,
    #[serde(default)]
    genre: Vec<String>,
    country: Option<String>,
    #[serde(rename = "imdbRating")]
    imdb_rating: Option<String>,
    #[serde(default)]
    cast: Vec<String>,
    #[serde(default)]
    director: Vec<String>,
    #[serde(default)]
    writer: Vec<String>,
    awards: Option<String>,
    #[serde(default)]
    trailers: Vec<CinemetaTrailer>,
    #[serde(default, rename = "trailerStreams")]
    trailer_streams: Vec<CinemetaTrailerStream>,
    popularities: Option<CinemetaPopularities>,
}

#[derive(Debug, Deserialize)]
struct CinemetaTrailer {
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CinemetaTrailerStream {
    title: Option<String>,
    #[serde(rename = "ytId")]
    yt_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CinemetaPopularities {
    trakt: Option<f64>,
    moviedb: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct WikiSummaryResp {
    #[serde(rename = "type")]
    page_type: Option<String>,
    description: Option<String>,
    extract: Option<String>,
    content_urls: Option<WikiContentUrls>,
}

#[derive(Debug, Deserialize)]
struct WikiContentUrls {
    desktop: Option<WikiDesktopUrl>,
}

#[derive(Debug, Deserialize)]
struct WikiDesktopUrl {
    page: Option<String>,
}

/* ---------------- Mood Definitions ---------------- */

struct MoodDef {
    slug: &'static str,
    default_genres: &'static [&'static str],
    min_imdb: Option<f64>,
    keywords: &'static [&'static str],
}

const MOOD_DEFS: &[MoodDef] = &[
    MoodDef {
        slug: "mind_bending",
        default_genres: &["scf", "trl", "drm"],
        min_imdb: Some(7.0),
        keywords: &[
            "dream", "memory", "time", "reality", "twist", "illusion", "simulation", "identity",
            "mind", "parallel", "universe", "paradox", "consciousness", "labyrinth", "ruya",
            "zihin", "zaman", "gerceklik", "bilinc", "paradoks", "bulmaca",
        ],
    },
    MoodDef {
        slug: "feel_good",
        default_genres: &["cmy", "fml", "rma", "msc"],
        min_imdb: Some(6.5),
        keywords: &[
            "friendship", "heartwarming", "joy", "hope", "kind", "inspire", "wonderful",
            "uplifting", "smile", "celebrate", "community", "music", "dostluk", "umut", "neseli",
            "mutluluk", "ilham", "sevgi",
        ],
    },
    MoodDef {
        slug: "dark_gritty",
        default_genres: &["crm", "trl", "drm"],
        min_imdb: Some(6.8),
        keywords: &[
            "murder", "detective", "corrupt", "crime", "gang", "revenge", "dark", "brutal",
            "cartel", "assassin", "noir", "serial", "mafia", "underworld", "cinayet", "suc",
            "karanlik", "intikam", "dedektif", "mafya",
        ],
    },
    MoodDef {
        slug: "adrenaline",
        default_genres: &["act", "trl"],
        min_imdb: Some(6.2),
        keywords: &[
            "chase", "mission", "escape", "spy", "explosion", "combat", "survival", "race",
            "heist", "agent", "mercenary", "rescue", "takip", "gorev", "kacis", "ajan", "soygun",
            "aksiyon",
        ],
    },
    MoodDef {
        slug: "tearjerker",
        default_genres: &["drm", "rma"],
        min_imdb: Some(7.0),
        keywords: &[
            "loss", "grief", "terminal", "farewell", "tears", "sacrifice", "tragic", "struggle",
            "father", "mother", "devotion", "memory", "kayip", "veda", "fedakarlik", "huzun",
            "gozyasi", "dram",
        ],
    },
    MoodDef {
        slug: "spooky",
        default_genres: &["hrr", "trl"],
        min_imdb: Some(6.0),
        keywords: &[
            "haunted", "ghost", "demon", "curse", "evil", "terror", "nightmare", "monster",
            "supernatural", "occult", "possession", "vampire", "zombie", "hayalet", "lanet",
            "kabus", "korku", "seytan",
        ],
    },
    MoodDef {
        slug: "laugh_out_loud",
        default_genres: &["cmy"],
        min_imdb: Some(6.3),
        keywords: &[
            "comedy", "hilarious", "parody", "satire", "wedding", "buddy", "absurd", "awkward",
            "chaos", "funny", "misadventure", "komedi", "eglenceli", "mizah", "gulunc",
        ],
    },
    MoodDef {
        slug: "romantic",
        default_genres: &["rma", "drm", "cmy"],
        min_imdb: Some(6.4),
        keywords: &[
            "love", "romance", "couple", "wedding", "passion", "heart", "soulmate", "kiss",
            "affair", "fall in love", "ask", "romantik", "sevgi", "tutku", "kalp",
        ],
    },
    MoodDef {
        slug: "epic_adventure",
        default_genres: &["act", "fnt", "scf", "war"],
        min_imdb: Some(6.8),
        keywords: &[
            "journey", "kingdom", "quest", "galaxy", "empire", "legend", "hero", "expedition",
            "destiny", "myth", "odyssey", "battle", "destan", "yolculuk", "krallik", "kahraman",
            "macera", "galaksi",
        ],
    },
    MoodDef {
        slug: "true_story",
        default_genres: &["doc", "hst", "drm"],
        min_imdb: Some(7.0),
        keywords: &[
            "true story", "based on", "biography", "real life", "historical", "journalist",
            "inventor", "scandal", "memoir", "gercek hikaye", "biyografi", "tarih", "belgesel",
        ],
    },
    MoodDef {
        slug: "late_night_cult",
        default_genres: &["scf", "hrr", "trl", "cmy"],
        min_imdb: Some(6.5),
        keywords: &[
            "cult", "bizarre", "surreal", "neon", "underground", "strange", "psychedelic",
            "midnight", "alien", "cyber", "dystopian", "tuhaf", "gercekustu", "yeralti",
        ],
    },
    MoodDef {
        slug: "family_night",
        default_genres: &["fml", "ani", "cmy"],
        min_imdb: Some(6.3),
        keywords: &[
            "animated", "magic", "kid", "animal", "princess", "toy", "school", "family", "robot",
            "magical", "aile", "cocuk", "animasyon", "sihir", "hayvan",
        ],
    },
];

fn find_mood_def(slug: &str) -> Option<&'static MoodDef> {
    let s = slug.trim().to_ascii_lowercase();
    MOOD_DEFS.iter().find(|m| m.slug == s)
}

/* ---------------- Normalization & Formatting Helpers ---------------- */

pub fn normalize_region(raw: &str) -> &'static str {
    match raw.trim().to_ascii_uppercase().as_str() {
        "TR" => "TR",
        "GB" | "UK" => "GB",
        "DE" => "DE",
        "FR" => "FR",
        "JP" => "JP",
        "KR" => "KR",
        _ => "US",
    }
}

pub fn normalize_lang(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "tr" => "tr",
        _ => "en",
    }
}

pub fn resolve_platform_packages(platforms: &[String], region: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    let reg = normalize_region(region);

    let mut push_unique = |code: &'static str| {
        if !out.contains(&code) {
            out.push(code);
        }
    };

    for p in platforms {
        let slug = p.trim().to_ascii_lowercase();
        match slug.as_str() {
            "netflix" | "nfx" => push_unique("nfx"),
            "prime" | "amp" | "prv" => {
                if reg == "US" {
                    push_unique("amp");
                } else {
                    push_unique("prv");
                }
            }
            "disney" | "dnp" => push_unique("dnp"),
            "max" | "hbomax" | "mxx" => push_unique("mxx"),
            "apple" | "atp" | "itu" => {
                if matches!(reg, "US" | "GB" | "DE" | "FR") {
                    push_unique("atp");
                    push_unique("itu");
                } else {
                    push_unique("itu");
                }
            }
            "mubi" | "mbi" => push_unique("mbi"),
            "youtube" | "ytr" | "yfr" => {
                push_unique("ytr");
                if reg == "US" {
                    push_unique("yfr");
                }
            }
            "crunchyroll" | "cru" => push_unique("cru"),
            "tvplus" | "ttp" => push_unique("ttp"),
            "puhutv" | "pht" => push_unique("pht"),
            "exxen" | "exn" => push_unique("exn"),
            "tod" | "tdt" => push_unique("tdt"),
            "tabii" | "tab" => push_unique("tab"),
            "hulu" | "hlu" => push_unique("hlu"),
            "paramount" | "ppp" | "ppe" => {
                push_unique("ppp");
                push_unique("ppe");
            }
            "peacock" | "pct" => push_unique("pct"),
            "pluto" | "ptv" => push_unique("ptv"),
            "plex" | "plx" => push_unique("plx"),
            "criterion" | "crc" => push_unique("crc"),
            _ => {}
        }
    }

    out
}

pub fn format_jw_poster(template: Option<&str>) -> Option<String> {
    let tpl = template?.trim();
    if tpl.is_empty() {
        return None;
    }
    let path = tpl
        .replace("{profile}", "s592")
        .replace("{format}", "webp");
    if path.starts_with("http://") || path.starts_with("https://") {
        Some(path)
    } else {
        Some(format!("https://images.justwatch.com{}", path))
    }
}

pub fn format_jw_backdrop(template: Option<&str>) -> Option<String> {
    let tpl = template?.trim();
    if tpl.is_empty() {
        return None;
    }
    let path = tpl
        .replace("{profile}", "s1440")
        .replace("{format}", "webp");
    if path.starts_with("http://") || path.starts_with("https://") {
        Some(path)
    } else {
        Some(format!("https://images.justwatch.com{}", path))
    }
}

pub fn format_jw_icon(template: Option<&str>) -> Option<String> {
    let tpl = template?.trim();
    if tpl.is_empty() {
        return None;
    }
    let path = tpl
        .replace("{profile}", "s100")
        .replace("{format}", "png");
    if path.starts_with("http://") || path.starts_with("https://") {
        Some(path)
    } else {
        Some(format!("https://images.justwatch.com{}", path))
    }
}

pub fn normalize_text_ascii(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            'ç' | 'Ç' => out.push('c'),
            'ğ' | 'Ğ' => out.push('g'),
            'ı' | 'İ' | 'î' | 'í' | 'ì' | 'ï' | 'Í' | 'Î' => out.push('i'),
            'ö' | 'Ö' | 'ó' | 'ò' | 'ô' | 'õ' | 'ø' | 'Ó' => out.push('o'),
            'ş' | 'Ş' => out.push('s'),
            'ü' | 'Ü' | 'ú' | 'ù' | 'û' | 'Ú' => out.push('u'),
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'Á' | 'À' | 'Â' | 'Ä' => out.push('a'),
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => out.push('e'),
            'ñ' | 'Ñ' => out.push('n'),
            _ => {
                for lc in ch.to_lowercase() {
                    out.push(lc);
                }
            }
        }
    }
    out
}

pub fn compute_netflix_metrics(
    jw_rating: Option<f64>,
    imdb_score: Option<f64>,
    tmdb_score: Option<f64>,
    tomato_meter: Option<f64>,
) -> (Option<u32>, Option<f64>) {
    let pct: Option<u32> = if let Some(jw) = jw_rating.filter(|v| v.is_finite() && *v > 0.0) {
        Some(((jw * 100.0).round() as i64).clamp(40, 99) as u32)
    } else {
        let mut sum = 0.0;
        let mut cnt = 0.0;
        if let Some(im) = imdb_score.filter(|v| v.is_finite() && *v > 0.0) {
            sum += im * 10.0;
            cnt += 1.0;
        }
        if let Some(tm) = tmdb_score.filter(|v| v.is_finite() && *v > 0.0) {
            sum += tm * 10.0;
            cnt += 1.0;
        }
        if let Some(rt) = tomato_meter.filter(|v| v.is_finite() && *v > 0.0) {
            sum += rt;
            cnt += 1.0;
        }
        if cnt > 0.0 {
            Some(((sum / cnt).round() as i64).clamp(15, 99) as u32)
        } else {
            None
        }
    };

    let stars: Option<f64> = if let Some(p) = pct {
        let s = (p as f64 / 20.0).clamp(0.5, 5.0);
        Some((s * 10.0).round() / 10.0)
    } else if let Some(im) = imdb_score.filter(|v| v.is_finite() && *v > 0.0) {
        let s = (im / 2.0).clamp(0.5, 5.0);
        Some((s * 10.0).round() / 10.0)
    } else {
        None
    };

    (pct, stars)
}

pub fn infer_moods(genre_codes: &[String], synopsis: &str, imdb_score: Option<f64>) -> Vec<String> {
    let syn_norm = normalize_text_ascii(synopsis);
    let mut scored: Vec<(&'static str, i32)> = Vec::new();

    for def in MOOD_DEFS {
        let mut score = 0i32;
        for g in genre_codes {
            if def.default_genres.contains(&g.as_str()) {
                score += 2;
            }
        }
        for kw in def.keywords {
            if syn_norm.contains(kw) {
                score += 3;
            }
        }
        if let (Some(floor), Some(im)) = (def.min_imdb, imdb_score) {
            if im >= floor {
                score += 1;
            }
        }
        if score >= 3 {
            scored.push((def.slug, score));
        }
    }

    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.into_iter().take(3).map(|(s, _)| s.to_string()).collect()
}

fn is_valid_yt_id(id: &str) -> bool {
    let t = id.trim();
    !t.is_empty()
        && t.len() <= 32
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn monetization_rank(m: &str) -> u8 {
    match m {
        "FLATRATE" => 0,
        "FREE" => 1,
        "ADS" => 2,
        "RENT" => 3,
        "BUY" => 4,
        _ => 5,
    }
}

fn map_jw_node(node: JwNode, lang: &str) -> Option<MovieItem> {
    let id = node.id?;
    if !id.starts_with("tm") {
        return None;
    }
    let c = node.content?;
    let title = c.title.unwrap_or_default().trim().to_string();
    if title.is_empty() {
        return None;
    }

    let original_title = c
        .original_title
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != &title);

    let synopsis = c.short_description.unwrap_or_default().trim().to_string();
    let poster_url = format_jw_poster(c.poster_url.as_deref());

    let mut screenshots: Vec<String> = Vec::new();
    for bd in &c.backdrops {
        if let Some(u) = format_jw_backdrop(bd.backdrop_url.as_deref()) {
            if !screenshots.contains(&u) {
                screenshots.push(u);
            }
        }
    }
    let backdrop_url = screenshots.first().cloned();

    let mut genres: Vec<GenreTag> = Vec::new();
    let mut genre_codes: Vec<String> = Vec::new();
    for g in c.genres {
        if let Some(code) = g.short_name {
            let label = g.translation.unwrap_or_else(|| code.clone());
            if !genre_codes.contains(&code) {
                genre_codes.push(code.clone());
                genres.push(GenreTag { code, label });
            }
        }
    }

    let scoring = c.scoring;
    let imdb_score = scoring
        .as_ref()
        .and_then(|s| s.imdb_score)
        .filter(|v| v.is_finite() && *v > 0.0)
        .map(|v| (v * 10.0).round() / 10.0);
    let imdb_votes = scoring
        .as_ref()
        .and_then(|s| s.imdb_votes)
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| v.round() as u64);
    let tmdb_score = scoring
        .as_ref()
        .and_then(|s| s.tmdb_score)
        .filter(|v| v.is_finite() && *v > 0.0)
        .map(|v| (v * 10.0).round() / 10.0);
    let tmdb_popularity = scoring
        .as_ref()
        .and_then(|s| s.tmdb_popularity)
        .filter(|v| v.is_finite())
        .unwrap_or(0.0);
    let tomato_meter_f = scoring
        .as_ref()
        .and_then(|s| s.tomato_meter)
        .filter(|v| v.is_finite() && *v >= 0.0);
    let tomato_meter = tomato_meter_f.map(|v| v.round().clamp(0.0, 100.0) as u32);
    let certified_fresh = scoring
        .as_ref()
        .and_then(|s| s.certified_fresh)
        .unwrap_or(false);
    let jw_rating = scoring.as_ref().and_then(|s| s.jw_rating);

    let (netflix_match_pct, netflix_stars) =
        compute_netflix_metrics(jw_rating, imdb_score, tmdb_score, tomato_meter_f);

    let imdb_id = c
        .external_ids
        .as_ref()
        .and_then(|e| e.imdb_id.clone())
        .filter(|s| !s.trim().is_empty());
    let tmdb_id = c
        .external_ids
        .as_ref()
        .and_then(|e| e.tmdb_id.clone())
        .filter(|s| !s.trim().is_empty());

    let mut directors: Vec<String> = Vec::new();
    let mut cast: Vec<CastMember> = Vec::new();
    let mut all_people_norm: Vec<String> = Vec::new();
    let mut seen_people: HashSet<String> = HashSet::new();

    for cr in c.credits {
        let Some(name) = cr.name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) else {
            continue;
        };
        let norm = normalize_text_ascii(&name);
        if seen_people.insert(norm.clone()) {
            all_people_norm.push(norm);
        }
        let role = cr.role.unwrap_or_default().to_ascii_uppercase();
        if role == "DIRECTOR" {
            if !directors.contains(&name) && directors.len() < 4 {
                directors.push(name);
            }
        } else if role == "ACTOR" && cast.len() < 16 {
            let ch = cr
                .character_name
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            cast.push(CastMember { name, character: ch });
        }
    }

    let mut trailers: Vec<TrailerClip> = Vec::new();
    let mut seen_yt: HashSet<String> = HashSet::new();
    for clip in c.clips {
        let provider = clip.provider.unwrap_or_default();
        if !provider.eq_ignore_ascii_case("YOUTUBE") {
            continue;
        }
        if let Some(yt_id) = clip.external_id.filter(|i| is_valid_yt_id(i)) {
            let yt_clean = yt_id.trim().to_string();
            if seen_yt.insert(yt_clean.clone()) {
                let clip_title = clip
                    .name
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| format!("{} Trailer", title));
                trailers.push(TrailerClip {
                    title: clip_title,
                    youtube_url: format!("https://www.youtube.com/watch?v={}", yt_clean),
                    embed_url: format!("https://www.youtube.com/embed/{}", yt_clean),
                    youtube_id: yt_clean,
                });
            }
        }
    }

    let mut offers_map: HashMap<(String, String), WatchOffer> = HashMap::new();
    for off in node.offers {
        let Some(pkg) = off.package else { continue };
        let short = pkg.short_name.unwrap_or_default();
        let name = pkg.clear_name.unwrap_or_else(|| short.clone());
        let mtype = off
            .monetization_type
            .unwrap_or_else(|| "FLATRATE".to_string());
        let url = off.standard_web_url.unwrap_or_default();
        if short.is_empty() || (!url.starts_with("https://") && !url.starts_with("http://")) {
            continue;
        }
        let price = off.retail_price.filter(|p| !p.trim().is_empty());
        let icon = format_jw_icon(pkg.icon.as_deref());
        let key = (short.clone(), mtype.clone());
        let entry = offers_map.entry(key).or_insert_with(|| WatchOffer {
            provider_name: name,
            provider_short: short,
            provider_icon: icon,
            monetization_type: mtype,
            price: price.clone(),
            url: url.clone(),
        });
        if entry.price.is_none() && price.is_some() {
            entry.price = price;
        }
    }

    let mut offers: Vec<WatchOffer> = offers_map.into_values().collect();
    offers.sort_by(|a, b| {
        monetization_rank(&a.monetization_type)
            .cmp(&monetization_rank(&b.monetization_type))
            .then_with(|| a.provider_name.cmp(&b.provider_name))
    });

    let mut sources: Vec<SourceLink> = Vec::new();
    if let Some(fp) = c.full_path.filter(|p| p.starts_with('/')) {
        sources.push(SourceLink {
            name: "JustWatch".to_string(),
            url: format!("https://www.justwatch.com{}", fp),
        });
    } else {
        sources.push(SourceLink {
            name: "JustWatch".to_string(),
            url: "https://www.justwatch.com".to_string(),
        });
    }
    if let Some(ref im) = imdb_id {
        sources.push(SourceLink {
            name: "IMDb".to_string(),
            url: format!("https://www.imdb.com/title/{}/", im),
        });
        sources.push(SourceLink {
            name: "Cinemeta".to_string(),
            url: format!("{}/meta/movie/{}.json", CINEMETA_BASE, im),
        });
    }
    if let Some(ref tm) = tmdb_id {
        let tm_clean = tm.trim();
        if tm_clean.chars().all(|ch| ch.is_ascii_digit()) {
            let lang_q = if lang == "tr" { "?language=tr-TR" } else { "" };
            sources.push(SourceLink {
                name: "TMDB".to_string(),
                url: format!("https://www.themoviedb.org/movie/{}{}", tm_clean, lang_q),
            });
        }
    }

    let moods = infer_moods(&genre_codes, &synopsis, imdb_score);
    let origin_countries = c
        .production_countries
        .into_iter()
        .map(|cc| cc.trim().to_ascii_uppercase())
        .filter(|cc| !cc.is_empty())
        .collect();

    Some(MovieItem {
        id,
        imdb_id,
        tmdb_id,
        title,
        original_title,
        year: c.original_release_year,
        runtime_min: c.runtime,
        synopsis,
        poster_url,
        backdrop_url,
        screenshots,
        genres,
        moods,
        origin_countries,
        imdb_score,
        imdb_votes,
        tmdb_score,
        tomato_meter,
        certified_fresh,
        netflix_match_pct,
        netflix_stars,
        directors,
        cast,
        offers,
        trailers,
        sources,
        all_people_norm,
        tmdb_popularity,
    })
}

fn parse_year_from_meta(year_val: Option<&Value>, release_info: Option<&str>) -> Option<i32> {
    if let Some(v) = year_val {
        if let Some(n) = v.as_i64() {
            return Some(n as i32);
        }
        if let Some(s) = v.as_str() {
            if let Ok(n) = s.trim().chars().take(4).collect::<String>().parse::<i32>() {
                return Some(n);
            }
        }
    }
    release_info.and_then(|r| r.trim().chars().take(4).collect::<String>().parse::<i32>().ok())
}

fn parse_runtime_min(runtime_str: Option<&str>) -> Option<i32> {
    let s = runtime_str?;
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<i32>().ok()
}

fn genre_label_to_code(label: &str) -> String {
    match label.trim().to_ascii_lowercase().as_str() {
        "action" | "adventure" | "action & adventure" => "act",
        "animation" => "ani",
        "comedy" => "cmy",
        "crime" => "crm",
        "documentary" => "doc",
        "drama" => "drm",
        "fantasy" => "fnt",
        "history" => "hst",
        "horror" => "hrr",
        "family" | "kids & family" => "fml",
        "music" | "musical" => "msc",
        "mystery" | "thriller" => "trl",
        "romance" => "rma",
        "sci-fi" | "science fiction" => "scf",
        "sport" => "spt",
        "war" => "war",
        "western" => "wsn",
        _ => "drm",
    }
    .to_string()
}

fn map_cinemeta_to_movie(meta: &CinemetaMeta) -> Option<MovieItem> {
    let imdb_id = meta
        .imdb_id
        .clone()
        .or_else(|| meta.id.clone())
        .filter(|s| s.starts_with("tt"))?;
    let title = meta.name.clone().unwrap_or_default().trim().to_string();
    if title.is_empty() {
        return None;
    }

    let year = parse_year_from_meta(meta.year.as_ref(), meta.release_info.as_deref());
    let runtime_min = parse_runtime_min(meta.runtime.as_deref());
    let synopsis = meta.description.clone().unwrap_or_default().trim().to_string();

    let imdb_score = meta
        .imdb_rating
        .as_deref()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .map(|v| (v * 10.0).round() / 10.0);

    let (netflix_match_pct, netflix_stars) = compute_netflix_metrics(None, imdb_score, None, None);

    let raw_genres = if !meta.genres.is_empty() {
        &meta.genres
    } else {
        &meta.genre
    };
    let mut genres = Vec::new();
    let mut genre_codes = Vec::new();
    for g in raw_genres {
        let code = genre_label_to_code(g);
        if !genre_codes.contains(&code) {
            genre_codes.push(code.clone());
        }
        genres.push(GenreTag {
            code,
            label: g.clone(),
        });
    }

    let mut screenshots = Vec::new();
    if let Some(ref bg) = meta.background {
        if bg.starts_with("http://") || bg.starts_with("https://") {
            screenshots.push(bg.clone());
        }
    }

    let mut all_people_norm = Vec::new();
    let mut cast = Vec::new();
    for person in &meta.cast {
        let clean = person.trim().to_string();
        if !clean.is_empty() {
            all_people_norm.push(normalize_text_ascii(&clean));
            cast.push(CastMember {
                name: clean,
                character: None,
            });
        }
    }
    for dir in &meta.director {
        let clean = dir.trim();
        if !clean.is_empty() {
            all_people_norm.push(normalize_text_ascii(clean));
        }
    }

    let mut trailers = Vec::new();
    let mut seen_yt = HashSet::new();
    for ts in &meta.trailer_streams {
        if let Some(ref yt) = ts.yt_id {
            if is_valid_yt_id(yt) && seen_yt.insert(yt.clone()) {
                trailers.push(TrailerClip {
                    title: ts
                        .title
                        .clone()
                        .unwrap_or_else(|| format!("{} Trailer", title)),
                    youtube_id: yt.clone(),
                    youtube_url: format!("https://www.youtube.com/watch?v={}", yt),
                    embed_url: format!("https://www.youtube.com/embed/{}", yt),
                });
            }
        }
    }
    for t in &meta.trailers {
        if let Some(ref yt) = t.source {
            if is_valid_yt_id(yt) && seen_yt.insert(yt.clone()) {
                trailers.push(TrailerClip {
                    title: format!("{} Trailer", title),
                    youtube_id: yt.clone(),
                    youtube_url: format!("https://www.youtube.com/watch?v={}", yt),
                    embed_url: format!("https://www.youtube.com/embed/{}", yt),
                });
            }
        }
    }

    let tmdb_id = meta.moviedb_id.as_ref().and_then(|v| {
        if let Some(n) = v.as_u64() {
            Some(n.to_string())
        } else {
            v.as_str().map(|s| s.to_string())
        }
    });

    let mut sources = vec![
        SourceLink {
            name: "IMDb".to_string(),
            url: format!("https://www.imdb.com/title/{}/", imdb_id),
        },
        SourceLink {
            name: "Cinemeta".to_string(),
            url: format!("{}/meta/movie/{}.json", CINEMETA_BASE, imdb_id),
        },
    ];
    if let Some(ref tm) = tmdb_id {
        sources.push(SourceLink {
            name: "TMDB".to_string(),
            url: format!("https://www.themoviedb.org/movie/{}", tm),
        });
    }

    let origin_countries = meta
        .country
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let moods = infer_moods(&genre_codes, &synopsis, imdb_score);

    Some(MovieItem {
        id: imdb_id.clone(),
        imdb_id: Some(imdb_id),
        tmdb_id,
        title,
        original_title: None,
        year,
        runtime_min,
        synopsis,
        poster_url: meta.poster.clone(),
        backdrop_url: meta.background.clone(),
        screenshots,
        genres,
        moods,
        origin_countries,
        imdb_score,
        imdb_votes: None,
        tmdb_score: None,
        tomato_meter: None,
        certified_fresh: false,
        netflix_match_pct,
        netflix_stars,
        directors: meta.director.clone(),
        cast,
        offers: Vec::new(),
        trailers,
        sources,
        all_people_norm,
        tmdb_popularity: meta.popularities.as_ref().and_then(|p| p.moviedb).unwrap_or(0.0),
    })
}

/* ---------------- Upstream Network Calls ---------------- */

async fn query_justwatch_search(
    client: &Client,
    region: &str,
    lang: &str,
    first: usize,
    filter: Value,
    sort_by: &str,
) -> Result<Vec<MovieItem>, String> {
    let payload = json!({
        "operationName": "SearchMovies",
        "variables": {
            "country": region,
            "language": lang,
            "first": first,
            "filter": filter,
            "sortBy": sort_by
        },
        "query": JW_SEARCH_QUERY
    });

    let resp = client
        .post(JUSTWATCH_GRAPHQL_URL)
        .timeout(Duration::from_secs(9))
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("JustWatch request error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("JustWatch HTTP {}", resp.status()));
    }

    let parsed: JwGraphqlResponse = resp
        .json()
        .await
        .map_err(|e| format!("JustWatch JSON error: {}", e))?;

    let edges = parsed
        .data
        .and_then(|d| d.popular_titles)
        .map(|p| p.edges)
        .unwrap_or_default();

    Ok(edges
        .into_iter()
        .filter_map(|e| e.node)
        .filter_map(|n| map_jw_node(n, lang))
        .collect())
}

async fn query_justwatch_detail(
    client: &Client,
    jw_id: &str,
    region: &str,
    lang: &str,
) -> Result<(MovieItem, Vec<MovieItem>), String> {
    let payload = json!({
        "operationName": "MovieDetail",
        "variables": {
            "id": jw_id,
            "country": region,
            "language": lang
        },
        "query": JW_DETAIL_QUERY
    });

    let resp = client
        .post(JUSTWATCH_GRAPHQL_URL)
        .timeout(Duration::from_secs(9))
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("JustWatch detail error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("JustWatch detail HTTP {}", resp.status()));
    }

    let parsed: JwGraphqlResponse = resp
        .json()
        .await
        .map_err(|e| format!("JustWatch detail JSON error: {}", e))?;

    let mut node = parsed
        .data
        .and_then(|d| d.node)
        .ok_or_else(|| "Movie node not found".to_string())?;

    let raw_similar = std::mem::take(&mut node.similar_titles);
    let movie = map_jw_node(node, lang).ok_or_else(|| "Invalid movie node".to_string())?;
    let similar: Vec<MovieItem> = raw_similar
        .into_iter()
        .filter_map(|n| map_jw_node(n, lang))
        .take(8)
        .collect();

    Ok((movie, similar))
}

async fn fetch_cinemeta_meta(client: &Client, imdb_id: &str) -> Option<CinemetaMeta> {
    let clean = imdb_id.trim();
    if !clean.starts_with("tt") {
        return None;
    }
    let url = format!("{}/meta/movie/{}.json", CINEMETA_BASE, urlencoding::encode(clean));
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let parsed: CinemetaMetaResp = resp.json().await.ok()?;
    parsed.meta
}

async fn search_cinemeta_fallback(client: &Client, query: &str) -> Result<Vec<MovieItem>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let url = format!(
        "{}/catalog/movie/top/search={}.json",
        CINEMETA_BASE,
        urlencoding::encode(q)
    );
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(6))
        .send()
        .await
        .map_err(|e| format!("Cinemeta search error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Cinemeta HTTP {}", resp.status()));
    }

    let parsed: CinemetaCatalogResp = resp
        .json()
        .await
        .map_err(|e| format!("Cinemeta JSON error: {}", e))?;

    let ids: Vec<String> = parsed
        .metas
        .into_iter()
        .filter_map(|m| m.imdb_id.or(m.id))
        .filter(|id| id.starts_with("tt"))
        .take(6)
        .collect();

    let metas = join_all(ids.iter().map(|id| fetch_cinemeta_meta(client, id))).await;
    Ok(metas
        .into_iter()
        .flatten()
        .filter_map(|m| map_cinemeta_to_movie(&m))
        .collect())
}

async fn fetch_wiki_for_title(
    client: &Client,
    wiki_lang: &str,
    candidate_title: &str,
    expected_year: Option<i32>,
) -> Option<(String, String)> {
    let underscored = candidate_title.replace(' ', "_");
    let encoded = urlencoding::encode(&underscored);
    let url = format!(
        "https://{}.wikipedia.org/api/rest_v1/page/summary/{}",
        wiki_lang, encoded
    );
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let parsed: WikiSummaryResp = resp.json().await.ok()?;
    if parsed.page_type.as_deref() != Some("standard") {
        return None;
    }

    let desc = parsed.description.unwrap_or_default();
    let extract = parsed.extract.unwrap_or_default();
    if extract.trim().is_empty() {
        return None;
    }

    let combined_norm = normalize_text_ascii(&format!("{} {}", desc, extract));
    let is_film = [
        "film", "movie", "sinema", "directed by", "yonet", "yapimi", "thriller",
        "comedy", "komedi", "drama", "horror", "korku", "documentary", "belgesel",
        "animated", "animasyon", "bilim kurgu", "sci-fi",
    ]
    .iter()
    .any(|kw| combined_norm.contains(kw));

    if !is_film {
        return None;
    }

    if let Some(yr) = expected_year {
        // If description explicitly mentions a 4-digit year, verify it's close to the movie's year.
        let desc_years: Vec<i32> = desc
            .split(|c: char| !c.is_ascii_digit())
            .filter(|tok| tok.len() == 4)
            .filter_map(|tok| tok.parse::<i32>().ok())
            .filter(|y| (1900..=2035).contains(y))
            .collect();
        if !desc_years.is_empty() && !desc_years.iter().any(|y| (*y - yr).abs() <= 2) {
            return None;
        }
    }

    let page_url = parsed
        .content_urls
        .and_then(|c| c.desktop)
        .and_then(|d| d.page)
        .filter(|u| u.starts_with("https://"))?;

    Some((extract, page_url))
}

async fn fetch_wikipedia_summary(
    client: &Client,
    title: &str,
    original_title: Option<&str>,
    year: Option<i32>,
    lang: &str,
) -> Option<(String, String)> {
    let mut attempts: Vec<(&str, String)> = Vec::new();
    if lang == "tr" {
        attempts.push(("tr", format!("{} (film)", title)));
        attempts.push(("tr", title.to_string()));
        let en_title = original_title.unwrap_or(title);
        if let Some(y) = year {
            attempts.push(("en", format!("{} ({} film)", en_title, y)));
        }
        attempts.push(("en", format!("{} (film)", en_title)));
        attempts.push(("en", en_title.to_string()));
    } else {
        if let Some(y) = year {
            attempts.push(("en", format!("{} ({} film)", title, y)));
        }
        attempts.push(("en", format!("{} (film)", title)));
        attempts.push(("en", title.to_string()));
    }

    for (wlang, cand) in attempts {
        if let Some(hit) = fetch_wiki_for_title(client, wlang, &cand, year).await {
            return Some(hit);
        }
    }
    None
}

/* ---------------- Search Orchestration ---------------- */

fn build_search_cache_key(req: &MovieSearchRequest) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{:?}|{:?}|{}|{:?}|{:?}|{}|{:?}",
        req.query.trim().to_ascii_lowercase(),
        req.actor.trim().to_ascii_lowercase(),
        req.genres.join(","),
        req.mood.trim().to_ascii_lowercase(),
        req.platforms.join(","),
        req.monetization.trim().to_ascii_lowercase(),
        req.origin_countries.join(","),
        normalize_region(&req.region),
        normalize_lang(&req.lang),
        req.rating_system.trim().to_ascii_lowercase(),
        req.min_rating,
        req.min_votes,
        req.must_have_rating,
        req.year_from,
        req.year_to,
        req.sort_by.trim().to_ascii_lowercase(),
        req.limit
    )
}

fn build_jw_filter(
    req: &MovieSearchRequest,
    region: &str,
    search_query_override: Option<&str>,
) -> Value {
    let mut filter = serde_json::Map::new();
    filter.insert("objectTypes".into(), json!(["MOVIE"]));

    if let Some(sq) = search_query_override.map(str::trim).filter(|s| !s.is_empty()) {
        filter.insert("searchQuery".into(), json!(sq));
    }

    let effective_genres: Vec<String> = if !req.genres.is_empty() {
        req.genres
            .iter()
            .map(|g| g.trim().to_ascii_lowercase())
            .filter(|g| !g.is_empty())
            .collect()
    } else if let Some(mood_def) = find_mood_def(&req.mood) {
        mood_def
            .default_genres
            .iter()
            .map(|g| (*g).to_string())
            .collect()
    } else {
        Vec::new()
    };

    if !effective_genres.is_empty() {
        filter.insert("genres".into(), json!(effective_genres));
    }

    let pkgs = resolve_platform_packages(&req.platforms, region);
    if !pkgs.is_empty() {
        filter.insert("packages".into(), json!(pkgs));
    }

    match req.monetization.trim().to_ascii_lowercase().as_str() {
        "flatrate" => {
            filter.insert("monetizationTypes".into(), json!(["FLATRATE"]));
        }
        "free" => {
            filter.insert("monetizationTypes".into(), json!(["FREE", "ADS"]));
        }
        "rent_buy" => {
            filter.insert("monetizationTypes".into(), json!(["RENT", "BUY"]));
        }
        _ => {}
    }

    let origins: Vec<String> = req
        .origin_countries
        .iter()
        .map(|c| c.trim().to_ascii_uppercase())
        .filter(|c| c.len() == 2 && c.chars().all(|ch| ch.is_ascii_alphabetic()))
        .collect();
    if !origins.is_empty() {
        filter.insert("productionCountries".into(), json!(origins));
    }

    let use_netflix = req.rating_system.eq_ignore_ascii_case("netflix");
    let min_imdb_for_jw: Option<f64> = if let Some(mr) = req.min_rating.filter(|v| v.is_finite() && *v > 0.0) {
        if use_netflix {
            // Relaxed IMDb hint to JustWatch; exact Netflix star threshold is enforced post-fetch.
            let hint = (mr * 2.0 - 1.5).max(1.0);
            Some(hint)
        } else {
            Some(mr.clamp(0.0, 10.0))
        }
    } else if req.query.trim().is_empty() && req.actor.trim().is_empty() {
        find_mood_def(&req.mood).and_then(|m| m.min_imdb)
    } else {
        None
    };

    if let Some(min_s) = min_imdb_for_jw {
        filter.insert("imdbScore".into(), json!({ "min": min_s }));
    }

    if req.year_from.is_some() || req.year_to.is_some() {
        let mut yr = serde_json::Map::new();
        if let Some(yf) = req.year_from {
            yr.insert("min".into(), json!(yf));
        }
        if let Some(yt) = req.year_to {
            yr.insert("max".into(), json!(yt));
        }
        filter.insert("releaseYear".into(), Value::Object(yr));
    }

    Value::Object(filter)
}

fn jw_sort_enum(sort_by: &str) -> &'static str {
    match sort_by.trim().to_ascii_lowercase().as_str() {
        "trending" => "TRENDING",
        "imdb_score" => "IMDB_SCORE",
        "release_year" => "RELEASE_YEAR",
        "random" => "RANDOM",
        // For most_watched & netflix_match, pull POPULAR (or IMDB_SCORE) from JustWatch and sort precisely locally.
        _ => "POPULAR",
    }
}

fn movie_matches_person(movie: &MovieItem, person_norm: &str) -> bool {
    if person_norm.is_empty() {
        return true;
    }
    let tokens: Vec<&str> = person_norm.split_whitespace().collect();
    movie.all_people_norm.iter().any(|p| {
        p.contains(person_norm) || tokens.iter().all(|tok| p.contains(tok))
    })
}

fn movie_matches_title(movie: &MovieItem, query_norm: &str) -> bool {
    if query_norm.is_empty() {
        return true;
    }
    let title_norm = normalize_text_ascii(&movie.title);
    let orig_norm = movie
        .original_title
        .as_deref()
        .map(normalize_text_ascii)
        .unwrap_or_default();
    let tokens: Vec<&str> = query_norm.split_whitespace().collect();
    title_norm.contains(query_norm)
        || orig_norm.contains(query_norm)
        || tokens.iter().all(|t| title_norm.contains(t) || orig_norm.contains(t))
}

fn mood_relevance_score(movie: &MovieItem, mood_slug: &str) -> i32 {
    let Some(def) = find_mood_def(mood_slug) else {
        return 0;
    };
    let mut score = 0i32;
    for g in &movie.genres {
        if def.default_genres.contains(&g.code.as_str()) {
            score += 2;
        }
    }
    let syn_norm = normalize_text_ascii(&movie.synopsis);
    for kw in def.keywords {
        if syn_norm.contains(kw) {
            score += 3;
        }
    }
    if movie.moods.iter().any(|m| m == mood_slug) {
        score += 3;
    }
    score
}

async fn execute_search(client: &Client, req: &MovieSearchRequest) -> MovieSearchResponse {
    let started = Instant::now();
    let region = normalize_region(&req.region);
    let lang = normalize_lang(&req.lang);
    let limit = req.limit.unwrap_or(36).clamp(1, 60);

    let q_trimmed = req.query.trim();
    let actor_trimmed = req.actor.trim();
    let jw_sort = jw_sort_enum(&req.sort_by);

    let mut sources_status: Vec<SourceStatus> = Vec::new();
    let mut merged: Vec<MovieItem> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut seen_imdb: HashSet<String> = HashSet::new();

    let push_unique = |item: MovieItem,
                       merged: &mut Vec<MovieItem>,
                       seen_ids: &mut HashSet<String>,
                       seen_imdb: &mut HashSet<String>| {
        if !seen_ids.insert(item.id.clone()) {
            return;
        }
        if let Some(ref im) = item.imdb_id {
            if !seen_imdb.insert(im.clone()) {
                return;
            }
        }
        merged.push(item);
    };

    let jw_started = Instant::now();
    let mut jw_ok = false;
    let mut jw_count = 0usize;

    if !q_trimmed.is_empty() && !actor_trimmed.is_empty() {
        let f_title = build_jw_filter(req, region, Some(q_trimmed));
        let f_actor = build_jw_filter(req, region, Some(actor_trimmed));
        let (res_title, res_actor) = tokio::join!(
            query_justwatch_search(client, region, lang, 60, f_title, jw_sort),
            query_justwatch_search(client, region, lang, 60, f_actor, jw_sort)
        );
        if let Ok(items) = res_title {
            jw_ok = true;
            jw_count += items.len();
            for item in items {
                push_unique(item, &mut merged, &mut seen_ids, &mut seen_imdb);
            }
        }
        if let Ok(items) = res_actor {
            jw_ok = true;
            jw_count += items.len();
            for item in items {
                push_unique(item, &mut merged, &mut seen_ids, &mut seen_imdb);
            }
        }
    } else {
        let search_str = if !q_trimmed.is_empty() {
            Some(q_trimmed)
        } else if !actor_trimmed.is_empty() {
            Some(actor_trimmed)
        } else {
            None
        };
        let filter = build_jw_filter(req, region, search_str);
        match query_justwatch_search(client, region, lang, 60, filter, jw_sort).await {
            Ok(items) => {
                jw_ok = true;
                jw_count = items.len();
                for item in items {
                    push_unique(item, &mut merged, &mut seen_ids, &mut seen_imdb);
                }
            }
            Err(err) => {
                tracing::warn!("JustWatch search failed: {}", err);
            }
        }
    }

    sources_status.push(SourceStatus {
        name: "JustWatch".to_string(),
        ok: jw_ok,
        count: jw_count,
        elapsed_ms: jw_started.elapsed().as_millis(),
    });

    // Cinemeta fallback when JustWatch failed or returned very few results on a title query without strict platform filtering.
    let need_cinemeta = (!jw_ok || merged.len() < 3)
        && !q_trimmed.is_empty()
        && req.platforms.is_empty();

    if need_cinemeta {
        let cm_started = Instant::now();
        match search_cinemeta_fallback(client, q_trimmed).await {
            Ok(items) => {
                let cnt = items.len();
                for item in items {
                    push_unique(item, &mut merged, &mut seen_ids, &mut seen_imdb);
                }
                sources_status.push(SourceStatus {
                    name: "Cinemeta".to_string(),
                    ok: true,
                    count: cnt,
                    elapsed_ms: cm_started.elapsed().as_millis(),
                });
            }
            Err(err) => {
                tracing::warn!("Cinemeta fallback failed: {}", err);
                sources_status.push(SourceStatus {
                    name: "Cinemeta".to_string(),
                    ok: false,
                    count: 0,
                    elapsed_ms: cm_started.elapsed().as_millis(),
                });
            }
        }
    }

    let total_fetched = merged.len();

    // Post-filtering
    let actor_norm = normalize_text_ascii(actor_trimmed);
    let query_norm = normalize_text_ascii(q_trimmed);
    let req_pkgs = resolve_platform_packages(&req.platforms, region);
    let mon_mode = req.monetization.trim().to_ascii_lowercase();
    let use_netflix_rating = req.rating_system.eq_ignore_ascii_case("netflix");

    let mut filtered: Vec<MovieItem> = merged
        .into_iter()
        .filter(|m| {
            // Actor check on full credits
            if !actor_norm.is_empty() && !movie_matches_person(m, &actor_norm) {
                return false;
            }
            // When both query and actor are supplied, also ensure title matches query
            if !query_norm.is_empty() && !actor_norm.is_empty() && !movie_matches_title(m, &query_norm) {
                return false;
            }
            // Must have rating check
            if req.must_have_rating {
                if use_netflix_rating {
                    if m.netflix_stars.is_none() || (m.imdb_score.is_none() && m.tmdb_score.is_none()) {
                        return false;
                    }
                } else if m.imdb_score.is_none() {
                    return false;
                }
            }
            // Rating threshold check
            if let Some(min_r) = req.min_rating.filter(|v| v.is_finite() && *v > 0.0) {
                if use_netflix_rating {
                    if m.netflix_stars.map(|s| s + 1e-6 < min_r).unwrap_or(true) {
                        return false;
                    }
                } else if m.imdb_score.map(|s| s + 1e-6 < min_r).unwrap_or(true) {
                    return false;
                }
            }
            // Min votes check
            if let Some(min_v) = req.min_votes.filter(|v| *v > 0) {
                if m.imdb_votes.map(|v| v < min_v).unwrap_or(true) {
                    return false;
                }
            }
            // Platform + monetization verification on offers
            if !req_pkgs.is_empty() || mon_mode != "all" {
                let matches_offer = m.offers.iter().any(|off| {
                    let pkg_ok = req_pkgs.is_empty()
                        || req_pkgs.contains(&off.provider_short.as_str());
                    let mon_ok = match mon_mode.as_str() {
                        "flatrate" => off.monetization_type == "FLATRATE",
                        "free" => off.monetization_type == "FREE" || off.monetization_type == "ADS",
                        "rent_buy" => off.monetization_type == "RENT" || off.monetization_type == "BUY",
                        _ => true,
                    };
                    pkg_ok && mon_ok
                });
                if !matches_offer {
                    return false;
                }
            }
            // Origin country verification
            if !req.origin_countries.is_empty() {
                let wanted: Vec<String> = req
                    .origin_countries
                    .iter()
                    .map(|c| c.trim().to_ascii_uppercase())
                    .collect();
                if !m.origin_countries.iter().any(|c| wanted.contains(c)) {
                    return false;
                }
            }
            // Year range verification
            if let Some(yf) = req.year_from {
                if m.year.map(|y| y < yf).unwrap_or(true) {
                    return false;
                }
            }
            if let Some(yt) = req.year_to {
                if m.year.map(|y| y > yt).unwrap_or(true) {
                    return false;
                }
            }
            true
        })
        .collect();

    // Smart vote floor when sorting top IMDb score in browse mode (no query/actor & no user min_votes)
    let sort_lower = req.sort_by.trim().to_ascii_lowercase();
    if sort_lower == "imdb_score"
        && req.min_votes.unwrap_or(0) == 0
        && q_trimmed.is_empty()
        && actor_trimmed.is_empty()
    {
        let well_voted: Vec<MovieItem> = filtered
            .iter()
            .filter(|m| m.imdb_votes.unwrap_or(0) >= 2500)
            .cloned()
            .collect();
        if well_voted.len() >= 10 {
            filtered = well_voted;
        }
    }

    // Sorting
    match sort_lower.as_str() {
        "imdb_score" => {
            filtered.sort_by(|a, b| {
                b.imdb_score
                    .unwrap_or(0.0)
                    .partial_cmp(&a.imdb_score.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.imdb_votes.unwrap_or(0).cmp(&a.imdb_votes.unwrap_or(0)))
            });
        }
        "netflix_match" => {
            filtered.sort_by(|a, b| {
                b.netflix_match_pct
                    .unwrap_or(0)
                    .cmp(&a.netflix_match_pct.unwrap_or(0))
                    .then_with(|| b.imdb_votes.unwrap_or(0).cmp(&a.imdb_votes.unwrap_or(0)))
            });
        }
        "most_watched" => {
            filtered.sort_by(|a, b| {
                b.imdb_votes
                    .unwrap_or(0)
                    .cmp(&a.imdb_votes.unwrap_or(0))
                    .then_with(|| {
                        b.imdb_score
                            .unwrap_or(0.0)
                            .partial_cmp(&a.imdb_score.unwrap_or(0.0))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
            });
        }
        "release_year" => {
            filtered.sort_by(|a, b| {
                b.year
                    .unwrap_or(0)
                    .cmp(&a.year.unwrap_or(0))
                    .then_with(|| b.imdb_votes.unwrap_or(0).cmp(&a.imdb_votes.unwrap_or(0)))
            });
        }
        _ => {
            if !req.mood.trim().is_empty() && q_trimmed.is_empty() && actor_trimmed.is_empty() {
                let mood_slug = req.mood.trim().to_ascii_lowercase();
                filtered.sort_by(|a, b| {
                    mood_relevance_score(b, &mood_slug)
                        .cmp(&mood_relevance_score(a, &mood_slug))
                        .then_with(|| {
                            b.tmdb_popularity
                                .partial_cmp(&a.tmdb_popularity)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                });
            }
        }
    }

    // Trim cast to top 10 in search results to keep payload snappy
    for item in &mut filtered {
        item.cast.truncate(10);
    }

    let filtered_count = filtered.len();
    filtered.truncate(limit);

    MovieSearchResponse {
        results: filtered,
        total_fetched,
        filtered_count,
        region: region.to_string(),
        lang: lang.to_string(),
        elapsed_ms: started.elapsed().as_millis(),
        sources_status,
        cached: false,
        updated_at: Utc::now().to_rfc3339(),
    }
}

/* ---------------- Detail Orchestration ---------------- */

fn merge_cinemeta_enrichment(
    movie: &mut MovieItem,
    meta: CinemetaMeta,
) -> (Option<String>, Vec<String>, Option<f64>) {
    if movie.poster_url.is_none() {
        movie.poster_url = meta.poster;
    }
    if let Some(bg) = meta.background.filter(|u| u.starts_with("http://") || u.starts_with("https://")) {
        if movie.screenshots.len() < 4 && !movie.screenshots.contains(&bg) {
            movie.screenshots.push(bg.clone());
        }
        if movie.backdrop_url.is_none() {
            movie.backdrop_url = Some(bg);
        }
    }
    if movie.synopsis.is_empty() {
        if let Some(desc) = meta.description.filter(|d| !d.trim().is_empty()) {
            movie.synopsis = desc;
        }
    }
    if movie.directors.is_empty() && !meta.director.is_empty() {
        movie.directors = meta.director;
    }
    if movie.cast.is_empty() && !meta.cast.is_empty() {
        movie.cast = meta
            .cast
            .into_iter()
            .take(16)
            .map(|name| CastMember {
                name,
                character: None,
            })
            .collect();
    }

    let mut seen_yt: HashSet<String> = movie.trailers.iter().map(|t| t.youtube_id.clone()).collect();
    for ts in meta.trailer_streams {
        if let Some(yt) = ts.yt_id.filter(|id| is_valid_yt_id(id)) {
            if seen_yt.insert(yt.clone()) {
                movie.trailers.push(TrailerClip {
                    title: ts
                        .title
                        .unwrap_or_else(|| format!("{} Trailer", movie.title)),
                    youtube_url: format!("https://www.youtube.com/watch?v={}", yt),
                    embed_url: format!("https://www.youtube.com/embed/{}", yt),
                    youtube_id: yt,
                });
            }
        }
    }
    for t in meta.trailers {
        if let Some(yt) = t.source.filter(|id| is_valid_yt_id(id)) {
            if seen_yt.insert(yt.clone()) {
                movie.trailers.push(TrailerClip {
                    title: format!("{} Trailer", movie.title),
                    youtube_url: format!("https://www.youtube.com/watch?v={}", yt),
                    embed_url: format!("https://www.youtube.com/embed/{}", yt),
                    youtube_id: yt,
                });
            }
        }
    }

    if let Some(ref im) = movie.imdb_id {
        if !movie.sources.iter().any(|s| s.name == "Cinemeta") {
            movie.sources.push(SourceLink {
                name: "Cinemeta".to_string(),
                url: format!("{}/meta/movie/{}.json", CINEMETA_BASE, im),
            });
        }
    }

    let awards = meta.awards.filter(|a| !a.trim().is_empty());
    let writers = meta.writer;
    let trakt = meta.popularities.and_then(|p| p.trakt);
    (awards, writers, trakt)
}

async fn execute_detail(
    client: &Client,
    query: &MovieDetailQuery,
    region: &'static str,
    lang: &'static str,
) -> Result<MovieDetailResponse, String> {
    let jw_id_opt = query
        .jw_id
        .as_deref()
        .map(str::trim)
        .filter(|s| s.starts_with("tm"))
        .map(str::to_string);
    let imdb_id_opt = query
        .imdb_id
        .as_deref()
        .map(str::trim)
        .filter(|s| s.starts_with("tt"))
        .map(str::to_string);

    let mut movie: Option<MovieItem> = None;
    let mut similar_movies: Vec<MovieItem> = Vec::new();
    let mut cinemeta_meta: Option<CinemetaMeta> = None;

    if let Some(ref jw_id) = jw_id_opt {
        if let Some(ref im_id) = imdb_id_opt {
            let (jw_res, cm_res) = tokio::join!(
                query_justwatch_detail(client, jw_id, region, lang),
                fetch_cinemeta_meta(client, im_id)
            );
            if let Ok((m, sim)) = jw_res {
                movie = Some(m);
                similar_movies = sim;
            }
            cinemeta_meta = cm_res;
        } else if let Ok((m, sim)) = query_justwatch_detail(client, jw_id, region, lang).await {
            if let Some(ref discovered_imdb) = m.imdb_id {
                cinemeta_meta = fetch_cinemeta_meta(client, discovered_imdb).await;
            }
            movie = Some(m);
            similar_movies = sim;
        }
    }

    if movie.is_none() {
        if let Some(ref im_id) = imdb_id_opt {
            if cinemeta_meta.is_none() {
                cinemeta_meta = fetch_cinemeta_meta(client, im_id).await;
            }
            if let Some(ref cm) = cinemeta_meta {
                movie = map_cinemeta_to_movie(cm);
            }
            // Try finding JustWatch offers/screenshots via title search if available
            let lookup_title = query
                .title
                .as_deref()
                .or_else(|| movie.as_ref().map(|m| m.title.as_str()));
            if let Some(t) = lookup_title {
                let f = json!({ "objectTypes": ["MOVIE"], "searchQuery": t });
                if let Ok(items) = query_justwatch_search(client, region, lang, 6, f, "POPULAR").await {
                    if let Some(matched) = items.into_iter().find(|it| {
                        it.imdb_id.as_deref() == Some(im_id.as_str())
                            || (query.year.is_some() && it.year == query.year)
                    }) {
                        let matched_id = matched.id.clone();
                        movie = Some(matched);
                        if let Ok((full_m, sim)) =
                            query_justwatch_detail(client, &matched_id, region, lang).await
                        {
                            movie = Some(full_m);
                            similar_movies = sim;
                        }
                    }
                }
            }
        }
    }

    let mut movie = movie.ok_or_else(|| "Movie not found".to_string())?;

    let mut awards = None;
    let mut writers = Vec::new();
    let mut trakt_popularity = None;

    if let Some(cm) = cinemeta_meta {
        let (aw, wr, tr) = merge_cinemeta_enrichment(&mut movie, cm);
        awards = aw;
        writers = wr;
        trakt_popularity = tr;
    }

    let wiki = fetch_wikipedia_summary(
        client,
        &movie.title,
        movie.original_title.as_deref(),
        movie.year,
        lang,
    )
    .await;

    let (wiki_extract, wiki_url) = match wiki {
        Some((ex, url)) => {
            if !movie.sources.iter().any(|s| s.name == "Wikipedia") {
                movie.sources.push(SourceLink {
                    name: "Wikipedia".to_string(),
                    url: url.clone(),
                });
            }
            (Some(ex), Some(url))
        }
        None => (None, None),
    };

    Ok(MovieDetailResponse {
        movie,
        awards,
        writers,
        wiki_extract,
        wiki_url,
        trakt_popularity,
        similar_movies,
        cached: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_platform_packages_region_aware() {
        let plats = vec!["netflix".into(), "prime".into(), "apple".into()];
        assert_eq!(
            resolve_platform_packages(&plats, "US"),
            vec!["nfx", "amp", "atp", "itu"]
        );
        assert_eq!(
            resolve_platform_packages(&plats, "TR"),
            vec!["nfx", "prv", "itu"]
        );
        let tr_only = vec!["puhutv".into(), "tabii".into(), "exxen".into()];
        assert_eq!(
            resolve_platform_packages(&tr_only, "TR"),
            vec!["pht", "tab", "exn"]
        );
    }

    #[test]
    fn test_image_url_formatting() {
        assert_eq!(
            format_jw_poster(Some("/poster/302449148/{profile}/inception.{format}")),
            Some("https://images.justwatch.com/poster/302449148/s592/inception.webp".into())
        );
        assert_eq!(
            format_jw_backdrop(Some("/backdrop/275196554/{profile}/inception.{format}")),
            Some("https://images.justwatch.com/backdrop/275196554/s1440/inception.webp".into())
        );
        assert_eq!(
            format_jw_icon(Some("/icon/207360008/{profile}/netflix.{format}")),
            Some("https://images.justwatch.com/icon/207360008/s100/netflix.png".into())
        );
    }

    #[test]
    fn test_scientific_imdb_votes_deserialization() {
        let raw = r#"{
            "imdbScore": 8.8,
            "imdbVotes": 2.891964e+06,
            "tmdbScore": 8.373,
            "tmdbPopularity": 49.7497,
            "tomatoMeter": 86,
            "certifiedFresh": true,
            "jwRating": 0.9704689
        }"#;
        let scoring: JwScoring = serde_json::from_str(raw).expect("valid scoring json");
        assert_eq!(scoring.imdb_votes.map(|v| v.round() as u64), Some(2891964));
        assert_eq!(scoring.tomato_meter.map(|v| v.round() as u32), Some(86));
        let (pct, stars) = compute_netflix_metrics(
            scoring.jw_rating,
            scoring.imdb_score,
            scoring.tmdb_score,
            scoring.tomato_meter,
        );
        assert_eq!(pct, Some(97));
        assert_eq!(stars, Some(4.9));
    }

    #[test]
    fn test_netflix_metrics_fallback_without_jw_rating() {
        let (pct, stars) = compute_netflix_metrics(None, Some(8.0), Some(8.0), None);
        assert_eq!(pct, Some(80));
        assert_eq!(stars, Some(4.0));
    }

    #[test]
    fn test_mood_inference() {
        let genres = vec!["scf".to_string(), "trl".to_string()];
        let moods = infer_moods(
            &genres,
            "A thief infiltrates dreams and subconscious reality twists.",
            Some(8.8),
        );
        assert!(moods.contains(&"mind_bending".to_string()));
    }

    #[test]
    fn test_turkish_diacritic_actor_matching() {
        let norm = normalize_text_ascii("Şener Şen & Haluk Bilginer");
        assert_eq!(norm, "sener sen & haluk bilginer");
        assert!(norm.contains(&normalize_text_ascii("Sener Sen")));
        assert!(norm.contains(&normalize_text_ascii("Şener Şen")));
    }
}
