//! Camada de download: monta os argumentos do yt-dlp, acompanha o progresso
//! e devolve os arquivos para a etapa de organização.

use crate::config::Config;
use crate::util;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Tipos de mídia e preferências
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MediaKind {
    #[default]
    Musica,
    Video,
    Spotify,
    Outro,
}

impl MediaKind {
    pub fn label(self) -> &'static str {
        match self {
            MediaKind::Musica => "Música",
            MediaKind::Video => "Vídeo",
            MediaKind::Spotify => "Spotify",
            MediaKind::Outro => "Outro site",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            MediaKind::Musica => "musica",
            MediaKind::Video => "video",
            MediaKind::Spotify => "spotify",
            MediaKind::Outro => "outro",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "video" => MediaKind::Video,
            "spotify" => MediaKind::Spotify,
            "outro" => MediaKind::Outro,
            _ => MediaKind::Musica,
        }
    }

    pub fn is_audio(self) -> bool {
        matches!(self, MediaKind::Musica | MediaKind::Spotify)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AudioFormat {
    Mp3,
    Flac,
    Opus,
    M4a,
    Wav,
    Original,
}

impl AudioFormat {
    pub const ALL: &'static [AudioFormat] = &[
        AudioFormat::Mp3,
        AudioFormat::Flac,
        AudioFormat::Opus,
        AudioFormat::M4a,
        AudioFormat::Wav,
        AudioFormat::Original,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AudioFormat::Mp3 => "MP3 — compatível com tudo",
            AudioFormat::Flac => "FLAC — sem perdas (máxima qualidade)",
            AudioFormat::Opus => "Opus — alta qualidade, arquivo pequeno",
            AudioFormat::M4a => "M4A/AAC — padrão Apple",
            AudioFormat::Wav => "WAV — sem perdas, arquivo grande",
            AudioFormat::Original => "Original — sem conversão",
        }
    }

    pub(crate) fn ffmpeg_name(self) -> Option<&'static str> {
        match self {
            AudioFormat::Mp3 => Some("mp3"),
            AudioFormat::Flac => Some("flac"),
            AudioFormat::Opus => Some("opus"),
            AudioFormat::M4a => Some("m4a"),
            AudioFormat::Wav => Some("wav"),
            AudioFormat::Original => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            AudioFormat::Mp3 => "mp3",
            AudioFormat::Flac => "flac",
            AudioFormat::Opus => "opus",
            AudioFormat::M4a => "m4a",
            AudioFormat::Wav => "wav",
            AudioFormat::Original => "original",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "flac" => AudioFormat::Flac,
            "opus" => AudioFormat::Opus,
            "m4a" => AudioFormat::M4a,
            "wav" => AudioFormat::Wav,
            "original" => AudioFormat::Original,
            _ => AudioFormat::Mp3,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VideoContainer {
    Mp4,
    Webm,
    Mkv,
    Original,
}

impl VideoContainer {
    pub const ALL: &'static [VideoContainer] = &[
        VideoContainer::Mp4,
        VideoContainer::Webm,
        VideoContainer::Mkv,
        VideoContainer::Original,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VideoContainer::Mp4 => "MP4 — compatível com tudo",
            VideoContainer::Webm => "WebM — menor tamanho",
            VideoContainer::Mkv => "MKV — máxima flexibilidade",
            VideoContainer::Original => "Original — sem conversão",
        }
    }

    pub(crate) fn name(self) -> Option<&'static str> {
        match self {
            VideoContainer::Mp4 => Some("mp4"),
            VideoContainer::Webm => Some("webm"),
            VideoContainer::Mkv => Some("mkv"),
            VideoContainer::Original => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            VideoContainer::Mp4 => "mp4",
            VideoContainer::Webm => "webm",
            VideoContainer::Mkv => "mkv",
            VideoContainer::Original => "original",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "webm" => VideoContainer::Webm,
            "mkv" => VideoContainer::Mkv,
            "original" => VideoContainer::Original,
            _ => VideoContainer::Mp4,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AudioQuality {
    Maxima,
    Alta,
    Media,
    Baixa,
}

impl AudioQuality {
    pub const ALL: &'static [AudioQuality] = &[
        AudioQuality::Maxima,
        AudioQuality::Alta,
        AudioQuality::Media,
        AudioQuality::Baixa,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AudioQuality::Maxima => "Qualidade máxima (recomendada)",
            AudioQuality::Alta => "Alta",
            AudioQuality::Media => "Média",
            AudioQuality::Baixa => "Baixa (arquivo menor)",
        }
    }

    pub(crate) fn value(self) -> &'static str {
        match self {
            AudioQuality::Maxima => "0",
            AudioQuality::Alta => "3",
            AudioQuality::Media => "6",
            AudioQuality::Baixa => "9",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            AudioQuality::Maxima => "maxima",
            AudioQuality::Alta => "alta",
            AudioQuality::Media => "media",
            AudioQuality::Baixa => "baixa",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "alta" => AudioQuality::Alta,
            "media" => AudioQuality::Media,
            "baixa" => AudioQuality::Baixa,
            _ => AudioQuality::Maxima,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VideoQuality {
    Melhor,
    Q2160,
    Q1440,
    Q1080,
    Q720,
    Q480,
    Q360,
    SomenteAudio,
}

impl VideoQuality {
    pub const ALL: &'static [VideoQuality] = &[
        VideoQuality::Melhor,
        VideoQuality::Q2160,
        VideoQuality::Q1440,
        VideoQuality::Q1080,
        VideoQuality::Q720,
        VideoQuality::Q480,
        VideoQuality::Q360,
        VideoQuality::SomenteAudio,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VideoQuality::Melhor => "Melhor disponível",
            VideoQuality::Q2160 => "2160p (4K)",
            VideoQuality::Q1440 => "1440p (2K)",
            VideoQuality::Q1080 => "1080p (Full HD)",
            VideoQuality::Q720 => "720p (HD)",
            VideoQuality::Q480 => "480p",
            VideoQuality::Q360 => "360p",
            VideoQuality::SomenteAudio => "Somente áudio",
        }
    }

    pub(crate) fn height(self) -> Option<u32> {
        match self {
            VideoQuality::Melhor => None,
            VideoQuality::Q2160 => Some(2160),
            VideoQuality::Q1440 => Some(1440),
            VideoQuality::Q1080 => Some(1080),
            VideoQuality::Q720 => Some(720),
            VideoQuality::Q480 => Some(480),
            VideoQuality::Q360 => Some(360),
            VideoQuality::SomenteAudio => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            VideoQuality::Melhor => "melhor",
            VideoQuality::Q2160 => "2160",
            VideoQuality::Q1440 => "1440",
            VideoQuality::Q1080 => "1080",
            VideoQuality::Q720 => "720",
            VideoQuality::Q480 => "480",
            VideoQuality::Q360 => "360",
            VideoQuality::SomenteAudio => "audio",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "2160" => VideoQuality::Q2160,
            "1440" => VideoQuality::Q1440,
            "1080" => VideoQuality::Q1080,
            "720" => VideoQuality::Q720,
            "480" => VideoQuality::Q480,
            "360" => VideoQuality::Q360,
            "audio" => VideoQuality::SomenteAudio,
            _ => VideoQuality::Melhor,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TrackSource {
    #[default]
    Youtube,
    Spotify,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CollectionKind {
    Album,
    Playlist,
    Artist,
}

impl CollectionKind {
    pub fn key(self) -> &'static str {
        match self {
            CollectionKind::Album => "album",
            CollectionKind::Playlist => "playlist",
            CollectionKind::Artist => "artist",
        }
    }

    pub fn from_key(s: &str) -> Option<Self> {
        match s {
            "album" => Some(CollectionKind::Album),
            "playlist" => Some(CollectionKind::Playlist),
            "artist" => Some(CollectionKind::Artist),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TrackMeta {
    /// ID na fonte (YouTube ou Spotify).
    pub id: String,
    /// ID do vídeo do YouTube usado para baixar uma faixa do Spotify.
    pub download_id: Option<String>,
    pub source: TrackSource,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub channel: Option<String>,
    pub year: Option<u32>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub playlist_index: Option<u32>,
    pub total_tracks: Option<u32>,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
    pub spotify_id: Option<String>,
    pub spotify_url: Option<String>,
    pub collection: Option<String>,
    pub collection_kind: Option<CollectionKind>,
    pub explicit: bool,
    pub match_confidence: Option<f32>,
    pub matched_title: Option<String>,
    pub matched_channel: Option<String>,
    pub match_query: Option<String>,
}

impl Default for TrackMeta {
    fn default() -> Self {
        Self {
            id: String::new(),
            download_id: None,
            source: TrackSource::Youtube,
            title: String::new(),
            artist: None,
            album: None,
            album_artist: None,
            channel: None,
            year: None,
            track_number: None,
            disc_number: None,
            playlist_index: None,
            total_tracks: None,
            duration: None,
            thumbnail: None,
            spotify_id: None,
            spotify_url: None,
            collection: None,
            collection_kind: None,
            explicit: false,
            match_confidence: None,
            matched_title: None,
            matched_channel: None,
            match_query: None,
        }
    }
}

impl TrackMeta {
    /// Prefixo usado nos arquivos dentro do diretório temporário.
    pub fn file_key(&self) -> &str {
        self.download_id.as_deref().unwrap_or(&self.id)
    }

    pub fn display_artist(&self) -> String {
        self.artist
            .clone()
            .or_else(|| self.channel.clone())
            .unwrap_or_else(|| "Artista desconhecido".into())
    }

    pub fn duration_secs(&self) -> Option<f64> {
        self.duration
    }

    pub fn is_spotify(&self) -> bool {
        self.source == TrackSource::Spotify
    }

    pub fn spotify_display(&self) -> String {
        let artist = self.display_artist();
        if artist.is_empty() {
            self.title.clone()
        } else {
            format!("{} — {}", artist, self.title)
        }
    }
}

#[derive(Clone, Debug)]
pub struct DownloadOptions {
    pub kind: MediaKind,
    pub audio_format: AudioFormat,
    pub video_container: VideoContainer,
    pub audio_quality: AudioQuality,
    pub video_quality: VideoQuality,
    pub convert: bool,
    pub embed_thumbnail: bool,
    pub embed_metadata: bool,
    pub organize: bool,
    pub number_tracks: bool,
    pub sponsorblock: bool,
    pub playlist_end: Option<u32>,
    /// Índices 1-based das faixas selecionadas em uma playlist.
    pub playlist_items: Option<Vec<usize>>,
    pub proxy: String,
    pub sleep_requests: u32,
    pub cookies_from_browser: String,
    pub limit_rate: String,
    pub download_archive: bool,
    pub write_m3u: bool,
    /// Spotify: o yt-dlp não deve embutir metadados/capa do YouTube; nós
    /// aplicamos os metadados e a capa corretos depois.
    pub tag_externally: bool,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            kind: MediaKind::Musica,
            audio_format: AudioFormat::Mp3,
            video_container: VideoContainer::Mp4,
            audio_quality: AudioQuality::Maxima,
            video_quality: VideoQuality::Melhor,
            convert: true,
            embed_thumbnail: true,
            embed_metadata: true,
            organize: true,
            number_tracks: true,
            sponsorblock: false,
            playlist_end: None,
            playlist_items: None,
            proxy: String::new(),
            sleep_requests: 0,
            cookies_from_browser: String::new(),
            limit_rate: String::new(),
            download_archive: false,
            write_m3u: false,
            tag_externally: false,
        }
    }
}

impl DownloadOptions {
    /// Aplica os padrões salvos sem mexer no tipo de mídia já escolhido.
    pub fn apply_config(&mut self, c: &Config) {
        self.audio_format = AudioFormat::from_key(&c.default_audio_format);
        self.audio_quality = AudioQuality::from_key(&c.default_audio_quality);
        self.video_container = VideoContainer::from_key(&c.default_video_container);
        self.video_quality = VideoQuality::from_key(&c.default_video_quality);
        self.sponsorblock = c.sponsorblock;
        self.proxy = c.proxy.clone();
        self.sleep_requests = c.sleep_requests;
        self.cookies_from_browser = c.cookies_from_browser.clone();
        self.limit_rate = c.limit_rate.clone();
        self.download_archive = c.download_archive;
        self.write_m3u = c.write_m3u;
    }
}

// ---------------------------------------------------------------------------
// Metadados do YouTube
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct YtJson {
    id: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    album_artist: Option<String>,
    uploader: Option<String>,
    channel: Option<String>,
    upload_date: Option<String>,
    release_year: Option<serde_json::Value>,
    track_number: Option<serde_json::Value>,
    disc_number: Option<serde_json::Value>,
    playlist_index: Option<u32>,
    playlist_count: Option<u32>,
    duration: Option<f64>,
    thumbnail: Option<String>,
    webpage_url: Option<String>,
    url: Option<String>,
    entries: Option<Vec<YtJson>>,
    #[serde(rename = "_type")]
    _type: Option<String>,
}

fn as_u32(v: &serde_json::Value) -> Option<u32> {
    match v {
        serde_json::Value::Number(n) => n.as_u64().map(|x| x as u32),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn parse_year(upload_date: &Option<String>) -> Option<u32> {
    upload_date
        .as_deref()
        .and_then(|d| d.get(..4))
        .and_then(|y| y.parse().ok())
}

fn id_from_url(url: &str) -> Option<String> {
    if let Some((_, rest)) = url.split_once("v=") {
        let id = rest
            .split(['&', '?', '#'])
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if !id.is_empty() {
            return Some(id);
        }
    }
    let trimmed = url.trim_end_matches('/');
    let last = trimmed.rsplit('/').next().unwrap_or("");
    if !last.is_empty() && !last.contains('.') {
        return Some(last.to_string());
    }
    None
}

fn to_track(j: &YtJson) -> Option<TrackMeta> {
    let id =
        j.id.clone()
            .or_else(|| {
                j.webpage_url
                    .as_deref()
                    .or(j.url.as_deref())
                    .and_then(id_from_url)
            })
            .filter(|s| !s.is_empty())?;
    Some(TrackMeta {
        id,
        download_id: None,
        source: TrackSource::Youtube,
        title: j.title.clone().unwrap_or_else(|| "Sem título".into()),
        artist: j.artist.clone(),
        album: j.album.clone(),
        album_artist: j.album_artist.clone(),
        channel: j.channel.clone().or_else(|| j.uploader.clone()),
        year: j
            .release_year
            .as_ref()
            .and_then(as_u32)
            .or_else(|| parse_year(&j.upload_date)),
        track_number: j.track_number.as_ref().and_then(as_u32),
        disc_number: j.disc_number.as_ref().and_then(as_u32),
        playlist_index: j.playlist_index,
        total_tracks: j.playlist_count,
        duration: j.duration,
        thumbnail: j.thumbnail.clone(),
        ..TrackMeta::default()
    })
}

#[derive(Clone, Debug)]
pub struct FetchedMetadata {
    pub title: String,
    pub tracks: Vec<TrackMeta>,
    pub is_playlist: bool,
}

/// Extrai metadados completos de uma URL do YouTube (ou de outro site aceito
/// pelo yt-dlp). Usa `--flat-playlist` para ser rápido em playlists grandes.
pub fn fetch_metadata_full(url: &str) -> Result<FetchedMetadata> {
    let output = Command::new("yt-dlp")
        .args([
            "-J",
            "--flat-playlist",
            "--no-warnings",
            "--extractor-retries",
            "3",
            "--retries",
            "3",
            "--socket-timeout",
            "30",
            url,
        ])
        .output()
        .context("Falha ao executar yt-dlp. Verifique se ele está instalado e no PATH.")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        bail!("yt-dlp falhou ao analisar o link: {}", err.trim());
    }
    let json: YtJson =
        serde_json::from_slice(&output.stdout).context("Resposta inválida do yt-dlp")?;

    if let Some(entries) = &json.entries {
        let total = entries.len() as u32;
        let mut tracks: Vec<TrackMeta> = entries.iter().filter_map(to_track).collect();
        if tracks.is_empty() {
            bail!("Playlist vazia ou sem faixas acessíveis.");
        }
        for (i, track) in tracks.iter_mut().enumerate() {
            if track.playlist_index.is_none() {
                track.playlist_index = Some((i + 1) as u32);
            }
            if track.total_tracks.is_none() && total > 1 {
                track.total_tracks = Some(total);
            }
        }
        Ok(FetchedMetadata {
            title: json
                .title
                .clone()
                .or_else(|| json.id.clone())
                .unwrap_or_else(|| "Playlist".into()),
            is_playlist: tracks.len() > 1
                || json
                    ._type
                    .as_deref()
                    .map(|t| t.contains("playlist"))
                    .unwrap_or(false),
            tracks,
        })
    } else {
        let track = to_track(&json).context("Não foi possível ler os metadados do link.")?;
        Ok(FetchedMetadata {
            title: track.title.clone(),
            is_playlist: false,
            tracks: vec![track],
        })
    }
}

/// Compatibilidade com chamadas antigas.
pub fn fetch_metadata(url: &str) -> Result<Vec<TrackMeta>> {
    Ok(fetch_metadata_full(url)?.tracks)
}

/// Lê os metadados completos de uma faixa a partir do `.info.json`
/// gerado pelo yt-dlp no diretório temporário de download.
pub fn info_json_track(tmpdir: &Path, id: &str) -> Option<TrackMeta> {
    let path = tmpdir.join(format!("{id}.info.json"));
    let data = std::fs::read(&path).ok()?;
    let j: YtJson = serde_json::from_slice(&data).ok()?;
    let mut track = to_track(&j)?;
    track.download_id = Some(id.to_string());
    Some(track)
}

// ---------------------------------------------------------------------------
// Progresso e execução
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Progress {
    Line(String),
    /// Começou o download de um arquivo (o id é o prefixo do arquivo).
    File {
        id: String,
    },
    PlaylistItem {
        index: usize,
        total: usize,
    },
    /// O yt-dlp pulou o item porque já consta no arquivo de histórico.
    Archived {
        id: String,
    },
    Percent {
        pct: f64,
        file: String,
        speed: Option<String>,
        eta: Option<String>,
    },
    Finished {
        success: bool,
        message: String,
    },
}

pub struct DownloadHandle {
    pub rx: Receiver<Progress>,
    pub child: Arc<Mutex<Child>>,
    pub tmpdir: PathBuf,
}

impl DownloadHandle {
    pub fn kill(&self) {
        if let Ok(mut guard) = self.child.lock() {
            let _ = guard.kill();
        }
    }
}

impl Drop for DownloadHandle {
    fn drop(&mut self) {
        // Garante que um pânico ou fechamento inesperado não deixe o yt-dlp
        // baixando em segundo plano.
        if let Ok(mut guard) = self.child.lock() {
            if let Ok(None) = guard.try_wait() {
                let _ = guard.kill();
            }
        }
    }
}

fn unique_tmpdir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("ytui-{}-{nanos}", std::process::id()))
}

fn normalize_file_key(path: &str) -> Option<String> {
    let trimmed = path.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return None;
    }
    let stem = Path::new(trimmed)
        .file_stem()?
        .to_string_lossy()
        .to_string();
    // Formatos intermediários do yt-dlp: 12345678901.f137
    if let Some((base, format)) = stem.rsplit_once('.') {
        if format.starts_with('f') && format[1..].chars().all(|c| c.is_ascii_digit()) {
            return Some(base.to_string());
        }
    }
    Some(stem)
}

fn parse_output_file(line: &str) -> Option<String> {
    for marker in ["Destination: ", "Merging formats into "] {
        if let Some(rest) = line.split(marker).nth(1) {
            if let Some(key) = normalize_file_key(rest) {
                return Some(key);
            }
        }
    }
    if line.contains(" has already been downloaded") {
        if let Some(rest) = line.split(" has already been downloaded").next() {
            if let Some(key) = normalize_file_key(rest.trim_start_matches("[download] ")) {
                return Some(key);
            }
        }
    }
    None
}

fn parse_archived(line: &str) -> Option<String> {
    if !line.contains("has already been recorded in the archive") {
        return None;
    }
    let prefix = line
        .split("has already been recorded in the archive")
        .next()?;
    let id = prefix
        .trim()
        .trim_start_matches("[download]")
        .trim()
        .trim_end_matches(':')
        .split(':')
        .next()?
        .trim();
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

fn parse_downloading_item(line: &str) -> Option<(usize, usize)> {
    let rest = line.split("Downloading item ").nth(1)?;
    let (index, rest) = rest.split_once(" of ")?;
    let index: usize = index.trim().parse().ok()?;
    let total: usize = rest
        .split_whitespace()
        .next()?
        .trim_end_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .ok()?;
    Some((index, total))
}

#[derive(Debug, PartialEq)]
pub struct PercentInfo {
    pub pct: f64,
    pub speed: Option<String>,
    pub eta: Option<String>,
}

pub fn parse_percent(line: &str) -> Option<PercentInfo> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("[download]") || !trimmed.contains('%') {
        return None;
    }
    let pct_pos = trimmed.find('%')?;
    let before = &trimmed[..pct_pos];
    let num_start = before
        .rfind(|c: char| !(c.is_ascii_digit() || c == '.' || c == ' '))
        .map(|i| i + 1)
        .unwrap_or(0);
    let pct: f64 = before[num_start..].trim().parse().ok()?;
    if !(0.0..=100.0).contains(&pct) {
        return None;
    }
    let speed = trimmed.split(" at ").nth(1).map(|s| {
        s.split(" ETA")
            .next()
            .unwrap_or(s)
            .split(" in ")
            .next()
            .unwrap_or(s)
            .trim()
            .to_string()
    });
    let speed = speed.filter(|s| !s.is_empty() && !s.to_ascii_lowercase().contains("unknown"));
    let eta = trimmed
        .split("ETA ")
        .nth(1)
        .map(|s| s.split([' ', '\n']).next().unwrap_or(s).trim().to_string());
    let eta = eta.filter(|s| !s.is_empty() && !s.to_ascii_lowercase().contains("unknown"));
    Some(PercentInfo { pct, speed, eta })
}

fn wait_child(shared: &Arc<Mutex<Child>>) -> std::io::Result<ExitStatus> {
    loop {
        {
            let mut guard = match shared.lock() {
                Ok(g) => g,
                Err(_) => {
                    return Err(std::io::Error::other("mutex do yt-dlp envenenado"));
                }
            };
            match guard.try_wait() {
                Ok(Some(status)) => return Ok(status),
                Ok(None) => {}
                Err(e) => return Err(e),
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn spawn_with_args(args: Vec<String>, tmpdir: PathBuf) -> Result<DownloadHandle> {
    if let Some(parent) = tmpdir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(&tmpdir)?;

    let mut child = match Command::new(&args[0])
        .args(&args[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmpdir);
            return Err(e).context("Falha ao iniciar o yt-dlp");
        }
    };
    let stdout = child.stdout.take().context("Sem stdout do yt-dlp")?;
    let stderr = child.stderr.take();
    let (tx, rx) = channel::<Progress>();
    let shared_child = Arc::new(Mutex::new(child));
    let child_wait = Arc::clone(&shared_child);
    let tx_out = tx.clone();
    let tx_err = tx.clone();

    let stderr_thread = stderr.map(|handle| {
        let tx = tx_err;
        std::thread::spawn(move || {
            let reader = BufReader::new(handle);
            for line in reader.lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    let _ = tx.send(Progress::Line(format!("ERRO: {line}")));
                }
            }
        })
    });

    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut current_file = String::new();
        for line in reader.lines().map_while(Result::ok) {
            if let Some(key) = parse_output_file(&line) {
                if key != current_file {
                    current_file = key.clone();
                    let _ = tx_out.send(Progress::File { id: key });
                }
            }
            if let Some((index, total)) = parse_downloading_item(&line) {
                let _ = tx_out.send(Progress::PlaylistItem { index, total });
            }
            if let Some(id) = parse_archived(&line) {
                let _ = tx_out.send(Progress::Archived { id });
            }
            if let Some(info) = parse_percent(&line) {
                let _ = tx_out.send(Progress::Percent {
                    pct: info.pct,
                    file: current_file.clone(),
                    speed: info.speed,
                    eta: info.eta,
                });
            }
            let _ = tx_out.send(Progress::Line(line));
        }
        if let Some(handle) = stderr_thread {
            let _ = handle.join();
        }
        let status = wait_child(&child_wait);
        match status {
            Ok(s) if s.success() => {
                let _ = tx_out.send(Progress::Finished {
                    success: true,
                    message: "Download concluído.".into(),
                });
            }
            Ok(s) => {
                let _ = tx_out.send(Progress::Finished {
                    success: false,
                    message: format!("yt-dlp terminou com código {}.", s.code().unwrap_or(-1)),
                });
            }
            Err(e) => {
                let _ = tx_out.send(Progress::Finished {
                    success: false,
                    message: format!("Falha ao aguardar o yt-dlp: {e}"),
                });
            }
        }
    });

    Ok(DownloadHandle {
        rx,
        child: shared_child,
        tmpdir,
    })
}

