from pyshexc.parser.ShExDocParser import ShExDocParser
from pyshexc.parser.ShExDocVisitor import ShExDocVisitor

from pyshexc.parser_impl.parser_context import ParserContext
from pyshexc.parser_impl.shex_node_expression_parser import ShexNodeExpressionParser
from ShExJSG.ShExJ import BNODE, IRIREF, ShapeOr, ShapeAnd, ShapeNot, Shape, shapeExpr


class ShexShapeExpressionParser(ShExDocVisitor):
    """ Parser for Shape Expressions branch """
    def __init__(self, context: ParserContext, label: IRIREF | BNODE | None = None):
        ShExDocVisitor.__init__(self)
        self.context = context
        self.label = label
        self.expr = None

    # ------------------
    # shapeOr
    # ------------------
    def _shape_or(self, ands: list[ShExDocParser.ShapeAndContext] | list[ShExDocParser.InlineShapeAndContext]):
        if len(ands) > 1:
            exprs = []
            for sa in ands:
                sep = ShexShapeExpressionParser(self.context)
                sep.visit(sa)
                exprs.append(sep.expr)
            self.expr = ShapeOr(id=self.label, shapeExprs=exprs)
        else:
            self.visit(ands[0])

    def visitShapeOr(self, ctx: ShExDocParser.ShapeOrContext):
        """ shapeOr: shapeAnd (KW_OR shapeAnd)* """
        self._shape_or(ctx.shapeAnd())

    def visitInlineShapeOr(self, ctx: ShExDocParser.InlineShapeOrContext):
        """ inlineShapeOr: inlineShapeAnd (KW_OR inlineShapeAnd)* """
        self._shape_or(ctx.inlineShapeAnd())

    # ------------------
    # shapeAnd
    # ------------------
    def _shape_and(self,
                   nots: list[ShExDocParser.ShapeNotContext] | list[ShExDocParser.InlineShapeNotContext],
                   is_inline: bool):
        if len(nots) > 1:
            exprs = []
            for sa in nots:
                sep = ShexShapeExpressionParser(self.context)
                sep.visit(sa)
                if is_inline:
                    self._collapse_ands(exprs, sep.expr)
                else:
                    exprs.append(sep.expr)
            self.expr = ShapeAnd(id=self.label, shapeExprs=exprs)
        else:
            self.visit(nots[0])

    @staticmethod
    def _collapse_ands(exprs: list, new_expr: shapeExpr) -> None:
        """ For various nefarious reaons, the reference parser has decided to implement
            And(And(a, b), c) --> And(a, b, c).  We've got to do the same
        """
        if isinstance(new_expr, ShapeAnd):
            for expr in new_expr.shapeExprs:
                exprs.append(expr)
        else:
            exprs.append(new_expr)

    def visitShapeAnd(self, ctx: ShExDocParser.ShapeAndContext):
        """ shapeAnd: shapeNot (KW_AND shapeNot)* """
        self._shape_and(ctx.shapeNot(), False)

    def visitInlineShapeAnd(self, ctx: ShExDocParser.InlineShapeAndContext):
        """ inlineShapeAnd: inlineShapeNot (KW_AND inlineShapeNot)* """
        self._shape_and(ctx.inlineShapeNot(), True)

    # ------------------
    # shapeNot
    # ------------------
    def _shape_not(self,
                   ctx: ShExDocParser.ShapeNotContext | ShExDocParser.InlineShapeNotContext,
                   is_inline: bool) -> None:
        if ctx.KW_NOT():
            self.expr = ShapeNot(id=self.label)
            sn = ShexShapeExpressionParser(self.context)
            sn.visit(ctx.shapeAtom() if not is_inline else ctx.inlineShapeAtom())
            self.expr.shapeExpr = sn.expr if sn.expr is not None else Shape()
        else:
            self.visitChildren(ctx)

    def visitShapeNot(self, ctx: ShExDocParser.ShapeNotContext):
        """ shapeNot: KW_NOT? shapeAtom """
        self._shape_not(ctx, False)

    def visitInlineShapeNot(self, ctx: ShExDocParser.InlineShapeNotContext):
        """ inlineShapeNot: KW_NOT? inlineShapeAtom """
        self._shape_not(ctx, True)

    # ------------------
    # shapeAtom
    # ------------------
    def _shape_atom(self, ctx: ShExDocParser.ShapeAtomNonLitNodeConstraintContext |
                               ShExDocParser.ShapeAtomLitNodeConstraintContext |
                               ShExDocParser.InlineShapeAtomNonLitNodeConstraintContext |
                               ShExDocParser.InlineShapeAtomLitNodeConstraintContext,
                    is_inline: bool, is_lit: bool):
        """ One of nonLitNodeConstraint shapeOrRef?  or shapeOrRef nonLitNodeConstraint? """

        # Process the node constraint if it exists
        if is_inline:
            node_constraint = ctx.inlineLitNodeConstraint() if is_lit else ctx.inlineNonLitNodeConstraint()
        else:
            node_constraint = ctx.litNodeConstraint() if is_lit else ctx.nonLitNodeConstraint()
        if node_constraint:
            nc = ShexNodeExpressionParser(self.context, self.label if is_inline else None)
            nc.visit(node_constraint)
            exprs = [nc.nodeconstraint]
        else:
            exprs = []

        # Process the shape or ref if it exists
        if not is_lit:
            shape_or_ref = ctx.inlineShapeOrRef() if is_inline else ctx.shapeOrRef()
        else:
            shape_or_ref = None
        if shape_or_ref:
            sorref_parser = ShexShapeExpressionParser(self.context)
            sorref_parser.visit(shape_or_ref)
            exprs.append(sorref_parser.expr)

        if len(exprs) > 1:
            self.expr = ShapeAnd(id=self.label if not is_inline else None, shapeExprs=exprs)
        else:
            self.expr = exprs[0]
        if not is_inline:
            self.expr.id = self.label

    def visitShapeAtomNonLitNodeConstraint(self, ctx: ShExDocParser.ShapeAtomNonLitNodeConstraintContext):
        """ shapeAtom : nonLitNodeConstraint shapeOrRef?             # shapeAtomNonLitNodeConstraint """
        self._shape_atom(ctx, False, False)

    def visitShapeAtomLitNodeConstraint(self, ctx: ShExDocParser.ShapeAtomLitNodeConstraintContext):
        """ shapeAtom : litNodeConstraint             # shapeAtomLitNodeConstraint"""
        self._shape_atom(ctx, False, True)

    def visitInlineShapeAtomNonLitNodeConstraint(self, ctx: ShExDocParser.InlineShapeAtomNonLitNodeConstraintContext):
        """ inlineShapeAtom : inlineNonLitNodeConstraint inlineShapeOrRef? # inlineShapeAtomNonLitNodeConstraint """
        self._shape_atom(ctx, True, False)

    def visitInlineShapeAtomLitNodeConstraint(self, ctx: ShExDocParser.InlineShapeAtomLitNodeConstraintContext):
        """ inlineShapeAtom : inlineLitNodeConstraint  # inlineShapeAtomLitNodeConstraint """
        self._shape_atom(ctx, True, True)

    def visitShapeAtomShapeOrRef(self, ctx: ShExDocParser.ShapeAtomShapeOrRefContext):
        """ shapeAtom :  shapeOrRef nonLitNodeConstraint?            # shapeAtomShapeOrRef """
        self._shape_atom(ctx, False, False)

    def visitInlineShapeAtomShapeOrRef(self, ctx: ShExDocParser.InlineShapeAtomShapeOrRefContext):
        """ inlineShapeAtom : inlineShapeOrRef inlineNonLitNodeConstraint? # inlineShapeAtomShapeOrRef """
        self._shape_atom(ctx, True, False)

    # ------------------
    # shapeOrRef
    # ------------------
    def _shape_or_ref(self, ctx: ShExDocParser.ShapeOrRefContext | ShExDocParser.InlineShapeOrRefContext,
                      is_inline: bool):
        defn = ctx.inlineShapeDefinition() if is_inline else ctx.shapeDefinition()
        if defn:
            from pyshexc.parser_impl.shex_shape_definition_parser import ShexShapeDefinitionParser
            shdef_parser = ShexShapeDefinitionParser(self.context, self.label)
            shdef_parser.visitChildren(ctx)
            self.expr = shdef_parser.shape
        else:
            self.expr = self.context.shapeRef_to_iriref(ctx.shapeRef())

    def visitShapeOrRef(self, ctx: ShExDocParser.ShapeOrRefContext):
        """ shapeOrRef: shapeDefinition | shapeRef """
        self._shape_or_ref(ctx, False)

    def visitInlineShapeOrRef(self, ctx: ShExDocParser.InlineShapeOrRefContext):
        """ inlineShapeOrRef: inlineShapeDefinition | shapeRef """
        self._shape_or_ref(ctx, True)
