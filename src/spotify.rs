//! Suporte ao Spotify.
//!
//! O Spotify não permite baixar o áudio diretamente (DRM), então o ytui usa o
//! Spotify como fonte de metadados e baixa a faixa correspondente do YouTube.
//! Isso é o mesmo princípio usado por ferramentas como o spotDL.
//!
//! Não é necessário login nem chave de API: as páginas públicas de embed do
//! Spotify (`open.spotify.com/embed/...`) trazem título, artistas, capa,
//! duração e a lista de faixas em JSON. Funciona para links públicos de
//! música, álbum, playlist e artista. Playlists privadas exigem login e não
//! são suportadas.

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SpotifyKind {
    #[default]
    Track,
    Album,
    Playlist,
    Artist,
    Episode,
    Show,
}

impl SpotifyKind {
    pub fn key(self) -> &'static str {
        match self {
            SpotifyKind::Track => "track",
            SpotifyKind::Album => "album",
            SpotifyKind::Playlist => "playlist",
            SpotifyKind::Artist => "artist",
            SpotifyKind::Episode => "episode",
            SpotifyKind::Show => "show",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SpotifyKind::Track => "música",
            SpotifyKind::Album => "álbum",
            SpotifyKind::Playlist => "playlist",
            SpotifyKind::Artist => "artista",
            SpotifyKind::Episode => "episódio",
            SpotifyKind::Show => "podcast",
        }
    }

    pub fn is_supported(self) -> bool {
        matches!(
            self,
            SpotifyKind::Track | SpotifyKind::Album | SpotifyKind::Playlist | SpotifyKind::Artist
        )
    }

    fn from_token(s: &str) -> Option<Self> {
        match s {
            "track" => Some(SpotifyKind::Track),
            "album" => Some(SpotifyKind::Album),
            "playlist" => Some(SpotifyKind::Playlist),
            "artist" => Some(SpotifyKind::Artist),
            "episode" => Some(SpotifyKind::Episode),
            "show" => Some(SpotifyKind::Show),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotifyRef {
    pub kind: SpotifyKind,
    pub id: String,
}

impl SpotifyRef {
    pub fn url(&self) -> String {
        format!("https://open.spotify.com/{}/{}", self.kind.key(), self.id)
    }
}

/// Verifica rapidamente se o texto parece um link do Spotify.
pub fn is_spotify_url(input: &str) -> bool {
    let lower = input.trim().to_ascii_lowercase();
    lower.starts_with("spotify:")
        || lower.contains("open.spotify.com/")
        || lower.contains("play.spotify.com/")
        || lower.contains("spotify.link/")
        || lower.contains("spoti.fi/")
}

fn short_link(input: &str) -> bool {
    let lower = input.trim().to_ascii_lowercase();
    lower.contains("spotify.link/") || lower.contains("spoti.fi/")
}

/// Interpreta links e URIs do Spotify.
///
/// Formatos aceitos:
/// - `https://open.spotify.com/track/ID?si=...`
/// - `https://open.spotify.com/intl-pt/album/ID`
/// - `https://open.spotify.com/embed/playlist/ID`
/// - `https://open.spotify.com/user/fulano/playlist/ID`
/// - `spotify:track:ID`
pub fn parse_url(input: &str) -> Option<SpotifyRef> {
    let text = input
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '<' || c == '>');
    if text.is_empty() {
        return None;
    }
    if let Some(rest) = text.strip_prefix("spotify:") {
        let parts: Vec<&str> = rest.split(':').filter(|p| !p.is_empty()).collect();
        for (i, part) in parts.iter().enumerate() {
            if let Some(kind) = SpotifyKind::from_token(part) {
                if let Some(id) = parts.get(i + 1) {
                    if valid_id(id) {
                        return Some(SpotifyRef {
                            kind,
                            id: clean_id(id),
                        });
                    }
                }
            }
        }
        return None;
    }

    let without_scheme = text
        .strip_prefix("https://")
        .or_else(|| text.strip_prefix("http://"))
        .unwrap_or(text);
    let (host, path) = match without_scheme.split_once('/') {
        Some((host, path)) => (host, path),
        None => (without_scheme, ""),
    };
    let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    if !(host == "open.spotify.com"
        || host == "play.spotify.com"
        || host == "embed.spotify.com"
        || host.ends_with(".open.spotify.com")
        || host == "spotify.link"
        || host == "spoti.fi")
    {
        return None;
    }
    let cleaned_path = path.split(['?', '#']).next().unwrap_or(path);
    let segments: Vec<&str> = cleaned_path
        .split('/')
        .filter(|s| !s.is_empty() && *s != "embed" && *s != "embed-podcast")
        .collect();
    if segments.is_empty() {
        return None;
    }
    for (i, segment) in segments.iter().enumerate() {
        if segment.starts_with("intl-") {
            continue;
        }
        if let Some(kind) = SpotifyKind::from_token(&segment.to_ascii_lowercase()) {
            // Em `/user/x/playlist/ID` e `/intl-pt/track/ID` o ID vem depois.
            if let Some(id) = segments.get(i + 1) {
                if valid_id(id) {
                    return Some(SpotifyRef {
                        kind,
                        id: clean_id(id),
                    });
                }
            }
        }
    }
    None
}

fn valid_id(id: &str) -> bool {
    let id = clean_id(id);
    id.len() >= 8 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

fn clean_id(id: &str) -> String {
    id.split(['?', '#', '&'])
        .next()
        .unwrap_or(id)
        .trim()
        .to_string()
}

fn id_from_uri(uri: Option<&str>) -> Option<String> {
    let uri = uri?;
    let id = uri.rsplit(':').next()?;
    if valid_id(id) {
        Some(clean_id(id))
    } else {
        None
    }
}

fn to_year(iso: Option<&str>) -> Option<u32> {
    let iso = iso?;
    let year = iso.get(..4)?;
    year.parse().ok()
}

#[derive(Clone, Debug, Default)]
pub struct SpotifyEntity {
    pub kind: SpotifyKind,
    pub id: String,
    pub title: String,
    pub url: String,
    /// Dono da playlist ou artista principal do álbum.
    pub owner: Option<String>,
    pub cover: Option<String>,
    pub year: Option<u32>,
    pub tracks: Vec<SpotifyTrack>,
    /// Quantidade de itens que não são faixas (episódios, itens locais...).
    pub skipped: usize,
}

#[derive(Clone, Debug, Default)]
pub struct SpotifyTrack {
    pub id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub duration_ms: u64,
    pub cover: Option<String>,
    pub explicit: bool,
    pub track_number: u32,
    pub disc_number: u32,
    pub year: Option<u32>,
}

impl SpotifyTrack {
    pub fn artist_line(&self) -> String {
        self.artists.join(", ")
    }
}

/// Resolve todas as informações públicas de um link do Spotify.
pub fn resolve(input: &str) -> Result<SpotifyEntity> {
    let normalized = if short_link(input) {
        expand_short_link(input).unwrap_or_else(|| input.trim().to_string())
    } else {
        input.trim().to_string()
    };
    let reference = parse_url(&normalized).with_context(|| {
        format!(
            "Link do Spotify não reconhecido: {}",
            crate::util::truncate(input, 80)
        )
    })?;
    if !reference.kind.is_supported() {
        bail!(
            "{} do Spotify ainda não é suportado. Use links de música, álbum, playlist ou artista.",
            capitalize(reference.kind.label())
        );
    }

    let embed_url = format!(
        "https://open.spotify.com/embed/{}/{}",
        reference.kind.key(),
        reference.id
    );
    let html = fetch_html(&embed_url)?;
    let json = extract_next_data(&html)
        .context("Não foi possível ler os dados do Spotify. O link é público?")?;
    let next: NextData = serde_json::from_str(json).map_err(|_| {
        anyhow!(
            "Não foi possível interpretar este link do Spotify. Ele existe e é público? \
             Playlists privadas e podcasts não são suportados."
        )
    })?;
    let entity = next.props.page_props.state.data.entity;
    parse_entity(entity, &reference)
}

fn fetch_html(url: &str) -> Result<String> {
    let output = std::process::Command::new("curl")
        .args([
            "-sSL",
            "--fail",
            "--compressed",
            "--retry",
            "2",
            "--retry-delay",
            "1",
            "--connect-timeout",
            "10",
            "--max-time",
            "40",
            "-A",
            USER_AGENT,
            "-H",
            "Accept-Language: pt-BR,pt;q=0.9,en;q=0.8",
            "-H",
            "Accept: text/html,application/xhtml+xml",
        ])
        .arg(url)
        .output()
        .context("Falha ao executar curl. Instale o curl para usar links do Spotify.")?;
    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let err = String::from_utf8_lossy(&output.stderr);
        let friendly = if code == 22 {
            "O Spotify recusou a requisição. O link é público e ainda existe?".to_string()
        } else if code == 28 {
            "O Spotify demorou demais para responder. Verifique sua conexão.".to_string()
        } else {
            format!("curl terminou com código {code}: {}", err.trim())
        };
        bail!("{friendly}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn expand_short_link(input: &str) -> Option<String> {
    let output = std::process::Command::new("curl")
        .args([
            "-sSL",
            "-o",
            "/dev/null",
            "-w",
            "%{url_effective}",
            "--max-time",
            "20",
            "-A",
            USER_AGENT,
        ])
        .arg(input.trim())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.starts_with("http") {
        Some(url)
    } else {
        None
    }
}

fn extract_next_data(html: &str) -> Option<&str> {
    let marker = "id=\"__NEXT_DATA__\"";
    let idx = html.find(marker)?;
    let open_end = idx + html[idx..].find('>')? + 1;
    let close = open_end + html[open_end..].find("</script>")?;
    Some(html[open_end..close].trim())
}

// ---------------------------------------------------------------------------
// Estruturas do JSON embutido pelo Spotify (somente as partes que usamos).
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct NextData {
    props: NextProps,
}

#[derive(Deserialize)]
struct NextProps {
    #[serde(rename = "pageProps")]
    page_props: PageProps,
}

#[derive(Deserialize)]
struct PageProps {
    state: PageState,
}

#[derive(Deserialize)]
struct PageState {
    data: PageData,
}

#[derive(Deserialize)]
struct PageData {
    entity: Entity,
}

#[derive(Deserialize, Default)]
struct Entity {
    #[serde(default, rename = "type")]
    entity_type: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(default)]
    duration: u64,
    #[serde(default, rename = "releaseDate")]
    release_date: Option<ReleaseDate>,
    #[serde(default, rename = "visualIdentity")]
    visual_identity: Option<VisualIdentity>,
    #[serde(default, rename = "coverArt")]
    cover_art: Option<CoverArt>,
    #[serde(default, rename = "trackList")]
    track_list: Option<Vec<EntityTrack>>,
    #[serde(default)]
    artists: Option<Vec<Artist>>,
    #[serde(default, rename = "isExplicit")]
    explicit: bool,
}

#[derive(Deserialize, Default)]
struct ReleaseDate {
    #[serde(default, rename = "isoString")]
    iso: Option<String>,
}

#[derive(Deserialize, Default)]
struct VisualIdentity {
    #[serde(default)]
    image: Vec<Image>,
}

#[derive(Deserialize, Default)]
struct CoverArt {
    #[serde(default)]
    sources: Vec<Image>,
}

#[derive(Deserialize, Default)]
struct Image {
    #[serde(default)]
    url: String,
    #[serde(default, rename = "maxWidth")]
    max_width: Option<u32>,
    #[serde(default, rename = "maxHeight")]
    max_height: Option<u32>,
}

#[derive(Deserialize, Default)]
struct Artist {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize, Default)]
struct EntityTrack {
    #[serde(default)]
    uri: Option<String>,
    #[serde(default)]
    title: String,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(default)]
    duration: u64,
    #[serde(default, rename = "isExplicit")]
    explicit: bool,
    #[serde(default, rename = "entityType")]
    entity_type: Option<String>,
}

