"""Python port of MarkdownSharp 2.0.5 (net40), rewritten from what EKOS recovered from the compiled assembly.

Every method below cites the IL method token it was written from. The regex and format literals live in
`_literals.py`, lifted byte-for-byte from the recovered constants.

Regex dialect: .NET and Python differ in four ways that matter here, all handled in `_rx`:
  * .NET `\\z` (absolute end) is Python `\\Z`; .NET `\\Z` (end, or before a final newline) is `(?=\\n?\\Z)`.
  * .NET allows variable-width lookbehind (`(?<=\\n\\n|\\A)`); Python does not.
  * `RegexOptions` are an integer in the IL (42 = Multiline | IgnorePatternWhitespace | Compiled).
  * Atomic groups `(?>...)` need Python 3.11+.
"""

from __future__ import annotations

import random
import re
import zlib

from . import _literals as L

NEST_DEPTH = 6
TAB_WIDTH = 4

# RegexOptions bits seen in the IL
IGNORE_CASE, MULTILINE, EXPLICIT_CAPTURE, COMPILED, SINGLELINE, IGNORE_WS = 1, 2, 4, 8, 16, 32


def _rx(pattern: str, options: int = 0) -> re.Pattern:
    flags = 0
    if options & IGNORE_CASE:
        flags |= re.I
    if options & MULTILINE:
        flags |= re.M
    if options & SINGLELINE:
        flags |= re.S
    if options & IGNORE_WS:
        flags |= re.X
    pattern = pattern.replace("\\z", "\x00Z")
    pattern = pattern.replace("\\Z", "(?=\\n?\\Z)")
    pattern = pattern.replace("\x00Z", "\\Z")
    pattern = pattern.replace("(?<=\\n\\n|\\A)", "(?:(?<=\\n\\n)|\\A)")
    return re.compile(pattern, flags)


def _g(m: re.Match, i: int) -> str:
    """.NET Groups[i].Value: an unmatched group is the empty string, not None."""
    return m.group(i) or ""


def repeat_string(text: str, count: int) -> str:  # 0x0600005B
    return text * count


def _get_nested_brackets_pattern() -> str:  # 0x06000025
    return repeat_string(L.NESTED_BRACKETS_A, NEST_DEPTH) + repeat_string(L.NESTED_BRACKETS_B, NEST_DEPTH)


def _get_nested_parens_pattern() -> str:  # 0x06000026
    return repeat_string(L.NESTED_PARENS_A, NEST_DEPTH) + repeat_string(L.NESTED_PARENS_B, NEST_DEPTH)


_NESTED_BRACKETS = _get_nested_brackets_pattern()
_NESTED_PARENS = _get_nested_parens_pattern()


def _get_block_pattern() -> str:  # 0x06000029
    content = repeat_string(L.BLOCK_CONTENT_A, NEST_DEPTH) + ".*?" + repeat_string(L.BLOCK_CONTENT_B, NEST_DEPTH)
    content2 = content.replace("\\2", "\\3")
    p = L.BLOCK_MAIN
    p = p.replace("$less_than_tab", str(TAB_WIDTH - 1))
    p = p.replace("$block_tags_b_re", "p|div|h[1-6]|blockquote|pre|table|dl|ol|ul|address|script|noscript|form|fieldset|iframe|math")
    p = p.replace("$block_tags_a_re", "ins|del")
    p = p.replace("$attr", L.BLOCK_ATTR)
    p = p.replace("$content2", content2)
    return p.replace("$content", content)


def _get_hash_key(s: str, is_html_block: bool) -> str:  # 0x0600002C
    # The original uses String.GetHashCode(), which is randomized per process on .NET Core; the key only has to
    # be stable within one Transform call and never reaches the output, so a fixed hash is a faithful stand-in.
    marker = "H" if is_html_block else "E"
    return "\x1a" + marker + str(zlib.crc32(s.encode("utf-8"))) + marker


