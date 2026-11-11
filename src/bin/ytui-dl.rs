//! ytui-dl — versão em linha de comando do ytui.
//! Usa o mesmo backend do TUI: yt-dlp, ffmpeg, organização, Spotify e M3U.
//!
//! Exemplos:
//!   ytui-dl --url "https://open.spotify.com/playlist/..." --format flac
//!   ytui-dl "https://youtu.be/..." --kind musica
//!   ytui-dl --url URL --list
//!   ytui-dl --url URL --select 1,3-5 --json

use anyhow::{bail, Result};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use ytui::backend::{
    self, AudioFormat, AudioQuality, DownloadOptions, MediaKind, Progress, TrackMeta,
    VideoContainer, VideoQuality,
};
use ytui::config::Config;
use ytui::matching;
use ytui::media;
use ytui::organize::{self, OrganizeOptions};
use ytui::util;

struct Args {
    url: String,
    opts: DownloadOptions,
    base: String,
    select: Option<String>,
    list: bool,
    json: bool,
    quiet: bool,
    workers: usize,
    limit: Option<u32>,
}

fn parse_args() -> Result<Args> {
    let config = match Config::load() {
        Ok(config) => {
            if let Some(warning) = &config.startup_warning {
                eprintln!("aviso: {warning}");
            }
            config
        }
        Err(e) => {
            eprintln!("aviso: não foi possível ler a configuração ({e:#}); usando padrões.");
            Config::default()
        }
    };
    let mut opts = DownloadOptions::default();
    opts.apply_config(&config);
    let mut url = String::new();
    let mut base = config.base_dir.clone();
    let mut select = None;
    let mut list = false;
    let mut json = false;
    let mut quiet = false;
    let mut workers = config.spotify_workers;
    let mut limit: Option<u32> = None;
    let mut kind_set = false;

    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].clone();
        let take = |name: &str, i: &mut usize| -> Result<String> {
            *i += 1;
            args.get(*i)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("opção {name} requer um valor"))
        };
        match arg.as_str() {
            "--url" | "-u" => url = take("--url", &mut i)?,
            "--kind" | "-k" => {
                opts.kind = match take("--kind", &mut i)?.as_str() {
                    "musica" | "music" | "música" => MediaKind::Musica,
                    "video" | "vídeo" => MediaKind::Video,
                    "spotify" => MediaKind::Spotify,
                    "outro" | "other" => MediaKind::Outro,
                    k => bail!("kind inválido: {k}"),
                };
                kind_set = true;
            }
            "--format" | "-f" => {
                opts.audio_format = AudioFormat::from_key(&take("--format", &mut i)?);
            }
            "--video-format" => {
                opts.video_container = VideoContainer::from_key(&take("--video-format", &mut i)?);
            }
            "--quality" | "-q" => {
                opts.audio_quality = AudioQuality::from_key(&take("--quality", &mut i)?);
            }
            "--resolution" | "-r" => {
                opts.video_quality = VideoQuality::from_key(&take("--resolution", &mut i)?);
            }
            "--base" | "-b" => base = take("--base", &mut i)?,
            "--proxy" => opts.proxy = take("--proxy", &mut i)?,
            "--sleep" => opts.sleep_requests = take("--sleep", &mut i)?.parse()?,
            "--cookies" => opts.cookies_from_browser = take("--cookies", &mut i)?,
            "--limit-rate" => opts.limit_rate = take("--limit-rate", &mut i)?,
            "--workers" => workers = take("--workers", &mut i)?.parse()?,
            "--playlist-end" | "--limit" => limit = Some(take("--playlist-end", &mut i)?.parse()?),
            "--select" => select = Some(take("--select", &mut i)?),
            "--no-convert" => opts.convert = false,
            "--no-thumbnail" => opts.embed_thumbnail = false,
            "--no-metadata" => opts.embed_metadata = false,
            "--no-organize" => opts.organize = false,
            "--no-number" => opts.number_tracks = false,
            "--sponsorblock" => opts.sponsorblock = true,
            "--m3u" => opts.write_m3u = true,
            "--no-m3u" => opts.write_m3u = false,
            "--archive" => opts.download_archive = true,
            "--list" | "-l" => list = true,
            "--json" => {
                json = true;
                quiet = true;
            }
            "--quiet" => quiet = true,
            "-h" | "--help" => {
                println!("{HELP}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("ytui-dl {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other if !other.starts_with('-') && url.is_empty() => url = other.to_string(),
            other => bail!("argumento desconhecido: {other}\n\n{HELP}"),
        }
        i += 1;
    }

    if url.is_empty() {
        bail!("informe a URL (--url URL)\n\n{HELP}");
    }
    if !kind_set {
        // O Spotify é detectado pela URL; para os demais mantemos a dica.
        opts.kind = MediaKind::Musica;
    }
    if base.trim().is_empty() {
        base = config.base_dir.clone();
    }
    workers = workers.clamp(1, 8);
    Ok(Args {
        url,
        opts,
        base,
        select,
        list,
        json,
        quiet,
        workers,
        limit,
    })
}

const HELP: &str = "ytui-dl — download e organização via yt-dlp

USO:
  ytui-dl URL [opções]
  ytui-dl --url URL [opções]

OPÇÕES:
  --kind TIPO            musica | video | spotify | outro (detectado pela URL)
  --format FMT           mp3 | flac | opus | m4a | wav | original
  --video-format FMT     mp4 | webm | mkv | original
  --quality Q            maxima | alta | media | baixa
  --resolution R         melhor | 2160 | 1440 | 1080 | 720 | 480 | 360 | audio
  --base DIR             pasta base (padrão: config ou ~/Mídia)
  --proxy URL            proxy HTTP/SOCKS
  --sleep SEG            espera entre requisições
  --cookies NAVEGADOR    ex.: firefox | chrome | chromium (usa cookies logados)
  --limit-rate RATE      ex.: 4M, 500K
  --workers N            buscas Spotify em paralelo (1-8)
  --playlist-end N       limita às N primeiras faixas
  --select LISTA         ex.: 1,3-5,8
  --m3u                  gera lista M3U
  --archive              evita baixar de novo (arquivo do yt-dlp)
  --sponsorblock         remove patrocínios de vídeos
  --no-convert           não converte o formato
  --no-thumbnail         não baixa/embute capa
  --no-metadata          não incorpora metadados
  --no-organize          não cria pastas de artista/álbum
  --no-number            não numera faixas
  --list                 lista as faixas e sai
  --json                 saída JSON (silencioso)
  --quiet                menos mensagens
  -h, --help             esta ajuda
  -V, --version          versão

Spotify:
  Links públicos de música, álbum, playlist e artista. O Spotify fornece os
  metadados e a capa; o áudio é baixado da melhor correspondência no YouTube.
  Nenhum login ou chave de API é necessário.

Exemplos:
  ytui-dl --url https://open.spotify.com/track/ID --format flac
  ytui-dl https://open.spotify.com/playlist/ID --select 1-10 --m3u
  ytui-dl https://youtu.be/ID --kind musica --quality maxima
  ytui-dl --url https://open.spotify.com/album/ID --list";

fn select_tracks(
    tracks: &[TrackMeta],
    select: Option<&str>,
    limit: Option<u32>,
) -> Result<Vec<TrackMeta>> {
    let mut base: Vec<TrackMeta> = tracks.to_vec();
    if let Some(limit) = limit {
        if limit == 0 {
            bail!("--playlist-end deve ser maior que zero");
        }
        base.truncate(limit as usize);
    }
    if base.is_empty() {
        bail!("nenhuma faixa para processar");
    }
    let indices = match select {
        Some(selection) => util::parse_selection(selection, base.len())?,
        None => (1..=base.len()).collect(),
    };
    Ok(indices
        .into_iter()
        .filter_map(|index| base.get(index - 1).cloned())
        .collect())
}

fn print_tracks(tracks: &[TrackMeta], title: &str) {
    println!(
        "{title} — {}",
        util::plural(tracks.len(), "faixa", "faixas")
    );
    for (i, track) in tracks.iter().enumerate() {
        println!(
            "  {:>3}. {:<10} {} — {} ({}){}",
            i + 1,
            if track.is_spotify() {
                "spotify"
            } else {
                "youtube"
            },
            track.display_artist(),
            track.title,
            track
                .duration
                .map(util::format_duration)
                .unwrap_or_else(|| "—".into()),
            if track.explicit { " [E]" } else { "" }
        );
    }
}

fn main() -> Result<()> {
    let args = parse_args()?;
    if !util::tool_available("yt-dlp", &["--version"]) {
        bail!("yt-dlp não encontrado no PATH. Instale-o para continuar.");
    }

    let hint = args.opts.kind;
    if !args.quiet {
        println!("Analisando: {}", args.url);
    }
    let fetched = media::fetch(&args.url, hint)?;
    let title = fetched.title.clone();
    let all_tracks = fetched.tracks.clone();

    if args.list {
        let selected = select_tracks(&all_tracks, args.select.as_deref(), args.limit)?;
        print_tracks(&selected, &title);
        return Ok(());
    }

    let selected = select_tracks(&all_tracks, args.select.as_deref(), args.limit)?;
    if selected.is_empty() {
        bail!("nenhuma faixa selecionada");
    }
    if !args.quiet {
        println!(
            "Fonte: {} — {} faixa(s) selecionada(s).",
            fetched.kind.label(),
            selected.len()
        );
        println!("Destino: {}", args.base);
    }

    let is_spotify = fetched.kind == MediaKind::Spotify || fetched.spotify.is_some();
    let mut opts = args.opts.clone();
    let mut nothing_to_do = false;
    let mut low_confidence = 0usize;

    let (moved, covers, tagged, missing, mut warnings, m3u, dirs, matched, failed) = if is_spotify {
        if fetched.spotify.is_none() {
            bail!("metadados do Spotify indisponíveis");
        }
        let mut tracks = selected.clone();
        let input: Vec<(String, Vec<String>, Option<f64>)> = tracks
            .iter()
            .map(|t| {
                (
                    t.title.clone(),
                    t.artist
                        .clone()
                        .into_iter()
                        .chain(t.channel.clone())
                        .collect(),
                    t.duration,
                )
            })
            .collect();
        let cancel = AtomicBool::new(false);
        let total = tracks.len();
        let mut done = 0usize;
        if !args.quiet {
            println!(
                "Procurando {} no YouTube…",
                util::plural(total, "faixa", "faixas")
            );
        }
        let batch = matching::match_all(&input, args.workers, &cancel, |index, result| {
            done += 1;
            if args.quiet {
                return;
            }
            match result {
                Ok(m) => {
                    let marker = if m.confidence < 0.45 { "⚠" } else { "✓" };
                    print!(
                        "\r  [{done}/{total}] {marker} {} → {} ({:.0}%)",
                        util::truncate(&tracks[index].title, 30),
                        util::truncate(&m.title, 42),
                        m.confidence * 100.0
                    );
                }
                Err(e) => {
                    print!(
                        "\r  [{done}/{total}] ✗ {}: {}",
                        util::truncate(&tracks[index].title, 30),
                        util::truncate(&format!("{e:#}"), 60)
                    );
                }
            }
            let _ = std::io::stdout().flush();
        });
        if !args.quiet {
            println!();
        }
        for (index, result) in batch.matches.into_iter().enumerate() {
            if let Some(Ok(m)) = result {
                media::apply_match(&mut tracks[index], &m);
            }
        }
        let matched_tracks: Vec<TrackMeta> = tracks
            .into_iter()
            .filter(|t| t.download_id.is_some())
            .collect();
        let matched = matched_tracks.len();
        let failed = selected.len() - matched;
        low_confidence = matched_tracks
            .iter()
            .filter(|t| t.match_confidence.map(|c| c < 0.45).unwrap_or(true))
            .count();
        if matched_tracks.is_empty() {
            bail!("nenhuma faixa do Spotify foi encontrada no YouTube");
        }
        let urls: Vec<String> = matched_tracks
            .iter()
            .filter_map(|t| t.download_id.as_deref().map(media::youtube_url))
            .collect();

        opts.kind = MediaKind::Spotify;
        opts.tag_externally = true;
        opts.playlist_items = None;
        opts.playlist_end = None;

        if !args.quiet {
            println!(
                "Baixando {} do YouTube ({} sem correspondência)…",
                util::plural(urls.len(), "faixa", "faixas"),
                failed
            );
        }
        let labels: HashMap<String, String> = matched_tracks
            .iter()
            .map(|t| (t.file_key().to_string(), t.spotify_display()))
            .collect();
        let handle = backend::start_download_many(&opts, &urls)?;
        let _already_archived = drain_progress(&handle, args.quiet, &labels)?;
        let tmpdir = handle.tmpdir.clone();
        let collection = Some(title.clone());
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
        if !args.quiet {
            println!("Organizando arquivos e aplicando metadados…");
        }
        let report = organize::organize(
            &matched_tracks,
            &tmpdir,
            &PathBuf::from(&args.base),
            &org_opts,
        )?;
        (
            report.moved,
            report.covers,
            report.tagged,
            report.missing,
            report.warnings,
            report.m3u,
            report.destination_dirs,
            matched,
            failed,
        )
    } else {
        let mut tracks = selected.clone();
        let all_selected = tracks.len() == all_tracks.len();
        opts.playlist_items = if fetched.is_playlist && !all_selected {
            Some(
                tracks
                    .iter()
                    .map(|t| {
                        all_tracks
                            .iter()
                            .position(|x| x.id == t.id)
                            .map(|p| p + 1)
                            .unwrap_or(1)
                    })
                    .collect(),
            )
        } else {
            None
        };
        opts.kind = fetched.kind;
        opts.tag_externally = false;
        if !args.quiet {
            println!("Baixando com o yt-dlp…");
        }
        let labels: HashMap<String, String> = tracks
            .iter()
            .map(|t| (t.file_key().to_string(), t.spotify_display()))
            .collect();
        let handle = backend::start_download(&opts, &args.url)?;
        let archived = drain_progress(&handle, args.quiet, &labels)?;
        if !archived.is_empty() {
            tracks.retain(|t| !archived.contains(&t.id));
        }
        let tmpdir = handle.tmpdir.clone();
        let collection = if fetched.is_playlist {
            Some(title.clone())
        } else {
            None
        };
        let org_opts = OrganizeOptions {
            kind: fetched.kind,
            organize: opts.organize,
            number: opts.number_tracks,
            tag_externally: false,
            embed_cover: false,
            install_cover: opts.embed_thumbnail,
            write_m3u: opts.write_m3u,
            collection,
        };
        if tracks.is_empty() {
            backend::cleanup_tmpdir(&tmpdir);
            nothing_to_do = true;
            if !args.quiet {
                println!("Nada a baixar: os itens já constavam no arquivo do yt-dlp.");
            }
            (
                0,
                0,
                0,
                Vec::new(),
                Vec::new(),
                None,
                vec![PathBuf::from(&args.base)],
                selected.len(),
                0usize,
            )
        } else {
            if !args.quiet {
                println!("Organizando arquivos…");
            }
            let report =
                organize::organize(&tracks, &tmpdir, &PathBuf::from(&args.base), &org_opts)?;
            let matched = tracks.len();
            (
                report.moved,
                report.covers,
                report.tagged,
                report.missing,
                report.warnings,
                report.m3u,
                report.destination_dirs,
                matched,
                0usize,
            )
        }
    };

    if is_spotify && low_confidence > 0 {
        warnings.push(format!(
            "{low_confidence} faixa(s) com correspondência de baixa confiança; confira o resultado."
        ));
    }
    let success = moved > 0 || nothing_to_do;
    if args.json {
        let output = serde_json::json!({
            "ok": success,
            "source": if is_spotify { "spotify" } else { "youtube" },
            "title": title,
            "matched": matched,
            "failed": if is_spotify { failed } else { 0 },
            "already_in_archive": nothing_to_do,
            "moved": moved,
            "covers": covers,
            "tagged": tagged,
            "missing": missing,
            "warnings": warnings,
            "m3u": m3u.as_ref().map(|p| p.to_string_lossy().to_string()),
            "dirs": dirs.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        if !args.quiet {
            println!();
        }
        println!(
            "Concluído: {} {}, {} {}, {} com metadados.",
            moved,
            if moved == 1 { "arquivo" } else { "arquivos" },
            covers,
            if covers == 1 { "capa" } else { "capas" },
            tagged
        );
        if !missing.is_empty() {
            println!("Não localizados: {}", missing.join("; "));
        }
        for warning in &warnings {
            eprintln!("aviso: {warning}");
        }
        if let Some(m3u) = &m3u {
            println!("Lista M3U: {}", m3u.display());
        }
        for dir in &dirs {
            println!("Destino: {}", dir.display());
        }
    }
    if !success {
        bail!("nenhum arquivo foi organizado");
    }
    Ok(())
}

fn drain_progress(
    handle: &backend::DownloadHandle,
    quiet: bool,
    labels: &HashMap<String, String>,
) -> Result<Vec<String>> {
    let mut last_pct = -1.0f64;
    let mut archived: Vec<String> = Vec::new();
    loop {
        match handle.rx.recv_timeout(Duration::from_secs(300)) {
            Ok(Progress::Line(line)) => {
                if !quiet
                    && (line.starts_with("ERRO") || line.to_ascii_lowercase().contains("error"))
                {
                    eprintln!("  {line}");
                }
            }
            Ok(Progress::File { .. }) | Ok(Progress::PlaylistItem { .. }) => {}
            Ok(Progress::Archived { id }) => archived.push(id),
            Ok(Progress::Percent {
                pct,
                file,
                speed,
                eta,
            }) => {
                if !quiet && ((pct - last_pct).abs() >= 5.0 || pct >= 100.0) {
                    last_pct = pct;
                    let display = labels.get(&file).cloned().unwrap_or(file);
                    let name = util::truncate(&display, 30);
                    let speed = speed.unwrap_or_default();
                    let eta = eta.map(|e| format!(" ETA {e}")).unwrap_or_default();
                    print!("\r\x1b[2K  {name:<30} {pct:5.1}%  {speed:<12}{eta}");
                    let _ = std::io::stdout().flush();
                }
            }
            Ok(Progress::Finished { success, message }) => {
                if !quiet {
                    println!();
                }
                if !success {
                    eprintln!("{message}");
                }
                return Ok(archived);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                bail!("o yt-dlp não respondeu por muito tempo");
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Ok(archived);
            }
        }
    }
}
