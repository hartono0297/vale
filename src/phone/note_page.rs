//! The read-only page of a sticky note sent to the phone ("Kirim ke HP").
//!
//! Pure: a `NoteShare` (the note as it was when it was sent) goes in, one
//! small HTML document comes out. It has inline CSS only: no script, no image,
//! no font and no outside address, so the server can answer it under
//! `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'`.
//! The boxes of a checklist are `disabled` and there is no form and no
//! script, so nothing ticked on the phone can reach the PC.
//!
//! The note's formatting is shown (bold, italic, underline, strike, a
//! highlight and a text colour, and bullets), every text segment escaped; the
//! colours are fixed hex values from `styles` (the lighter variants on a Gelap
//! note, which is a fixed dark fill here). The title stays plain. Images are
//! never sent.
//!
//! Every string of the note is HTML-escaped. The page's own words are an
//! `id` / `en` pair, chosen by the runtime language like the shelf page's.

use crate::i18n::Lang;
use crate::theme::{Accent, Palette};
use crate::widgets::checklist;
use crate::widgets::sticky::{self, Paper};
use crate::widgets::styles::{self, Run, Style};

/// A stretch of one line in one style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

impl Span {
    pub fn plain(text: &str) -> Span {
        Span { text: text.to_owned(), style: Style::default() }
    }
}

/// One line under the title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoteLine {
    /// A plain line (possibly empty), shown as it is typed.
    Text(Vec<Span>),
    /// A checklist item (`- [ ] ` or `- [x] `): shown, never tickable.
    Item { done: bool, spans: Vec<Span> },
    /// A bullet (`\u{2022} `).
    Bullet(Vec<Span>),
}

impl NoteLine {
    /// A plain line of one unstyled span.
    pub fn text(s: &str) -> NoteLine {
        NoteLine::Text(vec![Span::plain(s)])
    }

    /// An item of one unstyled span.
    pub fn item(done: bool, s: &str) -> NoteLine {
        NoteLine::Item { done, spans: vec![Span::plain(s)] }
    }

    /// A bullet of one unstyled span.
    pub fn bullet(s: &str) -> NoteLine {
        NoteLine::Bullet(vec![Span::plain(s)])
    }
}

/// A note as it was when the owner sent it: the island makes it, the phone
/// server holds it and `render` draws it. The colours are `0xRRGGBB`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteShare {
    pub title: String,
    pub lines: Vec<NoteLine>,
    /// The paper.
    pub fill: u32,
    /// The text on it.
    pub ink: u32,
    /// The footer and the done items: a little softer than `ink`, still
    /// 4.5:1 on `fill`.
    pub soft: u32,
    /// The PC's name, for "dari <PC>".
    pub pc: String,
}

/// The fixed fill of a Gelap note on the phone, whatever surface the island
/// wears: its light text always reads on it.
pub const GELAP_FILL: u32 = 0x1B1D1F;

impl NoteShare {
    /// The snapshot of a note's `text` and formatting `runs` on `paper`. The
    /// title is the first line (a marker on it is dropped, and it stays
    /// plain); the rest follow in order, `- [ ] ` and `- [x] ` lines as items,
    /// `\u{2022} ` lines as bullets, every line cut into spans by the runs. The
    /// colours are the note's own (`sticky::tones`); Gelap is `GELAP_FILL` with
    /// the island's light text.
    pub fn from_note(
        text: &str,
        runs: &[Run],
        paper: Paper,
        island: &Palette,
        accent: Accent,
        pc: &str,
    ) -> NoteShare {
        let text = checklist::normalize(text);
        let tones = sticky::tones(paper, island, accent);
        let fill = if paper == Paper::Gelap { GELAP_FILL } else { tones.fill };
        let runs = styles::normalise(runs, &text);
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut lines: Vec<NoteLine> = checklist::lines(&text)
            .into_iter()
            .skip(1)
            .map(|l| {
                let spans = spans_of(&units, l.content_start(), l.end, &runs);
                match l.marker {
                    checklist::Marker::Item { checked } => NoteLine::Item { done: checked, spans },
                    checklist::Marker::Bullet => NoteLine::Bullet(spans),
                    checklist::Marker::None => NoteLine::Text(spans),
                }
            })
            .collect();
        while matches!(lines.last(), Some(NoteLine::Text(t)) if t.iter().all(|s| s.text.trim().is_empty())) {
            lines.pop();
        }
        NoteShare {
            title: checklist::title_of(&text),
            lines,
            fill,
            ink: tones.body,
            soft: tones.secondary,
            pc: pc.to_owned(),
        }
    }
}

