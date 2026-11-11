//! Organização dos arquivos baixados.
//!
//! Responsabilidades:
//! - mover os arquivos do diretório temporário para a biblioteca do usuário;
//! - aplicar metadados e capa do Spotify (quando a fonte é o Spotify);
//! - instalar capas de álbum/playlist;
//! - gerar listas M3U para playlists;
//! - limpar o diretório temporário.

use crate::backend::{
    download_remote, info_json_track, CollectionKind, MediaKind, TrackMeta, TrackSource,
};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const MEDIA_EXTS_SKIP: &[&str] = &[
    "json",
    "jpg",
    "jpeg",
    "png",
    "webp",
    "description",
    "url",
    "part",
    "ytdl",
    "txt",
    "m3u",
    "m3u8",
];

#[derive(Clone, Debug)]
pub struct OrganizeOptions {
    pub kind: MediaKind,
    pub organize: bool,
    pub number: bool,
    /// Spotify: aplicar tags/capa da faixa original.
    pub tag_externally: bool,
    /// Embutir a capa no arquivo de áudio (quando o formato suportar).
    pub embed_cover: bool,
    /// Salvar uma cópia da capa na pasta.
    pub install_cover: bool,
    /// Gerar uma lista M3U para a coleção.
    pub write_m3u: bool,
    /// Nome da playlist/álbum (para M3U e organização).
    pub collection: Option<String>,
}

impl Default for OrganizeOptions {
    fn default() -> Self {
        Self {
            kind: MediaKind::Musica,
            organize: true,
            number: true,
            tag_externally: false,
            embed_cover: false,
            install_cover: true,
            write_m3u: false,
            collection: None,
        }
    }
}

pub fn sanitize(component: &str) -> String {
    let cleaned: String = component
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "Sem título".to_string()
    } else {
        trimmed.chars().take(120).collect()
    }
}

fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin")
        .to_lowercase()
}

fn find_media_file(tmpdir: &Path, key: &str) -> Option<PathBuf> {
    let prefix = format!("{key}.");
    let entries = fs::read_dir(tmpdir).ok()?;
    let mut candidates: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(&prefix) {
            continue;
        }
        let ext = ext_of(&entry.path());
        if MEDIA_EXTS_SKIP.contains(&ext.as_str()) || name.contains(".tag.") {
            continue;
        }
        candidates.push(entry.path());
    }
    candidates.sort_by_key(|p| {
        // Prefere o arquivo maior (o resultado final costuma ser maior que
        // fragmentos intermediários).
        std::cmp::Reverse(fs::metadata(p).map(|m| m.len()).unwrap_or(0))
    });
    candidates.into_iter().next()
}

fn find_thumbnail(tmpdir: &Path, key: &str) -> Option<PathBuf> {
    let prefix = format!("{key}.");
    let entries = fs::read_dir(tmpdir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(&prefix)
            && matches!(
                ext_of(&entry.path()).as_str(),
                "jpg" | "jpeg" | "png" | "webp"
            )
        {
            return Some(entry.path());
        }
    }
    None
}

fn track_number_label(t: &TrackMeta, number: bool) -> Option<String> {
    if !number {
        return None;
    }
    // Uma faixa isolada do Spotify não pertence a uma coleção; numerar só
    // criaria um "01 - " sem significado.
    if t.source == TrackSource::Spotify && t.collection_kind.is_none() {
        return None;
    }
    let n = t.track_number.or(t.playlist_index)?;
    let total = t.total_tracks.unwrap_or(99);
    let width = if total >= 100 { 3 } else { 2 };
    Some(format!("{n:0width$} - "))
}

