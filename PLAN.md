# PLAN.md — gpui-term

Version 0.1.0. Статус: **85% готовности, pre-publish.**

---

## ✅ DONE (37/43)

### Фаза 0 — Критические баги
- [x] 0.1 Скролл колёсиком — `ScrollWheelEvent` в `mouse.rs`
- [x] 0.2 X10 mouse encoding — raw bytes вместо `format!`
- [x] 0.3 SSH мышь — полный селекшен + mouse-reporting

### Фаза 1 — TerminalCore (убрать дубли)
- [x] 1.1 `core.rs` — `TerminalCore`, `CoreEventListener`, `CoreDimensions`, `MouseResult`, `ScrollResult`
- [x] 1.2 Все общие методы (19 шт.)
- [x] 1.3 `terminal.rs` → тонкая PTY-обёртка (265 строк)
- [x] 1.4 `ssh.rs` → тонкая SSH-обёртка (443 строки)
- [x] 1.5 `entity.rs` — диспетчер без изменений
- [x] 1.6 Компиляция чистая

### Фаза 2 — Клавиатурные протоколы
- [x] 2.1 Kitty keyboard (CSI u)
- [x] 2.2 F5-F12 с модификаторами
- [x] 2.3 F13-F20
- [x] 2.4 Ctrl+цифры
- [x] 2.5 App Cursor + модификаторы (CSI вместо SS3)
- [x] 2.6 Alt+Shift / Alt+Ctrl
- [x] 2.7 Super/Cmd в `modifier_csi_param`

### Фаза 3 — Мышь
- [x] 3.1 Double/triple click (word/line selection)
- [x] 3.2 Block selection (Ctrl+Alt)
- [x] 3.3 Mode 1003 bare motion
- [x] 3.4 Mode 1004 focus in/out (`\x1b[I`/`\x1b[O`)
- [x] 3.5 Mode 1007 alternate scroll
- [x] 3.6 Mouse-up реальные координаты (не 1,1)
- [x] 3.7 Scroll release (`m` после `M`)
- [x] 3.8 Horizontal scroll (66/67)
- [x] 3.9 UTF-8 mouse encoding (mode 1005)

### Фаза 4 — Расширенные протоколы
- [x] 4.1 EventListener с mpsc-каналами
- [x] 4.2 Terminal ID (DA) — ответы через pty_write
- [x] 4.3 OSC 52 clipboard (CopyPaste)
- [x] 4.4 Bracketed paste
- [x] 4.5 Hyperlink Cmd+click (OSC 8)
- [x] 4.6 OSC 0/2 window title
- [x] 4.7 Bell (флаг `bell_pending`)
- [x] 4.8 Color queries (OSC 4,10,11,12)

### Фаза 5 — Рендер
- [x] 5.1 Underline styles (double/curly)
- [x] 5.2 Hidden text

### Фаза 6 — Полировка
- [x] 6.1 Cell dimensions из шрифта (`compute_cell_dimensions`)

---

## ⚠️ PARTIAL (2)

- 4.3 OSC 52 — `ClipboardStore`/`ClipboardLoad` работают, но не тестированы с реальными OSC 52-приложениями
- 5.1 Underline — dotted/dashed не различаются визуально (ограничение GPUI `UnderlineStyle`)

---

## ❌ NOT STARTED / DEFERRED (4)

| Пункт | Причина |
|-------|---------|
| 3.10 Smart selection (URL regex detection) | Сложно, OSC 8 hyperlink Cmd+click покрывает основной случай |
| 5.3 Cursor blinking | Нужен таймер + межфайловая логика |
| 5.4 OSC 12 cursor color | Требует доступа к term.colors() из элемента |
| 5.5 Smart cursor color (контраст) | Межфайловое, сложное |
| 6.2 NumLock | GPUI не даёт состояние клавиши |
| 6.3 Resize артефакты | Частично починено (`resize_pending` + debounce SIGWINCH), но остаточные артефакты при сжатии окна — комплексная проблема |
| 6.4 Damage tracking | Оптимизация, не баг |

---

## Файлы и размеры

| Файл | Строк | Роль |
|------|-------|------|
| `core.rs` | 570 | Общая логика (TerminalCore) |
| `terminal.rs` | 265 | PTY-обёртка |
| `ssh.rs` | 443 | SSH-обёртка + russh |
| `element.rs` | 630 | GPUI Element |
| `painter.rs` | 480 | Cell → text runs |
| `keys.rs` | 370 | Keystroke → bytes |
| `entity.rs` | 270 | TerminalEntity enum |
| `mouse.rs` | 295 | Mouse listeners |
| `colors.rs` | 255 | 256-color + RGB |
| `contrast.rs` | 230 | APCA |
| `content.rs` | 100 | Content + TerminalBounds |
| `cursor.rs` | 140 | Cursor rendering |
| `font.rs` | 55 | Font fallback + cell dims |
| `highlight.rs` | 95 | Selection highlight |
| `scrollbar.rs` | 60 | Scrollbar |
| `url.rs` | 50 | URL opening |
| **Итого** | **~4450** | |

---

## Ближайшие шаги

1. Тестирование SSH на localhost
2. crates.io публикация
3. Фаза 5.3-5.5 (курсор) — после стабилизации API
4. Фаза 6.3 (артефакты ресайза) — исследовать подходы Ghostty/iTerm2
