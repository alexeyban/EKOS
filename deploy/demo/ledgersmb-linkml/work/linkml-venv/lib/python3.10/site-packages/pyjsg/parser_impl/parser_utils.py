import ast
import keyword
from collections import OrderedDict
from collections.abc import Iterable

from pyjsg.parser.jsgParser import ParserRuleContext


def t(n: int=1) -> str:
    return '    ' * n


def flatten(l: Iterable) -> list:
    """Return a list of all non-list items in l

    :param l: list to be flattened
    :return:
    """
    rval = []
    for e in l:
        if not isinstance(e, str) and isinstance(e, Iterable):
            if len(list(e)):
                rval += flatten(e)
        else:
            rval.append(e)
    return rval


def flatten_unique(l: Iterable) -> list:
    """ Return a list of UNIQUE non-list items in l """
    rval = OrderedDict()
    for e in l:
        if not isinstance(e, str) and isinstance(e, Iterable):
            for ev in flatten_unique(e):
                rval[ev] = None
        else:
            rval[e] = None
    return list(rval.keys())


def as_set(l: Iterable) -> set:
    """ Return the set of all terminals in list l

    :param l:
    """
    return set(flatten(l))


identifier_types = ['ID', 'LEXER_ID', 'LEXER_ID_REF', 'idref', 'STRING']


def get_terminal(ctx: ParserRuleContext) -> str:
    """ Extract the token for an identifier from the context. Tokens can be:
        * ID - The name of a JSG definition
        * idref -- A reference to an existing JSG definition
        * LEXER_ID - The name of a match pattern definition
        * LEXER_ID_REF - A reference to an existing match pattern definition
        * STRING -- A JSG item name enclosed in quotes

        :param ctx: JSG parser item with some sort of identifier
        :return:
        """
    tkn = None
    for ele_name in identifier_types:
        ele = getattr(ctx, ele_name, None)
        if ele and ele():
            tkn = ele().getText()[1:-1] if ele_name == 'STRING' else ele().getText()
    return str(tkn)

def as_token(ctx: ParserRuleContext) -> str:
    """ Extract the token for an identifier from the context. Tokens can be:
    * ID - The name of a JSG definition
    * idref -- A reference to an existing JSG definition
    * LEXER_ID - The name of a match pattern definition
    * LEXER_ID_REF - A reference to an existing match pattern definition
    * STRING -- A JSG item name enclosed in quotes

    :param ctx: JSG parser item with some sort of identifier
    :return:
    """
    return esc_kw(get_terminal(ctx))


def as_tokens(ctx: list[ParserRuleContext]) -> list[str]:
    """Return a stringified list of identifiers in ctx

    :param ctx: JSG parser item with a set of identifiers
    :return:
    """
    return [as_token(e) for e in ctx]


def is_valid_python(tkn: str) -> bool:
    """Determine whether tkn is a valid python identifier

    :param tkn:
    :return:
    """
    try:
        root = ast.parse(tkn)
    except SyntaxError:
        return False
    return len(root.body) == 1 and isinstance(root.body[0], ast.Expr) and isinstance(root.body[0].value, ast.Name)


def esc_kw(token: str) -> str:
    """ Escape python keywords

    :param token: token
    :return: token with '_' suffixed if it is a keyword
    """
    return token + '_' if keyword.iskeyword(token) else token
