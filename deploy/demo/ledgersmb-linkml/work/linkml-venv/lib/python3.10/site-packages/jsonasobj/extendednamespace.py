from argparse import Namespace


class ExtendedNamespace(Namespace):
    """ A combination of a namespace and a dictionary.  This allows direct acess to python properties plus
    dictionary access to everything.
    """
    def __init__(self, **kwargs):
        Namespace.__init__(self, **kwargs)

    def __contains__(self, key):
        return key in self.__dict__

    def __getitem__(self, item):
        return self.__dict__[item]

    def __setitem__(self, key, item):
        self.__dict__[key] = item

    def __delitem__(self, key):
        del self.__dict__[key]

    def __iter__(self):
        return self.__dict__.__iter__()

    def __len__(self):
        return len(self.__dict__)

    def _get(self, key, default=None):
        return self.__dict__.get(key, default)