fn embed_thumbnail_supported(opts: &DownloadOptions) -> bool {
    match opts.kind {
        MediaKind::Spotify => false,
        MediaKind::Musica => !matches!(opts.audio_format, AudioFormat::Wav | AudioFormat::Original),
        _ => {
            if opts.video_quality == VideoQuality::SomenteAudio {
                true
            } else {
                matches!(
                    opts.video_container,
                    VideoContainer::Mp4 | VideoContainer::Mkv
                )
            }
        }
    }
}

fn format_args(opts: &DownloadOptions) -> Vec<String> {
    let mut args = Vec::new();
    let audio_only = opts.kind.is_audio()
        || (opts.kind != MediaKind::Spotify && opts.video_quality == VideoQuality::SomenteAudio);
    if audio_only {
        if let Some(fmt) = opts.audio_format.ffmpeg_name() {
            if opts.convert {
                args.extend([
                    "-x".into(),
                    "--audio-format".into(),
                    fmt.into(),
                    "--audio-quality".into(),
                    opts.audio_quality.value().into(),
                ]);
            } else {
                args.extend(["-f".into(), "bestaudio/best".into()]);
            }
        } else {
            args.extend(["-f".into(), "bestaudio/best".into()]);
        }
    } else {
        if let Some(h) = opts.video_quality.height() {
            args.extend([
                "-f".into(),
                format!("bestvideo[height<={h}]+bestaudio/best[height<={h}]/best"),
            ]);
        } else {
            args.extend(["-f".into(), "bestvideo*+bestaudio/best".into()]);
        }
        if let Some(container) = opts.video_container.name() {
            if opts.convert {
                args.extend(["--merge-output-format".into(), container.into()]);
            }
        }
    }
    args
}

