from __future__ import annotations
import re

from pyjsg.parser_impl.jsg_builtinvaluetype_parser import JSGBuiltinValueType
from pyjsg.parser_impl.parser_utils import as_token
from pyjsg.parser.jsgParser import *

from pyjsg.parser.jsgParserVisitor import jsgParserVisitor
from pyjsg.parser_impl.jsg_doc_context import JSGDocContext, PythonGeneratorElement

python_template = """


class {name}({base_type}):
    pattern = {pattern}"""


class JSGLexerRuleBlock(jsgParserVisitor, PythonGeneratorElement):

    def __init__(self, context: JSGDocContext, ctx: jsgParser.lexerRuleBlock | None = None) -> None:
        self._context = context

        self._rulePattern: str = ""
        self._ruleTokens: set[str] = set()
        self._jsontype: JSGBuiltinValueType | None = None
        self.text = ""

        if ctx:
            self.text = ctx.getText()
            self.visit(ctx)

    def __str__(self):
        return f"pattern: r'{self._rulePattern}'"

    def dependency_list(self) -> list[str]:
        return list(self._ruleTokens)

    def members_entries(self, all_are_optional: bool=False) -> list[tuple[str, str]]:
        return []

    def python_type(self) -> str:
        return 'str'

    def signature_type(self) -> str:
        if self._jsontype:
            return self._jsontype.signature_type()
        if self._ruleTokens:
            return "jsg.JSGPattern(r'{}'.format({}))".\
                format(self._rulePattern, ', '.join(['{v}={v}.pattern'.format(v=v) for v in sorted(self._ruleTokens)]))
        else:
            return "jsg.JSGPattern(r'{}')".format(self._rulePattern)

    def reference_type(self) -> str:
        return self.signature_type()

    def mt_value(self) -> str:
        return "None"

    def as_python(self, name: str) -> str:
        """ Return the python representation """
        if self._ruleTokens:
            pattern = "jsg.JSGPattern(r'{}'.format({}))".\
                format(self._rulePattern, ', '.join(['{v}={v}.pattern'.format(v=v) for v in sorted(self._ruleTokens)]))
        else:
            pattern = "jsg.JSGPattern(r'{}')".format(self._rulePattern)
        base_type = self._jsontype.signature_type() if self._jsontype else "jsg.JSGString"
        return python_template.format(name=name, base_type=base_type, pattern=pattern)

    # ***************
    #   Visitors
    # ***************
    def visitBuiltinValueType(self, ctx: jsgParser.BuiltinValueTypeContext):
        self._jsontype = JSGBuiltinValueType(self._context, ctx)

    def visitLexerAltList(self, ctx: jsgParser.LexerAltListContext):
        """ lexerAltList: lexerAlt (LBAR lexerAlt)* """
        altlist = ctx.lexerAlt()
        self.visit(altlist[0])
        for alt in altlist[1:]:
            self._rulePattern += '|'
            self.visit(alt)

    def visitLexerElement(self, ctx: jsgParser.LexerElementContext):
        """ lexerElement: lexerAtom ebnfSuffix? | lexerBlock ebnfSuffix? """
        self.visitChildren(ctx)
        if ctx.ebnfSuffix():
            self._rulePattern += ctx.ebnfSuffix().getText()

    def visitLexerBlock(self, ctx: jsgParser.LexerBlockContext):
        """ lexerBlock: OPREN lexeraltList CPREN """
        self._rulePattern += '('
        self.visitChildren(ctx)
        self._rulePattern += ')'

    def visitLexerAtom(self, ctx: jsgParser.LexerAtomContext):
        """ lexerAtom : lexerTerminal | LEXER_CHAR_SET | ANY """
        if ctx.LEXER_CHAR_SET() or ctx.ANY():
            self._rulePattern += str(ctx.getText())
        else:
            self.visitChildren(ctx)

    def visitLexerTerminal(self, ctx: jsgParser.LexerTerminalContext):
        """ terminal: LEXER_ID | STRING  """
        if ctx.LEXER_ID():
            # Substitute LEXER_ID with its string equivalent - "{LEXER_ID}".format(LEXER_ID=LEXER_ID.pattern)
            idtoken = as_token(ctx)
            self._rulePattern += '({' + idtoken + '})'
            self._ruleTokens.add(idtoken)
        else:
            self.add_string(ctx.getText()[1:-1], False)

    def add_string(self, pattern: str, parens: bool) -> None:
        self._rulePattern += ('(' if parens else '') + re.escape(pattern) + (')' if parens else '')

    def add_strings(self, patterns: list[str]):
        self.add_string(patterns[0], len(patterns) > 1)
        for p in patterns[1:]:
            self._rulePattern += "|"
            self.add_string(p, True)
