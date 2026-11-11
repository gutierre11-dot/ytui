//! Configuração persistente do ytui.
//!
//! O arquivo fica em `~/.config/ytui/config.json` (ou `$XDG_CONFIG_HOME`).
//! Campos novos sempre têm valor padrão, então configurações antigas continuam
//! funcionando depois de uma atualização.

use crate::theme::Theme;
use crate::util;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistoryEntry {
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub items: usize,
    #[serde(default)]
    pub dest: String,
    /// Nome da playlist/álbum, quando conhecido.
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Reservado para migrações futuras. Configurações antigas valem 0.
    #[serde(default)]
    pub version: u32,
    #[serde(default = "default_base_dir")]
    pub base_dir: String,
    #[serde(default = "default_audio_format")]
    pub default_audio_format: String,
    #[serde(default = "default_audio_quality")]
    pub default_audio_quality: String,
    #[serde(default = "default_video_container")]
    pub default_video_container: String,
    #[serde(default = "default_video_quality")]
    pub default_video_quality: String,
    #[serde(default)]
    pub proxy: String,
    #[serde(default)]
    pub sleep_requests: u32,
    #[serde(default)]
    pub sponsorblock: bool,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_true")]
    pub notifications: bool,
    #[serde(default)]
    pub write_m3u: bool,
    #[serde(default)]
    pub cookies_from_browser: String,
    #[serde(default)]
    pub limit_rate: String,
    #[serde(default = "default_spotify_workers")]
    pub spotify_workers: usize,
    #[serde(default)]
    pub download_archive: bool,
    #[serde(default)]
    pub open_folder_after: bool,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
    /// Aviso de inicialização (não persistido).
    #[serde(skip)]
    pub startup_warning: Option<String>,
}

fn default_base_dir() -> String {
    format!("{}/Mídia", util::home_dir().display())
}
fn default_audio_format() -> String {
    "mp3".into()
}
fn default_audio_quality() -> String {
    "maxima".into()
}
fn default_video_container() -> String {
    "mp4".into()
}
fn default_video_quality() -> String {
    "melhor".into()
}
fn default_theme() -> String {
    Theme::default().key().into()
}
fn default_true() -> bool {
    true
}
fn default_spotify_workers() -> usize {
    3
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 2,
            base_dir: default_base_dir(),
            default_audio_format: default_audio_format(),
            default_audio_quality: default_audio_quality(),
            default_video_container: default_video_container(),
            default_video_quality: default_video_quality(),
            proxy: String::new(),
            sleep_requests: 0,
            sponsorblock: false,
            theme: default_theme(),
            notifications: true,
            write_m3u: false,
            cookies_from_browser: String::new(),
            limit_rate: String::new(),
            spotify_workers: default_spotify_workers(),
            download_archive: false,
            open_folder_after: false,
            history: Vec::new(),
            startup_warning: None,
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        util::config_dir().join("config.json")
    }

    pub fn archive_path() -> PathBuf {
        util::data_dir().join("archive.txt")
    }

    pub fn load() -> Result<Self> {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read_to_string(path)
            .with_context(|| format!("não foi possível ler {}", path.display()))?;
        match serde_json::from_str::<Config>(&data) {
            Ok(mut cfg) => {
                cfg.normalize();
                Ok(cfg)
            }
            Err(err) => {
                // Uma configuração corrompida nunca deve impedir o uso do app.
                // Preservamos o arquivo original e voltamos aos padrões.
                let backup = path.with_extension("json.invalid");
                let _ = fs::rename(path, &backup);
                Ok(Self {
                    startup_warning: Some(format!(
                        "config.json inválido ({err}). Backup em {}.",
                        backup.display()
                    )),
                    ..Self::default()
                })
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::path())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, data).context("não foi possível gravar a configuração")?;
        // Rename atômico evita config corrompida se o app for interrompido.
        #[cfg(windows)]
        {
            if path.exists() {
                let _ = fs::remove_file(path);
            }
        }
        fs::rename(&tmp, path).context("não foi possível concluir a gravação da configuração")?;
        Ok(())
    }

    pub fn theme(&self) -> Theme {
        Theme::from_key(&self.theme)
    }

    pub fn normalize(&mut self) {
        self.version = 2;
        if self.base_dir.trim().is_empty() {
            self.base_dir = default_base_dir();
        }
        if self.default_audio_format.trim().is_empty() {
            self.default_audio_format = default_audio_format();
        }
        if self.default_audio_quality.trim().is_empty() {
            self.default_audio_quality = default_audio_quality();
        }
        if self.default_video_container.trim().is_empty() {
            self.default_video_container = default_video_container();
        }
        if self.default_video_quality.trim().is_empty() {
            self.default_video_quality = default_video_quality();
        }
        if self.spotify_workers == 0 {
            self.spotify_workers = 1;
        }
        self.spotify_workers = self.spotify_workers.clamp(1, 8);
        self.sleep_requests = self.sleep_requests.min(60);
        if self.history.len() > 500 {
            let excess = self.history.len() - 500;
            self.history.drain(..excess);
        }
    }

    pub fn record_download(
        &mut self,
        kind: &str,
        url: &str,
        items: usize,
        dest: &str,
        title: &str,
    ) {
        self.history.push(HistoryEntry {
            date: current_date(),
            kind: kind.to_string(),
            url: url.to_string(),
            items,
            dest: dest.to_string(),
            title: title.to_string(),
        });
        if self.history.len() > 500 {
            let excess = self.history.len() - 500;
            self.history.drain(..excess);
        }
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}

fn current_date() -> String {
    // Sem chrono: usa `date` do sistema, com fallback para o horário Unix.
    std::process::Command::new("date")
        .arg("+%d/%m/%Y %H:%M")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            format!("{secs}s desde 1970")
        })
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ytui-test-{nanos}-{name}"))
    }

    #[test]
    fn roundtrip() {
        let path = temp_file("config.json");
        let mut cfg = Config::default();
        cfg.base_dir = "/tmp/musicas".into();
        cfg.theme = "nord".into();
        cfg.notifications = false;
        cfg.record_download(
            "spotify",
            "https://open.spotify.com/track/x",
            1,
            "/tmp",
            "Álbum",
        );
        cfg.save_to(&path).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.base_dir, "/tmp/musicas");
        assert_eq!(loaded.theme(), Theme::Nord);
        assert!(!loaded.notifications);
        assert_eq!(loaded.history.len(), 1);
        assert_eq!(loaded.history[0].title, "Álbum");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn legacy_config_gets_defaults() {
        let path = temp_file("legacy.json");
        fs::write(
            &path,
            r#"{"base_dir":"/x","default_audio_format":"flac","history":[]}"#,
        )
        .unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.base_dir, "/x");
        assert_eq!(cfg.default_audio_format, "flac");
        assert_eq!(cfg.spotify_workers, 3);
        assert!(cfg.notifications);
        assert_eq!(cfg.theme(), Theme::Neon);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn corrupt_config_is_backed_up() {
        let path = temp_file("corrupt.json");
        fs::write(&path, "{ isso não é json }").unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert!(cfg.startup_warning.is_some());
        let backup = path.with_extension("json.invalid");
        assert!(backup.exists());
        let _ = fs::remove_file(backup);
    }

    #[test]
    fn normalize_clamps_values() {
        let mut cfg = Config::default();
        cfg.spotify_workers = 0;
        cfg.normalize();
        assert_eq!(cfg.spotify_workers, 1);
        cfg.spotify_workers = 99;
        cfg.normalize();
        assert_eq!(cfg.spotify_workers, 8);
    }
}
