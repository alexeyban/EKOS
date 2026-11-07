import codecs
import os
import sys
from argparse import ArgumentParser
from urllib import request
from urllib.parse import urlparse

import chardet
from ShExJSG.ShExJ import Schema
from antlr4 import CommonTokenStream
from antlr4 import InputStream
from antlr4.error.ErrorListener import ErrorListener
from jsonasobj import as_json
from rdflib import Graph
from rdflib.plugin import plugins as rdflib_plugins, Serializer as rdflib_Serializer
from rdflib.util import SUFFIX_FORMAT_MAP

from pyshexc.parser.ShExDocLexer import ShExDocLexer
from pyshexc.parser.ShExDocParser import ShExDocParser
from pyshexc.parser_impl.shex_doc_parser import ShexDocParser


class ParseErrorListener(ErrorListener):
    """ Record errors and text """

    def __init__(self):
        self.n_errors = 0
        self.errors = []

    def syntaxError(self, recognizer, offending_symbol, line, column, msg, e):
        self.n_errors += 1
        self.errors.append("line " + str(line) + ":" + str(column) + " " + msg)


def load_shex_file(shexfilename: str) -> str:
    """
    Read a ShEx input file, processing BOM encodings if necessary

    :param shexfilename: file or URL to open
    :return:
    """
    if '://' in shexfilename:
        with request.urlopen(shexfilename) as response:
            data = response.read()
    else:
        with open(shexfilename, 'rb') as inf:
            data = inf.read()

    if data.startswith(codecs.BOM_UTF8):
        encoding = 'utf-8-sig'
    else:
        result = chardet.detect(data)
        encoding = result['encoding'] if float(result['confidence']) > 0.9 else 'UTF-8'

    return data.decode(encoding)



def do_parse(infilename: str, jsonfilename: str | None, rdffilename: str | None, rdffmt: str,
             context: str | None = None) -> bool:
    """
    Parse the jsg in infilename and save the results in outfilename
    :param infilename: name of the file containing the ShExC
    :param jsonfilename: target ShExJ equivalent
    :param rdffilename: target ShExR equivalent
    :param rdffmt: target RDF format
    :param context: @context to use for rdf generation. If None use what is in the file
    :return: true if success
    """

    inp = InputStream(load_shex_file(infilename))

    shexj = parse(inp)
    if shexj is not None:
        shexj['@context'] = context if context else "http://www.w3.org/ns/shex.jsonld"
        if jsonfilename:
            with open(jsonfilename, 'w') as outfile:
                outfile.write(as_json(shexj))
        if rdffilename:
            g = Graph().parse(data=as_json(shexj, indent=None), format="json-ld")
            g.serialize(open(rdffilename, "wb"), format=rdffmt)
        return True
    return False


def parse(input_: str | InputStream, default_base: str | None=None) -> Schema | None:
    """
    Parse the text in infile and return the resulting schema
    :param input_: text or input stream to parse
    :param default_base: base URI for relative URI's in schema
    :return: ShExJ Schema object.  None if error.
    """

    # Step 1: Tokenize the input stream
    error_listener = ParseErrorListener()
    if not isinstance(input_, InputStream):
        input_ = InputStream(input_)
    lexer = ShExDocLexer(input_)
    lexer.addErrorListener(error_listener)
    tokens = CommonTokenStream(lexer)
    tokens.fill()
    if error_listener.n_errors:         # Lexer prints errors directly
        return None

    # Step 2: Generate the parse tree
    parser = ShExDocParser(tokens)
    parser.addErrorListener(error_listener)
    parse_tree = parser.shExDoc()
    if error_listener.n_errors:
        print('\n'.join(error_listener.errors), file=sys.stderr)
        return None

    # Step 3: Transform the results the results
    parser = ShexDocParser(default_base=default_base)
    parser.visit(parse_tree)

    return parser.context.schema


def rdf_suffix(fmt: str) -> str:
    """ Map the RDF format to the approproate suffix """
    for k, v in SUFFIX_FORMAT_MAP.items():
        if fmt == v:
            return k
    return 'rdf'


def genargs() -> ArgumentParser:
    """
    Create a command line parser
    :return: parser
    """
    parser = ArgumentParser()
    parser.add_argument("infile", help="Input ShExC specification")
    parser.add_argument("-nj", "--nojson", help="Do not produce json output", action="store_true")
    parser.add_argument("-nr", "--nordf", help="Do not produce rdf output", action="store_true")
    parser.add_argument("-j", "--jsonfile", help="Output ShExJ file (Default: {infile}.json)")
    parser.add_argument("-r", "--rdffile", help="Output ShExR file (Default: {infile}.{fmt suffix})")
    parser.add_argument("--context", help="Alternative @context")
    parser.add_argument("-f", "--format",
                        choices=list(set(x.name for x in rdflib_plugins(None, rdflib_Serializer)
                                         if '/' not in str(x.name))),
                        help="Output format (Default: turtle)", default="turtle")
    return parser


def generate(argv: str | list[str]) -> bool:
    """
    Transform ShExC to ShExJ
    :param argv: Command line arguments
    :return: True if successful
    """
    if isinstance(argv, str):
        argv = argv.split()
    opts = genargs().parse_args(argv)
    if "://" in opts.infile:
        filebase = urlparse(opts.infile).path.split('/')[-1]
    else:
        filebase = os.path.dirname(opts.infile) + str(os.path.basename(opts.infile))
    filebase = filebase.rsplit('.', 1)[0]
    if opts.nojson:
        opts.jsonfile = None
    elif not opts.jsonfile:
        opts.jsonfile = filebase + ".json"
    if opts.nordf:
        opts.rdffile = None
    elif not opts.rdffile:
        opts.rdffile = filebase + "." + rdf_suffix(opts.format)
    if do_parse(opts.infile, opts.jsonfile, opts.rdffile, opts.format, opts.context):
        if opts.jsonfile:
            print("JSON output written to {}".format(opts.jsonfile))
        if opts.rdffile:
            print("{} output written to {}".format(opts.format, opts.rdffile))
        return True
    else:
        print("Conversion failed")
        return False
