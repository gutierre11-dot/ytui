use crate::app::{App, Screen, ToastKind, MENU_DESCRIPTIONS, MENU_ICONS, MENU_ITEMS, OPTION_ROWS};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap,
};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;
use ytui::backend::{AudioFormat, AudioQuality, MediaKind, VideoContainer, VideoQuality};
use ytui::spotify;
use ytui::theme::Palette;
use ytui::util;

const VERSION: &str = "0.2.0";

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let p = app.palette();
    f.render_widget(Block::default().style(Style::default().bg(p.bg)), area);
    if area.width < 60 || area.height < 16 {
        f.render_widget(
            Paragraph::new(vec![
                Line::styled(
                    "Terminal muito pequeno",
                    Style::default().fg(p.warning).add_modifier(Modifier::BOLD),
                ),
                Line::from(""),
                Line::styled(
                    "Redimensione a janela para pelo menos 60x16.",
                    Style::default().fg(p.dim),
                ),
            ])
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(p.border))
                    .style(Style::default().bg(p.panel)),
            ),
            centered_rect(46, 7, area),
        );
        return;
    }
    let content = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height.saturating_sub(1),
    };
    match app.screen {
        Screen::Menu => draw_menu(f, content, app, &p),
        Screen::Url => draw_url(f, content, app, &p),
        Screen::Tracks => draw_tracks(f, content, app, &p),
        Screen::Options => draw_options(f, content, app, &p),
        Screen::Progress => draw_progress(f, content, app, &p),
        Screen::History => draw_history(f, content, app, &p),
        Screen::Settings => draw_settings(f, content, app, &p),
        Screen::Done => draw_done(f, content, app, &p),
    }
    draw_status_bar(f, area, app, &p);
    if app.help_open || app.about_open {
        // Modal opaco: evita que o conteúdo de trás brigue visualmente com a
        // ajuda. A barra de status continua visível.
        let backdrop = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: area.height.saturating_sub(1),
        };
        f.render_widget(Clear, backdrop);
        f.render_widget(Block::default().style(Style::default().bg(p.bg)), backdrop);
    }
    if app.help_open {
        draw_help(f, area, app, &p);
    }
    if app.about_open {
        draw_about(f, area, &p);
    }
    if let Some(toast) = app.visible_toast() {
        draw_toast(f, area, toast, &p);
    }
}

// ---------------------------------------------------------------------------
// Componentes básicos
// ---------------------------------------------------------------------------

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

fn panel(f: &mut Frame, area: Rect, title: &str, p: &Palette) -> Rect {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(p.border))
        .style(Style::default().bg(p.panel))
        .title(Line::from(vec![
            Span::styled(
                " ◆ ",
                Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                title,
                Style::default().fg(p.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ◆ ", Style::default().fg(p.accent)),
        ]));
    let inner = block.inner(area);
    f.render_widget(block, area);
    inner
}

fn overlay_panel(f: &mut Frame, area: Rect, title: &str, p: &Palette) -> Rect {
    f.render_widget(Clear, area);
    panel(f, area, title, p)
}

fn chip(text: &str, selected: bool, p: &Palette, color: Color) -> Span<'static> {
    if selected {
        Span::styled(
            format!(" {text} "),
            Style::default()
                .fg(p.selected_fg)
                .bg(color)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(format!(" {text} "), Style::default().fg(p.dim))
    }
}

fn kind_color(kind: MediaKind, p: &Palette) -> Color {
    match kind {
        MediaKind::Spotify => p.spotify,
        MediaKind::Video => p.accent2,
        MediaKind::Outro => p.warning,
        MediaKind::Musica => p.accent,
    }
}

fn source_label(app: &App) -> &'static str {
    if spotify::is_spotify_url(&app.url) {
        return "Spotify";
    }
    let url = app.url.to_ascii_lowercase();
    if url.contains("music.youtube.com") {
        "YouTube Music"
    } else if url.contains("youtube.com") || url.contains("youtu.be") {
        "YouTube"
    } else if url.trim().is_empty() {
        match app.hint_kind {
            MediaKind::Musica => "YouTube Music",
            MediaKind::Video => "YouTube",
            MediaKind::Spotify => "Spotify",
            MediaKind::Outro => "Outro site",
        }
    } else {
        "Outro site"
    }
}

fn elapsed_text(app: &App) -> String {
    app.started_at
        .map(|t| {
            let secs = t.elapsed().as_secs();
            format!("{:02}:{:02}", secs / 60, secs % 60)
        })
        .unwrap_or_else(|| "—".into())
}

fn separator(p: &Palette) -> Span<'static> {
    Span::styled("  ·  ", Style::default().fg(p.border))
}

// ---------------------------------------------------------------------------
// Menu
// ---------------------------------------------------------------------------

fn draw_menu(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let outer = Rect {
        x: area.x + 2,
        y: area.y + 1,
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };
    if outer.width >= 96 {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
            .split(outer);
        draw_brand(f, chunks[0], app, p);
        draw_menu_list(f, chunks[1], app, p);
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(10), Constraint::Min(10)])
            .split(outer);
        draw_brand(f, chunks[0], app, p);
        draw_menu_list(f, chunks[1], app, p);
    }
}

fn draw_brand(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let inner = panel(f, area, "ytui", p);
    let logo = [
        " ██╗   ██╗████████╗██╗   ██╗██╗",
        " ╚██╗ ██╔╝╚══██╔══╝██║   ██║██║",
        "  ╚████╔╝    ██║   ██║   ██║██║",
        "   ╚██╔╝     ██║   ██║   ██║██║",
        "    ██║      ██║   ╚██████╔╝██║",
        "    ╚═╝      ╚═╝    ╚═════╝ ╚═╝",
    ];
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));
    for (i, line) in logo.iter().enumerate() {
        let color = if i % 2 == 0 { p.accent } else { p.accent2 };
        lines.push(Line::styled(
            *line,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
    }
    lines.push(Line::from(""));
    lines.push(Line::styled(
        "sua biblioteca, sem complicação",
        Style::default().fg(p.text),
    ));
    lines.push(Line::styled(
        "YouTube  •  YouTube Music  •  Spotify",
        Style::default().fg(p.dim),
    ));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  ♫ ", Style::default().fg(p.spotify)),
        Span::styled("metadados e capas oficiais", Style::default().fg(p.dim)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  ⚡ ", Style::default().fg(p.warning)),
        Span::styled("yt-dlp + ffmpeg", Style::default().fg(p.dim)),
    ]));
    lines.push(Line::from(""));
    lines.push(tool_line(app, p));
    lines.push(Line::styled(
        format!(
            "  v{VERSION}  •  {} downloads no histórico",
            app.config.history.len()
        ),
        Style::default().fg(p.dim),
    ));
    f.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn tool_line(app: &App, p: &Palette) -> Line<'static> {
    if app.tool_status.is_empty() {
        return Line::styled(
            "  diagnóstico disponível em Configurações",
            Style::default().fg(p.dim),
        );
    }
    let mut spans = vec![Span::styled("  ", Style::default())];
    for (i, (name, ok, _)) in app.tool_status.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("   ", Style::default()));
        }
        spans.push(Span::styled(format!("{name} "), Style::default().fg(p.dim)));
        spans.push(Span::styled(
            if *ok { "●" } else { "○" },
            Style::default().fg(if *ok { p.success } else { p.error }),
        ));
    }
    Line::from(spans)
}

