from typing import Optional

from pyjsg.parser.jsgParser import *
from pyjsg.parser.jsgParserVisitor import jsgParserVisitor
from pyjsg.parser_impl.jsg_doc_context import JSGDocContext, PythonGeneratorElement
from pyjsg.parser_impl.jsg_valuetype_parser import JSGValueType


class JSGEbnf(jsgParserVisitor):
    """ Cardinality processing """
    def __init__(self, context: JSGDocContext, ctx: jsgParser.EbnfSuffixContext | None = None):
        self._context = context
        self._ebnftext = ""                 # type: str
        self.min = 1                        # type: int
        self.max = 1                        # type: Optional[int]
        self.text = ""

        if ctx:
            self.text = ctx.getText()
            self.visit(ctx)

    def __str__(self):
        return self._ebnftext

    @property
    def one_optional_element(self) -> bool:
        """ Return True if exactly one optional element """
        return self.min == 0 and self.max == 1

    @property
    def multiple_elements(self) -> bool:
        """ Return True if cardinality is > 1"""
        return self.max is None or self.max > 1


    def python_cardinality(self, subject: str, all_are_optional: bool = False) -> str:
        """Add the appropriate python typing to subject (e.g. Optional, List, ...)

        :param subject: Subject to be decorated
        :param all_are_optional: Force everything to be optional
        :return: Typed subject
        """
        if self.multiple_elements:
            rval = f"list[{subject}]"
        elif self.one_optional_element:
            rval = subject if subject.startswith("typing.Optional[") else f"typing.Optional[{subject}]"
        elif self.max == 0:
            rval = "type(None)"
        else:
            rval = subject
        if all_are_optional and not self.one_optional_element:
            rval = f"typing.Optional[{rval}]"
        return rval

    def signature_cardinality(self, subject: str, all_are_optional: bool = False) -> str:
        """Add the appropriate python typing to subject (e.g. Optional, list, ...)

        :param subject: Subject to be decorated
        :param all_are_optional: Force everything to be optional
        :return: Typed subject
        """
        if self.multiple_elements:
            rval = f"jsg.ArrayFactory('{{name}}', _CONTEXT, {subject}, {self.min}, {self.max})"
        elif self.one_optional_element:
            rval = subject if subject.startswith("typing.Optional[") else f"typing.Optional[{subject}]"
        elif self.max == 0:
            rval = "type(None)"
        else:
            rval = subject
        if all_are_optional and not self.one_optional_element:
            rval = f"typing.Optional[{rval}]"
        return rval

    def mt_value(self, typ: JSGValueType) -> str:
        return "None" if self.multiple_elements else typ.mt_value()

    # ***************
    #   Visitors
    # ***************
    def visitEbnfSuffix(self, ctx: jsgParser.EbnfSuffixContext):
        """ ebnfSuffix: QMARK | STAR | PLUS | OBRACE INT (COMMA (INT|STAR)?)? CBRACE """
        self._ebnftext = ctx.getText()
        if ctx.INT():
            self.min = int(ctx.INT(0).getText())
            if ctx.COMMA():
                if len(ctx.INT()) > 1:
                    self.max = int(ctx.INT(1).getText())
                else:
                    self.max = None
            else:
                self.max = self.min
        elif ctx.QMARK():
            self.min = 0
            self.max = 1
        elif ctx.STAR():
            self.min = 0
            self.max = None
        elif ctx.PLUS():
            self.min = 1
            self.max = None
        else:
            raise NotImplementedError("Unknown ebnf construct: {}".format(self._ebnftext))
