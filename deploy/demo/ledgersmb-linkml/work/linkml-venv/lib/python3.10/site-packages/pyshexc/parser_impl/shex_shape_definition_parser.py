from pyshexc.parser.ShExDocParser import ShExDocParser
from pyshexc.parser.ShExDocVisitor import ShExDocVisitor

from pyshexc.parser_impl.parser_context import ParserContext
from pyshexc.parser_impl.shex_annotations_and_semacts_parser import ShexAnnotationAndSemactsParser
from pyshexc.parser_impl.shex_oneofshape_parser import ShexTripleExpressionParser
from ShExJSG.ShExJ import IRIREF, BNODE, Shape


class ShexShapeDefinitionParser(ShExDocVisitor):
    def __init__(self, context: ParserContext, label: IRIREF | BNODE | None=None):
        ShExDocVisitor.__init__(self)
        self.context = context
        self.shape = Shape(label)

    def visitShapeDefinition(self, ctx: ShExDocParser.ShapeDefinitionContext):
        """ shapeDefinition: inlineShapeDefinition annotation* semanticAction* """
        self.visitInlineShapeDefinition(ctx.inlineShapeDefinition())
        if ctx.annotation() or ctx.semanticAction():
            ansem_parser = ShexAnnotationAndSemactsParser(self.context)
            ansem_parser.visit(ctx)
            if ansem_parser.semacts:
                self.shape.semActs = ansem_parser.semacts
            if ansem_parser.annotations:
                self.shape.annotations = ansem_parser.annotations

    def visitInlineShapeDefinition(self, ctx: ShExDocParser.InlineShapeDefinitionContext):
        """ shapeDefinition: qualifier* '{' oneOfShape? '}' """
        if ctx.qualifier():
            for q in ctx.qualifier():
                self.visit(q)
        if ctx.tripleExpression():
            oneof_parser = ShexTripleExpressionParser(self.context)
            oneof_parser.visit(ctx.tripleExpression())
            self.shape.expression = oneof_parser.expression

    def visitQualifier(self, ctx: ShExDocParser.QualifierContext):
        """ qualifier: extension | extraPropertySet | KW_CLOSED
            extensions: KW_EXTENDS shapeExprLabel | '&' shapeExprLabel
        """
        if ctx.extension():
            ext = self.context.shapeexprlabel_to_IRI(ctx.extension().shapeExprLabel())
            if self.shape.extends is None:
                self.shape.extends = [ext]
            else:
                self.shape.extends.append(ext)
        if ctx.extraPropertySet():
            if self.shape.extra is None:
                self.shape.extra = [self.context.predicate_to_IRI(p) for p in ctx.extraPropertySet().predicate()]
            else:
                self.shape.extra += [self.context.predicate_to_IRI(p) for p in ctx.extraPropertySet().predicate()]

        elif ctx.KW_CLOSED():
            self.shape.closed = True