fn common_args(opts: &DownloadOptions, tmpdir: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "yt-dlp".into(),
        "--newline".into(),
        "--no-warnings".into(),
        "--no-color".into(),
        "--restrict-filenames".into(),
        "--ignore-errors".into(),
        "--retries".into(),
        "5".into(),
        "--fragment-retries".into(),
        "5".into(),
        "--extractor-retries".into(),
        "3".into(),
        "--concurrent-fragments".into(),
        "4".into(),
        "--paths".into(),
        tmpdir.to_string_lossy().into_owned(),
        "-o".into(),
        "%(id)s.%(ext)s".into(),
    ];
    args.extend(format_args(opts));

    if !opts.tag_externally {
        if opts.embed_thumbnail {
            args.extend([
                "--write-thumbnail".into(),
                "--convert-thumbnails".into(),
                "jpg".into(),
            ]);
            if embed_thumbnail_supported(opts) {
                args.push("--embed-thumbnail".into());
            }
        }
        if opts.embed_metadata {
            args.push("--embed-metadata".into());
        }
    }

    if opts.sponsorblock && matches!(opts.kind, MediaKind::Video | MediaKind::Outro) {
        args.extend(["--sponsorblock-remove".into(), "all".into()]);
    }
    if let Some(n) = opts.playlist_end {
        args.extend(["--playlist-end".into(), n.to_string()]);
    }
    if let Some(items) = &opts.playlist_items {
        if !items.is_empty() {
            args.extend(["--playlist-items".into(), util::format_ranges(items)]);
        }
    }
    if !opts.proxy.trim().is_empty() {
        args.extend(["--proxy".into(), opts.proxy.trim().into()]);
    }
    if opts.sleep_requests > 0 {
        args.extend([
            "--sleep-requests".into(),
            opts.sleep_requests.to_string(),
            "--sleep-interval".into(),
            opts.sleep_requests.to_string(),
            "--max-sleep-interval".into(),
            (opts.sleep_requests * 3).to_string(),
        ]);
    }
    if !opts.cookies_from_browser.trim().is_empty() {
        args.extend([
            "--cookies-from-browser".into(),
            opts.cookies_from_browser.trim().into(),
        ]);
    }
    if !opts.limit_rate.trim().is_empty() {
        args.extend(["--limit-rate".into(), opts.limit_rate.trim().into()]);
    }
    if opts.download_archive && !opts.tag_externally {
        let archive = Config::archive_path();
        if let Some(parent) = archive.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        args.extend([
            "--download-archive".into(),
            archive.to_string_lossy().into_owned(),
        ]);
    }
    args.push("--write-info-json".into());
    args
}