# ---- static tables (Markdown..cctor, 0x0600001D) ------------------------------------------------------------
_NEWLINES_LEADING_TRAILING = _rx("^\\n+|\\n+\\z", COMPILED)
_NEWLINES_MULTIPLE = _rx("\\n{2,}", COMPILED)
_LEADING_WHITESPACE = _rx("^[ ]*", COMPILED)
_HTML_BLOCK_HASH = _rx("\x1aH\\d+H", COMPILED)
_LINK_DEF = _rx(L.LINK_DEF.format(TAB_WIDTH - 1), 42)
_BLOCKS_HTML = _rx(_get_block_pattern(), 34)
_HTML_TOKENS = _rx(
    L.HTML_TOKENS_0 + repeat_string(L.HTML_TOKENS_1, 5) + L.HTML_TOKENS_2 + repeat_string(L.HTML_TOKENS_3, 6) + L.HTML_TOKENS_4,
    62,
)
_ANCHOR_REF = _rx(L.ANCHOR_REF.format(_NESTED_BRACKETS), 40)
_ANCHOR_INLINE = _rx(L.ANCHOR_INLINE.format(_NESTED_BRACKETS, _NESTED_PARENS), 40)
_ANCHOR_REF_SHORTCUT = _rx(L.ANCHOR_REF_SHORTCUT, 40)
_IMAGES_REF = _rx(L.IMAGES_REF, 40)
_IMAGES_INLINE = _rx(L.IMAGES_INLINE.format(_NESTED_PARENS), 40)
_HEADER_SETEXT = _rx(L.HEADER_SETEXT, 42)
_HEADER_ATX = _rx(L.HEADER_ATX, 42)
_HORIZONTAL_RULES = _rx(L.HORIZONTAL_RULES, 42)

_MARKER_UL = "[*+-]"
_MARKER_OL = "\\d+[.]"
_MARKER_ANY = "(?:{0}|{1})".format(_MARKER_UL, _MARKER_OL)
_WHOLE_LIST = L.WHOLE_LIST.format(_MARKER_ANY, TAB_WIDTH - 1)
_LIST_NESTED = _rx("^" + _WHOLE_LIST, 42)
_LIST_TOP_LEVEL = _rx(L.LIST_TOP_LEVEL_PREFIX + _WHOLE_LIST, 42)
_CODE_BLOCK = _rx(L.CODE_BLOCK.format(TAB_WIDTH), 42)
_CODE_SPAN = _rx(L.CODE_SPAN, 56)
_BOLD = _rx(L.BOLD, 56)
_SEMI_STRICT_BOLD = _rx(L.SEMI_STRICT_BOLD, 24)
_STRICT_BOLD = _rx(L.STRICT_BOLD, 24)
_ITALIC = _rx(L.ITALIC, 56)
_SEMI_STRICT_ITALIC = _rx(L.SEMI_STRICT_ITALIC, 24)
_STRICT_ITALIC = _rx(L.STRICT_ITALIC, 24)
_BLOCKQUOTE = _rx(L.BLOCKQUOTE, 42)
_AUTOLINK_BARE = _rx(L.AUTOLINK_BARE, 9)
_END_CHAR = _rx(L.END_CHAR, 9)
_LINK_EMAIL = _rx(L.LINK_EMAIL, 33)
_OUTDENT = _rx("^[ ]{1," + str(TAB_WIDTH) + "}", 10)
_CODE_ENCODER = _rx("&|<|>|\\\\|\\*|_|\\{|\\}|\\[|\\]", COMPILED)
_AMPS = _rx("&(?!((#[0-9]+)|(#[xX][a-fA-F0-9]+)|([a-zA-Z][a-zA-Z0-9]*));)", 12)
_ANGLES = _rx("<(?![A-Za-z/?\\$!])", 12)
_UNESCAPES = _rx("\x1aE\\d+E", COMPILED)

_ESCAPE_TABLE: dict[str, str] = {}
_INVERTED_ESCAPE_TABLE: dict[str, str] = {}
_BACKSLASH_ESCAPE_TABLE: dict[str, str] = {}
_pieces = []
for _c in "\\`*_{}[]()>#+-.!/:":
    _k = _get_hash_key(_c, False)
    _ESCAPE_TABLE[_c] = _k
    _INVERTED_ESCAPE_TABLE[_k] = _c
    _BACKSLASH_ESCAPE_TABLE["\\" + _c] = _k
    _pieces.append(re.escape("\\" + _c))
