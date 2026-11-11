//! Correspondência entre faixas do Spotify e vídeos do YouTube.
//!
//! O Spotify é usado apenas como fonte de metadados. Para cada faixa,
//! pesquisamos no YouTube e escolhemos o resultado com maior pontuação,
//! considerando título, artista, canal, duração e palavras suspeitas
//! (karaokê, cover, remix, versão acelerada etc.).
//!
//! A pontuação é explicável e testável — não dependemos de IA nem de serviços
//! externos além do próprio yt-dlp.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;

#[derive(Clone, Debug)]
pub struct Candidate {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub duration: Option<f64>,
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct Match {
    pub video_id: String,
    pub url: String,
    pub title: String,
    pub channel: String,
    pub duration: Option<f64>,
    pub confidence: f32,
    pub query: String,
}

impl Match {
    pub fn low_confidence(&self) -> bool {
        self.confidence < 0.45
    }
}

#[derive(Deserialize)]
struct SearchRoot {
    #[serde(default)]
    entries: Vec<SearchEntry>,
}

#[derive(Deserialize, Default)]
struct SearchEntry {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    uploader: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    url: Option<String>,
}

/// Pesquisa no YouTube usando o yt-dlp e devolve candidatos em modo "flat"
/// (rápido, sem resolver cada vídeo).
pub fn search(query: &str, limit: usize) -> Result<Vec<Candidate>> {
    let limit = limit.clamp(1, 10);
    let target = format!("ytsearch{limit}:{query}");
    let output = Command::new("yt-dlp")
        .args([
            "-J",
            "--flat-playlist",
            "--no-warnings",
            "--extractor-retries",
            "2",
            "--retries",
            "2",
            "--socket-timeout",
            "20",
            &target,
        ])
        .output()
        .context("Falha ao executar yt-dlp para buscar a faixa no YouTube.")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        bail!("Busca no YouTube falhou: {}", err.trim());
    }
    let root: SearchRoot =
        serde_json::from_slice(&output.stdout).context("Resposta de busca inválida do yt-dlp")?;
    let candidates: Vec<Candidate> = root
        .entries
        .into_iter()
        .filter(|e| !e.id.is_empty() && !e.title.is_empty())
        // Entradas de canal/playlist não são faixas.
        .filter(|e| e.duration.is_some() || e.url.as_deref().unwrap_or("").contains("watch"))
        .map(|e| {
            let channel = e
                .channel
                .or(e.uploader)
                .unwrap_or_default()
                .trim()
                .to_string();
            let url = e
                .url
                .filter(|u| u.starts_with("http"))
                .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={}", e.id));
            Candidate {
                id: e.id,
                title: e.title,
                channel,
                duration: e.duration,
                url,
            }
        })
        .collect();
    Ok(candidates)
}

/// Monta a consulta de busca a partir do título e dos artistas.
pub fn query_for(title: &str, artists: &[String]) -> String {
    let artist = artists.join(" ");
    let title = title.trim();
    if artist.trim().is_empty() {
        title.to_string()
    } else {
        format!("{artist} {title}")
    }
}

/// Busca e escolhe a melhor correspondência para uma faixa.
pub fn match_track(title: &str, artists: &[String], duration_secs: Option<f64>) -> Result<Match> {
    let query = query_for(title, artists);
    let candidates = search(&query, 6)?;
    let best = best_candidate(title, artists, duration_secs, &candidates);
    if let Some((candidate, confidence)) = best {
        if confidence >= 0.2 {
            return Ok(to_match(candidate, confidence, &query));
        }
    }

    // Segunda tentativa: só o título. Ajuda quando o artista está abreviado.
    if query.split_whitespace().count() > title.split_whitespace().count() {
        let candidates = search(title, 6)?;
        if let Some((candidate, confidence)) =
            best_candidate(title, artists, duration_secs, &candidates)
        {
            if confidence >= 0.2 {
                return Ok(to_match(candidate, confidence, title));
            }
        }
    }
    bail!("Nenhum resultado suficientemente parecido no YouTube para “{title}”.")
}

