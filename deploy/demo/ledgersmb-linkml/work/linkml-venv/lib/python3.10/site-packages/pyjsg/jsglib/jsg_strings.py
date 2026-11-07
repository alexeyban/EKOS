import re
from typing import Any

from pyjsg.jsglib.jsg_validateable import JSGValidateable
from pyjsg.jsglib.loader import Logger


class JSGPattern:
    """
    A lexerRuleBlock
    """
    def __init__(self, pattern: str):
        """
        Compile and record a match pattern

        :param pattern: regular expression
        """
        self.pattern_re = re.compile(pattern, flags=re.DOTALL)

    def __str__(self):
        return self.pattern_re.pattern

    def matches(self, txt: str) -> bool:
        """Determine whether txt matches pattern

        :param txt: text to check
        :return: True if match
        """
        # rval = ref.getText()[1:-1].encode('utf-8').decode('unicode-escape')
        if r'\\u' in self.pattern_re.pattern:
            txt = txt.encode('utf-8').decode('unicode-escape')
        match = self.pattern_re.match(txt)
        return match is not None and match.end() == len(txt)


class JSGPatternedValMeta(type):
    pattern: JSGPattern | None
    python_type: type | tuple[type]

    def __instancecheck__(self, instance) -> bool:
        return isinstance(instance, self.python_type) and (self.pattern is None or self.pattern.matches(str(instance)))


class JSGPatterned(JSGValidateable, metaclass=JSGPatternedValMeta):
    pattern: JSGPattern | None = None

    def __init__(self, val: Any) -> None:
        if not isinstance(val, type(self)):
            raise ValueError(f'Invalid {self._class_name} value: "{val}"')

    def _is_valid(self, log: Logger | None = None) -> bool:
        return True

    @property
    def val(self) -> Any:
        return self


class JSGString(str, JSGPatterned):
    """
    A lexerRuleSpec implementation
    """
    pattern: JSGPattern | None = None
    python_type = str


class String(JSGString):
    """ Implementation of JSG @string type """
    pass


class Number(float, JSGPatterned):
    """ Implementation of JSG @number type """
    pattern = JSGPattern(r'-?(0|[1-9][0-9]*)(.[0-9]+)?([eE][+-]?[0-9]+)?')
    python_type = (int, float, str)

    @property
    def val(self) -> int | float:
        return int(self) if Integer.pattern.matches(str(self)) else self


class Integer(int, JSGPatterned):
    """ Implementation of JSG @int type """
    pattern = JSGPattern(r'-?(0|[1-9][0-9]*)')
    python_type = (int, str)

    def __new__(cls, v):
        if isinstance(v, bool) or not isinstance(v, Integer):
            raise ValueError(f"Invalid {cls.__name__} value: {v}")
        return super().__new__(cls, v)


class Boolean(JSGValidateable, metaclass=JSGPatternedValMeta):
    true_pattern = JSGPattern(r'[Tt]rue')
    false_pattern = JSGPattern(r'[Ff]alse')
    pattern = JSGPattern(r'{}|{}'.format(true_pattern, false_pattern))
    python_type = (int, str)

    def __new__(cls, v) -> bool:
        if not isinstance(v, cls):
            raise ValueError(f"Invalid {cls.__name__} value: {v}")

        return v if isinstance(v, bool) else cls.true_pattern.matches(str(v))

    def _is_valid(self, log: Logger | None = None) -> bool:
        return True
