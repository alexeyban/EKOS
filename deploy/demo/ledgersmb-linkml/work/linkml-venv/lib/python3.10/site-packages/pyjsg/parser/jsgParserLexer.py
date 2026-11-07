# Generated from /Users/mrf7578/Development/git/hsolbrig/pyjsg/grammar/jsgParser.g4 by ANTLR 4.7
from antlr4 import *
from io import StringIO
from typing import TextIO
import sys


def serializedATN():
    with StringIO() as buf:
        buf.write("\3\u608b\ua72a\u8133\ub9ed\u417c\u3be7\u7786\u5964\2\35")
        buf.write("\u00e0\b\1\4\2\t\2\4\3\t\3\4\4\t\4\4\5\t\5\4\6\t\6\4\7")
        buf.write("\t\7\4\b\t\b\4\t\t\t\4\n\t\n\4\13\t\13\4\f\t\f\4\r\t\r")
        buf.write("\4\16\t\16\4\17\t\17\4\20\t\20\4\21\t\21\4\22\t\22\4\23")
        buf.write("\t\23\4\24\t\24\4\25\t\25\4\26\t\26\4\27\t\27\4\30\t\30")
        buf.write("\4\31\t\31\4\32\t\32\4\33\t\33\4\34\t\34\4\35\t\35\4\36")
        buf.write("\t\36\4\37\t\37\4 \t \4!\t!\3\2\3\2\3\2\3\2\3\2\3\2\3")
        buf.write("\2\3\2\3\2\3\2\3\2\3\3\3\3\3\3\3\3\3\3\3\3\3\4\3\4\3\5")
        buf.write("\3\5\3\6\3\6\3\6\3\6\3\6\3\6\3\6\3\6\3\7\3\7\3\b\3\b\3")
        buf.write("\t\3\t\3\t\3\n\3\n\3\13\3\13\3\f\3\f\3\r\3\r\3\16\3\16")
        buf.write("\3\17\3\17\3\20\3\20\3\21\3\21\3\22\3\22\3\23\3\23\3\24")
        buf.write("\3\24\3\25\3\25\6\25\u0080\n\25\r\25\16\25\u0081\3\26")
        buf.write("\3\26\3\27\3\27\5\27\u0088\n\27\3\30\3\30\7\30\u008c\n")
        buf.write("\30\f\30\16\30\u008f\13\30\3\30\3\30\7\30\u0093\n\30\f")
        buf.write("\30\16\30\u0096\13\30\3\30\3\30\7\30\u009a\n\30\f\30\16")
        buf.write("\30\u009d\13\30\5\30\u009f\n\30\3\31\5\31\u00a2\n\31\3")
        buf.write("\32\3\32\5\32\u00a6\n\32\3\33\3\33\5\33\u00aa\n\33\3\34")
        buf.write("\3\34\3\34\3\34\6\34\u00b0\n\34\r\34\16\34\u00b1\3\34")
        buf.write("\3\34\3\34\3\34\3\34\6\34\u00b9\n\34\r\34\16\34\u00ba")
        buf.write("\3\34\5\34\u00be\n\34\3\35\6\35\u00c1\n\35\r\35\16\35")
        buf.write("\u00c2\3\36\3\36\3\37\6\37\u00c8\n\37\r\37\16\37\u00c9")
        buf.write("\3\37\3\37\3 \3 \7 \u00d0\n \f \16 \u00d3\13 \3 \3 \3")
        buf.write("!\3!\3!\3!\6!\u00db\n!\r!\16!\u00dc\3!\3!\2\2\"\3\3\5")
        buf.write("\4\7\5\t\6\13\7\r\b\17\t\21\n\23\13\25\f\27\r\31\16\33")
        buf.write("\17\35\20\37\21!\22#\23%\24\'\25)\26+\2-\2/\27\61\2\63")
        buf.write("\2\65\2\67\309\31;\32=\33?\34A\35\3\2\f\3\2C\\\4\2\62")
        buf.write(";aa\16\2c|\u00c2\u00d8\u00da\u00f8\u00fa\u0301\u0372\u037f")
        buf.write("\u0381\u2001\u200e\u200f\u2072\u2191\u2c02\u2ff1\u3003")
        buf.write("\ud801\uf902\ufdd1\ufdf2\uffff\5\2\u00b9\u00b9\u0302\u0371")
        buf.write("\u2041\u2042\3\2$$\3\2))\3\2\62;\5\2\13\f\17\17\"\"\4")
        buf.write("\2\f\f\17\17\3\2__\2\u00ec\2\3\3\2\2\2\2\5\3\2\2\2\2\7")
        buf.write("\3\2\2\2\2\t\3\2\2\2\2\13\3\2\2\2\2\r\3\2\2\2\2\17\3\2")
        buf.write("\2\2\2\21\3\2\2\2\2\23\3\2\2\2\2\25\3\2\2\2\2\27\3\2\2")
        buf.write("\2\2\31\3\2\2\2\2\33\3\2\2\2\2\35\3\2\2\2\2\37\3\2\2\2")
        buf.write("\2!\3\2\2\2\2#\3\2\2\2\2%\3\2\2\2\2\'\3\2\2\2\2)\3\2\2")
        buf.write("\2\2/\3\2\2\2\2\67\3\2\2\2\29\3\2\2\2\2;\3\2\2\2\2=\3")
        buf.write("\2\2\2\2?\3\2\2\2\2A\3\2\2\2\3C\3\2\2\2\5N\3\2\2\2\7T")
        buf.write("\3\2\2\2\tV\3\2\2\2\13X\3\2\2\2\r`\3\2\2\2\17b\3\2\2\2")
        buf.write("\21d\3\2\2\2\23g\3\2\2\2\25i\3\2\2\2\27k\3\2\2\2\31m\3")
        buf.write("\2\2\2\33o\3\2\2\2\35q\3\2\2\2\37s\3\2\2\2!u\3\2\2\2#")
        buf.write("w\3\2\2\2%y\3\2\2\2\'{\3\2\2\2)}\3\2\2\2+\u0083\3\2\2")
        buf.write("\2-\u0087\3\2\2\2/\u009e\3\2\2\2\61\u00a1\3\2\2\2\63\u00a5")
        buf.write("\3\2\2\2\65\u00a9\3\2\2\2\67\u00bd\3\2\2\29\u00c0\3\2")
        buf.write("\2\2;\u00c4\3\2\2\2=\u00c7\3\2\2\2?\u00cd\3\2\2\2A\u00d6")
        buf.write("\3\2\2\2CD\7B\2\2DE\7v\2\2EF\7g\2\2FG\7t\2\2GH\7o\2\2")
        buf.write("HI\7k\2\2IJ\7p\2\2JK\7c\2\2KL\7n\2\2LM\7u\2\2M\4\3\2\2")
        buf.write("\2NO\7\60\2\2OP\7V\2\2PQ\7[\2\2QR\7R\2\2RS\7G\2\2S\6\3")
        buf.write("\2\2\2TU\7/\2\2U\b\3\2\2\2VW\7=\2\2W\n\3\2\2\2XY\7\60")
        buf.write("\2\2YZ\7K\2\2Z[\7I\2\2[\\\7P\2\2\\]\7Q\2\2]^\7T\2\2^_")
        buf.write("\7G\2\2_\f\3\2\2\2`a\7}\2\2a\16\3\2\2\2bc\7\177\2\2c\20")
        buf.write("\3\2\2\2de\7/\2\2ef\7@\2\2f\22\3\2\2\2gh\7~\2\2h\24\3")
        buf.write("\2\2\2ij\7]\2\2j\26\3\2\2\2kl\7_\2\2l\30\3\2\2\2mn\7.")
        buf.write("\2\2n\32\3\2\2\2op\7<\2\2p\34\3\2\2\2qr\7*\2\2r\36\3\2")
        buf.write("\2\2st\7+\2\2t \3\2\2\2uv\7?\2\2v\"\3\2\2\2wx\7A\2\2x")
        buf.write("$\3\2\2\2yz\7,\2\2z&\3\2\2\2{|\7-\2\2|(\3\2\2\2}\177\5")
        buf.write("+\26\2~\u0080\5-\27\2\177~\3\2\2\2\u0080\u0081\3\2\2\2")
        buf.write("\u0081\177\3\2\2\2\u0081\u0082\3\2\2\2\u0082*\3\2\2\2")
        buf.write("\u0083\u0084\t\2\2\2\u0084,\3\2\2\2\u0085\u0088\5+\26")
        buf.write("\2\u0086\u0088\t\3\2\2\u0087\u0085\3\2\2\2\u0087\u0086")
        buf.write("\3\2\2\2\u0088.\3\2\2\2\u0089\u008d\5\61\31\2\u008a\u008c")
        buf.write("\5\65\33\2\u008b\u008a\3\2\2\2\u008c\u008f\3\2\2\2\u008d")
        buf.write("\u008b\3\2\2\2\u008d\u008e\3\2\2\2\u008e\u009f\3\2\2\2")
        buf.write("\u008f\u008d\3\2\2\2\u0090\u0094\5+\26\2\u0091\u0093\5")
        buf.write("-\27\2\u0092\u0091\3\2\2\2\u0093\u0096\3\2\2\2\u0094\u0092")
        buf.write("\3\2\2\2\u0094\u0095\3\2\2\2\u0095\u0097\3\2\2\2\u0096")
        buf.write("\u0094\3\2\2\2\u0097\u009b\5\63\32\2\u0098\u009a\5\65")
        buf.write("\33\2\u0099\u0098\3\2\2\2\u009a\u009d\3\2\2\2\u009b\u0099")
        buf.write("\3\2\2\2\u009b\u009c\3\2\2\2\u009c\u009f\3\2\2\2\u009d")
        buf.write("\u009b\3\2\2\2\u009e\u0089\3\2\2\2\u009e\u0090\3\2\2\2")
        buf.write("\u009f\60\3\2\2\2\u00a0\u00a2\t\4\2\2\u00a1\u00a0\3\2")
        buf.write("\2\2\u00a2\62\3\2\2\2\u00a3\u00a6\5\61\31\2\u00a4\u00a6")
        buf.write("\t\5\2\2\u00a5\u00a3\3\2\2\2\u00a5\u00a4\3\2\2\2\u00a6")
        buf.write("\64\3\2\2\2\u00a7\u00aa\5\63\32\2\u00a8\u00aa\5-\27\2")
        buf.write("\u00a9\u00a7\3\2\2\2\u00a9\u00a8\3\2\2\2\u00aa\66\3\2")
        buf.write("\2\2\u00ab\u00af\7$\2\2\u00ac\u00b0\n\6\2\2\u00ad\u00ae")
        buf.write("\7^\2\2\u00ae\u00b0\7$\2\2\u00af\u00ac\3\2\2\2\u00af\u00ad")
        buf.write("\3\2\2\2\u00b0\u00b1\3\2\2\2\u00b1\u00af\3\2\2\2\u00b1")
        buf.write("\u00b2\3\2\2\2\u00b2\u00b3\3\2\2\2\u00b3\u00be\7$\2\2")
        buf.write("\u00b4\u00b8\7)\2\2\u00b5\u00b9\n\7\2\2\u00b6\u00b7\7")
        buf.write("^\2\2\u00b7\u00b9\7;\2\2\u00b8\u00b5\3\2\2\2\u00b8\u00b6")
        buf.write("\3\2\2\2\u00b9\u00ba\3\2\2\2\u00ba\u00b8\3\2\2\2\u00ba")
        buf.write("\u00bb\3\2\2\2\u00bb\u00bc\3\2\2\2\u00bc\u00be\7)\2\2")
        buf.write("\u00bd\u00ab\3\2\2\2\u00bd\u00b4\3\2\2\2\u00be8\3\2\2")
        buf.write("\2\u00bf\u00c1\t\b\2\2\u00c0\u00bf\3\2\2\2\u00c1\u00c2")
        buf.write("\3\2\2\2\u00c2\u00c0\3\2\2\2\u00c2\u00c3\3\2\2\2\u00c3")
        buf.write(":\3\2\2\2\u00c4\u00c5\7\60\2\2\u00c5<\3\2\2\2\u00c6\u00c8")
        buf.write("\t\t\2\2\u00c7\u00c6\3\2\2\2\u00c8\u00c9\3\2\2\2\u00c9")
        buf.write("\u00c7\3\2\2\2\u00c9\u00ca\3\2\2\2\u00ca\u00cb\3\2\2\2")
        buf.write("\u00cb\u00cc\b\37\2\2\u00cc>\3\2\2\2\u00cd\u00d1\7%\2")
        buf.write("\2\u00ce\u00d0\n\n\2\2\u00cf\u00ce\3\2\2\2\u00d0\u00d3")
        buf.write("\3\2\2\2\u00d1\u00cf\3\2\2\2\u00d1\u00d2\3\2\2\2\u00d2")
        buf.write("\u00d4\3\2\2\2\u00d3\u00d1\3\2\2\2\u00d4\u00d5\b \2\2")
        buf.write("\u00d5@\3\2\2\2\u00d6\u00da\7]\2\2\u00d7\u00db\n\13\2")
        buf.write("\2\u00d8\u00d9\7^\2\2\u00d9\u00db\7_\2\2\u00da\u00d7\3")
        buf.write("\2\2\2\u00da\u00d8\3\2\2\2\u00db\u00dc\3\2\2\2\u00dc\u00da")
        buf.write("\3\2\2\2\u00dc\u00dd\3\2\2\2\u00dd\u00de\3\2\2\2\u00de")
        buf.write("\u00df\7_\2\2\u00dfB\3\2\2\2\26\2\u0081\u0087\u008d\u0094")
        buf.write("\u009b\u009e\u00a1\u00a5\u00a9\u00af\u00b1\u00b8\u00ba")
        buf.write("\u00bd\u00c2\u00c9\u00d1\u00da\u00dc\3\b\2\2")
        return buf.getvalue()