/// Inicia o download de uma URL em segundo plano.
pub fn start_download(opts: &DownloadOptions, url: &str) -> Result<DownloadHandle> {
    let tmpdir = unique_tmpdir();
    let mut args = common_args(opts, &tmpdir);
    if opts.kind == MediaKind::Spotify {
        args.push("--no-playlist".into());
    }
    args.push(url.into());
    spawn_with_args(args, tmpdir)
}

/// Baixa várias URLs (usado pelo Spotify) no mesmo processo do yt-dlp.
pub fn start_download_many(opts: &DownloadOptions, urls: &[String]) -> Result<DownloadHandle> {
    if urls.is_empty() {
        bail!("Nenhuma faixa para baixar.");
    }
    let tmpdir = unique_tmpdir();
    std::fs::create_dir_all(&tmpdir)?;
    let batch_path = tmpdir.join("urls.txt");
    {
        let mut file = std::fs::File::create(&batch_path)
            .context("Não foi possível criar a lista de downloads")?;
        for url in urls {
            writeln!(file, "{url}")?;
        }
    }
    let mut args = common_args(opts, &tmpdir);
    args.extend([
        "--batch-file".into(),
        batch_path.to_string_lossy().into_owned(),
        "--no-playlist".into(),
    ]);
    spawn_with_args(args, tmpdir)
}

