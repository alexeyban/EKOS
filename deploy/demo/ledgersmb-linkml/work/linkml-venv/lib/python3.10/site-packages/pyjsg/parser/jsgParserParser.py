# Generated from /Users/mrf7578/Development/git/hsolbrig/pyjsg/grammar/jsgParser.g4 by ANTLR 4.7
# encoding: utf-8
from antlr4 import *
from io import StringIO
from typing import TextIO
import sys

def serializedATN():
    with StringIO() as buf:
        buf.write("\3\u608b\ua72a\u8133\ub9ed\u417c\u3be7\u7786\u5964\3\35")
        buf.write("\u0131\4\2\t\2\4\3\t\3\4\4\t\4\4\5\t\5\4\6\t\6\4\7\t\7")
        buf.write("\4\b\t\b\4\t\t\t\4\n\t\n\4\13\t\13\4\f\t\f\4\r\t\r\4\16")
        buf.write("\t\16\4\17\t\17\4\20\t\20\4\21\t\21\4\22\t\22\4\23\t\23")
        buf.write("\4\24\t\24\4\25\t\25\4\26\t\26\4\27\t\27\4\30\t\30\4\31")
        buf.write("\t\31\4\32\t\32\4\33\t\33\4\34\t\34\4\35\t\35\3\2\7\2")
        buf.write("<\n\2\f\2\16\2?\13\2\3\2\7\2B\n\2\f\2\16\2E\13\2\3\2\3")
        buf.write("\2\7\2I\n\2\f\2\16\2L\13\2\3\2\3\2\3\3\3\3\3\3\3\3\5\3")
        buf.write("T\n\3\3\3\3\3\3\3\7\3Y\n\3\f\3\16\3\\\13\3\3\3\5\3_\n")
        buf.write("\3\3\4\6\4b\n\4\r\4\16\4c\3\5\3\5\3\5\3\5\5\5j\n\5\3\6")
        buf.write("\3\6\3\6\3\7\3\7\5\7q\n\7\3\7\3\7\3\7\3\7\3\7\3\7\3\7")
        buf.write("\5\7z\n\7\3\b\6\b}\n\b\r\b\16\b~\3\b\3\b\7\b\u0083\n\b")
        buf.write("\f\b\16\b\u0086\13\b\3\t\7\t\u0089\n\t\f\t\16\t\u008c")
        buf.write("\13\t\3\n\3\n\3\n\3\13\3\13\3\13\3\13\7\13\u0095\n\13")
        buf.write("\f\13\16\13\u0098\13\13\3\13\5\13\u009b\n\13\3\13\3\13")
        buf.write("\3\f\3\f\5\f\u00a1\n\f\3\f\5\f\u00a4\n\f\3\f\3\f\5\f\u00a8")
        buf.write("\n\f\5\f\u00aa\n\f\3\r\3\r\3\r\3\r\5\r\u00b0\n\r\3\r\3")
        buf.write("\r\3\r\3\r\6\r\u00b6\n\r\r\r\16\r\u00b7\3\r\3\r\3\r\3")
        buf.write("\r\5\r\u00be\n\r\3\r\3\r\3\r\3\r\6\r\u00c4\n\r\r\r\16")
        buf.write("\r\u00c5\3\r\3\r\5\r\u00ca\n\r\3\16\6\16\u00cd\n\16\r")
        buf.write("\16\16\16\u00ce\3\17\3\17\3\17\3\17\3\17\3\17\3\17\3\17")
        buf.write("\3\17\3\17\5\17\u00db\n\17\3\20\3\20\3\20\6\20\u00e0\n")
        buf.write("\20\r\20\16\20\u00e1\3\21\3\21\3\21\3\21\3\21\3\22\3\22")
        buf.write("\3\22\3\22\3\22\3\23\3\23\3\23\7\23\u00f1\n\23\f\23\16")
        buf.write("\23\u00f4\13\23\3\24\3\24\3\24\3\24\3\24\3\24\3\24\5\24")
        buf.write("\u00fd\n\24\5\24\u00ff\n\24\3\24\5\24\u0102\n\24\3\25")
        buf.write("\3\25\3\25\3\25\3\25\3\26\3\26\3\27\3\27\3\27\7\27\u010e")
        buf.write("\n\27\f\27\16\27\u0111\13\27\3\30\3\30\5\30\u0115\n\30")
        buf.write("\3\31\6\31\u0118\n\31\r\31\16\31\u0119\3\32\3\32\5\32")
        buf.write("\u011e\n\32\3\32\3\32\5\32\u0122\n\32\5\32\u0124\n\32")
        buf.write("\3\33\3\33\3\33\3\33\3\34\3\34\3\34\5\34\u012d\n\34\3")
        buf.write("\35\3\35\3\35\2\2\36\2\4\6\b\n\f\16\20\22\24\26\30\32")
        buf.write("\34\36 \"$&(*,.\60\62\64\668\2\6\3\2\27\30\3\2\26\30\4")
        buf.write("\2\24\24\31\31\4\2\26\26\30\30\2\u0145\2=\3\2\2\2\4^\3")
        buf.write("\2\2\2\6a\3\2\2\2\bi\3\2\2\2\nk\3\2\2\2\fy\3\2\2\2\16")
        buf.write("|\3\2\2\2\20\u008a\3\2\2\2\22\u008d\3\2\2\2\24\u0090\3")
        buf.write("\2\2\2\26\u00a9\3\2\2\2\30\u00c9\3\2\2\2\32\u00cc\3\2")
        buf.write("\2\2\34\u00da\3\2\2\2\36\u00dc\3\2\2\2 \u00e3\3\2\2\2")
        buf.write("\"\u00e8\3\2\2\2$\u00ed\3\2\2\2&\u0101\3\2\2\2(\u0103")
        buf.write("\3\2\2\2*\u0108\3\2\2\2,\u010a\3\2\2\2.\u0114\3\2\2\2")
        buf.write("\60\u0117\3\2\2\2\62\u0123\3\2\2\2\64\u0125\3\2\2\2\66")
        buf.write("\u012c\3\2\2\28\u012e\3\2\2\2:<\5\4\3\2;:\3\2\2\2<?\3")
        buf.write("\2\2\2=;\3\2\2\2=>\3\2\2\2>C\3\2\2\2?=\3\2\2\2@B\5\b\5")
        buf.write("\2A@\3\2\2\2BE\3\2\2\2CA\3\2\2\2CD\3\2\2\2DF\3\2\2\2E")
        buf.write("C\3\2\2\2FJ\7\3\2\2GI\5(\25\2HG\3\2\2\2IL\3\2\2\2JH\3")
        buf.write("\2\2\2JK\3\2\2\2KM\3\2\2\2LJ\3\2\2\2MN\7\2\2\3N\3\3\2")
        buf.write("\2\2OP\7\4\2\2PS\7\27\2\2QR\7\5\2\2RT\5\6\4\2SQ\3\2\2")
        buf.write("\2ST\3\2\2\2TU\3\2\2\2U_\7\6\2\2VZ\7\7\2\2WY\7\27\2\2")
        buf.write("XW\3\2\2\2Y\\\3\2\2\2ZX\3\2\2\2Z[\3\2\2\2[]\3\2\2\2\\")
        buf.write("Z\3\2\2\2]_\7\6\2\2^O\3\2\2\2^V\3\2\2\2_\5\3\2\2\2`b\7")
        buf.write("\27\2\2a`\3\2\2\2bc\3\2\2\2ca\3\2\2\2cd\3\2\2\2d\7\3\2")
        buf.write("\2\2ej\5\n\6\2fj\5\22\n\2gj\5 \21\2hj\5\"\22\2ie\3\2\2")
        buf.write("\2if\3\2\2\2ig\3\2\2\2ih\3\2\2\2j\t\3\2\2\2kl\7\27\2\2")
        buf.write("lm\5\f\7\2m\13\3\2\2\2np\7\b\2\2oq\5\16\b\2po\3\2\2\2")
        buf.write("pq\3\2\2\2qr\3\2\2\2rz\7\t\2\2st\7\b\2\2tu\7\27\2\2uv")
        buf.write("\7\n\2\2vw\5\34\17\2wx\7\t\2\2xz\3\2\2\2yn\3\2\2\2ys\3")
        buf.write("\2\2\2z\r\3\2\2\2{}\5\26\f\2|{\3\2\2\2}~\3\2\2\2~|\3\2")
        buf.write("\2\2~\177\3\2\2\2\177\u0084\3\2\2\2\u0080\u0081\7\13\2")
        buf.write("\2\u0081\u0083\5\20\t\2\u0082\u0080\3\2\2\2\u0083\u0086")
        buf.write("\3\2\2\2\u0084\u0082\3\2\2\2\u0084\u0085\3\2\2\2\u0085")
        buf.write("\17\3\2\2\2\u0086\u0084\3\2\2\2\u0087\u0089\5\26\f\2\u0088")
        buf.write("\u0087\3\2\2\2\u0089\u008c\3\2\2\2\u008a\u0088\3\2\2\2")
        buf.write("\u008a\u008b\3\2\2\2\u008b\21\3\2\2\2\u008c\u008a\3\2")
        buf.write("\2\2\u008d\u008e\7\27\2\2\u008e\u008f\5\24\13\2\u008f")
        buf.write("\23\3\2\2\2\u0090\u0091\7\f\2\2\u0091\u0096\5\34\17\2")
        buf.write("\u0092\u0093\7\13\2\2\u0093\u0095\5\34\17\2\u0094\u0092")
        buf.write("\3\2\2\2\u0095\u0098\3\2\2\2\u0096\u0094\3\2\2\2\u0096")
        buf.write("\u0097\3\2\2\2\u0097\u009a\3\2\2\2\u0098\u0096\3\2\2\2")
        buf.write("\u0099\u009b\5&\24\2\u009a\u0099\3\2\2\2\u009a\u009b\3")
        buf.write("\2\2\2\u009b\u009c\3\2\2\2\u009c\u009d\7\r\2\2\u009d\25")
        buf.write("\3\2\2\2\u009e\u00a0\7\27\2\2\u009f\u00a1\5&\24\2\u00a0")
        buf.write("\u009f\3\2\2\2\u00a0\u00a1\3\2\2\2\u00a1\u00a3\3\2\2\2")
        buf.write("\u00a2\u00a4\7\16\2\2\u00a3\u00a2\3\2\2\2\u00a3\u00a4")
        buf.write("\3\2\2\2\u00a4\u00aa\3\2\2\2\u00a5\u00a7\5\30\r\2\u00a6")
        buf.write("\u00a8\7\16\2\2\u00a7\u00a6\3\2\2\2\u00a7\u00a8\3\2\2")
        buf.write("\2\u00a8\u00aa\3\2\2\2\u00a9\u009e\3\2\2\2\u00a9\u00a5")
        buf.write("\3\2\2\2\u00aa\27\3\2\2\2\u00ab\u00ac\t\2\2\2\u00ac\u00ad")
        buf.write("\7\17\2\2\u00ad\u00af\5\34\17\2\u00ae\u00b0\5&\24\2\u00af")
        buf.write("\u00ae\3\2\2\2\u00af\u00b0\3\2\2\2\u00b0\u00ca\3\2\2\2")
        buf.write("\u00b1\u00b2\7\20\2\2\u00b2\u00b5\7\27\2\2\u00b3\u00b4")
        buf.write("\7\13\2\2\u00b4\u00b6\7\27\2\2\u00b5\u00b3\3\2\2\2\u00b6")
        buf.write("\u00b7\3\2\2\2\u00b7\u00b5\3\2\2\2\u00b7\u00b8\3\2\2\2")
        buf.write("\u00b8\u00b9\3\2\2\2\u00b9\u00ba\7\21\2\2\u00ba\u00bb")
        buf.write("\7\17\2\2\u00bb\u00bd\5\34\17\2\u00bc\u00be\5&\24\2\u00bd")
        buf.write("\u00bc\3\2\2\2\u00bd\u00be\3\2\2\2\u00be\u00ca\3\2\2\2")
        buf.write("\u00bf\u00c0\7\20\2\2\u00c0\u00c3\5\32\16\2\u00c1\u00c2")
        buf.write("\7\13\2\2\u00c2\u00c4\5\32\16\2\u00c3\u00c1\3\2\2\2\u00c4")
        buf.write("\u00c5\3\2\2\2\u00c5\u00c3\3\2\2\2\u00c5\u00c6\3\2\2\2")
        buf.write("\u00c6\u00c7\3\2\2\2\u00c7\u00c8\7\21\2\2\u00c8\u00ca")
        buf.write("\3\2\2\2\u00c9\u00ab\3\2\2\2\u00c9\u00b1\3\2\2\2\u00c9")
        buf.write("\u00bf\3\2\2\2\u00ca\31\3\2\2\2\u00cb\u00cd\5\30\r\2\u00cc")
        buf.write("\u00cb\3\2\2\2\u00cd\u00ce\3\2\2\2\u00ce\u00cc\3\2\2\2")
        buf.write("\u00ce\u00cf\3\2\2\2\u00cf\33\3\2\2\2\u00d0\u00db\7\27")
        buf.write("\2\2\u00d1\u00db\7\26\2\2\u00d2\u00db\7\30\2\2\u00d3\u00db")
        buf.write("\5\f\7\2\u00d4\u00db\5\24\13\2\u00d5\u00d6\7\20\2\2\u00d6")
        buf.write("\u00d7\5\36\20\2\u00d7\u00d8\7\21\2\2\u00d8\u00db\3\2")
        buf.write("\2\2\u00d9\u00db\7\32\2\2\u00da\u00d0\3\2\2\2\u00da\u00d1")
        buf.write("\3\2\2\2\u00da\u00d2\3\2\2\2\u00da\u00d3\3\2\2\2\u00da")
        buf.write("\u00d4\3\2\2\2\u00da\u00d5\3\2\2\2\u00da\u00d9\3\2\2\2")
        buf.write("\u00db\35\3\2\2\2\u00dc\u00df\t\3\2\2\u00dd\u00de\7\13")
        buf.write("\2\2\u00de\u00e0\t\3\2\2\u00df\u00dd\3\2\2\2\u00e0\u00e1")
        buf.write("\3\2\2\2\u00e1\u00df\3\2\2\2\u00e1\u00e2\3\2\2\2\u00e2")
        buf.write("\37\3\2\2\2\u00e3\u00e4\7\27\2\2\u00e4\u00e5\7\22\2\2")
        buf.write("\u00e5\u00e6\5\16\b\2\u00e6\u00e7\7\6\2\2\u00e7!\3\2\2")
        buf.write("\2\u00e8\u00e9\7\27\2\2\u00e9\u00ea\7\22\2\2\u00ea\u00eb")
        buf.write("\5$\23\2\u00eb\u00ec\7\6\2\2\u00ec#\3\2\2\2\u00ed\u00f2")
        buf.write("\7\26\2\2\u00ee\u00ef\7\13\2\2\u00ef\u00f1\7\26\2\2\u00f0")
        buf.write("\u00ee\3\2\2\2\u00f1\u00f4\3\2\2\2\u00f2\u00f0\3\2\2\2")
        buf.write("\u00f2\u00f3\3\2\2\2\u00f3%\3\2\2\2\u00f4\u00f2\3\2\2")
        buf.write("\2\u00f5\u0102\7\23\2\2\u00f6\u0102\7\24\2\2\u00f7\u0102")
        buf.write("\7\25\2\2\u00f8\u00f9\7\b\2\2\u00f9\u00fe\7\31\2\2\u00fa")
        buf.write("\u00fc\7\16\2\2\u00fb\u00fd\t\4\2\2\u00fc\u00fb\3\2\2")
        buf.write("\2\u00fc\u00fd\3\2\2\2\u00fd\u00ff\3\2\2\2\u00fe\u00fa")
        buf.write("\3\2\2\2\u00fe\u00ff\3\2\2\2\u00ff\u0100\3\2\2\2\u0100")
        buf.write("\u0102\7\t\2\2\u0101\u00f5\3\2\2\2\u0101\u00f6\3\2\2\2")
        buf.write("\u0101\u00f7\3\2\2\2\u0101\u00f8\3\2\2\2\u0102\'\3\2\2")
        buf.write("\2\u0103\u0104\7\26\2\2\u0104\u0105\7\17\2\2\u0105\u0106")
        buf.write("\5*\26\2\u0106\u0107\7\6\2\2\u0107)\3\2\2\2\u0108\u0109")
        buf.write("\5,\27\2\u0109+\3\2\2\2\u010a\u010f\5.\30\2\u010b\u010c")
        buf.write("\7\13\2\2\u010c\u010e\5.\30\2\u010d\u010b\3\2\2\2\u010e")
        buf.write("\u0111\3\2\2\2\u010f\u010d\3\2\2\2\u010f\u0110\3\2\2\2")
        buf.write("\u0110-\3\2\2\2\u0111\u010f\3\2\2\2\u0112\u0115\5\60\31")
        buf.write("\2\u0113\u0115\3\2\2\2\u0114\u0112\3\2\2\2\u0114\u0113")
        buf.write("\3\2\2\2\u0115/\3\2\2\2\u0116\u0118\5\62\32\2\u0117\u0116")
        buf.write("\3\2\2\2\u0118\u0119\3\2\2\2\u0119\u0117\3\2\2\2\u0119")
        buf.write("\u011a\3\2\2\2\u011a\61\3\2\2\2\u011b\u011d\5\66\34\2")
        buf.write("\u011c\u011e\5&\24\2\u011d\u011c\3\2\2\2\u011d\u011e\3")
        buf.write("\2\2\2\u011e\u0124\3\2\2\2\u011f\u0121\5\64\33\2\u0120")
        buf.write("\u0122\5&\24\2\u0121\u0120\3\2\2\2\u0121\u0122\3\2\2\2")
        buf.write("\u0122\u0124\3\2\2\2\u0123\u011b\3\2\2\2\u0123\u011f\3")
        buf.write("\2\2\2\u0124\63\3\2\2\2\u0125\u0126\7\20\2\2\u0126\u0127")
        buf.write("\5,\27\2\u0127\u0128\7\21\2\2\u0128\65\3\2\2\2\u0129\u012d")
        buf.write("\58\35\2\u012a\u012d\7\35\2\2\u012b\u012d\7\32\2\2\u012c")
        buf.write("\u0129\3\2\2\2\u012c\u012a\3\2\2\2\u012c\u012b\3\2\2\2")
        buf.write("\u012d\67\3\2\2\2\u012e\u012f\t\5\2\2\u012f9\3\2\2\2(")
        buf.write("=CJSZ^cipy~\u0084\u008a\u0096\u009a\u00a0\u00a3\u00a7")
        buf.write("\u00a9\u00af\u00b7\u00bd\u00c5\u00c9\u00ce\u00da\u00e1")
        buf.write("\u00f2\u00fc\u00fe\u0101\u010f\u0114\u0119\u011d\u0121")
        buf.write("\u0123\u012c")
        return buf.getvalue()