fn parse_entity(entity: Entity, reference: &SpotifyRef) -> Result<SpotifyEntity> {
    let kind = if entity.entity_type.is_empty() {
        reference.kind
    } else {
        SpotifyKind::from_token(&entity.entity_type).unwrap_or(reference.kind)
    };
    let title = if entity.title.is_empty() {
        entity.name.clone()
    } else {
        entity.title.clone()
    };
    if title.trim().is_empty() {
        bail!("O Spotify não retornou um título para este link.");
    }
    let cover = best_image(&entity)
        .or_else(|| {
            entity
                .visual_identity
                .as_ref()
                .and_then(|v| best_from(&v.image))
        })
        .or_else(|| {
            entity
                .cover_art
                .as_ref()
                .and_then(|c| best_from(&c.sources))
        });
    let year = to_year(entity.release_date.as_ref().and_then(|r| r.iso.as_deref()));
    let owner = entity.subtitle.clone().filter(|s| !s.trim().is_empty());

    let mut tracks = Vec::new();
    let mut skipped = 0usize;

    if let Some(list) = &entity.track_list {
        for item in list {
            if let Some(kind) = &item.entity_type {
                if kind != "track" {
                    skipped += 1;
                    continue;
                }
            }
            let Some(id) = id_from_uri(item.uri.as_deref()) else {
                skipped += 1;
                continue;
            };
            let artists = split_artists(item.subtitle.as_deref());
            let number = tracks.len() as u32 + 1;
            tracks.push(SpotifyTrack {
                id,
                title: item.title.clone(),
                artists: artists.clone(),
                album: album_for(kind, &title, &owner),
                album_artist: album_artist_for(kind, &artists, &owner),
                duration_ms: item.duration,
                cover: cover.clone(),
                explicit: item.explicit,
                track_number: number,
                disc_number: 1,
                year,
            });
        }
    } else {
        let id = if reference.id.is_empty() {
            entity.id.clone()
        } else {
            reference.id.clone()
        };
        let artists: Vec<String> = entity
            .artists
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|a| a.name.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let artists = if artists.is_empty() {
            split_artists(owner.as_deref())
        } else {
            artists
        };
        let album_artist = album_artist_for(kind, &artists, &owner);
        tracks.push(SpotifyTrack {
            id,
            title: title.clone(),
            artists,
            album: album_for(kind, &title, &owner),
            album_artist,
            duration_ms: entity.duration,
            cover: cover.clone(),
            explicit: entity.explicit,
            track_number: 1,
            disc_number: 1,
            year,
        });
    }

    if tracks.is_empty() {
        bail!(
            "Nenhuma faixa encontrada neste {} do Spotify.{}",
            reference.kind.label(),
            if skipped > 0 {
                " (o conteúdo pode ser exclusivo para assinantes ou conter apenas episódios)"
            } else {
                ""
            }
        );
    }

    Ok(SpotifyEntity {
        kind,
        id: reference.id.clone(),
        title,
        url: reference.url(),
        owner,
        cover,
        year,
        tracks,
        skipped,
    })
}

