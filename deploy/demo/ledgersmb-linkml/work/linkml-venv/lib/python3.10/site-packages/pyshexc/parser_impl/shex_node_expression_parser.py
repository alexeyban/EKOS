from ShExJSG.ShExJ import Language, NodeConstraint, Wildcard, IriStemRange, LiteralStemRange, \
    LanguageStemRange, IriStem, LiteralStem, LanguageStem, LANGTAG, IRIREF, ObjectLiteral

from pyshexc.parser.ShExDocParser import ShExDocParser
from pyshexc.parser.ShExDocVisitor import ShExDocVisitor
from pyshexc.parser_impl.parser_context import ParserContext
from pyshexc.parser_impl.shex_annotations_and_semacts_parser import ShexAnnotationAndSemactsParser

from pyjsg.jsglib import Integer, String


class ShexNodeExpressionParser(ShExDocVisitor):

    def _non_literal_kind(self, kind: ShExDocParser.NonLiteralKindContext):
        """ Parser for nodeConstraint productions """
        self.nodeconstraint.nodeKind = 'iri' if kind.KW_IRI() \
            else 'bnode' if kind.KW_BNODE() \
            else 'nonliteral' if kind.KW_NONLITERAL() \
            else 'undefined'

    def __init__(self, context: ParserContext, label: str | None = None):
        ShExDocVisitor.__init__(self)
        self.context = context
        self.nodeconstraint = NodeConstraint(id=label)
        self._nc_values = None

    def visitNodeConstraintLiteral(self, ctx: ShExDocParser.NodeConstraintLiteralContext):
        """ inlineLitNodeConstraint: KW_LITERAL xsFacet* # nodeConstraintLiteral """
        self.nodeconstraint.nodeKind = 'literal'
        self.visitChildren(ctx)

    def visitNodeConstraintNonLiteral(self, ctx: ShExDocParser.NodeConstraintNonLiteralContext):
        """ inlineLitNodeConstraint: nonLiteralKind stringFacet*  # nodeConstraintNonLiteral """
        self._non_literal_kind(ctx.nonLiteralKind())
        self.visitChildren(ctx)

    def visitNodeConstraintDatatype(self, ctx: ShExDocParser.NodeConstraintDatatypeContext):
        """ inlineLitNodeConstraint: datatype xsFacet* # nodeConstraintDatatype """
        self.nodeconstraint.datatype = self.context.iri_to_iriref(ctx.datatype().iri())
        self.visitChildren(ctx)

    def visitNodeConstraintValueSet(self, ctx: ShExDocParser.NodeConstraintValueSetContext):
        """ inlineLitNodeConstraint: valueSet xsFacet* #nodeConstraintValueSet """
        self._nc_values = []
        self.visitChildren(ctx)
        self.nodeconstraint.values = self._nc_values

    def visitLitNodeConstraintLiteral(self, ctx: ShExDocParser.LitNodeConstraintLiteralContext):
        """ inlineNonLitNodeConstraint  : nonLiteralKind stringFacet*	# litNodeConstraintLiteral """
        self._non_literal_kind(ctx.nonLiteralKind())
        self.visitChildren(ctx)

    def visitValueSetValue(self, ctx: ShExDocParser.ValueSetValueContext):
        """ valueSetValue: iriRange | literalRange | languageRange | 
            '.' (iriExclusion+ | literalExclusion+ | languageExclusion+) """
        if ctx.iriRange() or ctx.literalRange() or ctx.languageRange():
            self.visitChildren(ctx)
        else:                               # '.' branch - wild card with exclusions
            if ctx.iriExclusion():
                vs_value = IriStemRange(Wildcard(), self._iri_exclusions(ctx.iriExclusion()))
            elif ctx.literalExclusion():
                vs_value = LiteralStemRange(Wildcard(), self._literal_exclusions(ctx.literalExclusion()))
            else:
                vs_value = LanguageStemRange(Wildcard(), self._language_exclusions(ctx.languageExclusion()))
            self._nc_values.append(vs_value)

    def visitIriRange(self, ctx: ShExDocParser.IriRangeContext):
        """ iriRange: iri (STEM_MARK iriExclusion*)? """
        baseiri = self.context.iri_to_iriref(ctx.iri())
        if not ctx.STEM_MARK():
            vsvalue = baseiri                   # valueSetValue = objectValue  / objectValue = IRI
        else:
            if ctx.iriExclusion():              # valueSetValue = IriStemRange / iriStemRange = stem + exclusions
                vsvalue = IriStemRange(baseiri, exclusions=self._iri_exclusions(ctx.iriExclusion()))
            else:
                vsvalue = IriStem(baseiri)      # valueSetValue = IriStem  /  IriStem: {stem:IRI}
        self._nc_values.append(vsvalue)

    def _iri_exclusions(self, exclusions: list[ShExDocParser.IriExclusionContext]) \
            -> list[IriStem | IRIREF]:
        rval: list[IriStem | IRIREF] = []
        for excl in exclusions:
            excl_iri = self.context.iri_to_iriref(excl.iri())
            rval.append(IriStem(excl_iri) if excl.STEM_MARK() else excl_iri)
        return rval

    def visitLiteralRange(self, ctx: ShExDocParser.LiteralRangeContext):
        """ ShExC: literalRange: literal (STEM_MARK literalExclusion*)? 
            ShExJ: valueSetValue: objectValue | LiteralStem | LiteralStemRange
                   literalStem: {stem:STRING}
                   literalStemRange: {stem:STRING exclusions:[STRING|LiteralStem+]?
        """
        baseliteral = self.context.literal_to_ObjectLiteral(ctx.literal())
        if not ctx.STEM_MARK():
            vsvalue = baseliteral               # valueSetValue = objectValue / objectValue = ObjectLiteral
        else:
            if ctx.literalExclusion():          # valueSetValue = LiteralStemRange /
                vsvalue = LiteralStemRange(baseliteral.value.val,
                                           exclusions=self._literal_exclusions(ctx.literalExclusion()))
            else:
                vsvalue = LiteralStem(baseliteral.value)
        self._nc_values.append(vsvalue)

    def _literal_exclusions(self, exclusions: list[ShExDocParser.LiteralExclusionContext]) \
            -> list[ObjectLiteral | LiteralStem]:
        """ ShExC: literalExclusion = '-' literal STEM_MARK?
            ShExJ: exclusions: [STRING|LiteralStem +]
                   literalStem: {stem:STRING}
        """
        rval: list[ObjectLiteral | LiteralStem] = []
        for excl in exclusions:
            excl_literal_v = self.context.literal_to_ObjectLiteral(excl.literal()).value
            rval.append(LiteralStem(excl_literal_v) if excl.STEM_MARK() else excl_literal_v)
        return rval

    def _visit_language_range(self,  ctx: ShExDocParser.LanguageRangeFullContext | ShExDocParser.LanguageRangeAtContext):
        """ ShExC: languageRange : LANGTAG (STEM_MARK languagExclusion*)?  # languageRangeFull
                                 | '@' STEM_MARK languageExclusion*        # languageRangeAt
            ShExJ: valueSetValue = objectValue | LanguageStem | LanguageStemRange """
        baselang = ctx.LANGTAG().getText()[1:] if isinstance(ctx, ShExDocParser.LanguageRangeFullContext) else ""
        if not ctx.STEM_MARK():                 # valueSetValue = objectValue / objectValue = ObjectLiteral
            vsvalue = Language()
            vsvalue.languageTag = baselang
        else:
            if ctx.languageExclusion():
                vsvalue = LanguageStemRange(baselang,
                                            exclusions=self._language_exclusions(ctx.languageExclusion()))
            else:
                vsvalue = LanguageStem(LANGTAG(baselang))
        self._nc_values.append(vsvalue)

    def visitLanguageRangeFull(self, ctx: ShExDocParser.LanguageRangeFullContext):
        self._visit_language_range(ctx)

    def visitLanguageRangeAt(self, ctx: ShExDocParser.LanguageRangeAtContext):
        if not self._nc_values and not ctx.languageExclusion():
            self._nc_values.append(LanguageStem(""))
        else:
            self._visit_language_range(ctx)

    @staticmethod
    def _language_exclusions(exclusions: list[ShExDocParser.LanguageExclusionContext]) \
            -> list[LANGTAG | LanguageStem]:
        """ languageExclusion = '-' LANGTAG STEM_MARK?"""
        rval: list[LANGTAG | LanguageStem] = []
        for excl in exclusions:
            excl_langtag = LANGTAG(excl.LANGTAG().getText()[1:])
            rval.append(LanguageStem(excl_langtag) if excl.STEM_MARK() else excl_langtag)
        return rval

    def visitStringFacet(self, ctx: ShExDocParser.StringFacetContext):
        """ stringFacet: stringLength INTEGER | REGEXP REGEXP_FLAGS
            stringLength: KW_LENGTH | KW_MINLENGTH | KW_MAXLENGTH """
        if ctx.stringLength():
            slen = Integer(ctx.INTEGER().getText())
            if ctx.stringLength().KW_LENGTH():
                self.nodeconstraint.length = slen
            elif ctx.stringLength().KW_MINLENGTH():
                self.nodeconstraint.minlength = slen
            else:
                self.nodeconstraint.maxlength = slen

        else:
            self.nodeconstraint.pattern = String(self.context.fix_re_escapes(ctx.REGEXP().getText()[1:-1]))
            if ctx.REGEXP_FLAGS():
                self.nodeconstraint.flags = String(ctx.REGEXP_FLAGS().getText())

    def visitNumericFacet(self, ctx: ShExDocParser.NumericFacetContext):
        """ numericFacet: numericRange rawNumeric | numericLength INTEGER
            numericRange: KW_MINEXCLUSIVE | KW_MININCLUSIVE | KW_MAXEXCLUSIVE | KW_MAXINCLUSIVE 
            numericLength: KW_TOTALDIGITS | KW_FRACTIONDIGITS """
        if ctx.numericRange():
            numlit = self.context.numeric_literal_to_type(ctx.rawNumeric())
            if ctx.numericRange().KW_MINEXCLUSIVE():
                self.nodeconstraint.minexclusive = numlit
            elif ctx.numericRange().KW_MAXEXCLUSIVE():
                self.nodeconstraint.maxexclusive = numlit
            elif ctx.numericRange().KW_MININCLUSIVE():
                self.nodeconstraint.mininclusive = numlit
            elif ctx.numericRange().KW_MAXINCLUSIVE():
                self.nodeconstraint.maxinclusive = numlit
        else:
            nlen = Integer(ctx.INTEGER().getText())
            if ctx.numericLength().KW_TOTALDIGITS():
                self.nodeconstraint.totaldigits = nlen
            elif ctx.numericLength().KW_FRACTIONDIGITS():
                self.nodeconstraint.fractiondigits = nlen

    def visitLitNodeConstraint(self, ctx: ShExDocParser.LitNodeConstraintContext):
        """ litNodeConstraint : inlineLitNodeConstraint  annotation* semanticAction*  """
        self.visitChildren(ctx)
        if ctx.annotation() or ctx.semanticAction():
            ansem_parser = ShexAnnotationAndSemactsParser(self.context)
            for annot in ctx.annotation():
                ansem_parser.visit(annot)
            ansem_parser.visit(ctx.semanticAction())
            if ansem_parser.semacts:
                self.nodeconstraint.semActs = ansem_parser.semacts
            if ansem_parser.annotations:
                self.nodeconstraint.annotations = ansem_parser.annotations
