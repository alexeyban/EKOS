"""Regex/format literals lifted verbatim from the recovered MarkdownSharp.dll (generated)."""

LINK_DEF = '\n                        ^[ ]{{0,{0}}}\\[([^\\[\\]]+)\\]:  # id = $1\n                          [ ]*\n                          \\n?                   # maybe *one* newline\n                          [ ]*\n                        <?(\\S+?)>?              # url = $2\n                          [ ]*\n                          \\n?                   # maybe one newline\n                          [ ]*\n                        (?:\n                            (?<=\\s)             # lookbehind for whitespace\n                            ["(]\n                            (.+?)               # title = $3\n                            [")]\n                            [ ]*\n                        )?                      # title is optional\n                        (?:\\n+|\\Z)'

HTML_TOKENS_0 = '\n            (<!--(?:|(?:[^>-]|-[^>])(?:[^-]|-[^-])*)-->)|        # match <!-- foo -->\n            (<\\?.*?\\?>)|                 # match <?foo?> '

HTML_TOKENS_1 = ' \n            (<[A-Za-z\\/!$](?:[^<>]|'

HTML_TOKENS_2 = ' \n            (<[A-Za-z\\/!$](?:[^<>]'

HTML_TOKENS_3 = ')*>)'

HTML_TOKENS_4 = ' # match <tag> and </tag>'

ANCHOR_REF = '\n            (                               # wrap whole match in $1\n                \\[\n                    ({0})                   # link text = $2\n                \\]\n\n                [ ]?                        # one optional space\n                (?:\\n[ ]*)?                 # one optional newline followed by spaces\n\n                \\[\n                    (.*?)                   # id = $3\n                \\]\n            )'

ANCHOR_INLINE = '\n                (                           # wrap whole match in $1\n                    \\[\n                        ({0})               # link text = $2\n                    \\]\n                    \\(                      # literal paren\n                        [ ]*\n                        ({1})               # href = $3\n                        [ ]*\n                        (                   # $4\n                        ([\'"])           # quote char = $5\n                        (.*?)               # title = $6\n                        \\5                  # matching quote\n                        [ ]*                # ignore any spaces between closing quote and )\n                        )?                  # title is optional\n                    \\)\n                )'

ANCHOR_REF_SHORTCUT = "\n            (                               # wrap whole match in $1\n              \\[\n                 ([^\\[\\]]+)                 # link text = $2; can't contain [ or ]\n              \\]\n            )"

IMAGES_REF = '\n                    (               # wrap whole match in $1\n                    !\\[\n                        (.*?)       # alt text = $2\n                    \\]\n\n                    [ ]?            # one optional space\n                    (?:\\n[ ]*)?     # one optional newline followed by spaces\n\n                    \\[\n                        (.*?)       # id = $3\n                    \\]\n\n                    )'

IMAGES_INLINE = '\n              (                     # wrap whole match in $1\n                !\\[\n                    (.*?)           # alt text = $2\n                \\]\n                \\s?                 # one optional whitespace character\n                \\(                  # literal paren\n                    [ ]*\n                    ({0})           # href = $3\n                    [ ]*\n                    (               # $4\n                    ([\'"])       # quote char = $5\n                    (.*?)           # title = $6\n                    \\5              # matching quote\n                    [ ]*\n                    )?              # title is optional\n                \\)\n              )'

HEADER_SETEXT = "\n                ^(.+?)\n                [ ]*\n                \\n\n                (=+|-+)     # $1 = string of ='s or -'s\n                [ ]*\n                \\n+"

HEADER_ATX = "\n                ^(\\#{1,6})  # $1 = string of #'s\n                [ ]*\n                (.+?)       # $2 = Header text\n                [ ]*\n                \\#*         # optional closing #'s (not counted)\n                \\n+"

HORIZONTAL_RULES = '\n            ^[ ]{0,3}         # Leading space\n                ([-*_])       # $1: First marker\n                (?>           # Repeated marker group\n                    [ ]{0,2}  # Zero, one, or two spaces.\n                    \\1        # Marker character\n                ){2,}         # Group repeated at least twice\n                [ ]*          # Trailing spaces\n                $             # End of line.\n            '