/// Remove o diretório temporário de um download já organizado.
pub fn cleanup_tmpdir(tmpdir: &Path) {
    let _ = std::fs::remove_dir_all(tmpdir);
}

pub fn check_tool(name: &str, args: &[&str]) -> String {
    match Command::new(name)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            text.lines().next().unwrap_or("ok").trim().to_string()
        }
        Err(_) => format!("{name}: NÃO ENCONTRADO"),
    }
}

/// Status resumido das ferramentas externas, exibido nas configurações.
pub fn tool_status() -> Vec<(String, bool, String)> {
    let tools = [
        ("yt-dlp", vec!["--version"]),
        ("ffmpeg", vec!["-version"]),
        ("ffprobe", vec!["-version"]),
        ("curl", vec!["--version"]),
    ];
    tools
        .iter()
        .map(|(name, args)| {
            let text = check_tool(name, args);
            let ok = !text.contains("NÃO ENCONTRADO");
            (name.to_string(), ok, text)
        })
        .collect()
}

/// Baixa um arquivo remoto (capa) usando curl.
pub fn download_remote(url: &str, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let status = Command::new("curl")
        .args([
            "-sSL",
            "--fail",
            "--retry",
            "2",
            "--retry-delay",
            "1",
            "--connect-timeout",
            "10",
            "--max-time",
            "40",
            "-A",
            "Mozilla/5.0 (X11; Linux x86_64) ytui/0.2",
            "-o",
        ])
        .arg(dest)
        .arg(url)
        .status()
        .context("curl não encontrado para baixar a capa")?;
    if !status.success() {
        let _ = std::fs::remove_file(dest);
        bail!("Falha ao baixar a imagem de capa");
    }
    if std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0) < 512 {
        let _ = std::fs::remove_file(dest);
        bail!("Imagem de capa inválida ou vazia");
    }
    Ok(())
}

