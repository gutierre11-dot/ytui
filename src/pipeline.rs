//! Orquestração de um download do Spotify.
//!
//! Etapas: buscar correspondências no YouTube → baixar em lote → organizar,
//! aplicar metadados/capa do Spotify e gerar M3U.
//!
//! A thread de trabalho envia eventos para a interface. O cancelamento mata o
//! processo do yt-dlp em andamento; buscas já iniciadas terminam sozinhas.

use crate::backend::{self, DownloadOptions, MediaKind, Progress, TrackMeta};
use crate::matching;
use crate::media;
use crate::organize::{self, OrganizeOptions};
use crate::spotify::{SpotifyEntity, SpotifyKind};
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

type ChildSlot = Arc<Mutex<Option<Arc<Mutex<Child>>>>>;

#[derive(Debug, Default)]
pub struct JobSummary {
    pub success: bool,
    pub cancelled: bool,
    pub message: String,
    pub matched: usize,
    pub failed: usize,
    pub low_confidence: usize,
    pub moved: usize,
    pub covers: usize,
    pub tagged: usize,
    pub missing: Vec<String>,
    pub failed_names: Vec<String>,
    pub warnings: Vec<String>,
    pub m3u: Option<PathBuf>,
    pub dirs: Vec<PathBuf>,
}

#[derive(Debug)]
pub enum JobEvent {
    Stage {
        title: String,
        detail: String,
    },
    Match {
        done: usize,
        total: usize,
        track: String,
        matched: String,
        confidence: f32,
    },
    MatchError {
        done: usize,
        total: usize,
        track: String,
        error: String,
    },
    DownloadStarted {
        total: usize,
        urls: usize,
    },
    Line(String),
    Progress {
        overall: f32,
        current: f32,
        file: Option<String>,
        speed: Option<String>,
        eta: Option<String>,
    },
    Organizing,
    Finished(JobSummary),
}

pub struct JobHandle {
    pub rx: Receiver<JobEvent>,
    cancel: Arc<AtomicBool>,
    child: ChildSlot,
}

impl JobHandle {
    /// Solicita o cancelamento e mata o yt-dlp em execução (se houver).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        if let Ok(slot) = self.child.lock() {
            if let Some(child) = slot.as_ref() {
                if let Ok(mut guard) = child.lock() {
                    let _ = guard.kill();
                }
            }
        }
    }
}

impl Drop for JobHandle {
    fn drop(&mut self) {
        // Nunca deixa um yt-dlp órfão quando a interface fecha.
        self.cancel.store(true, Ordering::SeqCst);
        if let Ok(slot) = self.child.lock() {
            if let Some(child) = slot.as_ref() {
                if let Ok(mut guard) = child.lock() {
                    let _ = guard.kill();
                }
            }
        }
    }
}

/// Inicia o fluxo completo do Spotify em segundo plano.
pub fn start_spotify(
    entity: SpotifyEntity,
    tracks: Vec<TrackMeta>,
    opts: DownloadOptions,
    base_dir: PathBuf,
    workers: usize,
) -> JobHandle {
    let (tx, rx) = channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let child: ChildSlot = Arc::new(Mutex::new(None));
    let thread_cancel = Arc::clone(&cancel);
    let thread_child = Arc::clone(&child);

    std::thread::spawn(move || {
        let summary = run(
            entity,
            tracks,
            opts,
            base_dir,
            workers,
            &tx,
            &thread_cancel,
            &thread_child,
        );
        let _ = tx.send(JobEvent::Finished(summary));
    });

    JobHandle { rx, cancel, child }
}

