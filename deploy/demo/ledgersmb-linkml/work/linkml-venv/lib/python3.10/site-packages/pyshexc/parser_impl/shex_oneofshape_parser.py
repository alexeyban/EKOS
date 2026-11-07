from ShExJSG.ShExJ import OneOf, EachOf, TripleConstraint

from pyshexc.parser.ShExDocParser import ShExDocParser
from pyshexc.parser.ShExDocVisitor import ShExDocVisitor
from pyshexc.parser_impl.parser_context import ParserContext
from pyshexc.parser_impl.shex_annotations_and_semacts_parser import ShexAnnotationAndSemactsParser
from pyshexc.parser_impl.shex_shape_expression_parser import ShexShapeExpressionParser


class ShexTripleExpressionParser(ShExDocVisitor):
    def __init__(self, context: ParserContext):
        ShExDocVisitor.__init__(self)
        self.context = context
        self.expression: TripleConstraint = None

    def visitMultiElementOneOf(self, ctx: ShExDocParser.MultiElementOneOfContext):
        """ multiElementOneOf: groupTripleExpr ('|' groupTripleExpr)+ """
        expressions = []
        for gs in ctx.groupTripleExpr():
            parser = ShexTripleExpressionParser(self.context)
            parser.visit(gs)
            expressions.append(parser.expression)
        self.expression = OneOf(expressions=expressions)

    def visitMultiElementGroup(self, ctx: ShExDocParser.MultiElementGroupContext):
        """ multiElementGroup: unaryTripleExpr (';' unaryTripleExpr)+ ';'? """
        expressions = []
        for us in ctx.unaryTripleExpr():
            parser = ShexTripleExpressionParser(self.context)
            parser.visit(us)
            expressions.append(parser.expression)
        self.expression = EachOf(expressions=expressions)

    def visitUnaryTripleExpr(self, ctx: ShExDocParser.UnaryTripleExprContext):
        """ unaryTripleExpr: ('$' tripleExprLabel)? (tripleConstraint | bracketedTripleExpr) | include """
        if ctx.include():
            self.expression = self.context.tripleexprlabel_to_iriref(ctx.include().tripleExprLabel())
        else:
            lbl = self.context.tripleexprlabel_to_iriref(ctx.tripleExprLabel()) if ctx.tripleExprLabel() else None
            if ctx.tripleConstraint():
                self.expression = TripleConstraint(lbl)
                self.visit(ctx.tripleConstraint())
            elif ctx.bracketedTripleExpr():
                self.visit(ctx.bracketedTripleExpr())
                self.expression.id = lbl

    def visitBracketedTripleExpr(self, ctx: ShExDocParser.BracketedTripleExprContext):
        """ bracketedTripleExpr: '(' tripleExpression ')' cardinality? /* onShapeExpr?*/ annotation* semanticAction* """
        enc_shape = ShexTripleExpressionParser(self.context)
        enc_shape.visit(ctx.tripleExpression())
        self.expression = enc_shape.expression
        # if ctx.onShapeExpr():
        #     self.expression.onShapeExpression = self._onShapeExpr(ctx.onShapeExpr())
        self._card_annotations_and_semacts(ctx)

    # def _onShapeExpr(self, ctx: ShExDocParser.OnShapeExprContext) -> TripleConstraint:
    #     """ onShapeExpr : KW_ON (KW_SHAPE KW_EXPRESSION)? inlineShapeExpression """
    #     expr_parser = ShexShapeExpressionParser(self.context)
    #     expr_parser.visit(ctx.inlineShapeExpression())
    #     return expr_parser.expr

    def visitTripleConstraint(self, ctx: ShExDocParser.TripleConstraintContext):
        """ tripleConstraint: senseFlags? predicate inlineShapeExpression cardinality? annotation* semanticAction """
        # This exists because of the predicate within annotation - if we default to visitchildren, we intercept both
        # predicates
        if ctx.senseFlags():
            self.visit(ctx.senseFlags())
        self.visit(ctx.predicate())
        self.visit(ctx.inlineShapeExpression())
        # if ctx.onShapeExpr():
        #     self.expression.onShapeExpression = self._onShapeExpr(ctx.onShapeExpr())
        self._card_annotations_and_semacts(ctx)

    def visitStarCardinality(self, ctx: ShExDocParser.StarCardinalityContext):
        """ '*' """
        self.expression.min = 0
        self.expression.max = -1

    def visitPlusCardinality(self, ctx: ShExDocParser.PlusCardinalityContext):
        """ '+' """
        self.expression.min = 1
        self.expression.max = -1

    def visitOptionalCardinality(self, ctx: ShExDocParser.OptionalCardinalityContext):
        """ '?' """
        self.expression.min = 0
        self.expression.max = 1

    def visitExactRange(self, ctx: ShExDocParser.ExactRangeContext):
        """ repeatRange: '{' INTEGER '}' #exactRange """
        self.expression.min = int(ctx.INTEGER().getText())
        self.expression.max = self.expression.min

    def visitMinMaxRange(self, ctx: ShExDocParser.MinMaxRangeContext):
        """ repeatRange: '{' INTEGER (',' (INTEGER | UNBOUNDED)? '}' """
        self.expression.min = int(ctx.INTEGER(0).getText())
        self.expression.max = int(ctx.INTEGER(1).getText()) if len(ctx.INTEGER()) > 1 else -1

    def visitSenseFlags(self, ctx: ShExDocParser.SenseFlagsContext):
        """ senseFlags: '^' """
        self.expression.inverse = True

    def visitPredicate(self, ctx: ShExDocParser.PredicateContext):
        """ predicate: iri | rdfType """
        self.expression.predicate = self.context.predicate_to_IRI(ctx)

    def visitInlineShapeExpression(self, ctx: ShExDocParser.InlineShapeExpressionContext):
        """ inlineShapeExpression: inlineShapeOr """
        expr_parser = ShexShapeExpressionParser(self.context)
        expr_parser.visitChildren(ctx)
        self.expression.valueExpr = expr_parser.expr

    def _card_annotations_and_semacts(self, ctx):
        if ctx.cardinality():
            self.visit(ctx.cardinality())
        annot_parser = ShexAnnotationAndSemactsParser(self.context)
        if ctx.annotation():
            for annot in ctx.annotation():
                annot_parser.visit(annot)
        for act in ctx.semanticAction():
            annot_parser.visit(act)
        if annot_parser.annotations:
            self.expression.annotations = annot_parser.annotations
        if annot_parser.semacts:
            self.expression.semActs = annot_parser.semacts