fn draw_menu_list(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let inner = panel(f, area, "Menu principal", p);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9),
            Constraint::Length(4),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(inner);

    let items: Vec<ListItem> = MENU_ITEMS
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let selected = i == app.menu_sel;
            let fg = if selected { p.selected_fg } else { p.text };
            let icon = MENU_ICONS[i];
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {icon}  "),
                    Style::default()
                        .fg(if selected {
                            p.selected_fg
                        } else {
                            kind_icon_color(i, p)
                        })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{label:<24}"),
                    Style::default().fg(fg).add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
                ),
            ]))
        })
        .collect();
    let list = List::new(items)
        .highlight_style(Style::default().bg(p.selected_bg))
        .highlight_symbol("▸");
    let mut state = ListState::default().with_selected(Some(app.menu_sel));
    f.render_stateful_widget(list, chunks[0], &mut state);

    let desc = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  ◆ ", Style::default().fg(p.accent)),
            Span::styled(MENU_DESCRIPTIONS[app.menu_sel], Style::default().fg(p.text)),
        ]),
        Line::from(vec![
            Span::styled("    ", Style::default()),
            Span::styled(
                match app.menu_sel {
                    0 => "Cole o link de uma música, álbum ou playlist.",
                    1 => "Escolha resolução, contêiner e patrocínios.",
                    2 => "Links públicos; o áudio vem da melhor correspondência.",
                    3 => "Aceita Vimeo, SoundCloud, Twitter e centenas de sites.",
                    4 => "Repita um download ou abra a pasta de destino.",
                    5 => "Tema, formatos, rede, cookies e diagnóstico.",
                    _ => "Pressione Enter ou Q para encerrar.",
                },
                Style::default().fg(p.dim),
            ),
        ]),
    ])
    .wrap(Wrap { trim: false });
    f.render_widget(desc, chunks[1]);

    let mut recents: Vec<Line> = vec![Line::styled(
        "  Recentes",
        Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
    )];
    if app.config.history.is_empty() {
        recents.push(Line::styled(
            "    seus últimos downloads aparecem aqui",
            Style::default().fg(p.dim),
        ));
    } else {
        for entry in app.config.history.iter().rev().take(3) {
            let kind = MediaKind::from_key(&entry.kind);
            let label = if entry.title.is_empty() {
                util::truncate(&entry.url, 30)
            } else {
                util::truncate(&entry.title, 30)
            };
            recents.push(Line::from(vec![
                Span::styled("    ● ", Style::default().fg(kind_color(kind, p))),
                Span::styled(format!("{label:<32}"), Style::default().fg(p.text)),
                Span::styled(entry.date.clone(), Style::default().fg(p.dim)),
            ]));
        }
    }
    f.render_widget(Paragraph::new(recents), chunks[2]);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  ↑↓ ", Style::default().fg(p.accent)),
            Span::styled("navegar", Style::default().fg(p.dim)),
            separator(p),
            Span::styled("Enter ", Style::default().fg(p.accent)),
            Span::styled("abrir", Style::default().fg(p.dim)),
            separator(p),
            Span::styled("t ", Style::default().fg(p.accent)),
            Span::styled("tema", Style::default().fg(p.dim)),
            separator(p),
            Span::styled("? ", Style::default().fg(p.accent)),
            Span::styled("ajuda", Style::default().fg(p.dim)),
        ])),
        chunks[3],
    );
}

fn kind_icon_color(index: usize, p: &Palette) -> Color {
    match index {
        0 | 2 => p.spotify,
        1 => p.accent2,
        3 => p.warning,
        5 => p.accent,
        6 => p.error,
        _ => p.text,
    }
}

// ---------------------------------------------------------------------------
// URL
// ---------------------------------------------------------------------------

fn draw_url(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let popup = centered_rect(area.width.saturating_sub(4).min(86), 18, area);
    let inner = overlay_panel(f, popup, "Nova busca", p);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(inner);

    let detected = source_label(app);
    let mut chips = vec![Span::styled("  Fonte: ", Style::default().fg(p.dim))];
    for (label, kind) in [
        ("YouTube Music", MediaKind::Musica),
        ("YouTube", MediaKind::Video),
        ("Spotify", MediaKind::Spotify),
        ("Outro site", MediaKind::Outro),
    ] {
        chips.push(chip(
            label,
            detected == label || (detected == "YouTube Music" && label == "YouTube Music"),
            p,
            kind_color(kind, p),
        ));
        chips.push(Span::raw(" "));
    }
    f.render_widget(Paragraph::new(Line::from(chips)), chunks[0]);
    f.render_widget(
        Paragraph::new(Line::styled(
            format!("  {detected} — cole o link abaixo e pressione Enter"),
            Style::default().fg(p.dim),
        )),
        chunks[1],
    );

    let input_area = chunks[3];
    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(p.accent))
        .style(Style::default().bg(p.bg))
        .title(Span::styled(" Link ", Style::default().fg(p.accent)));
    let input_inner = input_block.inner(input_area);
    f.render_widget(input_block, input_area);
    let width = input_inner.width.max(1) as usize;
    let prefix_width = UnicodeWidthStr::width(&app.url[..app.url_cursor.min(app.url.len())]);
    let mut start = 0usize;
    if prefix_width >= width {
        for (i, _) in app.url.char_indices() {
            if UnicodeWidthStr::width(&app.url[i..app.url_cursor.min(app.url.len())]) < width {
                start = i;
                break;
            }
        }
    }
    let shown = &app.url[start..];
    let cursor_col = UnicodeWidthStr::width(&app.url[start..app.url_cursor.min(app.url.len())]);
    f.render_widget(
        Paragraph::new(shown).style(Style::default().fg(p.text)),
        input_inner,
    );
    f.set_cursor_position((
        input_inner.x + (cursor_col as u16).min(input_inner.width.saturating_sub(1)),
        input_inner.y,
    ));

    if app.loading_meta {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!("  {} ", util::spinner_frame(app.tick)),
                    Style::default().fg(p.accent),
                ),
                Span::styled(
                    "Analisando o link… playlists grandes podem demorar.",
                    Style::default().fg(p.warning),
                ),
            ])),
            chunks[4],
        );
    } else if let Some(error) = &app.url_error {
        f.render_widget(
            Paragraph::new(Line::styled(
                format!("  ✗ {}", util::truncate(error, 150)),
                Style::default().fg(p.error),
            )),
            chunks[4],
        );
    } else if spotify::is_spotify_url(&app.url) {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled("  ● ", Style::default().fg(p.spotify)),
                    Span::styled(
                        "Spotify detectado: metadados oficiais; áudio correspondente do YouTube.",
                        Style::default().fg(p.text),
                    ),
                ]),
                Line::styled(
                    "    Funciona com links públicos de música, álbum, playlist e artista.",
                    Style::default().fg(p.dim),
                ),
            ]),
            chunks[4],
        );
    } else {
        f.render_widget(
            Paragraph::new(Line::styled(
                "  Dica: Ctrl+V cola o link; o tipo de mídia é detectado automaticamente.",
                Style::default().fg(p.dim),
            )),
            chunks[4],
        );
    }

    let hint = if app.loading_meta {
        "  Esc cancelar análise"
    } else {
        "  Enter analisar   Ctrl+V colar   Esc voltar   ? ajuda"
    };
    f.render_widget(
        Paragraph::new(Line::styled(hint, Style::default().fg(p.dim))),
        chunks[6],
    );
}

