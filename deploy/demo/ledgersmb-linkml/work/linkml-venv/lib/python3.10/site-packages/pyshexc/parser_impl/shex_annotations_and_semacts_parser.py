from pyshexc.parser.ShExDocParser import ShExDocParser
from pyshexc.parser.ShExDocVisitor import ShExDocVisitor

from pyshexc.parser_impl.parser_context import ParserContext
from ShExJSG.ShExJ import Annotation, SemAct


class ShexAnnotationAndSemactsParser(ShExDocVisitor):
    def __init__(self, context: ParserContext):
        ShExDocVisitor.__init__(self)
        self.context = context
        self.semacts = []                   # List[SemAct]
        self.annotations = []               # List[Annotation]

    def visitAnnotation(self, ctx: ShExDocParser.AnnotationContext):
        """ annotation: '//' predicate (iri | literal) """
        # Annotations apply to the expression, NOT the shape (!)
        annot = Annotation(self.context.predicate_to_IRI(ctx.predicate()))
        if ctx.iri():
            annot.object = self.context.iri_to_iriref(ctx.iri())
        else:
            annot.object = self.context.literal_to_ObjectLiteral(ctx.literal())
        self.annotations.append(annot)

    def visitSemanticAction(self, ctx: ShExDocParser.SemanticActionContext):
        """ semanticAction: '%' iri (CODE | '%') 
            CODE: : '{' (~[%\\] | '\\' [%\\] | UCHAR)* '%' '}' """
        semact = SemAct()
        semact.name = self.context.iri_to_iriref(ctx.iri())
        if ctx.CODE():
            semact.code = self.context.\
                _fix_unicode_escapes(ctx.CODE().getText()[1:-2].
                                     replace('\\%', '%').
                                     replace(r'\\n', '\\n').
                                     replace(r'\\', '\\'))
        self.semacts.append(semact)