fn album_for(kind: SpotifyKind, title: &str, _owner: &Option<String>) -> Option<String> {
    match kind {
        SpotifyKind::Album => Some(title.to_string()),
        // O embed de uma faixa isolada não informa o nome do álbum.
        SpotifyKind::Track => None,
        _ => None,
    }
}

fn album_artist_for(
    kind: SpotifyKind,
    artists: &[String],
    owner: &Option<String>,
) -> Option<String> {
    match kind {
        SpotifyKind::Album => owner.clone(),
        SpotifyKind::Track => artists.first().cloned(),
        _ => None,
    }
}

fn split_artists(subtitle: Option<&str>) -> Vec<String> {
    let Some(text) = subtitle else {
        return Vec::new();
    };
    let normalized = text.replace(" & ", ", ");
    normalized
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("Various Artists"))
        .collect()
}

fn best_image(entity: &Entity) -> Option<String> {
    entity
        .visual_identity
        .as_ref()
        .and_then(|v| best_from(&v.image))
        .or_else(|| {
            entity
                .cover_art
                .as_ref()
                .and_then(|c| best_from(&c.sources))
        })
}

fn best_from(images: &[Image]) -> Option<String> {
    images
        .iter()
        .filter(|i| !i.url.is_empty())
        .max_by_key(|i| i.max_width.unwrap_or(0) as u64 * i.max_height.unwrap_or(0) as u64)
        .map(|i| i.url.clone())
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_urls() {
        let cases = [
            (
                "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT?si=abc",
                SpotifyKind::Track,
                "4cOdK2wGLETKBW3PvgPWqT",
            ),
            (
                "https://open.spotify.com/intl-pt/album/1ATL5GLyefJaxhQzSPVrLX",
                SpotifyKind::Album,
                "1ATL5GLyefJaxhQzSPVrLX",
            ),
            (
                "https://open.spotify.com/embed/playlist/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyKind::Playlist,
                "37i9dQZF1DXcBWIGoYBM5M",
            ),
            (
                "spotify:artist:0gxyHStUsqpMadRV0Di1Qt",
                SpotifyKind::Artist,
                "0gxyHStUsqpMadRV0Di1Qt",
            ),
            (
                "https://open.spotify.com/user/spotify/playlist/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyKind::Playlist,
                "37i9dQZF1DXcBWIGoYBM5M",
            ),
            (
                "http://play.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT#x",
                SpotifyKind::Track,
                "4cOdK2wGLETKBW3PvgPWqT",
            ),
        ];
        for (input, kind, id) in cases {
            let parsed = parse_url(input).unwrap_or_else(|| panic!("não parseou {input}"));
            assert_eq!(parsed.kind, kind, "{input}");
            assert_eq!(parsed.id, id, "{input}");
        }
    }

    #[test]
    fn rejects_non_spotify() {
        assert!(parse_url("https://youtube.com/watch?v=x").is_none());
        assert!(parse_url("https://fakeopen.spotify.com/track/abc").is_none());
        assert!(parse_url("https://open.spotify.com/track/curto").is_none());
        assert!(!is_spotify_url("https://example.com"));
        assert!(is_spotify_url("spotify:track:4cOdK2wGLETKBW3PvgPWqT"));
    }

    #[test]
    fn extracts_next_data() {
        let html =
            r#"<html><script id="__NEXT_DATA__" type="application/json">{"a":1}</script></html>"#;
        assert_eq!(extract_next_data(html), Some(r#"{"a":1}"#));
        assert_eq!(extract_next_data("<html></html>"), None);
    }

    fn fixture(kind: &str, id: &str) -> SpotifyEntity {
        let json = match kind {
            "track" => {
                r#"{
              "props":{"pageProps":{"state":{"data":{"entity":{
                "type":"track","id":"4cOdK2wGLETKBW3PvgPWqT","title":"Never Gonna Give You Up",
                "duration":213573,"releaseDate":{"isoString":"1987-11-12T00:00:00Z"},
                "artists":[{"name":"Rick Astley"}],
                "isExplicit":false,
                "visualIdentity":{"image":[
                  {"url":"https://img/64.jpg","maxWidth":64,"maxHeight":64},
                  {"url":"https://img/640.jpg","maxWidth":640,"maxHeight":640}
                ]}
              }}}}}}"#
            }
            "playlist" => {
                r#"{
              "props":{"pageProps":{"state":{"data":{"entity":{
                "type":"playlist","id":"37i9dQZF1DXcBWIGoYBM5M","title":"Today's Top Hits",
                "subtitle":"Spotify","duration":0,
                "visualIdentity":{"image":[{"url":"https://img/pl.jpg","maxWidth":300,"maxHeight":300}]},
                "trackList":[
                  {"uri":"spotify:track:3zOYmiNOuyDL0ClxJcEJ0T","title":"Stop The Wedding!","subtitle":"Ashe","duration":198840,"isExplicit":false,"entityType":"track"},
                  {"uri":"spotify:episode:abc123456789","title":"Episódio","subtitle":"Podcast","duration":1000,"entityType":"episode"},
                  {"uri":"spotify:track:7bxaFZ1O3cHkgLKMsdC3xR","title":"Loser","subtitle":"Tame Impala","duration":223069,"isExplicit":true,"entityType":"track"}
                ]
              }}}}}}"#
            }
            "album" => {
                r#"{
              "props":{"pageProps":{"state":{"data":{"entity":{
                "type":"album","id":"1ATL5GLyefJaxhQzSPVrLX","title":"Scorpion","subtitle":"Drake",
                "releaseDate":{"isoString":"2018-06-29T00:00:00Z"},
                "visualIdentity":{"image":[{"url":"https://img/al.jpg","maxWidth":640,"maxHeight":640}]},
                "trackList":[
                  {"uri":"spotify:track:2yg9UN4eo5eMVJ7OB4RWj3","title":"Survival","subtitle":"Drake","duration":136186,"isExplicit":true,"entityType":"track"},
                  {"uri":"spotify:track:0TlLq3lA83rQOYtrqBqSct","title":"Nonstop","subtitle":"Drake","duration":238000,"isExplicit":true,"entityType":"track"}
                ]
              }}}}}}"#
            }
            _ => unreachable!(),
        };
        let next: NextData = serde_json::from_str(json).unwrap();
        let reference = SpotifyRef {
            kind: SpotifyKind::from_token(kind).unwrap(),
            id: id.into(),
        };
        parse_entity(next.props.page_props.state.data.entity, &reference).unwrap()
    }

    #[test]
    fn parses_track_entity() {
        let e = fixture("track", "4cOdK2wGLETKBW3PvgPWqT");
        assert_eq!(e.title, "Never Gonna Give You Up");
        assert_eq!(e.tracks.len(), 1);
        assert_eq!(e.tracks[0].artists, vec!["Rick Astley"]);
        assert_eq!(e.year, Some(1987));
        assert_eq!(e.cover.as_deref(), Some("https://img/640.jpg"));
        assert_eq!(e.tracks[0].track_number, 1);
    }

    #[test]
    fn parses_playlist_and_skips_episode() {
        let e = fixture("playlist", "37i9dQZF1DXcBWIGoYBM5M");
        assert_eq!(e.title, "Today's Top Hits");
        assert_eq!(e.tracks.len(), 2);
        assert_eq!(e.skipped, 1);
        assert!(e.tracks[1].explicit);
        assert_eq!(e.tracks[0].track_number, 1);
        assert_eq!(e.tracks[1].track_number, 2);
        assert_eq!(e.tracks[0].cover.as_deref(), Some("https://img/pl.jpg"));
    }

    #[test]
    fn parses_album_entity() {
        let e = fixture("album", "1ATL5GLyefJaxhQzSPVrLX");
        assert_eq!(e.title, "Scorpion");
        assert_eq!(e.year, Some(2018));
        assert_eq!(e.tracks.len(), 2);
        assert_eq!(e.tracks[0].album.as_deref(), Some("Scorpion"));
        assert_eq!(e.tracks[0].album_artist.as_deref(), Some("Drake"));
    }

    #[test]
    fn splits_artists() {
        assert_eq!(
            split_artists(Some("Drake, Future & Metro Boomin")),
            vec!["Drake", "Future", "Metro Boomin"]
        );
        assert!(split_artists(None).is_empty());
    }
}