// ---------------------------------------------------------------------------
// Faixas
// ---------------------------------------------------------------------------

fn draw_tracks(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let desired_height = (app.tracks.len() as u16 + 9).clamp(14, 26);
    let popup = centered_rect(
        area.width.saturating_sub(4).min(104),
        desired_height.min(area.height.saturating_sub(3)),
        area,
    );
    let title = format!(
        "Selecionar faixas — {}",
        util::truncate(
            &app.fetched
                .as_ref()
                .map(|f| f.title.clone())
                .unwrap_or_default(),
            60
        )
    );
    let inner = overlay_panel(f, popup, &title, p);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(inner);

    let source = app
        .fetched
        .as_ref()
        .map(|f| f.kind)
        .unwrap_or(app.hint_kind);
    let mut info = vec![
        Span::styled("  ", Style::default()),
        Span::styled(
            format!("{} ", source.label()),
            Style::default()
                .fg(kind_color(source, p))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("• {} faixas • ", app.tracks.len()),
            Style::default().fg(p.dim),
        ),
        Span::styled(
            format!(
                "{} de {} selecionadas",
                app.track_selected_count(),
                app.tracks.len()
            ),
            Style::default().fg(p.success),
        ),
    ];
    if source == MediaKind::Spotify {
        info.push(Span::styled(
            "  • metadados Spotify, áudio via YouTube",
            Style::default().fg(p.spotify),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(info)), chunks[0]);

    let filtered = app.filtered_track_indices();
    let list_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(p.border))
        .title(Span::styled(
            format!(" Faixas ({}) ", filtered.len()),
            Style::default().fg(p.dim),
        ));
    let list_inner = list_block.inner(chunks[1]);
    f.render_widget(list_block, chunks[1]);

    if filtered.is_empty() {
        f.render_widget(
            Paragraph::new(Line::styled(
                "  Nenhuma faixa corresponde ao filtro.",
                Style::default().fg(p.warning),
            )),
            list_inner,
        );
    } else {
        let items: Vec<ListItem> = filtered
            .iter()
            .enumerate()
            .map(|(pos, absolute)| {
                let t = &app.tracks[*absolute];
                let checked = app.track_selected.get(*absolute).copied().unwrap_or(true);
                let selected = pos == app.track_sel;
                let fg = if selected { p.selected_fg } else { p.text };
                let dim = if selected { p.selected_fg } else { p.dim };
                let check_color = if selected {
                    p.selected_fg
                } else if checked {
                    p.success
                } else {
                    p.dim
                };
                let mut spans = vec![
                    Span::styled(
                        if checked { " [x] " } else { " [ ] " },
                        Style::default()
                            .fg(check_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("{:>3}  ", absolute + 1), Style::default().fg(dim)),
                ];
                spans.push(Span::styled(
                    if t.explicit { "E " } else { "  " },
                    Style::default().fg(p.warning),
                ));
                let artist = t.display_artist();
                let available = (list_inner.width as usize).saturating_sub(46).max(12);
                let artist_width = (list_inner.width as usize)
                    .saturating_sub(available + 41)
                    .clamp(12, 24);
                spans.push(Span::styled(
                    format!(
                        "{:<available$}",
                        util::truncate(&t.title, available),
                        available = available
                    ),
                    Style::default().fg(fg).add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
                ));
                spans.push(Span::styled(
                    format!(
                        "  {:<artist_width$}",
                        util::truncate(&artist, artist_width),
                        artist_width = artist_width
                    ),
                    Style::default().fg(dim),
                ));
                spans.push(Span::styled(
                    format!(
                        " {:>7}",
                        t.duration
                            .map(util::format_duration)
                            .unwrap_or_else(|| "—".into())
                    ),
                    Style::default().fg(dim),
                ));
                ListItem::new(Line::from(spans))
            })
            .collect();
        let list = List::new(items)
            .highlight_style(Style::default().bg(p.selected_bg))
            .highlight_symbol("▸");
        let mut state = ListState::default().with_selected(Some(app.track_sel));
        f.render_stateful_widget(list, list_inner, &mut state);
    }

    let mut footer: Vec<Line> = Vec::new();
    if app.filtering || !app.track_filter.is_empty() {
        footer.push(Line::from(vec![
            Span::styled("  Filtro: ", Style::default().fg(p.accent)),
            Span::styled(
                if app.filtering {
                    format!("{}▌", app.filter_buf)
                } else {
                    app.track_filter.clone()
                },
                Style::default().fg(p.text),
            ),
            Span::styled(
                format!("  ({} resultado(s))", filtered.len()),
                Style::default().fg(p.dim),
            ),
        ]));
    } else {
        footer.push(Line::styled(
            "  Marque o que deseja baixar; o padrão é tudo selecionado.",
            Style::default().fg(p.dim),
        ));
    }
    footer.push(Line::from(vec![
        Span::styled("  Espaço ", Style::default().fg(p.accent)),
        Span::styled("marcar", Style::default().fg(p.dim)),
        separator(p),
        Span::styled("A ", Style::default().fg(p.accent)),
        Span::styled("todas", Style::default().fg(p.dim)),
        separator(p),
        Span::styled("N ", Style::default().fg(p.accent)),
        Span::styled("nenhuma", Style::default().fg(p.dim)),
        separator(p),
        Span::styled("/ ", Style::default().fg(p.accent)),
        Span::styled("filtrar", Style::default().fg(p.dim)),
        separator(p),
        Span::styled("Enter ", Style::default().fg(p.accent)),
        Span::styled("continuar", Style::default().fg(p.dim)),
        separator(p),
        Span::styled("Esc ", Style::default().fg(p.accent)),
        Span::styled("voltar", Style::default().fg(p.dim)),
    ]));
    f.render_widget(Paragraph::new(footer), chunks[2]);
}

// ---------------------------------------------------------------------------
// Opções
// ---------------------------------------------------------------------------

fn option_rows(app: &App) -> Vec<(&'static str, String)> {
    let o = &app.opts;
    let mut rows: Vec<(&'static str, String)> = Vec::new();
    if o.kind.is_audio() {
        rows.push(("Formato de áudio", o.audio_format.label().into()));
        rows.push(("Qualidade de áudio", o.audio_quality.label().into()));
    } else {
        rows.push(("Contêiner de vídeo", o.video_container.label().into()));
        rows.push(("Resolução", o.video_quality.label().into()));
    }
    rows.push(("Converter durante o download", yes_no(o.convert)));
    rows.push(("Baixar e embutir capa", yes_no(o.embed_thumbnail)));
    rows.push(("Incorporar metadados", yes_no(o.embed_metadata)));
    rows.push(("Organizar em artista/álbum/ano", yes_no(o.organize)));
    rows.push(("Numerar faixas", yes_no(o.number_tracks)));
    rows.push(("Remover trechos patrocinados", yes_no(o.sponsorblock)));
    rows.push(("Gerar lista M3U", yes_no(o.write_m3u)));
    rows.push(("Iniciar download", "▶".into()));
    rows.push(("Voltar", "Esc".into()));
    rows
}

fn yes_no(value: bool) -> String {
    if value {
        "Sim".into()
    } else {
        "Não".into()
    }
}

fn draw_options(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let popup = centered_rect(area.width.saturating_sub(4).min(88), 19, area);
    let source = app
        .fetched
        .as_ref()
        .map(|f| f.kind)
        .unwrap_or(app.hint_kind);
    let title = format!(
        "Opções — {} — {}",
        source.label(),
        util::plural(app.track_selected_count(), "faixa", "faixas")
    );
    let inner = overlay_panel(f, popup, &title, p);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(OPTION_ROWS as u16 + 1),
            Constraint::Length(3),
        ])
        .split(inner);

    let mut intro = vec![
        Span::styled("  ", Style::default()),
        Span::styled("Configurações desta busca", Style::default().fg(p.dim)),
    ];
    if source == MediaKind::Spotify {
        intro.push(Span::styled(
            "  •  metadados e capa do Spotify, áudio do YouTube",
            Style::default().fg(p.spotify),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(intro)), chunks[0]);

    let rows = option_rows(app);
    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(i, (label, value))| {
            let selected = i == app.opt_sel;
            let is_action = i >= OPTION_ROWS - 2;
            let fg = if selected {
                p.selected_fg
            } else if is_action {
                p.success
            } else {
                p.text
            };
            let value_style = if selected {
                Style::default()
                    .fg(p.selected_fg)
                    .add_modifier(Modifier::BOLD)
            } else if is_action {
                Style::default().fg(p.success)
            } else {
                Style::default().fg(p.accent)
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  {label:<36}"),
                    Style::default().fg(fg).add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
                ),
                Span::styled(format!(" {value}"), value_style),
            ]))
        })
        .collect();
    let list = List::new(items)
        .highlight_style(Style::default().bg(p.selected_bg))
        .highlight_symbol("▸");
    let mut state = ListState::default().with_selected(Some(app.opt_sel));
    f.render_stateful_widget(list, chunks[1], &mut state);

    let first = app
        .fetched
        .as_ref()
        .and_then(|f| f.selected(&app.track_selected).into_iter().next())
        .or_else(|| app.tracks.first().cloned());
    let mut footer: Vec<Line> = Vec::new();
    if let Some(t) = first {
        footer.push(Line::from(vec![
            Span::styled("  Primeira: ", Style::default().fg(p.dim)),
            Span::styled(
                util::truncate(&format!("{} — {}", t.display_artist(), t.title), 72),
                Style::default().fg(p.text),
            ),
        ]));
    }
    footer.push(Line::from(vec![
        Span::styled("  ↑↓ ", Style::default().fg(p.accent)),
        Span::styled("navegar", Style::default().fg(p.dim)),
        separator(p),
        Span::styled("←→/Espaço ", Style::default().fg(p.accent)),
        Span::styled("alterar", Style::default().fg(p.dim)),
        separator(p),
        Span::styled("S ", Style::default().fg(p.accent)),
        Span::styled("iniciar", Style::default().fg(p.dim)),
        separator(p),
        Span::styled("Esc ", Style::default().fg(p.accent)),
        Span::styled("voltar", Style::default().fg(p.dim)),
    ]));
    f.render_widget(Paragraph::new(footer), chunks[2]);
}