/// Monta o caminho de destino de uma faixa conforme o tipo de mídia.
fn destination(t: &TrackMeta, opts: &OrganizeOptions, base_dir: &Path) -> (PathBuf, String) {
    let title = sanitize(&t.title);
    let prefix = track_number_label(t, opts.number).unwrap_or_default();
    let artist = t.display_artist();

    match opts.kind {
        MediaKind::Spotify => {
            let top = if opts.organize {
                match t.collection_kind {
                    Some(CollectionKind::Playlist) | Some(CollectionKind::Artist) => {
                        let collection = t
                            .collection
                            .clone()
                            .map(|c| sanitize(&c))
                            .unwrap_or_else(|| "Spotify".into());
                        base_dir.join("Músicas").join("Spotify").join(collection)
                    }
                    Some(CollectionKind::Album) => {
                        let album = t
                            .album
                            .as_ref()
                            .map(|a| sanitize(a))
                            .unwrap_or_else(|| "Álbum".into());
                        let album_dir = if let Some(year) = t.year {
                            format!("{album} ({year})")
                        } else {
                            album
                        };
                        base_dir
                            .join("Músicas")
                            .join(sanitize(&artist))
                            .join(album_dir)
                    }
                    None => base_dir.join("Músicas").join(sanitize(&artist)),
                }
            } else {
                base_dir.join("Músicas")
            };
            let stem = match t.collection_kind {
                Some(CollectionKind::Playlist) => format!("{prefix}{artist} - {title}"),
                Some(CollectionKind::Artist) | None => {
                    if opts.organize {
                        format!("{prefix}{title}")
                    } else {
                        format!("{prefix}{artist} - {title}")
                    }
                }
                Some(CollectionKind::Album) => format!("{prefix}{title}"),
            };
            (top, stem)
        }
        MediaKind::Musica => {
            if !opts.organize {
                return (base_dir.join("Músicas"), format!("{prefix}{title}"));
            }
            let artist_dir = t
                .artist
                .clone()
                .or_else(|| t.channel.clone())
                .map(|a| sanitize(&a))
                .unwrap_or_else(|| "Diversos".into());
            let dir = if let Some(album) = &t.album {
                let mut album_dir = sanitize(album);
                if let Some(y) = t.year {
                    album_dir = format!("{album_dir} ({y})");
                }
                base_dir.join("Músicas").join(artist_dir).join(album_dir)
            } else {
                base_dir.join("Músicas").join(artist_dir)
            };
            (dir, format!("{prefix}{title}"))
        }
        MediaKind::Video | MediaKind::Outro => {
            let top = if opts.kind == MediaKind::Video {
                "Vídeos"
            } else {
                "Outros"
            };
            if !opts.organize {
                return (base_dir.join(top), format!("{prefix}{title}"));
            }
            let year = t
                .year
                .map(|y| y.to_string())
                .unwrap_or_else(|| "Sem ano".into());
            (base_dir.join(top).join(year), format!("{prefix}{title}"))
        }
    }
}

#[derive(Default, Debug)]
pub struct OrganizeReport {
    pub moved: usize,
    pub covers: usize,
    pub tagged: usize,
    pub missing: Vec<String>,
    pub warnings: Vec<String>,
    pub m3u: Option<PathBuf>,
    pub destination_dirs: Vec<PathBuf>,
}

struct MoveRecord {
    dir: PathBuf,
    file_name: String,
    artist: String,
    title: String,
    duration: Option<f64>,
}

