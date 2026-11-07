from abc import abstractmethod
from typing import TextIO

from pyjsg.jsglib.logger import Logger


class JSGValidateable:
    """
    Mixin -- any class with an _is_valid function
    """
    @abstractmethod
    def _is_valid(self, log: TextIO | Logger | None = None) -> bool:
        """ Determine whether the element is valid

        :param log: Logger or IO device to record errors
        :return: True if valid, false otherwise
        """
        raise NotImplementedError("_is_valid must be implemented")

    @property
    def _class_name(self) -> str:
        return type(self).__name__