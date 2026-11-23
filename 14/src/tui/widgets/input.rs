//! Ввод: многострочное отображение длинного текста и видимый курсор.

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::tui::state::AppState;
use crate::tui::theme;
use crate::tui::widgets::modal::centered;

pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let Some(input) = &state.input else {
        return;
    };

    // Длинные правила инвариантов не помещаются в одну строку: окно
    // расширяется под терминал, текст переносится, а вьюпорт следует за
    // курсором. Минимум — 8 строк, чтобы были видны заголовок, текст и подсказка.
    let width = area.width.saturating_sub(4).clamp(30, 100);
    let text_width = width.saturating_sub(2).max(1) as usize;
    let chars_count = input.buffer.chars().count();
    let wrapped_lines = chars_count.div_ceil(text_width).max(1);
    let desired_height = (wrapped_lines as u16 + 4).clamp(8, area.height.saturating_sub(2));
    let rect = centered(area, width, desired_height);
    frame.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Ввод · длинный текст переносится ")
        .border_style(theme::active_border());
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    if inner.width == 0 || inner.height < 3 {
        return;
    }

    let label = Paragraph::new(Line::from(Span::styled(
        format!("{} ({chars_count}/500)", input.label),
        theme::accent(),
    )));
    frame.render_widget(
        label,
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        },
    );

    let content_area = Rect {
        x: inner.x,
        y: inner.y.saturating_add(1),
        width: inner.width,
        height: inner.height.saturating_sub(2),
    };
    if content_area.height == 0 || content_area.width == 0 {
        return;
    }

    let chars: Vec<char> = input.buffer.chars().collect();
    let cursor = input.cursor.min(chars.len());
    let text_width = content_area.width as usize;

    // Жёсткий перенос по ширине: позиция курсора однозначно отображается
    // на строку и колонку. Пустой буфер — одна пустая строка.
    let mut rows: Vec<(usize, usize)> = Vec::new();
    if chars.is_empty() {
        rows.push((0, 0));
    } else {
        let mut start = 0;
        while start < chars.len() {
            let end = (start + text_width).min(chars.len());
            rows.push((start, end));
            start = end;
        }
        // Курсор в конце строки, ровно заполнившей ширину, должен быть виден
        // на следующей (пустой) строке, а не «прилипать» к последнему символу.
        if cursor == chars.len() && chars.len() % text_width == 0 {
            rows.push((chars.len(), chars.len()));
        }
    }

    let cursor_row = cursor / text_width;
    let cursor_col = cursor % text_width;
    let visible_height = content_area.height as usize;
    let max_scroll = rows.len().saturating_sub(visible_height);
    let scroll = cursor_row
        .saturating_sub(visible_height.saturating_sub(1))
        .min(max_scroll);

    let mut lines: Vec<Line<'static>> = Vec::new();
    for (row_index, (start, end)) in rows.iter().enumerate().skip(scroll).take(visible_height) {
        let row_text: String = chars[*start..*end].iter().collect();
        if row_index == cursor_row {
            let local_cursor = cursor.saturating_sub(*start).min(row_text.chars().count());
            let before: String = row_text.chars().take(local_cursor).collect();
            let after: String = row_text.chars().skip(local_cursor + 1).collect();
            let cursor_char: String = row_text
                .chars()
                .nth(local_cursor)
                .map(|ch| ch.to_string())
                .unwrap_or_else(|| " ".to_string());
            lines.push(Line::from(vec![
                Span::raw(before),
                Span::styled(cursor_char, theme::selected()),
                Span::raw(after),
            ]));
            // Показываем системный курсор в той же точке: так он виден даже
            // в терминалах, которые не отрисовывают REVERSED-блок как курсор.
            frame.set_cursor_position(Position {
                x: content_area.x + cursor_col as u16,
                y: content_area.y + (row_index - scroll) as u16,
            });
        } else {
            lines.push(Line::from(row_text));
        }
    }
    frame.render_widget(Paragraph::new(lines), content_area);

    let hint = Paragraph::new(Line::from(Span::styled(
        "Enter — подтвердить · Esc — отмена · ←/→/Home/End · текст переносится",
        theme::muted(),
    )));
    frame.render_widget(
        hint,
        Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        },
    );
}
