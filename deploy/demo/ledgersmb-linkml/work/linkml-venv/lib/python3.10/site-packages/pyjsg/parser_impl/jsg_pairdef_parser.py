from collections import OrderedDict

from pyjsg.parser.jsgParser import *
from pyjsg.parser.jsgParserVisitor import jsgParserVisitor
from pyjsg.parser_impl.jsg_doc_context import JSGDocContext, PythonGeneratorElement
from pyjsg.parser_impl.jsg_ebnf_parser import JSGEbnf
from pyjsg.parser_impl.jsg_valuetype_parser import JSGValueType
from pyjsg.parser_impl.parser_utils import as_token, is_valid_python, get_terminal, esc_kw, flatten, t


class JSGPairDef(jsgParserVisitor, PythonGeneratorElement):
    def __init__(self, context: JSGDocContext, ctx: jsgParser.PairDefContext | None = None):
        self._context = context

        # PairDef can be one of "name: valueType [ebnsuffix]", "(name [|] name ...): valueType [ebnsuffix]" or type_ref
        self._typ: JSGValueType | None = None       # If absent, then _type reference
        self._names: dict[str, str] = OrderedDict()    # list of names associated with _typ

        self._type_reference: str | None = None     # Reference to external type

        self._ebnf = JSGEbnf(context)                  # Cardinality of either branch
        self.text = ""

        if ctx:
            self.text = ctx.getText()
            self.visit(ctx)

    def __str__(self):
        if self._typ:
            names = list(self._names.keys())
            if len(names) > 1:
                namelist = '(' + ' | '.join(name for name in names) + ')'
            else:
                namelist = names[0]
            return f"pairDef: {namelist} : {self._typ}{self._ebnf}"
        else:
            return f"pairDef: typeReference: {self._type_reference}{self._ebnf}"

    def is_reference_type(self) -> bool:
        return self._type_reference is not None

    def members_entries(self, all_are_optional: bool | None = False) -> list[tuple[str, str]]:
        """ Generate a list quoted raw name, signature type entries for this pairdef, recursively traversing
        reference types

        :param all_are_optional: If true, all types are forced optional
        :return: raw name/ signature type for all elements in this pair
        """
        if self._type_reference:
            rval: list[tuple[str, str]] = []
            for n, t in self._context.reference(self._type_reference).members_entries(all_are_optional):
                rval.append((n, self._ebnf.signature_cardinality(t, all_are_optional).format(name=n)))
            return rval
        else:
            sig = self._ebnf.signature_cardinality(self._typ.reference_type(), all_are_optional)
            return [(name, sig.format(name=name)) for name in self._names]

    def signature_type(self) -> str:
        base_type = self._typ.signature_type() if self._typ \
            else self._context.reference(self._type_reference).signature_type()
        return self._ebnf.signature_cardinality(base_type)

    def reference_type(self) -> str:
        return self.signature_type()

    def python_base_type(self) -> str:
        return self._typ.python_type() if self._typ \
            else self._context.reference(self._type_reference).python_type()

    def python_type(self) -> str:
        return self._ebnf.python_cardinality(self.python_base_type())

    def mt_value(self) -> str:
        return self._typ.mt_value() if self._typ else self._context.reference(self._type_reference).mt_value()

    def signatures(self, all_are_optional: bool | None = False) -> list[str]:
        """ Return the __init__ signature element(s) (var: type = default value).  Note that signatures are not
        generated for non-python names, although we do take the liberty of suffixing a '_' for reserved words
        (e.g. class: @int  generates "class_: int = None"

        :param all_are_optional: If True, all items are considered to be optional

        :return: list of signatures
        """
        if self._type_reference:
            # This assumes that references are to things that have signatures
            ref = self._context.reference(self._type_reference)
            if not getattr(ref, 'signatures', None):
                raise NotImplementedError("Reference to " + self._type_reference + " is not valid")
            return self._context.reference(self._type_reference).signatures(all_are_optional)
        else:
            return [f"{self._names[rn]}: {self.python_type()} = " 
                    f"{self._ebnf.mt_value(self._typ)}" for rn, cn in self._names.items() if is_valid_python(cn)]

    def _initializer_for(self, raw_name: str, cooked_name: str, prefix: str | None) -> list[str]:
        """Create an initializer entry for the entry

        :param raw_name: name unadjusted for python compatibility.
        :param cooked_name: name that may or may not be python compatible

        :param prefix: owner of the element - used when objects passed as arguments

        :return: Initialization statements
        """
        mt_val = self._ebnf.mt_value(self._typ)
        rval = []

        if is_valid_python(raw_name):
            if prefix:
                # If a prefix exists, the input has already been processed - no if clause is necessary
                rval.append(f"self.{raw_name} = {prefix}.{raw_name}")
            else:
                cons = raw_name
                rval.append(f"self.{raw_name} = {cons}")
        elif is_valid_python(cooked_name):
            if prefix:
                rval.append(f"setattr(self, '{raw_name}', getattr({prefix}, '{raw_name}')")
            else:
                cons = f"{cooked_name} if {cooked_name} is not {mt_val} else _kwargs.get('{raw_name}', {mt_val})"
                rval.append(f"setattr(self, '{raw_name}', {cons})")
        else:
            getter = f"_kwargs.get('{raw_name}', {mt_val})"
            if prefix:
                rval.append(f"setattr(self, '{raw_name}', getattr({prefix}, '{getter}')")
            else:
                rval.append(f"setattr(self, '{raw_name}', {getter})")

        return rval

    def initializers(self, prefix: str | None = None) -> list[str]:
        """ Return the __init__ initializer assignment block """
        if self._type_reference:
            # This assumes that references are to things that have initializers
            # TODO: Remove this check once we are certian things are good
            ref = self._context.reference(self._type_reference)
            if not getattr(ref, 'signatures', None):
                raise NotImplementedError("Reference to " + self._type_reference + " is not valid")
            return self._context.reference(self._type_reference).initializers(prefix)
        else:
            return flatten([self._initializer_for(rn, cn, prefix) for rn, cn in self._names.items()])

    def dependency_list(self) -> list[str]:
        return self._typ.dependency_list() if self._typ else [self._type_reference]

    # ***************
    #   Visitors
    # ***************
    def visitPairDef(self, ctx: jsgParser.PairDefContext):
        """ pairDef: name COLON valueType ebnfSuffix? 
                     | idref ebnfSuffix?
                     | OPREN name (BAR? name)+ CPREN COLON valueType ebnfSuffix?
        """
        if ctx.name():          # Options 1 or 3
            self.visitChildren(ctx)
        else:
            self._type_reference = as_token(ctx)
            if ctx.ebnfSuffix():
                self.visit(ctx.ebnfSuffix())

    def visitName(self, ctx: jsgParser.NameContext):
        """ name: ID | STRING """
        rtkn = get_terminal(ctx)
        tkn = esc_kw(rtkn)
        self._names[rtkn] = tkn

    def visitValueType(self, ctx: jsgParser.ValueTypeContext):
        self._typ = JSGValueType(self._context, ctx)

    def visitEbnfSuffix(self, ctx: jsgParser.EbnfSuffixContext):
        self._ebnf = JSGEbnf(self._context, ctx)
