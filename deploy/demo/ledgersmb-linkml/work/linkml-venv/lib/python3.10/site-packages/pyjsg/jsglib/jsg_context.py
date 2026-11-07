from typing import Any


class JSGContext:
    """ Context available to all JSG constructs """
    def __init__(self):
        # the object member name, if any, that identifies the type of object. If present, must match the name of
        # a JSGObject
        self.TYPE: str = ""

        # Objects that lack type identifiers.  Any objects lacking a TYPE variable will be matched against
        # the list below in order
        self.TYPE_EXCEPTIONS: list[str] = []

        # Object pair names that can always exist in an object
        self.IGNORE: list[str] = []

        # True means that we allow JSON_LD constructs (parameters starting with "@")
        self.JSON_LD = True

        # NAMESPACE prevents references from being resolved against other JSG modules
        self.NAMESPACE: dict[str, Any] = None

    def unvalidated_parm(self, parm: str) -> bool:
        """Return true if the pair name should be ignored

        :param parm: string part of pair string:value
        :return: True if it should be accepted
        """
        return parm.startswith("_") or parm == self.TYPE or parm in self.IGNORE or \
            (self.JSON_LD and parm.startswith('@'))