// ---------------------------------------------------------------------------
// Progresso
// ---------------------------------------------------------------------------

fn draw_progress(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let popup = centered_rect(
        area.width.saturating_sub(4).min(96),
        area.height.saturating_sub(3).clamp(18, 30),
        area,
    );
    let inner = overlay_panel(f, popup, "Download em andamento", p);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(inner);

    let spinner = util::spinner_frame(app.tick);
    let stage = if app.stage.is_empty() {
        "Preparando"
    } else {
        &app.stage
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    format!("  {spinner} "),
                    Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    stage.to_uppercase(),
                    Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("   {:.0}%", app.progress_pct),
                    Style::default().fg(p.text),
                ),
                Span::styled(
                    format!("   {} ", elapsed_text(app)),
                    Style::default().fg(p.dim),
                ),
            ]),
            Line::styled(format!("  {}", app.detail), Style::default().fg(p.dim)),
        ]),
        chunks[0],
    );

    let overall = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(p.border))
                .title(Span::styled(
                    " Progresso geral ",
                    Style::default().fg(p.dim),
                )),
        )
        .gauge_style(
            Style::default()
                .fg(p.accent)
                .bg(p.bg)
                .add_modifier(Modifier::BOLD),
        )
        .percent(app.progress_pct.clamp(0.0, 100.0) as u16)
        .label(Span::raw(""));
    f.render_widget(overall, chunks[1]);

    let current = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(p.border))
                .title(Span::styled(" Faixa atual ", Style::default().fg(p.dim))),
        )
        .gauge_style(Style::default().fg(p.spotify).bg(p.bg))
        .percent(app.progress_current.clamp(0.0, 100.0) as u16)
        .label(Span::raw(""));
    f.render_widget(current, chunks[2]);

    let mut stats = vec![Span::styled("  ", Style::default())];
    if let Some(speed) = &app.speed {
        stats.push(Span::styled(
            format!("↓ {speed}"),
            Style::default().fg(p.success),
        ));
        stats.push(separator(p));
    }
    if let Some(eta) = &app.eta {
        stats.push(Span::styled(
            format!("ETA {eta}"),
            Style::default().fg(p.warning),
        ));
        stats.push(separator(p));
    }
    if app.item_index > 0 {
        stats.push(Span::styled(
            format!(
                "{} de {} {}",
                app.item_index.min(app.item_total),
                app.item_total,
                if app.item_total == 1 {
                    "faixa"
                } else {
                    "faixas"
                }
            ),
            Style::default().fg(p.dim),
        ));
    } else if app.active_tracks.len() > 1 && app.seen_files > 0 {
        stats.push(Span::styled(
            format!(
                "{} de {} {}",
                app.seen_files.min(app.active_tracks.len()),
                app.active_tracks.len(),
                if app.active_tracks.len() == 1 {
                    "faixa"
                } else {
                    "faixas"
                }
            ),
            Style::default().fg(p.dim),
        ));
    } else {
        stats.push(Span::styled(
            util::plural(app.active_tracks.len(), "faixa", "faixas"),
            Style::default().fg(p.dim),
        ));
    }
    stats.push(separator(p));
    stats.push(Span::styled(
        format!("faixa {:.0}%", app.progress_current),
        Style::default().fg(p.accent),
    ));
    if !app.progress_name.is_empty() {
        stats.push(separator(p));
        stats.push(Span::styled(
            util::truncate(&app.progress_name, 44),
            Style::default().fg(p.text),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(stats)), chunks[3]);

    let log_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(p.border))
        .title(Span::styled(" Registro ", Style::default().fg(p.dim)));
    let log_inner = log_block.inner(chunks[4]);
    f.render_widget(log_block, chunks[4]);
    let visible = log_inner.height as usize;
    let end = app.log.len().saturating_sub(app.log_scroll);
    let start = end.saturating_sub(visible);
    let lines: Vec<Line> = app.log[start..end]
        .iter()
        .map(|line| {
            let style = if line.starts_with("ERRO") || line.to_ascii_lowercase().contains("error") {
                Style::default().fg(p.error)
            } else if line.starts_with('✓') {
                Style::default().fg(p.success)
            } else if line.starts_with('⚠') {
                Style::default().fg(p.warning)
            } else if line.starts_with('✗') {
                Style::default().fg(p.error)
            } else {
                Style::default().fg(p.dim)
            };
            Line::styled(format!(" {line}"), style)
        })
        .collect();
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), log_inner);

    let hint = if app.organizing {
        "  Aguarde: organizando arquivos e capas…"
    } else if app.cancelled {
        "  Cancelando…"
    } else {
        "  Esc cancelar   PgUp/PgDn rolar registro   ? ajuda"
    };
    f.render_widget(
        Paragraph::new(Line::styled(hint, Style::default().fg(p.dim))),
        chunks[5],
    );
}