/// The units `a..b` of the text cut into spans at every change of style.
fn spans_of(units: &[u16], a: usize, b: usize, runs: &[Run]) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut start = a;
    while start < b {
        let style = styles::style_at(runs, start + 1);
        let mut end = start + 1;
        while end < b && styles::style_at(runs, end + 1) == style {
            end += 1;
        }
        out.push(Span {
            text: String::from_utf16_lossy(&units[start..end]),
            style,
        });
        start = end;
    }
    out
}

/// The page's own words.
struct Words {
    code: &'static str,
    read_only: &'static str,
    from: &'static str,
    shelf: &'static str,
    untitled: &'static str,
}

fn words(lang: Lang) -> Words {
    match lang {
        Lang::Id => Words {
            code: "id",
            read_only: "Hanya baca",
            from: "dari",
            shelf: "Berkas di rak",
            untitled: "Catatan tempel",
        },
        Lang::En => Words {
            code: "en",
            read_only: "Read only",
            from: "from",
            shelf: "Shelf files",
            untitled: "Sticky note",
        },
    }
}

/// `s` safe in element text and in a double or single quoted attribute.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn hex(rgb: u32) -> String {
    format!("#{:06X}", rgb & 0xFF_FFFF)
}

const CSS: &str = "\
html{-webkit-text-size-adjust:100%}\
body{margin:0;font:17px/1.5 system-ui,-apple-system,'Segoe UI',Roboto,sans-serif}\
main{box-sizing:border-box;max-width:640px;margin:0 auto;padding:28px 20px 12px}\
h1{margin:0 0 14px;font-size:1.4rem;line-height:1.25;overflow-wrap:anywhere;white-space:pre-wrap}\
p{margin:0;min-height:1.5em;overflow-wrap:anywhere;white-space:pre-wrap}\
p.i{display:flex;gap:10px;align-items:flex-start}\
input{flex:none;width:20px;height:20px;margin:3px 0 0}\
.d{text-decoration:line-through}\
.m{flex:none;width:20px;text-align:center}\
mark{border-radius:3px;padding:0 1px}\
footer{box-sizing:border-box;max-width:640px;margin:0 auto;padding:12px 20px 32px;font-size:.875rem;display:flex;flex-wrap:wrap;gap:6px 16px;justify-content:space-between}\
a{padding:8px 0}";

/// The spans of a line as HTML: each text escaped, wrapped in `<b>`, `<i>`,
/// `<u>`, `<s>` and, inside them, `<mark>` (the highlight, always in the dark
/// ink) or a coloured `<span>`. A done item keeps its dimmed look, so it takes
/// no colour or highlight (a bold or an italic still shows).
fn push_spans(html: &mut String, spans: &[Span], dark: bool, done: bool) {
    for span in spans {
        let st = span.style;
        let (hl, fg) = if done { (None, None) } else { (st.hl, st.fg) };
        for (on, tag) in [(st.b, "b"), (st.i, "i"), (st.u, "u"), (st.x, "s")] {
            if on {
                html.push('<');
                html.push_str(tag);
                html.push('>');
            }
        }
        if let Some(h) = hl {
            html.push_str(&format!(
                "<mark style=\"background:{};color:{}\">",
                hex(styles::hl_rgb(h)),
                hex(styles::HL_INK)
            ));
        } else if let Some(f) = fg {
            html.push_str(&format!("<span style=\"color:{}\">", hex(styles::fg_rgb(f, dark))));
        }
        html.push_str(&escape(&span.text));
        if hl.is_some() {
            html.push_str("</mark>");
        } else if fg.is_some() {
            html.push_str("</span>");
        }
        for (on, tag) in [(st.x, "s"), (st.u, "u"), (st.i, "i"), (st.b, "b")] {
            if on {
                html.push_str("</");
                html.push_str(tag);
                html.push('>');
            }
        }
    }
}

