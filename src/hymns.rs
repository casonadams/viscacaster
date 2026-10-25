//! Church Hymns Catalog and Lyric Downloader
//!
//! Indexes all 341 hymns from The Church of Jesus Christ of Latter-day Saints
//! and provides automatic lookup by hymn number or title, with cached lyric extraction.

use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HymnEntry {
    pub num: u16,
    pub title: String,
    pub slug: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HymnLyrics {
    pub num: u16,
    pub title: String,
    pub sections: Vec<String>, // Each section is a verse or chorus
}

pub struct HymnCatalog {
    pub hymns: Vec<HymnEntry>,
}

impl HymnCatalog {
    pub fn load_embedded() -> Self {
        let json_str = include_str!("hymns_index.json");
        let hymns: Vec<HymnEntry> = serde_json::from_str(json_str).unwrap_or_default();
        Self { hymns }
    }

    pub fn find_by_number(&self, num: u16) -> Option<&HymnEntry> {
        self.hymns.iter().find(|h| h.num == num)
    }

    pub fn search(&self, query: &str) -> Vec<&HymnEntry> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }

        // Check if query is a number
        if let Ok(num) = q.parse::<u16>() {
            if let Some(h) = self.find_by_number(num) {
                return vec![h];
            }
        }

        // Keyword title match
        self.hymns
            .iter()
            .filter(|h| h.title.to_lowercase().contains(&q) || h.num.to_string().contains(&q))
            .take(12)
            .collect()
    }
}

fn hymn_cache_path(num: u16) -> Option<PathBuf> {
    dirs::config_dir().map(|mut p| {
        p.push("viscacaster");
        p.push("hymns");
        p.push(format!("{}.json", num));
        p
    })
}

/// Fetches lyrics for a hymn, reading from local cache first, or downloading from churchofjesuschrist.org
pub fn get_hymn_lyrics(hymn: &HymnEntry) -> Result<HymnLyrics, String> {
    // 1. Check local disk cache
    if let Some(cache_file) = hymn_cache_path(hymn.num) {
        if cache_file.exists() {
            if let Ok(content) = fs::read_to_string(&cache_file) {
                if let Ok(lyrics) = serde_json::from_str::<HymnLyrics>(&content) {
                    return Ok(lyrics);
                }
            }
        }
    }

    // 2. Fetch from official Gospel Library website
    let url = format!(
        "https://www.churchofjesuschrist.org/study/manual/hymns/{}?lang=eng",
        hymn.slug
    );

    let html = fetch_url(&url)?;
    let sections = parse_hymn_html(&html);

    if sections.is_empty() {
        return Err("No lyric sections could be extracted from page".into());
    }

    let lyrics = HymnLyrics {
        num: hymn.num,
        title: hymn.title.clone(),
        sections,
    };

    // 3. Save to local disk cache for fast offline access
    if let Some(cache_file) = hymn_cache_path(hymn.num) {
        if let Some(parent) = cache_file.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&lyrics) {
            let _ = fs::write(cache_file, json);
        }
    }

    Ok(lyrics)
}

fn fetch_url(url: &str) -> Result<String, String> {
    // Lightweight blocking HTTP fetch using curl command
    let output = std::process::Command::new("curl")
        .args(["-sL", "-A", "Mozilla/5.0", url])
        .output()
        .map_err(|e| format!("curl execution failed: {}", e))?;

    if !output.status.success() {
        return Err(format!("HTTP request failed with status: {}", output.status));
    }

    String::from_utf8(output.stdout).map_err(|e| format!("UTF-8 decode error: {}", e))
}

fn parse_hymn_html(html: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut cursor = 0;

    while let Some(stanza_start) = html[cursor..].find("<div class=\"stanza\"")
        .or_else(|| html[cursor..].find("<div class=\"chorus\""))
    {
        let abs_start = cursor + stanza_start;
        let is_chorus = html[abs_start..].starts_with("<div class=\"chorus\"");

        if let Some(stanza_end) = html[abs_start..].find("</div>") {
            let block = &html[abs_start..abs_start + stanza_end];
            let mut lines = Vec::new();

            let mut line_cursor = 0;
            while let Some(p_start) = block[line_cursor..].find("<p class=\"line\"") {
                let abs_p_start = line_cursor + p_start;
                if let Some(p_content_start) = block[abs_p_start..].find('>') {
                    let content_start = abs_p_start + p_content_start + 1;
                    if let Some(p_end) = block[content_start..].find("</p>") {
                        let raw_line = &block[content_start..content_start + p_end];
                        let clean = clean_html(raw_line);
                        if !clean.is_empty() {
                            lines.push(clean);
                        }
                        line_cursor = content_start + p_end;
                        continue;
                    }
                }
                break;
            }

            if !lines.is_empty() {
                let prefix = if is_chorus { "[Chorus]\n" } else { "" };
                sections.push(format!("{}{}", prefix, lines.join("\n")));
            }

            cursor = abs_start + stanza_end + 6;
        } else {
            break;
        }
    }

    sections
}

fn clean_html(input: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;

    for ch in input.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            output.push(ch);
        }
    }

    // Strip leading verse numbers e.g. "1. "
    let trimmed = output.trim();
    if let Some(pos) = trimmed.find(". ") {
        if trimmed[..pos].chars().all(|c| c.is_ascii_digit()) {
            return trimmed[pos + 2..].trim().to_string();
        }
    }

    // HTML entity decodes
    trimmed
        .replace("&rsquo;", "'")
        .replace("&lsquo;", "'")
        .replace("&rdquo;", "\"")
        .replace("&ldquo;", "\"")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&amp;", "&")
        .replace("&#39;", "'")
}
