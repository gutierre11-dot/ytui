use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use ytui::backend::{
    self, check_tool, AudioFormat, AudioQuality, DownloadHandle, DownloadOptions, MediaKind,
    Progress, TrackMeta, VideoContainer, VideoQuality,
};
use ytui::config::Config;
use ytui::matching;
use ytui::media::{self, FetchedMedia};
use ytui::organize::{self, OrganizeOptions};
use ytui::pipeline::{self, JobEvent, JobHandle, JobSummary};
use ytui::theme::{Palette, Theme};
use ytui::util;

// ---------------------------------------------------------------------------
// Constantes compartilhadas com a interface
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Menu,
    Url,
    Tracks,
    Options,
    Progress,
    History,
    Settings,
    Done,
}

pub const MENU_ITEMS: [&str; 7] = [
    "YouTube Music",
    "YouTube",
    "Spotify",
    "Outro site",
    "Histórico",
    "Configurações",
    "Sair",
];

pub const MENU_DESCRIPTIONS: [&str; 7] = [
    "Músicas, álbuns e playlists do YouTube Music",
    "Vídeos e playlists do YouTube em qualquer resolução",
    "Músicas, álbuns e playlists do Spotify (áudio via YouTube)",
    "Qualquer link aceito pelo yt-dlp",
    "Downloads anteriores, com opção de repetir",
    "Preferências, tema e diagnóstico das ferramentas",
    "Encerrar o ytui",
];

pub const MENU_ICONS: [&str; 7] = ["♫", "▶", "●", "≠", "◷", "⚙", "⏻"];

pub const OPTION_ROWS: usize = 11;
pub const SETTINGS_ROWS: usize = 19;

pub const SET_BASE: usize = 0;
pub const SET_PROXY: usize = 5;
pub const SET_COOKIES: usize = 11;
pub const SET_LIMIT_RATE: usize = 12;
pub const SET_UPDATE: usize = 16;
pub const SET_ABOUT: usize = 17;
pub const SET_BACK: usize = 18;

// ---------------------------------------------------------------------------
// Estado da interface
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub until: Instant,
}

pub struct App {
    pub screen: Screen,
    pub menu_sel: usize,

    // URL / metadados
    pub url: String,
    pub url_cursor: usize,
    pub hint_kind: MediaKind,
    pub loading_meta: bool,
    pub meta_rx: Option<Receiver<Result<FetchedMedia>>>,
    pub fetched: Option<FetchedMedia>,
    pub url_error: Option<String>,

    // Seleção de faixas
    pub tracks: Vec<TrackMeta>,
    pub track_sel: usize,
    pub track_selected: Vec<bool>,
    pub track_filter: String,
    pub filtering: bool,
    pub filter_buf: String,

    // Opções de download
    pub opts: DownloadOptions,
    pub opt_sel: usize,

    // Download em andamento
    pub download: Option<DownloadHandle>,
    pub job: Option<JobHandle>,
    pub active_tracks: Vec<TrackMeta>,
    pub active_url: String,
    pub active_title: String,
    pub active_kind: MediaKind,
    pub active_labels: std::collections::HashMap<String, String>,
    pub item_index: usize,
    pub item_total: usize,
    pub seen_files: usize,
    pub archived_ids: std::collections::HashSet<String>,
    pub cancelled: bool,
    pub organizing: bool,
    pub organize_rx: Option<Receiver<Result<organize::OrganizeReport>>>,
    pub pending_dl: Option<(bool, String)>,
    pub started_at: Option<Instant>,

    // Progresso exibido
    pub progress_pct: f32,
    pub progress_current: f32,
    pub progress_name: String,
    pub stage: String,
    pub detail: String,
    pub speed: Option<String>,
    pub eta: Option<String>,
    pub log: Vec<String>,
    pub log_scroll: usize,

    // Configurações e histórico
    pub config: Config,
    pub settings_sel: usize,
    pub settings_editing: Option<usize>,
    pub edit_buf: String,
    pub edit_orig: String,
    pub about_open: bool,
    pub updating: bool,
    pub update_rx: Option<Receiver<String>>,
    pub update_msg: String,
    pub tool_status: Vec<(String, bool, String)>,
    pub tool_rx: Option<Receiver<Vec<(String, bool, String)>>>,
    pub history_sel: usize,
    pub pending_clear_history: bool,

    // Resultado
    pub done_success: bool,
    pub done_message: String,
    pub done_missing: Vec<String>,
    pub done_warnings: Vec<String>,
    pub done_dirs: Vec<PathBuf>,
    pub done_m3u: Option<PathBuf>,
    pub done_moved: usize,
    pub done_covers: usize,
    pub done_tagged: usize,
    pub done_total: usize,
    pub done_title: String,
    pub done_kind: MediaKind,

    // Globais
    pub tick: u64,
    pub toast: Option<Toast>,
    pub help_open: bool,
    pub help_scroll: u16,
    pub quit: bool,
}

impl App {
    pub fn new(config: Config) -> Self {
        let startup_warning = config.startup_warning.clone();
        let mut app = Self {
            screen: Screen::Menu,
            menu_sel: 0,
            url: String::new(),
            url_cursor: 0,
            hint_kind: MediaKind::Musica,
            loading_meta: false,
            meta_rx: None,
            fetched: None,
            url_error: None,
            tracks: Vec::new(),
            track_sel: 0,
            track_selected: Vec::new(),
            track_filter: String::new(),
            filtering: false,
            filter_buf: String::new(),
            opts: DownloadOptions::default(),
            opt_sel: 0,
            download: None,
            job: None,
            active_tracks: Vec::new(),
            active_url: String::new(),
            active_title: String::new(),
            active_kind: MediaKind::Musica,
            active_labels: std::collections::HashMap::new(),
            item_index: 0,
            item_total: 0,
            seen_files: 0,
            archived_ids: std::collections::HashSet::new(),
            cancelled: false,
            organizing: false,
            organize_rx: None,
            pending_dl: None,
            started_at: None,
            progress_pct: 0.0,
            progress_current: 0.0,
            progress_name: String::new(),
            stage: String::new(),
            detail: String::new(),
            speed: None,
            eta: None,
            log: Vec::new(),
            log_scroll: 0,
            config,
            settings_sel: 0,
            settings_editing: None,
            edit_buf: String::new(),
            edit_orig: String::new(),
            about_open: false,
            updating: false,
            update_rx: None,
            update_msg: String::new(),
            tool_status: Vec::new(),
            tool_rx: None,
            history_sel: 0,
            pending_clear_history: false,
            done_success: false,
            done_message: String::new(),
            done_missing: Vec::new(),
            done_warnings: Vec::new(),
            done_dirs: Vec::new(),
            done_m3u: None,
            done_moved: 0,
            done_covers: 0,
            done_tagged: 0,
            done_total: 0,
            done_title: String::new(),
            done_kind: MediaKind::Musica,
            tick: 0,
            toast: None,
            help_open: false,
            help_scroll: 0,
            quit: false,
        };
        app.opts.apply_config(&app.config);
        if let Some(warning) = startup_warning {
            app.toast_error(warning);
        }
        app
    }

