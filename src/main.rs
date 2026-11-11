mod app;
mod ui;

use anyhow::{bail, Result};
use crossterm::{
    cursor::{Hide, Show},
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, IsTerminal, Stdout};
use ytui::config::Config;

const HELP: &str = "ytui — interface de terminal para YouTube, YouTube Music e Spotify

Uso:
  ytui                 abre a interface
  ytui --version       mostra a versão
  ytui --help          mostra esta ajuda

Na interface:
  ? / F1              ajuda e atalhos
  T                   alterna o tema
  Q / Ctrl+C          sai (Q no menu; Ctrl+C em qualquer tela)
  Esc                 volta ou cancela

A versão em linha de comando é o binário ytui-dl.
Configuração: ~/.config/ytui/config.json";

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(e) = execute!(stdout, EnterAlternateScreen, EnableBracketedPaste, Hide) {
            let _ = disable_raw_mode();
            return Err(e.into());
        }
        let backend = CrosstermBackend::new(stdout);
        let terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(e) => {
                let _ = disable_raw_mode();
                let _ = execute!(
                    io::stdout(),
                    LeaveAlternateScreen,
                    DisableBracketedPaste,
                    Show
                );
                return Err(e.into());
            }
        };
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableBracketedPaste,
            Show
        );
        let _ = self.terminal.show_cursor();
    }
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableBracketedPaste,
        Show
    );
}

fn main() -> Result<()> {
    let mut cli = std::env::args().skip(1);
    if let Some(arg) = cli.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("ytui {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            other => bail!("argumento desconhecido: {other}\n\n{HELP}"),
        }
    }
    if let Some(extra) = cli.next() {
        bail!("argumento desconhecido: {extra}\n\n{HELP}");
    }
    if !io::stdout().is_terminal() {
        bail!("o ytui precisa de um terminal interativo. Use ytui-dl para automação.");
    }

    let config = Config::load()?;
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Restaura o terminal antes de mostrar o pânico; sem isso o terminal
        // do usuário fica preso em modo raw/alternate screen.
        restore_terminal();
        default_hook(info);
    }));

    let mut guard = TerminalGuard::new()?;
    let result = app::run(&mut guard.terminal, config);
    drop(guard);
    result
}
