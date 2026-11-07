from typing import cast

from pyjsg.parser.jsgParser import *
from pyjsg.parser.jsgParserVisitor import jsgParserVisitor
from pyjsg.parser_impl.jsg_arrayexpr_parser import JSGArrayExpr
from pyjsg.parser_impl.jsg_builtinvaluetype_parser import JSGBuiltinValueType
from pyjsg.parser_impl.jsg_doc_context import JSGDocContext, PythonGeneratorElement
from pyjsg.parser_impl.parser_utils import as_token, flatten_unique
import sys

version = sys.version_info
BELOW_314 = True if version < (3, 14) else False


class JSGValueType(jsgParserVisitor, PythonGeneratorElement):
    """ Parser for valueType production """

    def __init__(self, context: JSGDocContext,
                 ctx: jsgParser.ValueTypeContext | jsgParser.ValueTypeMacroContext | jsgParser.NonRefValueTypeContext |
                      None = None):
        self._context = context

        # With the exception of lexeridref and alttypelist, all options below are mutually exclusive
        self._typeid: str | None = None              # The name of a referenced (possibly anonymous) type
        self._arrayDef: JSGArrayExpr | None = None   # Type is an array
        self._lexeridref: str | None = None          # The name of a (possibly anonymous) string match pattern
        self._builtintype: JSGBuiltinValueType | None = None  # Type is a builtin type
        self._alttypelist: list[JSGValueType] = []      # Two or more alternative types
        self.text = ""

        if ctx:
            self.text = ctx.getText()
            self.visit(ctx)

    def __str__(self):
        if self._typeid:
            if self._context.is_anon(self._typeid):
                typ = f"(anonymous: {self._typeid}): {self._context.reference(self._typeid)}"
            else:
                typ = f"ID: {self._typeid}"
        elif self._builtintype:
            typ = str(self._builtintype)
        elif self._alttypelist:
            lid_str = f"({self._lexerid_str()}) | " if self._lexeridref else ""
            alts_str = ' | '.join(ta.signature_type() for ta in self._alttypelist)
            typ = f"({lid_str}{alts_str})"
        elif self._lexeridref:
            typ = self._lexerid_str()
        elif self._arrayDef:
            typ = str(self._arrayDef)
        else:
            raise NotImplementedError("Unknown Value Type")
        return "valueType: {}".format(typ)

    def _lexerid_str(self) -> str:
        if not self._context.is_anon(self._lexeridref):
            return "LEXER_ID_REF: {}".format(self._lexeridref)
        else:
            return "STRING: {}".format(self._context.grammarelts[self._lexeridref])

    def python_type(self) -> str:
        """ Return the official python type for the value. As an example, an '@int' aps to 'int' or a match pattern
        maps to 'str'
        """
        types = []
        if self._lexeridref:
            types.append('str')
        if self._typeid:
            types.append(self._context.python_type(self._typeid))
        if self._builtintype:
            types.append(self._builtintype.python_type())
        if self._alttypelist:
            types += [e.python_type() for e in self._alttypelist]
        if self._arrayDef:
            types.append(self._arrayDef.python_type())
        if BELOW_314:
            return "jsg.AnyType" if len(types) == 0 else \
                types[0] if len(types) == 1 else "typing.Union[{}]".format(', '.join(types))
        else:
            if len(types) == 0:
                return "jsg.AnyType"
            if len(set(types)) == 1:
                return types[0]
            # We do this just to ensure an order for easier testing.
            types = set(types)
            return " | ".join(sorted(list(types)))

    def signature_type(self) -> str:
        """ Return the signature type for the value. As an example, an '@int' maps to 'Integer' or a match pattern
        maps to the name of the JSGString class """
        types = []
        if self._lexeridref:
            types.append(self._lexeridref)
        if self._typeid:
            types.append(self._context.signature_type(self._typeid))
        if self._builtintype:
            types.append(self._builtintype.signature_type())
        if self._alttypelist:
            types += [e.signature_type() for e in self._alttypelist]
        if self._arrayDef:
            types.append(self._arrayDef.signature_type())
        return "jsg.AnyType" if len(types) == 0 else \
            types[0] if len(types) == 1 else "typing.Union[{}]".format(', '.join(types))

    def reference_type(self) -> str:
        types = []
        if self._lexeridref:
            types.append(self._lexeridref)
        if self._typeid:
            types.append(self._context.reference_type(self._typeid))
        if self._builtintype:
            types.append(self._builtintype.reference_type())
        if self._alttypelist:
            types += [e.reference_type() for e in self._alttypelist]
        if self._arrayDef:
            types.append(self._arrayDef.reference_type())
        return "jsg.AnyType" if len(types) == 0 else \
            types[0] if len(types) == 1 else "typing.Union[{}]".format(', '.join(types))


    def mt_value(self) -> str:
        return self._builtintype.mt_value() if self._builtintype else "None"


    def dependency_list(self) -> list[str]:
        rval = []
        if self._typeid:
            rval.append(self._typeid)
            rval += self._context.dependency_list(self._typeid)
        if self._lexeridref:
            rval.append(self._lexeridref)
        if self._arrayDef:
            rval += flatten_unique((self._arrayDef.dependency_list()))
        if self._alttypelist:
            rval += flatten_unique([e.dependency_list() for e in self._alttypelist])
        return rval

    def members_entries(self, all_are_optional: bool | None = False) -> list[tuple[str, str]]:
        return []

    # ***************
    #   Visitors
    # ***************
    def visitValueType(self, ctx: jsgParser.ValueTypeContext):
        """ valueType: idref | nonRefValueType """
        if ctx.idref():
            self._typeid = as_token(ctx)
        else:
            self.visitChildren(ctx)

    def visitNonRefValueType(self, ctx: jsgParser.NonRefValueTypeContext):
        """ nonRefValueType: LEXER_ID_REF | STRING | builtinValueType | objectExpr | arrayExpr  
                             | OPREN typeAlternatives CPREN | ANY """
        if ctx.LEXER_ID_REF():                  # Reference to a lexer token
            self._lexeridref = as_token(ctx)
        elif ctx.STRING():                      # Anonymous lexer token
            from pyjsg.parser_impl.jsg_lexerruleblock_parser import JSGLexerRuleBlock
            lrb = JSGLexerRuleBlock(self._context)
            lrb.add_string(ctx.getText()[1:-1], False)
            self._lexeridref = self._context.anon_id()
            self._context.grammarelts[self._lexeridref] = lrb
        else:
            self.visitChildren(ctx)

    def visitBuiltinValueType(self, ctx: jsgParser.BuiltinValueTypeContext):
        from pyjsg.parser_impl.jsg_builtinvaluetype_parser import JSGBuiltinValueType
        self._builtintype = JSGBuiltinValueType(self._context, ctx)

    def visitObjectExpr(self, ctx: jsgParser.ObjectExprContext):
        from pyjsg.parser_impl.jsg_objectexpr_parser import JSGObjectExpr
        oe = JSGObjectExpr(self._context, ctx)
        self._typeid = oe.python_type()
        self._context.grammarelts[self._typeid] = oe

    def visitArrayExpr(self, ctx: jsgParser.ArrayExprContext):
        from pyjsg.parser_impl.jsg_arrayexpr_parser import JSGArrayExpr
        self._arrayDef = JSGArrayExpr(self._context, ctx)

    def visitValueTypeMacro(self, ctx: jsgParser.ValueTypeMacroContext):
        # valueTypeMacro: ID EQUALS nonRefValueType (BAR nonRefValueType)* SEMI;
        if len(ctx.nonRefValueType()) > 1:
            self._proc_value_types(ctx.nonRefValueType())
        else:
            self.visit(ctx.nonRefValueType(0))

    def visitTypeAlternatives(self, ctx: jsgParser.TypeAlternativesContext):
        self._proc_value_types(ctx.valueType())

    def _proc_value_types(self,
                          ctx: list[jsgParser.ValueTypeContext] | list[jsgParser.NonRefValueTypeContext]):
        from pyjsg.parser_impl.jsg_lexerruleblock_parser import JSGLexerRuleBlock

        stringalts = []                 # Aggregate multiple strings into a single type
        for vt in ctx:
            nrvt = vt.nonRefValueType() if isinstance(vt, jsgParser.ValueTypeContext) \
                else cast(jsgParser.NonRefValueTypeContext, vt)
            stringval = nrvt.STRING() if nrvt else None
            if stringval:
                stringalts.append(stringval.getText()[1:-1])
            else:
                self._alttypelist.append(JSGValueType(self._context, vt))

        if stringalts:
            lrb = JSGLexerRuleBlock(self._context)
            lrb.add_strings(stringalts)
            self._lexeridref = self._context.anon_id()
            self._context.grammarelts[self._lexeridref] = lrb