// ---------------------------------------------------------------------------
// Histórico
// ---------------------------------------------------------------------------

fn draw_history(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let popup = centered_rect(
        area.width.saturating_sub(4).min(104),
        area.height.saturating_sub(3),
        area,
    );
    let inner = overlay_panel(f, popup, "Histórico de downloads", p);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(inner);

    let total_items: usize = app.config.history.iter().map(|h| h.items).sum();
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                util::plural(app.config.history.len(), "download", "downloads"),
                Style::default().fg(p.text).add_modifier(Modifier::BOLD),
            ),
            separator(p),
            Span::styled(format!("{total_items} itens"), Style::default().fg(p.dim)),
            separator(p),
            Span::styled(
                if app.pending_clear_history {
                    "pressione C novamente para limpar"
                } else {
                    "C limpa o histórico"
                },
                Style::default().fg(if app.pending_clear_history {
                    p.warning
                } else {
                    p.dim
                }),
            ),
        ])),
        chunks[0],
    );

    if app.config.history.is_empty() {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::styled(
                    "  Nenhum download registrado ainda.",
                    Style::default().fg(p.dim),
                ),
                Line::styled(
                    "  Conclua um download para vê-lo aqui.",
                    Style::default().fg(p.dim),
                ),
            ]),
            chunks[1],
        );
    } else {
        let visible = (chunks[1].height as usize / 3).max(1);
        let start = if app.history_sel >= visible {
            app.history_sel - visible + 1
        } else {
            0
        };
        let mut lines: Vec<Line> = Vec::new();
        for (offset, entry) in app
            .config
            .history
            .iter()
            .rev()
            .enumerate()
            .skip(start)
            .take(visible)
        {
            let selected = offset == app.history_sel;
            let bg = if selected { p.selected_bg } else { p.panel };
            let kind = MediaKind::from_key(&entry.kind);
            let fg = if selected { p.selected_fg } else { p.text };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {}  ", entry.date),
                    Style::default()
                        .fg(if selected { p.selected_fg } else { p.dim })
                        .bg(bg),
                ),
                Span::styled(
                    format!("{:<10}", entry.kind),
                    Style::default()
                        .fg(if selected {
                            p.selected_fg
                        } else {
                            kind_color(kind, p)
                        })
                        .bg(bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:>3} item(ns)  ", entry.items),
                    Style::default().fg(fg).bg(bg),
                ),
                Span::styled(
                    util::truncate(
                        if entry.title.is_empty() {
                            &entry.url
                        } else {
                            &entry.title
                        },
                        60,
                    ),
                    Style::default().fg(fg).bg(bg).add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
                ),
            ]));
            lines.push(Line::styled(
                format!("      {}", util::truncate(&entry.url, 90)),
                Style::default()
                    .fg(if selected { p.selected_fg } else { p.dim })
                    .bg(bg),
            ));
            lines.push(Line::styled(
                format!("      → {}", util::truncate(&entry.dest, 90)),
                Style::default()
                    .fg(if selected { p.selected_fg } else { p.border })
                    .bg(bg),
            ));
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), chunks[1]);
    }

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  Enter ", Style::default().fg(p.accent)),
            Span::styled("repetir", Style::default().fg(p.dim)),
            separator(p),
            Span::styled("O ", Style::default().fg(p.accent)),
            Span::styled("abrir pasta", Style::default().fg(p.dim)),
            separator(p),
            Span::styled("C ", Style::default().fg(p.accent)),
            Span::styled("limpar", Style::default().fg(p.dim)),
            separator(p),
            Span::styled("Esc ", Style::default().fg(p.accent)),
            Span::styled("voltar", Style::default().fg(p.dim)),
        ])),
        chunks[2],
    );
}

// ---------------------------------------------------------------------------
// Configurações
// ---------------------------------------------------------------------------

fn settings_rows(app: &App) -> Vec<(&'static str, String)> {
    let c = &app.config;
    let editing = app.settings_editing;
    let text = |row: usize, value: String, empty: &str| -> String {
        if editing == Some(row) {
            format!("{}▌", app.edit_buf)
        } else if value.is_empty() {
            empty.to_string()
        } else {
            value
        }
    };
    vec![
        ("Pasta base de download", text(0, c.base_dir.clone(), "—")),
        (
            "Formato de áudio padrão",
            AudioFormat::from_key(&c.default_audio_format)
                .label()
                .into(),
        ),
        (
            "Qualidade de áudio padrão",
            AudioQuality::from_key(&c.default_audio_quality)
                .label()
                .into(),
        ),
        (
            "Contêiner de vídeo padrão",
            VideoContainer::from_key(&c.default_video_container)
                .label()
                .into(),
        ),
        (
            "Resolução de vídeo padrão",
            VideoQuality::from_key(&c.default_video_quality)
                .label()
                .into(),
        ),
        ("Proxy (vazio = nenhum)", text(5, c.proxy.clone(), "—")),
        (
            "Espera entre requisições",
            if c.sleep_requests == 0 {
                "Nenhuma".into()
            } else {
                format!("{}s", c.sleep_requests)
            },
        ),
        ("Remover patrocínios (SponsorBlock)", yes_no(c.sponsorblock)),
        ("Tema da interface", c.theme().label().into()),
        ("Notificações desktop", yes_no(c.notifications)),
        ("Gerar listas M3U por padrão", yes_no(c.write_m3u)),
        (
            "Cookies do navegador",
            text(11, c.cookies_from_browser.clone(), "—"),
        ),
        ("Limite de velocidade", text(12, c.limit_rate.clone(), "—")),
        (
            "Buscas Spotify simultâneas",
            format!("{}", c.spotify_workers),
        ),
        ("Arquivo de histórico do yt-dlp", yes_no(c.download_archive)),
        ("Abrir pasta ao concluir", yes_no(c.open_folder_after)),
        (
            "Atualizar yt-dlp",
            if app.updating {
                "Atualizando…".into()
            } else if app.update_msg.is_empty() {
                "Enter para atualizar".into()
            } else {
                util::truncate(&app.update_msg, 44)
            },
        ),
        ("Sobre o ytui", format!("v{VERSION} — créditos")),
        ("Voltar", "Esc".into()),
    ]
}

