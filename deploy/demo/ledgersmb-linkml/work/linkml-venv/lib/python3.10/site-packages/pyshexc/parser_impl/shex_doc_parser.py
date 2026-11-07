from pyshexc.parser.ShExDocParser import ShExDocParser
from pyshexc.parser.ShExDocVisitor import ShExDocVisitor

from pyshexc.parser_impl.parser_context import ParserContext
from pyshexc.parser_impl.shex_annotations_and_semacts_parser import ShexAnnotationAndSemactsParser
from pyshexc.parser_impl.shex_shape_expression_parser import ShexShapeExpressionParser
from ShExJSG.ShExJ import ShapeExternal, IRIREF, ShapeDecl


class ShexDocParser(ShExDocVisitor):
    """ parser for sheExDoc production """
    def __init__(self, default_base: str | None = None):
        ShExDocVisitor.__init__(self)
        self.context = ParserContext()
        self.context.base = IRIREF(default_base) if default_base else None

    def visitShExDoc(self, ctx: ShExDocParser.ShExDocContext):
        """ shExDoc: directive* ((notStartAction | startActions) statement*)? EOF """
        super().visitShExDoc(ctx)

    def visitBaseDecl(self, ctx: ShExDocParser.BaseDeclContext):
        """ baseDecl: KW_BASE IRIREF """
        self.context.base = ''
        self.context.base = self.context.iriref_to_shexj_iriref(ctx.IRIREF())

    def visitPrefixDecl(self, ctx: ShExDocParser.PrefixDeclContext):
        """ prefixDecl: KW_PREFIX PNAME_NS IRIREF """
        iri = self.context.iriref_to_shexj_iriref(ctx.IRIREF())
        prefix = ctx.PNAME_NS().getText()
        if iri not in self.context.ld_prefixes:
            self.context.prefixes[prefix] = iri.val

    def visitImportDecl(self, ctx: ShExDocParser.ImportDeclContext):
        """ importDecl : KW_IMPORT IRIREF """
        if self.context.schema.imports is None:
            self.context.schema.imports = [self.context.iriref_to_shexj_iriref(ctx.IRIREF())]
        else:
            self.context.schema.imports.append(self.context.iriref_to_shexj_iriref(ctx.IRIREF()))

    def visitStart(self, ctx: ShExDocParser.StartContext):
        """ start: KW_START '=' shapeExpression """
        shexpr = ShexShapeExpressionParser(self.context, None)
        shexpr.visit(ctx.shapeExpression())
        self.context.schema.start = shexpr.expr

    def visitShapeExprDecl(self, ctx: ShExDocParser.ShapeExprDeclContext):
        """ shapeExprDecl: KW_ABSTRACT? shapeExprLabel restrictions*  (shapeExpression | KW_EXTERNAL) ;"""
        label = self.context.shapeexprlabel_to_IRI(ctx.shapeExprLabel())

        if ctx.KW_EXTERNAL():
            shape = ShapeExternal()
        else:
            shexpr = ShexShapeExpressionParser(self.context)
            shexpr.visit(ctx.shapeExpression())
            shape = shexpr.expr

        if ctx.KW_ABSTRACT() or ctx.restrictions():
            shape = ShapeDecl(shapeExpr=shape)
            if ctx.KW_ABSTRACT():
                shape.abstract = True
            if ctx.restrictions():
                shape.restricts = [self.context.shapeexprlabel_to_IRI(r.shapeExprLabel()) for r in ctx.restrictions()]

        if label:
            shape.id = label

        if self.context.schema.shapes is None:
            self.context.schema.shapes = [shape]
        else:
            self.context.schema.shapes.append(shape)

    def visitStartActions(self, ctx: ShExDocParser.StartActionsContext):
        """ startActions: semanticAction+ ; """
        startacts = []
        for cd in ctx.semanticAction():
            cdparser = ShexAnnotationAndSemactsParser(self.context)
            cdparser.visit(cd)
            startacts += cdparser.semacts
        self.context.schema.startActs = startacts
