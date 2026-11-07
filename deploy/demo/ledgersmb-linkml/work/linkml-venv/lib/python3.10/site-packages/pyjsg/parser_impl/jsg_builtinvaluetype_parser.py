from dataclasses import dataclass

from pyjsg.parser.jsgParser import *
from pyjsg.parser.jsgParserVisitor import jsgParserVisitor
from pyjsg.parser_impl.jsg_doc_context import JSGDocContext, PythonGeneratorElement


class JSGBuiltinValueType(jsgParserVisitor, PythonGeneratorElement):
    @dataclass
    class TypeInfo:
        sig_type: str
        ref_type: str
        py_type: type
        mt_value: str

    parserTypeToImplClass: dict[str, TypeInfo] = \
        {"@string": TypeInfo("jsg.String", "jsg.String", str, "None"),
         "@object": TypeInfo("jsg.ObjectFactory('{name}', _CONTEXT, jsg.Object)", "jsg.JSGObject", object, "None"),
         "@int": TypeInfo("jsg.Integer", "jsg.Integer", int, "None"),
         "@number": TypeInfo("jsg.Number", "jsg.Number", float, "None"),
         "@null": TypeInfo("jsg.JSGNull", "jsg.JSGNull", type(None), "jsg.Empty"),
         "@array": TypeInfo("jsg.ArrayFactory('{name}', _CONTEXT, jsg.AnyType, 0, None)", "jsg.JSGArray", list, "None"),
         "@bool": TypeInfo("jsg.Boolean", "jsg.Boolean", bool, "None"),
         ".": TypeInfo("jsg.AnyTypeFactory('{name}', _CONTEXT)", "jsg.AnyType", object, "jsg.Empty")}

    def __init__(self, context: JSGDocContext, ctx: jsgParser.BuiltinValueTypeContext | None = None):
        self._context = context
        self._value_type_text: str | None = None
        self._typeinfo: JSGBuiltinValueType.TypeInfo = None
        self.text = ""
        if ctx:
            self.text = ctx.getText()
            self.visit(ctx)

    def __str__(self):
        return f"builtinValueType: {self._value_type_text if self._value_type_text != '.' else 'jsg.AnyType'}"

    def python_type(self) -> str:
        return "type(None)" if self._typeinfo.py_type is type(None) else self._typeinfo.py_type.__name__

    def signature_type(self) -> str:
        return self._typeinfo.sig_type

    def reference_type(self) -> str:
        return self._typeinfo.sig_type

    def mt_value(self) -> str:
        return self._typeinfo.mt_value

    def members_entries(self, all_are_optional: bool | None = False) -> list[tuple[str, str]]:
        return []

    def dependency_list(self) -> list[str]:
        return []

    # ***************
    #   Visitors
    # ***************
    def visitBuiltinValueType(self, ctx: jsgParser.BuiltinValueTypeContext):
        """ valueTypeExpr: JSON_STRING | JSON_NUMBER | JSON_INT | JSON_BOOL | JSON_NULL | JSON_ARRAY | JSON_OBJECT """
        self._value_type_text = ctx.getText()
        self._typeinfo = self.parserTypeToImplClass[self._value_type_text]