fn draw_settings(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let popup = centered_rect(
        area.width.saturating_sub(4).min(108),
        area.height.saturating_sub(3),
        area,
    );
    let inner = overlay_panel(f, popup, "Configurações", p);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .split(inner);

    let rows = settings_rows(app);
    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(i, (label, value))| {
            let selected = i == app.settings_sel;
            let editing = app.settings_editing == Some(i);
            let action = matches!(i, 16..=18);
            let fg = if selected {
                p.selected_fg
            } else if editing {
                p.success
            } else if action {
                p.accent
            } else {
                p.text
            };
            let value_style = if selected {
                Style::default()
                    .fg(p.selected_fg)
                    .add_modifier(Modifier::BOLD)
            } else if editing {
                Style::default().fg(p.success)
            } else {
                Style::default().fg(p.accent2)
            };
            let value = util::truncate(value, 56);
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  {label:<38}"),
                    Style::default()
                        .fg(fg)
                        .add_modifier(if selected || editing {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(format!(" {value}"), value_style),
            ]))
        })
        .collect();
    let list = List::new(items)
        .highlight_style(Style::default().bg(p.selected_bg))
        .highlight_symbol("▸");
    let mut state = ListState::default().with_selected(Some(app.settings_sel));
    f.render_stateful_widget(list, chunks[0], &mut state);

    let mut tools: Vec<Line> = vec![Line::styled(
        "  Diagnóstico",
        Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
    )];
    if app.tool_status.is_empty() {
        tools.push(Line::styled(
            "    verificando ferramentas…",
            Style::default().fg(p.dim),
        ));
    }
    for (name, ok, version) in &app.tool_status {
        tools.push(Line::from(vec![
            Span::styled(
                if *ok { "    ● " } else { "    ○ " },
                Style::default().fg(if *ok { p.success } else { p.error }),
            ),
            Span::styled(format!("{name:<9}"), Style::default().fg(p.text)),
            Span::styled(
                util::truncate(version, 54),
                Style::default().fg(if *ok { p.dim } else { p.error }),
            ),
        ]));
    }
    f.render_widget(Paragraph::new(tools), chunks[1]);

    let hint = if app.settings_editing.is_some() {
        "  Digite o valor   Enter salvar   Esc cancelar"
    } else {
        "  ↑↓ navegar   ←→/Espaço alterar   Enter editar/executar   Esc voltar"
    };
    f.render_widget(
        Paragraph::new(Line::styled(hint, Style::default().fg(p.dim))),
        chunks[2],
    );
}

// ---------------------------------------------------------------------------
// Resultado
// ---------------------------------------------------------------------------

fn draw_done(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let extra = app.done_message.lines().count().min(5)
        + app.done_missing.iter().take(4).count()
        + app.done_warnings.iter().take(3).count()
        + usize::from(!app.done_dirs.is_empty())
        + usize::from(app.done_m3u.is_some());
    let desired_height = ((12 + extra).clamp(16, 30)) as u16;
    let popup = centered_rect(
        area.width.saturating_sub(4).min(90),
        desired_height.min(area.height.saturating_sub(3)),
        area,
    );
    let title = if app.done_success {
        "Concluído"
    } else {
        "Atenção"
    };
    let inner = overlay_panel(f, popup, title, p);
    let color = if app.done_success { p.success } else { p.error };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            if app.done_success { "  ✓  " } else { "  !  " },
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            util::truncate(&app.done_title, 60),
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  ({})", util::plural(app.done_total, "faixa", "faixas")),
            Style::default().fg(p.dim),
        ),
    ]));
    lines.push(Line::from(""));
    for msg_line in app.done_message.lines() {
        lines.push(Line::styled(
            format!("  {msg_line}"),
            Style::default().fg(p.text),
        ));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            format!(
                "  {} {}",
                app.done_moved,
                if app.done_moved == 1 {
                    "movido"
                } else {
                    "movidos"
                }
            ),
            Style::default().fg(p.success).add_modifier(Modifier::BOLD),
        ),
        separator(p),
        Span::styled(
            format!(
                "{} {}",
                app.done_covers,
                if app.done_covers == 1 {
                    "capa"
                } else {
                    "capas"
                }
            ),
            Style::default().fg(p.accent),
        ),
        separator(p),
        Span::styled(
            format!(
                "{} {}",
                app.done_tagged,
                if app.done_tagged == 1 { "tag" } else { "tags" }
            ),
            Style::default().fg(p.spotify),
        ),
        if !app.done_missing.is_empty() {
            Span::styled(
                format!("  •  {} não localizados", app.done_missing.len()),
                Style::default().fg(p.warning),
            )
        } else {
            Span::raw("")
        },
    ]));
    lines.push(Line::from(""));

    if !app.done_dirs.is_empty() {
        lines.push(Line::styled(
            format!(
                "  Destino: {}",
                util::truncate(&app.done_dirs[0].to_string_lossy(), 74)
            ),
            Style::default().fg(p.dim),
        ));
    }
    if let Some(m3u) = &app.done_m3u {
        lines.push(Line::styled(
            format!(
                "  Lista M3U: {}",
                util::truncate(&m3u.to_string_lossy(), 70)
            ),
            Style::default().fg(p.accent2),
        ));
    }
    if !app.done_missing.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::styled(
            "  Não localizados:",
            Style::default().fg(p.warning),
        ));
        for item in app.done_missing.iter().take(4) {
            lines.push(Line::styled(
                format!("    • {}", util::truncate(item, 72)),
                Style::default().fg(p.dim),
            ));
        }
    }
    if !app.done_warnings.is_empty() {
        lines.push(Line::from(""));
        for warning in app.done_warnings.iter().take(3) {
            lines.push(Line::styled(
                format!("  ! {}", util::truncate(warning, 76)),
                Style::default().fg(p.warning),
            ));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  O ", Style::default().fg(p.accent)),
        Span::styled("abrir pasta", Style::default().fg(p.dim)),
        separator(p),
        Span::styled("N ", Style::default().fg(p.accent)),
        Span::styled("novo download", Style::default().fg(p.dim)),
        separator(p),
        Span::styled("Enter/Esc ", Style::default().fg(p.accent)),
        Span::styled("menu", Style::default().fg(p.dim)),
    ]));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

