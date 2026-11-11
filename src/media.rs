//! Resolução de links e conversão de metadados para o modelo de download.
//!
//! Detecta automaticamente se a URL é do Spotify, do YouTube ou de outro site
//! aceito pelo yt-dlp. O tipo escolhido no menu é usado como dica apenas para
//! links genéricos.

use crate::backend::{self, CollectionKind, MediaKind, TrackMeta, TrackSource};
use crate::matching;
use crate::spotify::{self, SpotifyEntity, SpotifyKind};
use anyhow::Result;

#[derive(Clone, Debug)]
pub struct FetchedMedia {
    pub kind: MediaKind,
    pub title: String,
    pub tracks: Vec<TrackMeta>,
    pub is_playlist: bool,
    pub spotify: Option<SpotifyEntity>,
}

impl FetchedMedia {
    pub fn spotify_track_count(&self) -> usize {
        self.tracks.iter().filter(|t| t.is_spotify()).count()
    }

    pub fn selected(&self, selected: &[bool]) -> Vec<TrackMeta> {
        self.tracks
            .iter()
            .enumerate()
            .filter(|(i, _)| selected.get(*i).copied().unwrap_or(true))
            .map(|(_, t)| t.clone())
            .collect()
    }

    /// Índices 1-based das faixas selecionadas (para `--playlist-items`).
    pub fn selected_positions(&self, selected: &[bool]) -> Vec<usize> {
        self.tracks
            .iter()
            .enumerate()
            .filter(|(i, _)| selected.get(*i).copied().unwrap_or(true))
            .map(|(i, _)| i + 1)
            .collect()
    }
}

/// Resolve uma URL em uma lista de faixas, detectando a fonte.
pub fn fetch(url: &str, hint: MediaKind) -> Result<FetchedMedia> {
    if spotify::is_spotify_url(url) {
        let entity = spotify::resolve(url)?;
        let tracks = spotify_to_tracks(&entity);
        let is_playlist = entity.tracks.len() > 1 || entity.kind != SpotifyKind::Track;
        return Ok(FetchedMedia {
            kind: MediaKind::Spotify,
            title: entity.title.clone(),
            tracks,
            is_playlist,
            spotify: Some(entity),
        });
    }

    let metadata = backend::fetch_metadata_full(url)?;
    let kind = if hint == MediaKind::Spotify {
        MediaKind::Musica
    } else {
        hint
    };
    Ok(FetchedMedia {
        kind,
        title: metadata.title,
        is_playlist: metadata.is_playlist,
        tracks: metadata.tracks,
        spotify: None,
    })
}

/// Converte as faixas do Spotify para o modelo comum de download.
pub fn spotify_to_tracks(entity: &SpotifyEntity) -> Vec<TrackMeta> {
    let total = entity.tracks.len() as u32;
    entity
        .tracks
        .iter()
        .map(|track| {
            let collection_kind = match entity.kind {
                SpotifyKind::Album => Some(CollectionKind::Album),
                SpotifyKind::Playlist => Some(CollectionKind::Playlist),
                SpotifyKind::Artist => Some(CollectionKind::Artist),
                _ => None,
            };
            let collection = if collection_kind.is_some() {
                Some(entity.title.clone())
            } else {
                None
            };
            TrackMeta {
                id: track.id.clone(),
                download_id: None,
                source: TrackSource::Spotify,
                title: track.title.clone(),
                artist: track.artists.first().cloned(),
                album: track.album.clone(),
                album_artist: track.album_artist.clone(),
                channel: None,
                year: track.year.or(entity.year),
                track_number: Some(track.track_number),
                disc_number: Some(track.disc_number),
                playlist_index: Some(track.track_number),
                total_tracks: Some(total),
                duration: Some(track.duration_ms as f64 / 1000.0),
                thumbnail: track.cover.clone().or_else(|| entity.cover.clone()),
                spotify_id: Some(track.id.clone()),
                spotify_url: Some(format!("https://open.spotify.com/track/{}", track.id)),
                collection,
                collection_kind,
                explicit: track.explicit,
                match_confidence: None,
                matched_title: None,
                matched_channel: None,
                match_query: None,
            }
        })
        .collect()
}

/// Aplica o resultado da busca no YouTube a uma faixa do Spotify.
pub fn apply_match(track: &mut TrackMeta, matched: &matching::Match) {
    track.download_id = Some(matched.video_id.clone());
    track.match_confidence = Some(matched.confidence);
    track.matched_title = Some(matched.title.clone());
    track.matched_channel = Some(matched.channel.clone());
    track.match_query = Some(matched.query.clone());
    // A capa/duração do Spotify continuam sendo as fontes de verdade.
}

pub fn youtube_url(video_id: &str) -> String {
    format!("https://www.youtube.com/watch?v={video_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spotify::{SpotifyEntity, SpotifyKind, SpotifyTrack};

    fn entity(kind: SpotifyKind) -> SpotifyEntity {
        SpotifyEntity {
            kind,
            id: "x".into(),
            title: "Coleção".into(),
            url: "https://open.spotify.com/playlist/x".into(),
            owner: Some("Dono".into()),
            cover: Some("https://img/cover.jpg".into()),
            year: Some(2024),
            skipped: 0,
            tracks: vec![SpotifyTrack {
                id: "abc12345".into(),
                title: "Faixa".into(),
                artists: vec!["Artista".into()],
                album: Some("Álbum".into()),
                album_artist: Some("Artista".into()),
                duration_ms: 200_000,
                cover: None,
                explicit: true,
                track_number: 2,
                disc_number: 1,
                year: Some(2024),
            }],
        }
    }

    #[test]
    fn maps_playlist_tracks() {
        let tracks = spotify_to_tracks(&entity(SpotifyKind::Playlist));
        assert_eq!(tracks.len(), 1);
        let t = &tracks[0];
        assert_eq!(t.source, TrackSource::Spotify);
        assert_eq!(t.duration, Some(200.0));
        assert_eq!(t.collection_kind, Some(CollectionKind::Playlist));
        assert_eq!(t.collection.as_deref(), Some("Coleção"));
        assert_eq!(t.spotify_id.as_deref(), Some("abc12345"));
        assert!(t.explicit);
    }

    #[test]
    fn maps_single_track_without_collection() {
        let tracks = spotify_to_tracks(&entity(SpotifyKind::Track));
        assert!(tracks[0].collection.is_none());
        assert!(tracks[0].collection_kind.is_none());
    }

    #[test]
    fn applies_match_fields() {
        let mut tracks = spotify_to_tracks(&entity(SpotifyKind::Playlist));
        let matched = matching::Match {
            video_id: "vid123".into(),
            url: "https://www.youtube.com/watch?v=vid123".into(),
            title: "Faixa (Official Audio)".into(),
            channel: "Artista - Topic".into(),
            duration: Some(200.0),
            confidence: 0.91,
            query: "Artista Faixa".into(),
        };
        apply_match(&mut tracks[0], &matched);
        assert_eq!(tracks[0].download_id.as_deref(), Some("vid123"));
        assert_eq!(tracks[0].match_confidence, Some(0.91));
        assert_eq!(tracks[0].file_key(), "vid123");
    }
}
