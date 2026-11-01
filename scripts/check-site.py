"""Checks the static site in site/ (run from anywhere: python -I scripts/check-site.py).

Standard library only. Exit code 1 when any problem is found. For every
site/*.html it checks:
  - the file parses and its tags nest correctly;
  - every id is unique;
  - every internal href, src and CSS url() points at a file that exists, and
    every #fragment exists in the file it points at (index.html#features, the
    legal table-of-contents anchors, the SVG sprite ids used by <use>);
  - the shared contract in docs/memory/site-landing.md: the same inline head
    script, style.css and site.js, and the same header and footer on every page
    (apart from the index.html# prefix and aria-current);
  - every table sits in div.table-wrap[role=region][tabindex=0] with a name;
  - no third-party resource is loaded (external http(s) links are listed, not
    fetched).
It also checks that no text file under site/ contains an em dash (U+2014).
"""
import re
import sys
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit

SITE = Path(__file__).resolve().parent.parent / "site"
VOID = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"}
TEXT_SUFFIXES = {".html", ".css", ".js", ".md", ".svg", ".json", ".xml", ".webmanifest"}
EM_DASH = "—"
problems = []
externals = []


def problem(where, msg):
    problems.append(f"{where}: {msg}")


class Page(HTMLParser):
    def __init__(self, name):
        super().__init__(convert_charrefs=True)
        self.name = name
        self.ids = []
        self.refs = []  # (tag, attr, value, line)
        self.stack = []
        self.table_ok = []
        self.scripts = []
        self.in_script = False
        self.reveal = []

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        line = self.getpos()[0]
        if "id" in a:
            self.ids.append(a["id"])
        for attr in ("href", "src"):
            if a.get(attr) is not None:
                self.refs.append((tag, attr, a[attr], line))
        if tag == "img" and "alt" not in a:
            problem(self.name, f"line {line}: img without alt")
        if "reveal" in (a.get("class") or "").split() and tag not in ("h3", "figure"):
            problem(self.name, f"line {line}: .reveal on <{tag}> (only headings and screenshots)")
        if tag == "table":
            wrap = next((e for e in reversed(self.stack) if e[0] == "div"), None)
            ok = bool(wrap) and "table-wrap" in (wrap[1].get("class") or "").split() \
                and wrap[1].get("role") == "region" and wrap[1].get("tabindex") == "0" \
                and bool(wrap[1].get("aria-label") or wrap[1].get("aria-labelledby"))
            if not ok:
                problem(self.name, f"line {line}: table is not inside div.table-wrap[role=region][tabindex=0] with a name")
        if tag == "script":
            self.in_script = True
            self.scripts.append({"src": a.get("src"), "text": ""})
        if tag not in VOID:
            self.stack.append((tag, a, line))

    def handle_startendtag(self, tag, attrs):
        # <svg ... /> style self-closing tags: treat as start and end.
        self.handle_starttag(tag, attrs)
        if tag not in VOID:
            self.stack.pop()

    def handle_endtag(self, tag):
        if tag in VOID:
            return
        line = self.getpos()[0]
        if not self.stack:
            problem(self.name, f"line {line}: stray </{tag}>")
            return
        top = self.stack[-1]
        if top[0] == tag:
            self.stack.pop()
            if tag == "script":
                self.in_script = False
            return
        problem(self.name, f"line {line}: </{tag}> closes <{top[0]}> opened on line {top[2]}")
        for i in range(len(self.stack) - 1, -1, -1):
            if self.stack[i][0] == tag:
                del self.stack[i:]
                break

    def handle_data(self, data):
        if self.in_script and self.scripts:
            self.scripts[-1]["text"] += data

    def finish(self):
        for tag, a, line in self.stack:
            problem(self.name, f"line {line}: <{tag}> is never closed")


def split_ref(value):
    parts = urlsplit(value.strip())
    return parts, unquote(parts.path), parts.fragment


def is_external(value):
    v = value.strip().lower()
    return v.startswith(("http://", "https://", "//"))


def is_other_scheme(value):
    return value.strip().lower().startswith(("mailto:", "tel:", "data:", "javascript:"))


def region(text, start_marker, end_marker):
    i = text.find(start_marker)
    if i < 0:
        return None
    j = text.find(end_marker, i)
    return text[i:j + len(end_marker)] if j >= 0 else None


def normalise_shell(chunk):
    chunk = chunk.replace(' aria-current="page"', "")
    return chunk.replace('href="index.html#', 'href="#')


