#!/usr/bin/env python3
"""Emit the string literals of a recovered method body as Python constants.

The spec (ekos render output) carries every regex and format string byte-for-byte as a C# literal.
Copying them by machine avoids transcription typos in 3,000 characters of verbose-mode regex;
the logic around them is written by hand from the recovered statements.
"""
import re, sys

LIT = re.compile(r'"((?:[^"\\]|\\.)*)"')
ESC = {"n": "\n", "t": "\t", "r": "\r", '"': '"', "'": "'", "\\": "\\", "0": "\0"}

def unescape(s):
    out, i = [], 0
    while i < len(s):
        c = s[i]
        if c != "\\":
            out.append(c); i += 1; continue
        n = s[i + 1]
        if n == "u":
            out.append(chr(int(s[i + 2:i + 6], 16))); i += 6
        else:
            out.append(ESC[n]); i += 2
    return "".join(out)

def literals(line):
    return [unescape(m) for m in LIT.findall(line)]

def find(spec_lines, needle, nth=0):
    hits = [l for l in spec_lines if needle in l]
    return literals(hits[nth])

if __name__ == "__main__":
    spec = open(sys.argv[1], encoding="utf-8").read().split("\n")
    def lit(needle, nth=0, idx=0):
        return find(spec, needle, nth)[idx]
    C = {}
    C["LINK_DEF"] = lit("_linkDef = new")
    C["HTML_TOKENS_0"] = lit("t0[0] = ")
    C["HTML_TOKENS_1"] = lit('t0[1] = MarkdownSharp.Markdown.RepeatString(" \\n', 0, 0)
    C["HTML_TOKENS_2"] = lit("t0[2] = ")
    C["HTML_TOKENS_3"] = lit('t0[3] = ')
    C["HTML_TOKENS_4"] = lit("t0[4] = ")
    C["ANCHOR_REF"] = lit("_anchorRef = new")
    C["ANCHOR_INLINE"] = lit("_anchorInline = new")
    C["ANCHOR_REF_SHORTCUT"] = lit("_anchorRefShortcut = new")
    C["IMAGES_REF"] = lit("_imagesRef = new")
    C["IMAGES_INLINE"] = lit("_imagesInline = new")
    C["HEADER_SETEXT"] = lit("_headerSetext = new")
    C["HEADER_ATX"] = lit("_headerAtx = new")
    C["HORIZONTAL_RULES"] = lit("_horizontalRules = new")
    C["WHOLE_LIST"] = lit("_wholeList = string.Format")
    C["LIST_TOP_LEVEL_PREFIX"] = lit("_listTopLevel = new")
    C["CODE_BLOCK"] = lit("_codeBlock = new")
    C["CODE_SPAN"] = lit("_codeSpan = new")
    C["BOLD"] = lit("_bold = new")
    C["SEMI_STRICT_BOLD"] = lit("_semiStrictBold = new")
    C["STRICT_BOLD"] = lit("_strictBold = new")
    C["ITALIC"] = lit("_italic = new")
    C["SEMI_STRICT_ITALIC"] = lit("_semiStrictItalic = new")
    C["STRICT_ITALIC"] = lit("_strictItalic = new")
    C["BLOCKQUOTE"] = lit("_blockquote = new")
    C["AUTOLINK_BARE"] = lit("_autolinkBare = new")
    C["END_CHAR"] = lit("_endCharRegex = new")
    C["LINK_EMAIL"] = lit("_linkEmail = new")
    C["NESTED_BRACKETS_A"] = lit("_nestedBracketsPattern = MarkdownSharp.Markdown.RepeatString", 0, 0)
    C["NESTED_BRACKETS_B"] = lit("_nestedBracketsPattern = MarkdownSharp.Markdown.RepeatString", 0, 1)
    C["NESTED_PARENS_A"] = lit("_nestedParensPattern = MarkdownSharp.Markdown.RepeatString", 0, 0)
    C["NESTED_PARENS_B"] = lit("_nestedParensPattern = MarkdownSharp.Markdown.RepeatString", 0, 1)
    C["BLOCK_CONTENT_A"] = lit("loc0 = MarkdownSharp.Markdown.RepeatString", 0, 0)
    C["BLOCK_CONTENT_B"] = lit("loc0 = MarkdownSharp.Markdown.RepeatString", 0, 2)
    C["BLOCK_MAIN"] = lit("loc2 = \"\\n            (?>")
    C["BLOCK_ATTR"] = lit('loc2 = loc2.Replace("$attr"', 0, 1)
    C["LIST_ITEM"] = lit("loc1 = string.Format(\"(^[ ]*)")
    print('"""Regex/format literals lifted verbatim from the recovered MarkdownSharp.dll (generated)."""\n')
    for k, v in C.items():
        print(f"{k} = {v!r}\n")