WHOLE_LIST = '\n            (                               # $1 = whole list\n              (                             # $2\n                [ ]{{0,{1}}}\n                ({0})                       # $3 = first list item marker\n                [ ]+\n              )\n              (?s:.+?)\n              (                             # $4\n                  \\z\n                |\n                  \\n{{2,}}\n                  (?=\\S)\n                  (?!                       # Negative lookahead for another list item marker\n                    [ ]*\n                    {0}[ ]+\n                  )\n              )\n            )'

LIST_TOP_LEVEL_PREFIX = '(?:(?<=\\n\\n)|\\A\\n?)'

CODE_BLOCK = '\n                    (?:\\n\\n|\\A\\n?)\n                    (                        # $1 = the code block -- one or more lines, starting with a space\n                    (?:\n                        (?:[ ]{{{0}}})       # Lines must start with a tab-width of spaces\n                        .*\\n+\n                    )+\n                    )\n                    ((?=^[ ]{{0,{0}}}[^ \\t\\n])|\\Z) # Lookahead for non-space at line-start, or end of doc'

CODE_SPAN = "\n                    (?<![\\\\`])   # Character before opening ` can't be a backslash or backtick\n                    (`+)      # $1 = Opening run of `\n                    (?!`)     # and no more backticks -- match the full run\n                    (.+?)     # $2 = The code block\n                    (?<!`)\n                    \\1\n                    (?!`)"

BOLD = '(\\*\\*|__) (?=\\S) (.+?[*_]*) (?<=\\S) \\1'

SEMI_STRICT_BOLD = '(?=.[*_]|[*_])(^|(?=\\W__|(?!\\*)[\\W_]\\*\\*|\\w\\*\\*\\w).)(\\*\\*|__)(?!\\2)(?=\\S)((?:|.*?(?!\\2).)(?=\\S_|\\w|\\S\\*\\*(?:[\\W_]|$)).)(?=__(?:\\W|$)|\\*\\*(?:[^*]|$))\\2'

STRICT_BOLD = '(^|[\\W_])(?:(?!\\1)|(?=^))(\\*|_)\\2(?=\\S)(.*?\\S)\\2\\2(?!\\2)(?=[\\W_]|$)'

ITALIC = '(\\*|_) (?=\\S) (.+?) (?<=\\S) \\1'

SEMI_STRICT_ITALIC = '(?=.[*_]|[*_])(^|(?=\\W_|(?!\\*)(?:[\\W_]\\*|\\D\\*(?=\\w)\\D)).)(\\*|_)(?!\\2\\2\\2)(?=\\S)((?:(?!\\2).)*?(?=[^\\s_]_|(?=\\w)\\D\\*\\D|[^\\s*]\\*(?:[\\W_]|$)).)(?=_(?:\\W|$)|\\*(?:[^*]|$))\\2'

STRICT_ITALIC = '(^|[\\W_])(?:(?!\\1)|(?=^))(\\*|_)(?=\\S)((?:(?!\\2).)*?\\S)\\2(?!\\2)(?=[\\W_]|$)'

BLOCKQUOTE = "\n            (                           # Wrap whole match in $1\n                (\n                ^[ ]*>[ ]?              # '>' at the start of a line\n                    .+\\n                # rest of the first line\n                (.+\\n)*                 # subsequent consecutive lines\n                \\n*                     # blanks\n                )+\n            )"

AUTOLINK_BARE = '(<|=")?\\b(https?|ftp)(://[-A-Z0-9+&@#/%?=~_|\\[\\]\\(\\)!:,\\.;\x1a]*[-A-Z0-9+&@#/%=~_|\\[\\])])(?=$|\\W)'

END_CHAR = '[-A-Z0-9+&@#/%=~_|\\[\\])]'

LINK_EMAIL = '<\n                      (?:mailto:)?\n                      (\n                        [-.\\w]+\n                        \\@\n                        [-a-z0-9]+(\\.[-a-z0-9]+)*\\.[a-z]+\n                      )\n                      >'

NESTED_BRACKETS_A = '\n                    (?>              # Atomic matching\n                       [^\\[\\]]+      # Anything other than brackets\n                     |\n                       \\[\n                           '

NESTED_BRACKETS_B = ' \\]\n                    )*'

