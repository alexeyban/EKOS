import re

default_prefix = "_Anon"


class AnonymousIdentifierFactory:
    def __init__(self, prefix: str | None = None):
        """Create a factory

        :param prefix: Prefix - default_prefix if omitted
        """
        self._nextid = 0
        self._prefix = prefix if prefix is not None else default_prefix
        self._anon_re = re.compile(r'{}[1-9][0-9]*$'.format(self._prefix))

    def next_id(self) -> str:
        self._nextid += 1
        return self._prefix + "{:d}".format(self._nextid)

    def is_anon(self, tkn: str):
        return bool(self._anon_re.match(tkn))
