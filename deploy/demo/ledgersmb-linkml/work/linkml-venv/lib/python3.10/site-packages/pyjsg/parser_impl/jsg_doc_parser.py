import datetime

from pyjsg.parser.jsgParser import *
from pyjsg.parser_impl import __version__
from pyjsg.parser_impl.jsg_arrayexpr_parser import JSGArrayExpr
from pyjsg.parser_impl.jsg_lexerruleblock_parser import JSGLexerRuleBlock
from pyjsg.parser_impl.jsg_objectexpr_parser import JSGObjectExpr
from pyjsg.parser_impl.jsg_valuetype_parser import JSGValueType

from pyjsg.parser.jsgParserVisitor import jsgParserVisitor
from pyjsg.parser_impl.jsg_doc_context import JSGDocContext, JSGForwardRef
from .parser_utils import as_token, as_tokens

# Outermost python template
#
_jsg_python_template = '''{header}import typing
import pyjsg.jsglib as jsg
{original_shex}
# .TYPE and .IGNORE settings
_CONTEXT = jsg.JSGContext(){body}

_CONTEXT.NAMESPACE = locals()
'''

# Header is used by template if requested
_jsg_python_header = '''# Auto generated from {infile} by PyJSG version {version}
# Generation date: {gendate}
#
'''


class JSGDocParser(jsgParserVisitor):
    def __init__(self, context: JSGDocContext | None = None):
        jsgParserVisitor.__init__(self)
        self._context = JSGDocContext() if context is None else context
        self.text: str = ""

    def as_python(self, infile, include_original_shex: bool=False, emit_header: bool=True):
        """ Return the python representation of the document """
        self._context.resolve_circular_references()            # add forwards for any circular entries
        body = ''
        for k in self._context.ordered_elements():
            v = self._context.grammarelts[k]
            if isinstance(v, (JSGLexerRuleBlock, JSGObjectExpr)):
                body += v.as_python(k)
                if isinstance(v, JSGObjectExpr) and not self._context.has_typeid:
                    self._context.directives.append(f'_CONTEXT.TYPE_EXCEPTIONS.append("{k}")')
            elif isinstance(v, JSGForwardRef):
                pass
            elif isinstance(v, (JSGValueType, JSGArrayExpr)):
                body += f"\n\n\n{k} = {v.signature_type()}"
            else:
                raise NotImplementedError("Unknown grammar elt for {}".format(k))
            self._context.forward_refs.pop(k, None)

        body = '\n' + '\n'.join(self._context.directives) + body
        header = _jsg_python_header.format(infile=infile,
                                           version=__version__,
                                           gendate=datetime.datetime.now().strftime("%Y-%m-%d %H:%M")) \
            if emit_header else ''
        return _jsg_python_template.format(header=header,
                                           original_shex='# ' + self.text if include_original_shex else "",
                                           body=body)

    def undefined_tokens(self) -> list[str]:
        """
        Return a list of undefined tokens
        :return:
        """
        return sorted(self._context.undefined_entries())

    # ****************************
    # Directives
    # ****************************
    def visitDoc(self, ctx: jsgParser.DocContext):
        self.text = ctx.getText()
        self.visitChildren(ctx)

    def visitTypeDirective(self, ctx: jsgParser.TypeDirectiveContext):
        """ directive: '.TYPE' name typeExceptions? SEMI """
        self._context.directives.append('_CONTEXT.TYPE = "{}"'.format(as_token(ctx.name())))
        self._context.has_typeid = True
        self.visitChildren(ctx)

    def visitTypeExceptions(self, ctx: jsgParser.TypeExceptionsContext):
        """ typeExceptions: DASH idref+ """
        for tkn in as_tokens(ctx.idref()):
            self._context.directives.append('_CONTEXT.TYPE_EXCEPTIONS.append("{}")'.format(tkn))

    def visitIgnoreDirective(self, ctx: jsgParser.IgnoreDirectiveContext):
        """ directive: '.IGNORE' name* SEMI """
        for name in as_tokens(ctx.name()):
            self._context.directives.append('_CONTEXT.IGNORE.append("{}")'.format(name))

    # ****************************
    # JSON object definition
    # ****************************
    def visitObjectDef(self, ctx: jsgParser.ObjectDefContext):
        """ objectDef: ID objectExpr """
        name = as_token(ctx)
        self._context.grammarelts[name] = JSGObjectExpr(self._context, ctx.objectExpr(), name)

    # ****************************
    # JSON array definition
    # ****************************
    def visitArrayDef(self, ctx: jsgParser.ArrayDefContext):
        """ arrayDef : ID arrayExpr """
        self._context.grammarelts[as_token(ctx)] = JSGArrayExpr(self._context, ctx.arrayExpr())

    # ************************
    # Macro that represents an abstract object
    # ************************
    def visitObjectMacro(self, ctx: jsgParser.ObjectExprContext):
        """ objectMacro : ID EQUALS membersDef SEMI """
        name = as_token(ctx)
        self._context.grammarelts[name] = JSGObjectExpr(self._context, ctx.membersDef(), name)

    # ************************
    # Macro that represents an abstract value type
    # ************************
    def visitValueTypeMacro(self, ctx: jsgParser.ValueTypeMacroContext):
        """ valueTypeMacro : ID EQUALS nonRefValueType (BAR nonRefValueType)* SEMI """
        self._context.grammarelts[as_token(ctx)] = JSGValueType(self._context, ctx)

    # ************************
    # Lexer Rule
    # ************************
    def visitLexerRuleSpec(self, ctx: jsgParser.LexerRuleSpecContext):
        """ lexerRuleSpec: LEXER_ID COLON lexerRuleBlock SEMI """
        self._context.grammarelts[as_token(ctx)] = JSGLexerRuleBlock(self._context, ctx.lexerRuleBlock())
