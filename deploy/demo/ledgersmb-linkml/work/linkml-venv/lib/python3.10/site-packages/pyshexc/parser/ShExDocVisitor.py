# Generated from ShExDoc.g4 by ANTLR 4.9
from antlr4 import *
if __name__ is not None and "." in __name__:
    from .ShExDocParser import ShExDocParser
else:
    from ShExDocParser import ShExDocParser

# This class defines a complete generic visitor for a parse tree produced by ShExDocParser.

class ShExDocVisitor(ParseTreeVisitor):

    # Visit a parse tree produced by ShExDocParser#shExDoc.
    def visitShExDoc(self, ctx:ShExDocParser.ShExDocContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#directive.
    def visitDirective(self, ctx:ShExDocParser.DirectiveContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#baseDecl.
    def visitBaseDecl(self, ctx:ShExDocParser.BaseDeclContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#prefixDecl.
    def visitPrefixDecl(self, ctx:ShExDocParser.PrefixDeclContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#importDecl.
    def visitImportDecl(self, ctx:ShExDocParser.ImportDeclContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#notStartAction.
    def visitNotStartAction(self, ctx:ShExDocParser.NotStartActionContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#start.
    def visitStart(self, ctx:ShExDocParser.StartContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#startActions.
    def visitStartActions(self, ctx:ShExDocParser.StartActionsContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#statement.
    def visitStatement(self, ctx:ShExDocParser.StatementContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#shapeExprDecl.
    def visitShapeExprDecl(self, ctx:ShExDocParser.ShapeExprDeclContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#shapeExpression.
    def visitShapeExpression(self, ctx:ShExDocParser.ShapeExpressionContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#inlineShapeExpression.
    def visitInlineShapeExpression(self, ctx:ShExDocParser.InlineShapeExpressionContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#shapeOr.
    def visitShapeOr(self, ctx:ShExDocParser.ShapeOrContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#inlineShapeOr.
    def visitInlineShapeOr(self, ctx:ShExDocParser.InlineShapeOrContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#shapeAnd.
    def visitShapeAnd(self, ctx:ShExDocParser.ShapeAndContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#inlineShapeAnd.
    def visitInlineShapeAnd(self, ctx:ShExDocParser.InlineShapeAndContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#shapeNot.
    def visitShapeNot(self, ctx:ShExDocParser.ShapeNotContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#inlineShapeNot.
    def visitInlineShapeNot(self, ctx:ShExDocParser.InlineShapeNotContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#shapeAtomNonLitNodeConstraint.
    def visitShapeAtomNonLitNodeConstraint(self, ctx:ShExDocParser.ShapeAtomNonLitNodeConstraintContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#shapeAtomLitNodeConstraint.
    def visitShapeAtomLitNodeConstraint(self, ctx:ShExDocParser.ShapeAtomLitNodeConstraintContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#shapeAtomShapeOrRef.
    def visitShapeAtomShapeOrRef(self, ctx:ShExDocParser.ShapeAtomShapeOrRefContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#shapeAtomShapeExpression.
    def visitShapeAtomShapeExpression(self, ctx:ShExDocParser.ShapeAtomShapeExpressionContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#shapeAtomAny.
    def visitShapeAtomAny(self, ctx:ShExDocParser.ShapeAtomAnyContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#inlineShapeAtomNonLitNodeConstraint.
    def visitInlineShapeAtomNonLitNodeConstraint(self, ctx:ShExDocParser.InlineShapeAtomNonLitNodeConstraintContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#inlineShapeAtomLitNodeConstraint.
    def visitInlineShapeAtomLitNodeConstraint(self, ctx:ShExDocParser.InlineShapeAtomLitNodeConstraintContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#inlineShapeAtomShapeOrRef.
    def visitInlineShapeAtomShapeOrRef(self, ctx:ShExDocParser.InlineShapeAtomShapeOrRefContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#inlineShapeAtomShapeExpression.
    def visitInlineShapeAtomShapeExpression(self, ctx:ShExDocParser.InlineShapeAtomShapeExpressionContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#inlineShapeAtomAny.
    def visitInlineShapeAtomAny(self, ctx:ShExDocParser.InlineShapeAtomAnyContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#shapeOrRef.
    def visitShapeOrRef(self, ctx:ShExDocParser.ShapeOrRefContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#inlineShapeOrRef.
    def visitInlineShapeOrRef(self, ctx:ShExDocParser.InlineShapeOrRefContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#shapeRef.
    def visitShapeRef(self, ctx:ShExDocParser.ShapeRefContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#nodeConstraintLiteral.
    def visitNodeConstraintLiteral(self, ctx:ShExDocParser.NodeConstraintLiteralContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#nodeConstraintNonLiteral.
    def visitNodeConstraintNonLiteral(self, ctx:ShExDocParser.NodeConstraintNonLiteralContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#nodeConstraintDatatype.
    def visitNodeConstraintDatatype(self, ctx:ShExDocParser.NodeConstraintDatatypeContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#nodeConstraintValueSet.
    def visitNodeConstraintValueSet(self, ctx:ShExDocParser.NodeConstraintValueSetContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#nodeConstraintNumericFacet.
    def visitNodeConstraintNumericFacet(self, ctx:ShExDocParser.NodeConstraintNumericFacetContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#litNodeConstraint.
    def visitLitNodeConstraint(self, ctx:ShExDocParser.LitNodeConstraintContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#litNodeConstraintLiteral.
    def visitLitNodeConstraintLiteral(self, ctx:ShExDocParser.LitNodeConstraintLiteralContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#litNodeConstraintStringFacet.
    def visitLitNodeConstraintStringFacet(self, ctx:ShExDocParser.LitNodeConstraintStringFacetContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#nonLitNodeConstraint.
    def visitNonLitNodeConstraint(self, ctx:ShExDocParser.NonLitNodeConstraintContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#nonLiteralKind.
    def visitNonLiteralKind(self, ctx:ShExDocParser.NonLiteralKindContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#xsFacet.
    def visitXsFacet(self, ctx:ShExDocParser.XsFacetContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#stringFacet.
    def visitStringFacet(self, ctx:ShExDocParser.StringFacetContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#stringLength.
    def visitStringLength(self, ctx:ShExDocParser.StringLengthContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#numericFacet.
    def visitNumericFacet(self, ctx:ShExDocParser.NumericFacetContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#numericRange.
    def visitNumericRange(self, ctx:ShExDocParser.NumericRangeContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#numericLength.
    def visitNumericLength(self, ctx:ShExDocParser.NumericLengthContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#rawNumeric.
    def visitRawNumeric(self, ctx:ShExDocParser.RawNumericContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#shapeDefinition.
    def visitShapeDefinition(self, ctx:ShExDocParser.ShapeDefinitionContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#inlineShapeDefinition.
    def visitInlineShapeDefinition(self, ctx:ShExDocParser.InlineShapeDefinitionContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#qualifier.
    def visitQualifier(self, ctx:ShExDocParser.QualifierContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#extraPropertySet.
    def visitExtraPropertySet(self, ctx:ShExDocParser.ExtraPropertySetContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#tripleExpression.
    def visitTripleExpression(self, ctx:ShExDocParser.TripleExpressionContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#oneOfTripleExpr.
    def visitOneOfTripleExpr(self, ctx:ShExDocParser.OneOfTripleExprContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#multiElementOneOf.
    def visitMultiElementOneOf(self, ctx:ShExDocParser.MultiElementOneOfContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#groupTripleExpr.
    def visitGroupTripleExpr(self, ctx:ShExDocParser.GroupTripleExprContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#singleElementGroup.
    def visitSingleElementGroup(self, ctx:ShExDocParser.SingleElementGroupContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#multiElementGroup.
    def visitMultiElementGroup(self, ctx:ShExDocParser.MultiElementGroupContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#unaryTripleExpr.
    def visitUnaryTripleExpr(self, ctx:ShExDocParser.UnaryTripleExprContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#bracketedTripleExpr.
    def visitBracketedTripleExpr(self, ctx:ShExDocParser.BracketedTripleExprContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#tripleConstraint.
    def visitTripleConstraint(self, ctx:ShExDocParser.TripleConstraintContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#starCardinality.
    def visitStarCardinality(self, ctx:ShExDocParser.StarCardinalityContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#plusCardinality.
    def visitPlusCardinality(self, ctx:ShExDocParser.PlusCardinalityContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#optionalCardinality.
    def visitOptionalCardinality(self, ctx:ShExDocParser.OptionalCardinalityContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#repeatCardinality.
    def visitRepeatCardinality(self, ctx:ShExDocParser.RepeatCardinalityContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#exactRange.
    def visitExactRange(self, ctx:ShExDocParser.ExactRangeContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#minMaxRange.
    def visitMinMaxRange(self, ctx:ShExDocParser.MinMaxRangeContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#senseFlags.
    def visitSenseFlags(self, ctx:ShExDocParser.SenseFlagsContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#valueSet.
    def visitValueSet(self, ctx:ShExDocParser.ValueSetContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#valueSetValue.
    def visitValueSetValue(self, ctx:ShExDocParser.ValueSetValueContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#iriRange.
    def visitIriRange(self, ctx:ShExDocParser.IriRangeContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#iriExclusion.
    def visitIriExclusion(self, ctx:ShExDocParser.IriExclusionContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#literalRange.
    def visitLiteralRange(self, ctx:ShExDocParser.LiteralRangeContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#literalExclusion.
    def visitLiteralExclusion(self, ctx:ShExDocParser.LiteralExclusionContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#languageRangeFull.
    def visitLanguageRangeFull(self, ctx:ShExDocParser.LanguageRangeFullContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#languageRangeAt.
    def visitLanguageRangeAt(self, ctx:ShExDocParser.LanguageRangeAtContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#languageExclusion.
    def visitLanguageExclusion(self, ctx:ShExDocParser.LanguageExclusionContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#include.
    def visitInclude(self, ctx:ShExDocParser.IncludeContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#annotation.
    def visitAnnotation(self, ctx:ShExDocParser.AnnotationContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#semanticAction.
    def visitSemanticAction(self, ctx:ShExDocParser.SemanticActionContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#literal.
    def visitLiteral(self, ctx:ShExDocParser.LiteralContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#predicate.
    def visitPredicate(self, ctx:ShExDocParser.PredicateContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#rdfType.
    def visitRdfType(self, ctx:ShExDocParser.RdfTypeContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#datatype.
    def visitDatatype(self, ctx:ShExDocParser.DatatypeContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#shapeExprLabel.
    def visitShapeExprLabel(self, ctx:ShExDocParser.ShapeExprLabelContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#tripleExprLabel.
    def visitTripleExprLabel(self, ctx:ShExDocParser.TripleExprLabelContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#numericLiteral.
    def visitNumericLiteral(self, ctx:ShExDocParser.NumericLiteralContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#rdfLiteral.
    def visitRdfLiteral(self, ctx:ShExDocParser.RdfLiteralContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#booleanLiteral.
    def visitBooleanLiteral(self, ctx:ShExDocParser.BooleanLiteralContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#string.
    def visitString(self, ctx:ShExDocParser.StringContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#iri.
    def visitIri(self, ctx:ShExDocParser.IriContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#prefixedName.
    def visitPrefixedName(self, ctx:ShExDocParser.PrefixedNameContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#blankNode.
    def visitBlankNode(self, ctx:ShExDocParser.BlankNodeContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#extension.
    def visitExtension(self, ctx:ShExDocParser.ExtensionContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by ShExDocParser#restrictions.
    def visitRestrictions(self, ctx:ShExDocParser.RestrictionsContext):
        return self.visitChildren(ctx)



del ShExDocParser