import re
from functools import reduce

from ShExJSG import ShExJ
from pyjsg.jsglib import *

from pyshexc.parser_impl import generate_shexj

repl_list: list[tuple[str, str]] = [
    (r'"([0-9]+)"\^\^<http://www.w3.org/2001/XMLSchema#integer>\n?', r'\1')
]


class ShExC:
    """ Convert ShExJ into ShExC """
    def __init__(self, schema: ShExJ.Schema | str, base: str | None = None) -> None:
        """ Construct a converter

        :param schema: schema string or instance to parse
        """
        self.base = base
        if isinstance(schema, ShExJ.Schema):
            self.schema = schema
        else:
            self.schema = generate_shexj.parse(schema)

    def __str__(self) -> str:
        """ Return the stringified ShExC representation of the schema

        :return: A partially formatted representation
        """
        schema = self.tokenize()

        rval = rline = ""
        for e in schema:
            if e:
                if len(rline + e) > 60:
                    rval += rline + '\n'
                    rline = ""
                rline += " " + e
        rval += rline + '\n'

        rval = reduce(lambda r, p: re.sub(p[0], p[1], r), repl_list, rval)
        rval = rval.replace(self.base, '') if self.base is not None else rval
        if self.base is not None:
            rval = f'BASE <{self.base}>\n\n' + rval
        return rval

    def __repr__(self) -> str:
        """  Return a compact representation of the ShExC

        :return: space separated declaration
        """
        return ' '.join(e for e in self.tokenize() if e)

    def tokenize(self) -> list[str]:
        schema: list[str] = []

        schema += self.imports(self.schema.imports)
        schema += self.semActs(self.schema.startActs)
        schema += self.start(self.schema.start)
        schema += self.shapes(self.schema.shapes)
        return schema

    def implementation_error(self, tkn: Any) -> None:
        raise NotImplementedError(f"Unknown token: {type(tkn)}")

    def imports(self, imports: list[ShExJ.IRIREF] | None) -> list[str]:
        if imports is not None:
            return [f"IMPORT {self.iriref(e)}" for e in imports]
        return []

    def semActs(self, semActs: list[ShExJ.SemAct] | None) -> list[str]:
        rval = []
        if semActs is not None:
            for act in semActs:
                rval.append(f"%{self.iriref(act.name)}")
                act_code = self._escape_embedded_code(act.code).replace('%', '\\%') if act.code else None
                rval.append(f"{{{act_code}%}}" if act_code is not None else '%')
        return rval

    def start(self, start: ShExJ.shapeExpr | None) -> list[str]:
        return (["START="] + self.shapeExpr(start)) if start is not None else []

    def shapes(self, shapes: list[ShExJ.shapeExpr] | None) -> list[str]:
        rval = []
        if shapes is not None:
            for sexpr in shapes:
                rval += self.shapeExpr(sexpr)
        return rval

    def shapeExpr(self, sexpr: ShExJ.shapeExpr) -> list[str]:
        if isinstance(sexpr, ShExJ.ShapeOr):
            return self.shapeOr(sexpr)
        elif isinstance(sexpr, ShExJ.ShapeAnd):
            return self.shapeAnd(sexpr)
        elif isinstance(sexpr, ShExJ.ShapeNot):
            return self.shapeNot(sexpr)
        elif isinstance(sexpr, ShExJ.NodeConstraint):
            return self.nodeConstraint(sexpr)
        elif isinstance(sexpr, ShExJ.Shape):
            return self.shape(sexpr)
        elif isinstance_(sexpr, ShExJ.shapeExprLabel):
            return [self.shapeExprRef(sexpr)]
        elif isinstance(sexpr, ShExJ.ShapeExternal):
            return [self.shapeExternal(sexpr)]
        elif isinstance(sexpr, ShExJ.ShapeDecl):
            return self.shapeDecl(sexpr)
        else:
            self.implementation_error(sexpr)

    def shapeDecl(self, shapeDecl: ShExJ.ShapeDecl) -> list[str]:
        rval = []
        if shapeDecl.abstract:
            rval.append('ABSTRACT')
        rval .append(self.shapeExprLabel(shapeDecl.id))
        if shapeDecl.restricts:
            for lbl in shapeDecl.restricts:
                rval += ['RESTRICTS ' + self.shapeExprLabel(lbl)]
        rval += self.shapeExpr(shapeDecl.shapeExpr)
        return rval

    def binop(self, op: ShExJ.ShapeOr | ShExJ.ShapeAnd, txt: str) -> list[str]:
        rval = [self.shapeExprLabel(op.id)] + [' ('] + self.shapeExpr(op.shapeExprs[0]) + [txt]
        for e in op.shapeExprs[1:-1]:
            rval += self.shapeExpr(e) + [txt]
        rval += self.shapeExpr(op.shapeExprs[-1])
        rval += [')']
        return rval

    def shapeOr(self, shapeOr: ShExJ.ShapeOr) -> list[str]:
        return self.binop(shapeOr, 'OR')

    def shapeAnd(self, shapeAnd: ShExJ.ShapeAnd) ->list[str]:
        return self.binop(shapeAnd, 'AND')

    def shapeNot(self, shapeNot: ShExJ.ShapeNot) -> list[str]:
        return [self.shapeExprLabel(shapeNot.id)] + ['NOT ('] + self.shapeExpr(shapeNot.shapeExpr) + [')']

    def nodeConstraint(self, nc: ShExJ.NodeConstraint) -> list[str]:
        rval = [self.shapeExprLabel(nc.id)]
        if nc.nodeKind:
            rval += [str(nc.nodeKind).upper()]
        if nc.datatype:
            rval += [self.iriref(nc.datatype)]
        rval += self.xsFacet(nc)
        if nc.values is not None:
            rval.append('[')
            for e in nc.values:
                rval += self.valueSetValue(e)
            rval.append(']')
        return rval

    def shape(self, shape: ShExJ.Shape) -> list[str]:
        rval = [self.shapeExprLabel(shape.id)]
        if shape.extends is not None:
            for lbl in shape.extends:
                rval += ['EXTENDS ' + self.shapeExprLabel(lbl)]
        if shape.extra is not None:
            rval += ['EXTRA'] + [self.iriref(e) for e in shape.extra]
        if shape.closed:
            rval += ['CLOSED']
        rval += ['{'] + self.tripleExpr(shape.expression) + ['}']
        if shape.annotations:
            rval += self.annotations(shape.annotations)
        if shape.semActs:
            rval += self.semActs(shape.semActs)
        return rval

    def shapeExternal(self, se: ShExJ.ShapeExternal) -> str:
        return self.shapeExprLabel(se.id) + 'EXTERNAL'

    def tripleExpr(self, te: ShExJ.tripleExpr) -> list[str]:
        if te is None:
            return []
        elif isinstance(te, ShExJ.EachOf):
            return self.eachOf(te)
        elif isinstance(te, ShExJ.OneOf):
            return self.oneOf(te)
        elif isinstance(te, ShExJ.TripleConstraint):
            return self.tripleConstraint(te)
        elif isinstance_(te, ShExJ.tripleExprLabel):
            return ['&' + self.tripleExprLabel(te)]
        else:
            self.implementation_error(te)

    def eachOf(self, eo: ShExJ.EachOf) -> list[str]:
        return self._eachOneOf(eo, ';')

    def oneOf(self, oo: ShExJ.OneOf) -> list[str]:
        return self._eachOneOf(oo, '|')

    def _eachOneOf(self, eoo: ShExJ.EachOf | ShExJ.OneOf, sep: str) -> list[str]:
        rval = ['$' + self.tripleExprLabel(eoo.id)] if eoo.id is not None else []
        rval += ['(']
        rval += self.tripleExpr(eoo.expressions[0])
        for expr in eoo.expressions[1:]:
            rval += [sep] + self.tripleExpr(expr)
        rval += [')' + self.cardinality(eoo.min, eoo.max)]
        rval += self.annotations(eoo.annotations)
        rval += self.semActs(eoo.semActs)
        return rval

    def tripleConstraint(self, tc: ShExJ.TripleConstraint) -> list[str]:
        rval = ['$' + self.tripleExprLabel(tc.id)] if tc.id else []
        rval += [('^' if tc.inverse else '') + self.iriref(tc.predicate)] + \
            (self.shapeExpr(tc.valueExpr) if tc.valueExpr is not None else ['.'])
        rval += self.cardinality(tc.min, tc.max)
        if tc.onShapeExpression:
            rval.append("ON SHAPE EXPRESSION")
            rval += self.shapeExpr(tc.onShapeExpression)
        rval += self.annotations(tc.annotations)
        rval += self.semActs(tc.semActs)
        return rval

    def annotations(self, annotations: list[ShExJ.Annotation] | None) -> list[str]:
        if annotations:
            return ['// ' + self.iriref(annot.predicate) + self.objectValue(annot.object) for annot in annotations]
        else:
            return []

    @staticmethod
    def cardinality(min_: Integer | None, max_: Integer | None) -> str:
        minv = 1 if min_ is None else min_
        maxv = 1 if max_ is None else max_
        if minv == 0:
            if maxv == 1:
                return '?'
            elif maxv == -1:
                return '*'
        elif minv == 1:
            if maxv == -1:
                return '+'
            elif maxv == 1:
                return ""
        return f"{{{minv}}}" if minv == maxv else f"{{{minv},{maxv}}}" if maxv != -1 else f"{{{minv},*}}"

    @staticmethod
    def add_facet(facet, label: str) -> str:
        return f'{label} {facet}' if facet is not None else ''

    @staticmethod
    def add_pattern(pattern, flags) -> str:
        if pattern:
            pval = re.sub(r'/', r'\/', pattern)
            return f'/{pval}/' + (flags if flags is not None else '')
        return ''

    def xsFacet(self, nc: ShExJ.NodeConstraint) -> list[str]:
        return [self.add_facet(nc.length, 'LENGTH'),
                self.add_facet(nc.minlength, 'MINLENGTH'),
                self.add_facet(nc.maxlength, 'MAXLENGTH'),
                self.add_pattern(nc.pattern, nc.flags),
                self.add_facet(nc.mininclusive, 'MININCLUSIVE'),
                self.add_facet(nc.minexclusive, 'MINEXCLUSIVE'),
                self.add_facet(nc.maxinclusive, 'MAXINCLUSIVE'),
                self.add_facet(nc.maxexclusive, 'MAXEXCLUSIVE'),
                self.add_facet(nc.totaldigits, 'TOTALDIGITS'),
                self.add_facet(nc.fractiondigits, 'FRACTIONDIGITS')]

    def valueSetValue(self, vsv: ShExJ.valueSetValue) -> list[str]:
        if isinstance_(vsv, ShExJ.objectValue):
            return [self.objectValue(vsv)]
        elif isinstance(vsv, ShExJ.IriStem):
            return [self.iriStem(vsv)]
        elif isinstance(vsv, ShExJ.IriStemRange):
            return self.iriStemRange(vsv)
        elif isinstance(vsv, ShExJ.LiteralStem):
            return [self.literalStem(vsv)]
        elif isinstance(vsv, ShExJ.LiteralStemRange):
            return self.literalStemRange(vsv)
        elif isinstance(vsv, ShExJ.Language):
            return [self.language(vsv.languageTag)]
        elif isinstance(vsv, ShExJ.LanguageStem):
            return [self.languageStem(vsv)]
        elif isinstance(vsv, ShExJ.LanguageStemRange):
            return self.languageStemRange(vsv)
        else:
            self.implementation_error(vsv)

    def objectValue(self, v: ShExJ.objectValue) -> str:
        if isinstance(v, ShExJ.IRIREF):
            return self.iriref(v)
        elif isinstance(v, ShExJ.ObjectLiteral):
            return self.objectLiteral(v)
        else:
            self.implementation_error(v)

    def objectLiteral(self, v: ShExJ.ObjectLiteral) -> str:
        code_val = self._escape_embedded_code(v.value).\
            replace("'", "\\'").\
            replace('"', '\\"')

        return f'"{code_val}"' +\
               (f"@{v.language}" if v.language else f"^^{self.iriref(v.type)}" if v.type else "")

    def iriStem(self, v: ShExJ.IriStem) -> str:
        return self.iriref(v.stem) + '~'

    def iriStemRange(self, v: ShExJ.IriStemRange) -> [str]:
        return [('.' if isinstance(v.stem, ShExJ.Wildcard) else self.iriStem(v))] + \
               [f" - {self.iriref(e) if isinstance(e, ShExJ.IRIREF) else self.iriStem(e)}" for e in v.exclusions]

    def literalStem(self, v: ShExJ.LiteralStem) -> str:
        return self.literal(v.stem) + '~'

    @staticmethod
    def literal(v) -> str:
        return f'"{v}"'

    def literalStemRange(self, v: ShExJ.LiteralStemRange) -> [str]:
        return [('.' if isinstance(v.stem, ShExJ.Wildcard) else self.literalStem(v))] + \
               [f' - {self.literal(e) if isinstance(e, JSGString) else self.literalStem(e)}' for e in v.exclusions]

    @staticmethod
    def language(v: ShExJ.LANGTAG) -> str:
        return '@' + str(v)

    def languageStem(self, v: ShExJ.LanguageStem) -> str:
        return self.language(v.stem) + '~'

    def languageStemRange(self, v: ShExJ.LanguageStemRange) -> [str]:
        return [('.' if isinstance(v.stem, ShExJ.Wildcard) else self.languageStem(v))] + \
               [f" - {self.language(e) if isinstance(e, ShExJ.LANGTAG) else self.languageStem(e)}"
                for e in v.exclusions]

    def shapeExprLabel(self, shapeExprLabel: ShExJ.shapeExprLabel) -> str:
        return self.exprLabel(shapeExprLabel)

    def shapeExprRef(self, shapeExprRef: ShExJ.shapeExprLabel) -> str:
        # TODO: this is an issue in the JSG - the type should be shapeExprRef
        return '@' + self.exprLabel(shapeExprRef)

    def tripleExprLabel(self, tripleExprLabel: ShExJ.tripleExprLabel) -> str:
        return self.exprLabel(tripleExprLabel)

    def exprLabel(self, label: ShExJ.shapeExprLabel | ShExJ.tripleExprLabel) -> str:
        if label is None:
            return ""
        elif isinstance(label, ShExJ.IRIREF):
            return self.iriref(label)
        elif isinstance(label, ShExJ.BNODE):
            return self.bnode(label)
        else:
            self.implementation_error(label)

    @staticmethod
    def bnode(v: ShExJ.BNODE) -> str:
        return str(v)

    @staticmethod
    def iriref(v: ShExJ.IRIREF) -> str:
        return f"<{v}>"

    @staticmethod
    def _escape_embedded_code(s: str) -> str:
        rval = s.\
            encode('unicode_escape').decode().\
            replace('\\x08', '\\b')

        def _sub(o) -> str:
            c = o.group(0)
            return chr(int('0x' + c[2:], 16))

        return re.sub(r'\\x[0-9a-fA-F]+', _sub, rval, flags=re.MULTILINE + re.DOTALL + re.UNICODE)