NESTED_PARENS_A = '\n                    (?>              # Atomic matching\n                       [^()\\s]+      # Anything other than parens or whitespace\n                     |\n                       \\(\n                           '

NESTED_PARENS_B = ' \\)\n                    )*'

BLOCK_CONTENT_A = '\n                (?>\n                  [^<]+\t\t\t        # content without tag\n                |\n                  <\\2\t\t\t        # nested opening tag\n                    \n            (?>\t\t\t\t            # optional tag attributes\n              \\s\t\t\t            # starts with whitespace\n              (?>\n                [^>"/]+\t            # text outside quotes\n              |\n                /+(?!>)\t\t            # slash not followed by >\n              |\n                "[^"]*"\t\t        # text inside double quotes (tolerate >)\n              |\n                \'[^\']*\'\t                # text inside single quotes (tolerate >)\n              )*\n            )?\t\n                   # attributes\n                  (?>\n                      />\n                  |\n                      >'

BLOCK_CONTENT_B = '\n                      </\\2\\s*>\t        # closing nested tag\n                  )\n                  |\t\t\t\t\n                  <(?!/\\2\\s*>           # other tags with a different name\n                  )\n                )*'

BLOCK_MAIN = '\n            (?>\n                  (?>\n                    (?<=\\n)     # Starting at the beginning of a line\n                    |           # or\n                    \\A\\n?       # the beginning of the doc\n                  )\n                  (             # save in $1\n\n                    # Match from `\\n<tag>` to `</tag>\\n`, handling nested tags \n                    # in between.\n                      \n                        <($block_tags_b_re)   # start tag = $2\n                        $attr>                # attributes followed by > and \\n\n                        $content              # content, support nesting\n                        </\\2>                 # the matching end tag\n                        [ ]*                  # trailing spaces\n                        (?=\\n+|\\Z)            # followed by a newline or end of document\n\n                  | # Special version for tags of group a.\n\n                        <($block_tags_a_re)   # start tag = $3\n                        $attr>[ ]*\\n          # attributes followed by >\n                        $content2             # content, support nesting\n                        </\\3>                 # the matching end tag\n                        [ ]*                  # trailing spaces\n                        (?=\\n+|\\Z)            # followed by a newline or end of document\n                      \n                  | # Special case just for <hr />. It was easier to make a special \n                    # case than to make the other regex more complicated.\n                  \n                        [ ]{0,$less_than_tab}\n                        <hr\n                        $attr                 # attributes\n                        /?>                   # the matching end tag\n                        [ ]*\n                        (?=\\n{2,}|\\Z)         # followed by a blank line or end of document\n                  \n                  | # Special case for standalone HTML comments:\n                  \n                      (?<=\\n\\n|\\A)            # preceded by a blank line or start of document\n                      [ ]{0,$less_than_tab}\n                      (?s:\n                        <!--(?:|(?:[^>-]|-[^>])(?:[^-]|-[^-])*)-->\n                      )\n                      [ ]*\n                      (?=\\n{2,}|\\Z)            # followed by a blank line or end of document\n                  \n                  | # PHP and ASP-style processor instructions (<? and <%)\n                  \n                      [ ]{0,$less_than_tab}\n                      (?s:\n                        <([?%])                # $4\n                        .*?\n                        \\4>\n                      )\n                      [ ]*\n                      (?=\\n{2,}|\\Z)            # followed by a blank line or end of document\n                      \n                  )\n            )'

BLOCK_ATTR = '\n            (?>\t\t\t\t            # optional tag attributes\n              \\s\t\t\t            # starts with whitespace\n              (?>\n                [^>"/]+\t            # text outside quotes\n              |\n                /+(?!>)\t\t            # slash not followed by >\n              |\n                "[^"]*"\t\t        # text inside double quotes (tolerate >)\n              |\n                \'[^\']*\'\t                # text inside single quotes (tolerate >)\n              )*\n            )?\t\n            '

LIST_ITEM = '(^[ ]*)                    # leading whitespace = $1\n                ({0}) [ ]+                 # list marker = $2\n                ((?s:.+?)                  # list item text = $3\n                (\\n+))      \n                (?= (\\z | \\1 ({0}) [ ]+))'

