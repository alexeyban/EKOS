# Generated from jsgParser.g4 by ANTLR 4.9
from antlr4 import *
if __name__ is not None and "." in __name__:
    from .jsgParser import jsgParser
else:
    from jsgParser import jsgParser

# This class defines a complete generic visitor for a parse tree produced by jsgParser.

class jsgParserVisitor(ParseTreeVisitor):

    # Visit a parse tree produced by jsgParser#doc.
    def visitDoc(self, ctx:jsgParser.DocContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#typeDirective.
    def visitTypeDirective(self, ctx:jsgParser.TypeDirectiveContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#typeExceptions.
    def visitTypeExceptions(self, ctx:jsgParser.TypeExceptionsContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#ignoreDirective.
    def visitIgnoreDirective(self, ctx:jsgParser.IgnoreDirectiveContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#grammarElt.
    def visitGrammarElt(self, ctx:jsgParser.GrammarEltContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#objectDef.
    def visitObjectDef(self, ctx:jsgParser.ObjectDefContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#objectExpr.
    def visitObjectExpr(self, ctx:jsgParser.ObjectExprContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#membersDef.
    def visitMembersDef(self, ctx:jsgParser.MembersDefContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#altMemberDef.
    def visitAltMemberDef(self, ctx:jsgParser.AltMemberDefContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#member.
    def visitMember(self, ctx:jsgParser.MemberContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#lastComma.
    def visitLastComma(self, ctx:jsgParser.LastCommaContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#pairDef.
    def visitPairDef(self, ctx:jsgParser.PairDefContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#name.
    def visitName(self, ctx:jsgParser.NameContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#arrayDef.
    def visitArrayDef(self, ctx:jsgParser.ArrayDefContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#arrayExpr.
    def visitArrayExpr(self, ctx:jsgParser.ArrayExprContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#objectMacro.
    def visitObjectMacro(self, ctx:jsgParser.ObjectMacroContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#valueTypeMacro.
    def visitValueTypeMacro(self, ctx:jsgParser.ValueTypeMacroContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#builtinValueType.
    def visitBuiltinValueType(self, ctx:jsgParser.BuiltinValueTypeContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#valueType.
    def visitValueType(self, ctx:jsgParser.ValueTypeContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#nonRefValueType.
    def visitNonRefValueType(self, ctx:jsgParser.NonRefValueTypeContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#typeAlternatives.
    def visitTypeAlternatives(self, ctx:jsgParser.TypeAlternativesContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#idref.
    def visitIdref(self, ctx:jsgParser.IdrefContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#ebnfSuffix.
    def visitEbnfSuffix(self, ctx:jsgParser.EbnfSuffixContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#lexerRules.
    def visitLexerRules(self, ctx:jsgParser.LexerRulesContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#lexerRuleSpec.
    def visitLexerRuleSpec(self, ctx:jsgParser.LexerRuleSpecContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#lexerRuleBlock.
    def visitLexerRuleBlock(self, ctx:jsgParser.LexerRuleBlockContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#lexerAltList.
    def visitLexerAltList(self, ctx:jsgParser.LexerAltListContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#lexerAlt.
    def visitLexerAlt(self, ctx:jsgParser.LexerAltContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#lexerElements.
    def visitLexerElements(self, ctx:jsgParser.LexerElementsContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#lexerElement.
    def visitLexerElement(self, ctx:jsgParser.LexerElementContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#lexerBlock.
    def visitLexerBlock(self, ctx:jsgParser.LexerBlockContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#lexerAtom.
    def visitLexerAtom(self, ctx:jsgParser.LexerAtomContext):
        return self.visitChildren(ctx)


    # Visit a parse tree produced by jsgParser#lexerTerminal.
    def visitLexerTerminal(self, ctx:jsgParser.LexerTerminalContext):
        return self.visitChildren(ctx)



del jsgParser