// ---------------------------------------------------------------------------
// Sobreposições
// ---------------------------------------------------------------------------

fn draw_help(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let popup = centered_rect(
        area.width.saturating_sub(4).min(86),
        area.height.saturating_sub(2).min(31),
        area,
    );
    let inner = overlay_panel(f, popup, "Ajuda e atalhos", p);
    let section = |title: &'static str| {
        Line::styled(
            format!("  {title}"),
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
        )
    };
    let item = |keys: &'static str, desc: &'static str| {
        Line::from(vec![
            Span::styled(format!("    {keys:<16}"), Style::default().fg(p.text)),
            Span::styled(desc, Style::default().fg(p.dim)),
        ])
    };
    let lines = vec![
        section("Global"),
        item("↑ / ↓", "navegar menus e listas"),
        item("Enter", "confirmar / abrir"),
        item("Esc", "voltar / cancelar"),
        item("? / F1", "esta ajuda"),
        item("T", "alternar tema"),
        item("Q / Ctrl+C", "sair (Q no menu; Ctrl+C em qualquer tela)"),
        Line::from(""),
        section("Link"),
        item("Ctrl+V", "colar da área de transferência"),
        item("Ctrl+U", "limpar o campo"),
        item("← / →", "mover o cursor"),
        Line::from(""),
        section("Seleção de faixas"),
        item("Espaço", "marcar / desmarcar"),
        item("A / N / I", "todas / nenhuma / inverter"),
        item("/", "filtrar por título ou artista"),
        item("PgUp / PgDn", "pular 10 faixas"),
        Line::from(""),
        section("Download"),
        item("Esc", "cancelar"),
        item("PgUp / PgDn", "rolar o registro"),
        Line::from(""),
        section("Resultado"),
        item("O", "abrir a pasta de destino"),
        item("N", "iniciar outro download"),
        Line::from(""),
        Line::styled(
            "  Spotify: links públicos fornecem metadados; o áudio é baixado do YouTube.",
            Style::default().fg(p.spotify),
        ),
        Line::styled("  Feche com Esc, ? ou Enter.", Style::default().fg(p.dim)),
    ];
    let list_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(1),
    };
    let skip = (app.help_scroll as usize).min(lines.len());
    let visible: Vec<Line> = lines.into_iter().skip(skip).collect();
    f.render_widget(
        Paragraph::new(visible).wrap(Wrap { trim: false }),
        list_area,
    );
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  PgUp/PgDn ", Style::default().fg(p.accent)),
            Span::styled("rolar  ·  ", Style::default().fg(p.dim)),
            Span::styled("Esc ", Style::default().fg(p.accent)),
            Span::styled("fechar", Style::default().fg(p.dim)),
        ])),
        Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        },
    );
}