/// Move os arquivos baixados do diretório temporário para a estrutura
/// organizada em `base_dir`, aplicando tags do Spotify quando aplicável.
pub fn organize(
    tracks: &[TrackMeta],
    tmpdir: &Path,
    base_dir: &Path,
    opts: &OrganizeOptions,
) -> Result<OrganizeReport> {
    let mut report = OrganizeReport::default();
    let mut records: Vec<MoveRecord> = Vec::new();
    let mut dirs: BTreeMap<PathBuf, ()> = BTreeMap::new();

    for t in tracks {
        let Some(media) = find_media_file(tmpdir, t.file_key()) else {
            report.missing.push(t.title.clone());
            continue;
        };

        // Para o YouTube, o info.json contém metadados mais ricos que a
        // listagem inicial. Para o Spotify, usamos os metadados da faixa
        // original e ignoramos os do vídeo encontrado.
        let meta = if opts.tag_externally || t.source == TrackSource::Spotify {
            t.clone()
        } else {
            info_json_track(tmpdir, t.file_key()).unwrap_or_else(|| t.clone())
        };
        let (dir, stem) = destination(&meta, opts, base_dir);
        fs::create_dir_all(&dir)
            .with_context(|| format!("não foi possível criar {}", dir.display()))?;
        let ext = ext_of(&media);
        let target = dir.join(format!("{stem}.{ext}"));

        // Capa: usa a miniatura baixada pelo yt-dlp ou a capa do Spotify.
        let cover = if opts.install_cover || opts.embed_cover {
            ensure_cover(tmpdir, &meta)
        } else {
            None
        };

        if opts.tag_externally {
            match store_tags(&media, &meta, cover.as_deref(), opts.embed_cover) {
                Ok(true) => report.tagged += 1,
                Ok(false) => {}
                Err(e) => report.warnings.push(format!(
                    "Não foi possível gravar tags de “{}”: {e}",
                    meta.title
                )),
            }
        }

        if target.exists() {
            let _ = fs::remove_file(&target);
        }
        fs::rename(&media, &target)
            .or_else(|_| fs::copy(&media, &target).map(|_| fs::remove_file(&media).unwrap_or(())))
            .with_context(|| format!("não foi possível mover {}", media.display()))?;
        report.moved += 1;

        if opts.install_cover {
            if let Some(cover) = &cover {
                let cover_name = if opts.organize {
                    "capa.jpg".to_string()
                } else {
                    format!("{stem}.capa.jpg")
                };
                let cover_target = dir.join(cover_name);
                if !cover_target.exists()
                    && (fs::copy(cover, &cover_target).is_ok()
                        || fs::rename(cover, &cover_target).is_ok())
                {
                    report.covers += 1;
                }
            }
        }

        records.push(MoveRecord {
            dir: dir.clone(),
            file_name: target
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default(),
            artist: meta.display_artist(),
            title: meta.title.clone(),
            duration: meta.duration,
        });
        dirs.insert(dir, ());
    }

    if opts.write_m3u && opts.kind.is_audio() {
        if let Some(m3u) = write_m3u(&records, opts.collection.as_deref())? {
            report.m3u = Some(m3u);
        }
    }
    report.destination_dirs = dirs.into_keys().collect();

    cleanup_tmpdir(tmpdir);
    Ok(report)
}

fn ensure_cover(tmpdir: &Path, t: &TrackMeta) -> Option<PathBuf> {
    if let Some(local) = find_thumbnail(tmpdir, t.file_key()) {
        return Some(local);
    }
    let url = t.thumbnail.as_deref()?;
    // Playlists e álbuns compartilham a mesma capa. O cache por URL evita
    // baixar a mesma imagem uma vez por faixa.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(url, &mut hasher);
    let key = std::hash::Hasher::finish(&hasher);
    let target = tmpdir.join(format!("cover-{key:016x}.jpg"));
    if target.exists() {
        return Some(target);
    }
    match download_remote(url, &target) {
        Ok(()) => Some(target),
        Err(_) => None,
    }
}

fn supports_cover(ext: &str) -> bool {
    matches!(ext, "mp3" | "m4a" | "mp4" | "flac")
}

/// Grava tags (e opcionalmente capa) usando ffmpeg, preservando o áudio.
/// Retorna `true` quando o arquivo foi reescrito.
fn store_tags(path: &Path, t: &TrackMeta, cover: Option<&Path>, embed_cover: bool) -> Result<bool> {
    let ext = ext_of(path);
    let cover_ok = embed_cover && cover.is_some() && supports_cover(&ext);
    let args = build_tag_args(path, t, cover.filter(|_| cover_ok), &ext);
    let tmp = path.with_file_name(format!(
        ".{}.tag.{}",
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "audio".into()),
        ext
    ));
    let output = std::process::Command::new("ffmpeg")
        .args(&args)
        .arg(&tmp)
        .output();
    match output {
        Ok(out) if out.status.success() && tmp.exists() => {
            fs::rename(&tmp, path).or_else(|_| {
                fs::copy(&tmp, path).map(|_| {
                    let _ = fs::remove_file(&tmp);
                })
            })?;
            Ok(true)
        }
        Ok(out) => {
            let _ = fs::remove_file(&tmp);
            let err = String::from_utf8_lossy(&out.stderr);
            if cover_ok {
                // Se a capa falhou (formato exótico), tenta só as tags.
                return store_tags_without_cover(path, t, &ext);
            }
            anyhow::bail!(
                "ffmpeg: {}",
                err.lines().last().unwrap_or("falha desconhecida")
            )
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            anyhow::bail!("ffmpeg não encontrado: {e}")
        }
    }
}