fn to_match(candidate: &Candidate, confidence: f32, query: &str) -> Match {
    Match {
        video_id: candidate.id.clone(),
        url: candidate.url.clone(),
        title: candidate.title.clone(),
        channel: candidate.channel.clone(),
        duration: candidate.duration,
        // A pontuação interna pode passar de 1 para manter a ordenação, mas a
        // confiança exibida é sempre uma porcentagem.
        confidence: confidence.min(1.0),
        query: query.to_string(),
    }
}

pub fn best_candidate<'a>(
    title: &str,
    artists: &[String],
    duration_secs: Option<f64>,
    candidates: &'a [Candidate],
) -> Option<(&'a Candidate, f32)> {
    candidates
        .iter()
        .map(|c| (c, score(title, artists, duration_secs, c)))
        .max_by(|(_, a), (_, b)| a.total_cmp(b))
}

/// Pontuação de 0 a 1. Valores acima de 0.45 são considerados confiáveis.
pub fn score(
    title: &str,
    artists: &[String],
    duration_secs: Option<f64>,
    candidate: &Candidate,
) -> f32 {
    let source_title = normalize_title(title);
    let cand_title = normalize_title(&candidate.title);
    let source_tokens = tokens(&source_title);
    let cand_tokens = tokens(&cand_title);
    if source_tokens.is_empty() || cand_tokens.is_empty() {
        return 0.0;
    }

    let lev = 1.0
        - levenshtein(&source_title, &cand_title) as f32
            / source_title
                .chars()
                .count()
                .max(cand_title.chars().count())
                .max(1) as f32;
    let jaccard = jaccard(&source_tokens, &cand_tokens);
    let mut score = 0.5 * lev.clamp(0.0, 1.0) + 0.5 * jaccard;

    // Artista no título ou no canal.
    let artist_line = fold(&artists.join(" "));
    let haystack = format!("{} {}", cand_title, fold(&candidate.channel));
    let artist_hit = artists.iter().any(|artist| {
        let a = fold(artist);
        !a.is_empty() && (haystack.contains(&a) || haystack.contains(&a.replace(' ', "")))
    });
    if artist_hit {
        score += 0.18;
    }
    let channel = fold(&candidate.channel);
    let artist_all = fold(&artist_line);
    if !artist_all.is_empty()
        && (channel.contains(&artist_all) || artist_all.contains(&channel))
        && !channel.is_empty()
    {
        score += 0.1;
    }
    if channel.ends_with("topic") || channel.contains(" - topic") {
        score += 0.08;
    }
    if channel.contains("vevo") {
        score += 0.04;
    }

    // Duração é um dos sinais mais fortes contra versões extendidas/ao vivo.
    if let (Some(source), Some(candidate_secs)) = (duration_secs, candidate.duration) {
        let delta = (source - candidate_secs).abs();
        score += match delta {
            d if d <= 2.0 => 0.18,
            d if d <= 5.0 => 0.14,
            d if d <= 10.0 => 0.08,
            d if d <= 20.0 => 0.0,
            d if d <= 45.0 => -0.12,
            _ => -0.3,
        };
    }

    // Penaliza versões que claramente não são a faixa original.
    const BAD: &[&str] = &[
        "karaoke",
        "instrumental",
        "nightcore",
        "sped up",
        "speed up",
        "slowed",
        "reverb",
        "8d",
        "reaction",
        "parody",
        "tutorial",
        "aula",
        "cover",
        "remix",
        "mashup",
        "medley",
        "live",
        "ao vivo",
        "traducao",
        "legendado",
        "legenda",
        "letra",
        "lyrics",
        "lyric",
    ];
    let source_folded = fold(title);
    let cand_folded = fold(&candidate.title);
    for word in BAD {
        if cand_folded.contains(word) && !source_folded.contains(word) {
            score -= 0.22;
        }
    }

    if cand_folded.contains("official") && !source_folded.contains("karaoke") {
        score += 0.03;
    }

    score.clamp(0.0, 1.3)
}

/// Remove acentos, caixa e pontuação para comparar títulos.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let mapped = match ch {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            'ý' | 'ÿ' => 'y',
            _ => ch.to_ascii_lowercase(),
        };
        out.push(mapped);
    }
    out
}

