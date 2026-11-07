from pyjsg.parser.jsgParser import *
from pyjsg.parser.jsgParserVisitor import jsgParserVisitor
from pyjsg.parser_impl.jsg_doc_context import JSGDocContext, PythonGeneratorElement
from pyjsg.parser_impl.parser_utils import flatten_unique
import sys

version = sys.version_info
BELOW_314 = True if version < (3, 14) else False

class JSGArrayExpr(jsgParserVisitor, PythonGeneratorElement):
    def __init__(self, context: JSGDocContext, ctx: jsgParser.ArrayExprContext | None = None):
        from pyjsg.parser_impl.jsg_valuetype_parser import JSGValueType
        from pyjsg.parser_impl.jsg_ebnf_parser import JSGEbnf

        self._context = context
        self._types: list[JSGValueType] | None = None
        self._ebnf: JSGEbnf = JSGEbnf(context)
        self._ebnf.min = 0
        self._ebnf.max = None
        self.text = ""

        if ctx:
            self.text = ctx.getText()
            self.visit(ctx)

    def __str__(self):
        type_list = ' | '.join([str(t) for t in self._types])
        if len(self._types) != 1:
            type_list = f'({type_list})'
        return f"arrayExpr: [{type_list}{self._ebnf}]"

    def python_type(self) -> str:
        if BELOW_314:
            type_list = ', '.join([t.python_type() for t in self._types])
            if len(self._types) > 1:
                type_list = f'typing.Union[{type_list}]'
        else:
            type_list = ' | '.join(sorted([t.python_type() for t in self._types]))
        return f"list[{type_list}]"

    def _inner_signature(self) -> str:
        if BELOW_314:
            type_list = ', '.join([t.signature_type() for t in self._types])
            if len(self._types) > 1:
                type_list = f'typing.Union[{type_list}]'
        else:
            type_list = ' | '.join(sorted([t.signature_type() for t in self._types]))
        return type_list

    def signature_type(self) -> str:
        return f"jsg.ArrayFactory('{{name}}', _CONTEXT, {self._inner_signature()}, {self._ebnf.min}, {self._ebnf.max})"

    def reference_type(self) -> str:
        return self.signature_type()

    def mt_value(self) -> str:
        return "None"

    def members_entries(self, all_are_optional: bool | None = False) -> list[tuple[str, str]]:
        return []

    def dependency_list(self) -> list[str]:
        return flatten_unique([t.dependency_list() for t in self._types])

    # ***************
    #   Visitors
    # ***************
    def visitArrayExpr(self, ctx: jsgParser.ArrayExprContext):
        """ arrayExpr: OBRACKET valueType (BAR valueType)* ebnfSuffix? CBRACKET; """
        from pyjsg.parser_impl.jsg_ebnf_parser import JSGEbnf

        from pyjsg.parser_impl.jsg_valuetype_parser import JSGValueType
        self._types = [JSGValueType(self._context, vt) for vt in ctx.valueType()]
        if ctx.ebnfSuffix():
            self._ebnf = JSGEbnf(self._context, ctx.ebnfSuffix())