fn store_tags_without_cover(path: &Path, t: &TrackMeta, ext: &str) -> Result<bool> {
    let args = build_tag_args(path, t, None, ext);
    let tmp = path.with_file_name(format!(
        ".{}.tag.{}",
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "audio".into()),
        ext
    ));
    let output = std::process::Command::new("ffmpeg")
        .args(&args)
        .arg(&tmp)
        .output()
        .context("ffmpeg não encontrado")?;
    if !output.status.success() || !tmp.exists() {
        let _ = fs::remove_file(&tmp);
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "ffmpeg: {}",
            err.lines().last().unwrap_or("falha desconhecida")
        );
    }
    fs::rename(&tmp, path).or_else(|_| {
        fs::copy(&tmp, path).map(|_| {
            let _ = fs::remove_file(&tmp);
        })
    })?;
    Ok(true)
}

fn build_tag_args(path: &Path, t: &TrackMeta, cover: Option<&Path>, ext: &str) -> Vec<String> {
    let mut args: Vec<String> = vec!["-v".into(), "error".into(), "-y".into()];
    args.push("-i".into());
    args.push(path.to_string_lossy().into_owned());
    if let Some(cover) = cover {
        args.push("-i".into());
        args.push(cover.to_string_lossy().into_owned());
    }
    args.extend(["-map".into(), "0:a".into()]);
    if cover.is_some() {
        args.extend([
            "-map".into(),
            "1:v".into(),
            "-c:v".into(),
            "mjpeg".into(),
            "-disposition:v".into(),
            "attached_pic".into(),
        ]);
    }
    args.extend([
        "-c:a".into(),
        "copy".into(),
        "-map_metadata".into(),
        "-1".into(),
    ]);

    let mut meta: Vec<(&str, String)> = Vec::new();
    meta.push(("title", t.title.clone()));
    meta.push(("artist", t.display_artist()));
    if let Some(album) = t.album.clone().or_else(|| {
        t.collection
            .clone()
            .filter(|_| t.collection_kind == Some(CollectionKind::Playlist))
    }) {
        meta.push(("album", album));
    }
    if let Some(album_artist) = &t.album_artist {
        meta.push(("album_artist", album_artist.clone()));
    }
    if let Some(year) = t.year {
        meta.push(("date", year.to_string()));
    }
    if let Some(n) = t.track_number.or(t.playlist_index) {
        let value = match t.total_tracks {
            Some(total) if total > 0 => format!("{n}/{total}"),
            _ => n.to_string(),
        };
        meta.push(("track", value));
    }
    if let Some(disc) = t.disc_number {
        meta.push(("disc", disc.to_string()));
    }
    if t.explicit {
        meta.push((
            "comment",
            "Conteúdo explícito • fonte: Spotify • ytui".into(),
        ));
    } else {
        meta.push(("comment", "Fonte: Spotify • ytui".into()));
    }
    for (key, value) in meta {
        args.extend(["-metadata".into(), format!("{key}={value}")]);
    }
    if cover.is_some() {
        args.extend([
            "-metadata:s:v".into(),
            "title=Album cover".into(),
            "-metadata:s:v".into(),
            "comment=Cover (front)".into(),
        ]);
    }
    if ext == "mp3" {
        args.extend(["-id3v2_version".into(), "3".into()]);
    }
    if ext == "mp4" || ext == "m4a" {
        args.extend(["-movflags".into(), "+faststart".into()]);
    }
    args
}

fn single_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ").trim().to_string()
}

fn write_m3u(records: &[MoveRecord], collection: Option<&str>) -> Result<Option<PathBuf>> {
    if records.is_empty() {
        return Ok(None);
    }
    let mut groups: BTreeMap<PathBuf, Vec<&MoveRecord>> = BTreeMap::new();
    for record in records {
        groups.entry(record.dir.clone()).or_default().push(record);
    }
    let mut last: Option<PathBuf> = None;
    for (dir, records) in groups {
        let name = collection
            .map(sanitize)
            .or_else(|| dir.file_name().map(|f| sanitize(&f.to_string_lossy())))
            .unwrap_or_else(|| "playlist".into());
        let path = dir.join(format!("{name}.m3u8"));
        let mut content = String::from("#EXTM3U\n");
        if let Some(collection) = collection {
            content.push_str(&format!("#PLAYLIST:{}\n", single_line(collection)));
        }
        for record in records {
            let secs = record.duration.unwrap_or(0.0).round() as u64;
            content.push_str(&format!(
                "#EXTINF:{secs},{} - {}\n{}\n",
                single_line(&record.artist),
                single_line(&record.title),
                record.file_name
            ));
        }
        fs::write(&path, content)
            .with_context(|| format!("não foi possível gravar a lista {}", path.display()))?;
        last = Some(path);
    }
    Ok(last)
}