fn normalize_title(title: &str) -> String {
    let mut cleaned = String::with_capacity(title.len());
    for part in strip_known_suffixes(title) {
        cleaned.push_str(&part);
        cleaned.push(' ');
    }
    let folded = fold(&cleaned);
    let mut out = String::with_capacity(folded.len());
    let mut last_space = false;
    for ch in folded.chars() {
        if ch.is_alphanumeric() {
            out.push(ch);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim().to_string()
}

/// Remove trechos entre parênteses/colchetes e sufixos comuns de YouTube,
/// mantendo o restante do título intacto.
fn strip_known_suffixes(title: &str) -> Vec<String> {
    let mut pieces: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for ch in title.chars() {
        match ch {
            '(' | '[' | '{' => {
                depth += 1;
                if depth == 1 {
                    if !current.trim().is_empty() {
                        pieces.push(current.trim().to_string());
                    }
                    current.clear();
                }
            }
            ')' | ']' | '}' => {
                depth = depth.saturating_sub(1);
            }
            c if depth == 0 => current.push(c),
            _ => {}
        }
    }
    if !current.trim().is_empty() {
        pieces.push(current.trim().to_string());
    }
    if pieces.is_empty() {
        pieces.push(title.trim().to_string());
    }

    const SUFFIXES: &[&str] = &[
        "official music video",
        "official video",
        "official audio",
        "official lyric video",
        "lyric video",
        "lyrics",
        "legendado",
        "clipe oficial",
        "video oficial",
        "audio oficial",
        "hd",
        "4k",
        "remaster",
        "remastered",
        "visualizer",
        "explicit",
    ];
    pieces
        .into_iter()
        .map(|piece| {
            let mut result = piece;
            let mut changed = true;
            while changed {
                changed = false;
                let lower = fold(result.trim());
                for suffix in SUFFIXES {
                    if lower.ends_with(suffix) {
                        let keep = result.len().saturating_sub(suffix.len());
                        result = result[..keep]
                            .trim_end_matches(|c: char| {
                                c == '-' || c == '|' || c == ':' || c.is_whitespace()
                            })
                            .to_string();
                        changed = true;
                        break;
                    }
                }
            }
            result
        })
        .filter(|s| !s.trim().is_empty())
        .collect()
}

fn tokens(s: &str) -> Vec<String> {
    s.split_whitespace()
        .filter(|t| {
            !t.is_empty() && !matches!(*t, "the" | "a" | "an" | "o" | "e" | "de" | "da" | "do")
        })
        .map(|t| t.to_string())
        .collect()
}

fn jaccard(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut intersection = 0usize;
    for token in a {
        if b.contains(token) {
            intersection += 1;
        }
    }
    let union = a.len() + b.len() - intersection;
    intersection as f32 / union.max(1) as f32
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Resultado da correspondência em lote, na ordem original.
pub struct BatchMatch {
    pub matches: Vec<Option<Result<Match>>>,
    pub cancelled: bool,
}

/// Executa as buscas em paralelo, preservando a ordem e reportando progresso.
pub fn match_all<F>(
    tracks: &[(String, Vec<String>, Option<f64>)],
    workers: usize,
    cancel: &AtomicBool,
    mut on_result: F,
) -> BatchMatch
where
    F: FnMut(usize, &Result<Match>),
{
    let total = tracks.len();
    let mut matches: Vec<Option<Result<Match>>> = (0..total).map(|_| None).collect();
    if total == 0 {
        return BatchMatch {
            matches,
            cancelled: false,
        };
    }
    let workers = workers.clamp(1, 6).min(total);
    let next = AtomicUsize::new(0);
    let (tx, rx) = channel::<(usize, Result<Match>)>();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let tx = tx.clone();
            let next = &next;
            scope.spawn(move || loop {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= tracks.len() {
                    break;
                }
                let (title, artists, duration) = &tracks[index];
                let result = match_track(title, artists, *duration);
                if tx.send((index, result)).is_err() {
                    break;
                }
            });
        }
        drop(tx);
        for (index, result) in rx {
            on_result(index, &result);
            matches[index] = Some(result);
        }
    });
    BatchMatch {
        matches,
        cancelled: cancel.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(title: &str, channel: &str, duration: Option<f64>) -> Candidate {
        Candidate {
            id: "dQw4w9WgXcQ".into(),
            title: title.into(),
            channel: channel.into(),
            duration,
            url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
        }
    }

    #[test]
    fn exact_match_scores_high() {
        let c = candidate(
            "Rick Astley - Never Gonna Give You Up (Official Video)",
            "Rick Astley",
            Some(213.0),
        );
        let score = score(
            "Never Gonna Give You Up",
            &["Rick Astley".into()],
            Some(213.573),
            &c,
        );
        assert!(score > 0.8, "score = {score}");
    }

    #[test]
    fn karaoke_scores_lower() {
        let original = candidate(
            "Rick Astley - Never Gonna Give You Up (Official Video)",
            "Rick Astley",
            Some(213.0),
        );
        let karaoke = candidate(
            "Never Gonna Give You Up (Karaoke Version)",
            "Karaoke Hits",
            Some(213.0),
        );
        let original_score = score(
            "Never Gonna Give You Up",
            &["Rick Astley".into()],
            Some(213.573),
            &original,
        );
        let karaoke_score = score(
            "Never Gonna Give You Up",
            &["Rick Astley".into()],
            Some(213.573),
            &karaoke,
        );
        assert!(original_score > karaoke_score + 0.2);
    }

    #[test]
    fn translated_lyrics_score_lower() {
        let original = candidate(
            "Ashe - Stop The Wedding! (Official Video)",
            "Ashe",
            Some(209.0),
        );
        let translated = candidate(
            "Ashe - Stop the wedding (TRADUÇÃO / LEGENDADO)",
            "Enny",
            Some(197.0),
        );
        let original_score = score(
            "Stop The Wedding!",
            &["Ashe".into()],
            Some(198.84),
            &original,
        );
        let translated_score = score(
            "Stop The Wedding!",
            &["Ashe".into()],
            Some(198.84),
            &translated,
        );
        assert!(
            original_score > translated_score,
            "oficial={original_score} tradução={translated_score}"
        );
    }

    #[test]
    fn duration_mismatch_penalized() {
        let close = candidate("Song", "Artist - Topic", Some(200.0));
        let far = candidate("Song", "Artist - Topic", Some(400.0));
        let a = score("Song", &["Artist".into()], Some(201.0), &close);
        let b = score("Song", &["Artist".into()], Some(201.0), &far);
        assert!(a > b);
    }

    #[test]
    fn accents_are_ignored() {
        assert_eq!(fold("Ação, Coração!"), "acao, coracao!");
        let c = candidate("Ação (Official Video)", "Artista", Some(180.0));
        let s = score("Ação", &["Artista".into()], Some(180.0), &c);
        assert!(s > 0.8, "score = {s}");
    }

    #[test]
    fn strips_youtube_suffixes() {
        assert_eq!(
            normalize_title("Never Gonna Give You Up (Official Video)"),
            "never gonna give you up"
        );
        assert_eq!(normalize_title("Song - Official Audio"), "song");
        assert_eq!(normalize_title("Song (Live at Wembley)"), "song");
    }

    #[test]
    fn best_candidate_picks_official() {
        let candidates = vec![
            candidate("Song (Karaoke)", "Karaoke", Some(200.0)),
            candidate("Song (Official Audio)", "Artist - Topic", Some(200.0)),
        ];
        let (best, confidence) =
            best_candidate("Song", &["Artist".into()], Some(200.0), &candidates).unwrap();
        assert_eq!(best.title, "Song (Official Audio)");
        assert!(confidence > 0.45);
    }

    #[test]
    fn low_confidence_flag() {
        let m = Match {
            video_id: "x".into(),
            url: "x".into(),
            title: "x".into(),
            channel: "x".into(),
            duration: None,
            confidence: 0.3,
            query: "x".into(),
        };
        assert!(m.low_confidence());
    }
}