    pub fn palette(&self) -> Palette {
        self.config.theme().palette()
    }

    pub fn theme(&self) -> Theme {
        self.config.theme()
    }

    pub fn on_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    pub fn toast(&mut self, text: impl Into<String>, kind: ToastKind) {
        self.toast = Some(Toast {
            text: text.into(),
            kind,
            until: Instant::now() + Duration::from_secs(4),
        });
    }

    pub fn toast_error(&mut self, text: impl Into<String>) {
        self.toast(text, ToastKind::Error);
    }

    pub fn toast_success(&mut self, text: impl Into<String>) {
        self.toast(text, ToastKind::Success);
    }

    pub fn visible_toast(&self) -> Option<&Toast> {
        self.toast.as_ref().filter(|t| Instant::now() < t.until)
    }

    pub fn draw(&self, f: &mut ratatui::Frame) {
        crate::ui::draw(f, self);
    }

    /// Verifica as ferramentas externas em segundo plano para não travar a
    /// interface (o yt-dlp pode levar mais de um segundo para iniciar).
    pub fn refresh_tools(&mut self) {
        if self.tool_rx.is_some() {
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(backend::tool_status());
        });
        self.tool_rx = Some(rx);
    }

    // -----------------------------------------------------------------------
    // Teclado
    // -----------------------------------------------------------------------

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind == crossterm::event::KeyEventKind::Release {
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            self.request_quit();
            return;
        }
        if key.code == KeyCode::F(1) {
            if self.help_open {
                self.help_open = false;
            } else {
                self.open_help();
            }
            return;
        }
        if key.code == KeyCode::Char('t') && self.theme_shortcut_allowed() {
            self.cycle_theme();
            return;
        }
        if self.help_open {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::Enter => {
                    self.help_open = false;
                }
                KeyCode::PageUp => self.help_scroll = self.help_scroll.saturating_sub(6),
                KeyCode::PageDown => self.help_scroll = self.help_scroll.saturating_add(6),
                KeyCode::Home => self.help_scroll = 0,
                KeyCode::End => self.help_scroll = 100,
                _ => {}
            }
            return;
        }
        if self.about_open {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')) {
                self.about_open = false;
            }
            return;
        }
        match self.screen {
            Screen::Menu => self.key_menu(key),
            Screen::Url => self.key_url(key),
            Screen::Tracks => self.key_tracks(key),
            Screen::Options => self.key_options(key),
            Screen::Progress => self.key_progress(key),
            Screen::History => self.key_history(key),
            Screen::Settings => self.key_settings(key),
            Screen::Done => self.key_done(key),
        }
    }

    fn open_help(&mut self) {
        self.help_scroll = 0;
        self.help_open = true;
    }

    fn theme_shortcut_allowed(&self) -> bool {
        !(self.screen == Screen::Url
            || (self.screen == Screen::Tracks && self.filtering)
            || (self.screen == Screen::Settings && self.settings_editing.is_some()))
    }

    /// Texto colado pelo terminal (bracketed paste) ou pelo atalho Ctrl+V.
    pub fn handle_paste(&mut self, text: &str) {
        let clean = text.replace(['\r', '\n'], " ").trim().to_string();
        if clean.is_empty() {
            return;
        }
        if self.screen == Screen::Url && !self.loading_meta {
            let value = util::extract_url(&clean);
            self.url.insert_str(self.url_cursor, &value);
            self.url_cursor += value.len();
            return;
        }
        if self.screen == Screen::Tracks && self.filtering {
            self.filter_buf.push_str(&clean);
            self.track_filter = self.filter_buf.clone();
            self.clamp_track_sel();
            return;
        }
        if self.screen == Screen::Settings {
            if let Some(row) = self.settings_editing {
                if matches!(row, SET_BASE | SET_PROXY | SET_COOKIES | SET_LIMIT_RATE) {
                    self.edit_buf.push_str(&clean);
                }
            }
        }
    }

    fn request_quit(&mut self) {
        if let Some(handle) = &self.download {
            handle.kill();
            backend::cleanup_tmpdir(&handle.tmpdir);
        }
        if let Some(job) = &self.job {
            job.cancel();
        }
        self.quit = true;
    }

    // -----------------------------------------------------------------------
    // Menu
    // -----------------------------------------------------------------------

    fn key_menu(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.request_quit(),
            KeyCode::Up => self.menu_sel = self.menu_sel.saturating_sub(1),
            KeyCode::Down => self.menu_sel = (self.menu_sel + 1).min(MENU_ITEMS.len() - 1),
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Enter => match self.menu_sel {
                0 => self.enter_url_screen(MediaKind::Musica),
                1 => self.enter_url_screen(MediaKind::Video),
                2 => self.enter_url_screen(MediaKind::Spotify),
                3 => self.enter_url_screen(MediaKind::Outro),
                4 => self.open_history(),
                5 => self.open_settings(),
                _ => self.request_quit(),
            },
            _ => {}
        }
    }

    fn cycle_theme(&mut self) {
        let current = self.config.theme();
        let index = Theme::ALL.iter().position(|t| *t == current).unwrap_or(0);
        let next = Theme::ALL[(index + 1) % Theme::ALL.len()];
        self.config.theme = next.key().into();
        let _ = self.config.save();
        self.toast(format!("Tema: {}", next.label()), ToastKind::Info);
    }

    fn enter_url_screen(&mut self, kind: MediaKind) {
        self.open_url_screen(kind, String::new());
    }

    fn open_url_screen(&mut self, kind: MediaKind, prefill: String) {
        self.hint_kind = kind;
        self.opts.kind = kind;
        self.opts.apply_config(&self.config);
        self.url = prefill;
        self.url_cursor = self.url.len();
        self.url_error = None;
        self.loading_meta = false;
        self.meta_rx = None;
        self.fetched = None;
        self.tracks.clear();
        self.track_selected.clear();
        self.track_filter.clear();
        self.filter_buf.clear();
        self.filtering = false;
        self.track_sel = 0;
        self.screen = Screen::Url;
    }

    // -----------------------------------------------------------------------
    // URL
    // -----------------------------------------------------------------------

    fn key_url(&mut self, key: KeyEvent) {
        if self.loading_meta {
            if key.code == KeyCode::Esc {
                self.loading_meta = false;
                self.meta_rx = None;
                self.toast("Análise cancelada.", ToastKind::Info);
            }
            return;
        }
        match key.code {
            KeyCode::Esc => self.screen = Screen::Menu,
            KeyCode::Enter => self.start_fetch(),
            KeyCode::Char('?') if self.url.is_empty() => self.open_help(),
            KeyCode::Backspace => {
                if self.url_cursor > 0 {
                    let prev = self.url[..self.url_cursor]
                        .char_indices()
                        .last()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.url.replace_range(prev..self.url_cursor, "");
                    self.url_cursor = prev;
                }
            }
            KeyCode::Delete => {
                if self.url_cursor < self.url.len() {
                    let next = self.url[self.url_cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.url_cursor + i)
                        .unwrap_or(self.url.len());
                    self.url.replace_range(self.url_cursor..next, "");
                }
            }
            KeyCode::Left => {
                if self.url_cursor > 0 {
                    self.url_cursor = self.url[..self.url_cursor]
                        .char_indices()
                        .last()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
            }
            KeyCode::Right => {
                if self.url_cursor < self.url.len() {
                    self.url_cursor = self.url[self.url_cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.url_cursor + i)
                        .unwrap_or(self.url.len());
                }
            }
            KeyCode::Home => self.url_cursor = 0,
            KeyCode::End => self.url_cursor = self.url.len(),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.url.clear();
                self.url_cursor = 0;
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.url_cursor = 0;
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.url_cursor = self.url.len();
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let prefix = &self.url[..self.url_cursor];
                let cut = prefix
                    .trim_end()
                    .rfind(char::is_whitespace)
                    .map(|i| i + 1)
                    .unwrap_or(0);
                self.url.replace_range(cut..self.url_cursor, "");
                self.url_cursor = cut;
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'v' {
                    self.try_clipboard_paste();
                } else if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                {
                    self.url.insert(self.url_cursor, c);
                    self.url_cursor += c.len_utf8();
                }
            }
            _ => {}
        }
    }

    fn start_fetch(&mut self) {
        let url = util::extract_url(&self.url);
        if url.is_empty() {
            self.toast_error("Cole ou digite um link antes de continuar.");
            return;
        }
        self.url = url.clone();
        self.url_cursor = self.url.len();
        self.loading_meta = true;
        self.url_error = None;
        self.log.clear();
        let hint = self.hint_kind;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(media::fetch(&url, hint));
        });
        self.meta_rx = Some(rx);
    }

    fn try_clipboard_paste(&mut self) {
        for cmd in [
            vec!["wl-paste", "--no-newline"],
            vec!["xclip", "-selection", "clipboard", "-o"],
            vec!["xsel", "--clipboard", "--output"],
        ] {
            if let Ok(out) = std::process::Command::new(cmd[0]).args(&cmd[1..]).output() {
                if out.status.success() {
                    let clip = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !clip.is_empty() {
                        self.handle_paste(&clip);
                        return;
                    }
                }
            }
        }
        self.toast_error("Não foi possível acessar a área de transferência.");
    }

    // -----------------------------------------------------------------------
    // Seleção de faixas
    // -----------------------------------------------------------------------

    pub fn filtered_track_indices(&self) -> Vec<usize> {
        if self.track_filter.trim().is_empty() {
            return (0..self.tracks.len()).collect();
        }
        let needle = matching::fold(&self.track_filter);
        self.tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                let haystack = format!(
                    "{} {} {}",
                    t.title,
                    t.display_artist(),
                    t.album.clone().unwrap_or_default()
                );
                matching::fold(&haystack).contains(&needle)
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub fn track_selected_count(&self) -> usize {
        self.track_selected.iter().filter(|b| **b).count()
    }

    fn current_track_index(&self) -> Option<usize> {
        self.filtered_track_indices().get(self.track_sel).copied()
    }

    fn clamp_track_sel(&mut self) {
        let len = self.filtered_track_indices().len();
        if len == 0 {
            self.track_sel = 0;
        } else if self.track_sel >= len {
            self.track_sel = len - 1;
        }
    }

    fn key_tracks(&mut self, key: KeyEvent) {
        if self.filtering {
            match key.code {
                KeyCode::Esc => {
                    self.filtering = false;
                    self.track_filter.clear();
                    self.filter_buf.clear();
                    self.clamp_track_sel();
                }
                KeyCode::Enter => self.filtering = false,
                KeyCode::Backspace => {
                    self.filter_buf.pop();
                    self.track_filter = self.filter_buf.clone();
                    self.clamp_track_sel();
                }
                KeyCode::Char(c) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'v' {
                        self.try_clipboard_paste();
                    } else if !key.modifiers.contains(KeyModifiers::CONTROL) {
                        self.filter_buf.push(c);
                        self.track_filter = self.filter_buf.clone();
                        self.clamp_track_sel();
                    }
                }
                _ => {}
            }
            return;
        }

        let filtered = self.filtered_track_indices();
        let len = filtered.len();
        match key.code {
            KeyCode::Esc => {
                if !self.track_filter.is_empty() {
                    self.track_filter.clear();
                    self.filter_buf.clear();
                    self.track_sel = 0;
                } else {
                    self.screen = Screen::Url;
                }
            }
            KeyCode::Up => self.track_sel = self.track_sel.saturating_sub(1),
            KeyCode::Down => {
                if len > 0 {
                    self.track_sel = (self.track_sel + 1).min(len - 1);
                }
            }
            KeyCode::PageUp => self.track_sel = self.track_sel.saturating_sub(10),
            KeyCode::PageDown => {
                if len > 0 {
                    self.track_sel = (self.track_sel + 10).min(len - 1);
                }
            }
            KeyCode::Home => self.track_sel = 0,
            KeyCode::End => self.track_sel = len.saturating_sub(1),
            KeyCode::Char(' ') => {
                if let Some(index) = self.current_track_index() {
                    if let Some(flag) = self.track_selected.get_mut(index) {
                        *flag = !*flag;
                    }
                }
            }
            KeyCode::Char('a') => {
                for index in filtered {
                    if let Some(flag) = self.track_selected.get_mut(index) {
                        *flag = true;
                    }
                }
            }
            KeyCode::Char('n') => {
                for index in filtered {
                    if let Some(flag) = self.track_selected.get_mut(index) {
                        *flag = false;
                    }
                }
            }
            KeyCode::Char('i') => {
                for index in filtered {
                    if let Some(flag) = self.track_selected.get_mut(index) {
                        *flag = !*flag;
                    }
                }
            }
            KeyCode::Char('/') => {
                self.filtering = true;
                self.filter_buf = self.track_filter.clone();
            }
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Enter => {
                if self.track_selected_count() == 0 {
                    self.toast_error("Selecione pelo menos uma faixa (Espaço).");
                } else {
                    self.open_options();
                }
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Opções
    // -----------------------------------------------------------------------

    fn open_options(&mut self) {
        self.opt_sel = self.opt_sel.min(OPTION_ROWS - 1);
        self.screen = Screen::Options;
        if self.fetched.as_ref().map(|f| f.kind) == Some(MediaKind::Spotify) {
            self.toast(
                "Spotify: metadados oficiais + áudio correspondente do YouTube.",
                ToastKind::Info,
            );
        }
    }

    fn key_options(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.screen = if self.is_collection() {
                    Screen::Tracks
                } else {
                    Screen::Url
                };
            }
            KeyCode::Up => self.opt_sel = self.opt_sel.saturating_sub(1),
            KeyCode::Down => self.opt_sel = (self.opt_sel + 1).min(OPTION_ROWS - 1),
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
                if self.opt_sel < OPTION_ROWS - 2 {
                    self.cycle_option();
                }
            }
            KeyCode::Char('s') => self.start_download_flow(),
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Enter => match self.opt_sel {
                i if i == OPTION_ROWS - 2 => self.start_download_flow(),
                i if i == OPTION_ROWS - 1 => {
                    self.screen = if self.is_collection() {
                        Screen::Tracks
                    } else {
                        Screen::Url
                    }
                }
                _ => self.cycle_option(),
            },
            _ => {}
        }
    }

    fn is_collection(&self) -> bool {
        self.fetched
            .as_ref()
            .map(|f| f.is_playlist)
            .unwrap_or(false)
    }

    fn cycle_option(&mut self) {
        let o = &mut self.opts;
        match self.opt_sel {
            0 => {
                if o.kind.is_audio() {
                    let all = AudioFormat::ALL;
                    let cur = all.iter().position(|f| *f == o.audio_format).unwrap_or(0);
                    o.audio_format = all[(cur + 1) % all.len()];
                } else {
                    let all = VideoContainer::ALL;
                    let cur = all
                        .iter()
                        .position(|c| *c == o.video_container)
                        .unwrap_or(0);
                    o.video_container = all[(cur + 1) % all.len()];
                }
            }
            1 => {
                if o.kind.is_audio() {
                    let all = AudioQuality::ALL;
                    let cur = all.iter().position(|q| *q == o.audio_quality).unwrap_or(0);
                    o.audio_quality = all[(cur + 1) % all.len()];
                } else {
                    let all = VideoQuality::ALL;
                    let cur = all.iter().position(|q| *q == o.video_quality).unwrap_or(0);
                    o.video_quality = all[(cur + 1) % all.len()];
                }
            }
            2 => o.convert = !o.convert,
            3 => o.embed_thumbnail = !o.embed_thumbnail,
            4 => o.embed_metadata = !o.embed_metadata,
            5 => o.organize = !o.organize,
            6 => o.number_tracks = !o.number_tracks,
            7 => o.sponsorblock = !o.sponsorblock,
            8 => o.write_m3u = !o.write_m3u,
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Início do download
    // -----------------------------------------------------------------------

    fn start_download_flow(&mut self) {
        let Some(fetched) = self.fetched.clone() else {
            self.toast_error("Nenhum link analisado.");
            return;
        };
        let selected = fetched.selected(&self.track_selected);
        if selected.is_empty() {
            self.toast_error("Selecione pelo menos uma faixa.");
            return;
        }
        let positions = fetched.selected_positions(&self.track_selected);
        self.active_tracks = selected;
        self.active_url = self.url.trim().to_string();
        self.active_title = fetched.title.clone();
        self.active_kind = fetched.kind;
        self.active_labels = backend::label_map(&self.active_tracks);
        self.cancelled = false;
        self.organizing = false;
        self.organize_rx = None;
        self.pending_dl = None;
        self.item_index = 0;
        self.item_total = self.active_tracks.len();
        self.seen_files = 0;
        self.archived_ids.clear();
        self.progress_pct = 0.0;
        self.progress_current = 0.0;
        self.progress_name.clear();
        self.speed = None;
        self.eta = None;
        self.log.clear();
        self.log_scroll = 0;
        self.started_at = Some(Instant::now());

        if fetched.kind == MediaKind::Spotify {
            let Some(entity) = fetched.spotify.clone() else {
                self.toast_error("Dados do Spotify indisponíveis.");
                return;
            };
            self.opts.kind = MediaKind::Spotify;
            self.opts.playlist_items = None;
            self.opts.playlist_end = None;
            self.stage = "Correspondências".into();
            self.detail = format!(
                "Procurando {} no YouTube…",
                util::plural(self.active_tracks.len(), "faixa", "faixas")
            );
            let job = pipeline::start_spotify(
                entity,
                self.active_tracks.clone(),
                self.opts.clone(),
                PathBuf::from(&self.config.base_dir),
                self.config.spotify_workers,
            );
            self.job = Some(job);
        } else {
            let all_selected = self.active_tracks.len() == fetched.tracks.len();
            self.opts.playlist_items = if fetched.is_playlist && !all_selected {
                Some(positions)
            } else {
                None
            };
            self.stage = "Download".into();
            self.detail = format!(
                "Baixando {} com o yt-dlp…",
                util::plural(self.active_tracks.len(), "faixa", "faixas")
            );
            match backend::start_download(&self.opts, &self.active_url) {
                Ok(handle) => self.download = Some(handle),
                Err(e) => {
                    self.done_success = false;
                    self.done_message = format!("Erro ao iniciar o download: {e:#}");
                    self.done_missing.clear();
                    self.done_warnings.clear();
                    self.screen = Screen::Done;
                    return;
                }
            }
        }
        self.screen = Screen::Progress;
    }

    // -----------------------------------------------------------------------
    // Progresso
    // -----------------------------------------------------------------------

    fn key_progress(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                if self.organizing {
                    self.toast("A organização já está terminando…", ToastKind::Info);
                    return;
                }
                if self.cancelled {
                    return;
                }
                self.cancelled = true;
                self.stage = "Cancelando".into();
                self.detail = "Encerrando o yt-dlp e limpando os arquivos temporários…".into();
                if let Some(handle) = &self.download {
                    handle.kill();
                }
                if let Some(job) = &self.job {
                    job.cancel();
                }
                if self.download.is_some() {
                    if let Some(handle) = self.download.take() {
                        backend::cleanup_tmpdir(&handle.tmpdir);
                    }
                    self.finish_cancelled();
                }
            }
            KeyCode::Char('?') => self.open_help(),
            KeyCode::PageUp => self.log_scroll = self.log_scroll.saturating_sub(10),
            KeyCode::PageDown => {
                self.log_scroll = (self.log_scroll + 10).min(self.log.len().saturating_sub(1));
            }
            KeyCode::Home => self.log_scroll = 0,
            KeyCode::End => self.log_scroll = self.log.len().saturating_sub(1),
            _ => {}
        }
    }

    fn finish_cancelled(&mut self) {
        self.job = None;
        self.organizing = false;
        self.organize_rx = None;
        self.pending_dl = None;
        self.done_success = false;
        self.done_message = "Download cancelado pelo usuário.".into();
        self.done_missing.clear();
        self.done_warnings.clear();
        self.done_dirs = vec![PathBuf::from(&self.config.base_dir)];
        self.done_m3u = None;
        self.done_moved = 0;
        self.done_covers = 0;
        self.done_tagged = 0;
        self.done_total = self.active_tracks.len();
        self.done_title = self.active_title.clone();
        self.done_kind = self.active_kind;
        self.screen = Screen::Done;
        self.toast("Download cancelado.", ToastKind::Info);
    }

    fn push_log(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > 500 {
            let excess = self.log.len() - 500;
            self.log.drain(..excess);
            self.log_scroll = self.log_scroll.saturating_sub(excess);
        }
    }

    // -----------------------------------------------------------------------
    // Conclusão
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn set_done(
        &mut self,
        success: bool,
        message: String,
        moved: usize,
        covers: usize,
        tagged: usize,
        total: usize,
        missing: Vec<String>,
        warnings: Vec<String>,
        dirs: Vec<PathBuf>,
        m3u: Option<PathBuf>,
    ) {
        self.done_success = success;
        self.done_message = message;
        self.done_moved = moved;
        self.done_covers = covers;
        self.done_tagged = tagged;
        self.done_total = total;
        self.done_missing = missing;
        self.done_warnings = warnings;
        self.done_dirs = if dirs.is_empty() {
            vec![PathBuf::from(&self.config.base_dir)]
        } else {
            dirs
        };
        self.done_m3u = m3u;
        self.done_title = self.active_title.clone();
        self.done_kind = self.active_kind;
        if moved > 0 {
            let kind = self.active_kind.key().to_string();
            let url = self.active_url.clone();
            let base = self.config.base_dir.clone();
            let title = self.active_title.clone();
            self.config
                .record_download(&kind, &url, total, &base, &title);
            let _ = self.config.save();
        }
        self.organizing = false;
        self.started_at = None;
        self.screen = Screen::Done;
        if self.config.notifications {
            let (summary, body) = if success {
                (
                    "Download concluído",
                    format!(
                        "{moved} arquivo(s) organizado(s) em {}",
                        self.config.base_dir
                    ),
                )
            } else {
                (
                    "Download com problemas",
                    util::truncate(&self.done_message, 180),
                )
            };
            util::desktop_notification(summary, &body);
        }
        if self.config.open_folder_after {
            if let Some(dir) = self.done_dirs.first() {
                let _ = util::open_path(dir);
            }
        }
    }

    fn finish_youtube_organize(&mut self, result: Result<organize::OrganizeReport>) {
        let (dl_success, dl_message) = self.pending_dl.take().unwrap_or((true, String::new()));
        self.organize_rx = None;
        match result {
            Ok(report) => {
                let mut err_tail: Vec<String> = self
                    .log
                    .iter()
                    .rev()
                    .filter(|l| {
                        l.starts_with("ERRO")
                            || l.to_ascii_lowercase().contains("error")
                            || l.to_ascii_lowercase().contains("erro")
                    })
                    .take(6)
                    .cloned()
                    .collect();
                err_tail.reverse();
                let err_text = if err_tail.is_empty() {
                    String::new()
                } else {
                    format!("\n\nÚltimos erros do yt-dlp:\n{}", err_tail.join("\n"))
                };
                if report.moved == 0 {
                    self.set_done(
                        false,
                        format!("Download falhou.\n{}{}", dl_message, err_text),
                        0,
                        0,
                        0,
                        self.active_tracks.len(),
                        Vec::new(),
                        report.warnings,
                        report.destination_dirs,
                        report.m3u,
                    );
                } else {
                    let success =
                        dl_success && report.missing.is_empty() && report.warnings.is_empty();
                    let mut message = format!(
                        "{} {}, {} {}.",
                        report.moved,
                        if report.moved == 1 {
                            "arquivo organizado"
                        } else {
                            "arquivos organizados"
                        },
                        report.covers,
                        if report.covers == 1 {
                            "capa instalada"
                        } else {
                            "capas instaladas"
                        }
                    );
                    if !report.missing.is_empty() {
                        message.push_str(&format!(
                            "\n{} {} não {}.",
                            report.missing.len(),
                            if report.missing.len() == 1 {
                                "item"
                            } else {
                                "itens"
                            },
                            if report.missing.len() == 1 {
                                "localizado"
                            } else {
                                "localizados"
                            }
                        ));
                    }
                    if !report.warnings.is_empty() {
                        message.push_str(&format!(
                            "\n{} {}.",
                            report.warnings.len(),
                            if report.warnings.len() == 1 {
                                "aviso"
                            } else {
                                "avisos"
                            }
                        ));
                    }
                    if !dl_success {
                        message.push_str(&format!(
                            "\n\nAtenção: o yt-dlp relatou problema.{}",
                            err_text
                        ));
                    }
                    self.set_done(
                        success,
                        message,
                        report.moved,
                        report.covers,
                        report.tagged,
                        self.active_tracks.len(),
                        report.missing,
                        report.warnings,
                        report.destination_dirs,
                        report.m3u,
                    );
                }
            }
            Err(e) => {
                self.set_done(
                    false,
                    format!("Erro ao organizar os arquivos: {e:#}"),
                    0,
                    0,
                    0,
                    self.active_tracks.len(),
                    Vec::new(),
                    Vec::new(),
                    vec![PathBuf::from(&self.config.base_dir)],
                    None,
                );
            }
        }
    }

    fn finish_spotify(&mut self, summary: JobSummary) {
        self.job = None;
        let total = summary.matched + summary.failed;
        let mut warnings = summary.warnings;
        if summary.low_confidence > 0 {
            warnings.push(format!(
                "{} faixa(s) com correspondência de baixa confiança; confira o resultado.",
                summary.low_confidence
            ));
        }
        let mut missing = summary.missing;
        missing.extend(summary.failed_names);
        let success = summary.success && !summary.cancelled;
        self.set_done(
            success,
            summary.message,
            summary.moved,
            summary.covers,
            summary.tagged,
            total,
            missing,
            warnings,
            summary.dirs,
            summary.m3u,
        );
    }

    // -----------------------------------------------------------------------
    // Histórico
    // -----------------------------------------------------------------------

    fn open_history(&mut self) {
        self.history_sel = 0;
        self.pending_clear_history = false;
        self.screen = Screen::History;
    }

    fn key_history(&mut self, key: KeyEvent) {
        let count = self.config.history.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.screen = Screen::Menu,
            KeyCode::Up => self.history_sel = self.history_sel.saturating_sub(1),
            KeyCode::Down => {
                if count > 0 {
                    self.history_sel = (self.history_sel + 1).min(count - 1);
                }
            }
            KeyCode::PageUp => self.history_sel = self.history_sel.saturating_sub(10),
            KeyCode::PageDown => {
                if count > 0 {
                    self.history_sel = (self.history_sel + 10).min(count - 1);
                }
            }
            KeyCode::Home => self.history_sel = 0,
            KeyCode::End => self.history_sel = count.saturating_sub(1),
            KeyCode::Char('c') if count > 0 => {
                if self.pending_clear_history {
                    self.config.clear_history();
                    let _ = self.config.save();
                    self.history_sel = 0;
                    self.pending_clear_history = false;
                    self.toast_success("Histórico limpo.");
                } else {
                    self.pending_clear_history = true;
                    self.toast(
                        "Pressione C novamente para limpar o histórico.",
                        ToastKind::Info,
                    );
                }
            }
            KeyCode::Char('o') if count > 0 => {
                let entry = &self.config.history[count - 1 - self.history_sel];
                let path = PathBuf::from(&entry.dest);
                if path.exists() {
                    let _ = util::open_path(&path);
                } else {
                    self.toast_error("A pasta deste download não existe mais.");
                }
            }
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Enter if count > 0 => {
                let entry = self.config.history[count - 1 - self.history_sel].clone();
                let kind = MediaKind::from_key(&entry.kind);
                self.open_url_screen(kind, entry.url);
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Configurações
    // -----------------------------------------------------------------------

    fn open_settings(&mut self) {
        self.refresh_tools();
        self.settings_sel = 0;
        self.settings_editing = None;
        self.screen = Screen::Settings;
    }

    fn key_settings(&mut self, key: KeyEvent) {
        if let Some(row) = self.settings_editing {
            match key.code {
                KeyCode::Enter | KeyCode::Tab => self.commit_edit(row, false),
                KeyCode::Esc => self.commit_edit(row, true),
                KeyCode::Backspace => {
                    self.edit_buf.pop();
                }
                KeyCode::Char(c) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'v' {
                        self.try_clipboard_paste();
                    } else if !key.modifiers.contains(KeyModifiers::CONTROL) {
                        self.edit_buf.push(c);
                    }
                }
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Esc => self.screen = Screen::Menu,
            KeyCode::Up => self.settings_sel = self.settings_sel.saturating_sub(1),
            KeyCode::Down => {
                self.settings_sel = (self.settings_sel + 1).min(SETTINGS_ROWS - 1);
            }
            KeyCode::Left => self.cycle_setting(-1),
            KeyCode::Right | KeyCode::Char(' ') => self.cycle_setting(1),
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Enter => match self.settings_sel {
                SET_BASE | SET_PROXY | SET_COOKIES | SET_LIMIT_RATE => {
                    self.edit_orig = self.current_setting_text();
                    self.edit_buf = self.edit_orig.clone();
                    self.settings_editing = Some(self.settings_sel);
                }
                SET_UPDATE => self.start_ytdlp_update(),
                SET_ABOUT => self.about_open = true,
                SET_BACK => self.screen = Screen::Menu,
                _ => self.cycle_setting(1),
            },
            _ => {}
        }
    }

    fn current_setting_text(&self) -> String {
        match self.settings_sel {
            SET_BASE => self.config.base_dir.clone(),
            SET_PROXY => self.config.proxy.clone(),
            SET_COOKIES => self.config.cookies_from_browser.clone(),
            SET_LIMIT_RATE => self.config.limit_rate.clone(),
            _ => String::new(),
        }
    }

    fn commit_edit(&mut self, row: usize, cancelled: bool) {
        let value = if cancelled {
            self.edit_orig.clone()
        } else {
            self.edit_buf.trim().to_string()
        };
        match row {
            SET_BASE => {
                if value.is_empty() {
                    self.toast_error("A pasta base não pode ficar vazia.");
                } else {
                    self.config.base_dir = value;
                }
            }
            SET_PROXY => self.config.proxy = value,
            SET_COOKIES => self.config.cookies_from_browser = value,
            SET_LIMIT_RATE => self.config.limit_rate = value,
            _ => {}
        }
        self.settings_editing = None;
        self.edit_buf.clear();
        self.config.normalize();
        let _ = self.config.save();
    }

    fn cycle_setting(&mut self, dir: i32) {
        let step = if dir < 0 { -1 } else { 1 };
        match self.settings_sel {
            1 => {
                let all = AudioFormat::ALL;
                let cur = all
                    .iter()
                    .position(|f| f.key() == self.config.default_audio_format)
                    .unwrap_or(0);
                let next = (cur as isize + step).rem_euclid(all.len() as isize) as usize;
                self.config.default_audio_format = all[next].key().into();
            }
            2 => {
                let all = AudioQuality::ALL;
                let cur = all
                    .iter()
                    .position(|q| q.key() == self.config.default_audio_quality)
                    .unwrap_or(0);
                let next = (cur as isize + step).rem_euclid(all.len() as isize) as usize;
                self.config.default_audio_quality = all[next].key().into();
            }
            3 => {
                let all = VideoContainer::ALL;
                let cur = all
                    .iter()
                    .position(|c| c.key() == self.config.default_video_container)
                    .unwrap_or(0);
                let next = (cur as isize + step).rem_euclid(all.len() as isize) as usize;
                self.config.default_video_container = all[next].key().into();
            }
            4 => {
                let all = VideoQuality::ALL;
                let cur = all
                    .iter()
                    .position(|q| q.key() == self.config.default_video_quality)
                    .unwrap_or(0);
                let next = (cur as isize + step).rem_euclid(all.len() as isize) as usize;
                self.config.default_video_quality = all[next].key().into();
            }
            6 => {
                const STEPS: [u32; 6] = [0, 1, 2, 3, 5, 10];
                let cur = STEPS
                    .iter()
                    .position(|&s| s == self.config.sleep_requests)
                    .unwrap_or(0);
                let next = (cur as isize + step).rem_euclid(STEPS.len() as isize) as usize;
                self.config.sleep_requests = STEPS[next];
            }
            7 => self.config.sponsorblock = !self.config.sponsorblock,
            8 => {
                let all = Theme::ALL;
                let cur = all
                    .iter()
                    .position(|t| *t == self.config.theme())
                    .unwrap_or(0);
                let next = (cur as isize + step).rem_euclid(all.len() as isize) as usize;
                self.config.theme = all[next].key().into();
            }
            9 => self.config.notifications = !self.config.notifications,
            10 => self.config.write_m3u = !self.config.write_m3u,
            13 => {
                const STEPS: [usize; 6] = [1, 2, 3, 4, 6, 8];
                let cur = STEPS
                    .iter()
                    .position(|&s| s == self.config.spotify_workers)
                    .unwrap_or(2);
                let next = (cur as isize + step).rem_euclid(STEPS.len() as isize) as usize;
                self.config.spotify_workers = STEPS[next];
            }
            14 => self.config.download_archive = !self.config.download_archive,
            15 => self.config.open_folder_after = !self.config.open_folder_after,
            _ => {}
        }
        self.config.normalize();
        let _ = self.config.save();
        if self.settings_sel == 8 {
            self.toast(
                format!("Tema: {}", self.config.theme().label()),
                ToastKind::Info,
            );
        }
    }

    fn start_ytdlp_update(&mut self) {
        if self.updating {
            return;
        }
        self.updating = true;
        self.update_msg = "Atualizando yt-dlp…".into();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let out = std::process::Command::new("yt-dlp").arg("-U").output();
            let mut msg = match out {
                Ok(o) => {
                    let mut s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                    if !err.is_empty() {
                        if !s.is_empty() {
                            s.push('\n');
                        }
                        s.push_str(&err);
                    }
                    if s.is_empty() {
                        s = format!("yt-dlp {}", check_tool("yt-dlp", &["--version"]));
                    }
                    s
                }
                Err(e) => format!("Falha ao executar yt-dlp: {e}"),
            };
            if msg.contains("package manager") || msg.contains("manual build") {
                let pip = std::process::Command::new("python3")
                    .args(["-m", "pip", "install", "--user", "-U", "yt-dlp"])
                    .output();
                msg = match pip {
                    Ok(o) => {
                        let mut s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                        let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                        if !err.is_empty() {
                            if !s.is_empty() {
                                s.push('\n');
                            }
                            s.push_str(&err);
                        }
                        if s.is_empty() {
                            s = "pip: sem saída".into();
                        }
                        s
                    }
                    Err(e) => format!("Falha ao executar pip: {e}"),
                };
            }
            let _ = tx.send(msg);
        });
        self.update_rx = Some(rx);
    }

    // -----------------------------------------------------------------------
    // Resultado
    // -----------------------------------------------------------------------

    fn key_done(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('o') => {
                if let Some(dir) = self.done_dirs.first() {
                    let _ = util::open_path(dir);
                }
            }
            KeyCode::Char('n') => {
                let kind = if self.done_kind == MediaKind::Spotify {
                    MediaKind::Spotify
                } else {
                    self.done_kind
                };
                self.enter_url_screen(kind);
            }
            KeyCode::Char('?') => self.open_help(),
            _ => self.screen = Screen::Menu,
        }
    }

    // -----------------------------------------------------------------------
    // Poll: metadados, atualizações, download, organização e Spotify
    // -----------------------------------------------------------------------

    pub fn poll(&mut self) -> Result<()> {
        self.poll_meta();
        self.poll_tools();
        self.poll_update();
        self.poll_download();
        self.poll_organize();
        self.poll_job();
        Ok(())
    }

    fn poll_meta(&mut self) {
        if !self.loading_meta {
            return;
        }
        let Some(rx) = &self.meta_rx else {
            self.loading_meta = false;
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.loading_meta = false;
                self.meta_rx = None;
                self.url_error = Some("A análise do link foi interrompida.".into());
                return;
            }
        };
        self.loading_meta = false;
        self.meta_rx = None;
        match result {
            Ok(fetched) => {
                self.opts.kind = fetched.kind;
                if fetched.kind == MediaKind::Spotify {
                    self.opts.kind = MediaKind::Spotify;
                }
                self.opts.playlist_items = None;
                self.tracks = fetched.tracks.clone();
                self.track_selected = vec![true; self.tracks.len()];
                self.track_sel = 0;
                self.track_filter.clear();
                self.filter_buf.clear();
                self.filtering = false;
                self.url_error = None;
                let count = self.tracks.len();
                let is_playlist = fetched.is_playlist && count > 1;
                let title = fetched.title.clone();
                self.fetched = Some(fetched);
                if is_playlist {
                    self.screen = Screen::Tracks;
                    self.toast(
                        format!("{} — {}", title, util::plural(count, "faixa", "faixas")),
                        ToastKind::Info,
                    );
                } else {
                    self.screen = Screen::Options;
                    self.toast(
                        format!(
                            "{} {}.",
                            util::plural(count, "faixa", "faixas"),
                            if count == 1 {
                                "encontrada"
                            } else {
                                "encontradas"
                            }
                        ),
                        ToastKind::Success,
                    );
                }
            }
            Err(e) => {
                let message = format!("{e:#}");
                self.url_error = Some(message.clone());
                self.toast_error(util::truncate(&message, 200));
            }
        }
    }

    fn poll_tools(&mut self) {
        let Some(rx) = &self.tool_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(status) => {
                let missing: Vec<String> = status
                    .iter()
                    .filter(|(_, ok, _)| !ok)
                    .map(|(name, _, _)| name.clone())
                    .collect();
                self.tool_status = status;
                self.tool_rx = None;
                if !missing.is_empty() {
                    self.toast_error(format!(
                        "Ferramenta(s) ausente(s): {}. Abra Configurações para o diagnóstico.",
                        missing.join(", ")
                    ));
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.tool_rx = None;
            }
        }
    }

    fn poll_update(&mut self) {
        if !self.updating {
            return;
        }
        let Some(rx) = &self.update_rx else {
            self.updating = false;
            return;
        };
        match rx.try_recv() {
            Ok(msg) => {
                self.updating = false;
                self.update_rx = None;
                self.update_msg = msg;
                self.refresh_tools();
                self.toast("Ferramentas atualizadas.", ToastKind::Success);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.updating = false;
                self.update_rx = None;
                self.update_msg = "Atualização interrompida.".into();
            }
        }
    }

    fn poll_download(&mut self) {
        let mut events = Vec::new();
        let mut disconnected = false;
        if let Some(handle) = &self.download {
            loop {
                match handle.rx.try_recv() {
                    Ok(ev) => {
                        events.push(ev);
                        if events.len() >= 800 {
                            break;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        let saw_finish = events
            .iter()
            .any(|e| matches!(e, Progress::Finished { .. }));
        for event in events {
            self.handle_download_event(event);
        }
        if disconnected && !saw_finish && self.download.is_some() {
            self.pending_dl = Some((false, "O yt-dlp encerrou sem relatar conclusão.".into()));
            self.start_organize();
        }
    }

    fn handle_download_event(&mut self, event: Progress) {
        match event {
            Progress::Line(line) => self.push_log(line),
            Progress::File { id } => {
                self.seen_files = self.seen_files.saturating_add(1);
                self.progress_name = self
                    .active_labels
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| id.clone());
                self.progress_current = 0.0;
            }
            Progress::PlaylistItem { index, total } => {
                // Em playlists com seleção parcial o yt-dlp pode numerar os
                // itens pela posição original. Limitamos ao total ativo para
                // não estourar a barra de progresso.
                let active = self.active_tracks.len().max(1);
                self.item_total = total.clamp(1, active);
                self.item_index = index.clamp(1, self.item_total);
            }
            Progress::Archived { id } => {
                self.archived_ids.insert(id.clone());
                self.push_log(format!("↷ {id} já estava no arquivo do yt-dlp."));
            }
            Progress::Percent {
                pct,
                file,
                speed,
                eta,
            } => {
                self.progress_current = pct as f32;
                self.speed = speed;
                self.eta = eta;
                if !file.is_empty() {
                    if let Some(label) = self.active_labels.get(&file) {
                        self.progress_name = label.clone();
                    }
                }
                let total = self.active_tracks.len().max(1) as f32;
                let base = if self.item_index > 0 && self.item_index <= self.active_tracks.len() {
                    (self.item_index - 1) as f32 + pct as f32 / 100.0
                } else {
                    self.seen_files.saturating_sub(1) as f32 + pct as f32 / 100.0
                };
                self.progress_pct = (base / total * 90.0).clamp(0.0, 90.0);
            }
            Progress::Finished { success, message } => {
                self.pending_dl = Some((success, message));
                self.start_organize();
            }
        }
    }

    fn start_organize(&mut self) {
        let Some(handle) = self.download.take() else {
            self.finish_youtube_organize(Err(anyhow::anyhow!(
                "diretório temporário não encontrado"
            )));
            return;
        };
        let tmpdir = handle.tmpdir.clone();
        let tracks: Vec<TrackMeta> = self
            .active_tracks
            .iter()
            .filter(|t| !self.archived_ids.contains(&t.id))
            .cloned()
            .collect();
        if tracks.is_empty() {
            backend::cleanup_tmpdir(&tmpdir);
            self.pending_dl = None;
            self.set_done(
                true,
                format!(
                    "Nada a baixar: {} já constava(m) no arquivo do yt-dlp.",
                    util::plural(self.archived_ids.len(), "item", "itens")
                ),
                0,
                0,
                0,
                self.active_tracks.len(),
                Vec::new(),
                Vec::new(),
                vec![PathBuf::from(&self.config.base_dir)],
                None,
            );
            return;
        }
        let base = PathBuf::from(&self.config.base_dir);
        let collection = if self
            .fetched
            .as_ref()
            .map(|f| f.is_playlist)
            .unwrap_or(false)
        {
            Some(self.active_title.clone())
        } else {
            None
        };
        let org_opts = OrganizeOptions {
            kind: self.active_kind,
            organize: self.opts.organize,
            number: self.opts.number_tracks,
            tag_externally: false,
            embed_cover: false,
            install_cover: self.opts.embed_thumbnail,
            write_m3u: self.opts.write_m3u,
            collection,
        };
        self.organizing = true;
        self.stage = "Organização".into();
        self.detail = "Movendo arquivos, capas e aplicando metadados…".into();
        self.progress_pct = 95.0;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let result = organize::organize(&tracks, &tmpdir, &base, &org_opts);
            let _ = tx.send(result);
        });
        self.organize_rx = Some(rx);
    }

    fn poll_organize(&mut self) {
        if !self.organizing {
            return;
        }
        let Some(rx) = &self.organize_rx else {
            self.organizing = false;
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.organize_rx = None;
                self.finish_youtube_organize(result);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.organize_rx = None;
                self.finish_youtube_organize(Err(anyhow::anyhow!("organização interrompida")));
            }
        }
    }

    fn poll_job(&mut self) {
        let mut events = Vec::new();
        let mut disconnected = false;
        if let Some(job) = &self.job {
            loop {
                match job.rx.try_recv() {
                    Ok(ev) => {
                        events.push(ev);
                        if events.len() >= 800 {
                            break;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        let saw_finish = events.iter().any(|e| matches!(e, JobEvent::Finished(_)));
        for event in events {
            self.handle_job_event(event);
        }
        if disconnected && !saw_finish && self.job.is_some() {
            self.job = None;
            self.set_done(
                false,
                "A tarefa do Spotify terminou inesperadamente. Tente novamente.".into(),
                0,
                0,
                0,
                self.active_tracks.len(),
                Vec::new(),
                Vec::new(),
                vec![PathBuf::from(&self.config.base_dir)],
                None,
            );
        }
    }

    fn handle_job_event(&mut self, event: JobEvent) {
        match event {
            JobEvent::Stage { title, detail } => {
                self.stage = title;
                self.detail = detail;
            }
            JobEvent::Match {
                done,
                total,
                track,
                matched,
                confidence,
            } => {
                self.progress_pct = if total > 0 {
                    (done as f32 / total as f32) * 15.0
                } else {
                    0.0
                };
                let marker = if confidence < 0.45 { "⚠" } else { "✓" };
                self.push_log(format!(
                    "{marker} {track} → {matched} ({:.0}%)",
                    confidence * 100.0
                ));
                self.detail = format!("{done}/{total} faixas correspondidas");
            }
            JobEvent::MatchError {
                done,
                total,
                track,
                error,
            } => {
                self.progress_pct = if total > 0 {
                    (done as f32 / total as f32) * 15.0
                } else {
                    0.0
                };
                self.push_log(format!("✗ {track}: {error}"));
                self.detail = format!("{done}/{total} faixas correspondidas");
            }
            JobEvent::DownloadStarted { total, urls } => {
                self.item_total = total;
                self.stage = "Download".into();
                self.progress_pct = 15.0;
                self.detail = format!(
                    "Baixando {urls} do YouTube. {} correspondidas.",
                    util::plural(total, "faixa", "faixa")
                );
            }
            JobEvent::Line(line) => self.push_log(line),
            JobEvent::Progress {
                overall,
                current,
                file,
                speed,
                eta,
            } => {
                self.progress_pct = overall.clamp(0.0, 100.0);
                self.progress_current = current;
                if let Some(file) = file {
                    self.progress_name = file;
                }
                self.speed = speed;
                self.eta = eta;
            }
            JobEvent::Organizing => {
                self.stage = "Organização".into();
                self.detail = "Aplicando metadados e capas do Spotify…".into();
                self.progress_pct = 95.0;
                self.organizing = true;
            }
            JobEvent::Finished(summary) => {
                if summary.cancelled && self.cancelled {
                    self.finish_cancelled();
                } else {
                    self.finish_spotify(summary);
                }
            }
        }
    }
}

pub fn run(
    term: &mut Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    config: Config,
) -> Result<()> {
    let mut app = App::new(config);
    app.refresh_tools();
    loop {
        app.on_tick();
        term.draw(|f| app.draw(f))?;
        if crossterm::event::poll(Duration::from_millis(80))? {
            match crossterm::event::read()? {
                crossterm::event::Event::Key(key) => app.handle_key(key),
                crossterm::event::Event::Paste(text) => app.handle_paste(&text),
                _ => {}
            }
        }
        app.poll()?;
        if app.quit {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use ytui::backend::{CollectionKind, TrackMeta};

    fn track(id: &str, title: &str, artist: &str) -> TrackMeta {
        let mut t = TrackMeta::default();
        t.id = id.into();
        t.title = title.into();
        t.artist = Some(artist.into());
        t.collection = Some("Coleção".into());
        t.collection_kind = Some(CollectionKind::Playlist);
        t
    }

    fn app_with_tracks() -> App {
        let mut app = App::new(Config::default());
        app.screen = Screen::Tracks;
        app.tracks = vec![
            track("a", "Coração de Aço", "Artista Um"),
            track("b", "Noite Estrelada", "Artista Dois"),
            track("c", "Ação e Reação", "Artista Três"),
        ];
        app.track_selected = vec![true, false, true];
        app.fetched = Some(FetchedMedia {
            kind: MediaKind::Spotify,
            title: "Coleção".into(),
            tracks: app.tracks.clone(),
            is_playlist: true,
            spotify: None,
        });
        app
    }

    #[test]
    fn filter_ignores_accents_and_matches_artist() {
        let mut app = app_with_tracks();
        app.track_filter = "coracao".into();
        assert_eq!(app.filtered_track_indices(), vec![0]);
        app.track_filter = "artista dois".into();
        assert_eq!(app.filtered_track_indices(), vec![1]);
        app.track_filter = "inexistente".into();
        assert!(app.filtered_track_indices().is_empty());
    }

    #[test]
    fn space_toggles_current_track() {
        let mut app = app_with_tracks();
        app.track_sel = 1;
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(app.track_selected[1]);
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(!app.track_selected[1]);
    }

    #[test]
    fn select_all_and_none_apply_to_filtered_rows() {
        let mut app = app_with_tracks();
        app.track_filter = "reacao".into(); // apenas "Ação e Reação" (índice 2)
        app.track_sel = 0;
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(app.track_selected, vec![true, false, false]);
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(app.track_selected, vec![true, false, true]);
    }

    #[test]
    fn enters_options_requires_selection() {
        let mut app = app_with_tracks();
        app.track_selected = vec![false, false, false];
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.screen, Screen::Tracks);
        assert!(app.visible_toast().is_some());
    }

    #[test]
    fn media_kind_roundtrip() {
        for kind in [
            MediaKind::Musica,
            MediaKind::Video,
            MediaKind::Spotify,
            MediaKind::Outro,
        ] {
            assert_eq!(MediaKind::from_key(kind.key()), kind);
        }
    }
}