/// Mapa auxiliar usado pela interface para exibir títulos a partir do id do
/// arquivo que o yt-dlp está baixando.
pub fn label_map(tracks: &[TrackMeta]) -> HashMap<String, String> {
    tracks
        .iter()
        .map(|t| {
            (
                t.file_key().to_string(),
                format!("{} — {}", t.display_artist(), t.title),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_percent_lines() {
        let p = parse_percent("[download]  42.3% of  213.57MiB at  2.50MiB/s ETA 00:42").unwrap();
        assert!((p.pct - 42.3).abs() < 0.01);
        assert_eq!(p.speed.as_deref(), Some("2.50MiB/s"));
        assert_eq!(p.eta.as_deref(), Some("00:42"));
        assert!(parse_percent("[download] 100% of 3MiB in 00:03").is_some());
        assert!(parse_percent("[info] baixando").is_none());
        assert!(parse_percent("[download] 120% of 3MiB").is_none());
    }

    #[test]
    fn parses_output_files() {
        assert_eq!(
            parse_output_file("[download] Destination: /tmp/ytui/abc123.webm").as_deref(),
            Some("abc123")
        );
        assert_eq!(
            parse_output_file("[download] Destination: /tmp/ytui/abc123.f137.mp4").as_deref(),
            Some("abc123")
        );
        assert_eq!(
            parse_output_file("[Merger] Merging formats into \"/tmp/ytui/abc123.mkv\"").as_deref(),
            Some("abc123")
        );
        assert_eq!(
            parse_output_file("[download]  42.3% of  10.0MiB at 2.0MiB/s ETA 00:10"),
            None
        );
        assert_eq!(
            parse_output_file("[download] /tmp/ytui/abc123.mp3 has already been downloaded")
                .as_deref(),
            Some("abc123")
        );
    }

    #[test]
    fn parses_archive_skip() {
        assert_eq!(
            parse_archived("[download] dQw4w9WgXcQ: has already been recorded in the archive")
                .as_deref(),
            Some("dQw4w9WgXcQ")
        );
        assert_eq!(parse_archived("[download] baixando"), None);
    }

    #[test]
    fn parses_playlist_item() {
        assert_eq!(
            parse_downloading_item("[download] Downloading item 3 of 10"),
            Some((3, 10))
        );
        assert_eq!(parse_downloading_item("nada"), None);
    }

    #[test]
    fn format_args_spotify_audio() {
        let opts = DownloadOptions {
            kind: MediaKind::Spotify,
            audio_format: AudioFormat::Flac,
            ..Default::default()
        };
        let args = format_args(&opts);
        assert!(args.contains(&"-x".to_string()));
        assert!(args.contains(&"flac".to_string()));
        assert!(!args.contains(&"bestvideo*+bestaudio/best".to_string()));
    }

    #[test]
    fn format_args_video_resolution() {
        let opts = DownloadOptions {
            kind: MediaKind::Video,
            video_quality: VideoQuality::Q1080,
            ..Default::default()
        };
        let args = format_args(&opts).join(" ");
        assert!(args.contains("height<=1080"));
    }

    #[test]
    fn file_key_prefers_download_id() {
        let t = TrackMeta {
            id: "spotify123".into(),
            download_id: Some("youtube123".into()),
            ..TrackMeta::default()
        };
        assert_eq!(t.file_key(), "youtube123");
    }
}
