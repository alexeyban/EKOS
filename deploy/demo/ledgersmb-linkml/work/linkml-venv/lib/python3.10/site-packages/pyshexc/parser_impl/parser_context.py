import re
from ShExJSG import ShExJ
from pyjsg.jsglib import *
from rdflib import RDF, XSD

from pyshexc.parser.ShExDocParser import ShExDocParser

IRIstr = str
PREFIXstr = str

RDF_TYPE = ShExJ.IRIREF(str(RDF.type))

RDF_INTEGER_TYPE = ShExJ.IRIREF(str(XSD.integer))
RDF_DOUBLE_TYPE = ShExJ.IRIREF(str(XSD.double))
RDF_DECIMAL_TYPE = ShExJ.IRIREF(str(XSD.decimal))
RDF_BOOL_TYPE = ShExJ.IRIREF(str(XSD.boolean))


class ParserContext:
    """
    Context maintained across ShExC parser implementation modules
    """
    def __init__(self):
        self.schema = ShExJ.Schema()
        self.ld_prefixes: dict[PREFIXstr, IRIstr] = {}       # prefixes in the JSON-LD module
        self.prefixes: dict[PREFIXstr, IRIstr] = {}          # Assigned prefixes
        self.base: IRIstr | None = None

    def _lookup_prefix(self, prefix: PREFIXstr) -> str:
        if len(prefix) == 0:
            return self.base
        elif prefix in self.ld_prefixes:
            return self.ld_prefixes[prefix]
        else:
            return self.prefixes.get(prefix, "")

    def iriref_to_str(self, ref: ShExDocParser.IRIREF) -> str:
        """ IRIREF: '<' (~[\u0000-\u0020=<>\"{}|^`\\] | UCHAR)* '>' """
        rval = self._fix_unicode_escapes(ref.getText()[1:-1])
        return rval if ':' in rval or not self.base else self.base.val + rval

    def iriref_to_shexj_iriref(self, ref: ShExDocParser.IRIREF) -> ShExJ.IRIREF:
        """  IRIREF: '<' (~[\u0000-\u0020=<>\"{}|^`\\] | UCHAR)* '>'
             IRI: (PN_CHARS | '!' | ''.' | ':' | '/' | '\\' | '#' | '@' | '%' | '&' | UCHAR)* """
        return ShExJ.IRIREF(self.iriref_to_str(ref))

    def prefixedname_to_str(self, prefix: ShExDocParser.PrefixedNameContext) -> str:
        """ prefixedName: PNAME_LN | PNAME_NS
            PNAME_NS: PN_PREFIX? ':' ;
            PNAME_LN: PNAME_NS PN_LOCAL ;
        """
        if prefix.PNAME_NS():
            return self._lookup_prefix(prefix.PNAME_NS().getText())
        else:
            prefix, local = prefix.PNAME_LN().getText().split(':', 1)
            return self._lookup_prefix(prefix + ':') + (local if local else "")

    def prefixedname_to_iriref(self, prefix: ShExDocParser.PrefixedNameContext) -> ShExJ.IRIREF:
        """ prefixedName: PNAME_LN | PNAME_NS
            PNAME_NS: PN_PREFIX? ':' ;
            PNAME_LN: PNAME_NS PN_LOCAL ;
        """
        return ShExJ.IRIREF(self.prefixedname_to_str(prefix))

    def shapeRef_to_iriref(self, ref: ShExDocParser.ShapeRefContext) -> ShExJ.IRIREF:
        """ shapeRef: ATPNAME_NS | ATPNAME_LN | '@' shapeExprLabel """
        if ref.ATPNAME_NS():
            return ShExJ.IRIREF(self._lookup_prefix(ref.ATPNAME_NS().getText()[1:]))
        elif ref.ATPNAME_LN():
            prefix, local = ref.ATPNAME_LN().getText()[1:].split(':', 1)
            return ShExJ.IRIREF(self._lookup_prefix(prefix + ':') + (local if local else ""))
        else:
            return self.shapeexprlabel_to_IRI(ref.shapeExprLabel())

    def iri_to_str(self, iri_: ShExDocParser.IriContext) -> str:
        """ iri: IRIREF | prefixedName 
        """
        if iri_.IRIREF():
            return self.iriref_to_str(iri_.IRIREF())
        else:
            return self.prefixedname_to_str(iri_.prefixedName())

    def iri_to_iriref(self, iri_: ShExDocParser.IriContext) -> ShExJ.IRIREF:
        """ iri: IRIREF | prefixedName 
            prefixedName: PNAME_LN | PNAME_NS 
        """
        return ShExJ.IRIREF(self.iri_to_str(iri_))

    def tripleexprlabel_to_iriref(self, tripleExprLabel: ShExDocParser.TripleExprLabelContext) \
            -> ShExJ.BNODE | ShExJ.IRIREF:
        """ tripleExprLabel: iri | blankNode """
        if tripleExprLabel.iri():
            return self.iri_to_iriref(tripleExprLabel.iri())
        else:
            return ShExJ.BNODE(tripleExprLabel.blankNode().getText())

    def shapeexprlabel_to_IRI(self, shapeExprLabel: ShExDocParser.ShapeExprLabelContext) \
            -> ShExJ.BNODE | ShExJ.IRIREF:
        """ shapeExprLabel: iri | blankNode """
        if shapeExprLabel.iri():
            return self.iri_to_iriref(shapeExprLabel.iri())
        else:
            return ShExJ.BNODE(shapeExprLabel.blankNode().getText())

    def predicate_to_IRI(self, predicate: ShExDocParser.PredicateContext) -> ShExJ.IRIREF:
        """ predicate: iri | rdfType """
        if predicate.iri():
            return self.iri_to_iriref(predicate.iri())
        else:
            return RDF_TYPE

    @staticmethod
    def numeric_literal_to_type(numlit: ShExDocParser.NumericLiteralContext) -> Integer | Number:
        """ numericLiteral: INTEGER | DECIMAL | DOUBLE """
        if numlit.INTEGER():
            rval = Integer(str(int(numlit.INTEGER().getText())))
        elif numlit.DECIMAL():
            rval = Number(str(float(numlit.DECIMAL().getText())))
        else:
            rval = Number(str(float(numlit.DOUBLE().getText())))
        return rval

    def literal_to_ObjectLiteral(self, literal: ShExDocParser.LiteralContext) -> ShExJ.ObjectLiteral:
        """ literal: rdfLiteral | numericLiteral | booleanLiteral """
        rval = ShExJ.ObjectLiteral()
        if literal.rdfLiteral():
            rdflit = literal.rdfLiteral()
            txt = rdflit.string().getText()
            quote_char = ''
            if len(txt) > 5 and (txt.startswith("'''") and txt.endswith("'''") or
                                 txt.startswith('"""') and txt.endswith('"""')):
                txt = txt[3:-3]
            else:
                quote_char = txt[0]
                txt = txt[1:-1]

            txt = self.fix_text_escapes(txt, quote_char)
            rval.value = String(txt)
            if rdflit.LANGTAG():
                rval.language = ShExJ.LANGTAG(rdflit.LANGTAG().getText()[1:].lower())
            if rdflit.datatype():
                rval.type = self.iri_to_str(rdflit.datatype().iri())
        elif literal.numericLiteral():
            numlit = literal.numericLiteral()
            if numlit.INTEGER():
                rval.value = String(numlit.INTEGER().getText())
                rval.type = RDF_INTEGER_TYPE
            elif numlit.DECIMAL():
                rval.value = String(numlit.DECIMAL().getText())
                rval.type = RDF_DECIMAL_TYPE
            elif numlit.DOUBLE():
                rval.value = String(numlit.DOUBLE().getText())
                rval.type = RDF_DOUBLE_TYPE
        elif literal.booleanLiteral():
            rval.value = String(literal.booleanLiteral().getText().lower())
            rval.type = RDF_BOOL_TYPE
        return rval

    @staticmethod
    def is_empty_shape(sh: ShExJ.Shape) -> bool:
        """ Determine whether sh has any value """
        return sh.closed is None and sh.expression is None and sh.extra is None and \
            sh.semActs is None

    @staticmethod
    def _fix_unicode_escapes(txt: str) -> str:
        def _subf2(matchobj):
            if len(matchobj.group(1)) % 2 == 0:
                return matchobj.group(1) + matchobj.group(2).encode().decode('unicode-escape')
            else:
                return matchobj.group(1) + matchobj.group(2)

        # match rule -- zero or more pairs of backslashes w/ an odd number in front of the 'u' or 'U'
        txt1 = re.sub(r'(\\*)(\\u[a-fA-F0-9]{4})', _subf2, txt, flags=re.MULTILINE + re.DOTALL + re.UNICODE)
        return re.sub(r'(\\*)(\\U[a-fA-F0-9]{8})', _subf2, txt1, flags=re.MULTILINE + re.DOTALL + re.UNICODE)

    def fix_text_escapes(self, txt: str, quote_char: str) -> str:
        """ Fix the various text escapes """
        def _subf(matchobj):
            o = matchobj.group(0)
            if o[1] in 'bfntr':
                return '\b\f\n\t\r'['bfntr'.index(o[1])]
            return o[1]

        if quote_char:
            txt = re.sub(r'\\'+quote_char, quote_char, txt)
        return re.sub(r'\\.', _subf, self._fix_unicode_escapes(txt), flags=re.MULTILINE + re.DOTALL + re.UNICODE)

    re_trans_table = str.maketrans('bfnrt', '\b\f\n\r\t')

    def fix_re_escapes(self, txt: str) -> str:
        """ The ShEx RE engine allows escaping any character.  We have to remove that escape for everything except those
        that CAN be legitimately escaped

        :param txt: text to be escaped
        """
        def _subf(matchobj):
            # o = self.fix_text_escapes(matchobj.group(0))
            o = matchobj.group(0).translate(self.re_trans_table)
            if o[1] in '\b\f\n\t\r\\':
                return o[0] + 'bfntr\\'['\b\f\n\t\r\\'.index(o[1])]
            else:
                return o if o[1] in '\\.?*+^$()[]{|}-' else o[1]

        return re.sub(r'\\.', _subf, self._fix_unicode_escapes(txt), flags=re.MULTILINE + re.DOTALL + re.UNICODE)

# var stringEscapeReplacements = { '\\': '\\', "'": "'", '"': '"',
#                                    't': '\t', 'b': '\b', 'n': '\n', 'r': '\r', 'f': '\f' },
#       semactEscapeReplacements = { '\\': '\\', '%': '%' },
#       pnameEscapeReplacements = {
#         '\\': '\\', "'": "'", '"': '"',
#         'n': '\n', 'r': '\r', 't': '\t', 'f': '\f', 'b': '\b',
#         '_': '_', '~': '~', '.': '.', '-': '-', '!': '!', '$': '$', '&': '&',
#         '(': '(', ')': ')', '*': '*', '+': '+', ',': ',', ';': ';', '=': '=',
#         '/': '/', '?': '?', '#': '#', '@': '@', '%': '%',
#       };