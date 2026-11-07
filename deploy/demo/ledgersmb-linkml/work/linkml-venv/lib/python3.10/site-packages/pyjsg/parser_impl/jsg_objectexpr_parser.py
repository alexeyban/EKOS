from pyjsg.parser.jsgParser import *
from pyjsg.parser.jsgParserVisitor import jsgParserVisitor
from pyjsg.parser_impl.jsg_doc_context import JSGDocContext, PythonGeneratorElement
from pyjsg.parser_impl.jsg_ebnf_parser import JSGEbnf
from pyjsg.parser_impl.jsg_lexerruleblock_parser import JSGLexerRuleBlock
from pyjsg.parser_impl.jsg_pairdef_parser import JSGPairDef
from pyjsg.parser_impl.jsg_valuetype_parser import JSGValueType
from pyjsg.parser_impl.parser_utils import t, flatten
from .parser_utils import as_token, flatten_unique, is_valid_python

_class_template = """


class {name}(jsg.JSGObject):
    _reference_types = [{reference_types}]
    _members = {{{members}}}
    _strict = {strict}

{init_fctn}"""

_map_template = """


class {name}(jsg.JSGObjectMap):{name_filter}{value_type}

    def __init__(self,
                 **_kwargs):
        super().__init__(_CONTEXT, **_kwargs)"""


_init_template = """    def __init__(self{signatures},
                 **_kwargs: dict[str, object]):
        super().__init__(_CONTEXT, **_kwargs){initializers}
"""
indent0 = ",\n                "
indent1n = "\n    "
indent1 = indent0 + " "
indent2 = "\n        "
indent3 = indent2 + "    "