fn cleanup_tmpdir(tmpdir: &Path) {
    if let Ok(entries) = fs::read_dir(tmpdir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let _ = fs::remove_dir_all(path);
            } else {
                let _ = fs::remove_file(path);
            }
        }
    }
    let _ = fs::remove_dir(tmpdir);
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    fn spotify_track(kind: CollectionKind, collection: &str) -> TrackMeta {
        let mut t = TrackMeta::default();
        t.source = TrackSource::Spotify;
        t.id = "sp1".into();
        t.title = "Minha Música".into();
        t.artist = Some("Artista".into());
        t.album = if kind == CollectionKind::Album {
            Some(collection.into())
        } else {
            None
        };
        t.collection = Some(collection.into());
        t.collection_kind = Some(kind);
        t.track_number = Some(3);
        t.total_tracks = Some(12);
        t.year = Some(2024);
        t
    }

    #[test]
    fn sanitize_removes_forbidden_chars() {
        assert_eq!(sanitize("a/b:c*?\"<>|"), "a-b-c------");
        assert_eq!(sanitize("   "), "Sem título");
        assert_eq!(sanitize("...nome..."), "nome");
    }

    #[test]
    fn spotify_album_destination() {
        let t = spotify_track(CollectionKind::Album, "Álbum Incrível");
        let opts = OrganizeOptions {
            kind: MediaKind::Spotify,
            organize: true,
            number: true,
            ..Default::default()
        };
        let (dir, stem) = destination(&t, &opts, Path::new("/base"));
        assert_eq!(
            dir,
            Path::new("/base/Músicas/Artista/Álbum Incrível (2024)")
        );
        assert_eq!(stem, "03 - Minha Música");
    }

    #[test]
    fn spotify_playlist_destination() {
        let t = spotify_track(CollectionKind::Playlist, "Minha Playlist");
        let opts = OrganizeOptions {
            kind: MediaKind::Spotify,
            organize: true,
            number: true,
            ..Default::default()
        };
        let (dir, stem) = destination(&t, &opts, Path::new("/base"));
        assert_eq!(dir, Path::new("/base/Músicas/Spotify/Minha Playlist"));
        assert_eq!(stem, "03 - Artista - Minha Música");
    }

    #[test]
    fn spotify_unorganized_avoids_collisions() {
        let t = spotify_track(CollectionKind::Playlist, "P");
        let opts = OrganizeOptions {
            kind: MediaKind::Spotify,
            organize: false,
            number: false,
            ..Default::default()
        };
        let (dir, stem) = destination(&t, &opts, Path::new("/base"));
        assert_eq!(dir, Path::new("/base/Músicas"));
        assert_eq!(stem, "Artista - Minha Música");
    }

    #[test]
    fn tag_args_include_metadata() {
        let t = spotify_track(CollectionKind::Album, "Álbum");
        let args = build_tag_args(
            Path::new("/tmp/a.mp3"),
            &t,
            Some(Path::new("/tmp/c.jpg")),
            "mp3",
        );
        let joined = args.join(" ");
        assert!(joined.contains("title=Minha Música"));
        assert!(joined.contains("artist=Artista"));
        assert!(joined.contains("album=Álbum"));
        assert!(joined.contains("track=3/12"));
        assert!(joined.contains("attached_pic"));
        assert!(joined.contains("-id3v2_version 3"));
    }

    #[test]
    fn tag_args_without_cover() {
        let t = spotify_track(CollectionKind::Playlist, "P");
        let args = build_tag_args(Path::new("/tmp/a.opus"), &t, None, "opus");
        let joined = args.join(" ");
        assert!(!joined.contains("attached_pic"));
        assert!(joined.contains("album=P"));
        assert!(!joined.contains("id3v2"));
    }
}