_BACKSLASH_ESCAPES = re.compile("|".join(_pieces))


# ---- pure string helpers --------------------------------------------------------------------------------------
def _attribute_encode(s: str) -> str:  # 0x06000057
    return s.replace(">", "&gt;").replace("<", "&lt;").replace('"', "&quot;").replace("'", "&#39;")


def _attribute_safe_url(s: str) -> str:  # 0x06000058
    s = _attribute_encode(s)
    for c in "*_:()[]":
        s = s.replace(c, _ESCAPE_TABLE[c])
    return s


def _escape_bold_italic(s: str) -> str:  # 0x06000056
    return s.replace("*", _ESCAPE_TABLE["*"]).replace("_", _ESCAPE_TABLE["_"])


def _save_from_auto_linking(s: str) -> str:  # 0x0600002F
    return s.replace("://", "\x1aP")


def _outdent(block: str) -> str:  # 0x0600004D
    return _OUTDENT.sub("", block)


def _encode_amps_and_angles(s: str) -> str:  # 0x06000051
    s = _AMPS.sub("&amp;", s)
    return _ANGLES.sub("&lt;", s)


def _encode_code_evaluator(m: re.Match) -> str:  # 0x06000050
    v = m.group(0)
    if v == "&":
        return "&amp;"
    if v == "<":
        return "&lt;"
    if v == ">":
        return "&gt;"
    return _ESCAPE_TABLE[v]


def _encode_code(code: str) -> str:  # 0x0600004F
    return _CODE_ENCODER.sub(_encode_code_evaluator, code)


def _encode_email_address(addr: str) -> str:  # 0x0600004E
    # System.Random in the original: every call picks entities differently. Kept random on purpose.
    out = []
    rnd = random.Random()
    for ch in addr:
        r = rnd.randint(1, 99)
        if (r > 90 or ch == ":") and ch != "@":
            out.append(ch)
        elif r >= 45:
            out.append(f"&#{ord(ch)};")
        else:
            out.append(f"&#x{ord(ch):x};")
    return "".join(out)