/// The note's page: one complete HTML document, read only.
pub fn render(note: &NoteShare, lang: Lang) -> String {
    let w = words(lang);
    let title = if note.title.trim().is_empty() {
        w.untitled.to_owned()
    } else {
        escape(&note.title)
    };
    let dark = sticky::luminance(note.fill) < 0.2;
    let (fill, ink, soft) = (hex(note.fill), hex(note.ink), hex(note.soft));
    let mut html = String::with_capacity(1024 + note.lines.len() * 64);
    html.push_str("<!doctype html><html lang=\"");
    html.push_str(w.code);
    html.push_str("\"><head><meta charset=\"utf-8\">");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">");
    html.push_str("<title>");
    html.push_str(&title);
    html.push_str("</title><style>");
    html.push_str(CSS);
    html.push_str(&format!(
        "body{{background:{fill};color:{ink};color-scheme:{}}}\
         footer{{color:{soft}}}a{{color:{ink}}}.d{{color:{soft}}}input{{accent-color:{ink}}}",
        if dark { "dark" } else { "light" }
    ));
    html.push_str("</style></head><body><main><h1>");
    html.push_str(&title);
    html.push_str("</h1>");
    for line in &note.lines {
        match line {
            NoteLine::Text(spans) => {
                html.push_str("<p>");
                push_spans(&mut html, spans, dark, false);
                html.push_str("</p>");
            }
            NoteLine::Bullet(spans) => {
                html.push_str("<p class=\"i\"><span class=\"m\">&#8226;</span><span>");
                push_spans(&mut html, spans, dark, false);
                html.push_str("</span></p>");
            }
            NoteLine::Item { done, spans } => {
                html.push_str("<p class=\"i\"><input type=\"checkbox\" disabled");
                if *done {
                    html.push_str(" checked");
                }
                html.push_str("><span");
                if *done {
                    html.push_str(" class=\"d\"");
                }
                html.push('>');
                push_spans(&mut html, spans, dark, *done);
                html.push_str("</span></p>");
            }
        }
    }
    html.push_str("</main><footer><span>");
    html.push_str(w.read_only);
    html.push_str(" \u{b7} ");
    html.push_str(w.from);
    html.push(' ');
    html.push_str(&escape(&note.pc));
    html.push_str("</span><a href=\"/\">");
    html.push_str(w.shelf);
    html.push_str("</a></footer></body></html>");
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Surface, island};
    use crate::widgets::sticky::contrast;

    const SURFACES: [Surface; 6] = [
        Surface::Espresso,
        Surface::Graphite,
        Surface::Sea,
        Surface::Night,
        Surface::Plum,
        Surface::Terminal,
    ];

    const NASTY: &str = "<b>&amp\"'";

    fn note(title: &str, lines: Vec<NoteLine>) -> NoteShare {
        NoteShare {
            title: title.into(),
            lines,
            fill: 0xF4E7C6,
            ink: 0x2B211A,
            soft: 0x4A3F36,
            pc: "KANTOR".into(),
        }
    }

    fn snapshot(text: &str, paper: Paper) -> NoteShare {
        let isl = island(Accent::Terracotta, Surface::Espresso);
        NoteShare::from_note(text, &[], paper, &isl, Accent::Terracotta, "KANTOR")
    }

    #[test]
    fn every_special_character_of_the_title_and_the_lines_is_escaped() {
        let n = note(
            NASTY,
            vec![
                NoteLine::text(NASTY),
                NoteLine::item(false, NASTY),
                NoteLine::item(true, NASTY),
            ],
        );
        let page = render(&n, Lang::En);
        let safe = "&lt;b&gt;&amp;amp&quot;&#39;";
        // The title (h1 and <title>) and three lines.
        assert_eq!(page.matches(safe).count(), 5, "{page}");
        assert!(!page.contains("<b>"), "{page}");
        assert!(!page.contains("&amp\""), "{page}");
        assert_eq!(escape("a&b<c>d\"e'f"), "a&amp;b&lt;c&gt;d&quot;e&#39;f");
        // The PC name too.
        let mut n = n;
        n.pc = "<PC>".into();
        assert!(render(&n, Lang::En).contains("from &lt;PC&gt;"));
    }

    #[test]
    fn a_checklist_is_shown_with_disabled_boxes_and_done_items_struck_through() {
        let n = note(
            "Daftar belanja",
            vec![
                NoteLine::item(false, "Telur"),
                NoteLine::item(true, "Kopi"),
                NoteLine::text("  dua  spasi"),
            ],
        );
        let page = render(&n, Lang::Id);
        assert!(page.contains("<input type=\"checkbox\" disabled><span>Telur</span>"), "{page}");
        assert!(
            page.contains("<input type=\"checkbox\" disabled checked><span class=\"d\">Kopi</span>"),
            "{page}"
        );
        assert_eq!(page.matches("type=\"checkbox\"").count(), 2);
        assert_eq!(page.matches("disabled").count(), 2, "every box is disabled");
        assert!(page.contains(".d{text-decoration:line-through}"));
        // Whitespace is kept.
        assert!(page.contains("white-space:pre-wrap"));
        assert!(page.contains("<p>  dua  spasi</p>"));
        // In order, under the title in an h1.
        let at = |s: &str| page.find(s).unwrap_or_else(|| panic!("{s}"));
        assert!(at("<h1>Daftar belanja</h1>") < at("Telur"));
        assert!(at("Telur") < at("Kopi") && at("Kopi") < at("dua  spasi"));
    }

    #[test]
    fn the_page_is_one_small_document_with_no_script_and_no_outside_address() {
        let n = note(
            "T",
            vec![NoteLine::text("javascript:alert(1) http://evil.example")],
        );
        // The note's own words may say "http"; the page itself adds none.
        let mut plain = n.clone();
        plain.lines = vec![];
        for lang in [Lang::Id, Lang::En] {
            let page = render(&plain, lang);
            assert!(page.starts_with("<!doctype html>"));
            assert!(page.contains("<meta charset=\"utf-8\">"));
            assert!(page.contains("name=\"viewport\""));
            let lower = page.to_ascii_lowercase();
            for bad in ["<script", "http", "//", "<link", "<img", "<form", "<iframe", "url(", "@import", "onload", "onclick"] {
                assert!(!lower.contains(bad), "{bad} in {page}");
            }
            assert_eq!(page.matches("<a ").count(), 1);
            assert!(page.contains("<a href=\"/\">"));
            assert!(page.len() < 4096, "{} bytes", page.len());
        }
        // A note that quotes an address shows it as text, escaped, never a tag.
        let page = render(&n, Lang::En);
        assert!(page.contains("<p>javascript:alert(1) http://evil.example</p>"));
        assert!(!page.contains("<script"));
    }

    #[test]
    fn both_languages_have_the_footer_and_the_link() {
        let n = note("T", vec![]);
        let id = render(&n, Lang::Id);
        assert!(id.contains("<html lang=\"id\">"));
        assert!(id.contains("Hanya baca \u{b7} dari KANTOR"));
        assert!(id.contains(">Berkas di rak</a>"));
        let en = render(&n, Lang::En);
        assert!(en.contains("<html lang=\"en\">"));
        assert!(en.contains("Read only \u{b7} from KANTOR"));
        assert!(en.contains(">Shelf files</a>"));
        // An empty title gets the page's own word.
        let blank = note("  ", vec![]);
        assert!(render(&blank, Lang::Id).contains("<h1>Catatan tempel</h1>"));
        assert!(render(&blank, Lang::En).contains("<h1>Sticky note</h1>"));
    }

    #[test]
    fn the_body_is_the_paper_and_the_text_is_the_ink() {
        let n = snapshot("Judul\n- [ ] a", Paper::Sage);
        let page = render(&n, Lang::En);
        assert!(page.contains("body{background:#D3DFCF;color:#2B211A;color-scheme:light}"), "{page}");
        let g = snapshot("Judul", Paper::Gelap);
        let page = render(&g, Lang::En);
        assert!(page.contains("background:#1B1D1F;"), "{page}");
        assert!(page.contains("color-scheme:dark"), "{page}");
    }

    #[test]
    fn the_ink_reads_on_every_paper_including_gelap_on_every_surface() {
        for surface in SURFACES {
            for accent in Accent::ALL {
                let isl = island(*accent, surface);
                for paper in Paper::ALL {
                    let n = NoteShare::from_note("T\n- [x] a", &[], paper, &isl, *accent, "PC");
                    for (name, ink) in [("ink", n.ink), ("soft", n.soft)] {
                        let c = contrast(ink, n.fill);
                        assert!(c >= 4.5, "{paper:?} {name} on {surface:?}/{accent:?}: {c:.2}");
                    }
                    if paper == Paper::Gelap {
                        assert_eq!(n.fill, GELAP_FILL);
                        assert_eq!(n.ink, isl.text, "the island's light text");
                    } else {
                        assert_eq!(n.fill, sticky::tones(paper, &isl, *accent).fill);
                    }
                }
            }
        }
    }

    #[test]
    fn a_note_text_becomes_a_title_plain_lines_and_items() {
        let n = snapshot("Daftar belanja\n- [ ] Telur\n- [x] Kopi\n- [X] Susu\nbiasa\n\n", Paper::Krem);
        assert_eq!(n.title, "Daftar belanja");
        assert_eq!(
            n.lines,
            vec![
                NoteLine::item(false, "Telur"),
                NoteLine::item(true, "Kopi"),
                NoteLine::item(true, "Susu"),
                NoteLine::text("biasa"),
            ],
            "trailing blank lines are dropped"
        );
        assert_eq!(n.pc, "KANTOR");
        // CRLF is read like LF; a marker on the first line is not shown in the title.
        let n = snapshot("- [ ] Judul\r\nsatu\r\n\r\ndua", Paper::Krem);
        assert_eq!(n.title, "Judul");
        assert_eq!(
            n.lines,
            vec![
                NoteLine::text("satu"),
                NoteLine::Text(vec![]),
                NoteLine::text("dua"),
            ]
        );
        // `- [ ]` with no space after it is just text.
        let n = snapshot("T\n- [ ]\n-[ ] x", Paper::Krem);
        assert_eq!(
            n.lines,
            vec![NoteLine::text("- [ ]"), NoteLine::text("-[ ] x")]
        );
        // An empty note has an empty title and no lines.
        let n = snapshot("", Paper::Krem);
        assert!(n.title.is_empty() && n.lines.is_empty());
    }

    // ------------------------------------------------------ formatting, bullets

    fn run(s: usize, e: usize, style: Style) -> Run {
        Run { s, e, style }
    }

    fn bold() -> Style {
        Style { b: true, ..Style::default() }
    }

    fn formatted(text: &str, runs: &[Run], paper: Paper) -> NoteShare {
        let isl = island(Accent::Terracotta, Surface::Espresso);
        NoteShare::from_note(text, runs, paper, &isl, Accent::Terracotta, "KANTOR")
    }

    #[test]
    fn a_line_is_cut_into_spans_at_every_change_of_style() {
        let text = "T\nbold plain ital";
        // "bold" is 2..6, "ital" 13..17.
        let ital = Style { i: true, ..Style::default() };
        let n = formatted(text, &[run(2, 6, bold()), run(13, 17, ital)], Paper::Krem);
        assert_eq!(
            n.lines,
            vec![NoteLine::Text(vec![
                Span { text: "bold".into(), style: bold() },
                Span::plain(" plain "),
                Span { text: "ital".into(), style: ital },
            ])]
        );
        // The title is plain whatever the runs say.
        let n = formatted(text, &[run(0, 1, bold())], Paper::Krem);
        assert_eq!(n.title, "T");
        assert_eq!(n.lines, vec![NoteLine::text("bold plain ital")]);
    }

    #[test]
    fn bullets_and_items_keep_their_markers_out_of_the_spans() {
        let text = "T\n\u{2022} one\n- [x] two\n\u{2022} \u{1F600} three";
        let n = formatted(text, &[run(4, 7, bold())], Paper::Krem);
        assert_eq!(n.lines.len(), 3);
        assert_eq!(
            n.lines[0],
            NoteLine::Bullet(vec![Span { text: "one".into(), style: bold() }])
        );
        assert_eq!(n.lines[1], NoteLine::item(true, "two"));
        assert_eq!(n.lines[2], NoteLine::bullet("\u{1F600} three"), "an emoji stays whole");
        // A run over a marker never puts the marker in a span.
        let n = formatted(text, &[run(0, 100, bold())], Paper::Krem);
        assert_eq!(n.lines[0], NoteLine::Bullet(vec![Span { text: "one".into(), style: bold() }]));
    }

    #[test]
    fn every_segment_is_escaped_inside_styled_spans() {
        let st = Style { b: true, hl: Some(styles::Hl::Pink), ..Style::default() };
        let n = note(
            "T",
            vec![
                NoteLine::Text(vec![Span { text: NASTY.into(), style: st }]),
                NoteLine::Bullet(vec![Span {
                    text: NASTY.into(),
                    style: Style { fg: Some(styles::Fg::Merah), ..Style::default() },
                }]),
            ],
        );
        let page = render(&n, Lang::En);
        let safe = "&lt;b&gt;&amp;amp&quot;&#39;";
        assert_eq!(page.matches(safe).count(), 2, "the two spans: {page}");
        assert!(!page.contains("<b>&amp"), "{page}");
        assert!(!page.contains("&amp\""), "{page}");
        // The styles are tags around the escaped text.
        assert!(page.contains(&format!("<b><mark style=\"background:#F7B8CF;color:#2B211A\">{safe}</mark></b>")), "{page}");
        assert!(page.contains(&format!("<span style=\"color:#A11F18\">{safe}</span>")), "{page}");
    }

    #[test]
    fn styles_nest_in_a_fixed_order_and_close_in_reverse() {
        let all = Style {
            b: true,
            i: true,
            u: true,
            x: true,
            hl: None,
            fg: Some(styles::Fg::Biru),
        };
        let n = note("T", vec![NoteLine::Text(vec![Span { text: "x".into(), style: all }])]);
        let page = render(&n, Lang::En);
        assert!(
            page.contains("<p><b><i><u><s><span style=\"color:#1D4E9E\">x</span></s></u></i></b></p>"),
            "{page}"
        );
        // Plain text has no tag at all.
        let n = note("T", vec![NoteLine::text("plain")]);
        assert!(render(&n, Lang::En).contains("<p>plain</p>"));
    }

    #[test]
    fn a_bullet_is_a_dot_and_its_text_and_no_image_is_ever_sent() {
        let n = note("T", vec![NoteLine::bullet("satu")]);
        let page = render(&n, Lang::Id);
        assert!(page.contains("<p class=\"i\"><span class=\"m\">&#8226;</span><span>satu</span></p>"), "{page}");
        assert!(!page.to_ascii_lowercase().contains("<img"));
        // The page with every kind of formatting is still one small document.
        let st = Style { b: true, i: true, hl: Some(styles::Hl::Kuning), ..Style::default() };
        let n = note("T", vec![NoteLine::Text(vec![Span { text: "a".into(), style: st }])]);
        let page = render(&n, Lang::En);
        for bad in ["<script", "http", "//", "<link", "<img", "url(", "@import"] {
            assert!(!page.to_ascii_lowercase().contains(bad), "{bad} in {page}");
        }
    }

    #[test]
    fn a_done_item_keeps_its_dimmed_look_whatever_its_runs_say() {
        let st = Style { b: true, hl: Some(styles::Hl::Kuning), fg: Some(styles::Fg::Merah), ..Style::default() };
        let n = note(
            "T",
            vec![NoteLine::Item { done: true, spans: vec![Span { text: "x".into(), style: st }] }],
        );
        let page = render(&n, Lang::En);
        assert!(page.contains("<span class=\"d\"><b>x</b></span>"), "bold stays, colour and highlight do not: {page}");
        assert!(!page.contains("<mark"));
    }

    #[test]
    fn the_text_colours_read_on_every_phone_fill() {
        for surface in SURFACES {
            for accent in Accent::ALL {
                let isl = island(*accent, surface);
                for paper in Paper::ALL {
                    let n = NoteShare::from_note("T\nx", &[], paper, &isl, *accent, "PC");
                    let dark = n.fill == GELAP_FILL;
                    for f in styles::Fg::ALL {
                        let c = contrast(styles::fg_rgb(f, dark), n.fill);
                        assert!(c >= 4.5, "{f:?} on {paper:?} ({surface:?}/{accent:?}): {c:.2}");
                    }
                    // A highlight is its own fill with the dark ink.
                    for h in styles::Hl::ALL {
                        assert!(contrast(styles::HL_INK, styles::hl_rgb(h)) >= 4.5);
                    }
                }
            }
        }
        // The Gelap fill itself, with the light variants.
        for f in styles::Fg::ALL {
            assert!(contrast(styles::fg_rgb(f, true), GELAP_FILL) >= 4.5);
        }
    }

    #[test]
    fn the_page_picks_the_variant_of_the_fill() {
        let st = Style { fg: Some(styles::Fg::Merah), ..Style::default() };
        let mut n = note("T", vec![NoteLine::Text(vec![Span { text: "x".into(), style: st }])]);
        assert!(render(&n, Lang::En).contains("color:#A11F18"), "a pastel fill");
        n.fill = GELAP_FILL;
        assert!(render(&n, Lang::En).contains("color:#FF8A80"), "the Gelap fill");
    }
}