def main():
    if not SITE.is_dir():
        print(f"site folder not found: {SITE}")
        return 1
    pages = {}
    texts = {}
    for path in sorted(SITE.glob("*.html")):
        text = path.read_text(encoding="utf-8")
        texts[path.name] = text
        page = Page(path.name)
        try:
            page.feed(text)
            page.close()
        except Exception as exc:  # html.parser is lenient; anything raised is a real problem
            problem(path.name, f"does not parse: {exc}")
        page.finish()
        pages[path.name] = page
    if not pages:
        problem("site", "no html files found")

    for name, page in pages.items():
        seen = set()
        for i in page.ids:
            if i in seen:
                problem(name, f"duplicate id {i!r}")
            seen.add(i)

    def check_ref(name, where, value):
        if not value.strip():
            problem(name, f"{where}: empty reference")
            return
        if is_external(value):
            externals.append((name, value))
            return
        if is_other_scheme(value):
            return
        parts, rel, frag = split_ref(value)
        if parts.scheme or parts.netloc:
            problem(name, f"{where}: unsupported reference {value!r}")
            return
        target_name = name if rel == "" else rel
        target = SITE / target_name
        if rel and not (target.is_file() or (rel.endswith("/") and (target / "index.html").is_file())):
            problem(name, f"{where}: {value!r} does not exist under site/")
            return
        if frag:
            if target_name in pages:
                if frag not in pages[target_name].ids:
                    problem(name, f"{where}: {value!r} points at #{frag}, which is not an id in {target_name}")
            elif rel and target.suffix == ".html":
                problem(name, f"{where}: {value!r} points into a page that was not parsed")

    for name, page in pages.items():
        for tag, attr, value, line in page.refs:
            where = f"line {line} <{tag} {attr}>"
            check_ref(name, where, value)
            if is_external(value) and (tag in ("script", "img", "source") or (tag == "link" and attr == "href")):
                problem(name, f"{where}: loads a third-party resource {value!r}")

    for css in sorted(SITE.glob("*.css")):
        for m in re.finditer(r"url\(\s*(['\"]?)([^)'\"]+)\1\s*\)", css.read_text(encoding="utf-8")):
            value = m.group(2)
            if value.startswith("data:"):
                continue
            check_ref(css.name, "css url()", value)

    # The shared contract between the pages.
    head_scripts = {}
    for name, page in pages.items():
        inline = [s["text"].strip() for s in page.scripts if not s["src"]]
        head_scripts[name] = inline[0] if inline else ""
        srcs = [s["src"] for s in page.scripts if s["src"]]
        if "site.js" not in srcs:
            problem(name, "does not load site.js")
        links = [v for tag, attr, v, _ in page.refs if tag == "link" and attr == "href"]
        if "style.css" not in links:
            problem(name, "does not load style.css")
    if len(set(head_scripts.values())) > 1:
        problem("site", "the inline head script differs between pages: " + ", ".join(sorted(head_scripts)))
    if head_scripts and not next(iter(head_scripts.values())):
        problem("site", "no inline head script that adds the js class")

    shells = {}
    for name, text in texts.items():
        header = region(text, '<header class="site-header">', "</header>")
        footer = region(text, '<footer class="site-footer">', "</footer>")
        if header is None:
            problem(name, "no header.site-header")
        if footer is None:
            problem(name, "no footer.site-footer")
        shells[name] = (normalise_shell(header or ""), normalise_shell(footer or ""))
        if header:
            for label, frag in (("Features", "index.html#features"), ("Try the cat", "index.html#cat"), ("Install", "index.html#install")):
                if f'href="{frag}">{label}<' not in header and f'href="#{frag.split("#")[1]}">{label}<' not in header:
                    problem(name, f"header link {label!r} is not {frag}")
    if shells:
        ref_name = "index.html" if "index.html" in shells else next(iter(shells))
        for name, (h, f) in shells.items():
            if h != shells[ref_name][0]:
                problem(name, f"header differs from {ref_name}")
            if f != shells[ref_name][1]:
                problem(name, f"footer differs from {ref_name}")

    # No em dash in any text file under site/.
    for path in sorted(SITE.rglob("*")):
        if path.is_file() and path.suffix.lower() in TEXT_SUFFIXES:
            body = path.read_text(encoding="utf-8", errors="replace")
            for n, line in enumerate(body.splitlines(), 1):
                if EM_DASH in line:
                    problem(str(path.relative_to(SITE)), f"line {n}: contains an em dash (U+2014)")

    if externals:
        print("External links (listed, not fetched):")
        for name, value in sorted(set(externals)):
            print(f"  {name}: {value}")
    for p in problems:
        print("PROBLEM", p)
    print(f"{len(pages)} pages checked, {len(problems)} problems")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
