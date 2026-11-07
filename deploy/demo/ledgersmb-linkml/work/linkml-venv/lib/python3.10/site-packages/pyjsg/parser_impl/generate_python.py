import importlib.abc
import importlib.util
import os
from argparse import ArgumentParser

import sys
from antlr4 import CommonTokenStream
from antlr4 import FileStream, InputStream
from antlr4.error.ErrorListener import ErrorListener
from pyjsg.parser.jsgParser import jsgParser

from pyjsg.parser.jsgLexer import jsgLexer
from pyjsg.parser_impl.jsg_doc_parser import JSGDocParser


class ParseErrorListener(ErrorListener):

    def __init__(self):
        self.n_errors = 0

    def syntaxError(self, recognizer, offendingSymbol, line, column, msg, e):
        self.n_errors += 1

    def reportAmbiguity(self, recognizer, dfa, startIndex, stopIndex, exact, ambigAlts, configs):
        print("*** Parsing ambiguity error", file=sys.stderr)
        self.n_errors += 1

    def reportAttemptingFullContext(self, recognizer, dfa, startIndex, stopIndex, conflictingAlts, configs):
        print("*** Parsing full context error", file=sys.stderr)
        self.n_errors += 1

    def reportContextSensitivity(self, recognizer, dfa, startIndex, stopIndex, prediction, configs):
        print("*** Context sensitivity error", file=sys.stderr)
        self.n_errors += 1


def do_parse(infilename: str, outfilename: str, verbose: bool, header: bool) -> bool:
    """
    Parse the jsg in infilename and save the results in outfilename
    :param infilename: file containing jsg
    :param outfilename: target python file
    :param verbose: verbose output flag
    :param header: output header flag
    :return: true if success
    """
    python = parse(FileStream(infilename, encoding="utf-8"), infilename, emit_header=header)
    if python is not None:
        with open(outfilename, 'w') as outfile:
            outfile.write(python)
        if verbose:
            print("Output written to {}".format(outfilename))
        return True
    return False


def parse(input_: str | FileStream, source: str, emit_header: bool=True) -> str | None:
    """Parse the text in infile and save the results in outfile

    :param input_: string or stream to parse
    :param source: source name for python file header
    :param emit_header: True means include header in python file
    :return: python text if successful
    """

    # Step 1: Tokenize the input stream
    error_listener = ParseErrorListener()
    if not isinstance(input_, FileStream):
        input_ = InputStream(input_)
    lexer = jsgLexer(input_)
    lexer.addErrorListener(error_listener)
    tokens = CommonTokenStream(lexer)
    tokens.fill()
    if error_listener.n_errors:
        return None

    # Step 2: Generate the parse tree
    parser = jsgParser(tokens)
    parser.addErrorListener(error_listener)
    parse_tree = parser.doc()
    if error_listener.n_errors:
        return None

    # Step 3: Transform the results the results
    parser = JSGDocParser()
    parser.visit(parse_tree)

    if parser.undefined_tokens():
        for tkn in parser.undefined_tokens():
            print("Undefined token: " + tkn)
        return None

    return parser.as_python(source, emit_header=emit_header)


def genargs() -> ArgumentParser:
    """
    Create a command line parser
    :return: parser
    """
    parser = ArgumentParser()
    parser.add_argument("infile", help="Input JSG specification")
    parser.add_argument("-o", "--outfile", help="Output python file (Default: {infile}.py)")
    parser.add_argument("-e", "--evaluate", help="Evaluate resulting python file as a test", action="store_true")
    parser.add_argument("-v", "--verbose", help="Verbose output", action="store_true")
    parser.add_argument("-nh", "--noheader", help="Omit date and version information from header", action="store_false")
    return parser


def evaluate(module_name: str, fname: str, verbose: bool):
    """
    Load fname as a module.  Will raise an exception if there is an error
    :param module_name: resulting name of module
    :param fname: name to load
    :param verbose: verbose houtput
    """
    if verbose:
        print("Testing {}".format(fname))
    spec = importlib.util.spec_from_file_location(module_name, fname)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)


def generate(argv: list[str] | None = None) -> int:
    opts = genargs().parse_args(argv)
    file_base = str(os.path.basename(opts.infile.rsplit('.', 1)[0]))
    if not opts.outfile:
        opts.outfile = os.path.join(os.path.dirname(opts.infile), file_base + ".py")
    if do_parse(opts.infile, opts.outfile, opts.verbose, opts.noheader):
        if opts.evaluate:
            evaluate("generate_python_namespace", opts.outfile, opts.verbose)               # Don't pollute namespace
        return 0
    else:
        print("Conversion failed")
        return 1