class jsgParserLexer(Lexer):

    atn = ATNDeserializer().deserialize(serializedATN())

    decisionsToDFA = [ DFA(ds, i) for i, ds in enumerate(atn.decisionToState) ]

    T__0 = 1
    T__1 = 2
    T__2 = 3
    T__3 = 4
    T__4 = 5
    T__5 = 6
    T__6 = 7
    T__7 = 8
    T__8 = 9
    T__9 = 10
    T__10 = 11
    T__11 = 12
    T__12 = 13
    T__13 = 14
    T__14 = 15
    T__15 = 16
    T__16 = 17
    T__17 = 18
    T__18 = 19
    LEXER_ID = 20
    ID = 21
    STRING = 22
    INT = 23
    ANY = 24
    PASS = 25
    COMMENT = 26
    LEXER_CHAR_SET = 27

    channelNames = [ u"DEFAULT_TOKEN_CHANNEL", u"HIDDEN" ]

    modeNames = [ "DEFAULT_MODE" ]

    literalNames = [ "<INVALID>",
            "'@terminals'", "'.TYPE'", "'-'", "';'", "'.IGNORE'", "'{'", 
            "'}'", "'->'", "'|'", "'['", "']'", "','", "':'", "'('", "')'", 
            "'='", "'?'", "'*'", "'+'", "'.'" ]

    symbolicNames = [ "<INVALID>",
            "LEXER_ID", "ID", "STRING", "INT", "ANY", "PASS", "COMMENT", 
            "LEXER_CHAR_SET" ]

    ruleNames = [ "T__0", "T__1", "T__2", "T__3", "T__4", "T__5", "T__6", 
                  "T__7", "T__8", "T__9", "T__10", "T__11", "T__12", "T__13", 
                  "T__14", "T__15", "T__16", "T__17", "T__18", "LEXER_ID", 
                  "LEXER_ID_START_CHAR", "LEXER_ID_CHAR", "ID", "ID_START_CHAR", 
                  "ID_CHAR", "ANY_CHAR", "STRING", "INT", "ANY", "PASS", 
                  "COMMENT", "LEXER_CHAR_SET" ]

    grammarFileName = "jsgParser.g4"

    def __init__(self, input=None, output:TextIO = sys.stdout):
        super().__init__(input, output)
        self.checkVersion("4.7")
        self._interp = LexerATNSimulator(self, self.atn, self.decisionsToDFA, PredictionContextCache())
        self._actions = None
        self._predicates = None