fn draw_about(f: &mut Frame, area: Rect, p: &Palette) {
    let popup = centered_rect(area.width.saturating_sub(4).min(74), 21, area);
    let inner = overlay_panel(f, popup, "Sobre o ytui", p);
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  ytui ",
                Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("v{VERSION}"), Style::default().fg(p.text)),
        ]),
        Line::styled(
            "  Interface de terminal para baixar e organizar mídia.",
            Style::default().fg(p.dim),
        ),
        Line::from(""),
        Line::styled(
            "  YouTube, YouTube Music e Spotify em um só lugar.",
            Style::default().fg(p.text),
        ),
        Line::styled(
            "  Metadados, capas, playlists e listas M3U.",
            Style::default().fg(p.text),
        ),
        Line::from(""),
        Line::styled(
            "  Rust • ratatui • crossterm • yt-dlp • ffmpeg",
            Style::default().fg(p.accent2),
        ),
        Line::styled(
            "  Spotify: metadados públicos; áudio correspondente no YouTube.",
            Style::default().fg(p.spotify),
        ),
        Line::from(""),
        Line::styled(
            "  Use apenas conteúdo que você tem direito de baixar.",
            Style::default().fg(p.warning),
        ),
        Line::from(""),
        Line::styled(
            format!("  Config: {}", ytui::config::Config::path().display()),
            Style::default().fg(p.dim),
        ),
        Line::from(""),
        Line::styled("  Esc / Enter para fechar", Style::default().fg(p.dim)),
    ];
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_toast(f: &mut Frame, area: Rect, toast: &crate::app::Toast, p: &Palette) {
    let color = match toast.kind {
        ToastKind::Info => p.accent,
        ToastKind::Success => p.success,
        ToastKind::Error => p.error,
    };
    let width = area.width.saturating_sub(4).min(62);
    let rect = Rect {
        x: area.x + area.width.saturating_sub(width + 2),
        y: area.y + area.height.saturating_sub(5),
        width,
        height: 4,
    };
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color))
        .style(Style::default().bg(p.panel))
        .title(Span::styled(
            match toast.kind {
                ToastKind::Info => " aviso ",
                ToastKind::Success => " pronto ",
                ToastKind::Error => " erro ",
            },
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    f.render_widget(
        Paragraph::new(util::truncate(&toast.text, 180))
            .style(Style::default().fg(p.text))
            .wrap(Wrap { trim: true }),
        inner,
    );
}

// ---------------------------------------------------------------------------
// Barra de status
// ---------------------------------------------------------------------------

fn draw_status_bar(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let bar = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(1),
        width: area.width,
        height: 1,
    };
    let screen = match app.screen {
        Screen::Menu => "menu",
        Screen::Url => "link",
        Screen::Tracks => "faixas",
        Screen::Options => "opções",
        Screen::Progress => "download",
        Screen::History => "histórico",
        Screen::Settings => "configurações",
        Screen::Done => "resultado",
    };
    let narrow = area.width < 90;
    let left = if narrow {
        format!(" ◆ ytui {VERSION}  │  {screen} ")
    } else {
        format!(
            " ◆ ytui {VERSION}  │  {}  │  tema {} ",
            screen,
            app.theme().label()
        )
    };
    let tools_ok = !app.tool_status.is_empty() && app.tool_status.iter().all(|(_, ok, _)| *ok);
    let tools = if app.tool_status.is_empty() {
        "verificando ferramentas"
    } else if tools_ok {
        "ferramentas ok"
    } else {
        "ferramentas faltando"
    };
    let right = if narrow {
        " ? ajuda  │  ^C ".to_string()
    } else {
        format!(" {tools}  │  ? ajuda  │  Ctrl+C sair ")
    };
    let width = area.width as usize;
    let left_w = UnicodeWidthStr::width(left.as_str());
    let right_w = UnicodeWidthStr::width(right.as_str());
    let pad = width.saturating_sub(left_w + right_w);
    let line = Line::from(vec![
        Span::styled(left, Style::default().fg(p.selected_fg).bg(p.panel)),
        Span::styled(" ".repeat(pad), Style::default().bg(p.panel)),
        Span::styled(
            right,
            Style::default()
                .fg(if tools_ok { p.dim } else { p.warning })
                .bg(p.panel),
        ),
    ]);
    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(p.panel)),
        bar,
    );
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::app::Screen;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use ytui::backend::{CollectionKind, TrackMeta};
    use ytui::config::{Config, HistoryEntry};
    use ytui::media::FetchedMedia;

    fn render(app: &App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut out = String::new();
        for row in buffer.content.chunks(width as usize) {
            for cell in row {
                out.push_str(cell.symbol());
            }
            out.push('\n');
        }
        out
    }

    fn track(index: u32, title: &str, artist: &str) -> TrackMeta {
        let mut t = TrackMeta::default();
        t.id = format!("id{index}");
        t.title = title.into();
        t.artist = Some(artist.into());
        t.duration = Some(180.0 + index as f64);
        t.track_number = Some(index);
        t.collection = Some("Playlist de Teste".into());
        t.collection_kind = Some(CollectionKind::Playlist);
        t.total_tracks = Some(3);
        t
    }

    fn sample_app() -> App {
        let mut app = App::new(Config::default());
        app.tracks = vec![
            track(1, "Primeira Canção", "Artista Um"),
            track(2, "Segunda Canção", "Artista Dois"),
            track(3, "Terceira Canção", "Artista Três"),
        ];
        app.track_selected = vec![true, false, true];
        app.fetched = Some(FetchedMedia {
            kind: MediaKind::Spotify,
            title: "Playlist de Teste".into(),
            tracks: app.tracks.clone(),
            is_playlist: true,
            spotify: None,
        });
        app
    }

    #[test]
    fn menu_renders() {
        let app = sample_app();
        let text = render(&app, 110, 34);
        assert!(text.contains("sua biblioteca"));
        assert!(text.contains("YouTube Music"));
        assert!(text.contains("Spotify"));
        assert!(text.contains("Histórico"));
    }

    #[test]
    fn url_screen_renders_spotify_detection() {
        let mut app = sample_app();
        app.screen = Screen::Url;
        app.url = "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M".into();
        app.url_cursor = app.url.len();
        let text = render(&app, 100, 30);
        assert!(text.contains("Nova busca"));
        assert!(text.contains("Spotify detectado"));
    }

    #[test]
    fn tracks_screen_renders() {
        let mut app = sample_app();
        app.screen = Screen::Tracks;
        let text = render(&app, 110, 34);
        assert!(text.contains("Selecionar faixas"));
        assert!(text.contains("Primeira Canção"));
        assert!(text.contains("2 de 3 selecionadas"));
        assert!(text.contains("filtrar"));
    }

    #[test]
    fn options_screen_renders() {
        let mut app = sample_app();
        app.screen = Screen::Options;
        app.opts.kind = MediaKind::Spotify;
        let text = render(&app, 100, 30);
        assert!(text.contains("Opções"));
        assert!(text.contains("Formato de áudio"));
        assert!(text.contains("M3U"));
        assert!(text.contains("Iniciar download"));
    }

    #[test]
    fn progress_screen_renders() {
        let mut app = sample_app();
        app.screen = Screen::Progress;
        app.stage = "Correspondências".into();
        app.detail = "2/3 faixas".into();
        app.progress_pct = 42.0;
        app.progress_current = 17.0;
        app.progress_name = "Artista — Música".into();
        app.log = vec![
            "✓ Primeira → Primeira (Official Audio) (92%)".into(),
            "ERRO: exemplo de erro".into(),
        ];
        let text = render(&app, 100, 32);
        assert!(text.contains("Download em andamento"));
        assert!(text.contains("Progresso geral"));
        assert!(text.contains("CORRESPONDÊNCIAS"));
        assert!(text.contains("Primeira"));
    }

    #[test]
    fn history_screen_renders() {
        let mut app = sample_app();
        app.config.history = vec![HistoryEntry {
            date: "10/11/2026 21:00".into(),
            kind: "spotify".into(),
            url: "https://open.spotify.com/playlist/x".into(),
            items: 3,
            dest: "/tmp/Músicas".into(),
            title: "Playlist de Teste".into(),
        }];
        app.screen = Screen::History;
        let text = render(&app, 110, 30);
        assert!(text.contains("Histórico de downloads"));
        assert!(text.contains("Playlist de Teste"));
        assert!(text.contains("repetir"));
    }

    #[test]
    fn settings_screen_renders() {
        let mut app = sample_app();
        app.screen = Screen::Settings;
        app.tool_status = vec![
            ("yt-dlp".into(), true, "2026.10.26".into()),
            ("ffmpeg".into(), true, "ffmpeg version 9".into()),
        ];
        for _ in 0..3 {
            let text = render(&app, 110, 30);
            assert!(text.contains("Configurações"));
            assert!(text.contains("Pasta base"));
            assert!(text.contains("Diagnóstico"));
            app.settings_sel = (app.settings_sel + 5).min(18);
        }
    }

    #[test]
    fn done_screen_renders() {
        let mut app = sample_app();
        app.screen = Screen::Done;
        app.done_success = true;
        app.done_title = "Playlist de Teste".into();
        app.done_message = "3 arquivo(s) organizado(s), 1 capa(s) instalada(s).".into();
        app.done_moved = 3;
        app.done_covers = 1;
        app.done_tagged = 3;
        app.done_total = 3;
        app.done_dirs = vec![std::path::PathBuf::from("/tmp/Músicas")];
        let text = render(&app, 100, 30);
        assert!(text.contains("Concluído"));
        assert!(text.contains("3 movidos"));
        assert!(text.contains("abrir pasta"));
    }

    #[test]
    fn help_and_about_overlays_render() {
        let mut app = sample_app();
        app.help_open = true;
        let text = render(&app, 100, 32);
        assert!(text.contains("Ajuda e atalhos"));
        assert!(text.contains("Ctrl+V"));
        app.help_open = false;
        app.about_open = true;
        let text = render(&app, 100, 32);
        assert!(text.contains("Sobre o ytui"));
        assert!(text.contains("ratatui"));
    }

    #[test]
    fn all_themes_render() {
        let mut app = sample_app();
        for theme in ytui::theme::Theme::ALL {
            app.config.theme = theme.key().into();
            let text = render(&app, 90, 28);
            assert!(text.contains("YouTube Music"));
        }
    }

    #[test]
    fn tiny_terminal_shows_message() {
        let app = sample_app();
        let text = render(&app, 40, 10);
        assert!(text.contains("Terminal muito pequeno"));
    }

    #[test]
    fn minimum_terminal_renders_all_screens() {
        for screen in [
            Screen::Menu,
            Screen::Url,
            Screen::Tracks,
            Screen::Options,
            Screen::Progress,
            Screen::History,
            Screen::Settings,
            Screen::Done,
        ] {
            let mut app = sample_app();
            app.screen = screen;
            let text = render(&app, 60, 16);
            assert!(!text.trim().is_empty());
        }
    }

    #[test]
    fn toast_renders() {
        let mut app = sample_app();
        app.toast("Mensagem de teste", ToastKind::Success);
        let text = render(&app, 100, 30);
        assert!(text.contains("Mensagem de teste"));
    }
}
