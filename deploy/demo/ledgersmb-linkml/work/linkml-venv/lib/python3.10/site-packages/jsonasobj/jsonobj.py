import json
from typing import Union, List, Dict, Tuple, Optional, Callable, Any
from urllib.request import Request, urlopen

from .extendednamespace import ExtendedNamespace

# Possible types in the JsonObj representation
JsonObjTypes = Union["JsonObj", List["JsonObjTypes"], str, bool, int, float, None]

# Types in the pure JSON representation
JsonTypes = Union[Dict[str, "JsonTypes"], List["JsonTypes"], str, bool, int, float, None]


class JsonObj(ExtendedNamespace):
    """ A namespace/dictionary representation of a JSON object. Any name in a JSON object that is a valid python
    identifier is represented as a first-class member of the objects.  JSON identifiers that begin with "_" are
    disallowed in this implementation.
    """
    def __init__(self, _if_missing:Callable[[str], Any] = None, **kwargs):
        """ Construct a JsonObj from set of keyword/value pairs

        :param kwargs: keyword/value pairs
        """
        ExtendedNamespace.__init__(self, **kwargs)

    @staticmethod
    def _if_missing(item):
        pass

    def _get(self, item: str, default: JsonObjTypes=None) -> JsonObjTypes:
        return self[item] if item in self else default

    def _default(self, obj, filtr: Callable[[dict], dict] = lambda e: e):
        """ return a serialized version of obj or raise a TypeError

        :param obj:
        :param filtr: dictionary filter
        :return: Serialized version of obj
        """
        return filtr(obj.__dict__) if isinstance(obj, JsonObj) else json.JSONDecoder().decode(obj)

    def _as_json_obj(self) -> JsonTypes:
        """ Return jsonObj as pure json

        :return: Pure json image
        """
        return json.loads(self._as_json_dumps())

    def _setdefault(self, k: str, value: Union[Dict, JsonTypes]) -> JsonObjTypes:
        if k not in self:
            self[k] = JsonObj(**value) if isinstance(value, dict) else value
        return self[k]

    def _items(self) -> List[Tuple[str, JsonObjTypes]]:
        """ Same as dict items() except that the values are JsonObjs instead of vanilla dictionaries
        :return:
        """
        return [(k, self[k]) for k in self.__dict__.keys()]

    def __getitem__(self, item):
        return self._if_missing(item) if item not in self.__dict__ and self._if_missing != JsonObj._if_missing else super().__getitem__(item)

    def __getattr__(self, item):
        return self._if_missing(item) if item not in self.__dict__ and self._if_missing != JsonObj._if_missing else getattr(super(), item)

    @property
    def _as_json(self) -> str:
        """ Convert a JsonObj into straight json text

        :return: JSON formatted str
        """
        return json.dumps(self, default=self._default)

    def _as_json_dumps(self, indent: str = '   ', filtr: Callable[[dict], dict] = None, **kwargs) -> str:
        """ Convert to a stringified json object.

        This is the same as _as_json with the exception that it isn't
        a property, meaning that we can actually pass arguments...
        :param indent: indent argument to dumps
        :param filtr: dictionary filter
        :param kwargs: other arguments for dumps
        :return: JSON formatted string
        """
        return json.dumps(self, default=lambda obj: self._default(obj, filtr) if filtr else self._default(obj),
                          indent=indent, **kwargs)

    @staticmethod
    def __as_list(value: List[JsonObjTypes]) -> List[JsonTypes]:
        """ Return a json array as a list

        :param value: array
        :return: array with JsonObj instances removed
        """
        return [e._as_dict if isinstance(e, JsonObj) else e for e in value]

    @property
    def _as_dict(self) -> Dict[str, JsonTypes]:
        """ Convert a JsonObj into a straight dictionary

        :return: dictionary that cooresponds to the json object
        """
        return {k: v._as_dict if isinstance(v, JsonObj) else
                   self.__as_list(v) if isinstance(v, list) else
                   v for k, v in self.__dict__.items()}


def loads(s: str, **kwargs) -> JsonObj:
    """ Convert a json_str into a JsonObj

    :param s: a str instance containing a JSON document
    :param kwargs: arguments see: json.load for details
    :return: JsonObj representing the json string
    """
    if isinstance(s, (bytes, bytearray)):
        s = s.decode(json.detect_encoding(s), 'surrogatepass')
    return json.loads(s, object_hook=lambda pairs: JsonObj(**pairs), **kwargs)


def load(source, **kwargs) -> JsonObj:
    """ Deserialize a JSON source.

    :param source: a URI, File name or a .read()-supporting file-like object containing a JSON document
    :param kwargs: arguments. see: json.load for details
    :return: JsonObj representing fp
    """
    if isinstance(source, str):
        if '://' in source:
            req = Request(source)
            req.add_header("Accept", "application/json, text/json;q=0.9")
            with urlopen(req) as response:
                jsons = response.read()
        else:
            with open(source) as f:
                jsons = f.read()
    elif hasattr(source, "read"):
        jsons = source.read()
    else:
        raise TypeError("Unexpected type {} for source {}".format(type(source), source))

    return loads(jsons, **kwargs)


def as_dict(obj: JsonObj) -> Dict[str, JsonTypes]:
    """ Convert a JsonObj into a straight dictionary

    :param obj: pseudo 'self'
    :return: dictionary that cooresponds to the json object
    """
    return obj._as_dict


def as_list(obj: JsonObj) -> List[JsonTypes]:
    """ Return a json array as a list

    :param obj: pseudo 'self'
    """
    return obj._as_list


def as_json(obj: JsonObj, indent: Optional[str] = '   ', filtr: Callable[[dict], dict] = None, **kwargs) -> str:
    """ Convert obj to json string representation.

        :param obj: pseudo 'self'
        :param indent: indent argument to dumps
        :param filtr: filter to remove unwanted elements
        :param kwargs: other arguments for dumps
        :return: JSON formatted string
       """
    return obj._as_json_dumps(indent, filtr=filtr, **kwargs)


def as_json_obj(obj: JsonObj) -> JsonTypes:
    """ Return obj as pure python json (vs. JsonObj)
        :param obj: pseudo 'self'
        :return: Pure python json image
    """
    return obj._as_json_obj()


def get(obj: JsonObj, item: str, default: JsonObjTypes=None) -> JsonObjTypes:
    """ Dictionary get routine """
    return obj._get(item, default)


def setdefault(obj: JsonObj, k: str, value: Union[Dict, JsonTypes]) -> JsonObjTypes:
    """ Dictionary setdefault reoutine """
    return obj._setdefault(k, value)


def items(obj: JsonObj) -> List[Tuple[str, JsonObjTypes]]:
    """ Same as dict items() except that the values are JsonObjs instead of vanilla dictionaries
    :return:
    """
    return obj._items()