class Markdown:
    """MarkdownSharp.Markdown (0x02000004)."""

    def __init__(
        self,
        auto_hyperlink: bool = False,
        auto_newlines: bool = False,
        empty_element_suffix: str = " />",
        link_emails: bool = True,
        strict_bold_italic: bool = False,
        asterisk_intra_word_emphasis: bool = False,
    ) -> None:  # 0x0600000E / 0x06000010 (options object) — the app.config path of 0x0600000F is not ported
        self.empty_element_suffix = empty_element_suffix
        self.link_emails = link_emails
        self.auto_hyperlink = auto_hyperlink
        self.auto_newlines = auto_newlines
        self.strict_bold_italic = strict_bold_italic
        self.asterisk_intra_word_emphasis = asterisk_intra_word_emphasis
        self._urls: dict[str, str] = {}
        self._titles: dict[str, str] = {}
        self._html_blocks: dict[str, str] = {}
        self._list_level = 0

    version = "1.13"  # 0x0600001E

    # -- entry point ----------------------------------------------------------------------------------------
    def transform(self, text: str | None) -> str:  # 0x0600001F
        if not text:
            return ""
        self._setup()
        text = self._normalize(text)
        text = self._hash_html_blocks(text)
        text = self._strip_link_definitions(text)
        text = self._run_block_gamut(text, True, True)
        text = self._unescape(text)
        self._cleanup()
        return text + "\n"

    def _setup(self) -> None:  # 0x06000023
        self._urls.clear()
        self._titles.clear()
        self._html_blocks.clear()
        self._list_level = 0

    def _cleanup(self) -> None:  # 0x06000024
        self._setup()

    # -- gamuts -----------------------------------------------------------------------------------------------
    def _run_block_gamut(self, text: str, unhash: bool, create_paragraphs: bool) -> str:  # 0x06000020
        text = self._do_headers(text)
        text = self._do_horizontal_rules(text)
        text = self._do_lists(text)
        text = self._do_code_blocks(text)
        text = self._do_block_quotes(text)
        text = self._hash_html_blocks(text)
        return self._form_paragraphs(text, unhash, create_paragraphs)

    def _run_span_gamut(self, text: str) -> str:  # 0x06000021
        text = self._do_code_spans(text)
        text = self._escape_special_chars_within_tag_attributes(text)
        text = self._escape_backslashes(text)
        text = self._do_images(text)
        text = self._do_anchors(text)
        text = self._do_auto_links(text)
        text = text.replace("\x1aP", "://")
        text = _encode_amps_and_angles(text)
        text = self._do_italics_and_bold(text)
        return self._do_hard_breaks(text)

    # -- normalize / paragraphs (both recovered at control_flow fidelity: read against the IL, not the pseudo-code) --
    def _normalize(self, text: str) -> str:  # 0x0600005A
        output: list[str] = []
        line: list[str] = []
        has_text = False
        n = len(text)
        for i, c in enumerate(text):
            if c == "\t":
                line.append(" " * (TAB_WIDTH - len(line) % TAB_WIDTH))
            elif c == "\n" or (c == "\r" and i < n - 1 and text[i + 1] != "\n"):
                if has_text:
                    output.append("".join(line))
                output.append("\n")
                line.clear()
                has_text = False
            elif c == "\r" or c == "\x1a":
                pass
            else:
                if not has_text and c != " ":
                    has_text = True
                line.append(c)
        if has_text:
            output.append("".join(line))
        output.append("\n")
        return "".join(output) + "\n\n"

    def _form_paragraphs(self, text: str, unhash: bool, create_paragraphs: bool) -> str:  # 0x06000022
        grafs = _NEWLINES_MULTIPLE.split(_NEWLINES_LEADING_TRAILING.sub("", text))
        for i, g in enumerate(grafs):
            if "\x1aH" not in g:
                spanned = self._run_span_gamut(g)
                grafs[i] = _LEADING_WHITESPACE.sub("<p>" if create_paragraphs else "", spanned) + ("</p>" if create_paragraphs else "")
            elif unhash:
                keep_going, budget = True, 50
                while keep_going and budget > 0:
                    keep_going = False

                    def ev(m: re.Match) -> str:  # <FormParagraphs>b__0, 0x0600005E
                        nonlocal keep_going
                        keep_going = True
                        return self._html_blocks[m.group(0)]

                    grafs[i] = _HTML_BLOCK_HASH.sub(ev, grafs[i])
                    budget -= 1
        return "\n\n".join(grafs)

    # -- html blocks / link definitions ---------------------------------------------------------------------
    def _hash_html_blocks(self, text: str) -> str:  # 0x0600002A
        return _BLOCKS_HTML.sub(self._html_evaluator, text)

    def _html_evaluator(self, m: re.Match) -> str:  # 0x0600002B
        block = m.group(1)
        key = _get_hash_key(block, True)
        self._html_blocks[key] = block
        return "\n\n" + key + "\n\n"

    def _strip_link_definitions(self, text: str) -> str:  # 0x06000027
        return _LINK_DEF.sub(self._link_evaluator, text)

    def _link_evaluator(self, m: re.Match) -> str:  # 0x06000028
        link_id = _g(m, 1).lower()
        self._urls[link_id] = _encode_amps_and_angles(_g(m, 2))
        if len(_g(m, 3)) > 0:
            self._titles[link_id] = _g(m, 3).replace('"', "&quot;")
        return ""

    # -- spans ----------------------------------------------------------------------------------------------
    def _tokenize_html(self, text: str) -> list[tuple[str, str]]:  # 0x0600002D
        tokens: list[tuple[str, str]] = []
        pos = 0
        for m in _HTML_TOKENS.finditer(text):
            if pos < m.start():
                tokens.append(("text", text[pos : m.start()]))
            tokens.append(("tag", m.group(0)))
            pos = m.end()
        if pos < len(text):
            tokens.append(("text", text[pos:]))
        return tokens

    def _escape_special_chars_within_tag_attributes(self, text: str) -> str:  # 0x06000059
        out = []
        for kind, value in self._tokenize_html(text):
            if kind == "tag":
                value = value.replace("\\", _ESCAPE_TABLE["\\"])
                if self.auto_hyperlink and value.startswith("<!"):
                    value = value.replace("/", _ESCAPE_TABLE["/"])
                value = re.sub("(?<=.)</?code>(?=.)", lambda _m: _ESCAPE_TABLE["`"], value)
                value = _escape_bold_italic(value)
            out.append(value)
        return "".join(out)

    def _escape_backslashes(self, s: str) -> str:  # 0x06000052
        return _BACKSLASH_ESCAPES.sub(self._escape_backslashes_evaluator, s)

    @staticmethod
    def _escape_backslashes_evaluator(m: re.Match) -> str:  # 0x06000053
        return _BACKSLASH_ESCAPE_TABLE[m.group(0)]

    def _unescape(self, s: str) -> str:  # 0x06000054
        return _UNESCAPES.sub(self._unescape_evaluator, s)

    @staticmethod
    def _unescape_evaluator(m: re.Match) -> str:  # 0x06000055
        return _INVERTED_ESCAPE_TABLE[m.group(0)]

    def _do_code_spans(self, text: str) -> str:  # 0x06000041
        return _CODE_SPAN.sub(self._code_span_evaluator, text)

    def _code_span_evaluator(self, m: re.Match) -> str:  # 0x06000042
        span = _g(m, 2)
        span = re.sub("^[ ]*", "", span)
        span = re.sub("[ ]*$", "", span)
        span = _encode_code(span)
        span = _save_from_auto_linking(span)
        return "<code>" + span + "</code>"

    def _do_images(self, text: str) -> str:  # 0x06000033
        if "![" not in text:
            return text
        text = _IMAGES_REF.sub(self._image_reference_evaluator, text)
        return _IMAGES_INLINE.sub(self._image_inline_evaluator, text)

    def _image_reference_evaluator(self, m: re.Match) -> str:  # 0x06000035
        whole, alt = _g(m, 1), _g(m, 2)
        link_id = _g(m, 3).lower()
        if link_id == "":
            link_id = alt.lower()
        if link_id not in self._urls:
            return whole
        return self._image_tag(self._urls[link_id], alt, self._titles.get(link_id))

    def _image_inline_evaluator(self, m: re.Match) -> str:  # 0x06000036
        alt, url, title = _g(m, 2), _g(m, 3), _g(m, 6)
        if url.startswith("<") and url.endswith(">"):
            url = url[1:-1]
        return self._image_tag(url, alt, title)

    def _escape_image_alt_text(self, s: str) -> str:  # 0x06000034 / lambda 0x06000061
        s = _escape_bold_italic(s)
        return re.sub(r"[\[\]()]", lambda m: _ESCAPE_TABLE[m.group(0)], s)

    def _image_tag(self, url: str, alt: str, title: str | None) -> str:  # 0x06000037
        alt = self._escape_image_alt_text(_attribute_encode(alt))
        url = _attribute_safe_url(url)
        result = f'<img src="{url}" alt="{alt}"'
        if title:
            title = _attribute_encode(_escape_bold_italic(title))
            result += f' title="{title}"'
        return result + self.empty_element_suffix

    def _do_anchors(self, text: str) -> str:  # 0x0600002E
        if "[" not in text:
            return text
        text = _ANCHOR_REF.sub(self._anchor_ref_evaluator, text)
        text = _ANCHOR_INLINE.sub(self._anchor_inline_evaluator, text)
        return _ANCHOR_REF_SHORTCUT.sub(self._anchor_ref_shortcut_evaluator, text)

    def _anchor_ref_evaluator(self, m: re.Match) -> str:  # 0x06000030
        whole = _g(m, 1)
        link_text = _save_from_auto_linking(_g(m, 2))
        link_id = _g(m, 3).lower()
        if link_id == "":
            link_id = link_text.lower()
        if link_id not in self._urls:
            return whole
        url = _attribute_safe_url(self._urls[link_id])
        result = f'<a href="{url}"'
        if link_id in self._titles:
            title = _attribute_encode(self._titles[link_id])
            title = _attribute_encode(_escape_bold_italic(title))
            result += f' title="{title}"'
        return result + ">" + link_text + "</a>"

    def _anchor_ref_shortcut_evaluator(self, m: re.Match) -> str:  # 0x06000031
        whole = _g(m, 1)
        link_text = _save_from_auto_linking(_g(m, 2))
        link_id = re.sub(r"[ ]*\n[ ]*", " ", link_text.lower())
        if link_id not in self._urls:
            return whole
        url = _attribute_safe_url(self._urls[link_id])
        result = f'<a href="{url}"'
        if link_id in self._titles:
            title = _escape_bold_italic(_attribute_encode(self._titles[link_id]))
            result += f' title="{title}"'
        return result + ">" + link_text + "</a>"

    def _anchor_inline_evaluator(self, m: re.Match) -> str:  # 0x06000032
        link_text = _save_from_auto_linking(_g(m, 2))
        url, title = _g(m, 3), _g(m, 6)
        if url.startswith("<") and url.endswith(">"):
            url = url[1:-1]
        url = _attribute_safe_url(url)
        result = f'<a href="{url}"'
        if title:
            title = _escape_bold_italic(_attribute_encode(title))
            result += f' title="{title}"'
        return result + f">{link_text}</a>"

    def _do_auto_links(self, text: str) -> str:  # 0x0600004A
        if self.auto_hyperlink:
            text = _AUTOLINK_BARE.sub(self._handle_trailing_parens, text)
        text = re.sub(r"<((https?|ftp):[^'\">\s]+)>", self._hyperlink_evaluator, text)
        if self.link_emails:
            text = _LINK_EMAIL.sub(self._email_evaluator, text)
        return text

    @staticmethod
    def _handle_trailing_parens(m: re.Match) -> str:  # 0x06000048
        if m.group(1) is not None:
            return m.group(0)
        protocol, link = _g(m, 2), _g(m, 3)
        if not link.endswith(")"):
            return "<" + protocol + link + ">"
        level = 0
        for c in re.finditer(r"[()]", link):
            if c.group(0) == "(":
                level = level + 1 if level > 0 else 1
            else:
                level -= 1
        tail = ""
        if level < 0:

            def cut(t: re.Match) -> str:  # <HandleTrailingParens>b__0, 0x06000065
                nonlocal tail
                tail = t.group(0)
                return ""

            link = re.sub(r"\){1," + str(-level) + r"}$", cut, link)
        if len(tail) > 0:
            last = link[-1]
            if not _END_CHAR.search(last):
                tail = last + tail
                link = link[:-1]
        return "<" + protocol + link + ">" + tail

    @staticmethod
    def _hyperlink_evaluator(m: re.Match) -> str:  # 0x0600004B
        url = _g(m, 1)
        return '<a href="{0}">{1}</a>'.format(_attribute_safe_url(url), url)

    def _email_evaluator(self, m: re.Match) -> str:  # 0x0600004C
        addr = self._unescape(_g(m, 1))
        addr = "mailto:" + addr
        addr = _encode_email_address(addr)
        addr = f'<a href="{addr}">{addr}</a>'
        return re.sub('">.+?:', '">', addr)

    def _do_italics_and_bold(self, text: str) -> str:  # 0x06000043
        if "*" in text or "_" in text:
            if not self.strict_bold_italic:
                text = _BOLD.sub(lambda m: f"<strong>{_g(m, 2)}</strong>", text)
                text = _ITALIC.sub(lambda m: f"<em>{_g(m, 2)}</em>", text)
            elif not self.asterisk_intra_word_emphasis:
                text = _STRICT_BOLD.sub(lambda m: f"{_g(m, 1)}<strong>{_g(m, 3)}</strong>", text)
                text = _STRICT_ITALIC.sub(lambda m: f"{_g(m, 1)}<em>{_g(m, 3)}</em>", text)
            else:
                text = _SEMI_STRICT_BOLD.sub(lambda m: f"{_g(m, 1)}<strong>{_g(m, 3)}</strong>", text)
                text = _SEMI_STRICT_ITALIC.sub(lambda m: f"{_g(m, 1)}<em>{_g(m, 3)}</em>", text)
        return text

    def _do_hard_breaks(self, text: str) -> str:  # 0x06000044
        br = "<br" + self.empty_element_suffix + "\n"
        if not self.auto_newlines:
            return re.sub(r" {2,}\n", lambda _m: br, text)
        return re.sub(r"\n", lambda _m: br, text)

    # -- blocks ---------------------------------------------------------------------------------------------
    def _do_headers(self, text: str) -> str:  # 0x06000038
        text = _HEADER_SETEXT.sub(self._setext_header_evaluator, text)
        return _HEADER_ATX.sub(self._atx_header_evaluator, text)

    def _setext_header_evaluator(self, m: re.Match) -> str:  # 0x06000039
        level = 1 if _g(m, 2).startswith("=") else 2
        return f"<h{level}>{self._run_span_gamut(_g(m, 1))}</h{level}>\n\n"

    def _atx_header_evaluator(self, m: re.Match) -> str:  # 0x0600003A
        level = len(_g(m, 1))
        return f"<h{level}>{self._run_span_gamut(_g(m, 2))}</h{level}>\n\n"

    def _do_horizontal_rules(self, text: str) -> str:  # 0x0600003B
        hr = "<hr" + self.empty_element_suffix + "\n"
        return _HORIZONTAL_RULES.sub(lambda _m: hr, text)

    def _do_lists(self, text: str) -> str:  # 0x0600003C
        pattern = _LIST_TOP_LEVEL if self._list_level <= 0 else _LIST_NESTED
        return pattern.sub(self._list_evaluator, text)

    def _list_evaluator(self, m: re.Match) -> str:  # 0x0600003D
        lst, marker = _g(m, 1), _g(m, 3)
        list_type = "ul" if re.search("[*+-]", marker) else "ol"
        start = ""
        if list_type == "ol":
            try:
                n = int(marker[:-1])
            except ValueError:
                n = 0
            if n != 1 and n != 0:
                start = f' start="{n}"'
        result = self._process_list_items(lst, _MARKER_UL if list_type == "ul" else _MARKER_OL)
        return f"<{list_type}{start}>\n{result}</{list_type}>\n"

    def _process_list_items(self, lst: str, marker: str) -> str:  # 0x0600003E
        self._list_level += 1
        lst = re.sub("\\n{2,}\\Z", "\n", lst)
        item_pattern = _rx(L.LIST_ITEM.format(marker), 34)
        last_item_had_double_newline = False

        def item(m: re.Match) -> str:  # <ProcessListItems>g__ListItemEvaluator|0, 0x06000063
            nonlocal last_item_had_double_newline
            text = _g(m, 3)
            ends_double = text.endswith("\n\n")
            loose = (ends_double or "\n\n" in text) or last_item_had_double_newline
            text = self._run_block_gamut(_outdent(text) + "\n", False, loose)
            last_item_had_double_newline = ends_double
            return f"<li>{text}</li>\n"

        lst = item_pattern.sub(item, lst)
        self._list_level -= 1
        return lst

    def _do_code_blocks(self, text: str) -> str:  # 0x0600003F
        return _CODE_BLOCK.sub(self._code_block_evaluator, text)

    def _code_block_evaluator(self, m: re.Match) -> str:  # 0x06000040
        block = _encode_code(_outdent(_g(m, 1)))
        block = _NEWLINES_LEADING_TRAILING.sub("", block)
        return "\n\n<pre><code>" + block + "\n</code></pre>\n\n"

    def _do_block_quotes(self, text: str) -> str:  # 0x06000045
        return _BLOCKQUOTE.sub(self._block_quote_evaluator, text)

    def _block_quote_evaluator(self, m: re.Match) -> str:  # 0x06000046
        bq = _g(m, 1)
        bq = re.sub("^[ ]*>[ ]?", "", bq, flags=re.M)
        bq = re.sub("^[ ]+$", "", bq, flags=re.M)
        bq = self._run_block_gamut(bq, True, True)
        bq = re.sub("^", "  ", bq, flags=re.M)
        bq = re.sub(r"(\s*<pre>.+?</pre>)", self._block_quote_evaluator2, bq, flags=re.X | re.S)
        bq = f"<blockquote>\n{bq}\n</blockquote>"
        key = _get_hash_key(bq, True)
        self._html_blocks[key] = bq
        return "\n\n" + key + "\n\n"

    @staticmethod
    def _block_quote_evaluator2(m: re.Match) -> str:  # 0x06000047
        return re.sub("^  ", "", _g(m, 1), flags=re.M)
