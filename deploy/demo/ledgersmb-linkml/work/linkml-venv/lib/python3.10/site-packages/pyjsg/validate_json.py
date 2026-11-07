import os
from argparse import ArgumentParser
from io import StringIO
from types import ModuleType
from typing import cast, TextIO, NamedTuple

import requests

from pyjsg.jsglib.loader import loads, Logger, is_valid
from pyjsg.parser_impl.generate_python import parse


class ValidationResult(NamedTuple):
    success: bool
    fail_reason: str
    test_name: str
    type: str | None

    def __str__(self) -> str:
        return (f"{self.test_name}: " if self.test_name else "") +\
               (f"Conforms to {self.type}" if self.success else f"FAIL - {self.fail_reason}")


class JSGPython:
    def __init__(self, jsg: str | None = None, python: str | None = None, print_python: bool = False) -> None:
        """ Construct a jsg validation module

        :param jsg: JSG specification.  If none, use python
        :param python: Python specification.
        :param print_python: True means print Python to stdout
        """
        if jsg is not None:
            self.schema = self._to_string(jsg) if not self._is_jsg(jsg) else jsg
        else:
            self.schema = None
        self.python = parse(self.schema, self.__class__.__name__) if self.schema else self._to_string(python)
        if print_python:
            print(self.python)
        self.json_obj = None
        if not self.python:
            raise ValueError("JSGPython: jsg parsing error")
        spec = compile(self.python, self.__class__.__name__, 'exec')
        self.module = ModuleType(self.__class__.__name__)
        exec(spec, self.module.__dict__)

    @staticmethod
    def _is_jsg(s: str) -> bool:
        """ Determine whether s looks like a JSG spec """
        return isinstance(s, str) and ('\n' in s or '{' in s)

    @staticmethod
    def is_json(s: str) -> bool:
        """ Determine whether s looks like JSON """
        return s.strip().startswith(('{', '['))

    @staticmethod
    def _to_string(inp: str) -> str:
        """ Convert a URL or file name to a string """
        if '://' in inp:
            req = requests.get(inp)
            if not req.ok:
                raise ValueError(f"Unable to read {inp}")
            return req.text
        else:
            with open(inp) as infile:
                return infile.read()

    def conforms(self, json: str, name: str = "") -> ValidationResult:
        """ Determine whether json conforms with the JSG specification

        :param json: JSON string, URI to JSON or file name with JSON
        :param name: Test name for ValidationResult -- printed in dx if present
        :return: pass/fail + fail reason
        """
        json = self._to_string(json) if not self.is_json(json) else json
        try:
            self.json_obj = loads(json, self.module)
        except ValueError as v:
            return ValidationResult(False, str(v), name, None)
        logfile = StringIO()
        logger = Logger(cast(TextIO, logfile))      # cast because of bug in ide
        if not is_valid(self.json_obj, logger):
            return ValidationResult(False, logfile.getvalue().strip('\n'), name, None)
        return ValidationResult(True, "", name, type(self.json_obj).__name__)


def genargs() -> ArgumentParser:
    """
    Create a command line parser

    :return: parser
    """
    parser = ArgumentParser()
    parser.add_argument("spec", help="JSG specification - can be file name, URI or string")
    parser.add_argument("-o", "--outfile", help="Output python file - if omitted, python is not saved")
    parser.add_argument("-p", "--print", help="Print python file to stdout")
    parser.add_argument("-id", "--inputdir", help="Input directory with JSON files")
    parser.add_argument("-i", "--json", help="URL, file name or json text", nargs='*')
    return parser


def validate_json(argv) -> bool:
    def do_validation(entry: str) -> bool:
        if opts.verbose and not validator.is_json(entry):
            print(f"Validating {entry}... ", end='')
        success, reason = validator.conforms(entry)
        if opts.verbose:
            if success:
                print("Success")
                return True
            else:
                print(f"Fail: {reason}")
                return False

    opts = genargs().parse_args(argv)
    validator = JSGPython(opts.spec, print_python=opts.print)
    all_pass = True

    if opts.outfile:
        with open(opts.outfile, 'w') as outf:
            outf.write(validator.python)

    if opts.indir:
        for filedir, _, files in os.walk(opts.indir):
            for file in files:
                if file.endswith('.json') or file.endswith('.jsonld'):
                    fname = os.path.join(filedir, file)
                    if not do_validation(fname):
                        all_pass = False

    if opts.json:
        for json in opts.json:
            if not do_validation(json):
                all_pass = False
    return all_pass