class JSGObjectExpr(jsgParserVisitor, PythonGeneratorElement):
    """ objectExpr: OBRACE membersDef? CBRACE | OBRACE ID MAPSTO valueType CBRACE """
    def __init__(self, context: JSGDocContext,
                 ctx: jsgParser.ObjectExprContext | jsgParser.MembersDefContext | None = None,
                 name: str | None = None):
        self._context = context
        self._name = name
        self._strict = True

        # _members, _choices and _map_name_type are mutually exclusive
        self._members: list[JSGPairDef] = []
        self._choices: list[str] = []

        # _map is for a map style definition
        self._map_name_type: str | JSGLexerRuleBlock | None = None               # Name of a lexer rule block
        self._map_valuetype: JSGValueType | None = None
        self._map_ebnf: JSGEbnf | None = None
        self.text = ""

        if ctx:
            self.text = ctx.getText()
            self.visit(ctx)

    def __str__(self):
        return "objectExpr: {}" \
            .format("object map" if self._map_name_type else "object choices" if self._choices else "simple object")

    def map_as_python(self, name: str) -> str:
        if self._map_name_type is not None:
            if isinstance(self._map_name_type, str):
                filtr = self._map_name_type
            else:
                filtr = self._map_name_type.signature_type()
            name_filter = f"{indent1n}_name_filter = {filtr}"
        else:
            name_filter = ""
        if self._map_valuetype is not None:
            typ = self._map_valuetype.python_type()
        else:
            typ = "jsg.AnyType"
        value_type = f"{indent1n}_value_type = {typ}"
        return _map_template.format(**locals())

    def as_python(self, name: str) -> str:
        """ Return the python representation of the class represented by this object """
        if self._map_valuetype:
            return self.map_as_python(name)
        else:
            return self.obj_as_python(name)

    def obj_as_python(self, name: str) -> str:
        reference_types = self._gen_reference_types()
        members = self._gen_members_list()
        strict = self._strict
        init_fctn = self._gen_init_fctn()
        return _class_template.format(**locals())

    def _gen_reference_types(self) -> str:
        from pyjsg.jsglib import AnyType
        if self._map_valuetype:
            return self._map_valuetype if self._map_valuetype is not AnyType else ''
        elif self._choices:
            return ', '.join(r for r in self._choices)
        else:
            return ', '.join(pair.python_base_type() for pair in self._members if pair.is_reference_type())

    def _gen_members_list(self) -> str:
        if self._map_valuetype:
            return ''
        else:
            return indent0.join(f"'{mk}': {mt}" for mk, mt in self.members_entries())

    def _gen_init_fctn(self) -> str:
        pair_signatures = self.signatures()
        pair_initializers = self.initializers()
        if self._map_valuetype:
            return ''
        elif self._choices:
            signatures = indent1 + indent1.join(pair_signatures) if pair_signatures else ""
            initializers = indent2 + indent2.join(pair_initializers)
            return _init_template.format(**locals())
        else:
            signatures = indent1 + indent1.join(self.signatures()) if pair_signatures else ""
            initializers = indent2 + indent2.join(flatten(pair_initializers)) if pair_initializers else ""
            return _init_template.format(**locals())

    def python_type(self) -> str:
        return self._name

    def signature_type(self) -> str:
        return self._name

    def reference_type(self):
        return self._name

    def mt_value(self) -> str:
        return 'None'

    def signatures(self, _: bool=False) -> list[str]:
        if self._map_valuetype:
            return []
        elif self._choices:
            pair_signatures = ', '.join(self._choices)
            if len(self._choices) > 1:
                pair_signatures = f'opts_: typing.Union[{pair_signatures}] = None'
            else:
                pair_signatures = f'{self._choices[0]}: {pair_signatures} = None'
            return [pair_signatures]
        else:
            return flatten([pairdef.signatures() for pairdef in self._members])

    def initializers(self, prefix: str | None = None) -> list[str]:

        if self._map_valuetype:
            return []
        elif self._choices:
            if not prefix:
                prefix = 'opts_'
            # Overall test
            rval = [f'if {prefix} is not None:']

            # First choice option
            choice = self._choices[0]
            rval.append(f'{t(1)}if isinstance({prefix}, {choice}):')
            choice_ref = self._context.reference(choice)
            inits = choice_ref.initializers(prefix)
            for initializer in inits:
                rval.append(t(2) + initializer)
            if not inits:
                rval.append(t(2) + 'pass')

            # Second through nth choice options
            for choice in self._choices[1:]:
                if is_valid_python(choice):
                    rval.append(f'{t(1)}elif isinstance({prefix}, {choice}):')
                    choice_ref = self._context.reference(choice)
                    choice_inits = []
                    for initializer in choice_ref.initializers(prefix):
                        choice_inits.append(t(2) + initializer)
                    if not choice_inits:
                        choice_inits.append(t(2) + 'pass')
                    rval += choice_inits
                else:
                    raise NotImplementedError("Can this happen?")

            # Error checker
            rval.append(t() + 'else:')
            rval.append(t(2) + f'raise ValueError(f"Unrecognized value type: {{{prefix}}}")')
            return rval
        else:
            return flatten([pairdef.initializers(prefix) for pairdef in self._members])

    def members_entries(self, all_are_optional: bool = False) -> list[tuple[str, str]]:
        """ Return an ordered list of elements for the _members section

        :param all_are_optional: True means we're in a choice situation so everything is optional
        :return:
        """
        rval = []
        if self._members:
            for member in self._members:
                rval += member.members_entries(all_are_optional)
        elif self._choices:
            for choice in self._choices:
                rval += self._context.reference(choice).members_entries(True)
        else:
            return []
        return rval

    def dependency_list(self) -> list[str]:
        if self._members:
            return flatten_unique(member.dependency_list() for member in self._members)
        elif self._map_valuetype:
            return self._map_valuetype.dependency_list()
        else:
            return flatten_unique(self._context.dependency_list(c) + [c] for c in self._choices)

    # ***************
    #   Visitors
    # ***************
    def visitObjectExpr(self, ctx: jsgParser.ObjectExprContext):
        """ objectExpr: OBRACE membersDef? CBRACE 
                        OBRACE (LEXER_ID_REF | ANY)? MAPSTO valueType ebnfSuffix? CBRACE
        """
        if not self._name:
            self._name = self._context.anon_id()
        if ctx.membersDef():
            self.visitChildren(ctx)
        elif ctx.MAPSTO():
            if ctx.LEXER_ID_REF():
                self._map_name_type = as_token(ctx)
            # Any and absent mean the same thing
            self._map_valuetype = JSGValueType(self._context, ctx.valueType())
            if ctx.ebnfSuffix():
                self._map_ebnf = JSGEbnf(self._context, ctx.ebnfSuffix())

    def visitMembersDef(self, ctx: jsgParser.MembersDefContext):
        """ membersDef: COMMA | member+ (BAR altMemberDef)* (BAR lastComma)? ;
            altMemberDef: member* ;
            member: pairDef COMMA?
            lastComma: COMMA ;
        """
        if not self._name:
            self._name = self._context.anon_id()
        if ctx.COMMA():                             # lone comma - wild card
            self._strict = False
        if not ctx.BAR():                           # member+
            self.visitChildren(ctx)
        else:
            entry = 1
            self._add_choice(entry, ctx.member())       # add first brance (member+)
            for alt in ctx.altMemberDef():
                entry += 1
                self._add_choice(entry, alt.member())
            if ctx.lastComma():
                entry += 1
                self._add_choice(entry, [])

    def visitMember(self, ctx: jsgParser.MemberContext):
        """ member: pairDef COMMA? """
        self._strict = ctx.COMMA() is None
        self.visitChildren(ctx)

    def visitPairDef(self, ctx: jsgParser.PairDefContext):
        self._members.append(JSGPairDef(self._context, ctx))

    def _add_choice(self, branch: int, ctx: list[jsgParser.MemberContext]):
        choice_name = "{}_{}_".format(self._name, branch)
        choice_obj = JSGObjectExpr(self._context, name=choice_name)
        for member in ctx:
            choice_obj.visit(member)
        self._choices.append(choice_name)
        self._context.grammarelts[choice_name] = choice_obj