#[allow(clippy::too_many_arguments)]
fn run(
    entity: SpotifyEntity,
    tracks: Vec<TrackMeta>,
    mut opts: DownloadOptions,
    base_dir: PathBuf,
    workers: usize,
    tx: &std::sync::mpsc::Sender<JobEvent>,
    cancel: &AtomicBool,
    child_slot: &ChildSlot,
) -> JobSummary {
    let total = tracks.len();
    let mut summary = JobSummary {
        matched: 0,
        failed: total,
        ..Default::default()
    };

    let _ = tx.send(JobEvent::Stage {
        title: "Correspondências".into(),
        detail: format!(
            "Procurando {} nos arquivos do YouTube Music…",
            crate::util::plural(total, "faixa", "faixas")
        ),
    });

    let input: Vec<(String, Vec<String>, Option<f64>)> = tracks
        .iter()
        .map(|t| {
            (
                t.title.clone(),
                t.artist
                    .clone()
                    .into_iter()
                    .chain(t.channel.clone())
                    .collect::<Vec<_>>(),
                t.duration_secs(),
            )
        })
        .collect();

    let mut tracks = tracks;
    let mut matched_flags = vec![false; total];
    let mut match_errors: Vec<(String, String)> = Vec::new();
    matching::match_all(&input, workers, cancel, |index, result| match result {
        Ok(m) => {
            media::apply_match(&mut tracks[index], m);
            matched_flags[index] = true;
            let done = matched_flags.iter().filter(|b| **b).count() + match_errors.len();
            let _ = tx.send(JobEvent::Match {
                done,
                total,
                track: tracks[index].title.clone(),
                matched: m.title.clone(),
                confidence: m.confidence,
            });
        }
        Err(e) => {
            let error = format!("{e:#}");
            match_errors.push((tracks[index].title.clone(), error.clone()));
            let done = matched_flags.iter().filter(|b| **b).count() + match_errors.len();
            let _ = tx.send(JobEvent::MatchError {
                done,
                total,
                track: tracks[index].title.clone(),
                error,
            });
        }
    });

    if cancel.load(Ordering::SeqCst) {
        summary.cancelled = true;
        summary.message = "Download cancelado durante a busca de correspondências.".into();
        return summary;
    }

    let mut matched_tracks: Vec<TrackMeta> = Vec::new();
    for (index, track) in tracks.iter().enumerate() {
        if matched_flags.get(index).copied().unwrap_or(false) {
            matched_tracks.push(track.clone());
        } else {
            summary.failed_names.push(track.title.clone());
        }
    }
    summary.matched = matched_tracks.len();
    summary.low_confidence = matched_tracks
        .iter()
        .filter(|t| t.match_confidence.map(|c| c < 0.45).unwrap_or(true))
        .count();
    summary.failed = total - matched_tracks.len();

    if matched_tracks.is_empty() {
        summary.message = format!(
            "Nenhuma das {} faixas foi encontrada no YouTube.{}",
            total,
            if let Some((title, error)) = match_errors.first() {
                format!("\nPrimeiro erro — {title}: {error}")
            } else {
                String::new()
            }
        );
        return summary;
    }

    // Preparação do lote: nada de metadados/capa do YouTube; o Spotify manda.
    opts.kind = MediaKind::Spotify;
    opts.tag_externally = true;
    opts.playlist_items = None;
    opts.playlist_end = None;

    let urls: Vec<String> = matched_tracks
        .iter()
        .filter_map(|t| t.download_id.as_deref().map(media::youtube_url))
        .collect();
    if urls.is_empty() {
        summary.message =
            "As faixas foram encontradas, mas os IDs de download estão vazios.".into();
        return summary;
    }
    let _ = tx.send(JobEvent::DownloadStarted {
        total: matched_tracks.len(),
        urls: urls.len(),
    });
    let _ = tx.send(JobEvent::Stage {
        title: "Download".into(),
        detail: format!(
            "Baixando {} do YouTube…",
            crate::util::plural(urls.len(), "faixa", "faixas")
        ),
    });

    let handle = match backend::start_download_many(&opts, &urls) {
        Ok(h) => h,
        Err(e) => {
            summary.message = format!("Não foi possível iniciar o download: {e:#}");
            return summary;
        }
    };
    if let Ok(mut slot) = child_slot.lock() {
        *slot = Some(Arc::clone(&handle.child));
    }
    let tmpdir = handle.tmpdir.clone();

    let mut seen: HashSet<String> = HashSet::new();
    let mut item_index: usize = 0;
    let mut item_total: usize = matched_tracks.len();
    let mut download_ok = false;
    let mut download_message = String::new();

    loop {
        if cancel.load(Ordering::SeqCst) {
            handle.kill();
            summary.cancelled = true;
            summary.message = "Download cancelado pelo usuário.".into();
            break;
        }
        let event = match handle.rx.recv() {
            Ok(event) => event,
            Err(_) => break,
        };
        match event {
            Progress::Line(line) => {
                let _ = tx.send(JobEvent::Line(line));
            }
            Progress::File { id } => {
                seen.insert(id);
            }
            Progress::PlaylistItem { index, total } => {
                item_index = index;
                item_total = total.max(1);
            }
            Progress::Archived { id } => {
                let _ = tx.send(JobEvent::Line(format!(
                    "↷ {id} já estava no arquivo do yt-dlp."
                )));
            }
            Progress::Percent {
                pct,
                file,
                speed,
                eta,
            } => {
                let base = if item_index > 0 {
                    (item_index - 1) as f32 + pct as f32 / 100.0
                } else {
                    seen.len().saturating_sub(1) as f32 + pct as f32 / 100.0
                };
                let fraction = (base / item_total.max(1) as f32).clamp(0.0, 1.0);
                let overall = 15.0 + fraction * 77.0;
                let label = matched_tracks
                    .iter()
                    .find(|t| t.file_key() == file)
                    .map(|t| format!("{} — {}", t.display_artist(), t.title));
                let _ = tx.send(JobEvent::Progress {
                    overall,
                    current: pct as f32,
                    file: label,
                    speed,
                    eta,
                });
            }
            Progress::Finished { success, message } => {
                download_ok = success;
                download_message = message;
                break;
            }
        }
    }
    if let Ok(mut slot) = child_slot.lock() {
        *slot = None;
    }

    if summary.cancelled {
        backend::cleanup_tmpdir(&tmpdir);
        return summary;
    }

    let _ = tx.send(JobEvent::Organizing);
    let _ = tx.send(JobEvent::Stage {
        title: "Organização".into(),
        detail: "Aplicando metadados, capas e movendo para a biblioteca…".into(),
    });

    let collection = match entity.kind {
        SpotifyKind::Track => None,
        _ => Some(entity.title.clone()),
    };
    let org_opts = OrganizeOptions {
        kind: MediaKind::Spotify,
        organize: opts.organize,
        number: opts.number_tracks,
        tag_externally: opts.embed_metadata,
        embed_cover: opts.embed_thumbnail,
        install_cover: opts.embed_thumbnail,
        write_m3u: opts.write_m3u,
        collection,
    };
    match organize::organize(&matched_tracks, &tmpdir, &base_dir, &org_opts) {
        Ok(report) => {
            let missing_len = report.missing.len();
            let warnings_len = report.warnings.len();
            summary.moved = report.moved;
            summary.covers = report.covers;
            summary.tagged = report.tagged;
            summary.missing = report.missing;
            summary.warnings = report.warnings;
            summary.m3u = report.m3u;
            summary.dirs = report.destination_dirs;
            summary.success = report.moved > 0;
            let mut message = if summary.success {
                format!(
                    "{} de {} {} na biblioteca.",
                    report.moved,
                    matched_tracks.len(),
                    if matched_tracks.len() == 1 {
                        "faixa"
                    } else {
                        "faixas"
                    }
                )
            } else {
                format!(
                    "Nenhum arquivo foi organizado. {}",
                    if download_message.is_empty() {
                        String::new()
                    } else {
                        format!("O yt-dlp informou: {download_message}.")
                    }
                )
            };
            if summary.failed > 0 {
                message.push_str(&format!(
                    " {} não {} no YouTube.",
                    summary.failed,
                    if summary.failed == 1 {
                        "encontrada"
                    } else {
                        "encontradas"
                    }
                ));
            }
            if missing_len > 0 {
                message.push_str(&format!(
                    " {missing_len} {} baixada{}, mas não localizada{} no diretório temporário.",
                    if missing_len == 1 { "faixa" } else { "faixas" },
                    if missing_len == 1 { "" } else { "s" },
                    if missing_len == 1 { "" } else { "s" }
                ));
            }
            if warnings_len > 0 {
                message.push_str(&format!(
                    " {} {}.",
                    warnings_len,
                    if warnings_len == 1 { "aviso" } else { "avisos" }
                ));
            }
            if !download_ok
                && summary.success
                && (report.moved < matched_tracks.len() || !summary.missing.is_empty())
            {
                message.push_str(" O yt-dlp relatou problemas em parte do lote.");
            }
            summary.message = message;
        }
        Err(e) => {
            summary.message = format!("Erro ao organizar os arquivos: {e:#}");
            summary.success = false;
            backend::cleanup_tmpdir(&tmpdir);
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_defaults() {
        let s = JobSummary::default();
        assert!(!s.success);
        assert_eq!(s.moved, 0);
    }
}