class jsgParserParser ( Parser ):

    grammarFileName = "jsgParser.g4"

    atn = ATNDeserializer().deserialize(serializedATN())

    decisionsToDFA = [ DFA(ds, i) for i, ds in enumerate(atn.decisionToState) ]

    sharedContextCache = PredictionContextCache()

    literalNames = [ "<INVALID>", "'@terminals'", "'.TYPE'", "'-'", "';'", 
                     "'.IGNORE'", "'{'", "'}'", "'->'", "'|'", "'['", "']'", 
                     "','", "':'", "'('", "')'", "'='", "'?'", "'*'", "'+'", 
                     "<INVALID>", "<INVALID>", "<INVALID>", "<INVALID>", 
                     "'.'" ]

    symbolicNames = [ "<INVALID>", "<INVALID>", "<INVALID>", "<INVALID>", 
                      "<INVALID>", "<INVALID>", "<INVALID>", "<INVALID>", 
                      "<INVALID>", "<INVALID>", "<INVALID>", "<INVALID>", 
                      "<INVALID>", "<INVALID>", "<INVALID>", "<INVALID>", 
                      "<INVALID>", "<INVALID>", "<INVALID>", "<INVALID>", 
                      "LEXER_ID", "ID", "STRING", "INT", "ANY", "PASS", 
                      "COMMENT", "LEXER_CHAR_SET" ]

    RULE_doc = 0
    RULE_directive = 1
    RULE_typeExceptions = 2
    RULE_grammarElt = 3
    RULE_objectDef = 4
    RULE_objectExpr = 5
    RULE_objectExprDef = 6
    RULE_particleOpt = 7
    RULE_arrayDef = 8
    RULE_arrayExpr = 9
    RULE_particle = 10
    RULE_propertyOrGroup = 11
    RULE_propertyOrGroupList = 12
    RULE_propertyType = 13
    RULE_typeAlternatives = 14
    RULE_objectAlias = 15
    RULE_lexerAlias = 16
    RULE_lexerAliasDef = 17
    RULE_ebnfSuffix = 18
    RULE_lexerRuleSpec = 19
    RULE_lexerRuleBlock = 20
    RULE_lexerAltList = 21
    RULE_lexerAlt = 22
    RULE_lexerElements = 23
    RULE_lexerElement = 24
    RULE_lexerBlock = 25
    RULE_lexerAtom = 26
    RULE_terminal = 27

    ruleNames =  [ "doc", "directive", "typeExceptions", "grammarElt", "objectDef", 
                   "objectExpr", "objectExprDef", "particleOpt", "arrayDef", 
                   "arrayExpr", "particle", "propertyOrGroup", "propertyOrGroupList", 
                   "propertyType", "typeAlternatives", "objectAlias", "lexerAlias", 
                   "lexerAliasDef", "ebnfSuffix", "lexerRuleSpec", "lexerRuleBlock", 
                   "lexerAltList", "lexerAlt", "lexerElements", "lexerElement", 
                   "lexerBlock", "lexerAtom", "terminal" ]

    EOF = Token.EOF
    T__0=1
    T__1=2
    T__2=3
    T__3=4
    T__4=5
    T__5=6
    T__6=7
    T__7=8
    T__8=9
    T__9=10
    T__10=11
    T__11=12
    T__12=13
    T__13=14
    T__14=15
    T__15=16
    T__16=17
    T__17=18
    T__18=19
    LEXER_ID=20
    ID=21
    STRING=22
    INT=23
    ANY=24
    PASS=25
    COMMENT=26
    LEXER_CHAR_SET=27

    def __init__(self, input:TokenStream, output:TextIO = sys.stdout):
        super().__init__(input, output)
        self.checkVersion("4.7")
        self._interp = ParserATNSimulator(self, self.atn, self.decisionsToDFA, self.sharedContextCache)
        self._predicates = None



    class DocContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def EOF(self):
            return self.getToken(jsgParserParser.EOF, 0)

        def directive(self, i:int=None):
            if i is None:
                return self.getTypedRuleContexts(jsgParserParser.DirectiveContext)
            else:
                return self.getTypedRuleContext(jsgParserParser.DirectiveContext,i)


        def grammarElt(self, i:int=None):
            if i is None:
                return self.getTypedRuleContexts(jsgParserParser.GrammarEltContext)
            else:
                return self.getTypedRuleContext(jsgParserParser.GrammarEltContext,i)


        def lexerRuleSpec(self, i:int=None):
            if i is None:
                return self.getTypedRuleContexts(jsgParserParser.LexerRuleSpecContext)
            else:
                return self.getTypedRuleContext(jsgParserParser.LexerRuleSpecContext,i)


        def getRuleIndex(self):
            return jsgParserParser.RULE_doc

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitDoc" ):
                return visitor.visitDoc(self)
            else:
                return visitor.visitChildren(self)




    def doc(self):

        localctx = jsgParserParser.DocContext(self, self._ctx, self.state)
        self.enterRule(localctx, 0, self.RULE_doc)
        self._la = 0 # Token type
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 59
            self._errHandler.sync(self)
            _la = self._input.LA(1)
            while _la==jsgParserParser.T__1 or _la==jsgParserParser.T__4:
                self.state = 56
                self.directive()
                self.state = 61
                self._errHandler.sync(self)
                _la = self._input.LA(1)

            self.state = 65
            self._errHandler.sync(self)
            _la = self._input.LA(1)
            while _la==jsgParserParser.ID:
                self.state = 62
                self.grammarElt()
                self.state = 67
                self._errHandler.sync(self)
                _la = self._input.LA(1)

            self.state = 68
            self.match(jsgParserParser.T__0)
            self.state = 72
            self._errHandler.sync(self)
            _la = self._input.LA(1)
            while _la==jsgParserParser.LEXER_ID:
                self.state = 69
                self.lexerRuleSpec()
                self.state = 74
                self._errHandler.sync(self)
                _la = self._input.LA(1)

            self.state = 75
            self.match(jsgParserParser.EOF)
        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class DirectiveContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser


        def getRuleIndex(self):
            return jsgParserParser.RULE_directive

     
        def copyFrom(self, ctx:ParserRuleContext):
            super().copyFrom(ctx)



    class TypeDirectiveContext(DirectiveContext):

        def __init__(self, parser, ctx:ParserRuleContext): # actually a jsgParserParser.DirectiveContext
            super().__init__(parser)
            self.copyFrom(ctx)

        def ID(self):
            return self.getToken(jsgParserParser.ID, 0)
        def typeExceptions(self):
            return self.getTypedRuleContext(jsgParserParser.TypeExceptionsContext,0)


        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitTypeDirective" ):
                return visitor.visitTypeDirective(self)
            else:
                return visitor.visitChildren(self)


    class IgnoreDirectiveContext(DirectiveContext):

        def __init__(self, parser, ctx:ParserRuleContext): # actually a jsgParserParser.DirectiveContext
            super().__init__(parser)
            self.copyFrom(ctx)

        def ID(self, i:int=None):
            if i is None:
                return self.getTokens(jsgParserParser.ID)
            else:
                return self.getToken(jsgParserParser.ID, i)

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitIgnoreDirective" ):
                return visitor.visitIgnoreDirective(self)
            else:
                return visitor.visitChildren(self)



    def directive(self):

        localctx = jsgParserParser.DirectiveContext(self, self._ctx, self.state)
        self.enterRule(localctx, 2, self.RULE_directive)
        self._la = 0 # Token type
        try:
            self.state = 92
            self._errHandler.sync(self)
            token = self._input.LA(1)
            if token in [jsgParserParser.T__1]:
                localctx = jsgParserParser.TypeDirectiveContext(self, localctx)
                self.enterOuterAlt(localctx, 1)
                self.state = 77
                self.match(jsgParserParser.T__1)
                self.state = 78
                self.match(jsgParserParser.ID)
                self.state = 81
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                if _la==jsgParserParser.T__2:
                    self.state = 79
                    self.match(jsgParserParser.T__2)
                    self.state = 80
                    self.typeExceptions()


                self.state = 83
                self.match(jsgParserParser.T__3)
                pass
            elif token in [jsgParserParser.T__4]:
                localctx = jsgParserParser.IgnoreDirectiveContext(self, localctx)
                self.enterOuterAlt(localctx, 2)
                self.state = 84
                self.match(jsgParserParser.T__4)
                self.state = 88
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                while _la==jsgParserParser.ID:
                    self.state = 85
                    self.match(jsgParserParser.ID)
                    self.state = 90
                    self._errHandler.sync(self)
                    _la = self._input.LA(1)

                self.state = 91
                self.match(jsgParserParser.T__3)
                pass
            else:
                raise NoViableAltException(self)

        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class TypeExceptionsContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def ID(self, i:int=None):
            if i is None:
                return self.getTokens(jsgParserParser.ID)
            else:
                return self.getToken(jsgParserParser.ID, i)

        def getRuleIndex(self):
            return jsgParserParser.RULE_typeExceptions

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitTypeExceptions" ):
                return visitor.visitTypeExceptions(self)
            else:
                return visitor.visitChildren(self)




    def typeExceptions(self):

        localctx = jsgParserParser.TypeExceptionsContext(self, self._ctx, self.state)
        self.enterRule(localctx, 4, self.RULE_typeExceptions)
        self._la = 0 # Token type
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 95 
            self._errHandler.sync(self)
            _la = self._input.LA(1)
            while True:
                self.state = 94
                self.match(jsgParserParser.ID)
                self.state = 97 
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                if not (_la==jsgParserParser.ID):
                    break

        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class GrammarEltContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def objectDef(self):
            return self.getTypedRuleContext(jsgParserParser.ObjectDefContext,0)


        def arrayDef(self):
            return self.getTypedRuleContext(jsgParserParser.ArrayDefContext,0)


        def objectAlias(self):
            return self.getTypedRuleContext(jsgParserParser.ObjectAliasContext,0)


        def lexerAlias(self):
            return self.getTypedRuleContext(jsgParserParser.LexerAliasContext,0)


        def getRuleIndex(self):
            return jsgParserParser.RULE_grammarElt

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitGrammarElt" ):
                return visitor.visitGrammarElt(self)
            else:
                return visitor.visitChildren(self)




    def grammarElt(self):

        localctx = jsgParserParser.GrammarEltContext(self, self._ctx, self.state)
        self.enterRule(localctx, 6, self.RULE_grammarElt)
        try:
            self.state = 103
            self._errHandler.sync(self)
            la_ = self._interp.adaptivePredict(self._input,7,self._ctx)
            if la_ == 1:
                self.enterOuterAlt(localctx, 1)
                self.state = 99
                self.objectDef()
                pass

            elif la_ == 2:
                self.enterOuterAlt(localctx, 2)
                self.state = 100
                self.arrayDef()
                pass

            elif la_ == 3:
                self.enterOuterAlt(localctx, 3)
                self.state = 101
                self.objectAlias()
                pass

            elif la_ == 4:
                self.enterOuterAlt(localctx, 4)
                self.state = 102
                self.lexerAlias()
                pass


        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class ObjectDefContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def ID(self):
            return self.getToken(jsgParserParser.ID, 0)

        def objectExpr(self):
            return self.getTypedRuleContext(jsgParserParser.ObjectExprContext,0)


        def getRuleIndex(self):
            return jsgParserParser.RULE_objectDef

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitObjectDef" ):
                return visitor.visitObjectDef(self)
            else:
                return visitor.visitChildren(self)




    def objectDef(self):

        localctx = jsgParserParser.ObjectDefContext(self, self._ctx, self.state)
        self.enterRule(localctx, 8, self.RULE_objectDef)
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 105
            self.match(jsgParserParser.ID)
            self.state = 106
            self.objectExpr()
        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class ObjectExprContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser


        def getRuleIndex(self):
            return jsgParserParser.RULE_objectExpr

     
        def copyFrom(self, ctx:ParserRuleContext):
            super().copyFrom(ctx)



    class ObjectExprMapContext(ObjectExprContext):

        def __init__(self, parser, ctx:ParserRuleContext): # actually a jsgParserParser.ObjectExprContext
            super().__init__(parser)
            self.copyFrom(ctx)

        def ID(self):
            return self.getToken(jsgParserParser.ID, 0)
        def propertyType(self):
            return self.getTypedRuleContext(jsgParserParser.PropertyTypeContext,0)


        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitObjectExprMap" ):
                return visitor.visitObjectExprMap(self)
            else:
                return visitor.visitChildren(self)


    class ObjectExprObjContext(ObjectExprContext):

        def __init__(self, parser, ctx:ParserRuleContext): # actually a jsgParserParser.ObjectExprContext
            super().__init__(parser)
            self.copyFrom(ctx)

        def objectExprDef(self):
            return self.getTypedRuleContext(jsgParserParser.ObjectExprDefContext,0)


        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitObjectExprObj" ):
                return visitor.visitObjectExprObj(self)
            else:
                return visitor.visitChildren(self)



    def objectExpr(self):

        localctx = jsgParserParser.ObjectExprContext(self, self._ctx, self.state)
        self.enterRule(localctx, 10, self.RULE_objectExpr)
        self._la = 0 # Token type
        try:
            self.state = 119
            self._errHandler.sync(self)
            la_ = self._interp.adaptivePredict(self._input,9,self._ctx)
            if la_ == 1:
                localctx = jsgParserParser.ObjectExprObjContext(self, localctx)
                self.enterOuterAlt(localctx, 1)
                self.state = 108
                self.match(jsgParserParser.T__5)
                self.state = 110
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                if (((_la) & ~0x3f) == 0 and ((1 << _la) & ((1 << jsgParserParser.T__13) | (1 << jsgParserParser.ID) | (1 << jsgParserParser.STRING))) != 0):
                    self.state = 109
                    self.objectExprDef()


                self.state = 112
                self.match(jsgParserParser.T__6)
                pass

            elif la_ == 2:
                localctx = jsgParserParser.ObjectExprMapContext(self, localctx)
                self.enterOuterAlt(localctx, 2)
                self.state = 113
                self.match(jsgParserParser.T__5)
                self.state = 114
                self.match(jsgParserParser.ID)
                self.state = 115
                self.match(jsgParserParser.T__7)
                self.state = 116
                self.propertyType()
                self.state = 117
                self.match(jsgParserParser.T__6)
                pass


        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class ObjectExprDefContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def particle(self, i:int=None):
            if i is None:
                return self.getTypedRuleContexts(jsgParserParser.ParticleContext)
            else:
                return self.getTypedRuleContext(jsgParserParser.ParticleContext,i)


        def particleOpt(self, i:int=None):
            if i is None:
                return self.getTypedRuleContexts(jsgParserParser.ParticleOptContext)
            else:
                return self.getTypedRuleContext(jsgParserParser.ParticleOptContext,i)


        def getRuleIndex(self):
            return jsgParserParser.RULE_objectExprDef

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitObjectExprDef" ):
                return visitor.visitObjectExprDef(self)
            else:
                return visitor.visitChildren(self)




    def objectExprDef(self):

        localctx = jsgParserParser.ObjectExprDefContext(self, self._ctx, self.state)
        self.enterRule(localctx, 12, self.RULE_objectExprDef)
        self._la = 0 # Token type
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 122 
            self._errHandler.sync(self)
            _la = self._input.LA(1)
            while True:
                self.state = 121
                self.particle()
                self.state = 124 
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                if not ((((_la) & ~0x3f) == 0 and ((1 << _la) & ((1 << jsgParserParser.T__13) | (1 << jsgParserParser.ID) | (1 << jsgParserParser.STRING))) != 0)):
                    break

            self.state = 130
            self._errHandler.sync(self)
            _la = self._input.LA(1)
            while _la==jsgParserParser.T__8:
                self.state = 126
                self.match(jsgParserParser.T__8)
                self.state = 127
                self.particleOpt()
                self.state = 132
                self._errHandler.sync(self)
                _la = self._input.LA(1)

        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class ParticleOptContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def particle(self, i:int=None):
            if i is None:
                return self.getTypedRuleContexts(jsgParserParser.ParticleContext)
            else:
                return self.getTypedRuleContext(jsgParserParser.ParticleContext,i)


        def getRuleIndex(self):
            return jsgParserParser.RULE_particleOpt

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitParticleOpt" ):
                return visitor.visitParticleOpt(self)
            else:
                return visitor.visitChildren(self)




    def particleOpt(self):

        localctx = jsgParserParser.ParticleOptContext(self, self._ctx, self.state)
        self.enterRule(localctx, 14, self.RULE_particleOpt)
        self._la = 0 # Token type
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 136
            self._errHandler.sync(self)
            _la = self._input.LA(1)
            while (((_la) & ~0x3f) == 0 and ((1 << _la) & ((1 << jsgParserParser.T__13) | (1 << jsgParserParser.ID) | (1 << jsgParserParser.STRING))) != 0):
                self.state = 133
                self.particle()
                self.state = 138
                self._errHandler.sync(self)
                _la = self._input.LA(1)

        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class ArrayDefContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def ID(self):
            return self.getToken(jsgParserParser.ID, 0)

        def arrayExpr(self):
            return self.getTypedRuleContext(jsgParserParser.ArrayExprContext,0)


        def getRuleIndex(self):
            return jsgParserParser.RULE_arrayDef

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitArrayDef" ):
                return visitor.visitArrayDef(self)
            else:
                return visitor.visitChildren(self)




    def arrayDef(self):

        localctx = jsgParserParser.ArrayDefContext(self, self._ctx, self.state)
        self.enterRule(localctx, 16, self.RULE_arrayDef)
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 139
            self.match(jsgParserParser.ID)
            self.state = 140
            self.arrayExpr()
        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class ArrayExprContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def propertyType(self, i:int=None):
            if i is None:
                return self.getTypedRuleContexts(jsgParserParser.PropertyTypeContext)
            else:
                return self.getTypedRuleContext(jsgParserParser.PropertyTypeContext,i)


        def ebnfSuffix(self):
            return self.getTypedRuleContext(jsgParserParser.EbnfSuffixContext,0)


        def getRuleIndex(self):
            return jsgParserParser.RULE_arrayExpr

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitArrayExpr" ):
                return visitor.visitArrayExpr(self)
            else:
                return visitor.visitChildren(self)




    def arrayExpr(self):

        localctx = jsgParserParser.ArrayExprContext(self, self._ctx, self.state)
        self.enterRule(localctx, 18, self.RULE_arrayExpr)
        self._la = 0 # Token type
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 142
            self.match(jsgParserParser.T__9)
            self.state = 143
            self.propertyType()
            self.state = 148
            self._errHandler.sync(self)
            _la = self._input.LA(1)
            while _la==jsgParserParser.T__8:
                self.state = 144
                self.match(jsgParserParser.T__8)
                self.state = 145
                self.propertyType()
                self.state = 150
                self._errHandler.sync(self)
                _la = self._input.LA(1)

            self.state = 152
            self._errHandler.sync(self)
            _la = self._input.LA(1)
            if (((_la) & ~0x3f) == 0 and ((1 << _la) & ((1 << jsgParserParser.T__5) | (1 << jsgParserParser.T__16) | (1 << jsgParserParser.T__17) | (1 << jsgParserParser.T__18))) != 0):
                self.state = 151
                self.ebnfSuffix()


            self.state = 154
            self.match(jsgParserParser.T__10)
        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class ParticleContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def ID(self):
            return self.getToken(jsgParserParser.ID, 0)

        def ebnfSuffix(self):
            return self.getTypedRuleContext(jsgParserParser.EbnfSuffixContext,0)


        def propertyOrGroup(self):
            return self.getTypedRuleContext(jsgParserParser.PropertyOrGroupContext,0)


        def getRuleIndex(self):
            return jsgParserParser.RULE_particle

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitParticle" ):
                return visitor.visitParticle(self)
            else:
                return visitor.visitChildren(self)




    def particle(self):

        localctx = jsgParserParser.ParticleContext(self, self._ctx, self.state)
        self.enterRule(localctx, 20, self.RULE_particle)
        self._la = 0 # Token type
        try:
            self.state = 167
            self._errHandler.sync(self)
            la_ = self._interp.adaptivePredict(self._input,18,self._ctx)
            if la_ == 1:
                self.enterOuterAlt(localctx, 1)
                self.state = 156
                self.match(jsgParserParser.ID)
                self.state = 158
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                if (((_la) & ~0x3f) == 0 and ((1 << _la) & ((1 << jsgParserParser.T__5) | (1 << jsgParserParser.T__16) | (1 << jsgParserParser.T__17) | (1 << jsgParserParser.T__18))) != 0):
                    self.state = 157
                    self.ebnfSuffix()


                self.state = 161
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                if _la==jsgParserParser.T__11:
                    self.state = 160
                    self.match(jsgParserParser.T__11)


                pass

            elif la_ == 2:
                self.enterOuterAlt(localctx, 2)
                self.state = 163
                self.propertyOrGroup()
                self.state = 165
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                if _la==jsgParserParser.T__11:
                    self.state = 164
                    self.match(jsgParserParser.T__11)


                pass


        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class PropertyOrGroupContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser


        def getRuleIndex(self):
            return jsgParserParser.RULE_propertyOrGroup

     
        def copyFrom(self, ctx:ParserRuleContext):
            super().copyFrom(ctx)



    class PropertyOrGroupChoiceContext(PropertyOrGroupContext):

        def __init__(self, parser, ctx:ParserRuleContext): # actually a jsgParserParser.PropertyOrGroupContext
            super().__init__(parser)
            self.copyFrom(ctx)

        def propertyOrGroupList(self, i:int=None):
            if i is None:
                return self.getTypedRuleContexts(jsgParserParser.PropertyOrGroupListContext)
            else:
                return self.getTypedRuleContext(jsgParserParser.PropertyOrGroupListContext,i)


        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitPropertyOrGroupChoice" ):
                return visitor.visitPropertyOrGroupChoice(self)
            else:
                return visitor.visitChildren(self)


    class PropertyOrGroupShorthandContext(PropertyOrGroupContext):

        def __init__(self, parser, ctx:ParserRuleContext): # actually a jsgParserParser.PropertyOrGroupContext
            super().__init__(parser)
            self.copyFrom(ctx)

        def ID(self, i:int=None):
            if i is None:
                return self.getTokens(jsgParserParser.ID)
            else:
                return self.getToken(jsgParserParser.ID, i)
        def propertyType(self):
            return self.getTypedRuleContext(jsgParserParser.PropertyTypeContext,0)

        def ebnfSuffix(self):
            return self.getTypedRuleContext(jsgParserParser.EbnfSuffixContext,0)


        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitPropertyOrGroupShorthand" ):
                return visitor.visitPropertyOrGroupShorthand(self)
            else:
                return visitor.visitChildren(self)


    class PropertyOrGroupSimpleContext(PropertyOrGroupContext):

        def __init__(self, parser, ctx:ParserRuleContext): # actually a jsgParserParser.PropertyOrGroupContext
            super().__init__(parser)
            self.copyFrom(ctx)

        def propertyType(self):
            return self.getTypedRuleContext(jsgParserParser.PropertyTypeContext,0)

        def ID(self):
            return self.getToken(jsgParserParser.ID, 0)
        def STRING(self):
            return self.getToken(jsgParserParser.STRING, 0)
        def ebnfSuffix(self):
            return self.getTypedRuleContext(jsgParserParser.EbnfSuffixContext,0)


        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitPropertyOrGroupSimple" ):
                return visitor.visitPropertyOrGroupSimple(self)
            else:
                return visitor.visitChildren(self)



    def propertyOrGroup(self):

        localctx = jsgParserParser.PropertyOrGroupContext(self, self._ctx, self.state)
        self.enterRule(localctx, 22, self.RULE_propertyOrGroup)
        self._la = 0 # Token type
        try:
            self.state = 199
            self._errHandler.sync(self)
            la_ = self._interp.adaptivePredict(self._input,23,self._ctx)
            if la_ == 1:
                localctx = jsgParserParser.PropertyOrGroupSimpleContext(self, localctx)
                self.enterOuterAlt(localctx, 1)
                self.state = 169
                _la = self._input.LA(1)
                if not(_la==jsgParserParser.ID or _la==jsgParserParser.STRING):
                    self._errHandler.recoverInline(self)
                else:
                    self._errHandler.reportMatch(self)
                    self.consume()
                self.state = 170
                self.match(jsgParserParser.T__12)
                self.state = 171
                self.propertyType()
                self.state = 173
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                if (((_la) & ~0x3f) == 0 and ((1 << _la) & ((1 << jsgParserParser.T__5) | (1 << jsgParserParser.T__16) | (1 << jsgParserParser.T__17) | (1 << jsgParserParser.T__18))) != 0):
                    self.state = 172
                    self.ebnfSuffix()


                pass

            elif la_ == 2:
                localctx = jsgParserParser.PropertyOrGroupShorthandContext(self, localctx)
                self.enterOuterAlt(localctx, 2)
                self.state = 175
                self.match(jsgParserParser.T__13)
                self.state = 176
                self.match(jsgParserParser.ID)
                self.state = 179 
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                while True:
                    self.state = 177
                    self.match(jsgParserParser.T__8)
                    self.state = 178
                    self.match(jsgParserParser.ID)
                    self.state = 181 
                    self._errHandler.sync(self)
                    _la = self._input.LA(1)
                    if not (_la==jsgParserParser.T__8):
                        break

                self.state = 183
                self.match(jsgParserParser.T__14)
                self.state = 184
                self.match(jsgParserParser.T__12)
                self.state = 185
                self.propertyType()
                self.state = 187
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                if (((_la) & ~0x3f) == 0 and ((1 << _la) & ((1 << jsgParserParser.T__5) | (1 << jsgParserParser.T__16) | (1 << jsgParserParser.T__17) | (1 << jsgParserParser.T__18))) != 0):
                    self.state = 186
                    self.ebnfSuffix()


                pass

            elif la_ == 3:
                localctx = jsgParserParser.PropertyOrGroupChoiceContext(self, localctx)
                self.enterOuterAlt(localctx, 3)
                self.state = 189
                self.match(jsgParserParser.T__13)
                self.state = 190
                self.propertyOrGroupList()
                self.state = 193 
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                while True:
                    self.state = 191
                    self.match(jsgParserParser.T__8)
                    self.state = 192
                    self.propertyOrGroupList()
                    self.state = 195 
                    self._errHandler.sync(self)
                    _la = self._input.LA(1)
                    if not (_la==jsgParserParser.T__8):
                        break

                self.state = 197
                self.match(jsgParserParser.T__14)
                pass


        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class PropertyOrGroupListContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def propertyOrGroup(self, i:int=None):
            if i is None:
                return self.getTypedRuleContexts(jsgParserParser.PropertyOrGroupContext)
            else:
                return self.getTypedRuleContext(jsgParserParser.PropertyOrGroupContext,i)


        def getRuleIndex(self):
            return jsgParserParser.RULE_propertyOrGroupList

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitPropertyOrGroupList" ):
                return visitor.visitPropertyOrGroupList(self)
            else:
                return visitor.visitChildren(self)




    def propertyOrGroupList(self):

        localctx = jsgParserParser.PropertyOrGroupListContext(self, self._ctx, self.state)
        self.enterRule(localctx, 24, self.RULE_propertyOrGroupList)
        self._la = 0 # Token type
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 202 
            self._errHandler.sync(self)
            _la = self._input.LA(1)
            while True:
                self.state = 201
                self.propertyOrGroup()
                self.state = 204 
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                if not ((((_la) & ~0x3f) == 0 and ((1 << _la) & ((1 << jsgParserParser.T__13) | (1 << jsgParserParser.ID) | (1 << jsgParserParser.STRING))) != 0)):
                    break

        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class PropertyTypeContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def ID(self):
            return self.getToken(jsgParserParser.ID, 0)

        def LEXER_ID(self):
            return self.getToken(jsgParserParser.LEXER_ID, 0)

        def STRING(self):
            return self.getToken(jsgParserParser.STRING, 0)

        def objectExpr(self):
            return self.getTypedRuleContext(jsgParserParser.ObjectExprContext,0)


        def arrayExpr(self):
            return self.getTypedRuleContext(jsgParserParser.ArrayExprContext,0)


        def typeAlternatives(self):
            return self.getTypedRuleContext(jsgParserParser.TypeAlternativesContext,0)


        def ANY(self):
            return self.getToken(jsgParserParser.ANY, 0)

        def getRuleIndex(self):
            return jsgParserParser.RULE_propertyType

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitPropertyType" ):
                return visitor.visitPropertyType(self)
            else:
                return visitor.visitChildren(self)




    def propertyType(self):

        localctx = jsgParserParser.PropertyTypeContext(self, self._ctx, self.state)
        self.enterRule(localctx, 26, self.RULE_propertyType)
        try:
            self.state = 216
            self._errHandler.sync(self)
            token = self._input.LA(1)
            if token in [jsgParserParser.ID]:
                self.enterOuterAlt(localctx, 1)
                self.state = 206
                self.match(jsgParserParser.ID)
                pass
            elif token in [jsgParserParser.LEXER_ID]:
                self.enterOuterAlt(localctx, 2)
                self.state = 207
                self.match(jsgParserParser.LEXER_ID)
                pass
            elif token in [jsgParserParser.STRING]:
                self.enterOuterAlt(localctx, 3)
                self.state = 208
                self.match(jsgParserParser.STRING)
                pass
            elif token in [jsgParserParser.T__5]:
                self.enterOuterAlt(localctx, 4)
                self.state = 209
                self.objectExpr()
                pass
            elif token in [jsgParserParser.T__9]:
                self.enterOuterAlt(localctx, 5)
                self.state = 210
                self.arrayExpr()
                pass
            elif token in [jsgParserParser.T__13]:
                self.enterOuterAlt(localctx, 6)
                self.state = 211
                self.match(jsgParserParser.T__13)
                self.state = 212
                self.typeAlternatives()
                self.state = 213
                self.match(jsgParserParser.T__14)
                pass
            elif token in [jsgParserParser.ANY]:
                self.enterOuterAlt(localctx, 7)
                self.state = 215
                self.match(jsgParserParser.ANY)
                pass
            else:
                raise NoViableAltException(self)

        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class TypeAlternativesContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def LEXER_ID(self, i:int=None):
            if i is None:
                return self.getTokens(jsgParserParser.LEXER_ID)
            else:
                return self.getToken(jsgParserParser.LEXER_ID, i)

        def ID(self, i:int=None):
            if i is None:
                return self.getTokens(jsgParserParser.ID)
            else:
                return self.getToken(jsgParserParser.ID, i)

        def STRING(self, i:int=None):
            if i is None:
                return self.getTokens(jsgParserParser.STRING)
            else:
                return self.getToken(jsgParserParser.STRING, i)

        def getRuleIndex(self):
            return jsgParserParser.RULE_typeAlternatives

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitTypeAlternatives" ):
                return visitor.visitTypeAlternatives(self)
            else:
                return visitor.visitChildren(self)




    def typeAlternatives(self):

        localctx = jsgParserParser.TypeAlternativesContext(self, self._ctx, self.state)
        self.enterRule(localctx, 28, self.RULE_typeAlternatives)
        self._la = 0 # Token type
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 218
            _la = self._input.LA(1)
            if not((((_la) & ~0x3f) == 0 and ((1 << _la) & ((1 << jsgParserParser.LEXER_ID) | (1 << jsgParserParser.ID) | (1 << jsgParserParser.STRING))) != 0)):
                self._errHandler.recoverInline(self)
            else:
                self._errHandler.reportMatch(self)
                self.consume()
            self.state = 221 
            self._errHandler.sync(self)
            _la = self._input.LA(1)
            while True:
                self.state = 219
                self.match(jsgParserParser.T__8)
                self.state = 220
                _la = self._input.LA(1)
                if not((((_la) & ~0x3f) == 0 and ((1 << _la) & ((1 << jsgParserParser.LEXER_ID) | (1 << jsgParserParser.ID) | (1 << jsgParserParser.STRING))) != 0)):
                    self._errHandler.recoverInline(self)
                else:
                    self._errHandler.reportMatch(self)
                    self.consume()
                self.state = 223 
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                if not (_la==jsgParserParser.T__8):
                    break

        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class ObjectAliasContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def ID(self):
            return self.getToken(jsgParserParser.ID, 0)

        def objectExprDef(self):
            return self.getTypedRuleContext(jsgParserParser.ObjectExprDefContext,0)


        def getRuleIndex(self):
            return jsgParserParser.RULE_objectAlias

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitObjectAlias" ):
                return visitor.visitObjectAlias(self)
            else:
                return visitor.visitChildren(self)




    def objectAlias(self):

        localctx = jsgParserParser.ObjectAliasContext(self, self._ctx, self.state)
        self.enterRule(localctx, 30, self.RULE_objectAlias)
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 225
            self.match(jsgParserParser.ID)
            self.state = 226
            self.match(jsgParserParser.T__15)
            self.state = 227
            self.objectExprDef()
            self.state = 228
            self.match(jsgParserParser.T__3)
        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class LexerAliasContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def ID(self):
            return self.getToken(jsgParserParser.ID, 0)

        def lexerAliasDef(self):
            return self.getTypedRuleContext(jsgParserParser.LexerAliasDefContext,0)


        def getRuleIndex(self):
            return jsgParserParser.RULE_lexerAlias

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitLexerAlias" ):
                return visitor.visitLexerAlias(self)
            else:
                return visitor.visitChildren(self)




    def lexerAlias(self):

        localctx = jsgParserParser.LexerAliasContext(self, self._ctx, self.state)
        self.enterRule(localctx, 32, self.RULE_lexerAlias)
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 230
            self.match(jsgParserParser.ID)
            self.state = 231
            self.match(jsgParserParser.T__15)
            self.state = 232
            self.lexerAliasDef()
            self.state = 233
            self.match(jsgParserParser.T__3)
        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class LexerAliasDefContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def LEXER_ID(self, i:int=None):
            if i is None:
                return self.getTokens(jsgParserParser.LEXER_ID)
            else:
                return self.getToken(jsgParserParser.LEXER_ID, i)

        def getRuleIndex(self):
            return jsgParserParser.RULE_lexerAliasDef

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitLexerAliasDef" ):
                return visitor.visitLexerAliasDef(self)
            else:
                return visitor.visitChildren(self)




    def lexerAliasDef(self):

        localctx = jsgParserParser.LexerAliasDefContext(self, self._ctx, self.state)
        self.enterRule(localctx, 34, self.RULE_lexerAliasDef)
        self._la = 0 # Token type
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 235
            self.match(jsgParserParser.LEXER_ID)
            self.state = 240
            self._errHandler.sync(self)
            _la = self._input.LA(1)
            while _la==jsgParserParser.T__8:
                self.state = 236
                self.match(jsgParserParser.T__8)
                self.state = 237
                self.match(jsgParserParser.LEXER_ID)
                self.state = 242
                self._errHandler.sync(self)
                _la = self._input.LA(1)

        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class EbnfSuffixContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def INT(self, i:int=None):
            if i is None:
                return self.getTokens(jsgParserParser.INT)
            else:
                return self.getToken(jsgParserParser.INT, i)

        def getRuleIndex(self):
            return jsgParserParser.RULE_ebnfSuffix

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitEbnfSuffix" ):
                return visitor.visitEbnfSuffix(self)
            else:
                return visitor.visitChildren(self)




    def ebnfSuffix(self):

        localctx = jsgParserParser.EbnfSuffixContext(self, self._ctx, self.state)
        self.enterRule(localctx, 36, self.RULE_ebnfSuffix)
        self._la = 0 # Token type
        try:
            self.state = 255
            self._errHandler.sync(self)
            token = self._input.LA(1)
            if token in [jsgParserParser.T__16]:
                self.enterOuterAlt(localctx, 1)
                self.state = 243
                self.match(jsgParserParser.T__16)
                pass
            elif token in [jsgParserParser.T__17]:
                self.enterOuterAlt(localctx, 2)
                self.state = 244
                self.match(jsgParserParser.T__17)
                pass
            elif token in [jsgParserParser.T__18]:
                self.enterOuterAlt(localctx, 3)
                self.state = 245
                self.match(jsgParserParser.T__18)
                pass
            elif token in [jsgParserParser.T__5]:
                self.enterOuterAlt(localctx, 4)
                self.state = 246
                self.match(jsgParserParser.T__5)
                self.state = 247
                self.match(jsgParserParser.INT)
                self.state = 252
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                if _la==jsgParserParser.T__11:
                    self.state = 248
                    self.match(jsgParserParser.T__11)
                    self.state = 250
                    self._errHandler.sync(self)
                    _la = self._input.LA(1)
                    if _la==jsgParserParser.T__17 or _la==jsgParserParser.INT:
                        self.state = 249
                        _la = self._input.LA(1)
                        if not(_la==jsgParserParser.T__17 or _la==jsgParserParser.INT):
                            self._errHandler.recoverInline(self)
                        else:
                            self._errHandler.reportMatch(self)
                            self.consume()




                self.state = 254
                self.match(jsgParserParser.T__6)
                pass
            else:
                raise NoViableAltException(self)

        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class LexerRuleSpecContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def LEXER_ID(self):
            return self.getToken(jsgParserParser.LEXER_ID, 0)

        def lexerRuleBlock(self):
            return self.getTypedRuleContext(jsgParserParser.LexerRuleBlockContext,0)


        def getRuleIndex(self):
            return jsgParserParser.RULE_lexerRuleSpec

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitLexerRuleSpec" ):
                return visitor.visitLexerRuleSpec(self)
            else:
                return visitor.visitChildren(self)




    def lexerRuleSpec(self):

        localctx = jsgParserParser.LexerRuleSpecContext(self, self._ctx, self.state)
        self.enterRule(localctx, 38, self.RULE_lexerRuleSpec)
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 257
            self.match(jsgParserParser.LEXER_ID)
            self.state = 258
            self.match(jsgParserParser.T__12)
            self.state = 259
            self.lexerRuleBlock()
            self.state = 260
            self.match(jsgParserParser.T__3)
        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class LexerRuleBlockContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def lexerAltList(self):
            return self.getTypedRuleContext(jsgParserParser.LexerAltListContext,0)


        def getRuleIndex(self):
            return jsgParserParser.RULE_lexerRuleBlock

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitLexerRuleBlock" ):
                return visitor.visitLexerRuleBlock(self)
            else:
                return visitor.visitChildren(self)




    def lexerRuleBlock(self):

        localctx = jsgParserParser.LexerRuleBlockContext(self, self._ctx, self.state)
        self.enterRule(localctx, 40, self.RULE_lexerRuleBlock)
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 262
            self.lexerAltList()
        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class LexerAltListContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def lexerAlt(self, i:int=None):
            if i is None:
                return self.getTypedRuleContexts(jsgParserParser.LexerAltContext)
            else:
                return self.getTypedRuleContext(jsgParserParser.LexerAltContext,i)


        def getRuleIndex(self):
            return jsgParserParser.RULE_lexerAltList

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitLexerAltList" ):
                return visitor.visitLexerAltList(self)
            else:
                return visitor.visitChildren(self)




    def lexerAltList(self):

        localctx = jsgParserParser.LexerAltListContext(self, self._ctx, self.state)
        self.enterRule(localctx, 42, self.RULE_lexerAltList)
        self._la = 0 # Token type
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 264
            self.lexerAlt()
            self.state = 269
            self._errHandler.sync(self)
            _la = self._input.LA(1)
            while _la==jsgParserParser.T__8:
                self.state = 265
                self.match(jsgParserParser.T__8)
                self.state = 266
                self.lexerAlt()
                self.state = 271
                self._errHandler.sync(self)
                _la = self._input.LA(1)

        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class LexerAltContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def lexerElements(self):
            return self.getTypedRuleContext(jsgParserParser.LexerElementsContext,0)


        def getRuleIndex(self):
            return jsgParserParser.RULE_lexerAlt

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitLexerAlt" ):
                return visitor.visitLexerAlt(self)
            else:
                return visitor.visitChildren(self)




    def lexerAlt(self):

        localctx = jsgParserParser.LexerAltContext(self, self._ctx, self.state)
        self.enterRule(localctx, 44, self.RULE_lexerAlt)
        try:
            self.state = 274
            self._errHandler.sync(self)
            token = self._input.LA(1)
            if token in [jsgParserParser.T__13, jsgParserParser.LEXER_ID, jsgParserParser.STRING, jsgParserParser.ANY, jsgParserParser.LEXER_CHAR_SET]:
                self.enterOuterAlt(localctx, 1)
                self.state = 272
                self.lexerElements()
                pass
            elif token in [jsgParserParser.T__3, jsgParserParser.T__8, jsgParserParser.T__14]:
                self.enterOuterAlt(localctx, 2)

                pass
            else:
                raise NoViableAltException(self)

        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class LexerElementsContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def lexerElement(self, i:int=None):
            if i is None:
                return self.getTypedRuleContexts(jsgParserParser.LexerElementContext)
            else:
                return self.getTypedRuleContext(jsgParserParser.LexerElementContext,i)


        def getRuleIndex(self):
            return jsgParserParser.RULE_lexerElements

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitLexerElements" ):
                return visitor.visitLexerElements(self)
            else:
                return visitor.visitChildren(self)




    def lexerElements(self):

        localctx = jsgParserParser.LexerElementsContext(self, self._ctx, self.state)
        self.enterRule(localctx, 46, self.RULE_lexerElements)
        self._la = 0 # Token type
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 277 
            self._errHandler.sync(self)
            _la = self._input.LA(1)
            while True:
                self.state = 276
                self.lexerElement()
                self.state = 279 
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                if not ((((_la) & ~0x3f) == 0 and ((1 << _la) & ((1 << jsgParserParser.T__13) | (1 << jsgParserParser.LEXER_ID) | (1 << jsgParserParser.STRING) | (1 << jsgParserParser.ANY) | (1 << jsgParserParser.LEXER_CHAR_SET))) != 0)):
                    break

        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class LexerElementContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def lexerAtom(self):
            return self.getTypedRuleContext(jsgParserParser.LexerAtomContext,0)


        def ebnfSuffix(self):
            return self.getTypedRuleContext(jsgParserParser.EbnfSuffixContext,0)


        def lexerBlock(self):
            return self.getTypedRuleContext(jsgParserParser.LexerBlockContext,0)


        def getRuleIndex(self):
            return jsgParserParser.RULE_lexerElement

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitLexerElement" ):
                return visitor.visitLexerElement(self)
            else:
                return visitor.visitChildren(self)




    def lexerElement(self):

        localctx = jsgParserParser.LexerElementContext(self, self._ctx, self.state)
        self.enterRule(localctx, 48, self.RULE_lexerElement)
        self._la = 0 # Token type
        try:
            self.state = 289
            self._errHandler.sync(self)
            token = self._input.LA(1)
            if token in [jsgParserParser.LEXER_ID, jsgParserParser.STRING, jsgParserParser.ANY, jsgParserParser.LEXER_CHAR_SET]:
                self.enterOuterAlt(localctx, 1)
                self.state = 281
                self.lexerAtom()
                self.state = 283
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                if (((_la) & ~0x3f) == 0 and ((1 << _la) & ((1 << jsgParserParser.T__5) | (1 << jsgParserParser.T__16) | (1 << jsgParserParser.T__17) | (1 << jsgParserParser.T__18))) != 0):
                    self.state = 282
                    self.ebnfSuffix()


                pass
            elif token in [jsgParserParser.T__13]:
                self.enterOuterAlt(localctx, 2)
                self.state = 285
                self.lexerBlock()
                self.state = 287
                self._errHandler.sync(self)
                _la = self._input.LA(1)
                if (((_la) & ~0x3f) == 0 and ((1 << _la) & ((1 << jsgParserParser.T__5) | (1 << jsgParserParser.T__16) | (1 << jsgParserParser.T__17) | (1 << jsgParserParser.T__18))) != 0):
                    self.state = 286
                    self.ebnfSuffix()


                pass
            else:
                raise NoViableAltException(self)

        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class LexerBlockContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def lexerAltList(self):
            return self.getTypedRuleContext(jsgParserParser.LexerAltListContext,0)


        def getRuleIndex(self):
            return jsgParserParser.RULE_lexerBlock

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitLexerBlock" ):
                return visitor.visitLexerBlock(self)
            else:
                return visitor.visitChildren(self)




    def lexerBlock(self):

        localctx = jsgParserParser.LexerBlockContext(self, self._ctx, self.state)
        self.enterRule(localctx, 50, self.RULE_lexerBlock)
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 291
            self.match(jsgParserParser.T__13)
            self.state = 292
            self.lexerAltList()
            self.state = 293
            self.match(jsgParserParser.T__14)
        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class LexerAtomContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def terminal(self):
            return self.getTypedRuleContext(jsgParserParser.TerminalContext,0)


        def LEXER_CHAR_SET(self):
            return self.getToken(jsgParserParser.LEXER_CHAR_SET, 0)

        def ANY(self):
            return self.getToken(jsgParserParser.ANY, 0)

        def getRuleIndex(self):
            return jsgParserParser.RULE_lexerAtom

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitLexerAtom" ):
                return visitor.visitLexerAtom(self)
            else:
                return visitor.visitChildren(self)




    def lexerAtom(self):

        localctx = jsgParserParser.LexerAtomContext(self, self._ctx, self.state)
        self.enterRule(localctx, 52, self.RULE_lexerAtom)
        try:
            self.state = 298
            self._errHandler.sync(self)
            token = self._input.LA(1)
            if token in [jsgParserParser.LEXER_ID, jsgParserParser.STRING]:
                self.enterOuterAlt(localctx, 1)
                self.state = 295
                self.terminal()
                pass
            elif token in [jsgParserParser.LEXER_CHAR_SET]:
                self.enterOuterAlt(localctx, 2)
                self.state = 296
                self.match(jsgParserParser.LEXER_CHAR_SET)
                pass
            elif token in [jsgParserParser.ANY]:
                self.enterOuterAlt(localctx, 3)
                self.state = 297
                self.match(jsgParserParser.ANY)
                pass
            else:
                raise NoViableAltException(self)

        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx

    class TerminalContext(ParserRuleContext):

        def __init__(self, parser, parent:ParserRuleContext=None, invokingState:int=-1):
            super().__init__(parent, invokingState)
            self.parser = parser

        def LEXER_ID(self):
            return self.getToken(jsgParserParser.LEXER_ID, 0)

        def STRING(self):
            return self.getToken(jsgParserParser.STRING, 0)

        def getRuleIndex(self):
            return jsgParserParser.RULE_terminal

        def accept(self, visitor:ParseTreeVisitor):
            if hasattr( visitor, "visitTerminal" ):
                return visitor.visitTerminal(self)
            else:
                return visitor.visitChildren(self)




    def terminal(self):

        localctx = jsgParserParser.TerminalContext(self, self._ctx, self.state)
        self.enterRule(localctx, 54, self.RULE_terminal)
        self._la = 0 # Token type
        try:
            self.enterOuterAlt(localctx, 1)
            self.state = 300
            _la = self._input.LA(1)
            if not(_la==jsgParserParser.LEXER_ID or _la==jsgParserParser.STRING):
                self._errHandler.recoverInline(self)
            else:
                self._errHandler.reportMatch(self)
                self.consume()
        except RecognitionException as re:
            localctx.exception = re
            self._errHandler.reportError(self, re)
            self._errHandler.recover(self, re)
        finally:
            self.exitRule()
        return localctx





